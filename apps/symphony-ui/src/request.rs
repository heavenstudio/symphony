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

/// [`get_if`] of something the server may not have: `None` when it answers 404.
pub async fn get_if_found(url: &str, etag: &str) -> Result<Option<Answer>, String> {
    match exchange(url, None, Some(etag)).await {
        Ok(answer) => Ok(Some(answer)),
        Err(Failure::Missing(_)) => Ok(None),
        Err(failure) => Err(failure.into_text()),
    }
}

/// [`get_if_found`] of a document read, with the feed's cursor it was read at, as the server names
/// it in a header on its `200` and its `304` alike ([`symphony_api::VERSION_HEADER`]): a page that
/// draws the copy it kept on a `304`, which has no body, opens the feed from it. `None` from a
/// server that names none.
#[cfg(feature = "page-docs")]
pub async fn get_document(url: &str, etag: &str) -> Result<Option<(Answer, Option<u64>)>, String> {
    match exchange(url, None, Some(etag)).await {
        // `read` set it as it ended, and nothing else ran since.
        Ok(answer) => Ok(Some((answer, CURSOR.take()))),
        Err(Failure::Missing(_)) => Ok(None),
        Err(failure) => Err(failure.into_text()),
    }
}

#[cfg(feature = "page-docs")]
thread_local! {
    /// The cursor the last response named ([`get_document`]): `read` sets it after its last wait,
    /// and its caller takes it in the same turn, before any other task runs.
    static CURSOR: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
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
/// does. The task pages drop reads they no longer want, so the program they are part of carries
/// this (`page-program` turns on `page-tasks`), for every kind of it: the task pages, Reports, the
/// chat, the menus and the timeline. The board's, home's and the documents' `send` is the one it
/// had before.
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

/// A post's way through [`exchange`], on the page's performance timeline as `send N POST PATH PHASE`
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
    // A read's failure, plain or kept, is what a page says in place of what it could not show
    // (`trouble`): the server's own error with its status, or that the server could not be
    // reached. A post's keeps the server's words alone, beside the draft it kept.
    let reading = payload.is_none();
    let window = web_sys::window().ok_or_else(|| Failure::known("No window"))?;
    let controller = AbortController::new().map_err(|e| Failure::known(describe(e)))?;
    #[cfg(feature = "page-tasks")]
    let mut abandoned = Abandoned(payload.is_none().then(|| controller.clone()));
    let marks = Marks::of(payload.is_some(), url);
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
    marks.mark("start");
    let fetch = window.fetch_with_str_and_init(&url, &init);
    marks.settles(&fetch, "response");
    // The deadline is for the server's response, and the body has one of its own from when the page
    // takes the response up. Each is polled before its deadline, so a response or body that came in
    // while the page was busy wins over a deadline that passed meanwhile.
    let (ended, how) = match in_time(JsFuture::from(fetch), sleep(ANSWER_MS)).await {
        Some(Ok(response)) => {
            marks.mark("fetched");
            read(response.unchecked_into(), reading, &marks).await
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
    #[cfg(feature = "page-tasks")]
    {
        abandoned.0 = None;
    }
    ended
}

/// A response's answer: [`Answer::Same`] for a `304`, which has no body to wait for; else its body,
/// as JSON, with the tag it came with, or the refusal its status tells, whatever its body: a 404
/// without JSON is still missing. A read's refusal says what the page shows in place of what it
/// could not ([`crate::trouble::answered`]), a post's the server's words. With how it ended, for the
/// marks: `ok`, `refused` (any status not ok), `failed` (a body that is not JSON) or `no-answer` (a
/// body that did not come in time; a refusal is still one). The documents' module also takes the
/// cursor a document read names ([`get_document`]); every other module reads the answer alone.
#[cfg(feature = "page-docs")]
async fn read(
    response: Response,
    reading: bool,
    marks: &Marks,
) -> (Result<Answer, Failure>, &'static str) {
    let cursor = response
        .headers()
        .get(symphony_api::VERSION_HEADER)
        .ok()
        .flatten()
        .and_then(|cursor| cursor.parse::<u64>().ok());
    let answered = answer(response, reading, marks).await;
    // After the body's wait, so no other read's answer comes between this and its caller.
    CURSOR.set(cursor);
    answered
}

#[cfg(not(feature = "page-docs"))]
use self::answer as read;

/// [`read`]'s answer, its body read.
async fn answer(
    response: Response,
    reading: bool,
    marks: &Marks,
) -> (Result<Answer, Failure>, &'static str) {
    if response.status() == 304 {
        return (Ok(Answer::Same), "ok");
    }
    let tag = response
        .headers()
        .get("ETag")
        .ok()
        .flatten()
        .unwrap_or_default();
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
        Some(Ok(value)) => (Ok(Answer::Fresh(value, tag)), "ok"),
        Some(Err(error)) => (Err(Failure::uncertain(describe(error))), "failed"),
        None => (Err(Failure::uncertain(NO_ANSWER)), "no-answer"),
    }
}
