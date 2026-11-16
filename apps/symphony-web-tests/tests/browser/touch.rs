//! What a touch screen needs of the pages, read in the release client: a text field is drawn at
//! 16px there, as iOS zooms into a smaller one when it takes focus and does not zoom back out, and
//! keeps its size on a desk.
use super::components::site;
use super::{SHOWN, chrome, chrome::Chrome};
use serde_json::json;

const TASK: &str = "/task/visual/in-progress";

/// A phone of 390 by 844 px that only touches, or a desk of 1280 by 900 px with a mouse.
fn device(page: &mut Chrome, touch: bool) {
    page.call(
        "Emulation.setTouchEmulationEnabled",
        json!({"enabled":touch,"maxTouchPoints":5}),
    );
    let (width, height) = if touch { (390, 844) } else { (1280, 900) };
    page.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":1,"mobile":touch}),
    );
}

/// The font size, in px, of the first element `selector` names once it is in the page.
fn font_size(page: &mut Chrome, selector: &str) -> f64 {
    assert!(
        page.wait_for(
            &format!("document.querySelector({}) !== null", json!(selector)),
            SHOWN
        ),
        "{selector} is drawn"
    );
    page.eval(&format!(
        "parseFloat(getComputedStyle(document.querySelector({})).fontSize)",
        json!(selector)
    ))
    .as_f64()
    .expect("a font size")
}

#[test]
fn a_text_field_is_sixteen_pixels_on_a_touch_screen_and_keeps_its_size_on_a_desk() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    for (touch, size) in [(true, 16.0), (false, 14.0)] {
        device(&mut page, touch);
        page.open(&site.url("/tasks"));
        assert_eq!(
            page.eval("matchMedia('(pointer: coarse)').matches"),
            json!(touch),
            "the setup emulates a touch screen, or a mouse"
        );
        let search = font_size(&mut page, "input[type=search]");
        page.open(&site.url(TASK));
        // The composer mounts as it nears the window, below the brief and the task's timeline.
        assert!(page.wait_for(
            "document.querySelector('[id^=\"composer-\"]') !== null",
            SHOWN
        ));
        page.eval("document.querySelector('[id^=\"composer-\"]').scrollIntoView({block:'center'})");
        let composer = font_size(&mut page, "#task-send textarea");
        assert_eq!(
            [search, composer],
            [size; 2],
            "touch {touch}: the board's search field and a task's composer, in px"
        );
    }
}
