//! A popup stays beside its trigger where possible, inside the visible viewport always, and no
//! taller than [`MAX_HEIGHT`]: a longer list scrolls inside it. It is as wide as its longest
//! option, its scrollbar beside it, up to the viewport's width.
//! Rectangles are [left, top, width, height], in layout-viewport CSS pixels.

/// The tallest a popup draws, 20rem: about eight options, a list the eye takes in at once.
pub(super) const MAX_HEIGHT: f64 = 320.0;

pub(super) fn popup_rect(trigger: [f64; 4], viewport: [f64; 4], content: [f64; 2]) -> [f64; 4] {
    let edge = 8.0_f64.min(viewport[2] / 2.0).min(viewport[3] / 2.0);
    let gap = 4.0;
    let available_width = (viewport[2] - 2.0 * edge).max(0.0);
    let available_height = (viewport[3] - 2.0 * edge).max(0.0);
    let below = (viewport[1] + viewport[3] - edge - trigger[1] - trigger[3] - gap)
        .clamp(0.0, available_height);
    let above = (trigger[1] - gap - viewport[1] - edge).clamp(0.0, available_height);
    let wanted = content[1].min(MAX_HEIGHT);
    let flipped = wanted > below && above > below;
    let height = wanted.min(if flipped { above } else { below });
    let width = content[0].max(trigger[2]).min(available_width);
    let left = trigger[0].clamp(
        viewport[0] + edge,
        viewport[0] + edge + available_width - width,
    );
    let top = if flipped {
        trigger[1] - gap - height
    } else {
        trigger[1] + trigger[3] + gap
    }
    .clamp(
        viewport[1] + edge,
        viewport[1] + edge + available_height - height,
    );
    [left, top, width, height]
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use wasm_bindgen::{JsCast, JsValue, closure::Closure};
    use web_sys::{Document, Event, EventTarget, HtmlElement, Window};

    fn visual_viewport(window: &Window) -> JsValue {
        js_sys::Reflect::get(window, &"visualViewport".into()).unwrap_or(JsValue::NULL)
    }

    fn bounds(window: &Window) -> [f64; 4] {
        let visual = visual_viewport(window);
        let number = |key: &str| {
            js_sys::Reflect::get(&visual, &key.into())
                .ok()
                .and_then(|v| v.as_f64())
        };
        [
            number("offsetLeft").unwrap_or(0.0),
            number("offsetTop").unwrap_or(0.0),
            number("width")
                .or_else(|| window.inner_width().ok()?.as_f64())
                .unwrap_or(0.0),
            number("height")
                .or_else(|| window.inner_height().ok()?.as_f64())
                .unwrap_or(0.0),
        ]
    }

    fn reveal(document: &Document, popup: &HtmlElement) {
        let Some(active) = document.active_element().filter(|active| {
            active.get_attribute("role").as_deref() == Some("option")
                && popup.contains(Some(active))
        }) else {
            return;
        };
        let item = active.get_bounding_client_rect();
        let list = popup.get_bounding_client_rect();
        let delta = if item.top() < list.top() + 4.0 {
            item.top() - list.top() - 4.0
        } else if item.bottom() > list.bottom() - 4.0 {
            item.bottom() - list.bottom() + 4.0
        } else {
            0.0
        };
        if delta != 0.0 {
            popup.set_scroll_top(popup.scroll_top() + delta.round() as i32);
        }
    }

    fn position(window: &Window, document: &Document, trigger_id: &str, popup_id: &str) {
        let Some(trigger) = document.get_element_by_id(trigger_id) else {
            return;
        };
        let Some(popup) = document
            .get_element_by_id(popup_id)
            .and_then(|e| e.dyn_into::<HtmlElement>().ok())
        else {
            return;
        };
        let viewport = bounds(window);
        let anchor = trigger.get_bounding_client_rect();
        let anchor = [anchor.left(), anchor.top(), anchor.width(), anchor.height()];
        // Measure the list with nothing clipped before choosing the side: its longest option on
        // one line (no wider than the viewport) and every option's height. Its stylesheet's
        // `min-width:100%` would be the whole window's width once fixed, so it is lifted here.
        // Hidden measurement avoids an unpositioned first frame; no style or focus work is deferred.
        let _ = popup.set_attribute("style", &format!("position:fixed;visibility:hidden;left:0;top:0;width:max-content;min-width:0;max-width:{}px;overflow:hidden;margin:0;z-index:1100", (viewport[2]-16.0).max(0.0)));
        let content = [
            f64::from(popup.offset_width()),
            f64::from(popup.scroll_height()),
        ];
        let place = |content: [f64; 2]| {
            let [left, top, width, height] = super::popup_rect(anchor, viewport, content);
            let _ = popup.set_attribute("style", &format!("position:fixed;left:{left}px;top:{top}px;width:{width}px;min-width:0;max-height:{height}px;overflow-y:auto;overflow-x:hidden;margin:0;z-index:1100"));
        };
        place(content);
        // A list that scrolls draws its scrollbar inside its width where the platform's
        // scrollbars take room: it is wider by that much, so no option wraps beside the bar.
        let bar = f64::from(popup.offset_width() - popup.client_width());
        if bar > 0.0 {
            place([content[0] + bar, content[1]]);
        }
        let _ = popup.set_attribute("data-positioned", "true");
        reveal(document, &popup);
    }

    /// Listeners belong to this popup, including visual-viewport changes from a phone keyboard.
    pub(crate) struct AnchoredPopup {
        window: Window,
        document: Document,
        visual: Option<EventTarget>,
        listener: Closure<dyn FnMut(Event)>,
    }

    impl AnchoredPopup {
        pub(crate) fn new(trigger: String, popup: String) -> Option<Self> {
            let window = web_sys::window()?;
            let document = window.document()?;
            let visual = visual_viewport(&window).dyn_into::<EventTarget>().ok();
            let watched_window = window.clone();
            let watched_document = document.clone();
            let watched_trigger = trigger.clone();
            let watched_popup = popup.clone();
            let listener = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
                if let Some(list) = watched_document
                    .get_element_by_id(&watched_popup)
                    .and_then(|e| e.dyn_into::<HtmlElement>().ok())
                {
                    if event.type_() == "focusin" {
                        reveal(&watched_document, &list);
                        return;
                    }
                    // Scrolling the list must not reset its own scroll position.
                    if event.type_() == "scroll"
                        && event
                            .target()
                            .and_then(|t| t.dyn_into::<web_sys::Node>().ok())
                            .is_some_and(|target| list.contains(Some(&target)))
                    {
                        return;
                    }
                }
                position(
                    &watched_window,
                    &watched_document,
                    &watched_trigger,
                    &watched_popup,
                );
            });
            let callback = listener.as_ref().unchecked_ref();
            let _ = window.add_event_listener_with_callback("resize", callback);
            let _ = document.add_event_listener_with_callback_and_bool("scroll", callback, true);
            let _ = document.add_event_listener_with_callback("focusin", callback);
            if let Some(visual) = &visual {
                let _ = visual.add_event_listener_with_callback("resize", callback);
                let _ = visual.add_event_listener_with_callback("scroll", callback);
            }
            position(&window, &document, &trigger, &popup);
            Some(Self {
                window,
                document,
                visual,
                listener,
            })
        }
    }

    impl Drop for AnchoredPopup {
        fn drop(&mut self) {
            let callback = self.listener.as_ref().unchecked_ref();
            let _ = self
                .window
                .remove_event_listener_with_callback("resize", callback);
            let _ = self
                .document
                .remove_event_listener_with_callback_and_bool("scroll", callback, true);
            let _ = self
                .document
                .remove_event_listener_with_callback("focusin", callback);
            if let Some(visual) = &self.visual {
                let _ = visual.remove_event_listener_with_callback("resize", callback);
                let _ = visual.remove_event_listener_with_callback("scroll", callback);
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) use browser::AnchoredPopup;

#[cfg(test)]
mod tests {
    use super::{MAX_HEIGHT, popup_rect};

    #[test]
    fn placement_fits_below_flips_above_and_scrolls_when_neither_side_fits() {
        assert_eq!(
            popup_rect(
                [20.0, 30.0, 80.0, 36.0],
                [0.0, 0.0, 400.0, 800.0],
                [100.0, 200.0]
            ),
            [20.0, 70.0, 100.0, 200.0]
        );
        assert_eq!(
            popup_rect(
                [20.0, 740.0, 80.0, 36.0],
                [0.0, 0.0, 400.0, 800.0],
                [100.0, 200.0]
            ),
            [20.0, 536.0, 100.0, 200.0]
        );
        assert_eq!(
            popup_rect(
                [20.0, 300.0, 80.0, 36.0],
                [0.0, 0.0, 400.0, 640.0],
                [100.0, 900.0]
            ),
            [20.0, 340.0, 100.0, 292.0]
        );
        assert_eq!(
            popup_rect(
                [20.0, 380.0, 80.0, 36.0],
                [0.0, 0.0, 400.0, 640.0],
                [100.0, 900.0]
            ),
            [20.0, 56.0, 100.0, MAX_HEIGHT]
        );
    }

    #[test]
    fn a_long_list_stops_at_the_cap_even_with_room_to_spare_and_flips_only_for_the_cap() {
        // Twelve options with the whole of a tall window below: the list still stops at the cap.
        assert_eq!(
            popup_rect(
                [20.0, 30.0, 80.0, 36.0],
                [0.0, 0.0, 1280.0, 1000.0],
                [100.0, 450.0]
            ),
            [20.0, 70.0, 100.0, MAX_HEIGHT]
        );
        // The cap fits below (330 px of room), so a list taller than the room stays below.
        assert_eq!(
            popup_rect(
                [20.0, 400.0, 80.0, 36.0],
                [0.0, 0.0, 400.0, 778.0],
                [100.0, 900.0]
            ),
            [20.0, 440.0, 100.0, MAX_HEIGHT]
        );
        // A short list is as tall as its options.
        assert_eq!(
            popup_rect(
                [20.0, 30.0, 80.0, 36.0],
                [0.0, 0.0, 1280.0, 1000.0],
                [100.0, MAX_HEIGHT - 1.0]
            ),
            [20.0, 70.0, 100.0, MAX_HEIGHT - 1.0]
        );
    }

    #[test]
    fn placement_clamps_wide_and_offscreen_triggers_in_the_visual_viewport() {
        assert_eq!(
            popup_rect(
                [380.0, 30.0, 80.0, 36.0],
                [0.0, 0.0, 400.0, 800.0],
                [900.0, 200.0]
            ),
            [8.0, 70.0, 384.0, 200.0]
        );
        assert_eq!(
            popup_rect(
                [-20.0, 900.0, 80.0, 36.0],
                [0.0, 0.0, 400.0, 800.0],
                [100.0, 200.0]
            ),
            [8.0, 592.0, 100.0, 200.0]
        );
        assert_eq!(
            popup_rect(
                [0.0, 460.0, 80.0, 36.0],
                [10.0, 200.0, 380.0, 300.0],
                [100.0, 200.0]
            ),
            [18.0, 256.0, 100.0, 200.0]
        );
    }
}
