//! Small browser JSON transport. Server refusals stay visible and failed saves keep their draft.

use std::sync::atomic::{AtomicU32, Ordering};

use futures_util::future::{Either, select};
use js_sys::{Object, Reflect};
use serde::de::DeserializeOwned;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
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

pub async fn post(url: &str, fields: &[(&str, JsValue)]) -> Result<JsValue, String> {
    send(url, Some(object(fields)))
        .await
        .map_err(Failure::into_text)
}

/// A post that fails as [`Failure`] says, so a page can tell a refusal, which wrote nothing, from a
/// save no reply answered, which the server may have written.
#[cfg(feature = "page-docs")]
pub(crate) async fn save(url: &str, fields: &[(&str, JsValue)]) -> Result<JsValue, Failure> {
    send(url, Some(object(fields))).await
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

/// A post's way through [`send`], on the page's performance timeline as `send N POST PATH PHASE`
/// marks: `start`; `response` and `body` where the page's own callbacks saw the fetch's and the
/// body's promises settle, and `fetched` and `read` where the task waiting on each took it up;
/// then how it ended (`ok`, `refused`, `failed`, or `no-answer` when the deadline won). So a test,
/// or the reader's DevTools, tells a reply that came late from one the page took up late. Reads are
/// not marked: a page reads all its life, and the timeline keeps every mark.
struct Marks(Option<String>);

impl Marks {
    fn of(post: bool, url: &str) -> Self {
        static SENT: AtomicU32 = AtomicU32::new(0);
        Self(post.then(|| {
            let n = SENT.fetch_add(1, Ordering::Relaxed);
            format!("send {n} POST {}", url.split('?').next().unwrap_or(url))
        }))
    }

    fn mark(&self, phase: &str) {
        if let Some(name) = &self.0 {
            crate::live::mark(&format!("{name} {phase}"));
        }
    }

    /// Marks `phase` once `promise` settles, from the promise's own callback: however late the task
    /// awaiting it is polled. Both outcomes are taken, so the callback's promise never rejects.
    fn settles(&self, promise: &js_sys::Promise, phase: &str) {
        let Some(name) = &self.0 else {
            return;
        };
        let name = format!("{name} {phase}");
        let settled = Closure::once_into_js(move |_: JsValue| crate::live::mark(&name));
        if let Ok(then) = Reflect::get(promise, &"then".into())
            && let Some(then) = then.dyn_ref::<js_sys::Function>()
        {
            let _ = then.call2(promise, &settled, &settled);
        }
    }
}

async fn send(url: &str, payload: Option<JsValue>) -> Result<JsValue, Failure> {
    let window = web_sys::window().ok_or_else(|| Failure::known("No window"))?;
    let controller = AbortController::new().map_err(|e| Failure::known(describe(e)))?;
    #[cfg(feature = "page-tasks")]
    let mut abandoned = Abandoned(payload.is_none().then(|| controller.clone()));
    let marks = Marks::of(payload.is_some(), url);
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
    marks.mark("start");
    let fetch = window.fetch_with_str_and_init(&url, &init);
    marks.settles(&fetch, "response");
    // The deadline is for the server's answer. One that is in is read to its end, however late the
    // page takes it up: the fetch is polled first, so a response that arrived while the page was
    // busy wins over a deadline that passed meanwhile, and its body has no deadline of its own.
    let ended = match select(
        std::pin::pin!(JsFuture::from(fetch)),
        std::pin::pin!(sleep(15_000)),
    )
    .await
    {
        Either::Left((Ok(response), _)) => {
            marks.mark("fetched");
            read(response.unchecked_into(), &marks).await
        }
        Either::Left((Err(error), _)) => Err(Failure::uncertain(describe(error))),
        Either::Right(_) => {
            controller.abort();
            Err(Failure::uncertain(NO_ANSWER))
        }
    };
    marks.mark(match &ended {
        Ok(_) => "ok",
        Err(Failure::Uncertain(why)) if why == NO_ANSWER => "no-answer",
        Err(Failure::Uncertain(_)) => "failed",
        Err(_) => "refused",
    });
    #[cfg(feature = "page-tasks")]
    {
        abandoned.0 = None;
    }
    ended
}

/// A response's body, as JSON, or the refusal its status tells, whatever its body: a 404 without
/// JSON is still missing.
async fn read(response: Response, marks: &Marks) -> Result<JsValue, Failure> {
    let value = match response.json() {
        Ok(body) => {
            marks.settles(&body, "body");
            JsFuture::from(body).await
        }
        Err(error) => Err(error),
    };
    marks.mark("read");
    if !response.ok() {
        let text = value
            .ok()
            .and_then(|value| field(&value, "error").as_string())
            .unwrap_or_else(|| format!("The server answered {}", response.status()));
        return Err(Failure::refused(response.status(), text));
    }
    value.map_err(|e| Failure::uncertain(describe(e)))
}
