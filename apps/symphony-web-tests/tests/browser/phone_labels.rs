//! What a phone 320 and 390 px wide shows of the rows it scrolls across (`src/scrollrow.ts`): the
//! header's pages, the board's wait strip and its stage tabs. Each is measured as Chrome draws
//! it, in the light and in the dark: the chosen one whole and clear of a faded edge, with its
//! full count; the first, a middle and the last reached by a tap, by the keyboard and by a
//! scroll that rests on a whole label; a focused one's outline uncut; no label ever read in
//! part, one the row cuts being drawn as its empty shape; and an edge faded only while the row
//! continues past it, by fading the row itself, never by a colour drawn over it.
//!
//! `SYMPHONY_PHONE_LABEL_SHOTS=NAME` keeps the pictures a task shows as its evidence, under the
//! checkout's `artifacts/app/phone-navigation-labels/NAME-*.base64`.
use serde_json::{Value, json};

use crate::chrome::Chrome;
use crate::site::Site;
use crate::{READY, SHOWN, chrome, look_parity};

/// The lifecycle stages with the ordinary and the blocked tasks each counts: a stage with none
/// of either kind among them, and counts of two digits.
const STAGES: [(&str, usize, usize); 6] = [
    ("concept", 3, 0),
    ("design", 0, 12),
    ("ready", 31, 5),
    ("in_progress", 2, 18),
    ("review", 1, 18),
    ("done", 12, 0),
];

/// A phone's tabs in their order, each as it reads, with a machine reporting.
const TABS: [&str; 9] = [
    "Review 1+18",
    "In progress 2+18",
    "Ready 31+5",
    "Done 12",
    "Design 0+12",
    "Concept 3",
    "Backlog 0",
    "Build 2",
    "Test 0",
];

fn task(slug: &str, stage: &str, blocked: bool) -> Value {
    json!({"slug": slug, "title": format!("The task {slug}"), "body": "",
        "status": if blocked {"blocked"} else {stage}, "from": if blocked {Some(stage)} else {None},
        "priority": 2, "parent": null, "created_by": "owner", "created": 0.0, "updated": 0.0,
        "links": [], "depends": [], "roles": [], "instruction": false,
        "waits_on": {"kind": if blocked {"check"} else {"queued"}, "who": "tests",
            "why": "The checks have not passed", "since": 0.0}})
}

/// The waiting board's listing: two cards at most of each kind in a stage, the counts whole, a
/// stale orchestrator for its banner and a machine for the Build and Test tabs.
fn listing() -> Value {
    let mut tasks = Vec::new();
    for (stage, ordinary, blocked) in STAGES {
        for n in 0..ordinary.min(2) {
            tasks.push(task(&format!("{stage}/ordinary-{n}"), stage, false));
        }
        for n in 0..blocked.min(2) {
            tasks.push(task(&format!("{stage}/blocked-{n}"), stage, true));
        }
    }
    let mut statuses: Vec<&str> = STAGES.iter().map(|(stage, ..)| *stage).collect();
    statuses.extend(["build", "test"]);
    let mut counts: Vec<Value> = STAGES
        .iter()
        .map(|(stage, ordinary, blocked)| json!([stage, ordinary + blocked]))
        .collect();
    counts.extend([json!(["build", 2]), json!(["test", 0])]);
    let columns: Vec<Value> = STAGES
        .iter()
        .map(|(stage, ordinary, blocked)| {
            json!({"status": stage, "ordinary": ordinary, "blocked": blocked, "oldest": null})
        })
        .collect();
    json!({"tasks": tasks, "statuses": statuses, "counts": counts, "signoff": [], "next": null,
        "waiting": {"kinds": [["decision", 9], ["nobody", 11], ["check", 7], ["task", 14],
            ["working", 2], ["queued", 30], ["parked", 0]], "columns": columns},
        "orchestrators": [{"handle": "quiet-orchestrator", "health": "stale"}], "unstarted": 1,
        "machine_work": {"freshness":"fresh","unassigned":[],"machines":[{
            "machine_id":"alpha","label":"Workstation","producer_id":"fixture","sequence":1,
            "age_ms":0,"freshness":"fresh","complete":true,"reason":"","counts":null,
            "operations":[]}]}})
}

fn site() -> Site {
    let site = crate::site();
    let listing = listing();
    site.set_json("/api/task-board", &listing);
    site.set_json("/api/machine-work", &listing["machine_work"]);
    let content = look_parity::responses();
    for path in ["/api/document", "/api/section", "/api/asks"] {
        site.set_json(path, &content[path]);
    }
    site
}

/// A phone `width` wide in `scheme`, with no pointer moved yet, so a focus shows its outline.
fn phone(page: &mut Chrome, width: u32, scheme: &str) {
    page.scheme(scheme);
    page.viewport_scaled(width, 844, 2, true);
}

fn open(page: &mut Chrome, site: &Site, path: &str, ready: &str) {
    page.open(&site.url(path));
    assert!(
        page.wait_for(
            &format!(
                "{READY} && document.querySelector({}) !== null && document.querySelector('.app-pages[data-overflow]') !== null",
                json!(ready)
            ),
            SHOWN
        ),
        "{path}: {}",
        page.eval("document.body ? document.body.innerText.slice(0, 600) : '(no body)'")
    );
}

/// The board's tabs a machine's work adds, as they read: that work is a read of its own, which
/// may land after the first card is drawn.
const MACHINE_TABS: &str = "(tabs => tabs.includes('Build 2') && tabs.includes('Test 0'))([...document.querySelectorAll('.stage-tabs button')].map(tab => tab.textContent.trim()))";

/// Open the board, its first card drawn and the machine's Build and Test tabs with it.
fn board(page: &mut Chrome, site: &Site) {
    open(page, site, "/tasks", ".waiting-board .taskcard");
    assert!(
        page.wait_for(MACHINE_TABS, SHOWN),
        "the machine's tabs never drew: {}",
        page.eval(
            "[...document.querySelectorAll('.stage-tabs button')].map(tab => tab.textContent)"
        )
    );
}

/// The page's own functions for a row: `row(selector)` measures it, `settled(selector)` says
/// whether it rests where its sheet lets it.
const MEASURES: &str = r#"
window.measured = selector => {
    const row = document.querySelector(selector);
    const box = row.getBoundingClientRect();
    const style = getComputedStyle(row);
    const fade = parseFloat(style.scrollPaddingLeft) || 0;
    const sides = row.dataset.overflow;
    const most = row.scrollWidth - row.clientWidth;
    // The sides the row continues past, measured here and not read from what the page says.
    const real = (row.scrollLeft > 1 ? (most - row.scrollLeft > 1 ? 'both' : 'start') : most - row.scrollLeft > 1 ? 'end' : 'none');
    const from = box.left + (real === 'start' || real === 'both' ? fade : 0);
    const to = box.right - (real === 'end' || real === 'both' ? fade : 0);
    const of = item => {
        const b = item.getBoundingClientRect();
        const shown = Math.max(0, Math.min(b.right, box.right) - Math.max(b.left, box.left));
        return {
            text: item.textContent.trim(),
            // Whole in the row's window, and clear of a faded edge as well.
            whole: shown >= b.width - 0.5,
            clear: b.left >= from - 0.5 && b.right <= to + 0.5,
            showing: shown > 0.5,
            // Drawn as its empty shape: no text or mark of its own to read, and no tap taken.
            blank: item.hasAttribute('data-cut'),
            unread: /, 0\)$/.test(getComputedStyle(item).color) && getComputedStyle(item).pointerEvents === 'none'
                && [...item.children].every(part => getComputedStyle(part).visibility === 'hidden'),
            // The focus outline's box: its width and its offset out from the item's.
            ring: (() => {
                const s = getComputedStyle(item);
                const out = s.outlineStyle === 'none' ? 0 : parseFloat(s.outlineWidth) + parseFloat(s.outlineOffset);
                return {out, uncut: b.top - out >= box.top - 0.5 && b.bottom + out <= box.bottom + 0.5
                    && b.left - out >= box.left - 0.5 && b.right + out <= box.right + 0.5};
            })(),
        };
    };
    // A choice the test set aside (`display: none`) is no part of the row.
    const drawn = [...row.children].filter(item => item.getBoundingClientRect().width > 0);
    const items = drawn.map(of);
    const current = row.querySelectorAll('[aria-current="page"], [aria-pressed="true"]');
    return {
        said: sides, real, fade,
        scrolls: style.overflowX === 'auto' && most > 1,
        oneRow: (mids => Math.max(...mids) - Math.min(...mids) < 2)(drawn.map(item => (b => (b.top + b.bottom) / 2)(item.getBoundingClientRect()))),
        height: Math.round(box.height),
        mask: (style.maskImage || style.webkitMaskImage || 'none') !== 'none',
        painted: style.backgroundImage !== 'none',
        bar: style.scrollbarWidth,
        texts: items.map(item => item.text),
        whole: items.filter(item => item.whole && !item.blank).map(item => item.text),
        // A label showing in part that can be read, a whole one that cannot, and a blank one
        // whose text or marks still draw: each a defect.
        partial: items.filter(item => item.showing && !item.whole && !item.blank).map(item => item.text),
        hidden: items.filter(item => item.whole && item.blank).map(item => item.text),
        legible: items.filter(item => item.blank && !item.unread).map(item => item.text),
        shapes: items.filter(item => item.showing && item.blank).length,
        current: current.length === 1 ? of(current[0]) : current.length,
        focused: row.contains(document.activeElement) ? of(document.activeElement) : null,
    };
};
window.settled = selector => {
    const one = window.measured(selector);
    // A focus scrolls its choice into view a frame on: the row rests once that choice is there.
    const there = item => item === null || typeof item !== 'object' || (item.whole && item.clear && !item.blank);
    return one.said === one.real && one.partial.length + one.hidden.length + one.legible.length === 0
        && there(one.focused);
};
"#;

fn measured(page: &mut Chrome, selector: &str) -> Value {
    page.eval(&format!("window.measured({})", json!(selector)))
}

/// Wait until `selector`'s row rests: it says the sides it really continues past, every label in
/// its window is whole and readable, and each one it cuts is an empty shape.
fn rests(page: &mut Chrome, selector: &str, when: &str) -> Value {
    assert!(
        page.wait_for(&format!("window.settled({})", json!(selector)), SHOWN),
        "{selector} never rested {when}: {}",
        measured(page, selector)
    );
    measured(page, selector)
}

fn shot(page: &mut Chrome, name: &str) {
    let Ok(run) = std::env::var("SYMPHONY_PHONE_LABEL_SHOTS") else {
        return;
    };
    page.frames_with_fonts();
    let image = page.call("Page.captureScreenshot", json!({"format":"png"}));
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../artifacts/app/phone-navigation-labels");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join(format!("{run}-{name}.base64")),
        image["data"].as_str().unwrap(),
    )
    .unwrap();
}

/// The one chosen of a row, whole, clear of a fade and its outline uncut; panics otherwise.
fn chosen(row: &Value, text: &str, what: &str) {
    assert_eq!(row["current"]["text"], json!(text), "{what}: {row}");
    assert_eq!(
        (&row["current"]["whole"], &row["current"]["clear"]),
        (&json!(true), &json!(true)),
        "{what}: the chosen one shows whole, clear of a faded edge: {row}"
    );
}

/// The focused one of a row with its outline drawn and uncut, itself clear of a fade.
fn ringed(row: &Value, text: &str, what: &str) {
    let focused = &row["focused"];
    assert_eq!(focused["text"], json!(text), "{what}: {row}");
    assert!(
        focused["ring"]["out"].as_f64().unwrap() >= 2.0,
        "{what}: the focused one draws an outline: {row}"
    );
    assert_eq!(
        (
            &focused["ring"]["uncut"],
            &focused["whole"],
            &focused["clear"]
        ),
        (&json!(true), &json!(true), &json!(true)),
        "{what}: the outline is whole, on a label clear of a faded edge: {row}"
    );
}

/// The cue of a row that continues past `sides`: the row fades there and nowhere else, itself,
/// with no scrollbar and nothing painted over the surface behind it.
fn cued(row: &Value, sides: &str, what: &str) {
    assert_eq!(
        (&row["said"], &row["real"]),
        (&json!(sides), &json!(sides)),
        "{what}: {row}"
    );
    assert_eq!(row["mask"], json!(sides != "none"), "{what}: {row}");
    assert_eq!(row["painted"], json!(false), "{what}: {row}");
    assert_eq!(row["bar"], json!("none"), "{what}: {row}");
    assert_eq!(
        (&row["partial"], &row["hidden"], &row["legible"]),
        (&json!([]), &json!([]), &json!([])),
        "{what}: no label reads in part, and every whole one reads: {row}"
    );
}

/// The Symphony's seven pages show whole at 320 and at 390, the page shown lit among them; a
/// project's own page beyond them makes the row scroll, faded at the edge it continues past and
/// only there, and that page, opened, is brought whole into view. The first, a middle and the
/// last page take the keyboard's focus with their outline uncut.
#[test]
fn a_phones_pages_show_whole_and_the_row_fades_only_where_it_continues() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    for (width, scheme) in [(320, "light"), (320, "dark"), (390, "light"), (390, "dark")] {
        let what = format!("{width} {scheme}");
        phone(&mut page, width, scheme);
        board(&mut page, &site);
        page.eval(MEASURES);
        let row = rests(&mut page, ".app-pages", &what);
        assert_eq!(row["oneRow"], json!(true), "{what}: {row}");
        // The Symphony's seven whole, with the project's own beyond the edge.
        assert_eq!(
            row["whole"],
            json!([
                "Home", "Queue", "Docs", "Board", "Reports", "Runs", "Agents"
            ]),
            "{what}: {row}"
        );
        assert_eq!(row["scrolls"], json!(true), "{what}: {row}");
        cued(&row, "end", &what);
        chosen(&row, "Board", &what);
        // The keyboard: the first, a middle and the last, each scrolled to as it is focused.
        for (at, name, sides) in [
            (0, "Home", "end"),
            (3, "Board", "end"),
            (7, "⬡ lamps", "start"),
        ] {
            page.eval(&format!(
                "document.querySelectorAll('.app-pages a')[{at}].focus()"
            ));
            let row = rests(&mut page, ".app-pages", &format!("{what}, {name} focused"));
            ringed(&row, name, &what);
            cued(&row, sides, &format!("{what}, {name} focused"));
        }
        shot(&mut page, &format!("{width}-{scheme}-pages-last-focused"));
        page.eval("document.activeElement.blur()");
        // A touch's scroll rests on a label's start, wherever it let go.
        page.eval("document.querySelector('.app-pages').scrollTo({left: 23})");
        let row = rests(&mut page, ".app-pages", &format!("{what}, scrolled"));
        assert_eq!(row["partial"], json!([]), "{what}: {row}");
        // The project's own page, the last, is in view when it is the one shown.
        open(&mut page, &site, "/lamps", "#appearance");
        page.eval(MEASURES);
        let row = rests(
            &mut page,
            ".app-pages",
            &format!("{what}, the last page shown"),
        );
        chosen(&row, "⬡ lamps", &what);
        cued(&row, "start", &format!("{what}, the last page shown"));
        // With the Symphony's pages alone nothing is past either edge, and nothing fades.
        page.eval("document.querySelector('.app-pages a:last-child').style.display = 'none'; document.querySelector('.app-pages').dispatchEvent(new Event('scroll'))");
        let row = rests(&mut page, ".app-pages", &format!("{what}, seven pages"));
        assert_eq!(row["scrolls"], json!(false), "{what}: {row}");
        assert_eq!(row["whole"].as_array().unwrap().len(), 7, "{what}: {row}");
        cued(&row, "none", &format!("{what}, seven pages"));
        assert_eq!(
            page.eval("document.documentElement.scrollWidth <= innerWidth && document.querySelector('.app-header').getBoundingClientRect().height < 100"),
            json!(true),
            "{what}: the header is two rows, as wide as the window"
        );
    }
}

/// The board's stage tabs on a phone: one stage chosen, shown whole with its full count, zero or
/// not; the first, a middle and the last stage reached by a tap and by the keyboard, each whole
/// and clear of a fade with its outline uncut, the row fading only where it continues; and a
/// scroll let go anywhere rests on a whole label.
#[test]
fn a_phones_stage_tabs_keep_the_chosen_stage_whole_with_its_count_and_its_outline() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    for (width, scheme) in [(320, "light"), (320, "dark"), (390, "light"), (390, "dark")] {
        let what = format!("{width} {scheme}");
        phone(&mut page, width, scheme);
        board(&mut page, &site);
        page.eval(MEASURES);
        page.eval("localStorage.clear()");
        let row = rests(&mut page, ".stage-tabs", &what);
        assert_eq!(row["texts"], json!(TABS), "{what}: {row}");
        assert_eq!(row["oneRow"], json!(true), "{what}: {row}");
        chosen(&row, "Review 1+18", &what);
        cued(&row, "end", &what);
        shot(&mut page, &format!("{width}-{scheme}-review"));
        // A tap on the first, a middle (one with no ordinary task) and the last (with none at
        // all), each then the one stage mounted.
        for (name, status, sides) in [
            ("Design 0+12", "design", "both"),
            ("Test 0", "test", "start"),
            ("Review 1+18", "review", "end"),
        ] {
            let at = TABS.iter().position(|tab| *tab == name).unwrap();
            page.eval(&format!(
                "(tab => {{ tab.focus(); tab.click(); }})(document.querySelectorAll('.stage-tabs button')[{at}])"
            ));
            assert!(
                page.wait_for(
                    &format!("document.querySelector('.stage-tabs [aria-current=page]')?.textContent.startsWith({}) && document.querySelector('.waiting-columns .column[data-status={status}]') !== null", json!(&name[..4])),
                    SHOWN
                ),
                "{what}: {name} never opened"
            );
            let row = rests(&mut page, ".stage-tabs", &format!("{what}, {name} chosen"));
            chosen(&row, name, &what);
            ringed(&row, name, &what);
            cued(&row, sides, &format!("{what}, {name} chosen"));
            assert_eq!(
                page.eval("[...document.querySelectorAll('.waiting-columns .column')].map(column => column.dataset.status)"),
                json!([status]),
                "{what}: one stage mounted"
            );
            if status == "design" {
                shot(&mut page, &format!("{width}-{scheme}-design"));
            }
        }
        // The keyboard alone: each tab focused in turn is scrolled to, its outline uncut.
        for (at, tab) in TABS.iter().enumerate() {
            page.eval(&format!(
                "document.querySelectorAll('.stage-tabs button')[{at}].focus()"
            ));
            let row = rests(
                &mut page,
                ".stage-tabs",
                &format!("{what}, tab {at} focused"),
            );
            ringed(&row, tab, &what);
        }
        page.eval("document.activeElement.blur()");
        // A touch's scroll let go between two labels rests on one's start.
        for left in [57, 311, 505] {
            page.eval(&format!(
                "document.querySelector('.stage-tabs').scrollTo({{left: {left}}})"
            ));
            let row = rests(
                &mut page,
                ".stage-tabs",
                &format!("{what}, scrolled to {left}"),
            );
            cued(
                &row,
                row["real"].as_str().unwrap(),
                &format!("{what} at {left}"),
            );
            // What fades at an edge the row continues past is a choice's shape.
            assert!(
                row["shapes"].as_u64().unwrap() >= 1,
                "{what} at {left}: {row}"
            );
        }
    }
}

/// The wait strip is one row on a phone, so the cards start higher: its count of open tasks
/// first, a filter pressed brought whole into view, and both rows of choices together no taller
/// than 90 px above a first card that shows whole on the first screen. The page never scrolls
/// sideways, and the strip never moves as a filter is pressed and released, the banner over it
/// saying the same words.
#[test]
fn a_phones_wait_strip_is_one_row_that_keeps_the_pressed_filter_in_view() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    for (width, scheme) in [(320, "dark"), (390, "light")] {
        let what = format!("{width} {scheme}");
        phone(&mut page, width, scheme);
        board(&mut page, &site);
        page.eval(MEASURES);
        let row = rests(&mut page, ".wait-strip", &what);
        assert_eq!(row["oneRow"], json!(true), "{what}: {row}");
        assert_eq!(
            row["texts"],
            json!([
                "73 open",
                "Your decisions 9",
                "On nobody 11",
                "On checks 7",
                "On tasks 14",
                "Working 2",
                "Queued 30",
                "Parked 0"
            ]),
            "{what}: {row}"
        );
        chosen(&row, "73 open", &what);
        cued(&row, "end", &what);
        assert_eq!(
            page.eval(
                r#"(() => {
                    const box = s => document.querySelector(s).getBoundingClientRect();
                    const card = box('.waiting-columns .taskcard');
                    return {
                        rows: Math.round(box('.stage-tabs').bottom - box('.wait-strip').top) <= 90,
                        card: card.top >= 0 && card.bottom <= innerHeight,
                        fits: document.documentElement.scrollWidth <= innerWidth,
                    };
                })()"#
            ),
            json!({"rows":true,"card":true,"fits":true}),
            "{what}"
        );
        // The strip stays where it is on the screen as a chip is pressed and released: the banner
        // over it keeps its words, the board's ready total among them, which the filter's listing
        // does not count, and the row of work under way between them keeps its room.
        const STRIP_TOP: &str =
            "Math.round(document.querySelector('.wait-strip').getBoundingClientRect().top)";
        const BANNER: &str = "document.querySelector('.orchestrator-banner').textContent";
        let (strip_top, banner) = (page.eval(STRIP_TOP), page.eval(BANNER));
        assert_eq!(
            banner,
            json!(
                "No live orchestrator. quiet-orchestrator is stale · 1 of the 31 ready tasks wait to start"
            ),
            "{what}"
        );
        // The row names the tasks no machine has by their stage, the count their tab says, so it
        // reads apart from the strip's Working, the tasks an agent is at in any stage.
        assert_eq!(
            page.eval(
                "[...document.querySelectorAll('.under-way > *')].map(cell => cell.textContent)"
            ),
            json!(["4 under way", "2 in progress", "2 building", "0 testing"]),
            "{what}: the row of work under way is over the strip"
        );
        shot(&mut page, &format!("{width}-{scheme}-open"));
        page.eval("(chip => { chip.focus(); chip.click(); })(document.querySelector('.wait-chip.k-parked'))");
        assert!(
            page.wait_for(
                "document.querySelector('.wait-chip.k-parked[aria-pressed=true]') !== null && document.querySelector('.column[data-status=waiting]') !== null && document.querySelector('.stage-tabs') === null",
                SHOWN
            ),
            "{what}: Parked never pressed"
        );
        let row = rests(&mut page, ".wait-strip", &format!("{what}, Parked pressed"));
        chosen(&row, "Parked 0", &what);
        ringed(&row, "Parked 0", &what);
        cued(&row, "start", &format!("{what}, Parked pressed"));
        assert_eq!(
            (page.eval(STRIP_TOP), page.eval(BANNER)),
            (strip_top.clone(), banner.clone()),
            "{what}: pressing a chip moved the strip or changed the banner's words"
        );
        shot(&mut page, &format!("{width}-{scheme}-parked"));
        // Back to every open task: the stages return with the stage chosen before.
        page.eval(
            "(count => { count.focus(); count.click(); })(document.querySelector('.open-count'))",
        );
        assert!(
            page.wait_for(
                "document.querySelector('.stage-tabs [aria-current=page]') !== null",
                SHOWN
            ),
            "{what}: the stages never returned"
        );
        let row = rests(&mut page, ".stage-tabs", &format!("{what}, stages back"));
        chosen(&row, "Review 1+18", &what);
        let row = rests(&mut page, ".wait-strip", &format!("{what}, every task"));
        chosen(&row, "73 open", &what);
        cued(&row, "end", &format!("{what}, every task"));
        assert_eq!(
            (page.eval(STRIP_TOP), page.eval(BANNER)),
            (strip_top, banner),
            "{what}: releasing the chip moved the strip or changed the banner's words"
        );
    }
}
