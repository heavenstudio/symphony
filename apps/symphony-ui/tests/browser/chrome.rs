//! Headless Chrome for these cases: the shared driver (crates/chrome-devtools), one browser per
//! test on a profile of its own, leashed by this test binary so it dies with the test process
//! however that ends. A driver failure fails the case with the driver's message.

use std::time::Duration;

use serde_json::{Value, json};

pub use symphony_chrome::{Request, WEB_SOCKET, find};

/// The driver's failures, as this suite's assertion: the case fails naming what went wrong.
trait Asserted<T> {
    fn asserted(self) -> T;
}

impl<T> Asserted<T> for symphony_chrome::Result<T> {
    #[track_caller]
    fn asserted(self) -> T {
        self.unwrap_or_else(|error| panic!("{error}"))
    }
}

pub struct Chrome(symphony_chrome::Chrome);

impl Chrome {
    pub fn launch(binary: &str) -> Chrome {
        let leash = symphony_chrome::Leash::new(
            std::env::current_exe().unwrap(),
            &["--exact", "chrome::browser_leash", "--nocapture"],
        );
        let chrome = symphony_chrome::Chrome::launch(binary, &leash).asserted();
        eprintln!(
            "Chrome launch leash={} profile={}",
            chrome.leash_id(),
            chrome.profile().display()
        );
        Chrome(chrome)
    }

    /// Emulate a mid phone on a tailnet link: CPU slowed 4x, 10 Mbit/s each way, 40 ms round trip.
    pub fn mid_phone(&mut self) {
        self.0.mid_phone().asserted();
    }

    /// Ask to save data, as a browser in data saver mode does, on every page opened from now on:
    /// the shell then warms no module at idle, and a module's first use alone requests its files.
    /// A test that holds or fails such a request uses it, so no idle prefetch serves the file
    /// first.
    pub fn save_data(&mut self) {
        self.connection(true, "4g");
    }

    /// Pages opened from now on read `navigator.connection` as asking to save data or not, on a
    /// link of the Network Information API's `effective_type` (`4g`, `3g`, `2g`, `slow-2g`).
    pub fn connection(&mut self, save_data: bool, effective_type: &str) {
        self.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source": format!(
                "Object.defineProperty(Navigator.prototype, 'connection', {{configurable: true, get: () => ({{saveData: {save_data}, effectiveType: {}}})}});",
                json!(effective_type)
            )}),
        );
    }

    /// Go to `url` and wait for its document to be parsed.
    pub fn open(&mut self, url: &str) {
        self.0.open(url).asserted();
    }

    /// Evaluate `expression` in the page (awaiting a promise), its value as JSON.
    pub fn eval(&mut self, expression: &str) -> Value {
        self.0.eval(expression).asserted()
    }

    /// Wait until `expression` is truthy, bounded by browser CPU and completed polls.
    pub fn wait_for(&mut self, expression: &str, cpu: Duration) -> bool {
        self.0.wait_for(expression, cpu).asserted()
    }

    /// Observe protocol events while the real browser is paused at a fixture boundary.
    pub fn wait_for_protocol_events(
        &mut self,
        method: &str,
        count: usize,
        cpu: Duration,
    ) -> Vec<Value> {
        self.0
            .wait_for_protocol_events(method, count, cpu)
            .asserted()
    }

    /// The requests seen since the last `open`, by Chrome's request id.
    pub fn requests(&mut self) -> Vec<Request> {
        self.0.requests().asserted()
    }

    /// One DevTools command; its result. Events read meanwhile are kept.
    pub fn call(&mut self, method: &str, params: Value) -> Value {
        self.0.call(method, params).asserted()
    }

    /// One DevTools command to the browser, not the page (`Target.createTarget`, say); its result.
    pub fn browser_call(&mut self, method: &str, params: Value) -> Value {
        self.0.browser_call(method, params).asserted()
    }

    /// One DevTools command on another tab's `session` (`Target.attachToTarget`'s); its result.
    pub fn call_on(&mut self, session: &str, method: &str, params: Value) -> Value {
        self.0.call_on(session, method, params).asserted()
    }

    /// Drain through a browser-scheduled observation marker.
    pub fn pump(&mut self, window: Duration) {
        self.0.pump(window).asserted();
    }
}

/// Reexecuted in a separate process as a launch's leash, with no shared mutable environment in
/// the suite: the driver's leash, ending this process with its code.
#[test]
fn browser_leash() {
    if let Some(code) = symphony_chrome::leash_main() {
        std::process::exit(code);
    }
}
