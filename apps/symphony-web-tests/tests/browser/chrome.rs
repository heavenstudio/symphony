//! Headless Chrome for these cases: the shared driver (crates/chrome-devtools), one browser per
//! test on a profile of its own, leashed by this test binary so it dies with the test process
//! however that ends. A test of several first visits takes a fresh context for each in its one
//! browser ([`Chrome::fresh_context`]), never a browser another test drives: a wait's allowance
//! is the work of the browser's whole process group. A driver failure fails the case with the driver's message.

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

    /// Leave the page for a blank one in a browser context of its own, as a launch leaves it: a
    /// first visit again (no cookie, storage or cache entry of the pages before) without another
    /// Chrome started, what was set on the page before set again by the test. A context keeps
    /// what it holds in memory, so a next visit in it is still a return visit.
    pub fn fresh_context(&mut self) {
        self.0.fresh_context().asserted();
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

    /// The window as a screen of `width` by `height` CSS px at one device pixel each, `mobile`
    /// as a phone's browser lays a page out (its meta viewport read, overlay scrollbars). Which
    /// widths are a phone's is each test's own rule, passed here; touch is asked for apart
    /// (`Emulation.setTouchEmulationEnabled`).
    pub fn viewport(&mut self, width: u32, height: u32, mobile: bool) {
        self.viewport_scaled(width, height, 1, mobile);
    }

    /// [`Chrome::viewport`] on a screen of `scale` device pixels to each CSS px.
    pub fn viewport_scaled(&mut self, width: u32, height: u32, scale: u32, mobile: bool) {
        self.call(
            "Emulation.setDeviceMetricsOverride",
            metrics(width, height, scale, mobile),
        );
    }

    /// The page's colour scheme (`light` or `dark`), as the operating system would set it.
    pub fn scheme(&mut self, mode: &str) {
        self.call("Emulation.setEmulatedMedia", media(mode));
    }

    /// Two of the page's frames, so what a viewport, a scheme or a click changed is laid out and
    /// painted before it is read.
    pub fn frames(&mut self) {
        self.eval(FRAMES);
    }

    /// [`Chrome::frames`] once the page's fonts are in, as a picture or a measure of text needs.
    pub fn frames_with_fonts(&mut self) {
        self.eval(&format!("document.fonts.ready.then(() => {FRAMES})"));
    }

    /// Go to `url` and wait for its document to be parsed.
    pub fn open(&mut self, url: &str) {
        self.0.open(url).asserted();
    }

    /// Evaluate `expression` in the page (awaiting a promise), its value as JSON.
    pub fn eval(&mut self, expression: &str) -> Value {
        self.0.eval(expression).asserted()
    }

    /// Approach a lazy region's slot as the reader does, then let its ordinary viewport observer
    /// mount it. The reader's scroll starts with their own input (a wheel), which ends whatever
    /// holds the page's place: the shell's hold of a row while the rows near it draw, and the
    /// board's return to the place its history entry keeps once it draws again. A script's scroll
    /// alone ends neither, and they put the page back where they hold it in the next frame, before
    /// the observer looks, so the slot is never seen. Both waits are [`Chrome::until`] their
    /// condition: the slot mounts in the frame its observer runs in, which a loaded machine's
    /// renderer can hold back over many of the page's turns, so no count of them bounds it.
    pub fn reveal(&mut self, selector: &str) {
        let slot = format!("document.querySelector({})", json!(selector));
        self.until(
            &format!("a slot to reveal: {selector}"),
            &format!("{slot} !== null"),
        );
        self.eval(&format!(
            "dispatchEvent(new WheelEvent('wheel')); {slot}.scrollIntoView({{block:'center',inline:'center'}})"
        ));
        self.until(
            &format!("the lazy slot mounted: {selector}"),
            &format!("!{slot}.querySelector(':scope > .viewport-sentinel')"),
        );
    }

    /// Wait until `expression` is `true` in the page: a wait on its condition, which only a stall
    /// past [`super::STALL`] ends, failing with `what` and the load average then (RULES.md
    /// `tests/no-test-fails-on-load`).
    #[track_caller]
    pub fn until(&mut self, what: &str, expression: &str) {
        if let Err(stall) =
            test_time::until_true(what, super::STALL, || self.eval(expression) == json!(true))
        {
            panic!("{stall}");
        }
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

    /// The viewport's pixels as a PNG.
    pub fn screenshot(&mut self) -> Vec<u8> {
        self.0.screenshot(json!({})).asserted()
    }

    /// Drain through a browser-scheduled observation marker.
    pub fn pump(&mut self, window: Duration) {
        self.0.pump(window).asserted();
    }

    /// A tap of a finger at the middle of what `selector` names, on a page emulating a touch
    /// screen (`Emulation.setTouchEmulationEnabled`).
    pub fn tap(&mut self, selector: &str) {
        let at = self.eval(&format!(
            "(() => {{ const r = document.querySelector({})?.getBoundingClientRect(); return r ? [r.left + r.width / 2, r.top + r.height / 2] : null; }})()",
            json!(selector)
        ));
        assert!(at.is_array(), "nothing to tap at {selector}");
        self.tap_at(&at[0], &at[1]);
    }

    /// A tap of a finger at `x`, `y` of the page's viewport.
    pub fn tap_at(&mut self, x: &Value, y: &Value) {
        for (kind, points) in [
            ("touchStart", json!([{"x": x, "y": y}])),
            ("touchEnd", json!([])),
        ] {
            self.call(
                "Input.dispatchTouchEvent",
                json!({"type": kind, "touchPoints": points}),
            );
        }
    }
}

/// A promise of two of the page's frames ([`Chrome::frames`]).
const FRAMES: &str =
    "new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))";

/// What `Emulation.setDeviceMetricsOverride` takes for [`Chrome::viewport_scaled`].
fn metrics(width: u32, height: u32, scale: u32, mobile: bool) -> Value {
    json!({"width": width, "height": height, "deviceScaleFactor": scale, "mobile": mobile})
}

/// What `Emulation.setEmulatedMedia` takes for [`Chrome::scheme`].
fn media(mode: &str) -> Value {
    json!({"features": [{"name": "prefers-color-scheme", "value": mode}]})
}

#[test]
fn the_viewport_and_scheme_commands_carry_what_they_are_given() {
    assert_eq!(
        metrics(390, 844, 3, true),
        json!({"width": 390, "height": 844, "deviceScaleFactor": 3, "mobile": true})
    );
    assert_eq!(
        metrics(0, 0, 1, false),
        json!({"width": 0, "height": 0, "deviceScaleFactor": 1, "mobile": false}),
        "a size of 0 lifts the override of that side, as the protocol reads it"
    );
    assert_eq!(
        media("dark"),
        json!({"features": [{"name": "prefers-color-scheme", "value": "dark"}]})
    );
    assert_eq!(
        media(""),
        json!({"features": [{"name": "prefers-color-scheme", "value": ""}]}),
        "no scheme named is the browser's own again"
    );
}

/// The page's window, pixel ratio and scheme as [`Chrome::viewport`], [`Chrome::viewport_scaled`]
/// and [`Chrome::scheme`] set them, each kept until set again, and a frame wait returning once
/// its frames are drawn.
#[test]
fn a_page_has_the_viewport_and_the_scheme_the_harness_sets_and_draws_the_frames_it_waits() {
    let Some(mut page) = super::chrome() else {
        return;
    };
    const SEEN: &str = "[innerWidth, innerHeight, devicePixelRatio, matchMedia('(prefers-color-scheme: dark)').matches]";
    page.viewport(1280, 900, false);
    page.scheme("dark");
    page.open("data:text/html,<meta name=viewport content='width=device-width,initial-scale=1'><p>page</p>");
    assert_eq!(page.eval(SEEN), json!([1280, 900, 1, true]));
    page.viewport_scaled(390, 844, 3, true);
    page.scheme("light");
    page.until(
        "a phone's screen in the light scheme",
        &format!("JSON.stringify({SEEN}) === '[390,844,3,false]'"),
    );
    page.viewport(600, 400, false);
    page.until(
        "one device pixel to each CSS px again, the scheme kept",
        &format!("JSON.stringify({SEEN}) === '[600,400,1,false]'"),
    );
    page.eval("window.drawn = 0; (function count() { window.drawn += 1; requestAnimationFrame(count); })()");
    let before = page.eval("window.drawn").as_u64().unwrap();
    page.frames();
    let between = page.eval("window.drawn").as_u64().unwrap();
    assert!(between >= before + 2, "{before} then {between}");
    page.frames_with_fonts();
    let after = page.eval("window.drawn").as_u64().unwrap();
    assert!(after >= between + 2, "{between} then {after}");
    assert_eq!(page.eval("document.fonts.status"), json!("loaded"));
}

/// Reexecuted in a separate process as a launch's leash, with no shared mutable environment in
/// the suite: the driver's leash, ending this process with its code.
#[test]
fn browser_leash() {
    if let Some(code) = symphony_chrome::leash_main() {
        std::process::exit(code);
    }
}
