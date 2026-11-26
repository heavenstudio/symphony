//! A task's timeline on its page (apps/symphony-web: src/timeline-slot.tsx, the page's box;
//! src/timeline.tsx, the chunk the box imports into it), the release client against the stand-in
//! server answering `/api/task-timeline` with the approved pictures' task: a list that fits a
//! phone's width, lanes over the same hours on a wide screen, a box that moves nothing when the
//! timeline arrives, and a read each minute only while the page is shown and the task open.
use super::{SHOWN, binaries, chrome, components, look_parity, site, site::Site};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;

mod messages;
mod quiet;
mod zoom;

const TASK: &str = "/task/visual/in-progress";
const SLUG: &str = "visual/in-progress";

/// Seconds between the stand-in's minute reads: none of the page's other timers waits as long, so
/// the page's timer for it is told apart by its length ([`MINUTES`]).
const EVERY: u64 = 61;

/// Before the page's own scripts: the timer of the timeline's minute read is held, not run, as
/// `minute()` for the test to run, and `minuteTimers` counts how many the page set.
const MINUTES: &str = "window.minuteTimers=0;window.minute=null;const timer=window.setTimeout;\
    window.setTimeout=function(fn,ms,...args){if(ms===61000){minuteTimers++;window.minute=()=>{window.minute=null;fn(...args)};return 0}return timer(fn,ms,...args)};";

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../../crates/symphony-api/fixtures/task-timeline.json"
    ))
    .expect("the timeline fixture")
}

/// A week-long task of fifty steps: twenty-four review rounds of two minutes each, each sent back
/// after six hours' work, the last of it going on now.
fn week() -> Value {
    let mut week = fixture();
    let now = week["now"].as_f64().unwrap();
    let made = now - 7.0 * 86_400.0;
    let mut stages =
        vec![json!({"status":"ready","from":made,"to":made + 3600.0,"by":"glacial-merlin"})];
    let mut rounds = Vec::new();
    let mut from = made + 3600.0;
    for round in 1..=24 {
        let review = from + 6.0 * 3600.0;
        stages.push(json!({"status":"in_progress","from":from,"to":review,"by":"quiet-wren"}));
        stages.push(json!({"status":"review","from":review,"to":review + 120.0,"by":"quiet-wren","round":round}));
        rounds.push(json!({"round":round,"from":review,"taken":review + 30.0,"to":review + 120.0,"by":"lucid-heron","verdict":"changes"}));
        from = review + 120.0;
    }
    stages.push(json!({"status":"in_progress","from":from,"by":"lucid-heron"}));
    week["zone"] = json!([{"from": made, "seconds": 7200}]);
    week["created"] = json!(made);
    week["first_claimed"] = json!(made + 3600.0);
    week["bounces"] = json!(24);
    week["stages"] = json!(stages);
    week["claims"] = json!([{"by":"quiet-wren","from":made + 3600.0}]);
    week["rounds"] = json!(rounds);
    week["runs"] = json!([]);
    week["gates"] = json!([{"name":"review","states":[{"state":"missing","from":made + 7.0 * 3600.0,"detail":"Changes asked"}]}]);
    week
}

/// The card's example task (docs/design/analysis/timeline-navigation.md), `tools/workflow-revision-guards`
/// as it closed on 2026-11-20, from the analysis's mockup: 42 builds and 135 tests, the runs kept
/// from 11-19 11:09, 60 messages, and the 10 h 54 m after it closed with nothing recorded.
fn revision_guards() -> Value {
    serde_json::from_str(include_str!("timeline/revision-guards.json"))
        .expect("the card's example task")
}

/// The stand-in server at `site` answering the card's example task, its thread a page of all its
/// messages.
fn guards_on(site: Site) -> Site {
    let guards = revision_guards();
    let site = served_by(site, &guards);
    let ids: Vec<i64> = guards["history"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["id"].as_i64().unwrap())
        .collect();
    site.set_json("/api/task-history", &messages::thread(&guards, &ids, None));
    site
}

/// The stand-in server answering the look-parity task's page and thread, no offer to the owner, and
/// `timeline` for its timeline.
fn served(timeline: &Value) -> Site {
    served_by(site(), timeline)
}

/// [`served`], from `site`: the client it serves.
fn served_by(site: Site, timeline: &Value) -> Site {
    let content = look_parity::responses();
    for path in ["/api/task-page", "/api/task-history"] {
        site.set_json(path, &content[path]);
    }
    site.set_json(
        "/api/task-offer",
        &json!({"action": "approve", "label": "Approve", "branch": SLUG, "payload": null}),
    );
    site.set_json(symphony_api::TASK_TIMELINE_PATH, timeline);
    site
}

/// The fixture read each [`EVERY`] seconds.
fn timed(mut timeline: Value) -> Value {
    timeline["every"] = json!(EVERY);
    timeline
}

fn view(chrome: &mut super::chrome::Chrome, width: u32, phone: bool) {
    chrome.viewport(width, 900, phone);
    chrome.frames();
}

/// Whether the elements `selector` matches are drawn (laid out with a size).
fn drawn(chrome: &mut super::chrome::Chrome, selector: &str) -> bool {
    chrome.eval(&format!(
        "[...document.querySelectorAll({})].some(e => e.getClientRects().length > 0 && e.getBoundingClientRect().width > 0)",
        json!(selector)
    )) == json!(true)
}

/// The text of every element `selector` matches that is drawn.
fn shown(chrome: &mut super::chrome::Chrome, selector: &str) -> Vec<String> {
    serde_json::from_value(chrome.eval(&format!(
        "[...document.querySelectorAll({})].filter(e => e.getClientRects().length > 0).map(e => e.textContent.trim())",
        json!(selector)
    )))
    .unwrap_or_default()
}

/// How many times the page read its task's timeline.
fn reads(site: &Site) -> usize {
    site.targets()
        .iter()
        .filter(|target| target.starts_with(symphony_api::TASK_TIMELINE_PATH))
        .count()
}

/// Open the task's page at `width` and wait for its timeline's `rows` rows.
fn open(chrome: &mut super::chrome::Chrome, site: &Site, width: u32, rows: usize) {
    view(chrome, width, width < 900);
    chrome.open(&site.url(TASK));
    let drawn =
        format!("document.querySelectorAll('.tl-drawn .tl-list .tl-row').length === {rows}");
    assert!(
        chrome.wait_for(&drawn, SHOWN),
        "the timeline's {rows} rows were not drawn: {:?}",
        chrome.eval("document.querySelector('article.task')?.innerText")
    );
    chrome.frames();
}

/// Open the task's page at `width` and wait for its lanes (wide) or its list (on a phone) to be
/// drawn, however many steps the timeline has.
fn open_drawn(chrome: &mut super::chrome::Chrome, site: &Site, width: u32) {
    view(chrome, width, width < 900);
    chrome.open(&site.url(TASK));
    let part = if width < 900 { ".tl-list" } else { ".tl-lanes" };
    assert!(
        chrome.wait_for(
            &format!(
                "document.querySelector('.tl-drawn {part}')?.getBoundingClientRect().height > 0"
            ),
            SHOWN
        ),
        "the timeline was not drawn: {:?}",
        chrome.eval("document.querySelector('article.task')?.innerText")
    );
    chrome.frames();
}

/// A person's click on what `selector` matches: brought to the middle of the screen, pressed and
/// released at its centre, where it is the element on top.
fn tap(chrome: &mut super::chrome::Chrome, selector: &str) {
    let at = chrome.eval(&format!(
        "(() => {{ const e = document.querySelector({0}); e.scrollIntoView({{block: 'center'}}); const b = e.getBoundingClientRect(); const x = b.left + b.width / 2, y = b.top + b.height / 2; return {{x, y, hit: document.elementFromPoint(x, y)?.closest({0}) === e}}; }})()",
        json!(selector)
    ));
    assert_eq!(
        at["hit"],
        json!(true),
        "{selector} is covered at its centre: {at}"
    );
    for kind in ["mousePressed", "mouseReleased"] {
        chrome.call(
            "Input.dispatchMouseEvent",
            json!({"type": kind, "x": at["x"], "y": at["y"], "button": "left", "clickCount": 1}),
        );
    }
    chrome.frames();
}

/// After a click on "Show all" or "Show the latest": whether the "+" chat sheet is open, and
/// whether the focus is still on the button. A click in the timeline is the timeline's alone.
const LEFT_ALONE: &str = "[document.getElementById('quick-open').getAttribute('aria-expanded'), document.activeElement === document.querySelector('.tl-more')]";

/// Where the brief and the thread's heading start on the page, and the timeline's box.
fn places(chrome: &mut super::chrome::Chrome) -> Value {
    chrome.eval("(() => { const top = e => e ? Math.round((e.getBoundingClientRect().top + scrollY) * 10) / 10 : null; const box = document.getElementById('timeline-slot').getBoundingClientRect(); return {brief: top(document.querySelector('.task-body')), thread: top([...document.querySelectorAll('article.task > h2')].find(h => h.textContent === 'Thread')), box: [Math.round(box.top + scrollY), Math.round(box.height)]}; })()")
}

#[test]
fn a_tasks_timeline_is_a_list_that_fits_a_phone_and_lanes_on_a_wide_screen() {
    let site = served(&fixture());
    let Some(mut chrome) = chrome() else { return };
    open(&mut chrome, &site, 390, 13);
    assert!(
        site.targets()
            .iter()
            .any(|target| target == "/api/task-timeline?slug=visual%2Fin-progress"),
        "the page asks for its own task's timeline: {:?}",
        site.targets()
    );
    assert_eq!(
        chrome.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true),
        "the page scrolls sideways at 390 px: {}",
        chrome.eval("document.documentElement.scrollWidth")
    );
    assert!(drawn(&mut chrome, ".tl-list") && !drawn(&mut chrome, ".tl-lanes"));
    assert_eq!(
        shown(&mut chrome, ".tl-totals span"),
        [
            "6 h 18 m so far · 2 review rounds · 1 bounce",
            "Machine 1 h 33 m, queued 26 m"
        ]
    );
    let labels = super::texts(&mut chrome, ".tl-row .tl-label");
    assert_eq!(labels[0], "Ready, unclaimed");
    assert_eq!(labels[11], "Main moved: certificate again");
    assert_eq!(labels[12], "Certificate verify · 4be91c2");
    // Every label on one line, cut short where it must be, never wrapped.
    assert_eq!(
        chrome.eval("[...document.querySelectorAll('.tl-label')].every(e => e.getClientRects().length === 1 && e.getBoundingClientRect().height < 24)"),
        json!(true)
    );
    // Every row keeps inside the box, its bar and its word too.
    assert_eq!(
        chrome.eval("(() => { const box = document.getElementById('timeline-slot').getBoundingClientRect(); return [...document.querySelectorAll('.tl-row, .tl-row *')].every(e => { const r = e.getBoundingClientRect(); return r.width === 0 || (r.left >= box.left - 0.5 && r.right <= box.right + 0.5); }); })()"),
        json!(true)
    );
    // Thirteen steps overflow the phone's box: the latest in view, the rest behind "Show all".
    assert_eq!(chrome.eval(MORE), json!("Show all 13 steps"));
    assert_eq!(
        chrome.eval(LATEST_IN_VIEW),
        json!(true),
        "the latest step is in view"
    );
    // The theme colours the run that goes on now: striped, between its queue and its word.
    assert_eq!(
        chrome.eval("getComputedStyle(document.querySelector('.tl-list .tl-running')).backgroundImage.startsWith('repeating-linear-gradient')"),
        json!(true)
    );

    view(&mut chrome, 1280, false);
    assert!(drawn(&mut chrome, ".tl-lanes"));
    // Wide, the box holds every lane, with no "Show all" even though the phone's draw offered it.
    assert_eq!(chrome.eval(EVERY_STEP_SHOWN), json!(true));
    assert_eq!(shown(&mut chrome, ".tl-more"), Vec::<String>::new());
    assert_eq!(
        super::texts(&mut chrome, ".tl-lane[data-lane] .tl-name b"),
        ["Stage", "Executor", "Reviewer", "Builds", "Tests"]
    );
    assert_eq!(
        super::texts(&mut chrome, ".tl-gate .tl-name"),
        ["certificate", "dependency", "nightly", "review"]
    );
    assert_eq!(super::texts(&mut chrome, ".tl-nowtag"), ["now 15:20"]);
    assert_eq!(
        shown(&mut chrome, ".tl-totals span"),
        [
            "6 h 18 m so far · 2 review rounds · 1 bounce",
            "· machine 1 h 33 m, queued 26 m"
        ]
    );
    // The run that goes on now is striped in its lane too.
    assert_eq!(
        chrome.eval("getComputedStyle(document.querySelector('.tl-lanes .tl-seg.tl-running')).backgroundImage.startsWith('repeating-linear-gradient')"),
        json!(true)
    );
    // The lanes share the hours: the line at now meets the end of every lane's track.
    assert_eq!(
        chrome.eval("(() => { const now = document.querySelector('.tl-now').getBoundingClientRect().left; return [...document.querySelectorAll('.tl-lane:not(.tl-gates-head) .tl-track')].every(t => Math.abs(t.getBoundingClientRect().right - now) < 1.5); })()"),
        json!(true)
    );
    assert_eq!(
        chrome.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
}

/// Whether the phone's list shows its latest step whole inside the part that holds it.
const LATEST_IN_VIEW: &str = "(() => { const body = document.getElementById('tl-body').getBoundingClientRect(); const last = [...document.querySelectorAll('.tl-list .tl-row')].pop().getBoundingClientRect(); return last.bottom <= body.bottom + 1 && last.top >= body.top; })()";

/// A page drawn wide, where the box holds every lane, and narrowed to a phone (a tablet turned, a
/// window halved) collapses its list as a phone draws it: the latest steps in view, the rest
/// behind "Show all"; widened again, it holds every lane, with no "Show all".
#[test]
fn a_timeline_drawn_wide_and_narrowed_to_a_phone_collapses_as_a_phone_draws_it() {
    let site = served(&fixture());
    let Some(mut chrome) = chrome() else { return };
    open(&mut chrome, &site, 1280, 13);
    assert_eq!(chrome.eval(MORE), Value::Null, "Show all offered wide");
    view(&mut chrome, 390, true);
    assert!(
        chrome.wait_for(
            "document.querySelector('.tl-drawn.tl-clipped .tl-more') !== null",
            SHOWN
        ),
        "narrowed, the list was not measured again: {}",
        chrome.eval("document.querySelector('.tl-drawn').className")
    );
    chrome.frames();
    assert_eq!(chrome.eval(MORE), json!("Show all 13 steps"));
    assert_eq!(chrome.eval(BOX_HEIGHT), json!(240));
    assert_eq!(
        chrome.eval(LATEST_IN_VIEW),
        json!(true),
        "the latest step is in view"
    );
    view(&mut chrome, 1280, false);
    assert!(
        chrome.wait_for("document.querySelector('.tl-clipped') === null", SHOWN),
        "widened, the lanes were not measured again"
    );
    assert_eq!(chrome.eval(EVERY_STEP_SHOWN), json!(true));
    assert_eq!(chrome.eval(MORE), Value::Null, "Show all offered wide");
}

#[test]
fn a_task_with_no_run_kept_draws_no_builds_or_tests() {
    let mut timeline = fixture();
    timeline["runs"] = json!([]);
    let site = served(&timeline);
    let Some(mut chrome) = chrome() else { return };
    open(&mut chrome, &site, 1280, 9);
    assert_eq!(
        chrome.eval("document.querySelectorAll('.tl-lanes .tl-lane[data-lane]').length"),
        json!(3)
    );
    assert_eq!(
        chrome.eval("document.querySelector('.tl-lane[data-lane=builds], .tl-lane[data-lane=tests], .tl-running, .tl-machine')"),
        Value::Null
    );
    assert_eq!(
        shown(&mut chrome, ".tl-totals"),
        ["6 h 18 m so far · 2 review rounds · 1 bounce"]
    );
}

/// The pictures' task's tests run on the same commit as its build: a run's words name its command,
/// its wait and its run time apart.
const LINT: &str =
    "Build, just lint-rs · waited 3 m · ran 4 m · green · at 9ac5149 by quiet-wren on studio";
const VERIFY: &str =
    "Test, just verify · waited 6 m · ran 24 m · green · at 9ac5149 by quiet-wren on studio";
const CERTIFICATE: &str =
    "Test, certificate verify · waited 10 m · ran 18 m so far · at 4be91c2 on studio";

/// Where the list's last row is in the box and what it says under its bar: the row's top and bottom
/// from the box's top, its words
/// and whether they are whole inside the part of the box that shows the steps, that part's scroll,
/// the box's height and the words the box's foot shows.
const LAST_ROW: &str = "(() => { const row = document.querySelector('.tl-list > li:last-of-type'); const facts = row.querySelector('.tl-facts'); const body = document.getElementById('tl-body').getBoundingClientRect(); const f = facts.getBoundingClientRect(); const shown = facts.getClientRects().length > 0; const slot = document.getElementById('timeline-slot').getBoundingClientRect().top; return {top: Math.round(row.getBoundingClientRect().top - slot), bottom: Math.round(row.getBoundingClientRect().bottom - slot), facts: shown ? facts.textContent : null, whole: shown && f.top >= body.top - 0.5 && f.bottom <= body.bottom + 0.5, scrolled: document.getElementById('tl-body').scrollTop, box: Math.round(document.getElementById('timeline-slot').getBoundingClientRect().height), foot: [...document.querySelectorAll('.tl-foot .tl-detail, .tl-foot .tl-legend li')].filter(e => e.getClientRects().length > 0).map(e => e.textContent.trim())}; })()";

/// A run's words, for a pointer resting on its part and for its focus: wide, a lane's part the
/// keyboard or a click reaches says them in the legend's place, and a pointer resting on it reads
/// them; on a phone, a run's row a tap reaches says them under its bar, growing upward in a box that
/// keeps its height and the rows under it where they were, the legend left in its place.
#[test]
fn a_runs_hover_and_focus_say_its_command_its_wait_and_its_run_time_apart() {
    let site = served(&fixture());
    let Some(mut chrome) = chrome() else { return };
    open(&mut chrome, &site, 1280, 13);
    // The build is in its lane, the tests in theirs, each a part of its own the keyboard stops at.
    let said = |chrome: &mut super::chrome::Chrome, lane: &str| {
        chrome.eval(&format!(
            "[...document.querySelectorAll('.tl-lane[data-lane={lane}] [role=img]')].map(e => e.getAttribute('aria-label'))"
        ))
    };
    assert_eq!(said(&mut chrome, "builds"), json!([LINT]));
    // A run 30 px wide or more says its distinguishing words and its length inside its bar, cut
    // to fit there.
    assert_eq!(
        chrome.eval(RUN_WORDS),
        json!({"shown": ["verify · 24 m", "verify · 23 m", "certificate verify · 24 m", "certificate verify · 18 m"], "held": true})
    );
    assert_eq!(
        said(&mut chrome, "tests")[0],
        json!(VERIFY),
        "the tests' lane: {}",
        said(&mut chrome, "tests")
    );
    // A pointer resting on the test's part reads all of it, as on the queue before it.
    let selector = ".tl-lane[data-lane=tests] [role=img]";
    let under = chrome.eval(&format!(
        "(() => {{ const b = document.querySelector({}).getBoundingClientRect(); const at = document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2); return [at.closest('[title]').title, document.querySelector('.tl-lane[data-lane=tests] .tl-waiting').title]; }})()",
        json!(selector)
    ));
    assert_eq!(under, json!([VERIFY, VERIFY]));
    // A click focuses it, as the keyboard does: its words take the legend's place.
    tap(&mut chrome, selector);
    assert_eq!(shown(&mut chrome, ".tl-detail"), [VERIFY]);
    assert!(shown(&mut chrome, ".tl-legend li").is_empty());
    // On a phone, the latest run's row a tap reaches says all of the run under its bar, whole in
    // the box, which keeps its height; the row grows upward, its bottom where it was, and the foot
    // keeps the legend.
    view(&mut chrome, 390, true);
    assert!(chrome.wait_for(
        "document.querySelector('.tl-drawn.tl-clipped .tl-more') !== null",
        SHOWN
    ));
    chrome.frames();
    let legend = json!(["worked or ran", "queued or waiting", "running now"]);
    let before = chrome.eval(LAST_ROW);
    assert_eq!(
        (&before["facts"], &before["box"], &before["foot"]),
        (&Value::Null, &json!(240), &legend)
    );
    tap(&mut chrome, ".tl-list > li:last-of-type");
    assert_eq!(
        chrome.eval(
            "document.activeElement === document.querySelector('.tl-list > li:last-of-type')"
        ),
        json!(true)
    );
    chrome.frames();
    let mut focused = chrome.eval(LAST_ROW);
    let top = focused["top"].take();
    assert!(
        top.as_f64() < before["top"].as_f64(),
        "the row grew down from {before} to {focused}"
    );
    assert_eq!(
        focused,
        json!({"top": null, "bottom": before["bottom"], "facts": CERTIFICATE, "whole": true, "scrolled": 0, "box": 240, "foot": legend}),
        "before the tap: {before}"
    );
    assert_eq!(
        chrome.eval("document.getElementById('quick-open').getAttribute('aria-expanded')"),
        json!("false"),
        "the tap reached the page"
    );
    // The totals give the row their room while it has the focus.
    assert!(!drawn(&mut chrome, ".tl-totals"));
    // A tap elsewhere takes the focus: the row says no more and the totals are back.
    tap(&mut chrome, ".tl-legend");
    assert!(drawn(&mut chrome, ".tl-totals"));
    assert_eq!(chrome.eval(LAST_ROW), before);
    assert_eq!(
        chrome.eval("document.activeElement?.closest('.tl-row') ?? null"),
        Value::Null
    );
}

/// On a phone, a timeline the box holds whole, its run's row a tap reaches says all of the run
/// under its bar, the row growing down, its top where it was, and the box with it where it needs to,
/// never under its height and no step cut off; in a box that clips, long words are whole too; and
/// "Show all" open, the box grows to hold them, as the reader asked.
#[test]
fn on_a_phone_a_runs_row_says_all_of_it_in_every_box() {
    let mut short = fixture();
    let first = short["stages"][0].clone();
    short["stages"] = json!([first]);
    short["stages"][0]["to"] = Value::Null;
    let last = short["runs"][0].clone();
    short["runs"] = json!([last]);
    for empty in ["gates", "claims", "rounds"] {
        short[empty] = json!([]);
    }
    let site = served(&short);
    let Some(mut chrome) = chrome() else { return };
    open(&mut chrome, &site, 390, 2);
    assert_eq!(
        chrome.eval("document.querySelector('.tl-drawn').className"),
        json!("tl-drawn"),
        "the box holds every step"
    );
    let before = chrome.eval(LAST_ROW);
    tap(&mut chrome, ".tl-list > li:last-of-type");
    chrome.frames();
    let focused = chrome.eval(LAST_ROW);
    assert_eq!(
        (&focused["top"], &focused["facts"], &focused["whole"]),
        (&before["top"], &json!(LINT), &json!(true)),
        "before the tap: {before}, then {focused}"
    );
    assert!(
        focused["bottom"].as_f64() > before["bottom"].as_f64()
            && focused["box"].as_f64() >= before["box"].as_f64(),
        "before the tap: {before}, then {focused}"
    );
    assert_eq!(chrome.eval(EVERY_STEP_SHOWN), json!(true), "a step cut off");
    // A box that clips, its totals on two lines, a row more for the runs kept from a time, and a
    // run whose words take five lines: they are whole, the box as tall as before.
    let mut long = fixture();
    let host = "a-long-machine-name-of-the-studio-in-the-back-room.local";
    for run in long["runs"].as_array_mut().unwrap() {
        run["host"] = json!(host);
    }
    long["runs_from"] = json!(long["created"].as_f64().unwrap() + 83.0 * 60.0);
    let site = served(&long);
    // A browser of its own: this one keeps the short timeline's height for the task.
    let Some(mut chrome) = super::chrome() else {
        return;
    };
    open(&mut chrome, &site, 390, 14);
    assert!(drawn(&mut chrome, ".tl-kept"));
    tap(&mut chrome, ".tl-list > li:last-of-type");
    chrome.frames();
    let focused = chrome.eval(LAST_ROW);
    assert!(
        focused["facts"].as_str().unwrap().ends_with(host)
            && focused["whole"] == json!(true)
            && focused["box"] == json!(240),
        "{focused}"
    );
    // Every step shown, the words the row grows by take room of their own.
    tap(&mut chrome, ".tl-more");
    let shown_all = chrome.eval(BOX_HEIGHT);
    let row = ".tl-list > li.tl-sub:not(.tl-event)";
    tap(&mut chrome, row);
    chrome.frames();
    let grown = chrome.eval(BOX_HEIGHT);
    let words = chrome.eval(&format!(
        "(() => {{ const facts = document.querySelector({}).querySelector('.tl-facts'); return [facts.textContent, facts.getBoundingClientRect().height]; }})()",
        json!(row)
    ));
    assert!(
        words[0].as_str().unwrap().ends_with(host)
            && grown.as_f64().unwrap() >= shown_all.as_f64().unwrap() + words[1].as_f64().unwrap(),
        "the box was {shown_all} px and is {grown} px for {words}"
    );
    assert_eq!(chrome.eval(EVERY_STEP_SHOWN), json!(true));
}

/// The pairs of runs in one row of a runs lane drawn over each other by more than a bar's least
/// width (2 px) makes them: none, as a run takes the first row free when it is queued, so runs in
/// one row never overlap in time. Runs seconds apart may share a pixel at a timeline's scale.
const MERGED: &str = "(() => { const met = []; for (const lane of document.querySelectorAll('.tl-lane[data-lane=builds], .tl-lane[data-lane=tests]')) { const parts = [...lane.querySelectorAll('.tl-seg')].map(e => [e.title, e.getBoundingClientRect()]); for (let i = 0; i < parts.length; i++) for (let j = i + 1; j < parts.length; j++) { const [a, r] = parts[i], [b, s] = parts[j]; if (a !== b && r.top < s.bottom && s.top < r.bottom && Math.min(r.right, s.right) - Math.max(r.left, s.left) > 2.5) met.push([a, b]); } } return met; })()";

/// Four tests at once, and one queued half a minute after another's end, each drawn as a bar of its
/// own in the tests' lane: a row each where they overlap, as many rows as they need, and the one
/// after another's end in that one's row.
#[test]
fn runs_that_overlap_take_a_row_each_and_runs_one_after_another_share_one() {
    let mut timeline = fixture();
    let made = timeline["created"].as_f64().unwrap();
    let run = |phase: &str, label: &str, from: f64, to: f64| json!({"phase": phase, "label": label, "by": "quiet-wren", "commit": "9ac5149", "host": "studio", "queued": made + from * 60.0, "started": made + from * 60.0 + 30.0, "ended": made + to * 60.0, "result": "green"});
    timeline["runs"] = json!([
        run("build", "just lint-rs", 60.0, 200.0),
        run("test", "just test-rs -p a", 60.0, 200.0),
        run("test", "just test-rs -p b", 70.0, 190.0),
        run("test", "just test-rs -p c", 80.0, 180.0),
        run("test", "just test-rs -p d", 90.0, 170.0),
        // Half a minute after c's end: in c's row, free again.
        run("test", "just test-rs -p e", 180.5, 250.0),
        run("test", "just verify", 260.0, 300.0),
    ]);
    let site = served(&timeline);
    let Some(mut chrome) = chrome() else { return };
    open(&mut chrome, &site, 1280, 16);
    assert_eq!(
        chrome.eval("[...document.querySelectorAll('.tl-lane[data-lane=builds], .tl-lane[data-lane=tests]')].map(lane => [lane.dataset.lane, lane.style.getPropertyValue('--rows'), lane.querySelectorAll('[role=img]').length])"),
        json!([["builds", "1", 1], ["tests", "4", 6]])
    );
    assert_eq!(
        chrome.eval("[...document.querySelectorAll('.tl-lane[data-lane=tests] .tl-seg[role=img]')].map(e => e.style.getPropertyValue('--row'))"),
        json!(["0", "1", "2", "3", "2", "0"])
    );
    for width in [900, 1280, 1600] {
        view(&mut chrome, width, false);
        assert_eq!(chrome.eval(MERGED), json!([]), "at {width} px");
        assert_eq!(chrome.eval(EVERY_STEP_SHOWN), json!(true), "at {width} px");
    }
    // A burst of forty tests a minute apart on a timeline of a week: each after the one before it,
    // all on one row, every run still a bar of its own and a part the keyboard stops at, each too
    // narrow for words inside it.
    let mut timeline = week();
    let now = timeline["now"].as_f64().unwrap();
    let burst: Vec<Value> = (0..40)
        .map(|at| {
            let queued = now - 7200.0 + f64::from(at) * 60.0;
            json!({"phase": "test", "label": format!("just test-rs -p p{at}"), "by": "quiet-wren", "commit": "9ac5149", "host": "studio", "queued": queued, "started": queued + 1.0, "ended": queued + 30.0, "result": "green"})
        })
        .collect();
    timeline["runs"] = json!(burst);
    let site = served(&timeline);
    open(&mut chrome, &site, 1280, 90);
    assert_eq!(
        chrome.eval("(() => { const lane = document.querySelector('.tl-lane[data-lane=tests]'); return [lane.style.getPropertyValue('--rows'), lane.querySelectorAll('[role=img]').length, [...lane.querySelectorAll('.tl-fit')].filter(e => e.getClientRects().length > 0).length, Math.round(lane.querySelector('.tl-track').getBoundingClientRect().height)]; })()"),
        json!(["1", 40, 0, 36])
    );
    for width in [900, 1280, 1600] {
        view(&mut chrome, width, false);
        assert_eq!(chrome.eval(MERGED), json!([]), "at {width} px");
    }
}

/// The words drawn inside the runs' bars, and whether each is in a bar at least 30 px wide that
/// cuts it with an ellipsis.
const RUN_WORDS: &str = "(() => { const shown = [...document.querySelectorAll('.tl-lane[data-lane=builds] .tl-seg .tl-fit, .tl-lane[data-lane=tests] .tl-seg .tl-fit')].filter(e => e.getClientRects().length > 0); return {shown: shown.map(e => e.textContent), held: shown.every(e => { const bar = e.parentElement; return bar.getBoundingClientRect().width >= 29.5 && getComputedStyle(bar).textOverflow === 'ellipsis' && getComputedStyle(bar).overflow === 'hidden'; })}; })()";

/// Each runs lane: its key, its rows, its name's counts and its track's height.
const RUN_LANES: &str = "[...document.querySelectorAll('.tl-lane[data-lane=builds], .tl-lane[data-lane=tests]')].map(lane => [lane.dataset.lane, lane.style.getPropertyValue('--rows'), lane.querySelector('.tl-name small').textContent, Math.round(lane.querySelector('.tl-track').getBoundingClientRect().height)])";

/// The card's example task on a desk: its 177 builds and tests on one row of builds and two of
/// tests, each lane's name saying its counts, every run a bar of its own at its time; the time
/// no run is kept of hatched over both lanes up to the first kept, saying so; and nothing
/// scrolls sideways.
#[test]
fn the_cards_task_draws_its_runs_on_one_or_two_rows_and_hatches_the_time_not_kept() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    open_drawn(&mut chrome, &site, 1440);
    assert_eq!(
        chrome.eval(RUN_LANES),
        json!([
            ["builds", "1", "42 runs · 12 red", 36],
            ["tests", "2", "135 runs · 34 red", 56]
        ])
    );
    assert_eq!(chrome.eval(MERGED), json!([]));
    // Each run's bar is at its time: its left the share of the track its start is of the hours.
    let off = chrome.eval("(() => { const lanes = [...document.querySelectorAll('.tl-lane[data-lane=builds] .tl-seg, .tl-lane[data-lane=tests] .tl-seg')]; return Math.max(...lanes.map(e => { const t = e.parentElement.getBoundingClientRect(); return Math.abs(e.getBoundingClientRect().left - (t.left + parseFloat(e.style.left) / 100 * t.width)); })); })()");
    assert!(
        off.as_f64().unwrap() < 0.5,
        "a bar is {off} px off its time"
    );
    // The hatch: both lanes, from the task's making to the first run kept, which no bar is before;
    // its words whole, no sentence over the lanes.
    let hatch = chrome.eval("(() => { const hatches = [...document.querySelectorAll('.tl-notkept')]; const first = Math.min(...[...document.querySelectorAll('.tl-lane[data-lane=builds] .tl-seg, .tl-lane[data-lane=tests] .tl-seg')].map(e => e.getBoundingClientRect().left)); return {count: hatches.length, lanes: hatches.map(h => h.closest('.tl-lane').dataset.lane), words: hatches.map(h => [...h.querySelectorAll('.tl-fit')].filter(e => e.getClientRects().length > 0).map(e => e.textContent)), end: Math.round(Math.max(...hatches.map(h => h.getBoundingClientRect().right)) - first), title: hatches[0].title}; })()");
    assert_eq!(
        hatch,
        json!({"count": 2, "lanes": ["builds", "tests"], "words": [["not kept before 11-19 11:09"], ["not kept before 11-19 11:09"]], "end": 0, "title": "Builds and tests before 11-19 11:09 are not kept"})
    );
    assert_eq!(chrome.eval(CUT), json!([]));
    assert_eq!(
        chrome.eval("[...document.querySelectorAll('.tl-head .tl-note')].map(e => e.textContent)"),
        json!([])
    );
    // No run of 27 hours' worth is 30 px wide at 1440: no words in any bar.
    assert_eq!(chrome.eval(RUN_WORDS)["shown"], json!([]));
    assert_eq!(
        chrome.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
}

/// Where each message's tick is against its lane: its x off its time, its middle off its lane's
/// line, how far its drawn tick's top is below its lane's lowest bar, its press area, and the
/// board's and others' lane's height.
const TICKS: &str = "(() => { const marks = [...document.querySelectorAll('.tl-message-mark')]; const worst = (f) => Math.max(...marks.map(f)); return {count: marks.length, time: worst(m => { const t = m.parentElement.getBoundingClientRect(), b = m.getBoundingClientRect(); return Math.abs(b.left + b.width / 2 - (t.left + parseFloat(m.style.left) / 100 * t.width)); }), line: worst(m => { const r = m.parentElement.querySelector('.tl-rail').getBoundingClientRect(), b = m.getBoundingClientRect(); return Math.abs(b.top + b.height / 2 - (r.top + r.height / 2)); }), over: worst(m => { const b = m.getBoundingClientRect(), tick = b.top + b.height / 2 - 4; return Math.max(0, ...[...m.parentElement.querySelectorAll('.tl-seg')].map(s => s.getBoundingClientRect().bottom - tick)); }), press: Math.min(...marks.map(m => Math.min(m.getBoundingClientRect().width, m.getBoundingClientRect().height))), board: Math.round(document.querySelector('.tl-lane[data-lane=other] .tl-track').getBoundingClientRect().height), lines: [...document.querySelectorAll('.tl-lane[data-rail]')].map(l => l.dataset.lane)}; })()";

/// The card's example task's messages on a desk: each a tick on its sender's lane's dotted line,
/// at its time and under the bars, in a press area of its own of 24 px; the board's and others'
/// lane a 22 px line alone; and each hour's line where its label says it, on the tracks' own
/// start.
#[test]
fn messages_are_ticks_under_the_bars_and_the_hour_lines_meet_their_labels() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    for width in [1440, 960] {
        open_drawn(&mut chrome, &site, width);
        let ticks = chrome.eval(TICKS);
        assert_eq!(
            [&ticks["count"], &ticks["board"], &ticks["lines"]],
            [
                &json!(60),
                &json!(22),
                &json!(["orchestrator", "executor", "reviewer", "other"])
            ],
            "{width}: {ticks}"
        );
        for (what, most) in [("time", 0.5), ("line", 0.75), ("over", 0.0)] {
            assert!(
                ticks[what].as_f64().unwrap() <= most,
                "{width}: a tick's {what} is off by {}: {ticks}",
                ticks[what]
            );
        }
        assert!(ticks["press"].as_f64().unwrap() >= 24.0, "{width}: {ticks}");
        // Each hour's line drawn is where its label's middle is, a line shown where its label is
        // (one that gives way to a band's words, neither), and the layers start where the tracks
        // do.
        let hours = chrome.eval("(() => { const seen = e => e.getClientRects().length > 0, lines = [...document.querySelectorAll('.tl-hours .tl-hour')].filter(seen), labels = [...document.querySelectorAll('.tl-axis .tl-tick:not(.tl-nowtag)')].filter(seen); const track = document.querySelector('.tl-lane[data-lane=stage] .tl-track').getBoundingClientRect(); return {count: [lines.length, labels.length], off: Math.max(...lines.map((l, at) => { const b = labels[at].getBoundingClientRect(); return Math.abs(l.getBoundingClientRect().left - (b.left + b.width / 2)); })), hours: document.querySelector('.tl-hours').getBoundingClientRect().left - track.left, arrows: document.querySelector('.tl-address-track')?.getBoundingClientRect().left - track.left}; })()");
        let lines = hours["count"][0].as_u64().unwrap();
        assert!(
            lines > 3 && hours["count"][1] == json!(lines),
            "{width}: {hours}"
        );
        assert!(hours["off"].as_f64().unwrap() < 0.5, "{width}: {hours}");
        assert!(
            hours["hours"].as_f64().unwrap().abs() < 0.5,
            "{width}: {hours}"
        );
    }
}

/// Where the selected message's marks are: its tick, the arrow's line and ends, the ring at its
/// end and its recipient's line, the guide, the hours line's foot and its words there.
fn selected_marks(chrome: &mut super::chrome::Chrome, id: i64) -> Value {
    chrome.eval(&format!(r#"(() => {{
        const mid = b => b ? [b.left + b.width / 2, b.top + b.height / 2] : null;
        const mark = document.querySelector('.tl-message-mark[data-message="{id}"]');
        const arrow = document.querySelector('.tl-address-arrow');
        const a = arrow?.getBoundingClientRect();
        const to = arrow ? document.querySelector(`[data-lane="${{arrow.dataset.to}}"] .tl-rail`).getBoundingClientRect() : null;
        const guide = document.querySelector('.tl-address-guide')?.getBoundingClientRect();
        const said = document.querySelector('.tl-address-said');
        const axis = document.querySelector('.tl-axis .tl-track').getBoundingClientRect();
        const s = said?.getBoundingClientRect();
        return {{
            pressed: mark.getAttribute('aria-pressed'), tick: mid(mark.getBoundingClientRect()),
            ends: arrow ? [arrow.dataset.from, arrow.dataset.to] : null,
            line: a ? [a.left + a.width / 2, a.top, a.bottom] : null,
            recipient: to ? to.top + to.height / 2 : null,
            ring: mid(document.querySelector('.tl-address-ring')?.getBoundingClientRect()),
            around: document.querySelector('.tl-address-ring')?.classList.contains('tl-ring-around') ?? null,
            guide: guide ? [guide.left, guide.top] : null, foot: document.querySelector('.tl-axis').getBoundingClientRect().bottom,
            said: said?.textContent ?? null, inside: s ? s.left >= axis.left - 0.5 && s.right <= axis.right + 0.5 : null,
        }};
    }})()"#))
}

/// A selected message of the card's example task: a filled dot on its tick and one arrow straight
/// to a ring on its addressed lane's line at its instant, its address and time on the hours line
/// with a dashed guide down to it; one whose two ends are one lane a ring around its tick and no
/// arrow, the hours line naming both ends.
#[test]
fn a_selected_tick_points_at_a_ring_on_its_recipients_line_and_says_its_address_on_the_hours() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    open_drawn(&mut chrome, &site, 1440);
    // The reviewer's verdict to the executor, picked from the chooser: up to the executor's line.
    messages::pick(&mut chrome, 15396);
    let marks = selected_marks(&mut chrome, 15396);
    assert_eq!(
        [
            &marks["pressed"],
            &marks["ends"],
            &marks["around"],
            &marks["said"],
            &marks["inside"]
        ],
        [
            &json!("true"),
            &json!(["reviewer", "executor"]),
            &json!(false),
            &json!("#15396 · gilded-gecko-838 → @executor · 11:31 · verdict"),
            &json!(true)
        ],
        "{marks}"
    );
    let f = |path: &[usize]| {
        let mut at = &marks[["tick", "line", "ring"][path[0]]];
        for index in &path[1..] {
            at = &at[*index];
        }
        at.as_f64().unwrap()
    };
    let near = |a: f64, b: f64, what: &str| {
        assert!((a - b).abs() < 0.75, "{what}: {a} against {b}: {marks}")
    };
    let recipient = marks["recipient"].as_f64().unwrap();
    near(f(&[1, 0]), f(&[0, 0]), "the arrow's line through the tick");
    near(f(&[1, 2]), f(&[0, 1]), "the arrow from the tick, up");
    near(f(&[1, 1]), recipient, "the arrow to the recipient's line");
    near(f(&[2, 0]), f(&[0, 0]), "the ring at the tick's instant");
    near(f(&[2, 1]), recipient, "the ring on the recipient's line");
    near(
        marks["guide"][0].as_f64().unwrap(),
        f(&[0, 0]),
        "the guide over the tick",
    );
    near(
        marks["guide"][1].as_f64().unwrap(),
        marks["foot"].as_f64().unwrap(),
        "the guide from the hours' foot",
    );
    // A tap on another tick picks it: the executor's to the orchestrator, by its press area.
    tap(&mut chrome, ".tl-message-mark[data-message='17040']");
    assert!(chrome.wait_for("document.querySelector('.tl-message-details h3')?.textContent.startsWith('Message #17040 ')", SHOWN));
    assert_eq!(
        selected_marks(&mut chrome, 17040)["ends"],
        json!(["executor", "reviewer"])
    );
    // A verdict to everyone after the task closed: its two ends are the board's and others' lane.
    messages::pick(&mut chrome, 17313);
    let ringed = selected_marks(&mut chrome, 17313);
    assert_eq!(
        [&ringed["ends"], &ringed["around"], &ringed["said"]],
        [
            &Value::Null,
            &json!(true),
            &json!("#17313 · coral-badger-11 → Everyone on task · 04:34 · verdict")
        ],
        "{ringed}"
    );
    let tick = &ringed["tick"];
    for at in 0..2 {
        assert!(
            (ringed["ring"][at].as_f64().unwrap() - tick[at].as_f64().unwrap()).abs() < 0.75,
            "{ringed}"
        );
    }
    assert!(
        ringed["guide"][1].as_f64().unwrap() - ringed["foot"].as_f64().unwrap() < 0.75,
        "{ringed}"
    );
    assert_eq!(
        chrome.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
}

/// Each message row of the phone's list: its height, how far its time's, its diamond's and its
/// words' middles are apart, and whether a press on its time reaches its button.
const MESSAGE_ROWS: &str = "(() => { const rows = [...document.querySelectorAll('.tl-list .tl-message')]; const mid = e => { const b = e.getBoundingClientRect(); return b.top + b.height / 2; }; return {count: rows.length, heights: [...new Set(rows.map(r => Math.round(r.getBoundingClientRect().height * 2) / 2))], apart: Math.max(...rows.map(r => { const m = [r.querySelector('.tl-at'), r.querySelector('.tl-mark'), r.querySelector('.tl-message-row')].map(mid); return Math.max(...m) - Math.min(...m); })), pressed: rows.every(r => { r.scrollIntoView({block: 'center'}); const b = r.querySelector('.tl-at').getBoundingClientRect(); return document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2)?.closest('.tl-message-row') === r.querySelector('.tl-message-row'); })}; })()";

/// The card's example task on a phone: a message is one line of 28 px, its time, its diamond and
/// its words level, the whole row its press area; the runs kept from 11-19 11:09 is a row at
/// that time before the first run, in place of a sentence; and nothing scrolls sideways.
#[test]
fn on_a_phone_a_message_is_one_line_and_the_runs_kept_from_a_time_is_a_row() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    open_drawn(&mut chrome, &site, 390);
    tap(&mut chrome, ".tl-more");
    let rows = chrome.eval(MESSAGE_ROWS);
    assert_eq!(
        [&rows["count"], &rows["heights"], &rows["pressed"]],
        [&json!(60), &json!([28]), &json!(true)],
        "{rows}"
    );
    assert!(rows["apart"].as_f64().unwrap() <= 1.0, "{rows}");
    assert_eq!(
        chrome.eval("(() => { const items = [...document.querySelectorAll('.tl-list > .tl-row')]; const kept = items.findIndex(r => r.classList.contains('tl-kept')); const run = items.findIndex(r => r.classList.contains('tl-sub') && !r.classList.contains('tl-event')); return [items[kept]?.querySelector('.tl-at').textContent, items[kept]?.querySelector('.tl-label').textContent, kept < run, document.querySelectorAll('.tl-kept').length, [...document.querySelectorAll('.tl-head .tl-note')].length]; })()"),
        json!(["11:09", "Builds and tests kept from here on", true, 1, 0])
    );
    // A press on a row's time, where the row's button reaches, chooses its message.
    let at = chrome.eval("(() => { const e = document.querySelector('.tl-message-row[data-message=\"15266\"]').closest('li').querySelector('.tl-at'); e.scrollIntoView({block: 'center'}); const b = e.getBoundingClientRect(); return {x: b.left + b.width / 2, y: b.top + b.height / 2}; })()");
    for kind in ["mousePressed", "mouseReleased"] {
        chrome.call(
            "Input.dispatchMouseEvent",
            json!({"type": kind, "x": at["x"], "y": at["y"], "button": "left", "clickCount": 1}),
        );
    }
    assert!(chrome.wait_for("document.querySelector('.tl-message-details h3')?.textContent.startsWith('Message #15266 ')", SHOWN));
    assert_eq!(
        chrome.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
}

/// The pairs of words drawn in one track of the lanes whose boxes meet, a run's words measured as
/// its bar cuts them: none, where every word is
/// whole in its own box ([`CUT`]).
const OVERPRINT: &str = "(() => { const met = []; const drawn = e => { const r = e.getBoundingClientRect(), bar = e.parentElement.closest('[data-lane=builds] .tl-seg, [data-lane=tests] .tl-seg'); if (!bar) return r; const b = bar.getBoundingClientRect(); return {left: Math.max(r.left, b.left), right: Math.min(r.right, b.right), top: Math.max(r.top, b.top), bottom: Math.min(r.bottom, b.bottom)}; }; for (const track of document.querySelectorAll('.tl-lanes .tl-track')) { const words = [...track.querySelectorAll('.tl-fit, .tl-tick')].filter(e => e.getClientRects().length > 0 && e.textContent.trim() !== '').map(e => [e.textContent.trim(), drawn(e)]); for (let i = 0; i < words.length; i++) for (let j = i + 1; j < words.length; j++) { const [a, r] = words[i], [b, s] = words[j]; if (r.left < s.right - 0.5 && s.left < r.right - 0.5 && r.top < s.bottom - 0.5 && s.top < r.bottom - 0.5) met.push([a, b]); } } return met; })()";

/// The parts of the lanes drawn with words wider than they are, cut short: the words as drawn (a
/// Range over them, to the fraction of a pixel an ellipsis already takes) reaching past their box's
/// padding. A run's bar alone cuts its words to fit, and is not counted.
const CUT: &str = "[...document.querySelectorAll('.tl-lanes .tl-seg, .tl-lanes .tl-after, .tl-lanes .tl-g, .tl-lanes .tl-notkept .tl-fit')].filter(e => !e.closest('[data-lane=builds], [data-lane=tests]') || e.matches('.tl-notkept .tl-fit')).filter(e => e.getClientRects().length > 0 && e.innerText.trim() !== '').filter(e => { const words = document.createRange(); words.selectNodeContents(e); const w = words.getBoundingClientRect(), b = e.getBoundingClientRect(), s = getComputedStyle(e); return w.right > b.right - parseFloat(s.borderRightWidth) - parseFloat(s.paddingRight) + 0.01 || w.left < b.left + parseFloat(s.borderLeftWidth) + parseFloat(s.paddingLeft) - 0.01; }).map(e => e.innerText)";

/// The words of a segment or of a label after one that another segment of their track is drawn
/// over (a segment of a few minutes drawn at its least width reaching into the next), each with the
/// segment over it: none. A runs lane's bars cut their words to fit, and are not counted.
const COVERED: &str = "(() => { const met = []; for (const track of document.querySelectorAll('.tl-lanes .tl-track')) { if (track.closest('[data-lane=builds], [data-lane=tests]')) continue; const segs = [...track.querySelectorAll('.tl-seg')].filter(e => e.getClientRects().length > 0); for (const e of track.querySelectorAll('.tl-seg, .tl-after')) { if (e.getClientRects().length === 0 || e.innerText.trim() === '') continue; const words = document.createRange(); words.selectNodeContents(e); const w = words.getBoundingClientRect(); for (const seg of segs) { if (seg === e) continue; const b = seg.getBoundingClientRect(); if (b.left < w.right - 0.5 && w.left < b.right - 0.5 && b.top < w.bottom - 0.5 && w.top < b.bottom - 0.5) met.push([e.innerText.trim(), seg.title]); } } } return met; })()";

/// Each label after a bar that is drawn: its words, how far past its bar as drawn they start, and
/// how far before the next segment of its track they end (null where none follows them).
const AFTER: &str = "[...document.querySelectorAll('.tl-lanes .tl-after')].filter(e => e.getClientRects().length > 0).map(e => { const r = document.createRange(); r.selectNodeContents(e); const words = r.getBoundingClientRect(), bar = e.previousElementSibling.getBoundingClientRect(); const next = [...e.parentElement.querySelectorAll('.tl-seg')].map(s => s.getBoundingClientRect().left).filter(left => left >= words.left); return [e.textContent, words.left - bar.right, next.length === 0 ? null : Math.min(...next) - words.right]; })";

/// No word of the lanes is drawn over another, under another segment or cut short, at any width,
/// for the fixture, a week and the card's task. Each label after a bar starts its padding past the bar as drawn, which a
/// round of a few minutes draws wider than its time, and ends its gap (`GAP_PX`, timeline-layout.ts)
/// or more before the next segment, as its words are weighed at the widest face's advances
/// (`textPx`); a label with no room is not drawn, as the card's reviewer's "changes" is not, 4 % of
/// the track before the next round.
#[test]
fn no_word_of_the_lanes_is_drawn_over_another_at_any_width_for_the_fixture_or_a_week() {
    // The labels after a bar at each width: the fixture's and a week's at every one, the card's
    // task's as its quiet stretch skipped gives its work the width, each verdict from the width its
    // room holds it before the next round or the band (a 555 px track at 900 wide, 935 at 1280,
    // 1,095 at 1440, 1,158 at 1600): the first "changes" from a 950 px track, the second from 1,200,
    // wider than the page draws.
    let card = |width: u32| -> &'static [&'static str] {
        match width {
            ..1100 => &["approved"],
            1100..1440 => &["approved", "approved"],
            _ => &["approved", "changes", "approved"],
        }
    };
    let fixed = |labels: &'static [&'static str]| move |_: u32| labels;
    for (name, timeline, labels) in [
        (
            "the fixture",
            fixture(),
            Box::new(fixed(&["changes", "approved"]))
                as Box<dyn Fn(u32) -> &'static [&'static str]>,
        ),
        ("a week", week(), Box::new(fixed(&["changes"]))),
        ("the card's task", revision_guards(), Box::new(card)),
    ] {
        let site = if name == "the card's task" {
            guards_on(site())
        } else {
            served(&timeline)
        };
        let Some(mut chrome) = chrome() else { return };
        open_drawn(&mut chrome, &site, 1280);
        for width in [900, 960, 1100, 1280, 1440, 1600] {
            view(&mut chrome, width, false);
            assert_eq!(chrome.eval(OVERPRINT), json!([]), "{name} at {width} px");
            assert_eq!(
                chrome.eval(CUT),
                json!([]),
                "{name} at {width} px: a word cut short"
            );
            assert_eq!(
                chrome.eval(COVERED),
                json!([]),
                "{name} at {width} px: a word under another segment"
            );
            let after = chrome.eval(AFTER);
            let drawn: Vec<&str> = (after.as_array().unwrap().iter())
                .map(|label| label[0].as_str().unwrap())
                .collect();
            assert_eq!(
                drawn,
                labels(width),
                "{name} at {width} px: the labels after a bar"
            );
            for label in after.as_array().unwrap() {
                assert!(
                    label[1].as_f64().unwrap() >= 5.5,
                    "{name} at {width} px: a label's words start inside or at its bar: {label}"
                );
                assert!(
                    label[2].as_f64().is_none_or(|gap| gap >= 4.0),
                    "{name} at {width} px: a label's words within its gap of the next: {label}"
                );
            }
            assert_eq!(
                chrome.eval("document.documentElement.scrollWidth <= innerWidth"),
                json!(true),
                "{name} at {width} px scrolls sideways"
            );
        }
    }
}

#[test]
fn the_lanes_say_each_part_to_assistive_technology_and_the_keyboard_walks_them() {
    let site = served(&fixture());
    let Some(mut chrome) = chrome() else { return };
    open(&mut chrome, &site, 1280, 13);
    // The list stays for assistive technology, out of sight but not taken out.
    assert_eq!(
        chrome.eval("(() => { const list = getComputedStyle(document.querySelector('.tl-list')); return [list.display !== 'none', list.visibility, list.clipPath]; })()"),
        json!([true, "visible", "inset(50%)"])
    );
    // Each part of the lanes is said, or is part of one that is; the hours are not.
    assert_eq!(
        chrome.eval("[...document.querySelectorAll('.tl-lanes .tl-seg, .tl-lanes .tl-g')].filter(e => !(e.getAttribute('role') === 'img' && e.getAttribute('aria-label')) && e.getAttribute('aria-hidden') !== 'true').length"),
        json!(0)
    );
    assert_eq!(
        chrome.eval("document.querySelectorAll('.tl-lanes [tabindex=\"0\"]').length"),
        json!(1)
    );
    let stops = chrome
        .eval("document.querySelectorAll('.tl-lanes [role=img]').length")
        .as_u64()
        .unwrap();
    assert_eq!(stops, 24);
    chrome.eval("document.getElementById('tl-stop-0').focus()");
    assert_eq!(
        shown(&mut chrome, ".tl-detail"),
        ["ready 41 m, from 09:02 to 09:43"],
        "a part with the focus says all of it under the lanes"
    );
    components::key(&mut chrome, "ArrowRight", "ArrowRight", 39);
    assert_eq!(
        chrome.eval("[document.activeElement.id, document.activeElement.tabIndex, document.getElementById('tl-stop-0').tabIndex, document.querySelector('.tl-detail').textContent]"),
        json!(["tl-stop-1", 0, -1, "in progress 1 h 52 m, from 09:43 to 11:35"])
    );
    components::key(&mut chrome, "End", "End", 35);
    assert_eq!(
        chrome
            .eval("[document.activeElement.id, document.activeElement.getAttribute('aria-label')]"),
        json!([
            format!("tl-stop-{}", stops - 1),
            "review met from 14:05: Approved in round 2 of 2"
        ])
    );
    components::key(&mut chrome, "Home", "Home", 36);
    assert_eq!(chrome.eval("document.activeElement.id"), json!("tl-stop-0"));
    // The keys walked the lanes and reached no control of the page's.
    assert_eq!(
        chrome.eval("['quick-open', 'bell'].map(id => document.getElementById(id).getAttribute('aria-expanded'))"),
        json!(["false", "false"])
    );
    // A gate's state is told by its line's pattern as well as its colour.
    assert_eq!(
        chrome.eval("['met', 'missing', 'waived'].map(state => getComputedStyle(document.querySelector(`.tl-lanes .tl-g-${state}`), '::before').borderTopStyle)"),
        json!(["solid", "dotted", "dashed"])
    );
}

/// Open the task's page with its timeline's read held, and leave it held: once the box's
/// placeholder is drawn, where the page's parts stand ([`places`]). The timeline's chunk is asked
/// for once its part has something to draw, so none has come.
fn placeholder_drawn(chrome: &mut super::chrome::Chrome, site: &Site) -> Value {
    site.hold_route(symphony_api::TASK_TIMELINE_PATH, true);
    chrome.open(&site.url(TASK));
    assert!(
        chrome.wait_for(
            "document.querySelector('.task-body') !== null && document.querySelectorAll('#timeline-slot .tl-sk-row').length > 0 && document.getElementById('timeline-root') === null",
            SHOWN
        ),
        "the box's placeholder was not drawn: {:?}",
        chrome.eval("document.querySelector('article.task')?.innerText")
    );
    chrome.frames();
    places(chrome)
}

#[test]
fn on_a_phone_nothing_on_the_page_moves_when_the_timeline_arrives_or_a_read_adds_a_step() {
    let site = served(&timed(fixture()));
    let Some(mut chrome) = chrome() else { return };
    chrome.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": MINUTES}),
    );
    view(&mut chrome, 390, true);
    // Its read held: the box is drawn with its placeholder rows, as tall as it will be.
    let before = placeholder_drawn(&mut chrome, &site);
    assert_eq!(
        chrome.eval("[...document.querySelectorAll('#timeline-slot .tl-sk-row')].every(row => row.getClientRects().length > 0)"),
        json!(true),
        "the placeholder is the box's own rows, no one-line note"
    );
    site.hold_route(symphony_api::TASK_TIMELINE_PATH, false);
    assert!(chrome.wait_for("document.querySelectorAll('.tl-drawn .tl-list .tl-row').length === 13 && window.minute !== null", SHOWN));
    chrome.frames();
    assert_eq!(places(&mut chrome), before, "the timeline moved the page");
    // A minute's read that brings a step more adds it inside the box.
    let mut more = timed(fixture());
    more["runs"].as_array_mut().unwrap().push(json!({"phase":"build","label":"just lint-rs","by":"quiet-wren","commit":"5d6e7f8","host":"studio","queued":1794402600.0,"started":1794402660.0,"ended":1794402900.0,"result":"green"}));
    site.set_json(symphony_api::TASK_TIMELINE_PATH, &more);
    chrome.eval("window.minute()");
    assert!(chrome.wait_for(
        "document.querySelectorAll('.tl-drawn .tl-list .tl-row').length === 14",
        SHOWN
    ));
    chrome.frames();
    assert_eq!(places(&mut chrome), before, "a step more moved the page");
    // "Show all" lets the box grow, as the reader asked, and the brief goes below it: at most
    // 240 px tall, the box does not hold the phone's list whole.
    tap(&mut chrome, ".tl-more");
    assert_eq!(
        chrome.eval("document.querySelector('.tl-more').getAttribute('aria-expanded')"),
        json!("true")
    );
    assert_eq!(
        chrome.eval(LEFT_ALONE),
        json!(["false", true]),
        "Show all reached the page"
    );
    assert_eq!(
        chrome.eval(EVERY_STEP_SHOWN),
        json!(true),
        "every step shown"
    );
}

/// The box's height on the page, in whole pixels.
const BOX_HEIGHT: &str =
    "Math.round(document.getElementById('timeline-slot').getBoundingClientRect().height)";

/// What "Show all" says as drawn.
const MORE: &str = "document.querySelector('.tl-more')?.innerText.trim()";

/// Whether the part that holds the steps shows all of them, none cut off by the box.
const EVERY_STEP_SHOWN: &str = "(() => { const body = document.getElementById('tl-body'); return body.scrollHeight <= body.clientHeight + 1; })()";

#[test]
fn the_box_is_at_most_240_px_on_a_phone_and_show_all_opens_the_rest_in_place() {
    for (timeline, rows) in [(fixture(), 13), (week(), 50)] {
        let site = served(&timeline);
        let Some(mut chrome) = chrome() else { return };
        open(&mut chrome, &site, 390, rows);
        assert_eq!(chrome.eval(BOX_HEIGHT), json!(240), "{rows} steps");
        // The brief starts right under the box, not a screen further down.
        let gap = chrome.eval("document.querySelector('.task-body').getBoundingClientRect().top - document.getElementById('timeline-slot').getBoundingClientRect().bottom");
        assert!(
            (0.0..48.0).contains(&gap.as_f64().unwrap()),
            "{rows} steps: the brief starts {gap} px under the box"
        );
        // The button names what it shows: the list's steps.
        let more = format!("Show all {rows} steps");
        assert_eq!(chrome.eval(MORE), json!(more), "{rows} steps");
        // A person's click opens the rest in place, and only that: the "+" chat sheet stays
        // closed and the focus stays on the button, for "Show the latest" too.
        tap(&mut chrome, ".tl-more");
        assert!(
            chrome.eval(BOX_HEIGHT).as_f64().unwrap() > 240.0,
            "{rows} steps: Show all left the box {}",
            chrome.eval(BOX_HEIGHT)
        );
        assert_eq!(
            chrome.eval(EVERY_STEP_SHOWN),
            json!(true),
            "{rows} steps: every step shown"
        );
        assert_eq!(
            chrome.eval(LEFT_ALONE),
            json!(["false", true]),
            "{rows} steps: Show all reached the page"
        );
        tap(&mut chrome, ".tl-more");
        assert_eq!(
            (chrome.eval(BOX_HEIGHT), chrome.eval(MORE)),
            (json!(240), json!(more)),
            "{rows} steps: Show the latest"
        );
        assert_eq!(
            chrome.eval(LEFT_ALONE),
            json!(["false", true]),
            "{rows} steps: Show the latest reached the page"
        );
    }
}

/// What each module's root heard (`heard`, by the root's id, and `document`): the kind of each
/// event and the part of the page it came from. The task page's module draws its timeline in the
/// root it has inside its box (`timeline-root`).
const SPIES: &str = "(() => { const from = node => node.closest('#timeline-root') ? 'timeline' : node.closest('#quick-root') ? 'panel' : 'page'; window.heard = {}; for (const id of ['review-root-task', 'timeline-root', 'quick-root', 'document']) { const at = id === 'document' ? document : document.getElementById(id); heard[id] = []; for (const type of ['click', 'keydown']) at.addEventListener(type, event => heard[id].push(`${type} ${from(event.target)}`)); } return true; })()";

#[test]
fn a_modules_root_hears_only_its_own_parts_events_and_the_document_hears_them_all() {
    let site = served(&fixture());
    let Some(mut chrome) = chrome() else { return };
    open(&mut chrome, &site, 1280, 13);
    // A third module beside the page's and the timeline's: the chat's, loaded and its sheet open.
    chrome.eval("document.getElementById('quick-open').click()");
    assert!(chrome.wait_for(
        "window.reviewPanelReady === true && document.querySelector('#quick-root #quick-text') !== null",
        SHOWN
    ));
    // Listeners on the roots, as each module's framework listens, and one on the document. A
    // click and a key in the timeline, the task page module's own part inside its root, a click on
    // the page's own heading and one in the chat's sheet, the chat's module beside it.
    assert_eq!(chrome.eval(SPIES), json!(true));
    chrome.eval(
        "(() => { const click = selector => document.querySelector(selector).dispatchEvent(new MouseEvent('click', {bubbles: true, cancelable: true})); click('article.task h1'); click('#timeline-root .tl-totals'); document.getElementById('tl-stop-0').dispatchEvent(new KeyboardEvent('keydown', {key: 'Home', bubbles: true, cancelable: true})); click('#quick-root #quick-text'); })()",
    );
    assert_eq!(
        chrome.eval("heard"),
        json!({
            "review-root-task": ["click page", "click timeline", "keydown timeline"],
            "timeline-root": ["click timeline", "keydown timeline"],
            "quick-root": ["click panel"],
            "document": ["click page", "click timeline", "keydown timeline", "click panel"],
        })
    );
    // Taken off, a listener on a root hears nothing more.
    chrome.eval("(() => { window.off = () => heard.off.push('click'); heard.off = []; const root = document.getElementById('review-root-task'); root.addEventListener('click', off); root.removeEventListener('click', off); document.querySelector('article.task h1').click(); })()");
    assert_eq!(chrome.eval("heard.off"), json!([]));
}

#[test]
fn a_box_is_reserved_as_tall_as_this_browser_last_drew_the_tasks_timeline() {
    let mut short = fixture();
    let first = short["stages"][0].clone();
    short["stages"] = json!([first]);
    short["stages"][0]["to"] = Value::Null;
    for empty in ["runs", "gates", "claims", "rounds"] {
        short[empty] = json!([]);
    }
    let site = served(&short);
    let Some(mut chrome) = chrome() else { return };
    open(&mut chrome, &site, 390, 1);
    // The first visit reserves the default; the next, the height the one step took.
    let first =
        chrome.eval("document.getElementById('timeline-slot').getBoundingClientRect().height");
    site.hold_route(symphony_api::TASK_TIMELINE_PATH, true);
    chrome.open(&site.url(TASK));
    assert!(chrome.wait_for(
        "document.querySelectorAll('#timeline-slot .tl-sk-row').length > 0",
        SHOWN
    ));
    let reserved =
        chrome.eval("document.getElementById('timeline-slot').getBoundingClientRect().height");
    assert!(
        reserved.as_f64().unwrap() < 300.0 && reserved.as_f64().unwrap() < first.as_f64().unwrap(),
        "a one-step timeline reserves {reserved} px again, {first} the first time"
    );
    site.hold_route(symphony_api::TASK_TIMELINE_PATH, false);
    assert!(chrome.wait_for(
        "document.querySelectorAll('.tl-drawn .tl-row').length === 1",
        SHOWN
    ));
    assert_eq!(
        chrome.eval("document.getElementById('timeline-slot').getBoundingClientRect().height"),
        reserved,
        "the box keeps its height when the timeline arrives"
    );
}

/// Wide, the box holds every lane with no "Show all", taller than a phone's most, the brief right
/// under it, and the next visit reserves it as tall: nothing on the page moves when the timeline
/// arrives. A click on a part of the last lane focuses it, as the keyboard does: its detail shows
/// in the legend's place, and nothing on the page moves.
#[test]
fn a_wide_box_holds_every_lane_with_no_show_all_and_the_next_visit_reserves_it_as_tall() {
    for (timeline, rows) in [(fixture(), 13), (week(), 50)] {
        let site = served(&timeline);
        let Some(mut chrome) = chrome() else { return };
        open(&mut chrome, &site, 1280, rows);
        let drawn = chrome.eval(BOX_HEIGHT);
        assert!(
            drawn.as_f64().unwrap() > 240.0,
            "{rows} steps: the box is {drawn} px"
        );
        assert_eq!(
            chrome.eval(EVERY_STEP_SHOWN),
            json!(true),
            "{rows} steps: a lane cut off"
        );
        assert_eq!(
            chrome.eval("document.querySelector('.tl-more')"),
            Value::Null,
            "{rows} steps: Show all offered"
        );
        let gap = chrome.eval("document.querySelector('.task-body').getBoundingClientRect().top - document.getElementById('timeline-slot').getBoundingClientRect().bottom");
        assert!(
            (0.0..48.0).contains(&gap.as_f64().unwrap()),
            "{rows} steps: the brief starts {gap} px under the box"
        );
        // The next visit reserves the height drawn in whole pixels, rounded up.
        let whole = chrome.eval(
            "Math.ceil(document.getElementById('timeline-slot').getBoundingClientRect().height)",
        );
        let before = placeholder_drawn(&mut chrome, &site);
        assert_eq!(before["box"][1], whole, "{rows} steps: the next visit");
        site.hold_route(symphony_api::TASK_TIMELINE_PATH, false);
        let arrived =
            format!("document.querySelectorAll('.tl-drawn .tl-list .tl-row').length === {rows}");
        assert!(chrome.wait_for(&arrived, SHOWN), "{rows} steps: not drawn");
        chrome.frames();
        // The height is kept in whole pixels, the timeline's own a fraction under it: what is under
        // the box may move by less than one.
        let after = places(&mut chrome);
        let height =
            chrome.eval("document.getElementById('timeline-slot').getBoundingClientRect().height");
        assert!(
            ["brief", "thread"].iter().all(|part| {
                (after[part].as_f64().unwrap() - before[part].as_f64().unwrap()).abs() < 1.0
            }) && after["box"][0] == before["box"][0]
                && (height.as_f64().unwrap() - before["box"][1].as_f64().unwrap()).abs() < 1.0,
            "{rows} steps: the timeline moved the page: {before} then {after}, {height} px"
        );
        chrome.eval("document.getElementById('timeline').scrollIntoView()");
        chrome.frames();
        let at_rest = (places(&mut chrome), chrome.eval(FOOT));
        let part = chrome.eval(LAST_PART);
        assert_eq!(
            part["hit"],
            json!(true),
            "{rows} steps: the part is covered: {part}"
        );
        for kind in ["mousePressed", "mouseReleased"] {
            chrome.call(
                "Input.dispatchMouseEvent",
                json!({"type": kind, "x": part["x"], "y": part["y"], "button": "left", "clickCount": 1}),
            );
        }
        chrome.frames();
        assert_eq!(
            chrome.eval("document.activeElement?.closest('.tl-lane')?.dataset.lane ?? null"),
            part["lane"],
            "{rows} steps: the click focused the part"
        );
        assert_eq!(
            shown(&mut chrome, ".tl-detail").len(),
            1,
            "its detail shows"
        );
        assert!(
            shown(&mut chrome, ".tl-legend li").is_empty(),
            "{rows} steps: the detail takes the legend's place"
        );
        assert_eq!(
            (places(&mut chrome), chrome.eval(FOOT)),
            at_rest,
            "{rows} steps: the focus moved the page or the foot"
        );
    }
}

/// The height of the box's foot, in whole pixels.
const FOOT: &str = "Math.round(document.querySelector('.tl-foot').getBoundingClientRect().height)";

/// The widest part of the last lane that has parts: its lane, the point a click takes, and whether
/// the part is on top there.
const LAST_PART: &str = "(() => { const lane = [...document.querySelectorAll('.tl-lanes > .tl-lane[data-lane]')].filter(lane => lane.querySelector('[role=img]')).pop(); const part = [...lane.querySelectorAll('[role=img]')].sort((a, b) => b.getBoundingClientRect().width - a.getBoundingClientRect().width)[0]; const b = part.getBoundingClientRect(); const x = b.left + b.width / 2, y = b.top + b.height / 2; return {x, y, lane: lane.dataset.lane, hit: document.elementFromPoint(x, y)?.closest('[role=img]') === part}; })()";

#[test]
fn the_plus_button_covers_nothing_the_timeline_says_at_a_phones_width() {
    for (timeline, rows) in [(fixture(), 13), (week(), 50)] {
        let site = served(&timeline);
        let Some(mut chrome) = chrome() else { return };
        open(&mut chrome, &site, 390, rows);
        chrome.eval("document.querySelector('.tl-more')?.click()");
        chrome.frames();
        // Whatever the page's scroll, the button's column holds none of the timeline's words.
        assert_eq!(
            chrome.eval("(() => { const button = document.getElementById('quick-open').getBoundingClientRect(); return [...document.querySelectorAll('#timeline-slot .tl-at, #timeline-slot .tl-label, #timeline-slot .tl-took, #timeline-slot .tl-word, #timeline-slot .tl-totals span, #timeline-slot .tl-legend li, #timeline-slot .tl-more, #timeline-slot .tl-day')].filter(e => e.getClientRects().length > 0 && e.getBoundingClientRect().right > button.left).map(e => e.textContent); })()"),
            json!([]),
            "{rows} steps"
        );
    }
}

#[test]
fn the_timeline_reads_each_minute_only_while_shown_and_open_and_stops_when_left() {
    let site = served(&timed(fixture()));
    let Some(mut chrome) = chrome() else { return };
    chrome.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": MINUTES}),
    );
    open(&mut chrome, &site, 1280, 13);
    assert!(chrome.wait_for("window.minuteTimers === 1", SHOWN));
    assert_eq!(reads(&site), 1);
    // A minute on, the same timeline: asked by its tag, answered 304, nothing redrawn.
    chrome.eval("document.querySelector('.tl-list .tl-row').dataset.kept = 'yes'");
    chrome.eval("window.minute()");
    assert!(chrome.wait_for("window.minuteTimers === 2", SHOWN));
    assert_eq!(reads(&site), 2);
    assert!(
        chrome.requests().iter().any(|request| request
            .url
            .contains(symphony_api::TASK_TIMELINE_PATH)
            && request.status == 304),
        "the minute's read was not answered 304: {:#?}",
        chrome.requests()
    );
    assert_eq!(
        chrome.eval("document.querySelector('.tl-list .tl-row').dataset.kept"),
        json!("yes")
    );

    // Hidden, its minute passes without a read; shown again, one read, and the minutes go on.
    chrome.eval("Object.defineProperty(document, 'hidden', {configurable: true, get: () => true}); document.dispatchEvent(new Event('visibilitychange'))");
    chrome.eval("window.minute()");
    chrome.frames();
    chrome.eval("new Promise(resolve => setTimeout(resolve, 300))");
    assert_eq!(
        (reads(&site), chrome.eval("window.minuteTimers")),
        (2, json!(2))
    );
    chrome.eval("Object.defineProperty(document, 'hidden', {configurable: true, get: () => false}); document.dispatchEvent(new Event('visibilitychange'))");
    assert!(chrome.wait_for("window.minuteTimers === 3", SHOWN));
    assert_eq!(reads(&site), 3);

    // A read that fails leaves the last answer shown, and the minutes go on.
    site.fail_route(symphony_api::TASK_TIMELINE_PATH, true);
    chrome.eval("window.minute()");
    assert!(chrome.wait_for("window.minuteTimers === 4", SHOWN));
    assert_eq!(reads(&site), 4);
    assert_eq!(
        chrome.eval("[document.querySelectorAll('.tl-drawn .tl-list .tl-row').length, document.querySelector('.tl-failed')]"),
        json!([13, null])
    );
    site.fail_route(symphony_api::TASK_TIMELINE_PATH, false);
    // The read that failed is tried again by itself, the minute's wait left running.
    wait_reads(&mut chrome, &site, "visual%2Fin-progress", 5);

    // The task changes: its timeline is read again at once, the minute's wait still running.
    let mut page = look_parity::responses()["/api/task-page"].clone();
    page["task"]["updated"] = json!(2.0);
    site.set_json("/api/task-page", &page);
    super::wait_for_feed(&site).expect("the page follows no feed");
    site.push(
        4183,
        &json!({"version":4183,"changes":[{"region":"task","data":{"key":SLUG,"value":page["task"]}}]}).to_string(),
    );
    wait_reads(&mut chrome, &site, "visual%2Fin-progress", 6);
    assert_eq!(chrome.eval("window.minuteTimers"), json!(4));

    // Left for the board, the timeline is gone and reads no more.
    chrome.eval("document.querySelector('header a[href=\"/tasks\"]').click()");
    assert!(chrome.wait_for(
        "document.querySelector('[data-ready=board]') !== null && document.getElementById('timeline-root') === null",
        SHOWN
    ));
    chrome.eval("window.minute && window.minute()");
    chrome.frames();
    chrome.eval("new Promise(resolve => setTimeout(resolve, 300))");
    assert_eq!(
        (reads(&site), chrome.eval("window.minuteTimers")),
        (6, json!(4))
    );

    // Back on the task, read again; once the task is done, no minute after.
    chrome.eval("history.back()");
    assert!(chrome.wait_for("window.minuteTimers === 5", SHOWN));
    assert_eq!(reads(&site), 7);
    let mut done = timed(fixture());
    done["closed"] = done["now"].clone();
    site.set_json(symphony_api::TASK_TIMELINE_PATH, &done);
    chrome.eval("window.minute()");
    assert!(chrome.wait_for(
        "document.querySelector('.tl-totals span')?.textContent.includes('in all')",
        SHOWN
    ));
    chrome.frames();
    chrome.eval("new Promise(resolve => setTimeout(resolve, 300))");
    assert_eq!(
        (
            reads(&site),
            chrome.eval("[window.minuteTimers, window.minute]")
        ),
        (8, json!([5, null]))
    );
}

/// Where a failed read's Try again is: brought to the middle of the screen, its centre, whether it
/// lies inside the box, and whether a tap at its centre lands on it.
const TRY_AGAIN: &str = "(() => { const button = document.querySelector('#timeline-slot .tl-failed button'); button.scrollIntoView({block: 'center'}); const b = button.getBoundingClientRect(), box = document.getElementById('timeline-slot').getBoundingClientRect(); const x = b.left + b.width / 2, y = b.top + b.height / 2; return {x, y, inside: b.top >= box.top && b.bottom <= box.bottom && b.left >= box.left && b.right <= box.right, hit: document.elementFromPoint(x, y)?.closest('button') === button}; })()";

/// Wait until the page has drawn a failed read's Try again in the timeline's box.
fn failed_read(chrome: &mut super::chrome::Chrome) {
    assert!(
        chrome.wait_for(
            "document.querySelector('#timeline-slot .tl-failed button') !== null",
            SHOWN
        ),
        "no failure was drawn: {:?}",
        chrome.eval("document.querySelector('article.task')?.innerText")
    );
    chrome.frames();
}

#[test]
fn a_timeline_that_cannot_be_read_says_so_in_its_box_and_try_again_reads_it() {
    let site = served(&fixture());
    // A phone off its network: the read's fetch itself fails, and no server says why.
    site.drop_route(symphony_api::TASK_TIMELINE_PATH, true);
    let Some(mut chrome) = chrome() else { return };
    view(&mut chrome, 390, true);
    chrome.open(&site.url(TASK));
    failed_read(&mut chrome);
    // Said from the error's message, never its stack, and Try again inside the box, where a tap
    // at its centre lands on it.
    assert_eq!(
        shown(&mut chrome, "#timeline-slot .tl-failed"),
        ["The timeline could not be read: Failed to fetch. Trying again… Try again"]
    );
    let tap = chrome.eval(TRY_AGAIN);
    assert_eq!(
        (tap["inside"].clone(), tap["hit"].clone()),
        (json!(true), json!(true)),
        "{tap}"
    );
    let before = reads(&site);
    site.drop_route(symphony_api::TASK_TIMELINE_PATH, false);
    for kind in ["mousePressed", "mouseReleased"] {
        chrome.call(
            "Input.dispatchMouseEvent",
            json!({"type": kind, "x": tap["x"], "y": tap["y"], "button": "left", "clickCount": 1}),
        );
    }
    assert!(chrome.wait_for("document.querySelectorAll('.tl-drawn .tl-row').length === 13 && !document.querySelector('.tl-failed')", SHOWN));
    // The tap's read, or the one the page was making by itself, which the tap joins.
    assert!(reads(&site) > before);
    // The tap was the timeline's alone: no control of the page's opened.
    assert_eq!(
        chrome.eval("['quick-open', 'bell'].map(id => document.getElementById(id).getAttribute('aria-expanded'))"),
        json!(["false", "false"])
    );

    // A server that answers why: its words, and Try again within reach wide too.
    site.fail_route(symphony_api::TASK_TIMELINE_PATH, true);
    view(&mut chrome, 1280, false);
    chrome.open(&site.url(TASK));
    failed_read(&mut chrome);
    assert_eq!(
        shown(&mut chrome, "#timeline-slot .tl-failed"),
        ["The timeline could not be read: Board snapshot unavailable. Trying again… Try again"]
    );
    let tap = chrome.eval(TRY_AGAIN);
    assert_eq!(
        (tap["inside"].clone(), tap["hit"].clone()),
        (json!(true), json!(true)),
        "{tap}"
    );
}

/// How many times the page read the timeline of the task `slug` (as its address writes it).
fn reads_of(site: &Site, slug: &str) -> usize {
    let asked = format!("{}?slug={slug}", symphony_api::TASK_TIMELINE_PATH);
    site.targets()
        .iter()
        .filter(|target| **target == asked)
        .count()
}

/// Wait until the page has read the timeline of `slug` `count` times.
#[track_caller]
fn wait_reads(chrome: &mut super::chrome::Chrome, site: &Site, slug: &str, count: usize) {
    let read = test_time::until_true(
        format!("{slug}'s timeline read {count} times"),
        test_time::backstop(std::time::Duration::from_secs(10)),
        || {
            reads_of(site, slug) >= count || {
                chrome.eval("new Promise(resolve => setTimeout(resolve, 25))");
                false
            }
        },
    );
    if let Err(stall) = read {
        panic!("{stall}: {:?}", site.targets());
    }
    assert_eq!(reads_of(site, slug), count, "{slug}: {:?}", site.targets());
}

/// A link on the page to `path`, followed as a click follows it: the shell moves the page.
fn follow(chrome: &mut super::chrome::Chrome, path: &str) {
    chrome.eval(&format!(
        "(() => {{ const next = document.createElement('a'); next.href = {}; document.body.append(next); next.click(); next.remove(); }})()",
        json!(path)
    ));
}

/// Wait until Chrome says the page gave up its read of `slug`'s timeline (as its address writes
/// it): the box that asked for it went.
#[track_caller]
fn given_up(chrome: &mut super::chrome::Chrome, slug: &str) {
    let asked = format!("{}?slug={slug}", symphony_api::TASK_TIMELINE_PATH);
    if let Err(stall) = test_time::until_true(
        format!("{slug}'s timeline read given up"),
        super::STALL,
        || {
            chrome
                .requests()
                .iter()
                .any(|request| request.url.ends_with(&asked) && request.canceled)
        },
    ) {
        panic!("{stall}: {:#?}", chrome.requests());
    }
}

/// Where the timeline is drawn: each box on the page with the task its page is of, and how many
/// steps are drawn in it.
const BOXES: &str = "[...document.querySelectorAll('#timeline-slot')].map(box => [box.closest('article.task')?.dataset.slug ?? null, box.dataset.slug, box.querySelectorAll('.tl-drawn .tl-list .tl-row').length])";

/// A task's timeline is drawn only in the box of the task it was read for
/// (apps/symphony-web/src/timeline.tsx): the box the page takes down as it moves to another task
/// gives its read up, and its answer, however late, draws nothing in the next task's box, which
/// reads and draws its own; a page that leaves the task pages while a read is out draws nothing
/// anywhere.
#[test]
fn an_answer_for_a_task_the_page_left_never_draws_one_tasks_timeline_in_anothers_box() {
    const PATH: &str = symphony_api::TASK_TIMELINE_PATH;
    // A's timeline is one step; B's the pictures' thirteen.
    let mut one = fixture();
    one["stages"] = json!([one["stages"][0].clone()]);
    one["stages"][0]["to"] = Value::Null;
    for empty in ["runs", "gates", "claims", "rounds"] {
        one[empty] = json!([]);
    }
    let site = served(&fixture());
    for slug in ["a/one", "b/two"] {
        let mut page = look_parity::responses()["/api/task-page"].clone();
        page["task"]["slug"] = json!(slug);
        page["task"]["title"] = json!(format!("Task {slug}"));
        site.set_json(
            &format!("/api/task-page?slug={}", slug.replace('/', "%2F")),
            &page,
        );
    }
    let a = format!("{PATH}?slug=a%2Fone");
    site.set_json(&a, &one);
    site.set_json(&format!("{PATH}?slug=b%2Ftwo"), &fixture());
    let Some(mut chrome) = chrome() else { return };
    view(&mut chrome, 1280, false);

    // A reads, its answer held: A's box shows its placeholder.
    site.hold_route(&a, true);
    chrome.open(&site.url("/task/a/one"));
    wait_reads(&mut chrome, &site, "a%2Fone", 1);
    assert!(chrome.wait_for(
        "document.querySelector('article.task[data-slug=\"a/one\"] #timeline-slot .tl-sk-row') !== null",
        SHOWN
    ));
    // The page goes to B's task before A's answer: A's box goes, giving its read up, and B's box
    // reads and draws B's thirteen steps.
    follow(&mut chrome, "/task/b/two");
    given_up(&mut chrome, "a%2Fone");
    assert!(
        chrome.wait_for(
            "document.querySelectorAll('article.task[data-slug=\"b/two\"] #timeline-slot .tl-drawn .tl-list .tl-row').length === 13",
            SHOWN
        ),
        "{:?}",
        chrome.eval(BOXES)
    );
    // A's answer is sent, late: B's box still draws B's alone.
    site.hold_route(&a, false);
    if let Err(stall) = test_time::until_true("A's late answer sent", super::STALL, || {
        site.answers(PATH)
            .iter()
            .any(|answer| answer.target.ends_with("slug=a%2Fone"))
    }) {
        panic!("{stall}: {:?}", site.targets());
    }
    chrome.frames();
    assert_eq!(chrome.eval(BOXES), json!([["b/two", "b/two", 13]]));
    assert_eq!(
        (reads_of(&site, "a%2Fone"), reads_of(&site, "b%2Ftwo")),
        (1, 1)
    );

    // Back on A, its read held again, the page leaves for the board before it answers: the read is
    // given up, and its answer draws no timeline anywhere.
    site.hold_route(&a, true);
    follow(&mut chrome, "/task/a/one");
    wait_reads(&mut chrome, &site, "a%2Fone", 2);
    follow(&mut chrome, "/tasks");
    assert!(chrome.wait_for(
        "document.querySelector('.waiting-board:not(.waiting-ghost)') !== null && document.getElementById('timeline-root') === null",
        SHOWN
    ));
    if let Err(stall) = test_time::until_true("A's second read given up", super::STALL, || {
        chrome
            .requests()
            .iter()
            .filter(|request| request.url.ends_with(&a) && request.canceled)
            .count()
            == 2
    }) {
        panic!("{stall}: {:#?}", chrome.requests());
    }
    site.hold_route(&a, false);
    chrome.frames();
    assert_eq!(
        chrome.eval(
            "[document.querySelector('.tl-drawn'), document.getElementById('timeline-root')]"
        ),
        json!([null, null])
    );
}

/// The pictures a review of the timeline reads beside the approved ones
/// (docs/product/analysis/reports-timeline-*.svg): `symphony shot`, the workspace's build run as an agent
/// runs it, of the pictures' task and of a week-long task of fifty steps on its page from the
/// stand-in server, at a phone's width and wide, light and dark, and of a phone off its network
/// failing to read it, into the folder `SYMPHONY_TIMELINE_SHOTS` names. Without it the case takes
/// none.
#[test]
fn pictures_of_the_timeline_when_a_folder_is_named() {
    let Some(out) = std::env::var_os("SYMPHONY_TIMELINE_SHOTS").map(PathBuf::from) else {
        eprintln!("SYMPHONY_TIMELINE_SHOTS names no folder: no pictures taken");
        return;
    };
    let Some(browser) = super::chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): no pictures taken");
        return;
    };
    std::fs::create_dir_all(&out).unwrap();
    let symphony = binaries::built("symphony");
    let shoot_after = |site: &Site, view: &[&str], steps: &[&str], picture: PathBuf| {
        let done = Command::new(&symphony)
            .args(["shot", "--timeout", "120", &site.url(TASK)])
            .args(view)
            .args([
                "--wait-for",
                ".tl-drawn",
                "--eval",
                "document.getElementById('timeline').scrollIntoView()",
            ])
            .args(steps)
            .args(["--pause", "300", "--report", "--out"])
            .arg(&picture)
            .env("SYMPHONY_CHROME", &browser)
            .output()
            .expect("run symphony shot");
        assert!(
            done.status.success() && picture.is_file(),
            "{}: {}\n{}",
            picture.display(),
            String::from_utf8_lossy(&done.stdout),
            String::from_utf8_lossy(&done.stderr)
        );
        eprintln!(
            "{}: {}",
            picture.display(),
            String::from_utf8_lossy(&done.stdout).trim()
        );
    };
    let shoot =
        |site: &Site, view: &[&str], picture: PathBuf| shoot_after(site, view, &[], picture);
    let phone = [
        ("phone-light", &["--phone", "--scheme", "light"][..]),
        ("phone-dark", &["--phone", "--scheme", "dark"]),
    ];
    let wide = [
        (
            "wide-light",
            &["--size", "1280x900", "--scheme", "light"][..],
        ),
        ("wide-dark", &["--size", "1280x900", "--scheme", "dark"]),
    ];
    for (task, timeline) in [("fixture", fixture()), ("week", week())] {
        let site = served(&timeline);
        for (name, view) in phone.iter().chain(&wide) {
            shoot(&site, view, out.join(format!("timeline-{task}-{name}.png")));
        }
    }
    // The card's example task, before (the client `SYMPHONY_TIMELINE_BEFORE` names, a build's
    // `public/`, else the committed one) and after (this build), on a phone and 1440 wide as the
    // card's pictures are, and 1600 wide, where its reviewer's label is weighed closest to its
    // room; after, a message selected, zoomed, on the true clock and zoomed across its quiet
    // stretch's band, and a phone's row of that stretch.
    let committed = std::env::var_os("SYMPHONY_TIMELINE_BEFORE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../symphony-server/client/public")
        });
    let card = [
        (
            "wide-light",
            &["--size", "1440x1100", "--scheme", "light"][..],
        ),
        ("wide-dark", &["--size", "1440x1100", "--scheme", "dark"]),
        (
            "wide-1600-light",
            &["--size", "1600x1100", "--scheme", "light"],
        ),
    ];
    for (version, site) in [
        (
            "before",
            guards_on(Site::start(
                &committed,
                &super::common::fixture("board.json"),
            )),
        ),
        ("after", guards_on(site())),
    ] {
        for (name, view) in phone.iter().chain(&card) {
            shoot(
                &site,
                view,
                out.join(format!("timeline-guards-{version}-{name}.png")),
            );
        }
    }
    let site = guards_on(site());
    for (name, view) in card {
        shoot_after(
            &site,
            view,
            &["--click", ".tl-message-mark[data-message='15396']"],
            out.join(format!("timeline-guards-after-selected-{name}.png")),
        );
        // Zoomed to the first review, 11:22 to 11:31, by a double click on its bar, then out
        // three times from it, the verdict's message selected: 10:53 to 12:01 with the overview.
        let review = "document.querySelector('.tl-lane[data-lane=stage] .tl-seg:nth-of-type(3)')";
        shoot_after(
            &site,
            view,
            &[
                "--eval",
                &format!("{review}.dispatchEvent(new MouseEvent('dblclick', {{bubbles: true}}))"),
                "--wait-for",
                ".tl-overview",
                "--eval",
                &format!("{review}.focus()"),
                "--key",
                "-",
                "--key",
                "-",
                "--key",
                "-",
                "--click",
                ".tl-message-mark[data-message='15396']",
            ],
            out.join(format!("timeline-guards-after-zoomed-{name}.png")),
        );
        // The first review zoomed in once more, to the least, five minutes, where + cannot act.
        shoot_after(
            &site,
            view,
            &[
                "--eval",
                &format!("{review}.dispatchEvent(new MouseEvent('dblclick', {{bubbles: true}}))"),
                "--wait-for",
                ".tl-overview",
                "--click",
                ".tl-zoom button[aria-label='Zoom in']",
                "--wait-for",
                ".tl-zoom button[aria-label='Zoom in']:disabled",
            ],
            out.join(format!("timeline-guards-after-least-{name}.png")),
        );
        // A mouse held mid-drag across the hours from 08:00 to 12:30, its band drawn: the
        // pointer's events sent to the lanes, which capture none of a pointer the page made.
        shoot_after(
            &site,
            view,
            &[
                "--eval",
                "(() => { Element.prototype.setPointerCapture = () => {}; const h = document.querySelector('.tl-lanes .tl-hours').getBoundingClientRect(), s = document.querySelector('.tl-lane[data-lane=stage] .tl-track').getBoundingClientRect(), lanes = document.querySelector('.tl-lanes'); const fire = (type, f) => lanes.dispatchEvent(new PointerEvent(type, {bubbles: true, clientX: h.left + f * h.width, clientY: s.top + s.height / 2, pointerType: 'mouse', button: 0, buttons: 1, pointerId: 1})); fire('pointerdown', 7 / 27.58); fire('pointermove', 9 / 27.58); fire('pointermove', 11.5 / 27.58); })()",
                "--wait-for",
                ".tl-band",
            ],
            out.join(format!("timeline-guards-after-band-{name}.png")),
        );
        // The quiet stretch on the true clock, the switch pressed off.
        shoot_after(
            &site,
            view,
            &[
                "--click",
                ".tl-skip",
                "--wait-for",
                ".tl-skip[aria-pressed='false']",
            ],
            out.join(format!("timeline-guards-after-true-clock-{name}.png")),
        );
        // ⌘ + wheel about the review that ends the quiet stretch: zoomed across its band.
        shoot_after(
            &site,
            view,
            &[
                "--eval",
                "(() => { const b = document.querySelector('.tl-lane[data-lane=stage] .tl-seg[data-from=\"1795132641.4\"]').getBoundingClientRect(); document.querySelector('.tl-lanes').dispatchEvent(new WheelEvent('wheel', {bubbles: true, cancelable: true, clientX: b.left + b.width / 2, clientY: b.top + b.height / 2, deltaY: -300, ctrlKey: true})); })()",
                "--wait-for",
                ".tl-overview .tl-ov-break",
            ],
            out.join(format!("timeline-guards-after-across-{name}.png")),
        );
    }
    // On a phone, the list shown whole and the quiet stretch's row in the middle of the screen.
    for (name, view) in phone {
        shoot_after(
            &site,
            view,
            &[
                "--click",
                ".tl-more",
                "--eval",
                "document.querySelector('.tl-list .tl-quiet').scrollIntoView({block: 'center'})",
            ],
            out.join(format!("timeline-guards-after-quiet-{name}.png")),
        );
    }
    let site = served(&fixture());
    site.drop_route(symphony_api::TASK_TIMELINE_PATH, true);
    for (name, view) in phone {
        shoot(&site, view, out.join(format!("timeline-failed-{name}.png")));
    }
}
