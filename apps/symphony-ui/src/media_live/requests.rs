//! The document page's asks' requests: a JSON read or post under the page's project, a field of
//! its answer and a pause between polls.

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// A read of `path`, its fetch aborted when the read is dropped before it ends: the asks read again
/// as the page's feed opens, dropping the read before, and a read dropped once its answer came
/// would leave its body unread, which the browser counts in flight for as long as the page is open.
pub(crate) async fn fetch(path: &str) -> Result<JsValue, String> {
    let window = web_sys::window().ok_or("no browser")?;
    let url = crate::project::current().here(path);
    let controller = web_sys::AbortController::new().map_err(describe)?;
    let init = web_sys::RequestInit::new();
    init.set_signal(Some(&controller.signal()));
    let mut abandoned = Abandoned(Some(controller));
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str_and_init(&url, &init))
        .await
        .map_err(describe)?
        .unchecked_into();
    let read = response_json(response, path).await;
    abandoned.0 = None;
    read
}

/// The fetch a read still owes an end, aborted when the read is dropped first.
struct Abandoned(Option<web_sys::AbortController>);

impl Drop for Abandoned {
    fn drop(&mut self) {
        if let Some(controller) = self.0.take() {
            controller.abort();
        }
    }
}

pub(crate) async fn post(path: &str, fields: &[(&str, &str)]) -> Result<JsValue, String> {
    let object = js_sys::Object::new();
    for (name, value) in fields {
        js_sys::Reflect::set(&object, &JsValue::from_str(name), &JsValue::from_str(value))
            .map_err(describe)?;
    }
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_body(&js_sys::JSON::stringify(&object).map_err(describe)?.into());
    let headers = web_sys::Headers::new().map_err(describe)?;
    headers
        .set("Content-Type", "application/json")
        .map_err(describe)?;
    init.set_headers(&headers);
    let window = web_sys::window().ok_or("no browser")?;
    let url = crate::project::current().here(path);
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str_and_init(&url, &init))
        .await
        .map_err(describe)?
        .unchecked_into();
    response_json(response, path).await
}

async fn response_json(response: web_sys::Response, path: &str) -> Result<JsValue, String> {
    let value = JsFuture::from(response.json().map_err(describe)?)
        .await
        .map_err(describe)?;
    if response.ok() {
        Ok(value)
    } else {
        Err(js_sys::Reflect::get(&value, &JsValue::from_str("error"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_else(|| format!("{path}: {}", response.status())))
    }
}

pub(crate) fn field<T: serde::de::DeserializeOwned>(
    value: &JsValue,
    name: &str,
) -> Result<T, String> {
    serde_wasm_bindgen::from_value(
        js_sys::Reflect::get(value, &JsValue::from_str(name)).map_err(describe)?,
    )
    .map_err(|e| e.to_string())
}

pub(crate) async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = JsFuture::from(promise).await;
}

fn describe(value: JsValue) -> String {
    value.as_string().unwrap_or_else(|| format!("{value:?}"))
}
