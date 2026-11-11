//! Layout shift, as the Layout Instability API reports it, on each page's cold load (a fresh
//! profile) and warm load (the same profile again), at a phone's and a wide screen's width. The
//! fixtures answer as the server does: a document's outline carries no section bodies, which
//! arrive in a later read, and the panels' conversation and activity are read when they open.
//!
//! A page's score is its cumulative layout shift (CLS): the largest session window of its shifts
//! (a gap over 1 s or a window over 5 s starts the next one). Every shift counts, the ones Chrome
//! marks as following input too: its phone emulation marks a load's own shifts so, and a measured
//! panel is scored from after its opening, so no shift the click itself caused is in its score.
use super::{SHOWN, chrome, chrome::Chrome, common, dist, look_parity, site::Site};
use serde_json::{Value, json};

/// Each page reserves its regions before its data arrives: nothing it shows moves when content
/// arrives (app/skeleton-loading). The owner's goal, 2026-11-11.
const CLS_LIMIT: f64 = 0.05;

/// Record every layout shift from the document's first script on, with the nodes it moved.
const RECORD: &str = r#"
window.__shifts = [];
const shiftName = node => {
    if (!node) return '(gone)';
    if (node.nodeType !== 1) node = node.parentElement;
    const parts = [];
    for (let at = node; at && at !== document.body && parts.length < 4; at = at.parentElement) {
        let part = at.tagName.toLowerCase();
        if (at.id) part += '#' + at.id;
        if (typeof at.className === 'string' && at.className.trim()) part += '.' + at.className.trim().split(/\s+/).slice(0, 3).join('.');
        parts.push(part);
    }
    return parts.join(' < ');
};
const shiftBox = r => r ? [Math.round(r.x), Math.round(r.y), Math.round(r.width), Math.round(r.height)] : null;
new PerformanceObserver(list => {
    for (const entry of list.getEntries()) window.__shifts.push({
        value: entry.value, recent: entry.hadRecentInput, at: entry.startTime,
        sources: (entry.sources || []).map(s => `${shiftName(s.node)} ${JSON.stringify(shiftBox(s.previousRect))}→${JSON.stringify(shiftBox(s.currentRect))}`)
    });
}).observe({type: 'layout-shift', buffered: true});
"#;

/// Resolve once nothing has shifted and no resource has finished for 1.5 s (at most 15 s): the
/// page has drawn what its reads brought.
const SETTLED: &str = r#"new Promise(resolve => {
    const started = performance.now();
    let shifts = -1, resources = -1, since = started;
    const tick = () => {
        const s = window.__shifts.length, r = performance.getEntriesByType('resource').length;
        if (s !== shifts || r !== resources) { shifts = s; resources = r; since = performance.now(); }
        if (performance.now() - since >= 1500 || performance.now() - started > 15000) resolve(s);
        else setTimeout(tick, 100);
    };
    tick();
})"#;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Shift {
    pub value: f64,
    /// Chrome marks it as within 500 ms of input; reported, and counted all the same.
    pub recent: bool,
    /// ms from the page's start.
    pub at: f64,
    pub sources: Vec<String>,
}

/// The cumulative layout shift of `shifts`: their largest session window. A window ends at a gap
/// over 1 s, or once it spans 5 s.
pub(super) fn cls(shifts: &[Shift]) -> f64 {
    let (mut best, mut window, mut first, mut last) =
        (0.0_f64, 0.0_f64, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for shift in shifts {
        if shift.at - last > 1000.0 || shift.at - first > 5000.0 {
            window = 0.0;
            first = shift.at;
        }
        window += shift.value;
        last = shift.at;
        best = best.max(window);
    }
    best
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Width {
    pub name: &'static str,
    width: u32,
    height: u32,
    mobile: bool,
}

pub(super) const PHONE: Width = Width {
    name: "phone",
    width: 390,
    height: 844,
    mobile: true,
};
pub(super) const WIDE: Width = Width {
    name: "wide",
    width: 1280,
    height: 900,
    mobile: false,
};

/// What a measured load opens once its page is settled: a panel, by the control that opens it,
/// and what it shows once its read is in.
#[derive(Clone, Copy, Debug)]
pub(super) struct Open {
    pub control: &'static str,
    /// The read the panel makes when it opens, held until it is open: the score is of what its
    /// answer moves, not of the opening the reader asked for. None scores from the click on.
    pub read: Option<&'static str>,
    /// The panel is open, its read still held.
    pub opened: &'static str,
    pub shown: &'static str,
    /// Run before each load's first script: what the device holds for the panel.
    pub held: &'static str,
}

/// The chat, its conversation held on this device from an earlier visit.
pub(super) const CHAT: Open = Open {
    control: "#quick-open",
    read: Some("/api/ask"),
    opened: "document.querySelector('#quick-sheet:not([hidden]) form') !== null",
    shown: "document.querySelector('#quick-sheet:not([hidden]) .quick-message') !== null",
    held: r#"try { localStorage.setItem('review-quick-draft', JSON.stringify({ask: 7, ask_scope: '', priority: '2'})); } catch (_) {}"#,
};
/// The activity panel: a fixed overlay over the page, its items read with the page.
pub(super) const ACTIVITY: Open = Open {
    control: "#bell",
    read: None,
    opened: "true",
    shown: "document.querySelector('#activity-panel li') !== null",
    held: "",
};

/// One load's shifts: the cold one, then the warm one.
#[derive(Debug)]
pub(super) struct Loads {
    pub cold: Vec<Shift>,
    pub warm: Vec<Shift>,
}

/// Load `path` cold and then warm at `width`, waiting for `ready`, then (with `open`) open a panel
/// by a real click: the shifts of the page's load, or of the panel's opening alone.
pub(super) fn measure(
    site: &Site,
    path: &str,
    ready: &str,
    width: Width,
    open: Option<Open>,
) -> Option<Loads> {
    let mut browser = chrome()?;
    browser.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width": width.width, "height": width.height, "deviceScaleFactor": 1, "mobile": width.mobile}),
    );
    let held = open.map_or("", |open| open.held);
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": format!("{held}\n{RECORD}") }),
    );
    let cold = load(&mut browser, site, path, ready, open);
    browser.open("about:blank");
    let warm = load(&mut browser, site, path, ready, open);
    Some(Loads { cold, warm })
}

fn load(
    browser: &mut Chrome,
    site: &Site,
    path: &str,
    ready: &str,
    open: Option<Open>,
) -> Vec<Shift> {
    browser.open(&site.url(path));
    assert!(
        browser.wait_for(
            &format!("document.documentElement.dataset.navigation === 'ready' && ({ready})"),
            SHOWN
        ),
        "{path} never showed {ready}: {:?}",
        browser.eval("document.body.innerText.slice(0, 2000)")
    );
    let mut settled = browser.eval(SETTLED).as_u64().unwrap_or(0) as usize;
    let Some(open) = open else {
        return shifts(browser, 0);
    };
    if let Some(read) = open.read {
        site.hold_route(read, true);
    }
    let point = browser.eval(&format!(
        "(() => {{ const r = document.querySelector({}).getBoundingClientRect(); return {{x: r.x + r.width / 2, y: r.y + r.height / 2}}; }})()",
        json!(open.control)
    ));
    for kind in ["mousePressed", "mouseReleased"] {
        browser.call(
            "Input.dispatchMouseEvent",
            json!({"type": kind, "x": point["x"], "y": point["y"], "button": "left", "clickCount": 1}),
        );
    }
    if let Some(read) = open.read {
        let opened = browser.wait_for(open.opened, SHOWN);
        if opened {
            settled = browser.eval(SETTLED).as_u64().unwrap_or(0) as usize;
        }
        site.hold_route(read, false);
        assert!(
            opened,
            "{path}: {} never opened its panel: {:?}",
            open.control,
            browser.eval("document.body.innerText.slice(0, 2000)")
        );
    }
    assert!(
        browser.wait_for(open.shown, SHOWN),
        "{path}: {} never showed its panel: {:?}",
        open.control,
        browser.eval("document.body.innerText.slice(0, 2000)")
    );
    browser.eval(SETTLED);
    shifts(browser, settled)
}

/// The shifts recorded since the `from`th.
fn shifts(browser: &mut Chrome, from: usize) -> Vec<Shift> {
    let recorded = browser.eval("window.__shifts");
    recorded
        .as_array()
        .map(|all| all.iter().skip(from).map(shift).collect())
        .unwrap_or_default()
}

fn shift(value: &Value) -> Shift {
    Shift {
        value: value["value"].as_f64().unwrap_or_default(),
        recent: value["recent"].as_bool().unwrap_or_default(),
        at: value["at"].as_f64().unwrap_or_default(),
        sources: value["sources"]
            .as_array()
            .map(|sources| {
                sources
                    .iter()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// One line per load, its worst shifts after it, for the task thread and the run's log.
pub(super) fn report(page: &str, width: Width, loads: &Loads) -> String {
    let mut lines = Vec::new();
    for (kind, shifts) in [("cold", &loads.cold), ("warm", &loads.warm)] {
        let total: f64 = shifts.iter().map(|s| s.value).sum();
        lines.push(format!(
            "LAYOUT SHIFT {page} {} {kind}: cls {:.4} (all shifts {total:.4}, {} entries)",
            width.name,
            cls(shifts),
            shifts.len()
        ));
        let mut worst: Vec<_> = shifts.iter().collect();
        worst.sort_by(|a, b| b.value.total_cmp(&a.value));
        for shift in worst.iter().take(4) {
            lines.push(format!(
                "    {:.4}{} at {:.0} ms: {}",
                shift.value,
                if shift.recent { " (input)" } else { "" },
                shift.at,
                shift.sources.join(" | ")
            ));
        }
    }
    lines.join("\n")
}

fn paragraph(text: &str) -> Value {
    json!({"type":"paragraph","spans":[{"type":"text","text":text}]})
}

/// A site answering every page's reads with look-parity content, the panels' activity and a
/// conversation, and a document at `path` whose `sections` arrive as the server sends them: an
/// outline without their bodies, then the bodies a later read asks for.
pub(super) fn site(path: &str, sections: &[Value], cards: Value) -> Site {
    let site = Site::start(dist(), &common::fixture("board.json"));
    for (route, value) in look_parity::responses() {
        if route != "/api/board" {
            site.set_json(&route, &value);
        }
    }
    let outline: Vec<Value> = sections
        .iter()
        .map(|section| {
            let mut section = section.clone();
            section["blocks"] = json!([]);
            section
        })
        .collect();
    let headings: Vec<Value> = sections
        .iter()
        .filter_map(|section| {
            section["blocks"].as_array()?.iter().find_map(|block| {
                (block["type"] == "heading").then(|| json!({"level": block["level"], "anchor": block["anchor"], "text": block["spans"][0]["text"]}))
            })
        })
        .collect();
    site.set_json(
        "/api/document",
        &json!({"document":{"version":4182,"path":path,"title":"Probe","waiting":[],"crumbs":[],
            "headings":headings,"sections":outline,"cards":cards,"threads":{},"tasks":[]},"anchors":{}}),
    );
    site.set_json(
        "/api/section",
        &json!({"version":4182,"change":{"path":path,"sections":sections}}),
    );
    site.set_json("/api/asks", &json!({"asks":[],"listening":false}));
    let items: Vec<Value> = (0..6)
        .map(|n| json!({"id":format!("a{n}"),"kind":"answer","path":"docs/DECISIONS.md","slug":format!("card-{n}"),
            "ts":"2026-11-11 09:00","text":"An agent answered the question on this card with a few words.",
            "href":format!("/docs/DECISIONS.md#card-{n}"),"answered":n % 2 == 0}))
        .collect();
    site.set_json("/api/activity", &json!({ "items": items }));
    let messages: Vec<Value> = (1..=4)
        .map(|seq| json!({"seq":seq,"kind":"message","by":if seq % 2 == 1 { "owner" } else { "agent" },
            "text":"","at":1.0,"blocks":[paragraph("A message in the conversation, long enough to wrap onto a second line on a phone.")]}))
        .collect();
    site.set_json(
        "/api/ask",
        &json!({"ask":{"id":7,"path":"docs/README.md","anchor":"","quote":"","question":"Hello","asked":1.0,
            "updated":1.0,"answers":[],"messages":messages,"status":"answered"},"recipient":"agent"}),
    );
    site
}

/// A plain document: prose of several lengths, a list, code and a table.
pub(super) fn document_sections() -> Vec<Value> {
    let mut sections = vec![
        json!({"start":1,"end":2,"key":"title","hash":"title","part":0,"blocks":[
        {"type":"heading","level":1,"anchor":"probe","spans":[{"type":"text","text":"Probe"}]}]}),
    ];
    for n in 0..8 {
        let start = 10 + n * 10;
        let mut blocks = vec![
            json!({"type":"heading","level":2,"anchor":format!("part-{n}"),"spans":[{"type":"text","text":format!("Part {n}")}]}),
            paragraph(&"A sentence of the document that runs on for a while. ".repeat(1 + n % 4)),
        ];
        if n % 3 == 1 {
            blocks.push(json!({"type":"code","language":"text","text":"line\n".repeat(6)}));
        }
        if n % 3 == 2 {
            blocks.push(paragraph(&"Another paragraph after it. ".repeat(6)));
        }
        sections.push(json!({"start":start,"end":start + 8,"key":format!("part-{n}"),"hash":format!("h{n}"),"part":0,"blocks":blocks}));
    }
    sections
}

/// The register: decision cards, open and settled.
pub(super) fn register_sections() -> (Vec<Value>, Value) {
    let mut sections = vec![
        json!({"start":1,"end":2,"key":"title","hash":"title","part":0,"blocks":[
        {"type":"heading","level":1,"anchor":"decisions","spans":[{"type":"text","text":"Decisions"}]},
        paragraph("Every decision the owner makes, newest first.")]}),
    ];
    let mut cards = Vec::new();
    for n in 0..8 {
        let slug = format!("card-{n}");
        let (state, group) = if n < 5 {
            ("pending", "decide")
        } else {
            ("approved", "resolved")
        };
        let start = 10 + n * 20;
        let body = vec![paragraph(
            &"The card's question, with enough words to wrap. ".repeat(2 + n % 3),
        )];
        let block = if group == "resolved" {
            json!({"type":"settled-card","open":false,"summary":[{"type":"text","text":format!("Card {n} · approved")}],"blocks":[
                {"type":"card","slug":slug,"mark":state,"title":format!("Card {n}"),"body":body,"verdict":{"group":group,"open":false,"posted":null,"decided":null},"thread":[],"line":start,"line_offset":null,"comparison":[]}]})
        } else {
            json!({"type":"card","slug":slug,"mark":state,"title":format!("Card {n}"),"body":body,"verdict":{"group":group,"open":true,"posted":null,"decided":null},"thread":[],"line":start,"line_offset":null,"comparison":[]})
        };
        sections.push(json!({"start":start,"end":start + 12,"key":format!("card:{slug}"),"hash":format!("c{n}"),"part":1,"blocks":[block]}));
        cards.push(json!({"slug":slug,"state":state,"text":format!("Card {n}"),"group":group,"open":group == "decide"}));
    }
    (sections, json!(cards))
}

/// Measure every page and width, report each, and say which loads miss the limit.
pub(super) fn misses(
    page: &str,
    site: &Site,
    path: &str,
    ready: &str,
    open: Option<Open>,
) -> Vec<String> {
    let mut misses = Vec::new();
    for width in [PHONE, WIDE] {
        let Some(loads) = measure(site, path, ready, width, open) else {
            return misses;
        };
        eprintln!("{}", report(page, width, &loads));
        for (kind, shifts) in [("cold", &loads.cold), ("warm", &loads.warm)] {
            let score = cls(shifts);
            if score >= CLS_LIMIT {
                misses.push(format!("{page} {} {kind}: cls {score:.4}", width.name));
            }
        }
    }
    misses
}

#[test]
fn cls_takes_the_largest_session_window_of_every_shift() {
    let at = |at: f64, value: f64| Shift {
        value,
        recent: false,
        at,
        sources: Vec::new(),
    };
    assert_eq!(cls(&[]), 0.0);
    // One window: each shift within 1 s of the last.
    assert!((cls(&[at(0.0, 0.1), at(900.0, 0.1), at(1800.0, 0.1)]) - 0.3).abs() < 1e-9);
    // A gap over 1 s starts another window; the larger one counts.
    assert!((cls(&[at(0.0, 0.1), at(1200.0, 0.05), at(1300.0, 0.02)]) - 0.1).abs() < 1e-9);
    assert!((cls(&[at(0.0, 0.01), at(1200.0, 0.05), at(1300.0, 0.02)]) - 0.07).abs() < 1e-9);
    // A window spans at most 5 s, even with no gap over 1 s.
    let steady: Vec<_> = (0..12).map(|n| at(f64::from(n) * 900.0, 0.01)).collect();
    assert!((cls(&steady) - 0.06).abs() < 1e-9, "{}", cls(&steady));
    // A shift Chrome marks as following input counts all the same, and joins its window.
    let mut input = at(500.0, 0.5);
    input.recent = true;
    assert!((cls(&[at(0.0, 0.01), input, at(1400.0, 0.02)]) - 0.53).abs() < 1e-9);
}

#[test]
fn measure_layout_shift_of_each_page() {
    let mut misses = Vec::new();
    let (register, cards) = register_sections();
    let document = document_sections();
    for (page, path, ready, open, sections, cards) in [
        (
            "home",
            "/",
            "document.querySelector('[data-ready=board] .hero-title') !== null",
            None,
            &document,
            json!([]),
        ),
        (
            "tasks",
            "/tasks",
            "document.querySelector('.taskcard') !== null",
            None,
            &document,
            json!([]),
        ),
        (
            "task",
            "/task/visual/in-progress",
            "document.querySelector('article.task .history') !== null",
            None,
            &document,
            json!([]),
        ),
        (
            "document",
            "/docs/PROBE.md",
            "document.querySelector('[data-ready=document]') !== null",
            None,
            &document,
            json!([]),
        ),
        (
            "register",
            "/docs/DECISIONS.md",
            "document.querySelector('[data-ready=document]') !== null",
            None,
            &register,
            cards.clone(),
        ),
        (
            "chat",
            "/",
            "document.querySelector('[data-ready=board] .hero-title') !== null",
            Some(CHAT),
            &document,
            json!([]),
        ),
        (
            "activity",
            "/",
            "document.querySelector('[data-ready=board] .hero-title') !== null",
            Some(ACTIVITY),
            &document,
            json!([]),
        ),
    ] {
        let doc = path.trim_start_matches('/');
        let doc = if doc.starts_with("docs/") {
            doc
        } else {
            "docs/PROBE.md"
        };
        let site = site(doc, sections, cards);
        misses.extend(self::misses(page, &site, path, ready, open));
    }
    eprintln!("LAYOUT SHIFT over {CLS_LIMIT}: {misses:#?}");
}
