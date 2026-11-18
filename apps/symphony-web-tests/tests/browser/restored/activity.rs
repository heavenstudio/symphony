//! Toast lifetime control for the document and reconnect notice geometry case.
use super::super::chrome::Chrome;
use serde_json::json;

/// Hold, from the page's first script on, every timer the page sets for a toast's lifetime
/// (`TOAST_MS`), so the case says when that lifetime runs out instead of racing it: on a loaded
/// machine the page's clock runs seconds between two of the case's polls, past a toast's whole
/// lifetime. The page's other timers run as they are. Call it before `open`.
pub(super) fn hold_toast_lifetimes(browser: &mut Chrome) {
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": format!(
            r#"window.heldToastLifetimes = [];
            const pageTimer = window.setTimeout;
            window.setTimeout = function (callback, delay, ...rest) {{
                if (delay === {lifetime}) {{ heldToastLifetimes.push(callback); return 0; }}
                return pageTimer.call(this, callback, delay, ...rest);
            }};"#,
            lifetime = symphony_web_tests::TOAST_MS
        )}),
    );
}
