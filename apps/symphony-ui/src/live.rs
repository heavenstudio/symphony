//! The browser half of the program's waits and marks: a timer to await, a mark on the page's
//! performance timeline, and a browser value said as text. No page is the program's: every page,
//! its reads and its feed are the web client's (apps/symphony-web), and the chat, all the program
//! draws over it, reads what it needs itself.

use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

pub(crate) async fn sleep(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = JsFuture::from(promise).await;
}

pub(crate) fn mark(name: &str) {
    if let Some(performance) = web_sys::window().and_then(|window| window.performance()) {
        let _ = performance.mark(name);
    }
}

pub(crate) fn describe(value: JsValue) -> String {
    value.as_string().unwrap_or_else(|| format!("{value:?}"))
}
