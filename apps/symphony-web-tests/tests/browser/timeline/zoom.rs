//! Zoom and sideways movement on a task's wide timeline (apps/symphony-web: src/timeline-zoom.ts,
//! src/timeline-navigate.tsx), on the card's example task: the whole task first, then zoomed and
//! moved by the toolbar, a wheel, a drag across the hours, a double click on a stage, the keys, the
//! overview's window and a touch screen's pinch and swipe, each back to the whole task, the
//! readout true at every zoom; a phone and a task too short to zoom into keep their drawing.
use super::*;

/// The toolbar's readout.
const RANGE: &str = "document.querySelector('.tl-range')?.textContent ?? null";
/// The card's task whole: from the hour it was made in to its last message.
const WHOLE: &str = "11-19 01:00 → 11-20 04:34 · 1 d 3 h";

/// Whether the readout is true of the hours drawn: each hour's label, at its place on the track,
/// within a minute of the time the readout's range puts there (its ends are said to the minute),
/// with how many hours are marked.
const TRUE: &str = "(() => { const said = document.querySelector('.tl-range').textContent; const m = said.match(/^(\\d\\d)-(\\d\\d) (\\d\\d):(\\d\\d) → (\\d\\d)-(\\d\\d) (\\d\\d):(\\d\\d) · /); if (!m) return {said, ticks: 0, off: null}; const at = (mo, d, h, mi) => Date.UTC(2026, mo - 1, d, h, mi) / 60000; const from = at(+m[1], +m[2], +m[3], +m[4]), to = at(+m[5], +m[6], +m[7], +m[8]); const ticks = [...document.querySelectorAll('.tl-axis .tl-tick:not(.tl-nowtag)')]; const off = ticks.map(t => { const expected = from + parseFloat(t.style.left) / 100 * (to - from); const date = t.textContent.match(/^(\\d\\d)-(\\d\\d)$/); if (date) return Math.abs(at(+date[1], +date[2], 0, 0) - expected); const [h, mi] = t.textContent.split(':').map(Number); const day = Math.floor(expected / 1440) * 1440; return Math.min(...[day - 1440, day, day + 1440].map(d => Math.abs(d + h * 60 + mi - expected))); }); return {said, ticks: ticks.length, off: Math.max(0, ...off)}; })()";

/// Where the lanes are on the screen: the hours' layer (the tracks' start and width) and the
/// middle of the stage lane's track, once the timeline is scrolled to the top of the window.
const PLACES: &str = "(() => { document.getElementById('timeline').scrollIntoView(); const h = document.querySelector('.tl-lanes .tl-hours').getBoundingClientRect(), s = document.querySelector('.tl-lane[data-lane=stage] .tl-track').getBoundingClientRect(); return {left: h.left, width: h.width, y: s.top + s.height / 2}; })()";

/// Whether the page scrolls sideways.
const SIDEWAYS: &str = "document.documentElement.scrollWidth > innerWidth";

/// The card's task open on a desk at 1440 wide, its lanes in the window: their places.
fn opened(chrome: &mut crate::chrome::Chrome, site: &Site) -> Value {
    open_drawn(chrome, site, 1440);
    let places = chrome.eval(PLACES);
    chrome.frames();
    places
}

/// The readout, once it has become what `test` holds, and its hours true of the lanes.
fn readout(chrome: &mut crate::chrome::Chrome, what: &str, test: &str) -> String {
    assert!(
        chrome.wait_for(&format!("(said => {test})({RANGE})"), SHOWN),
        "{what}: the readout is {}",
        chrome.eval(RANGE)
    );
    let truth = chrome.eval(TRUE);
    assert!(
        truth["ticks"].as_u64().unwrap() >= 2 && truth["off"].as_f64().unwrap() < 1.0,
        "{what}: the readout is not true of the hours drawn: {truth}"
    );
    assert_eq!(
        chrome.eval(SIDEWAYS),
        json!(false),
        "{what}: the page scrolls sideways"
    );
    truth["said"].as_str().unwrap().to_owned()
}

/// A mouse event at `x`, `y`, the left button held from a press to its release.
fn mouse(chrome: &mut crate::chrome::Chrome, kind: &str, x: f64, y: f64, clicks: u32) {
    chrome.call(
        "Input.dispatchMouseEvent",
        json!({"type": kind, "x": x, "y": y, "button": "left", "buttons": if kind == "mouseReleased" { 0 } else { 1 }, "clickCount": clicks}),
    );
}

/// A wheel's turn at `x`, `y` by `dx`, `dy` pixels with `modifiers` (Ctrl 2, Meta 4, Shift 8).
fn wheel(chrome: &mut crate::chrome::Chrome, x: f64, y: f64, dx: f64, dy: f64, modifiers: u32) {
    chrome.call(
        "Input.dispatchMouseEvent",
        json!({"type": "mouseWheel", "x": x, "y": y, "deltaX": dx, "deltaY": dy, "modifiers": modifiers}),
    );
    chrome.frames();
}

/// A press on the lanes at `from` dragged to `to`, each a fraction of the tracks, along `y`.
fn drag(chrome: &mut crate::chrome::Chrome, places: &Value, from: f64, to: f64) -> Value {
    let x = |fraction: f64| {
        places["left"].as_f64().unwrap() + fraction * places["width"].as_f64().unwrap()
    };
    let y = places["y"].as_f64().unwrap();
    mouse(chrome, "mouseMoved", x(from), y, 0);
    mouse(chrome, "mousePressed", x(from), y, 1);
    for step in 1..=4 {
        let at = from + (to - from) * f64::from(step) / 4.0;
        mouse(chrome, "mouseMoved", x(at), y, 0);
    }
    chrome.frames();
    let band = chrome.eval("(() => { const b = document.querySelector('.tl-band')?.getBoundingClientRect(); return b ? [b.left, b.width] : null; })()");
    mouse(chrome, "mouseReleased", x(to), y, 1);
    chrome.frames();
    band
}

/// The span the readout says, in minutes: its length before any "of".
fn minutes(said: &str) -> u64 {
    let length = said
        .split(" · ")
        .nth(1)
        .unwrap()
        .split(" of ")
        .next()
        .unwrap();
    let mut total = 0;
    let mut words = length.split(' ');
    while let (Some(count), Some(unit)) = (words.next(), words.next()) {
        let count: u64 = count.parse().unwrap();
        total += count
            * match unit {
                "d" => 1440,
                "h" => 60,
                _ => 1,
            };
    }
    total
}

#[test]
fn the_readout_says_a_span_in_minutes() {
    assert_eq!(minutes(WHOLE), 1620);
    assert_eq!(
        minutes("11-19 08:08 → 11-19 11:22 · 3 h 14 m of 1 d 3 h"),
        194
    );
    assert_eq!(minutes("a · 5 m of 1 d 3 h"), 5);
}

/// The card's task opens on the whole task; + and − zoom about the middle, each press the
/// timeline's alone, the overview there while zoomed; Whole task returns; the readout true at each.
#[test]
fn the_toolbar_zooms_in_and_out_and_whole_task_returns_with_a_true_readout() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    opened(&mut chrome, &site);
    readout(&mut chrome, "opened", &format!("said === {}", json!(WHOLE)));
    // Each button, whether it is drawn and takes the focus, and where it is: on the whole task
    // Whole task and − cannot act, so are not drawn, their places kept.
    let buttons = "[...document.querySelectorAll('.tl-zoom-group button')].map(b => { b.focus(); return [b.textContent, b.getAttribute('aria-label'), getComputedStyle(b).visibility === 'visible', document.activeElement === b, Math.round(b.getBoundingClientRect().left)]; })";
    let whole = chrome.eval(buttons);
    let shown = |buttons: &Value| -> Vec<Value> {
        buttons
            .as_array()
            .unwrap()
            .iter()
            .map(|b| json!([b[0], b[1], b[2], b[3]]))
            .collect()
    };
    assert_eq!(
        shown(&whole),
        [
            json!(["Whole task", null, false, false]),
            json!(["−", "Zoom out", false, false]),
            json!(["+", "Zoom in", true, true])
        ]
    );
    assert!(!drawn(&mut chrome, ".tl-overview"));
    // The toolbar line sits above the lanes, inside the box.
    let order = chrome.eval("(() => { const z = document.querySelector('.tl-zoom').getBoundingClientRect(), l = document.querySelector('.tl-lanes').getBoundingClientRect(); return z.bottom <= l.top && z.height <= 40; })()");
    assert_eq!(order, json!(true));
    tap(&mut chrome, ".tl-zoom button[aria-label='Zoom in']");
    let half = readout(&mut chrome, "zoomed in", "said.includes(' of 1 d 3 h')");
    assert_eq!(minutes(&half), 827, "{half}");
    // Zoomed, all three are drawn, and none moved: + is where it was pressed.
    let zoomed = chrome.eval(buttons);
    assert_eq!(
        shown(&zoomed),
        [
            json!(["Whole task", null, true, true]),
            json!(["−", "Zoom out", true, true]),
            json!(["+", "Zoom in", true, true])
        ]
    );
    for (now, then) in zoomed
        .as_array()
        .unwrap()
        .iter()
        .zip(whole.as_array().unwrap())
    {
        assert_eq!(now[4], then[4], "{now} moved");
    }
    assert_eq!(
        chrome.eval("document.getElementById('quick-open').getAttribute('aria-expanded')"),
        json!("false"),
        "a press in the timeline is the timeline's alone"
    );
    assert!(drawn(&mut chrome, ".tl-overview .tl-ov-window"));
    tap(&mut chrome, ".tl-zoom button[aria-label='Zoom in']");
    let quarter = readout(
        &mut chrome,
        "zoomed in twice",
        &format!("said !== {}", json!(half)),
    );
    assert_eq!(minutes(&quarter), 414, "{quarter}");
    tap(&mut chrome, ".tl-zoom button[aria-label='Zoom out']");
    let back = readout(
        &mut chrome,
        "zoomed out",
        &format!("said !== {}", json!(quarter)),
    );
    assert_eq!(back, half);
    tap(&mut chrome, ".tl-zoom-group button:first-child");
    readout(
        &mut chrome,
        "whole again",
        &format!("said === {}", json!(WHOLE)),
    );
    assert!(!drawn(&mut chrome, ".tl-overview"));
    assert_eq!(chrome.eval(buttons), whole);
}

/// ⌘/Ctrl + wheel zooms about the pointer, the instant under it staying there; Shift + wheel and
/// a sideways wheel move while zoomed; a plain wheel scrolls the page and leaves the view.
#[test]
fn a_wheel_zooms_about_the_pointer_moves_sideways_and_a_plain_wheel_scrolls_the_page() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    let places = opened(&mut chrome, &site);
    let y = places["y"].as_f64().unwrap();
    // The pointer on the stage that ends at 11:22, about a third of the way along, at a whole
    // pixel, as a wheel's event says its place.
    let stage = "document.querySelector('.tl-lane[data-lane=stage] .tl-seg[data-from][data-to]:nth-of-type(2)')";
    let x = chrome
        .eval(&format!("(() => {{ const b = {stage}.getBoundingClientRect(); return Math.round(b.left + b.width / 2); }})()"))
        .as_f64()
        .unwrap();
    // How much of the stage is before the pointer: a zoom about the pointer keeps it.
    let under = |chrome: &mut crate::chrome::Chrome| {
        chrome
            .eval(&format!("(() => {{ const b = {stage}.getBoundingClientRect(); return ({x} - b.left) / b.width; }})()"))
            .as_f64()
            .unwrap()
    };
    let share = under(&mut chrome);
    wheel(&mut chrome, x, y, 0.0, -300.0, 2);
    let zoomed = readout(&mut chrome, "ctrl + wheel", "said.includes(' of ')");
    assert_eq!(minutes(&zoomed), 827, "{zoomed}");
    assert!(
        (under(&mut chrome) - share).abs() < 0.002,
        "the stage under the pointer moved"
    );
    // ⌘ too, and a trackpad's pinch, a little at a time.
    wheel(&mut chrome, x, y, 0.0, -300.0, 4);
    let closer = readout(
        &mut chrome,
        "meta + wheel",
        &format!("said !== {}", json!(zoomed)),
    );
    assert_eq!(minutes(&closer), 414, "{closer}");
    for _ in 0..10 {
        wheel(&mut chrome, x, y, 0.0, 6.0, 2);
    }
    let pinched = readout(
        &mut chrome,
        "a pinch out",
        &format!("said !== {}", json!(closer)),
    );
    assert!(
        minutes(&pinched) > 414 && minutes(&pinched) < 827,
        "{pinched}"
    );
    assert!(
        (under(&mut chrome) - share).abs() < 0.002,
        "the stage under the pointer moved"
    );
    // Shift + wheel moves later, as long.
    let before = chrome.eval(RANGE);
    wheel(&mut chrome, x, y, 0.0, 200.0, 8);
    let shifted = readout(&mut chrome, "shift + wheel", &format!("said !== {before}"));
    assert_eq!(minutes(&shifted), minutes(&pinched));
    assert!(shifted > pinched, "{shifted} is not later than {pinched}");
    // A sideways wheel moves too, back.
    wheel(&mut chrome, x, y, -200.0, 0.0, 0);
    let back = readout(
        &mut chrome,
        "a sideways wheel",
        &format!("said !== {}", json!(shifted)),
    );
    assert_eq!(minutes(&back), minutes(&pinched));
    assert!(back < shifted);
    // A plain wheel scrolls the page, the view as it was.
    let scrolled = chrome.eval("scrollY").as_f64().unwrap();
    wheel(&mut chrome, x, y, 0.0, 300.0, 0);
    assert!(chrome.wait_for(&format!("scrollY > {scrolled}"), SHOWN));
    assert_eq!(chrome.eval(RANGE), json!(back));
}

/// A drag across the hours draws its band, zooms to the hours dragged, and its release picks no
/// message, though it began on one's tick.
#[test]
fn a_drag_across_the_hours_zooms_to_them_and_its_release_picks_nothing() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    let mut places = opened(&mut chrome, &site);
    // Begun on the first message's tick, on the orchestrator's line.
    let tick = chrome.eval("(() => { const b = document.querySelector('.tl-message-mark').getBoundingClientRect(); return [b.left + b.width / 2, b.top + b.height / 2]; })()");
    let from = (tick[0].as_f64().unwrap() - places["left"].as_f64().unwrap())
        / places["width"].as_f64().unwrap();
    places["y"] = tick[1].clone();
    let band = drag(&mut chrome, &places, from, from + 0.25);
    let width = places["width"].as_f64().unwrap();
    assert!(
        (band[1].as_f64().unwrap() - 0.25 * width).abs() < 1.5,
        "the band is {band}, not a quarter of {width}"
    );
    let dragged = readout(&mut chrome, "dragged", "said.includes(' of ')");
    // A quarter of the whole hours, 27 h 35 m, to the minute.
    assert!(
        minutes(&dragged).abs_diff(414) <= 1,
        "{dragged} is not a quarter of the whole"
    );
    assert!(dragged.starts_with("11-19 08:0"), "{dragged}");
    assert_eq!(
        chrome.eval("[document.querySelectorAll('.tl-message-mark[aria-pressed=true]').length, document.querySelector('.tl-band'), document.getElementById('tl-message-choice')?.value ?? null]"),
        json!([0, null, ""])
    );
    // A press that does not move is a click: it picks the tick it is on.
    tap(&mut chrome, ".tl-message-mark:not(.tl-out)");
    assert_eq!(
        chrome.eval("document.querySelectorAll('.tl-message-mark[aria-pressed=true]').length"),
        json!(1)
    );
    assert_eq!(chrome.eval(RANGE), json!(dragged));
    // A press let go below the lanes before it dragged leaves nothing held: back over the lanes
    // with no button down, further across than a drag needs, the pointer draws no band. The
    // lanes' places again, the overview now above them.
    let places = chrome.eval(PLACES);
    let x = places["left"].as_f64().unwrap() + places["width"].as_f64().unwrap() / 2.0;
    let y = places["y"].as_f64().unwrap();
    mouse(&mut chrome, "mousePressed", x, y, 1);
    mouse(&mut chrome, "mouseMoved", x + 1.0, y + 500.0, 0);
    mouse(&mut chrome, "mouseReleased", x + 1.0, y + 500.0, 1);
    for dx in [50.0, 150.0] {
        chrome.call(
            "Input.dispatchMouseEvent",
            json!({"type": "mouseMoved", "x": x + dx, "y": y, "buttons": 0}),
        );
    }
    chrome.frames();
    assert_eq!(
        chrome.eval("document.querySelector('.tl-band') === null"),
        json!(true)
    );
    assert_eq!(chrome.eval(RANGE), json!(dragged));
}

/// A double click on a stage's bar zooms to the stage: it fills the track, and the readout says
/// its start, its end and its length.
#[test]
fn a_double_click_on_a_stage_zooms_to_it() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    let places = opened(&mut chrome, &site);
    let stage = "document.querySelector('.tl-lane[data-lane=stage] .tl-seg[data-from][data-to]:nth-of-type(2)')";
    let at = chrome.eval(&format!(
        "(() => {{ const b = {stage}.getBoundingClientRect(); return b.left + b.width / 2; }})()"
    ));
    let (x, y) = (at.as_f64().unwrap(), places["y"].as_f64().unwrap());
    for clicks in [1, 2] {
        mouse(&mut chrome, "mousePressed", x, y, clicks);
        mouse(&mut chrome, "mouseReleased", x, y, clicks);
    }
    chrome.frames();
    let said = readout(
        &mut chrome,
        "a stage double clicked",
        "said.includes(' of ')",
    );
    assert_eq!(said, "11-19 08:08 → 11-19 11:22 · 3 h 14 m of 1 d 3 h");
    let fills = chrome.eval(&format!("(() => {{ const b = {stage}.getBoundingClientRect(), t = {stage}.parentElement.getBoundingClientRect(); return [Math.round(b.left - t.left), Math.round(t.right - b.right)]; }})()"));
    assert_eq!(fills, json!([0, 0]));
}

/// With the focus in the lanes, + and − zoom and 0 shows the whole task; a part the arrow keys
/// step to outside the view comes into view, as Home and End do.
#[test]
fn the_keys_zoom_and_a_part_stepped_to_outside_the_view_comes_into_view() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    opened(&mut chrome, &site);
    chrome.eval("document.getElementById('tl-stop-0').focus()");
    components::key(&mut chrome, "+", "Equal", 187);
    let half = readout(&mut chrome, "+", "said.includes(' of ')");
    assert_eq!(minutes(&half), 827);
    components::key(&mut chrome, "+", "Equal", 187);
    let quarter = readout(&mut chrome, "+ again", &format!("said !== {}", json!(half)));
    assert_eq!(minutes(&quarter), 414);
    // The part with the focus, whether it is drawn, and whether it is on the track.
    let focused = "(() => { const e = document.activeElement, b = e.getBoundingClientRect(), t = e.closest('.tl-track').getBoundingClientRect(); return {id: e.id, out: e.classList.contains('tl-out'), on: b.right > t.left && b.left < t.right, seen: getComputedStyle(e).opacity}; })()";
    for (key, code, vk, id) in [
        ("End", "End", 35, "last"),
        ("Home", "Home", 36, "tl-stop-0"),
    ] {
        components::key(&mut chrome, key, code, vk);
        chrome.frames();
        let at = chrome.eval(focused);
        if id != "last" {
            assert_eq!(at["id"], json!(id));
        }
        assert_eq!(
            [&at["out"], &at["on"], &at["seen"]],
            [&json!(false), &json!(true), &json!("1")],
            "{key}: {at}"
        );
        assert_eq!(minutes(&readout(&mut chrome, key, "true")), 414, "{key}");
    }
    // Each arrow stays in view, stepping on through the stage lane.
    for _ in 0..5 {
        components::key(&mut chrome, "ArrowRight", "ArrowRight", 39);
        chrome.frames();
        let at = chrome.eval(focused);
        assert_eq!(
            [&at["out"], &at["on"]],
            [&json!(false), &json!(true)],
            "{at}"
        );
    }
    components::key(&mut chrome, "-", "Minus", 189);
    let out = readout(&mut chrome, "-", "said.includes(' of ')");
    assert_eq!(minutes(&out), 827);
    components::key(&mut chrome, "0", "Digit0", 48);
    readout(&mut chrome, "0", &format!("said === {}", json!(WHOLE)));
}

/// A message chosen in the message list outside the view comes into view: its tick drawn on the
/// track (the last message's at its very end), its arrow from it.
#[test]
fn a_message_chosen_in_the_list_outside_the_view_comes_into_view() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    opened(&mut chrome, &site);
    // Zoomed to the first hours, which no message is in.
    tap(&mut chrome, ".tl-lane[data-lane=stage] .tl-seg:first-child");
    chrome.eval("document.getElementById('tl-stop-0').focus()");
    for _ in 0..3 {
        components::key(&mut chrome, "+", "Equal", 187);
    }
    components::key(&mut chrome, "Home", "Home", 36);
    let early = readout(&mut chrome, "zoomed early", "said.includes(' of ')");
    assert!(early.starts_with("11-19 01:"), "{early}");
    let tick = "(() => { const e = document.querySelector('.tl-message-mark[data-message=\"17313\"]'), b = e.getBoundingClientRect(), t = e.parentElement.getBoundingClientRect(); const middle = b.left + b.width / 2; return {out: e.classList.contains('tl-out'), on: middle >= t.left - 0.5 && middle <= t.right + 0.5, arrow: document.querySelector('.tl-address-track .tl-address-arrow, .tl-address-track .tl-ring-around') !== null}; })()";
    assert_eq!(chrome.eval(tick)["out"], json!(true));
    messages::pick(&mut chrome, 17313);
    let later = readout(
        &mut chrome,
        "the last message chosen",
        &format!("said !== {}", json!(early)),
    );
    assert!(later.contains("11-20 04:"), "{later}");
    assert_eq!(minutes(&later), minutes(&early));
    assert!(chrome.wait_for(&format!("{tick}.arrow"), SHOWN));
    assert_eq!(
        chrome.eval(tick),
        json!({"out": false, "on": true, "arrow": true})
    );
}

/// While zoomed, the overview's window shows where the view is on the whole task: a drag of it
/// moves the view, a drag of its edge resizes it, its arrow keys move it.
#[test]
fn the_overview_shows_while_zoomed_and_its_window_moves_and_resizes_the_view() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    opened(&mut chrome, &site);
    tap(&mut chrome, ".tl-zoom button[aria-label='Zoom in']");
    tap(&mut chrome, ".tl-zoom button[aria-label='Zoom in']");
    let start = readout(&mut chrome, "zoomed", "said.includes(' of ')");
    let window = "(() => { const w = document.querySelector('.tl-ov-window').getBoundingClientRect(), o = document.querySelector('.tl-overview').getBoundingClientRect(), h = document.querySelector('.tl-lanes .tl-hours').getBoundingClientRect(); return {left: w.left, right: w.right, y: w.top + w.height / 2, strip: [o.left, o.width], hours: [h.left, h.width]}; })()";
    let at = chrome.eval(window);
    // The strip lies over the tracks, the window a quarter of it, in its middle.
    let strip = at["strip"][1].as_f64().unwrap();
    assert!((at["strip"][0].as_f64().unwrap() - at["hours"][0].as_f64().unwrap()).abs() < 1.5);
    assert!((strip - at["hours"][1].as_f64().unwrap()).abs() < 1.5);
    let (left, right) = (at["left"].as_f64().unwrap(), at["right"].as_f64().unwrap());
    assert!(((right - left) - strip / 4.0).abs() < 2.0, "{at}");
    let y = at["y"].as_f64().unwrap();
    // A drag of the window, 100 px later.
    let middle = (left + right) / 2.0;
    mouse(&mut chrome, "mouseMoved", middle, y, 0);
    mouse(&mut chrome, "mousePressed", middle, y, 1);
    mouse(&mut chrome, "mouseMoved", middle + 50.0, y, 0);
    mouse(&mut chrome, "mouseMoved", middle + 100.0, y, 0);
    mouse(&mut chrome, "mouseReleased", middle + 100.0, y, 1);
    let left_at = |left: f64| {
        format!(
            "Math.abs(document.querySelector('.tl-ov-window').getBoundingClientRect().left - {left}) < 1.5"
        )
    };
    assert!(
        chrome.wait_for(&left_at(left + 100.0), SHOWN),
        "the window is not 100 px later: {}",
        chrome.eval(window)
    );
    let moved = readout(
        &mut chrome,
        "the window dragged",
        &format!("said !== {}", json!(start)),
    );
    assert_eq!(minutes(&moved), minutes(&start));
    let now = chrome.eval(window);
    // A drag of its end edge, 60 px later: longer, from the same start.
    let edge = now["right"].as_f64().unwrap() - 1.0;
    mouse(&mut chrome, "mouseMoved", edge, y, 0);
    mouse(&mut chrome, "mousePressed", edge, y, 1);
    mouse(&mut chrome, "mouseMoved", edge + 60.0, y, 0);
    mouse(&mut chrome, "mouseReleased", edge + 60.0, y, 1);
    let right_at = format!(
        "Math.abs(document.querySelector('.tl-ov-window').getBoundingClientRect().right - {}) < 1.5",
        edge + 61.0
    );
    assert!(
        chrome.wait_for(&right_at, SHOWN),
        "the window's end is not 60 px later: {}",
        chrome.eval(window)
    );
    let wider = readout(
        &mut chrome,
        "the edge dragged",
        &format!("said !== {}", json!(moved)),
    );
    assert_eq!(
        wider.split(" → ").next(),
        moved.split(" → ").next(),
        "{wider}"
    );
    assert!(minutes(&wider) > minutes(&moved), "{wider}");
    // Its keys move it a tenth of the hours shown.
    chrome.eval("document.querySelector('.tl-ov-window').focus()");
    components::key(&mut chrome, "ArrowLeft", "ArrowLeft", 37);
    let keyed = readout(
        &mut chrome,
        "its arrow key",
        &format!("said !== {}", json!(wider)),
    );
    assert_eq!(minutes(&keyed), minutes(&wider));
    assert!(keyed < wider);
    components::key(&mut chrome, "0", "Digit0", 48);
    readout(
        &mut chrome,
        "0 on the window",
        &format!("said === {}", json!(WHOLE)),
    );
    assert!(!drawn(&mut chrome, ".tl-overview"));
}

/// On a wide touch screen two fingers pinch the view, one swiped sideways moves it while zoomed,
/// and nothing scrolls the page sideways.
#[test]
fn a_touch_screens_pinch_zooms_and_a_sideways_swipe_moves_while_zoomed() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    chrome.call(
        "Emulation.setTouchEmulationEnabled",
        json!({"enabled": true, "maxTouchPoints": 5}),
    );
    let places = opened(&mut chrome, &site);
    let y = places["y"].as_f64().unwrap();
    let x = places["left"].as_f64().unwrap() + places["width"].as_f64().unwrap() / 2.0;
    let touch = |chrome: &mut crate::chrome::Chrome, kind: &str, points: Value| {
        chrome.call(
            "Input.dispatchTouchEvent",
            json!({"type": kind, "touchPoints": points}),
        );
    };
    // Two fingers 200 px apart spread to 400: the hours shown halve about their middle.
    touch(
        &mut chrome,
        "touchStart",
        json!([{"x": x - 100.0, "y": y, "id": 0}, {"x": x + 100.0, "y": y, "id": 1}]),
    );
    for spread in [125.0, 150.0, 175.0, 200.0] {
        touch(
            &mut chrome,
            "touchMove",
            json!([{"x": x - spread, "y": y, "id": 0}, {"x": x + spread, "y": y, "id": 1}]),
        );
    }
    touch(&mut chrome, "touchEnd", json!([]));
    chrome.frames();
    let pinched = readout(&mut chrome, "pinched", "said.includes(' of ')");
    assert!(minutes(&pinched).abs_diff(827) <= 1, "{pinched}");
    // One finger swiped left: later, as long.
    touch(
        &mut chrome,
        "touchStart",
        json!([{"x": x, "y": y, "id": 0}]),
    );
    for step in 1..=5 {
        touch(
            &mut chrome,
            "touchMove",
            json!([{"x": x - 40.0 * f64::from(step), "y": y + 2.0, "id": 0}]),
        );
    }
    touch(&mut chrome, "touchEnd", json!([]));
    chrome.frames();
    let swiped = readout(
        &mut chrome,
        "swiped",
        &format!("said !== {}", json!(pinched)),
    );
    assert_eq!(minutes(&swiped), minutes(&pinched));
    assert!(swiped > pinched, "{swiped} is not later than {pinched}");
    assert_eq!(chrome.eval("scrollX"), json!(0));
    tap(&mut chrome, ".tl-zoom-group button:first-child");
    readout(
        &mut chrome,
        "whole again",
        &format!("said === {}", json!(WHOLE)),
    );
}

/// On a phone the timeline keeps its list: no toolbar, no overview, and nothing on the page
/// scrolls sideways.
#[test]
fn on_a_phone_the_timeline_keeps_its_list_with_no_toolbar() {
    let site = guards_on(site());
    let Some(mut chrome) = chrome() else { return };
    open_drawn(&mut chrome, &site, 390);
    assert!(drawn(&mut chrome, ".tl-drawn .tl-list"));
    for part in [".tl-zoom", ".tl-overview", ".tl-lanes"] {
        assert!(!drawn(&mut chrome, part), "{part} is drawn on a phone");
    }
    assert_eq!(chrome.eval(SIDEWAYS), json!(false));
}

/// A task whose hours are no longer than a zoom's least (here made half a minute after its hour,
/// done two minutes later) draws its lanes as before: no toolbar, nothing to zoom.
#[test]
fn a_task_too_short_to_zoom_into_draws_no_toolbar() {
    let mut brief = fixture();
    let hour = (brief["created"].as_f64().unwrap() / 3600.0).floor() * 3600.0;
    let (made, done) = (hour + 30.0, hour + 150.0);
    brief["zone"] = json!([]);
    brief["created"] = json!(made);
    brief["now"] = json!(done);
    brief["closed"] = json!(done);
    brief["first_claimed"] = json!(made);
    brief["stages"] = json!([
        {"status": "in_progress", "from": made, "to": done, "by": "quiet-wren"},
        {"status": "done", "from": done, "by": "quiet-wren"}
    ]);
    brief["claims"] = json!([{"by": "quiet-wren", "from": made, "to": done}]);
    brief["rounds"] = json!([]);
    brief["runs"] = json!([]);
    brief["gates"] = json!([]);
    let site = served(&brief);
    let Some(mut chrome) = chrome() else { return };
    open_drawn(&mut chrome, &site, 1440);
    assert!(drawn(&mut chrome, ".tl-lanes"));
    assert!(!drawn(&mut chrome, ".tl-zoom"));
    assert_eq!(
        chrome.eval("[document.querySelector('.tl-lanes').className, document.querySelectorAll('.tl-lanes [data-from]').length]"),
        json!(["tl-lanes", 0])
    );
}
