//! Layout shift, as the Layout Instability API reports it, on each page's cold load (a fresh
//! profile) and warm load (the same profile again), at a phone's and a wide screen's width. The
//! fixtures answer as the server does: a document's outline carries no section bodies, which
//! arrive in a later read, and the panels' conversation and activity are read when they open.
//!
//! A page's score is its cumulative layout shift (CLS): the largest session window of its shifts
//! (a gap over 1 s or a window over 5 s starts the next one). Every shift counts, the ones Chrome
//! marks as following input too: its phone emulation marks a load's own shifts so, and a measured
//! panel is scored from after its opening, so no shift the click itself caused is in its score.
use super::{
    SHOWN, chrome, chrome::Chrome, common, dist, document_landing::TWO_FRAMES, look_parity,
    site::Site,
};
use serde_json::{Value, json};

/// Each page reserves its regions before its data arrives, so what it already shows stays put
/// when content arrives (app/skeleton-loading; the owner's goal, 2026-11-11). The score sees only
/// elements already drawn that move: where a ghost's box stands against the new element that takes
/// its place, it cannot see (`landmarks`).
const CLS_LIMIT: f64 = 0.05;

/// Record every layout shift from the document's first script on, with the nodes it moved.
pub(super) const RECORD: &str = r#"
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
pub(super) const SETTLED: &str = r#"new Promise(resolve => {
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
    pub width: u32,
    pub height: u32,
    pub mobile: bool,
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

/// The colour scheme a visit is measured under. A load that names none takes the machine's, which
/// headless Chrome follows; a score that depends on it (a card's shadow is part of the box a shift
/// moves, and the light theme's is the larger) names each and is held to that scheme's limit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Scheme {
    pub name: &'static str,
}

pub(super) const DARK: Scheme = Scheme { name: "dark" };
pub(super) const LIGHT: Scheme = Scheme { name: "light" };

/// What a measured load opens once its page is settled: a panel, by the control that opens it,
/// and what it shows once its read is in.
#[derive(Clone, Copy, Debug)]
pub(super) struct Open {
    pub control: &'static str,
    /// The read the panel makes when it opens, held until it is open: the score is of what its
    /// answer moves, not of the opening the reader asked for.
    pub read: Option<&'static str>,
    /// The panel is open, its read still held: its score starts once the page is still after it.
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
/// The activity panel: over the page on a phone, beside it on a wide window, its items read with
/// the page. The shell draws a loading sheet of the same id at once; it is open once its own,
/// with its close button, replaces it.
pub(super) const ACTIVITY: Open = Open {
    control: "#bell",
    read: None,
    opened: "document.querySelector('#activity-panel [aria-label=\"Close activity\"]') !== null",
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
/// by a real click: the shifts of the page's load, or of the panel's opening alone. `before` runs
/// before each load's first script.
pub(super) fn measure(
    site: &Site,
    path: &str,
    ready: &str,
    width: Width,
    open: Option<Open>,
    before: &str,
) -> Option<Loads> {
    let mut browser = chrome()?;
    Some(measure_in(
        &mut browser,
        site,
        path,
        ready,
        width,
        None,
        open,
        before,
    ))
}

/// [`measure`] in `browser`, its page as a launch leaves it: a fresh browser's, or a fresh
/// context's in a browser that has made other visits. Under `scheme`, or the machine's with none.
#[allow(clippy::too_many_arguments)]
fn measure_in(
    browser: &mut Chrome,
    site: &Site,
    path: &str,
    ready: &str,
    width: Width,
    scheme: Option<Scheme>,
    open: Option<Open>,
    before: &str,
) -> Loads {
    if let Some(scheme) = scheme {
        browser.scheme(scheme.name);
    }
    browser.viewport(width.width, width.height, width.mobile);
    let held = open.map_or("", |open| open.held);
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": format!("{held}\n{before}\n{RECORD}") }),
    );
    let cold = load(browser, site, path, ready, open);
    browser.open("about:blank");
    let warm = load(browser, site, path, ready, open);
    Loads { cold, warm }
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
    // The opening the reader asked for is not scored, nor what it moves (a side panel pushes
    // the page aside on a wide window): the score starts once the panel is open and still.
    let opened = browser.wait_for(open.opened, SHOWN);
    if opened {
        settled = browser.eval(SETTLED).as_u64().unwrap_or(0) as usize;
    }
    if let Some(read) = open.read {
        site.hold_route(read, false);
    }
    assert!(
        opened,
        "{path}: {} never opened its panel: {:?}",
        open.control,
        browser.eval("document.body.innerText.slice(0, 2000)")
    );
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
    let mut responses = look_parity::responses();
    // The task's gates as the server answers them, short enough for a line each on a phone, and
    // with its page what the server answered last (`GatesSeen`), the room they hold until then.
    let task = "visual/in-progress";
    responses.insert(
        "/api/task-gates".into(),
        json!({"slug":task,"branch":task,"gates":[
            {"name":"review","state":"missing","detail":"Not reviewed yet"},
            {"name":"nightly","state":"met","by":"nightly","at":1_794_363_240.0,"detail":"Green on main a71f0d2"}]}),
    );
    if let Some(page) = responses.get_mut("/api/task-page") {
        page["gates"] = json!({"count":2,"waiting":1,"unsigned":1});
    }
    for (route, value) in responses {
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
    site.set_json(
        "/api/task-offer",
        &json!({"action":"","label":"","branch":"","payload":null}),
    );
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

/// Measure the page at each width, report each load, and say which loads miss the limit.
/// `before` runs before each load's first script.
pub(super) fn misses(
    page: &str,
    site: &Site,
    path: &str,
    ready: &str,
    open: Option<Open>,
    before: &str,
) -> Vec<String> {
    let mut misses = Vec::new();
    for width in [PHONE, WIDE] {
        let Some(loads) = measure(site, path, ready, width, open, before) else {
            return misses;
        };
        misses.extend(over_the_limit(page, width, &loads));
    }
    misses
}

/// Report `loads` of `page` at `width`, and say which of them miss the limit.
fn over_the_limit(page: &str, width: Width, loads: &Loads) -> Vec<String> {
    eprintln!("{}", report(page, width, loads));
    [("cold", &loads.cold), ("warm", &loads.warm)]
        .into_iter()
        .map(|(kind, shifts)| (kind, cls(shifts)))
        .filter(|(_, score)| *score >= CLS_LIMIT)
        .map(|(kind, score)| format!("{page} {} {kind}: cls {score:.4}", width.name))
        .collect()
}

/// Each phone order has a limit of its own under each scheme, light's over dark's by the shadow's
/// area and no more; a wide window is held to every page's limit whatever the scheme and order.
#[test]
fn a_task_of_another_shape_has_a_limit_for_each_scheme_and_order() {
    let limits = [
        (DARK, PAGE_FIRST, 0.14),
        (DARK, GATES_FIRST, 0.064),
        (LIGHT, PAGE_FIRST, 0.156),
        (LIGHT, GATES_FIRST, 0.069),
    ];
    for (scheme, order, limit) in limits {
        assert_eq!(another_shape_limit(PHONE, scheme, order), limit);
        assert_eq!(another_shape_limit(WIDE, scheme, order), CLS_LIMIT);
    }
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

const HOME: &str = "document.querySelector('[data-ready=board] .hero-title') !== null";
const DOCUMENT: &str = "document.querySelector('[data-ready=document]') !== null";

/// Hold each load of `path`, cold and warm at both widths (with `open` opened after it), under
/// the limit, its document serving `sections` and `cards`.
fn holds_still(
    page: &str,
    path: &str,
    ready: &str,
    open: Option<Open>,
    (sections, cards): (Vec<Value>, Value),
) {
    let docs = path
        .strip_prefix('/')
        .filter(|path| path.starts_with("docs/"))
        .unwrap_or("docs/PROBE.md");
    let site = site(docs, &sections, cards);
    let misses = misses(page, &site, path, ready, open, "");
    assert!(
        misses.is_empty(),
        "layout shift at or over {CLS_LIMIT}: {misses:#?}"
    );
}

fn plain() -> (Vec<Value>, Value) {
    (document_sections(), json!([]))
}

#[test]
fn the_meter_scores_a_page_that_moves_after_it_is_drawn() {
    // Everything below the board's title moves 240 px down once the title is drawn.
    const MOVE: &str = r#"const mover = setInterval(() => {
        const title = document.querySelector('[data-ready=board] .hero-title');
        if (!title) return;
        clearInterval(mover);
        setTimeout(() => { const pad = document.createElement('div'); pad.style.height = '240px'; title.after(pad); }, 200);
    }, 50);"#;
    let site = site("docs/PROBE.md", &document_sections(), json!([]));
    let misses = misses("moved home", &site, "/", HOME, None, MOVE);
    assert_eq!(
        misses.len(),
        4,
        "every load of a page that moves misses the limit: {misses:#?}"
    );
}

#[test]
fn the_board_holds_still_while_it_loads() {
    holds_still("home", "/", HOME, None, plain());
}

#[test]
fn the_task_board_holds_still_while_its_columns_load() {
    holds_still(
        "tasks",
        "/tasks",
        "document.querySelector('.taskcard') !== null",
        None,
        plain(),
    );
}

const TASK: &str = "/task/visual/in-progress";
const TASK_READY: &str =
    "document.querySelector('article.task .settle[data-settled=true] .history') !== null";

/// Which of a task's two answers that size what is under its head comes first on a first visit:
/// the page's (its head, its brief, its count of gates) or the gates' own. The other's route is
/// held until a frame has laid out the first, so the order is the test's and never the machine's.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Order {
    name: &'static str,
    held: &'static str,
    drawn: &'static str,
}

const PAGE_FIRST: Order = Order {
    name: "the page before the gates",
    held: "/api/task-gates",
    drawn: "document.querySelector('article.task .task-head h1') !== null",
};
const GATES_FIRST: Order = Order {
    name: "the gates before the page",
    held: "/api/task-page",
    drawn: "document.querySelector('article.task .task-gates[data-gates=read]') !== null",
};

/// The score of a first visit to the task (a fresh context of `browser`) at `width` under
/// `scheme`, its answers in `order`.
fn first_visit(
    browser: &mut Chrome,
    page: &str,
    site: &Site,
    width: Width,
    scheme: Scheme,
    order: Order,
) -> f64 {
    browser.fresh_context();
    browser.scheme(scheme.name);
    browser.viewport(width.width, width.height, width.mobile);
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": RECORD }),
    );
    site.hold_route(order.held, true);
    browser.open(&site.url(TASK));
    let first = browser.wait_for(order.drawn, SHOWN);
    // A shift is scored when a frame lays the page out, not when an answer reaches the DOM. Let go
    // at once, the other answer can reach the DOM before that frame on a loaded renderer, and the
    // two then move the page in one frame, which is neither order's score.
    if first {
        browser.eval(TWO_FRAMES);
    }
    site.hold_route(order.held, false);
    assert!(
        first,
        "{page}, {}: the first answer was never drawn: {:?}",
        order.name,
        browser.eval("document.body.innerText.slice(0, 2000)")
    );
    assert!(
        browser.wait_for(
            &format!("document.documentElement.dataset.navigation === 'ready' && ({TASK_READY})"),
            SHOWN
        ),
        "{page}, {}: the task never showed its thread",
        order.name
    );
    browser.eval(SETTLED);
    let loads = Loads {
        cold: shifts(browser, 0),
        warm: Vec::new(),
    };
    eprintln!(
        "{}",
        report(
            &format!("{page}, {}, {},", scheme.name, order.name),
            width,
            &loads
        )
    );
    cls(&loads.cold)
}

/// The first visits that reach `limit(width, scheme, order)` or more, in each order at each width
/// under each scheme, each in a fresh context of `browser`.
fn first_visit_misses(
    browser: &mut Chrome,
    page: &str,
    site: &Site,
    limit: impl Fn(Width, Scheme, Order) -> f64,
) -> Vec<String> {
    let mut misses = Vec::new();
    for scheme in [DARK, LIGHT] {
        for width in [PHONE, WIDE] {
            for order in [PAGE_FIRST, GATES_FIRST] {
                let score = first_visit(browser, page, site, width, scheme, order);
                let limit = limit(width, scheme, order);
                if score >= limit {
                    misses.push(format!(
                        "{page} {} {}, {}: cls {score:.4}, limit {limit}",
                        width.name, scheme.name, order.name
                    ));
                }
            }
        }
    }
    misses
}

/// A site whose task has the gates of the default catalogue: four, one waited on, each with who
/// and when, which is what the section's outline holds for a task this browser knows nothing of
/// (`DEFAULT_ROOM`).
fn default_task() -> Site {
    let (sections, cards) = plain();
    let site = site("docs/PROBE.md", &sections, cards);
    let task = "visual/in-progress";
    let gate = |name: &str, state: &str, by: &str, detail: &str| json!({"name":name,"state":state,"by":by,"at":1_794_363_240.0,"detail":detail});
    site.set_json(
        "/api/task-gates",
        &json!({"slug":task,"branch":task,"gates":[
            gate("review", "missing", "lucid-heron", "Changes asked in round 1"),
            gate("tests", "met", "studio", "Green at a71f0d2"),
            gate("pictures", "met", "quiet-wren", "Attached at a71f0d2"),
            gate("nightly", "met", "nightly", "Green on main a71f0d2")]}),
    );
    let mut page = look_parity::responses()["/api/task-page"].clone();
    page["gates"] = json!({"count":4,"waiting":1,"unsigned":0});
    site.set_json("/api/task-page", &page);
    site
}

/// A task of the shape the outline guesses holds still on every visit, dark and light: its answers
/// as they come, and on a first visit in each order they can come in.
#[test]
fn a_task_holds_still_while_its_thread_loads() {
    let site = default_task();
    let Some(mut browser) = chrome() else { return };
    let mut misses = Vec::new();
    // Each scheme's and width's cold and warm loads, then each first visit, in a context of its
    // own.
    for scheme in [DARK, LIGHT] {
        for width in [PHONE, WIDE] {
            browser.fresh_context();
            let loads = measure_in(
                &mut browser,
                &site,
                TASK,
                TASK_READY,
                width,
                Some(scheme),
                None,
                "",
            );
            misses.extend(over_the_limit(
                &format!("task, {},", scheme.name),
                width,
                &loads,
            ));
        }
    }
    misses.extend(first_visit_misses(
        &mut browser,
        "task",
        &site,
        |_, _, _| CLS_LIMIT,
    ));
    assert!(
        misses.is_empty(),
        "layout shift at or over {CLS_LIMIT}: {misses:#?}"
    );
}

/// A phone's first visit to a task of another shape, on the dark theme, the page's answer drawn
/// before the gates': it scores 0.1399, one entry, the gates' slot going up 32 px under the head
/// and from four rows to two, the timeline drawn under it:
/// `div.task-gates-slot [13,396,364,434]→[13,364,364,243]`,
/// `section#timeline [0,0,0,0]→[13,638,364,206]`. The owner kept that move (the card
/// `task-first-visit-head-first` in docs/DECISIONS.md, every outline at once); the limit only
/// goes down.
const PHONE_PAGE_FIRST_LIMIT: f64 = 0.14;
/// The same visit with the gates' answer drawn before the page's: it scores 0.0636, the timeline
/// drawn (0.0434, `[0,0,0,0]→[13,670,364,174]`) and then both going up 32 px (0.0201,
/// `div.task-gates-slot [13,396,364,243]→[13,364,364,243]`,
/// `section#timeline [13,670,364,174]→[13,638,364,206]`). It only goes down.
const PHONE_GATES_FIRST_LIMIT: f64 = 0.064;
/// [`PHONE_PAGE_FIRST_LIMIT`]'s visit on the light theme: it scores 0.1556. The same elements move
/// the same 32 px; the box a shift moves holds the card's shadow, and the light one
/// (`--shadow-1`, `0 4px 16px` there, `0 1px 2px` dark) reaches 20 px above the card, 28 px below
/// and 24 px at its sides, past the screen's edges, where the dark one reaches 2, 4 and 3:
/// `div.task-gates-slot [0,378,390,466]→[0,346,390,285]`,
/// `section#timeline [0,0,0,0]→[0,638,390,206]`. The difference from dark is that shadow's area
/// alone. It only goes down.
const PHONE_PAGE_FIRST_LIGHT_LIMIT: f64 = 0.156;
/// [`PHONE_GATES_FIRST_LIMIT`]'s visit on the light theme: it scores 0.0689 (0.0465,
/// `section#timeline [0,0,0,0]→[0,670,390,174]`, then 0.0224,
/// `div.task-gates-slot [0,378,390,285]→[0,346,390,285]`,
/// `section#timeline [0,670,390,174]→[0,638,390,206]`), the same move with the light shadow's
/// area. It only goes down.
const PHONE_GATES_FIRST_LIGHT_LIMIT: f64 = 0.069;

/// What a first visit to a task of another shape than the outline's guess scores less than: on a
/// phone what its order measures under its scheme, wide the limit of every page (0.0312 and 0.0288
/// there dark, 0.0322 and 0.0297 light).
fn another_shape_limit(width: Width, scheme: Scheme, order: Order) -> f64 {
    if !width.mobile {
        return CLS_LIMIT;
    }
    match (scheme == LIGHT, order == PAGE_FIRST) {
        (false, true) => PHONE_PAGE_FIRST_LIMIT,
        (false, false) => PHONE_GATES_FIRST_LIMIT,
        (true, true) => PHONE_PAGE_FIRST_LIGHT_LIMIT,
        (true, false) => PHONE_GATES_FIRST_LIGHT_LIMIT,
    }
}

/// A task of another shape than the outline's guess (the shared fixture's two gates, one of them
/// unsigned, where the guess is four signed) holds still on a return visit, and on a first one
/// moves what is under its gates once, by less than its order's limit under each scheme.
#[test]
fn a_task_of_another_shape_moves_less_than_its_first_visits_limits() {
    let (sections, cards) = plain();
    let site = site("docs/PROBE.md", &sections, cards);
    let Some(mut browser) = chrome() else { return };
    let mut misses = Vec::new();
    for scheme in [DARK, LIGHT] {
        for width in [PHONE, WIDE] {
            browser.fresh_context();
            let loads = measure_in(
                &mut browser,
                &site,
                TASK,
                TASK_READY,
                width,
                Some(scheme),
                None,
                "",
            );
            eprintln!(
                "{}",
                report(
                    &format!("task of another shape, {},", scheme.name),
                    width,
                    &loads
                )
            );
            // As the answers come, a first visit is one of the two orders or both in one frame.
            let limit = another_shape_limit(width, scheme, PAGE_FIRST).max(another_shape_limit(
                width,
                scheme,
                GATES_FIRST,
            ));
            let cold = cls(&loads.cold);
            if cold >= limit {
                misses.push(format!(
                    "{} {} cold, as the answers come: cls {cold:.4}, limit {limit}",
                    width.name, scheme.name
                ));
            }
            let warm = cls(&loads.warm);
            if warm >= CLS_LIMIT {
                misses.push(format!(
                    "{} {} warm: cls {warm:.4}, limit {CLS_LIMIT}",
                    width.name, scheme.name
                ));
            }
        }
    }
    misses.extend(first_visit_misses(
        &mut browser,
        "task of another shape",
        &site,
        another_shape_limit,
    ));
    assert!(misses.is_empty(), "layout shift over a limit: {misses:#?}");
}

#[test]
fn a_document_holds_still_while_its_sections_load() {
    holds_still("document", "/docs/PROBE.md", DOCUMENT, None, plain());
}

#[test]
fn the_register_holds_still_while_its_cards_load() {
    holds_still(
        "register",
        "/docs/DECISIONS.md",
        DOCUMENT,
        None,
        register_sections(),
    );
}

#[test]
fn the_chat_holds_still_while_its_conversation_loads() {
    holds_still("chat", "/", HOME, Some(CHAT), plain());
}

/// Scored from once it is open: opening it pushes the page aside on a wide window, which the
/// reader asked for.
#[test]
fn the_activity_panel_holds_still_once_it_is_open() {
    holds_still("activity", "/", HOME, Some(ACTIVITY), plain());
}
