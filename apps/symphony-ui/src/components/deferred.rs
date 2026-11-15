//! The overlay module (the web client's persistent kind `overlay`, apps/symphony-web/src/overlay.tsx,
//! loaded by the shell on a first select or picture, or ahead once the page is idle) draws a
//! select's popup and the picture viewer.
//! The chat sends it data and one owned callback through the shell's `reviewOverlay`; no VDOM nodes
//! or Rust values cross into it.
use wasm_bindgen::{JsCast, JsValue};

pub(crate) fn property(object: &JsValue, key: &str, value: &JsValue) {
    let _ = js_sys::Reflect::set(object, &key.into(), value);
}

pub(crate) fn show(request: &JsValue) {
    if let Some(window) = web_sys::window()
        && let Ok(show) = js_sys::Reflect::get(&window, &"reviewOverlay".into())
        && let Some(show) = show.dyn_ref::<js_sys::Function>()
    {
        let _ = show.call1(&JsValue::UNDEFINED, request);
    }
}

pub(crate) fn close(owner: &str) {
    let request = js_sys::Object::new();
    property(&request, "kind", &"close".into());
    property(&request, "owner", &owner.into());
    show(&request);
}
