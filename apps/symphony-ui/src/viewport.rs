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

/// `children` mounted once they come near the screen; until then `ghost`, the outline of what
/// they will be, in a box of `height` px (lines of text where no ghost is given).
#[component]
pub fn NearViewport(
    children: Element,
    #[props(default)] anchors: Vec<String>,
    #[props(default = 160)] height: u32,
    #[props(default)] id: String,
    #[props(default)] class: String,
    #[props(default)] list_item: bool,
    onenter: Option<EventHandler<()>>,
    ghost: Option<Element>,
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
                    if let Some(ghost) = ghost { {ghost} } else { Skeleton { lines: lines_for(height) } }
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

/// The height of a line of body text, in px (16 px at a line height of 1.55, and its gap).
const LINE: u32 = 26;

/// How many lines of text a placeholder `height` px tall stands for: one at least, six at most.
pub fn lines_for(height: u32) -> u8 {
    u8::try_from((height / LINE).clamp(1, 6)).unwrap_or(6)
}

/// A paragraph waiting for its read: a bar where each of its lines will be, at the line's own
/// height, shimmering only for a reader who accepts motion, and `label` for one who reads the
/// page's text instead. A ghost of a larger part (a card, a page) draws its own boxes, with a
/// `span.bone` for each line in them.
#[component]
pub fn Skeleton(#[props(default = 3)] lines: u8, #[props(default)] label: String) -> Element {
    rsx! { div { class: "skeleton",
        if !label.is_empty() { span { class: "status-text", role: "status", "{label}" } }
        for line in 0..lines { span { key: "{line}", class: "bone", "aria-hidden": "true" } }
    } }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(app: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    #[test]
    fn a_skeleton_draws_a_bar_per_line_for_the_eye_and_its_label_for_a_reader_of_text() {
        let html = render(|| rsx! { Skeleton { lines: 4, label: "Loading tasks…" } });
        assert_eq!(
            html.matches("class=\"bone\" aria-hidden=\"true\"").count(),
            4,
            "{html}"
        );
        assert!(
            html.contains("<span class=\"status-text\" role=\"status\">Loading tasks…</span>"),
            "{html}"
        );
        let unlabelled = render(|| rsx! { Skeleton {} });
        assert_eq!(
            unlabelled.matches("class=\"bone\"").count(),
            3,
            "{unlabelled}"
        );
        assert!(!unlabelled.contains("status-text"), "{unlabelled}");
    }

    #[test]
    fn a_placeholder_stands_for_a_line_of_text_per_26_px_from_one_to_six() {
        assert_eq!(lines_for(0), 1);
        assert_eq!(lines_for(28), 1);
        assert_eq!(lines_for(51), 1);
        assert_eq!(lines_for(52), 2);
        assert_eq!(lines_for(100), 3);
        assert_eq!(lines_for(156), 6);
        assert_eq!(lines_for(160), 6);
        assert_eq!(lines_for(u32::MAX), 6);
    }
}
