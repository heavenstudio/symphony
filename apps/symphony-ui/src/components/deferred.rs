//! The existing persistent panel owns overlays. Page modules send data and one owned callback;
//! no VDOM nodes or Rust values cross independently compiled WASM runtimes.
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

#[cfg(feature = "page-panel")]
fn answer(request: &JsValue, value: &JsValue) {
    if let Ok(callback) = js_sys::Reflect::get(request, &"callback".into())
        && let Some(callback) = callback.dyn_ref::<js_sys::Function>()
    {
        let _ = callback.call1(&JsValue::UNDEFINED, value);
    }
}

#[cfg(feature = "page-panel")]
#[dioxus::prelude::component]
pub(crate) fn OverlayHost() -> dioxus::prelude::Element {
    use dioxus::prelude::*;
    let revision = crate::frame::use_revision(&["review:overlay"]);
    let _ = revision();
    let request = crate::frame::snapshot("reviewOverlayRequest");
    let field = |name: &str| js_sys::Reflect::get(&request, &name.into()).unwrap_or(JsValue::NULL);
    let kind = field("kind").as_string().unwrap_or_default();
    let owner = field("owner").as_string().unwrap_or_default();
    match kind.as_str() {
        "select" => {
            let id = field("id").as_string().unwrap_or_default();
            let label = field("label").as_string().unwrap_or_default();
            let value = field("value").as_string().unwrap_or_default();
            let options: Vec<(String, String)> =
                serde_wasm_bindgen::from_value(field("options")).unwrap_or_default();
            let changed = request.clone();
            rsx! { crate::components::select::SelectPopup {
                key: "{owner}", id: id.clone(), label, value, options,
                external_trigger: Some(id),
                on_change: move |value: String| answer(&changed, &value.into()),
                on_close: move |()| answer(&request, &JsValue::NULL),
            } }
        }
        "picture" => {
            let photos = serde_wasm_bindgen::from_value(field("photos")).unwrap_or_default();
            let at = field("at").as_f64().unwrap_or_default() as usize;
            rsx! { crate::media_live::PictureOverlay {
                key: "{owner}", photos, at,
                command: move |direction: isize| answer(&request, &JsValue::from_f64(direction as f64)),
            } }
        }
        _ => rsx! {},
    }
}
