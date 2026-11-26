//! The quiet stretches a task's wide timeline skips (apps/symphony-web: src/timeline-quiet.ts,
//! src/timeline-cut.ts, the bands in src/timeline.tsx), on the card's example task: the 10 h 54 m
//! after it closed is a band that says its length, the work given the width, the hours and the
//! readout true across it; a press on it, or the switch, gives the true clock and the switch gives
//! the band back; zoom and movement stay true across it; a phone's list says it as a row of its
//! own; a task with no quiet stretch draws no band and no switch.
use super::zoom::readout;
use super::*;

/// The card's task whole, its quiet stretch skipped, and on the true clock.
const SKIPPED: &str = "11-19 01:00 → 11-20 04:34 · 1 d 3 h · 10 h 54 m skipped";
const WHOLE: &str = "11-19 01:00 → 11-20 04:34 · 1 d 3 h";

/// The toolbar's readout.
const RANGE: &str = "document.querySelector('.tl-range')?.textContent ?? null";

/// The switch: its words, whether it reads pressed, and whether it has the focus.
const SWITCH: &str = "(() => { const s = document.querySelector('.tl-skip'); return s ? [s.textContent, s.getAttribute('aria-pressed'), document.activeElement === s] : null; })()";

/// The band as drawn: how many, its width and the track's, whether it runs through every lane,
/// its title and label, its words down it and the hours line's over and after it, as shown.
const BAND: &str = "(() => { const all = document.querySelectorAll('.tl-break'), b = all[0]; if (!b) return null; const r = b.getBoundingClientRect(), track = document.querySelector('.tl-lane[data-lane=stage] .tl-track').getBoundingClientRect(), lanes = [...document.querySelectorAll('.tl-lane[data-lane]')].map(l => l.getBoundingClientRect()); const seen = e => e !== null && e.getClientRects().length > 0; const length = document.querySelector('.tl-break-length'), day = document.querySelector('.tl-break-day'), words = b.querySelector('.tl-fit'); const l = length.getBoundingClientRect(), d = day.getBoundingClientRect(); return {count: all.length, width: Math.round(r.width * 10) / 10, track: Math.round(track.width), through: r.top <= lanes[0].top + 1 && r.bottom >= lanes[lanes.length - 1].bottom - 1, title: b.title, label: b.getAttribute('aria-label'), words: seen(words) ? words.textContent : null, length: seen(length) ? length.firstChild.textContent : null, day: seen(day) ? day.textContent : null, centred: Math.abs((l.left + l.right) / 2 - (r.left + r.right) / 2) < 1, after: d.left >= l.right - 1}; })()";

/// The width of the stage's second stretch, three hours of work before the band.
const WORK: &str = "document.querySelector('.tl-lane[data-lane=stage] .tl-seg[data-from]:nth-of-type(2)').getBoundingClientRect().width";

/// Whether the page scrolls sideways.
const SIDEWAYS: &str = "document.documentElement.scrollWidth > innerWidth";

/// The band's title: its stretch, its ends and why it is quiet.
const TITLE: &str = "10 h 54 m quiet, 11-19 14:03 → 11-20 00:57, task done, nothing recorded";

/// The card's task open on a desk at 1440 wide, its quiet stretch skipped as it opens.
fn opened(chrome: &mut crate::chrome::Chrome, site: &Site) {
    open_drawn(chrome, site, 1440);
    chrome.eval("document.getElementById('timeline').scrollIntoView()");
    readout(chrome, "opened", &format!("said === {}", json!(SKIPPED)));
}

/// The minutes the readout says are shown, and how many of them it says are skipped.
fn minutes(said: &str) -> (i64, i64) {
    let minutes = |words: &str| -> i64 {
        let words: Vec<&str> = words.split(' ').collect();
        words
            .chunks(2)
            .map(|pair| {
                pair[0].parse::<i64>().unwrap()
                    * match pair[1] {
                        "d" => 1440,
                        "h" => 60,
                        _ => 1,
                    }
            })
            .sum()
    };
    let parts: Vec<&str> = said.split(" · ").collect();
    let span = minutes(parts[1].split(" of ").next().unwrap());
    let skipped = parts
        .get(2)
        .map_or(0, |part| minutes(part.trim_end_matches(" skipped")));
    (span, skipped)
}

/// The minutes of work the readout says are shown: its span less what it says is skipped.
fn work(said: &str) -> i64 {
    let (span, skipped) = minutes(said);
    span - skipped
}

/// The minutes of work a track holds at the readout's zoom: the work shown, and the share of the
/// track the band takes for the part of its 654 minutes in view (BAND_SHARE, 0.033).
fn held(said: &str) -> f64 {
    let (_, skipped) = minutes(said);
    work(said) as f64 / (1.0 - 0.033 * skipped as f64 / 654.0)
}

#[test]
fn the_readout_says_the_work_shown_and_a_track_holds() {
    assert_eq!(work(WHOLE), 1620);
    assert_eq!(work(SKIPPED), 1620 - 654);
    assert_eq!(work("a · 2 h 5 m of 1 d 3 h · 1 h 1 m skipped"), 64);
    assert!((held(SKIPPED) - 966.0 / 0.967).abs() < 1e-9);
    assert_eq!(held("a · 4 h 19 m of 1 d 3 h"), 259.0);
}

/// The card's task opens with its 10 h 54 m after it closed a band through every lane, 36 px of
/// the 1,095 px track, its length over it and its day after that, its words down it; the work gets
/// 1.6 times the width; the hours and the readout are true across it; the switch says how many and
/// how long.
#[test]
fn the_cards_quiet_stretch_is_a_band_that_says_its_length_and_the_work_gets_the_width() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    opened(&mut chrome, &site);
    let band = chrome.eval(BAND);
    assert_eq!(
        band,
        json!({"count": 1, "width": 36.1, "track": 1095, "through": true, "title": TITLE, "label": format!("Show on the true clock: {TITLE}"), "words": "quiet · task done, nothing recorded", "length": "+10 h 54 m", "day": "11-20", "centred": true, "after": true})
    );
    assert_eq!(
        chrome.eval(SWITCH),
        json!(["Skip quiet stretches1 · 10 h 54 m", "true", false])
    );
    // The work's width against the true clock's: 1.6 times, as the card says.
    let skipped = chrome.eval(WORK).as_f64().unwrap();
    tap(&mut chrome, ".tl-skip");
    readout(
        &mut chrome,
        "switched off",
        &format!("said === {}", json!(WHOLE)),
    );
    let true_clock = chrome.eval(WORK).as_f64().unwrap();
    let wider = skipped / true_clock;
    assert!(
        (1.55..1.7).contains(&wider),
        "the work is {wider} times as wide"
    );
    assert_eq!(chrome.eval(SIDEWAYS), json!(false));
}

/// A press on the band draws its stretch on the true clock, the press the timeline's alone, and
/// the focus the press gave the band goes to the switch, which gives the band back; so too from
/// the keyboard.
#[test]
fn a_press_on_the_band_shows_the_true_clock_and_the_switch_gives_the_band_back() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    opened(&mut chrome, &site);
    tap(&mut chrome, ".tl-break");
    readout(
        &mut chrome,
        "the band pressed",
        &format!("said === {}", json!(WHOLE)),
    );
    assert_eq!(chrome.eval(BAND), json!(null));
    assert_eq!(
        chrome.eval(SWITCH),
        json!(["Skip quiet stretches1 · 10 h 54 m", "false", true])
    );
    assert_eq!(
        chrome.eval("document.getElementById('quick-open').getAttribute('aria-expanded')"),
        json!("false"),
        "a press in the timeline is the timeline's alone"
    );
    tap(&mut chrome, ".tl-skip");
    readout(
        &mut chrome,
        "switched on",
        &format!("said === {}", json!(SKIPPED)),
    );
    assert_eq!(chrome.eval(BAND)["count"], json!(1));
    // The band by the keyboard: Enter on it gives the true clock and the focus to the switch, which
    // Enter turns on again, the focus kept.
    chrome.eval("document.querySelector('.tl-break').focus()");
    components::key(&mut chrome, "Enter", "Enter", 13);
    readout(
        &mut chrome,
        "the band by Enter",
        &format!("said === {}", json!(WHOLE)),
    );
    assert_eq!(
        chrome.eval(SWITCH),
        json!(["Skip quiet stretches1 · 10 h 54 m", "false", true])
    );
    components::key(&mut chrome, "Enter", "Enter", 13);
    readout(
        &mut chrome,
        "the switch by Enter",
        &format!("said === {}", json!(SKIPPED)),
    );
    assert_eq!(
        chrome.eval(SWITCH),
        json!(["Skip quiet stretches1 · 10 h 54 m", "true", true])
    );
}

/// ⌘/Ctrl + wheel zooms about the pointer with the band in view, the stretch under the pointer
/// staying under it; + zooms; Shift + wheel moves across the band, as much work shown at each
/// step, the band said in the readout while it is in view and its hours true; the overview marks
/// it.
#[test]
fn zoom_and_movement_stay_true_across_the_band() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    opened(&mut chrome, &site);
    // The pointer on the review that ends the stretch, the stage's first after the band.
    let stage =
        "document.querySelector('.tl-lane[data-lane=stage] .tl-seg[data-from=\"1795132641.4\"]')";
    let place = chrome.eval(&format!("(() => {{ const b = {stage}.getBoundingClientRect(), s = document.querySelector('.tl-lane[data-lane=stage] .tl-track').getBoundingClientRect(); return [Math.round(b.left + b.width / 2), s.top + s.height / 2]; }})()"));
    let (x, y) = (place[0].as_f64().unwrap(), place[1].as_f64().unwrap());
    let under = |chrome: &mut crate::chrome::Chrome| {
        chrome
            .eval(&format!("(() => {{ const b = {stage}.getBoundingClientRect(); return ({x} - b.left) / b.width; }})()"))
            .as_f64()
            .unwrap()
    };
    let share = under(&mut chrome);
    chrome.call(
        "Input.dispatchMouseEvent",
        json!({"type": "mouseWheel", "x": x, "y": y, "deltaX": 0, "deltaY": -300, "modifiers": 2}),
    );
    chrome.frames();
    let zoomed = readout(
        &mut chrome,
        "ctrl + wheel by the band",
        "said.includes(' of ')",
    );
    assert!(zoomed.ends_with(" · 10 h 54 m skipped"), "{zoomed}");
    // Half as much on the track: the whole task's 1,654 minutes less the 654 skipped are 1,000 of
    // work, 1,034 a track with the band's 3.3 %; half of that, 517, less the band's share, 500.
    assert!((work(&zoomed) - 500).abs() <= 1, "{zoomed}");
    // The instant that was under the pointer is under it still, within half a pixel.
    let moved = chrome
        .eval(&format!("(() => {{ const b = {stage}.getBoundingClientRect(); return b.left + {share} * b.width - {x}; }})()"))
        .as_f64()
        .unwrap();
    assert!(
        moved.abs() < 0.5,
        "the instant under the pointer moved {moved} px"
    );
    assert!(drawn(&mut chrome, ".tl-overview .tl-ov-break"));
    assert_eq!(chrome.eval(BAND)["width"], json!(36.1));
    // + about the middle, then Shift + wheel back across the band and past it: the zoom kept at
    // each step, a track holding as much work, the band said only while it is in view.
    tap(&mut chrome, ".tl-zoom button[aria-label='Zoom in']");
    // The press scrolled its button to the middle: the lanes back where the pointer is.
    chrome.eval("document.getElementById('timeline').scrollIntoView()");
    let closer = readout(
        &mut chrome,
        "+ by the band",
        &format!("said !== {}", json!(zoomed)),
    );
    let track = held(&closer);
    let mut said = closer.clone();
    let mut saw = (closer.contains(" skipped"), false);
    for step in 0..12 {
        chrome.call(
            "Input.dispatchMouseEvent",
            json!({"type": "mouseWheel", "x": x, "y": y, "deltaX": 0, "deltaY": -150, "modifiers": 8}),
        );
        chrome.frames();
        said = readout(
            &mut chrome,
            &format!("shift + wheel, step {step}"),
            &format!("said !== {}", json!(said)),
        );
        assert!(
            (held(&said) - track).abs() <= 2.0,
            "{said} against {closer}"
        );
        if said.contains(" skipped") {
            saw.0 = true;
        } else if saw.0 {
            saw.1 = true;
            break;
        }
    }
    assert_eq!(saw, (true, true), "the band never came and went: {said}");
    assert_eq!(chrome.eval(BAND), json!(null));
    assert_eq!(chrome.eval(SIDEWAYS), json!(false));
    assert_eq!(chrome.eval(RANGE), json!(said));
}

/// A task with no quiet stretch, the approved pictures' task, draws no band and no switch.
#[test]
fn a_task_with_no_quiet_stretch_draws_no_band_and_no_switch() {
    let site = served(&fixture());
    let Some(mut chrome) = chrome() else { return };
    open_drawn(&mut chrome, &site, 1440);
    assert!(drawn(&mut chrome, ".tl-zoom"));
    assert_eq!(chrome.eval(SWITCH), json!(null));
    assert_eq!(chrome.eval(BAND), json!(null));
    assert!(!chrome.eval(RANGE).as_str().unwrap().contains("skipped"));
    assert_eq!(
        chrome.eval("document.querySelectorAll('.tl-quiet').length"),
        json!(0)
    );
}

/// On a phone the card's quiet stretch is a row of its list, its length, its ends and why, in the
/// box's width; no toolbar, no switch.
#[test]
fn on_a_phone_the_quiet_stretch_is_a_row_of_its_list() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    open_drawn(&mut chrome, &site, 390);
    tap(&mut chrome, ".tl-more");
    let row = chrome.eval("(() => { const r = document.querySelector('.tl-list .tl-quiet'); r.scrollIntoView({block: 'center'}); const box = document.querySelector('.tl-drawn').getBoundingClientRect(), b = r.getBoundingClientRect(); return {words: [...r.children].map(e => e.textContent), title: r.title, inside: b.left >= box.left && b.right <= box.right, whole: [...r.children].every(e => e.scrollWidth <= e.clientWidth + 1), shown: r.getClientRects().length > 0}; })()");
    assert_eq!(
        row,
        json!({"words": ["10 h 54 m quiet, 14:03 → 11-20 00:57", "Task done, nothing recorded"], "title": "10 h 54 m quiet, 14:03 → 11-20 00:57: task done, nothing recorded", "inside": true, "whole": true, "shown": true})
    );
    assert!(!drawn(&mut chrome, ".tl-zoom"));
    assert!(!drawn(&mut chrome, ".tl-skip"));
    assert_eq!(chrome.eval(SIDEWAYS), json!(false));
}
