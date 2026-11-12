//! Small browser JSON transport. Server refusals stay visible and failed saves keep their draft.

use futures_util::future::{Either, select};
use js_sys::{Object, Reflect};
use serde::de::DeserializeOwned;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortController, Headers, RequestInit, Response};

use crate::live::{describe, sleep};
use crate::task_save::{Failure, NO_ANSWER};
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

/// What a read that names the copy the page holds answered.
pub enum Answer {
    /// `304`: the copy is current.
    Same,
    /// The body as the browser parsed it, and the tag the server answered it with (empty from a
    /// server without validators).
    Fresh(JsValue, String),
}

/// A read of something this device kept (`crate::cache`): it names the kept copy's tag
/// (`If-None-Match`, when there is one), and goes past the browser's own cache, which the API's
/// `no-store` keeps empty anyway, so the browser adds no validator of its own.
pub async fn get_if(url: &str, etag: &str) -> Result<Answer, String> {
    exchange(url, None, Some(etag))
        .await
        .map_err(Failure::into_text)
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

/// A read's fetch, aborted when the read is dropped before it ends: a task page that left, or a
/// newer read of its gates, holds no connection open for an answer nobody will read. A post is
/// never aborted that way: it may already have reached the server, and it goes on as `keepalive`
/// does. Only the task pages' module drops reads it no longer wants, so only it carries this; every
/// other module's `send` is the one it had before.
#[cfg(feature = "page-tasks")]
struct Abandoned(Option<AbortController>);

#[cfg(feature = "page-tasks")]
impl Drop for Abandoned {
    fn drop(&mut self) {
        if let Some(controller) = self.0.take() {
            controller.abort();
        }
    }
}

async fn send(url: &str, payload: Option<JsValue>) -> Result<JsValue, Failure> {
    match exchange(url, payload, None).await? {
        Answer::Fresh(value, _) => Ok(value),
        // Only a read that names a tag is answered 304; a server that answers one to anything
        // else sent no body to read.
        Answer::Same => Err(Failure::uncertain(
            "The server answered 304 to a plain read",
        )),
    }
}

/// One request: a POST of `payload`, else a GET naming `etag` when one is given (a kept read).
async fn exchange(
    url: &str,
    payload: Option<JsValue>,
    etag: Option<&str>,
) -> Result<Answer, Failure> {
    let window = web_sys::window().ok_or_else(|| Failure::known("No window"))?;
    let controller = AbortController::new().map_err(|e| Failure::known(describe(e)))?;
    #[cfg(feature = "page-tasks")]
    let mut abandoned = Abandoned(payload.is_none().then(|| controller.clone()));
    let init = RequestInit::new();
    init.set_signal(Some(&controller.signal()));
    if let Some(etag) = etag {
        let _ = Reflect::set(init.as_ref(), &"cache".into(), &"no-store".into());
        if !etag.is_empty() {
            let headers = Headers::new().map_err(|e| Failure::known(describe(e)))?;
            headers
                .set("If-None-Match", etag)
                .map_err(|e| Failure::known(describe(e)))?;
            init.set_headers(&headers);
        }
    }
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
        if response.status() == 304 {
            return Ok(Answer::Same);
        }
        let tag = response
            .headers()
            .get("ETag")
            .ok()
            .flatten()
            .unwrap_or_default();
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
        value
            .map(|value| Answer::Fresh(value, tag))
            .map_err(|e| Failure::uncertain(describe(e)))
    };
    let ended = match select(std::pin::pin!(operation), std::pin::pin!(sleep(15_000))).await {
        Either::Left((result, _)) => result,
        Either::Right(_) => {
            controller.abort();
            Err(Failure::uncertain(NO_ANSWER))
        }
    };
    #[cfg(feature = "page-tasks")]
    {
        abandoned.0 = None;
    }
    ended
}
