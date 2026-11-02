//! Small browser JSON transport. Server refusals stay visible and failed saves keep their draft.

use futures_util::future::{Either, select};
use js_sys::{Object, Reflect};
use serde::de::DeserializeOwned;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortController, Headers, RequestInit, Response};

use crate::live::{describe, sleep};

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
    serde_wasm_bindgen::from_value(send(url, None).await?).map_err(|e| e.to_string())
}

/// Raw typed API data can stay in the shell until its deferred component needs decoding.
pub async fn get_json(url: &str) -> Result<JsValue, String> {
    send(url, None).await
}

pub async fn post(url: &str, fields: &[(&str, JsValue)]) -> Result<JsValue, String> {
    send(url, Some(object(fields))).await
}

async fn send(url: &str, payload: Option<JsValue>) -> Result<JsValue, String> {
    let window = web_sys::window().ok_or("No window")?;
    let controller = AbortController::new().map_err(describe)?;
    let init = RequestInit::new();
    init.set_signal(Some(&controller.signal()));
    if let Some(payload) = payload {
        init.set_method("POST");
        let _ = Reflect::set(init.as_ref(), &"keepalive".into(), &JsValue::TRUE);
        init.set_body(&js_sys::JSON::stringify(&payload).map_err(describe)?.into());
        let headers = Headers::new().map_err(describe)?;
        headers
            .set("Content-Type", "application/json")
            .map_err(describe)?;
        init.set_headers(&headers);
    }
    let operation = async {
        let response: Response = JsFuture::from(window.fetch_with_str_and_init(url, &init))
            .await
            .map_err(describe)?
            .unchecked_into();
        let value = JsFuture::from(response.json().map_err(describe)?)
            .await
            .map_err(describe)?;
        if !response.ok() {
            return Err(field(&value, "error")
                .as_string()
                .unwrap_or_else(|| format!("The server answered {}", response.status())));
        }
        Ok(value)
    };
    match select(std::pin::pin!(operation), std::pin::pin!(sleep(15_000))).await {
        Either::Left((result, _)) => result,
        Either::Right(_) => {
            controller.abort();
            Err("✖ no answer from the server; check before sending again".into())
        }
    }
}
