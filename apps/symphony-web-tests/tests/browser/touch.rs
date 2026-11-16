//! What a touch screen needs of the pages, read in the release client: a text field is drawn at
//! 16px there, as iOS zooms into a smaller one when it takes focus and does not zoom back out, and
//! keeps its size on a desk; a press is the control's own, which sinks it, never the browser's tap
//! flash or a selected label, and a hover that moves or raises a thing is a pointer's alone.
use super::components::site;
use super::{SHOWN, chrome, chrome::Chrome};
use serde_json::{Value, json};

const TASK: &str = "/task/visual/in-progress";
/// `scale(.97)`, as a computed transform.
const PRESSED: &str = "matrix(0.97, 0, 0, 0.97, 0, 0)";

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

/// The first element `selector` names, as DevTools knows it until the next one is asked for.
fn node(page: &mut Chrome, selector: &str) -> Value {
    page.call("DOM.enable", json!({}));
    page.call("CSS.enable", json!({}));
    let root = page.call("DOM.getDocument", json!({"depth":0}))["root"]["nodeId"].clone();
    let node = page.call(
        "DOM.querySelector",
        json!({"nodeId":root,"selector":selector}),
    )["nodeId"]
        .clone();
    assert_ne!(node, json!(0), "{selector} is in the page");
    node
}

/// `node` drawn in the states `states` (`active`, `hover`), as DevTools forces them, whatever the
/// pointer does; none lifts what was forced on it before.
fn force(page: &mut Chrome, node: &Value, states: &[&str]) {
    page.call(
        "CSS.forcePseudoState",
        json!({"nodeId":node,"forcedPseudoClasses":states}),
    );
}

/// The computed `property` of the first element `selector` names, and how many animations and
/// transitions it runs: read together, so a rule that applies has started its transition.
fn look(page: &mut Chrome, selector: &str, property: &str) -> Value {
    page.eval(&format!(
        "(e => [getComputedStyle(e)[{}], e.getAnimations().length])(document.querySelector({}))",
        json!(property),
        json!(selector)
    ))
}

/// Opens `path` and waits for what `selector` names.
fn open(page: &mut Chrome, site: &super::site::Site, path: &str, selector: &str) {
    page.open(&site.url(path));
    assert!(
        page.wait_for(
            &format!("document.querySelector({}) !== null", json!(selector)),
            SHOWN
        ),
        "{path} draws {selector}"
    );
}

#[test]
fn a_press_is_the_controls_own_and_selects_none_of_its_label() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    device(&mut page, true);
    open(&mut page, &site, "/", ".tile");
    assert_eq!(
        page.eval("(e => [getComputedStyle(document.documentElement).webkitTapHighlightColor, getComputedStyle(e('button')).userSelect, getComputedStyle(e('.dx-button')).userSelect, getComputedStyle(e('.tile')).userSelect])(s => document.querySelector(s))"),
        json!(["rgba(0, 0, 0, 0)", "none", "none", "none"]),
        "no tap flash, and a long press on a button, a component button or a tile selects nothing"
    );
    assert_eq!(
        page.eval("getComputedStyle(document.querySelector('.dx-button')).transitionProperty"),
        json!("background-color, border-color, color, box-shadow, opacity, transform"),
        "a component button eases what its states change, not every property"
    );
    for selector in ["button:not(.dx-button)", ".dx-button", ".tile"] {
        let pressed = node(&mut page, selector);
        force(&mut page, &pressed, &["active"]);
        assert!(
            page.wait_for(
                &format!(
                    "getComputedStyle(document.querySelector({})).transform === {}",
                    json!(selector),
                    json!(PRESSED)
                ),
                SHOWN
            ),
            "a press sinks {selector}: {}",
            look(&mut page, selector, "transform")
        );
        force(&mut page, &pressed, &[]);
        assert!(
            page.wait_for(
                &format!(
                    "getComputedStyle(document.querySelector({})).transform === 'none'",
                    json!(selector)
                ),
                SHOWN
            ),
            "{selector} comes back up once let go"
        );
    }
    // A disabled button answers no press.
    page.eval("document.querySelector('button:not(.dx-button)').disabled = true");
    let disabled = node(&mut page, "button:not(.dx-button)");
    force(&mut page, &disabled, &["active"]);
    assert_eq!(
        look(&mut page, "button:not(.dx-button)", "transform"),
        json!(["none", 0])
    );
}

#[test]
fn a_hover_that_moves_or_raises_a_thing_is_a_pointers_alone() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    for (path, selector, property) in [
        ("/", ".tile", "transform"),
        ("/tasks", ".taskcard", "boxShadow"),
    ] {
        // On a touch screen a tap leaves the thing hovered: it stays as it rests.
        device(&mut page, true);
        open(&mut page, &site, path, selector);
        let rest = look(&mut page, selector, property);
        let hovered = node(&mut page, selector);
        force(&mut page, &hovered, &["hover"]);
        assert_eq!(
            look(&mut page, selector, property),
            rest,
            "a touch screen's hover leaves {selector} as it rests"
        );
        // Under a mouse it rises.
        device(&mut page, false);
        open(&mut page, &site, path, selector);
        let rest = look(&mut page, selector, property);
        let hovered = node(&mut page, selector);
        force(&mut page, &hovered, &["hover"]);
        assert!(
            page.wait_for(
                &format!(
                    "(e => getComputedStyle(e)[{}] !== {} && e.getAnimations().length === 0)(document.querySelector({}))",
                    json!(property),
                    rest[0],
                    json!(selector)
                ),
                SHOWN
            ),
            "a mouse's hover moves or raises {selector}"
        );
    }
}
