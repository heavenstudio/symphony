//! Mount expensive content once, just before scrolling it into view. Keeping it mounted after
//! that first visit preserves open forms and media. The sentinel extends 600 px each way.

use dioxus::prelude::*;

/// The one visibility latch for sections and native media with their own reserved layout.
pub fn use_visibility(initial: bool) -> (Signal<bool>, Vec<Attribute>) {
    #[cfg(target_arch = "wasm32")]
    {
        crate::entry_events::use_visibility(initial)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut shown = use_signal(|| initial || cfg!(not(target_arch = "wasm32")));
        let reveal = use_callback(move |event: Event<VisibleData>| {
            if event.is_intersecting().unwrap_or(false) {
                shown.set(true);
            }
        });
        (shown, vec![dioxus::html::onvisible(reveal)])
    }
}

#[component]
pub fn NearViewport(
    children: Element,
    #[props(default)] anchors: Vec<String>,
    #[props(default = 160)] height: u32,
    #[props(default)] id: String,
    #[props(default)] class: String,
    #[props(default)] list_item: bool,
    onenter: Option<EventHandler<()>>,
) -> Element {
    #[cfg(target_arch = "wasm32")]
    let target = web_sys::window()
        .and_then(|w| w.location().hash().ok())
        .and_then(|hash| js_sys::decode_uri_component(hash.trim_start_matches('#')).ok())
        .map(String::from)
        .unwrap_or_default();
    #[cfg(target_arch = "wasm32")]
    let addressed = !target.is_empty() && (anchors.contains(&target) || target == id);
    #[cfg(not(target_arch = "wasm32"))]
    let addressed = false;
    let (shown, reveal) = use_visibility(addressed);
    let fired = use_hook(|| std::rc::Rc::new(std::cell::Cell::new(false)));
    use_effect(move || {
        if shown() {
            #[cfg(target_arch = "wasm32")]
            if addressed {
                crate::navigation::scroll_anchor();
            }
            if !fired.replace(true)
                && let Some(handler) = onenter
            {
                handler.call(());
            }
        }
    });
    let content = rsx! {
            if !shown() {
                div { class: "viewport-sentinel", "aria-hidden": "true", ..reveal }
                div { class: "viewport-placeholder", style: "min-height:{height}px", "aria-hidden": "true", "data-pending": "true",
                    for anchor in anchors { span { id: anchor } }
                }
            } else {
                {children}
            }
    };
    let style = format!(
        "contain-intrinsic-size:auto {height}px;min-height:{}px",
        if shown() { 0 } else { height }
    );
    if list_item {
        rsx! { li { id, style, class: "viewport-slot {class}", {content} } }
    } else {
        rsx! { div { id, style, class: "viewport-slot {class}", {content} } }
    }
}

/// The longest a page stays under its skeleton waiting for its first screen's reads, in ms.
pub const SETTLE_MS: f64 = 3_000.0;

/// Whether a reserved region's box (`[top, bottom, left, right]`) shows on `screen`, the part of
/// the viewport its page covers, in the same terms. A box with no area (a closed `details`, a
/// hidden tab) shows nowhere.
pub fn on_screen(region: [f64; 4], screen: [f64; 4]) -> bool {
    let [top, bottom, left, right] = region;
    bottom > top
        && right > left
        && bottom > screen[0]
        && top < screen[1]
        && right > screen[2]
        && left < screen[3]
}

/// A region waiting for its read: bars where its lines will be, shimmering only for a reader who
/// accepts motion, and `label` for one who reads the page's text instead.
#[component]
pub fn Skeleton(#[props(default = 3)] lines: u8, #[props(default)] label: String) -> Element {
    rsx! { div { class: "skeleton",
        if !label.is_empty() { span { class: "status-text", role: "status", "{label}" } }
        for line in 0..lines { span { key: "{line}", class: "bar", "aria-hidden": "true" } }
    } }
}

/// Draw `children` hidden under a skeleton until no region they reserve on the first screen
/// (`[data-pending=true]`) waits for its read any longer, until `failed`, or for [`SETTLE_MS`]
/// at most: the reader's first screen arrives whole, and no read that lands moves it.
#[component]
pub fn Settle(children: Element, #[props(default)] failed: bool) -> Element {
    #[cfg(target_arch = "wasm32")]
    let (settled, id) = use_settled(failed);
    #[cfg(not(target_arch = "wasm32"))]
    let (settled, id) = (use_signal(|| true), String::new());
    rsx! { div { class: "settle", "data-settle": id, "data-settled": "{settled}",
        if !settled() { Skeleton { lines: 8, label: "Loading…" } }
        {children}
    } }
}

/// The page's settled latch: it checks each frame, from the first its root is drawn in, whether
/// a pending region is still on the screen.
#[cfg(target_arch = "wasm32")]
fn use_settled(failed: bool) -> (Signal<bool>, String) {
    let id = use_hook(crate::entry_events::marker);
    let mut settled = use_signal(|| false);
    use_effect(use_reactive!(|failed| {
        if failed {
            settled.set(true);
        }
    }));
    let selector = format!("[data-settle='{id}']");
    use_hook(move || {
        spawn(async move {
            let mut drawn = None;
            while !*settled.peek() {
                crate::live::sleep(16).await;
                let Some(root) = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.query_selector(&selector).ok().flatten())
                else {
                    continue;
                };
                let since = *drawn.get_or_insert_with(js_sys::Date::now);
                if !waiting(&root) || js_sys::Date::now() - since > SETTLE_MS {
                    settled.set(true);
                }
            }
        });
    });
    (settled, id)
}

/// Whether `root` has not drawn its page yet (only its skeleton is in it), or a region of it on
/// the screen still waits for its read.
#[cfg(target_arch = "wasm32")]
fn waiting(root: &web_sys::Element) -> bool {
    if root.child_element_count() < 2 {
        return true;
    }
    let Some(window) = web_sys::window() else {
        return false;
    };
    let page = root.get_bounding_client_rect();
    let screen = [
        0.0,
        size(window.inner_height()),
        page.left().max(0.0),
        page.right().min(size(window.inner_width())),
    ];
    root.query_selector_all("[data-pending=true]")
        .is_ok_and(|pending| any_shows(&pending, screen))
}

/// A window dimension in CSS pixels, 0 when the browser gives none.
#[cfg(target_arch = "wasm32")]
fn size(value: Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>) -> f64 {
    value.ok().and_then(|v| v.as_f64()).unwrap_or_default()
}

/// Whether any of the reserved regions in `pending` shows on `screen`.
#[cfg(target_arch = "wasm32")]
fn any_shows(pending: &web_sys::NodeList, screen: [f64; 4]) -> bool {
    use wasm_bindgen::JsCast;
    (0..pending.length())
        .filter_map(|index| pending.item(index)?.dyn_into::<web_sys::Element>().ok())
        .any(|region| {
            let r = region.get_bounding_client_rect();
            on_screen([r.top(), r.bottom(), r.left(), r.right()], screen)
        })
}

/// The most frames a live update's hold lasts, about half a second.
const HOLD_FRAMES: u8 = 30;

/// Whether a hold goes on after `frame` frames, the last `quiet` of them with nothing moved,
/// while a reserved box on the screen still waits (`pending`) to draw the row it stands for.
pub fn holding(frame: u8, quiet: u8, pending: bool) -> bool {
    frame < HOLD_FRAMES && (quiet < 2 || pending)
}

/// The reader's own moves, which end a hold: it never scrolls against them.
#[cfg(target_arch = "wasm32")]
const GESTURES: [&str; 4] = ["wheel", "touchstart", "mousedown", "keydown"];

/// Keep what the reader looks at in place across a live update about to be drawn: the focused
/// field if it is on the screen, else the first of `rows` on it. The browser's own scroll
/// anchoring keeps neither a field being typed in below a message that lands, nor a row in a
/// scroller still at its top (the board's columns). So each frame once the update is drawn, until
/// nothing has moved for two frames and no reserved box on the screen still waits to draw its row,
/// the scroller around that one and then the page scroll by however far it moved. The reader's
/// own scroll, click or key (typing in a field aside) ends the hold.
#[cfg(target_arch = "wasm32")]
pub fn hold_place(rows: &str) {
    use wasm_bindgen::{JsCast, closure::Closure};
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let height = size(window.inner_height());
    let shows = |element: &web_sys::Element| {
        let top = element.get_bounding_client_rect().top();
        (0.0..height).contains(&top)
    };
    let focused = document
        .active_element()
        .filter(|e| e.matches("input, textarea, select").unwrap_or(false) && shows(e));
    let anchor = focused.or_else(|| {
        let all = document.query_selector_all(rows).ok()?;
        (0..all.length())
            .filter_map(|index| all.item(index)?.dyn_into::<web_sys::Element>().ok())
            .find(|row| shows(row))
    });
    let Some(anchor) = anchor else {
        return;
    };
    let stopped = std::rc::Rc::new(std::cell::Cell::new(false));
    let reader = stopped.clone();
    let gesture = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        let typing = event.type_() == "keydown"
            && event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
                .is_some_and(|field| {
                    field
                        .matches("input, textarea, select, [contenteditable]")
                        .unwrap_or(false)
                });
        if !typing {
            reader.set(true);
        }
    });
    for kind in GESTURES {
        let _ = window.add_event_listener_with_callback_and_bool(
            kind,
            gesture.as_ref().unchecked_ref(),
            true,
        );
    }
    let top = anchor.get_bounding_client_rect().top();
    restore_place(Hold {
        window,
        anchor,
        top,
        frame: 0,
        quiet: 0,
        stopped,
        gesture,
    });
}

/// A hold under way: the row or field kept at `top`, and the listener for the reader's moves.
#[cfg(target_arch = "wasm32")]
struct Hold {
    window: web_sys::Window,
    anchor: web_sys::Element,
    top: f64,
    frame: u8,
    quiet: u8,
    stopped: std::rc::Rc<std::cell::Cell<bool>>,
    gesture: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>,
}

#[cfg(target_arch = "wasm32")]
fn restore_place(mut hold: Hold) {
    use wasm_bindgen::{JsCast, closure::Closure};
    let window = hold.window.clone();
    let callback = Closure::once_into_js(move || {
        let going = !hold.stopped.get() && hold.anchor.is_connected();
        if going {
            let mut moved = hold.anchor.get_bounding_client_rect().top() - hold.top;
            hold.quiet = if moved.abs() >= 0.5 {
                0
            } else {
                hold.quiet + 1
            };
            if moved.abs() >= 0.5
                && let Some(scroller) = scroller_of(&hold.window, &hold.anchor)
            {
                let before = scroller.scroll_top();
                scroller.set_scroll_top(before + moved.round() as i32);
                moved -= f64::from(scroller.scroll_top() - before);
            }
            if moved.abs() >= 0.5 {
                hold.window.scroll_by_with_x_and_y(0.0, moved);
            }
            hold.frame += 1;
        }
        if going && holding(hold.frame, hold.quiet, pending_on_screen(&hold.window)) {
            restore_place(hold);
        } else {
            for kind in GESTURES {
                let _ = hold.window.remove_event_listener_with_callback_and_bool(
                    kind,
                    hold.gesture.as_ref().unchecked_ref(),
                    true,
                );
            }
        }
    });
    let _ = window.request_animation_frame(callback.unchecked_ref());
}

/// Whether a reserved box anywhere on the screen still waits to draw what it stands for.
#[cfg(target_arch = "wasm32")]
fn pending_on_screen(window: &web_sys::Window) -> bool {
    let screen = [
        0.0,
        size(window.inner_height()),
        0.0,
        size(window.inner_width()),
    ];
    window
        .document()
        .and_then(|d| d.query_selector_all("[data-pending=true]").ok())
        .is_some_and(|pending| any_shows(&pending, screen))
}

/// The nearest box around `element` that scrolls up and down on its own, short of the page.
#[cfg(target_arch = "wasm32")]
fn scroller_of(window: &web_sys::Window, element: &web_sys::Element) -> Option<web_sys::Element> {
    let mut at = element.parent_element();
    while let Some(candidate) = at {
        if candidate.scroll_height() > candidate.client_height()
            && let Ok(Some(style)) = window.get_computed_style(&candidate)
            && matches!(
                style.get_property_value("overflow-y").as_deref(),
                Ok("auto" | "scroll")
            )
        {
            return Some(candidate);
        }
        at = candidate.parent_element();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: [f64; 4] = [0.0, 844.0, 16.0, 374.0];

    #[test]
    fn a_region_shows_when_any_of_its_area_meets_the_screen() {
        assert!(on_screen([100.0, 300.0, 16.0, 374.0], SCREEN));
        // Partly above, partly below, partly to either side.
        assert!(on_screen([-200.0, 1.0, 16.0, 374.0], SCREEN));
        assert!(on_screen([843.0, 1200.0, 16.0, 374.0], SCREEN));
        assert!(on_screen([100.0, 300.0, -100.0, 17.0], SCREEN));
        assert!(on_screen([100.0, 300.0, 373.0, 800.0], SCREEN));
        // A region larger than the screen on every side.
        assert!(on_screen([-1000.0, 2000.0, -50.0, 900.0], SCREEN));
    }

    #[test]
    fn a_region_off_the_screen_or_without_area_does_not_show() {
        // Edges that only touch the screen's.
        assert!(!on_screen([-200.0, 0.0, 16.0, 374.0], SCREEN));
        assert!(!on_screen([844.0, 1000.0, 16.0, 374.0], SCREEN));
        // A column scrolled out past either side of the page.
        assert!(!on_screen([100.0, 300.0, 374.0, 700.0], SCREEN));
        assert!(!on_screen([100.0, 300.0, -300.0, 16.0], SCREEN));
        // A closed `details` or a hidden tab: no height, or no width.
        assert!(!on_screen([200.0, 200.0, 16.0, 374.0], SCREEN));
        assert!(!on_screen([200.0, 300.0, 50.0, 50.0], SCREEN));
        assert!(!on_screen([0.0, 0.0, 0.0, 0.0], SCREEN));
    }

    #[test]
    fn a_hold_lasts_until_two_frames_pass_still_with_no_reserved_box_waiting_on_the_screen() {
        // The update's own frame, and the one after it: always held.
        assert!(holding(1, 0, false));
        assert!(holding(1, 1, false));
        // Still for two frames, with nothing left to draw: done.
        assert!(!holding(2, 2, false));
        assert!(!holding(9, 5, false));
        // A row's reserved box on the screen still to draw what it stands for: held on.
        assert!(holding(9, 5, true));
        // Something moved this frame: held on, waiting or not.
        assert!(holding(9, 0, false));
    }

    #[test]
    fn a_hold_ends_after_its_last_frame_whatever_still_waits() {
        assert!(holding(HOLD_FRAMES - 1, 0, true));
        assert!(!holding(HOLD_FRAMES, 0, true));
        assert!(!holding(HOLD_FRAMES, 0, false));
        assert!(!holding(u8::MAX, 1, true));
    }

    fn render(app: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    #[test]
    fn a_skeleton_draws_its_bars_for_the_eye_and_its_label_for_a_reader_of_text() {
        let html = render(|| rsx! { Skeleton { lines: 4, label: "Loading tasks…" } });
        assert_eq!(
            html.matches("class=\"bar\" aria-hidden=\"true\"").count(),
            4,
            "{html}"
        );
        assert!(
            html.contains("<span class=\"status-text\" role=\"status\">Loading tasks…</span>"),
            "{html}"
        );
        let unlabelled = render(|| rsx! { Skeleton {} });
        assert_eq!(
            unlabelled.matches("class=\"bar\"").count(),
            3,
            "{unlabelled}"
        );
        assert!(!unlabelled.contains("status-text"), "{unlabelled}");
    }

    #[test]
    fn a_settled_page_draws_its_content_without_the_skeleton_over_it() {
        // A page drawn ahead of the browser (and its text for a test) is settled: only the
        // browser can tell what a first screen shows.
        let html = render(|| rsx! { Settle { p { "The page" } } });
        assert!(
            html.starts_with(
                "<div class=\"settle\" data-settle=\"\" data-settled=\"true\"><p>The page</p>"
            ),
            "{html}"
        );
        assert!(!html.contains("skeleton"), "{html}");
    }
}
