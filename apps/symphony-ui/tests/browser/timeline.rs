//! A task's timeline on its page (src/timeline_slot.rs, the page's box; src/task_timeline.rs, the
//! module the shell loads into it), the release client against the stand-in server answering
//! `/api/task-timeline` with the approved pictures' task: a list that fits a phone's width, lanes
//! over the same hours on a wide screen, a box that moves nothing when the timeline arrives, and a
//! read each minute only while the page is shown and the task open.
use super::{SHOWN, binaries, chrome, components, look_parity, site, site::Site};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;

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

/// The stand-in server answering the look-parity task's page and thread, no offer to the owner, and
/// `timeline` for its timeline.
fn served(timeline: &Value) -> Site {
    let site = site();
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
    chrome.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width": width, "height": 900, "deviceScaleFactor": 1, "mobile": phone}),
    );
    frames(chrome);
}

/// Two of the page's frames drawn.
fn frames(chrome: &mut super::chrome::Chrome) {
    chrome.eval(
        "new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))",
    );
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
    frames(chrome);
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
    frames(chrome);
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
        ["Stage", "Executor", "Reviewer", "Builds & tests"]
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
    frames(&mut chrome);
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
        chrome.eval("document.querySelector('.tl-lane[data-lane=runs], .tl-running, .tl-machine')"),
        Value::Null
    );
    assert_eq!(
        shown(&mut chrome, ".tl-totals"),
        ["6 h 18 m so far · 2 review rounds · 1 bounce"]
    );
}

/// The pairs of words drawn in one track of the lanes whose boxes meet: none, where every word is
/// whole in its own box ([`CUT`]).
const OVERPRINT: &str = "(() => { const met = []; for (const track of document.querySelectorAll('.tl-lanes .tl-track')) { const words = [...track.querySelectorAll('.tl-fit, .tl-tick')].filter(e => e.getClientRects().length > 0 && e.textContent.trim() !== '').map(e => [e.textContent.trim(), e.getBoundingClientRect()]); for (let i = 0; i < words.length; i++) for (let j = i + 1; j < words.length; j++) { const [a, r] = words[i], [b, s] = words[j]; if (r.left < s.right - 0.5 && s.left < r.right - 0.5 && r.top < s.bottom - 0.5 && s.top < r.bottom - 0.5) met.push([a, b]); } } return met; })()";

/// The parts of the lanes drawn with words wider than they are, cut short.
const CUT: &str = "[...document.querySelectorAll('.tl-lanes .tl-seg, .tl-lanes .tl-after, .tl-lanes .tl-under, .tl-lanes .tl-g')].filter(e => e.getClientRects().length > 0 && e.textContent.trim() !== '' && e.scrollWidth > e.clientWidth + 1).map(e => e.textContent)";

#[test]
fn no_word_of_the_lanes_is_drawn_over_another_at_any_width_for_the_fixture_or_a_week() {
    for (name, timeline, rows) in [("the fixture", fixture(), 13), ("a week", week(), 50)] {
        let site = served(&timeline);
        let Some(mut chrome) = chrome() else { return };
        open(&mut chrome, &site, 1280, rows);
        for width in [900, 1100, 1280, 1600] {
            view(&mut chrome, width, false);
            assert_eq!(chrome.eval(OVERPRINT), json!([]), "{name} at {width} px");
            assert_eq!(
                chrome.eval(CUT),
                json!([]),
                "{name} at {width} px: a word cut short"
            );
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
/// placeholder is drawn, where the page's parts stand ([`places`]).
fn placeholder_drawn(chrome: &mut super::chrome::Chrome, site: &Site) -> Value {
    site.hold_route(symphony_api::TASK_TIMELINE_PATH, true);
    chrome.open(&site.url(TASK));
    assert!(
        chrome.wait_for(
            "document.querySelector('.task-body') !== null && document.querySelectorAll('#timeline-slot .tl-sk-row').length > 0 && window.reviewTimelineReady === true",
            SHOWN
        ),
        "the box's placeholder was not drawn: {:?}",
        chrome.eval("document.querySelector('article.task')?.innerText")
    );
    frames(chrome);
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
    frames(&mut chrome);
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
    frames(&mut chrome);
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
/// event and the module whose part of the page it came from.
const SPIES: &str = "(() => { const from = node => node.closest('#timeline-root') ? 'timeline' : node.closest('#quick-root') ? 'panel' : 'page'; window.heard = {}; for (const id of ['review-root-tasks', 'timeline-root', 'quick-root', 'document']) { const at = id === 'document' ? document : document.getElementById(id); heard[id] = []; for (const type of ['click', 'keydown']) at.addEventListener(type, event => heard[id].push(`${type} ${from(event.target)}`)); } return true; })()";

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
    // Listeners on the roots, as each module's Dioxus listens, and one on the document. A click
    // and a key in the timeline, whose root is inside the page's, a click on the page's own
    // heading and one in the chat's sheet.
    assert_eq!(chrome.eval(SPIES), json!(true));
    chrome.eval(
        "(() => { const click = selector => document.querySelector(selector).dispatchEvent(new MouseEvent('click', {bubbles: true, cancelable: true})); click('article.task h1'); click('#timeline-root .tl-totals'); document.getElementById('tl-stop-0').dispatchEvent(new KeyboardEvent('keydown', {key: 'Home', bubbles: true, cancelable: true})); click('#quick-root #quick-text'); })()",
    );
    assert_eq!(
        chrome.eval("heard"),
        json!({
            "review-root-tasks": ["click page"],
            "timeline-root": ["click timeline", "keydown timeline"],
            "quick-root": ["click panel"],
            "document": ["click page", "click timeline", "keydown timeline", "click panel"],
        })
    );
    // Taken off, a listener on a root hears nothing more.
    chrome.eval("(() => { window.off = () => heard.off.push('click'); heard.off = []; const root = document.getElementById('review-root-tasks'); root.addEventListener('click', off); root.removeEventListener('click', off); document.querySelector('article.task h1').click(); })()");
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
        let before = placeholder_drawn(&mut chrome, &site);
        assert_eq!(before["box"][1], drawn, "{rows} steps: the next visit");
        site.hold_route(symphony_api::TASK_TIMELINE_PATH, false);
        let arrived =
            format!("document.querySelectorAll('.tl-drawn .tl-list .tl-row').length === {rows}");
        assert!(chrome.wait_for(&arrived, SHOWN), "{rows} steps: not drawn");
        frames(&mut chrome);
        // The height is kept in whole pixels, the timeline's own a fraction under it: what is under
        // the box may move by less than one.
        let after = places(&mut chrome);
        assert!(
            ["brief", "thread"].iter().all(|part| {
                (after[part].as_f64().unwrap() - before[part].as_f64().unwrap()).abs() < 1.0
            }) && after["box"] == before["box"],
            "{rows} steps: the timeline moved the page: {before} then {after}"
        );
        chrome.eval("document.getElementById('timeline').scrollIntoView()");
        frames(&mut chrome);
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
        frames(&mut chrome);
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
        frames(&mut chrome);
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
    frames(&mut chrome);
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

    // The task changes: its timeline is read again at once.
    let mut page = look_parity::responses()["/api/task-page"].clone();
    page["task"]["updated"] = json!(2.0);
    site.set_json("/api/task-page", &page);
    super::wait_for_feed(&site).expect("the page follows no feed");
    site.push(
        4183,
        &json!({"version":4183,"changes":[{"region":"task","data":{"key":SLUG,"value":page["task"]}}]}).to_string(),
    );
    assert!(
        chrome.wait_for("window.minuteTimers === 5", SHOWN),
        "the task's change did not read its timeline again: {:?}",
        site.targets()
    );
    assert_eq!(reads(&site), 5);

    // Left for the board, the timeline is gone and reads no more.
    chrome.eval("document.querySelector('header a[href=\"/tasks\"]').click()");
    assert!(chrome.wait_for(
        "document.querySelector('[data-ready=board]') !== null && !window.reviewTimelineRequest",
        SHOWN
    ));
    chrome.eval("window.minute && window.minute()");
    frames(&mut chrome);
    chrome.eval("new Promise(resolve => setTimeout(resolve, 300))");
    assert_eq!(
        (reads(&site), chrome.eval("window.minuteTimers")),
        (5, json!(5))
    );

    // Back on the task, read again; once the task is done, no minute after.
    chrome.eval("history.back()");
    assert!(chrome.wait_for("window.minuteTimers === 6", SHOWN));
    assert_eq!(reads(&site), 6);
    let mut done = timed(fixture());
    done["closed"] = done["now"].clone();
    site.set_json(symphony_api::TASK_TIMELINE_PATH, &done);
    chrome.eval("window.minute()");
    assert!(chrome.wait_for(
        "document.querySelector('.tl-totals span')?.textContent.includes('in all')",
        SHOWN
    ));
    frames(&mut chrome);
    chrome.eval("new Promise(resolve => setTimeout(resolve, 300))");
    assert_eq!(
        (
            reads(&site),
            chrome.eval("[window.minuteTimers, window.minute]")
        ),
        (7, json!([6, null]))
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
    frames(chrome);
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
        ["The timeline could not be read: Failed to fetch. Try again"]
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
    assert_eq!(reads(&site), before + 1);
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
        ["The timeline could not be read: Board snapshot unavailable. Try again"]
    );
    let tap = chrome.eval(TRY_AGAIN);
    assert_eq!(
        (tap["inside"].clone(), tap["hit"].clone()),
        (json!(true), json!(true)),
        "{tap}"
    );
}

/// Boxes the test puts in the page as a task page's would (`box`, the box it replaces kept as
/// `lastBox`), and their asks and closes to the shell (`ask`, `gone`); `place` says which box the
/// timeline's root is in and whether hidden.
const BOXES: &str = "window.box = slug => { window.lastBox = document.getElementById('timeline-slot'); lastBox?.remove(); document.body.insertAdjacentHTML('beforeend', `<div id=\"timeline-slot\" data-slug=\"${slug}\"></div>`); }; window.ask = (owner, slug, revision) => reviewTimeline({owner, slug, revision, narrow: 240, wide: 240}); window.gone = owner => reviewTimeline({kind: 'close', owner}); window.place = () => { const root = document.getElementById('timeline-root'); return [root?.parentElement?.dataset.slug ?? null, root?.hidden ?? null]; }; true";

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

/// The shell draws the timeline only in the box of the task it was asked for, by the newest box
/// that asked (assets/shell.js `reviewTimeline`): an answer that lands after the page moved to
/// another task, and an ask or a close from a box the page took down, however late, draw nothing
/// in the next box.
#[test]
fn asks_closes_and_answers_out_of_order_never_draw_one_tasks_timeline_in_anothers_box() {
    const PATH: &str = symphony_api::TASK_TIMELINE_PATH;
    // A's first answer is a one-step timeline; every answer after it the pictures' thirteen steps.
    let mut one = fixture();
    one["stages"] = json!([one["stages"][0].clone()]);
    one["stages"][0]["to"] = Value::Null;
    for empty in ["runs", "gates", "claims", "rounds"] {
        one[empty] = json!([]);
    }
    let site = served(&fixture());
    site.answer_in_turn(PATH, &[one, fixture()]);
    let Some(mut chrome) = chrome() else { return };
    view(&mut chrome, 1280, false);
    chrome.open(&site.url("/tasks"));
    assert!(chrome.wait_for(
        "document.querySelector(\"[data-slug='tools/example']\") !== null",
        SHOWN
    ));
    chrome.eval(BOXES);
    let slot_rows = "document.querySelectorAll('#timeline-slot .tl-row').length";

    // A reads: the root sits in A's box while A's read is held.
    site.hold_route(PATH, true);
    chrome.eval("box('a/one'); ask(1001, 'a/one', 1)");
    assert!(chrome.wait_for("window.reviewTimelineReady === true", SHOWN));
    wait_reads(&mut chrome, &site, "a%2Fone", 1);
    assert_eq!(chrome.eval("place()"), json!(["a/one", false]));

    // The page goes to B's task before A's box says it is gone, and A's answer lands late: drawn
    // in A's box, which the page took down, and nowhere in B's.
    chrome.eval("box('b/two')");
    site.hold_route(PATH, false);
    assert!(chrome.wait_for(
        "window.lastBox?.querySelector('.tl-drawn .tl-row') != null",
        SHOWN
    ));
    // A asks again from its old box (its effect running late): the timeline is put in no box.
    chrome.eval("ask(1001, 'a/one', 1)");
    assert_eq!(
        chrome.eval(&format!("[place(), {slot_rows}]")),
        json!([[null, true], 0])
    );
    // A's box closes, then asks once more, later still: dropped.
    chrome.eval("gone(1001); ask(1001, 'a/one', 1)");
    assert_eq!(
        chrome.eval("[window.reviewTimelineRequest, place()]"),
        json!([null, [null, true]])
    );
    // B's box asks, and an ask from A's older box after it changes nothing: B's box draws B's
    // thirteen steps.
    chrome.eval("ask(1002, 'b/two', 1); ask(1001, 'a/one', 2)");
    assert_eq!(
        chrome.eval("window.reviewTimelineRequest.slug"),
        json!("b/two")
    );
    assert!(
        chrome.wait_for(&format!("{slot_rows} === 13"), SHOWN),
        "{:?}",
        chrome.eval("document.getElementById('timeline-slot').innerText")
    );
    assert_eq!(chrome.eval("place()"), json!(["b/two", false]));
    assert_eq!(
        (reads_of(&site, "a%2Fone"), reads_of(&site, "b%2Ftwo")),
        (1, 1)
    );

    // A reads again; the page goes to B, whose box asks and closes (the page left) before either
    // answer; then A's close and an ask of A's arrive late. Every answer lands: nothing is drawn,
    // in any box.
    site.hold_route(PATH, true);
    chrome.eval("box('a/one'); gone(1002); ask(1003, 'a/one', 1)");
    wait_reads(&mut chrome, &site, "a%2Fone", 2);
    assert_eq!(chrome.eval("place()"), json!(["a/one", false]));
    chrome.eval("box('b/two'); ask(1004, 'b/two', 1)");
    wait_reads(&mut chrome, &site, "b%2Ftwo", 2);
    chrome.eval("gone(1004); gone(1003); ask(1003, 'a/one', 1)");
    site.hold_route(PATH, false);
    frames(&mut chrome);
    chrome.eval("new Promise(resolve => setTimeout(resolve, 300))");
    frames(&mut chrome);
    assert_eq!(
        chrome.eval(&format!(
            "[window.reviewTimelineRequest, document.querySelector('.tl-drawn'), place(), {slot_rows}]"
        )),
        json!([null, null, [null, true], 0])
    );
}

/// The pictures a review of the timeline reads beside the approved ones
/// (docs/analysis/reports-timeline-*.svg): `symphony shot`, the workspace's build run as an agent
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
    let shoot = |site: &Site, view: &[&str], picture: PathBuf| {
        let done = Command::new(&symphony)
            .args(["shot", "--timeout", "120", &site.url(TASK)])
            .args(view)
            .args([
                "--wait-for",
                ".tl-drawn",
                "--eval",
                "document.getElementById('timeline').scrollIntoView()",
                "--pause",
                "300",
                "--report",
                "--out",
            ])
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
    let site = served(&fixture());
    site.drop_route(symphony_api::TASK_TIMELINE_PATH, true);
    for (name, view) in phone {
        shoot(&site, view, out.join(format!("timeline-failed-{name}.png")));
    }
}
