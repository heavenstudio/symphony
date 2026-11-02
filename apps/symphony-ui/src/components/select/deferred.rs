//! The entry module renders the styled field; opening loads the panel's primitive popup.
use crate::components::deferred::{close, property, show};
use dioxus::prelude::*;
use futures_channel::mpsc::unbounded;
use futures_util::StreamExt;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};

const STYLE: Asset = asset!(
    "/src/components/select/trigger.css",
    AssetOptions::css().with_static_head(true)
);

#[component]
pub fn SelectField(
    id: Option<String>,
    label: String,
    value: String,
    options: Vec<(String, String)>,
    on_change: Callback<String>,
    #[props(default)] disabled: bool,
) -> Element {
    std::hint::black_box(&STYLE);
    let generated = use_hook(|| {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        format!(
            "symphony-{}-field-{}",
            crate::module::kind(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    });
    let id = id.unwrap_or(generated);
    let popup = format!("{id}-popup");
    let mut open = use_signal(|| false);
    let owner = id.clone();
    let (callback, dismiss) = use_hook(move || {
        let (send, mut recv) = unbounded::<Option<String>>();
        // Process callbacks in this field's owned task, after the popup's event and blur
        // handlers finish. A native JS callback must not reenter the same panel render.
        spawn(async move {
            while let Some(value) = recv.next().await {
                if let Some(value) = value {
                    on_change.call(value);
                }
                open.set(false);
                close(&owner);
                if let Some(trigger) = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|doc| doc.get_element_by_id(&owner))
                    .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
                {
                    let _ = trigger.focus();
                }
            }
        });
        let dismiss = send.clone();
        let callback =
            std::rc::Rc::new(Closure::<dyn FnMut(JsValue)>::new(move |value: JsValue| {
                let _ = send.unbounded_send(value.as_string());
            }));
        (callback, dismiss)
    });
    let dropped = id.clone();
    use_drop(move || close(&dropped));
    let text = options
        .iter()
        .find(|(key, _)| *key == value)
        .map(|(_, text)| text.clone())
        .unwrap_or_default();
    let trigger_id = id.clone();
    let trigger_label = label.clone();
    let key_dismiss = dismiss.clone();
    let activate = use_callback(move |()| {
        if open() {
            let _ = dismiss.unbounded_send(None);
            return;
        }
        let request = js_sys::Object::new();
        for (key, val) in [
            ("kind", "select"),
            ("owner", &id),
            ("id", &id),
            ("label", &label),
            ("value", &value),
        ] {
            property(&request, key, &val.into());
        }
        if let Ok(options) = serde_wasm_bindgen::to_value(&options) {
            property(&request, "options", &options);
        }
        property(&request, "callback", callback.as_ref().as_ref());
        open.set(true);
        show(&request);
    });
    let key_id = use_memo(use_reactive!(|trigger_id| trigger_id.clone()));
    let keys = use_hook(move || {
        let listener = std::rc::Rc::new(Closure::<dyn FnMut(web_sys::Event)>::new(
            move |event: web_sys::Event| {
                let Some(key) = event.dyn_ref::<web_sys::KeyboardEvent>() else {
                    return;
                };
                let Some(target) = event
                    .target()
                    .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
                else {
                    return;
                };
                if target.id() != *key_id.peek() || target.has_attribute("disabled") {
                    return;
                }
                match key.key().as_str() {
                    "Escape" if open() => {
                        event.prevent_default();
                        event.stop_propagation();
                        let _ = key_dismiss.unbounded_send(None);
                    }
                    "ArrowDown" | "ArrowUp" => {
                        event.prevent_default();
                        if !open() {
                            activate.call(());
                        }
                    }
                    _ => {}
                }
            },
        ));
        if let Some(window) = web_sys::window() {
            let _ = window.add_event_listener_with_callback(
                "keydown",
                listener.as_ref().as_ref().unchecked_ref(),
            );
        }
        listener
    });
    use_drop(move || {
        if let Some(window) = web_sys::window() {
            let _ = window.remove_event_listener_with_callback(
                "keydown",
                keys.as_ref().as_ref().unchecked_ref(),
            );
        }
    });
    let actions = crate::entry_events::click(move || activate.call(()));
    rsx! { div { class: "dx-select",
        button {
            id: trigger_id, r#type: "button", role: "combobox", class: "dx-select-trigger",
            disabled, "aria-label": trigger_label, "aria-haspopup": "listbox",
            "aria-expanded": open(), "aria-controls": popup,
            ..actions,
            span { "{text}" }
            svg { "aria-hidden": "true", width: 20, height: 20, view_box: "0 0 24 24", fill: "none", stroke: "currentColor", stroke_width: 2, stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "m6 9 6 6 6-6" }
            }
        }
    } }
}
