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
                div { class: "viewport-placeholder", style: "min-height:{height}px", "aria-hidden": "true",
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
