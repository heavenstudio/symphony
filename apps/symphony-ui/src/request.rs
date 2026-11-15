//! Small browser JSON transport. Server refusals stay visible and failed saves keep their draft.

use std::sync::atomic::{AtomicU32, Ordering};

use js_sys::{Object, Reflect};
use serde::de::DeserializeOwned;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortController, Headers, RequestInit, Response};

use crate::live::{describe, sleep};
use crate::task_save::{ANSWER_MS, Failure, NO_ANSWER, in_time};
#[cfg(feature = "page-panel")]
use crate::task_save::{Outcome, Reply};

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

/// A read's fetch, aborted when the read is dropped before it ends: a page that left holds no
/// connection open for an answer nobody will read. A post is never aborted that way: it may
/// already have reached the server, and it goes on as `keepalive` does. The program carries this,
/// for its kind: the chat.
#[cfg(feature = "page-program")]
struct Abandoned(Option<AbortController>);

#[cfg(feature = "page-program")]
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
/// then how it ended (`ok`; `refused` for a status not ok; `failed`; `no-answer` when the
/// response's deadline or the body's won). So a test,
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

/// One request: a POST of `payload`, else a GET. Its answer's body as the browser parsed it.
async fn send(url: &str, payload: Option<JsValue>) -> Result<JsValue, Failure> {
    // A read's failure is what the chat says in place of what it could not show (`trouble`): the
    // server's own error with its status, or that the server could not be reached. A post's keeps
    // the server's words alone, beside the draft it kept.
    let reading = payload.is_none();
    let window = web_sys::window().ok_or_else(|| Failure::known("No window"))?;
    let controller = AbortController::new().map_err(|e| Failure::known(describe(e)))?;
    #[cfg(feature = "page-program")]
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
    // The deadline is for the server's response, and the body has one of its own from when the page
    // takes the response up. Each is polled before its deadline, so a response or body that came in
    // while the page was busy wins over a deadline that passed meanwhile.
    let (ended, how) = match in_time(JsFuture::from(fetch), sleep(ANSWER_MS)).await {
        Some(Ok(response)) => {
            marks.mark("fetched");
            answer(response.unchecked_into(), reading, &marks).await
        }
        Some(Err(error)) => (
            Err(Failure::uncertain(if reading {
                crate::trouble::UNREACHABLE.to_string()
            } else {
                describe(error)
            })),
            "failed",
        ),
        None => (Err(Failure::uncertain(NO_ANSWER)), "no-answer"),
    };
    if how == "no-answer" {
        controller.abort();
    }
    marks.mark(how);
    #[cfg(feature = "page-program")]
    {
        abandoned.0 = None;
    }
    ended
}

/// A response's answer: its body, as JSON, or the refusal its status tells, whatever its body: a
/// 404 without JSON is still missing. A read's refusal says what the chat shows in place of what it
/// could not ([`crate::trouble::answered`]), a post's the server's words. With how it ended, for the
/// marks: `ok`, `refused` (any status not ok), `failed` (a body that is not JSON) or `no-answer` (a
/// body that did not come in time; a refusal is still one).
async fn answer(
    response: Response,
    reading: bool,
    marks: &Marks,
) -> (Result<JsValue, Failure>, &'static str) {
    let value = match response.json() {
        Ok(body) => {
            marks.settles(&body, "body");
            in_time(JsFuture::from(body), sleep(ANSWER_MS)).await
        }
        Err(error) => Some(Err(error)),
    };
    marks.mark("read");
    if !response.ok() {
        let status = response.status();
        let error = value
            .and_then(Result::ok)
            .and_then(|value| field(&value, "error").as_string());
        let text = if reading {
            crate::trouble::answered(status, error.as_deref())
        } else {
            error.unwrap_or_else(|| format!("The server answered {status}"))
        };
        return (Err(Failure::refused(status, text)), "refused");
    }
    match value {
        Some(Ok(value)) => (Ok(value), "ok"),
        Some(Err(error)) => (Err(Failure::uncertain(describe(error))), "failed"),
        None => (Err(Failure::uncertain(NO_ANSWER)), "no-answer"),
    }
}
