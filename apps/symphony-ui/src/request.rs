//! Small browser JSON transport. Server refusals stay visible and failed saves keep their draft.

use futures_util::future::{Either, select};
use js_sys::{Object, Reflect};
use serde::de::DeserializeOwned;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortController, Headers, RequestInit, Response};

use crate::live::{describe, sleep};
use crate::task_save::Failure;
#[cfg(feature = "page-panel")]
use crate::task_save::{Outcome, Reply};

pub fn encode(value: &str) -> String {
    js_sys::encode_uri_component(value).into()
}

pub fn field(value: &JsValue, key: &str) -> JsValue {
    Reflect::get(value, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

pub fn object(fields: &[(&str, JsValue)]) -> JsValue {
    let object = Object::new();
    for (key, value) in fields {
        let _ = Reflect::set(&object, &JsValue::from_str(key), value);
    }
    object.into()
}

pub async fn get<T: DeserializeOwned>(url: &str) -> Result<T, String> {
    serde_wasm_bindgen::from_value(send(url, None).await.map_err(Failure::into_text)?)
        .map_err(|e| e.to_string())
}

/// A read of something the server may not have: `None` when it answers 404, so a page can say
/// what is missing rather than that the read failed.
pub async fn get_found<T: DeserializeOwned>(url: &str) -> Result<Option<T>, String> {
    match send(url, None).await {
        Ok(value) => serde_wasm_bindgen::from_value(value)
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(Failure::Missing(_)) => Ok(None),
        Err(failure) => Err(failure.into_text()),
    }
}

/// Raw typed API data can stay in the shell until its deferred component needs decoding.
pub async fn get_json(url: &str) -> Result<JsValue, String> {
    send(url, None).await.map_err(Failure::into_text)
}

pub async fn post(url: &str, fields: &[(&str, JsValue)]) -> Result<JsValue, String> {
    send(url, Some(object(fields)))
        .await
        .map_err(Failure::into_text)
}

#[cfg(feature = "page-panel")]
pub(crate) async fn task_post(url: &str, fields: &[(&str, JsValue)]) -> Result<Outcome, Failure> {
    let value = send(url, Some(object(fields))).await?;
    serde_wasm_bindgen::from_value::<Reply>(value)
        .map_err(|e| Failure::uncertain(e.to_string()))?
        .outcome()
}

async fn send(url: &str, payload: Option<JsValue>) -> Result<JsValue, Failure> {
    let window = web_sys::window().ok_or_else(|| Failure::known("No window"))?;
    let controller = AbortController::new().map_err(|e| Failure::known(describe(e)))?;
    let init = RequestInit::new();
    init.set_signal(Some(&controller.signal()));
    if let Some(payload) = payload {
        init.set_method("POST");
        let _ = Reflect::set(init.as_ref(), &"keepalive".into(), &JsValue::TRUE);
        init.set_body(
            &js_sys::JSON::stringify(&payload)
                .map_err(|e| Failure::known(describe(e)))?
                .into(),
        );
        let headers = Headers::new().map_err(|e| Failure::known(describe(e)))?;
        headers
            .set("Content-Type", "application/json")
            .map_err(|e| Failure::known(describe(e)))?;
        init.set_headers(&headers);
    }
    // Every API path is the page's project's, under its base on the one server.
    let url = crate::project::current().here(url);
    let operation = async {
        let response: Response = JsFuture::from(window.fetch_with_str_and_init(&url, &init))
            .await
            .map_err(|e| Failure::uncertain(describe(e)))?
            .unchecked_into();
        let value = match response.json() {
            Ok(body) => JsFuture::from(body).await,
            Err(error) => Err(error),
        };
        // A refusal is told by its status, whatever its body: a 404 without JSON is still missing.
        if !response.ok() {
            let text = value
                .ok()
                .and_then(|value| field(&value, "error").as_string())
                .unwrap_or_else(|| format!("The server answered {}", response.status()));
            return Err(Failure::refused(response.status(), text));
        }
        value.map_err(|e| Failure::uncertain(describe(e)))
    };
    match select(std::pin::pin!(operation), std::pin::pin!(sleep(15_000))).await {
        Either::Left((result, _)) => result,
        Either::Right(_) => {
            controller.abort();
            Err(Failure::uncertain(
                "✖ no answer from the server; check before sending again",
            ))
        }
    }
}
