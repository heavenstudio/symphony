//! The desktop app's one script in a server's pages (`apps/symphony-desktop/src/notification.ts`):
//! notifications read as denied, so home, which tells the owner from the background when the count
//! rises, tells them nothing in the app's window, which tells them itself, and throws nothing.

use serde_json::json;

use super::tabs::{self, HEARD, Tab};
use super::{chrome, site};

/// The script the app runs before every page it shows.
const STUB: &str = include_str!("../../../symphony-desktop/src/notification.ts");

/// Run before the page: every error and unhandled rejection it throws, by its message.
const THROWN: &str = r"(() => {
  window.__thrown = [];
  addEventListener('error', (event) => window.__thrown.push(String(event.message)));
  addEventListener('unhandledrejection', (event) => window.__thrown.push(String(event.reason)));
})()";

/// What the stub answers a page that asks: its permission, a request's answer by callback and by
/// promise, and what showing one anyway does.
const ASKED: &str = r"(async () => {
  let called = null;
  const promised = await Notification.requestPermission((answer) => { called = answer; });
  let shown;
  try { new Notification('a card waits on you'); shown = 'shown'; }
  catch (error) { shown = error instanceof TypeError ? 'TypeError' : String(error); }
  return JSON.stringify({ permission: Notification.permission, called, promised, shown });
})()";

#[test]
fn a_rising_count_on_home_under_the_desktop_apps_stub_neither_throws_nor_notifies() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    // A browser's tab beside the app's, so the test can tell a notice from none.
    let browser = Tab::running(&mut chrome, &site, "/", &[HEARD, THROWN]);
    let app = Tab::running(&mut chrome, &site, "/", &[HEARD, THROWN, STUB]);
    let drawn = "document.querySelector('[data-ready=board] .hero-title') !== null";
    browser.until(&mut chrome, "draws home", drawn);
    app.until(&mut chrome, "draws home", drawn);
    // Both behind the page the test drives, where home tells the owner of a rising count.
    chrome.call("Page.bringToFront", json!({}));
    tabs::holding(&site, 2, 2);

    let (version, waiting, event) = tabs::more_waiting();
    site.push(version, &event);
    browser.until(
        &mut chrome,
        "tells the owner from the background",
        &format!(
            "window.__notified.includes({})",
            json!(format!("{waiting} cards wait on you"))
        ),
    );
    app.until(
        &mut chrome,
        "hears the event",
        "(window.__heard?.delta ?? 0) >= 1",
    );
    app.until(
        &mut chrome,
        "draws the count it heard",
        &format!(
            "document.body.innerText.includes({})",
            json!(format!("{waiting} cards wait on you"))
        ),
    );
    assert_eq!(app.eval(&mut chrome, "document.hidden"), json!(true));
    assert_eq!(
        app.eval(&mut chrome, "JSON.stringify(window.__notified)"),
        json!("[]"),
        "the app's page told the owner itself"
    );
    assert_eq!(
        app.eval(&mut chrome, "JSON.stringify(window.__thrown)"),
        json!("[]"),
        "the app's page threw"
    );
    let asked: serde_json::Value = serde_json::from_str(
        app.eval_promise(&mut chrome, ASKED)
            .as_str()
            .expect("the stub's answers"),
    )
    .unwrap();
    assert_eq!(
        asked,
        json!({
            "permission": "denied",
            "called": "denied",
            "promised": "denied",
            "shown": "TypeError",
        })
    );
}
