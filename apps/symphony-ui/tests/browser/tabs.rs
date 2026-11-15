//! Seven tabs of one server, each following the feed. A browser keeps six HTTP/1.1 connections to a
//! host, and a stream of Server-Sent Events holds one for as long as it is open, so six tabs on
//! event streams would leave a seventh none for its reads: its page would never load. The web
//! client follows the feed on a WebSocket, which the browser counts apart, so a seventh tab draws
//! and follows the feed as the first did.

use std::time::Duration;

use serde_json::{Value, json};

use super::chrome::Chrome;
use super::site::Site;
use super::{SHOWN, chrome, common, look_parity, site};

/// The seven tabs, by their address and what shows each drawn: every page kind, the task board
/// twice, the last the tab the test drives.
const TABS: [(&str, &str); 7] = [
    ("/", "[data-ready=board] .hero-title"),
    ("/tasks", "[data-slug='tools/example']"),
    ("/task/visual/in-progress", "article.task"),
    ("/docs/look-parity.md", ".doc-card"),
    ("/queue", "[data-ready=queue]"),
    ("/agents", ".agents"),
    ("/tasks", "[data-slug='tools/example']"),
];

/// Run in each tab before its page: the feed's events its sockets hear, by name, and the
/// notifications the page shows, the owner having allowed them.
const HEARD: &str = r"(() => {
  window.__heard = {};
  const heard = (name) => { window.__heard[name] = (window.__heard[name] ?? 0) + 1; };
  const Socket = window.WebSocket;
  window.WebSocket = class extends Socket {
    constructor(...args) {
      super(...args);
      this.addEventListener('message', (message) => heard(String(message.data).split('\n')[0]));
    }
  };
  window.__notified = [];
  window.Notification = class {
    static permission = 'granted';
    static requestPermission() { return Promise.resolve('granted'); }
    constructor(title) { window.__notified.push(title); }
  };
})()";

/// What a tab says of itself when a wait on it stalls: what its streams heard, its feed's state,
/// whether it is hidden and the start of its text.
const STATE: &str = "JSON.stringify({heard: window.__heard ?? null, notified: window.__notified ?? null, \
feed: document.querySelector('main[data-feed]')?.dataset.feed ?? null, hidden: document.hidden, \
text: document.body ? document.body.innerText.slice(0, 600) : ''})";

/// The wall-clock backstop of a wait here on a tab behind the one the test drives, whose renderer
/// the driver's own waits do not follow: only a stall reaches it.
const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(30));

/// One of the tabs behind the one the test drives: its target and the session it is driven on.
struct Tab {
    path: &'static str,
    target: String,
    session: String,
}

impl Tab {
    /// A tab in front at `path`, hearing as [`HEARD`] says, as a reader opens one: it goes behind
    /// the next one opened. A page drawn behind others would differ from one in front (the task
    /// board draws a card once it comes into view, which a hidden tab never reports).
    fn open(chrome: &mut Chrome, site: &Site, path: &'static str) -> Tab {
        let made = chrome.browser_call("Target.createTarget", json!({ "url": "about:blank" }));
        let target = made["targetId"]
            .as_str()
            .expect("a tab's target")
            .to_owned();
        let attached = chrome.browser_call(
            "Target.attachToTarget",
            json!({ "targetId": target, "flatten": true }),
        );
        let session = attached["sessionId"]
            .as_str()
            .expect("a tab's session")
            .to_owned();
        chrome.call_on(&session, "Page.enable", json!({}));
        chrome.call_on(
            &session,
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": HEARD }),
        );
        chrome.call_on(&session, "Page.bringToFront", json!({}));
        chrome.call_on(&session, "Page.navigate", json!({ "url": site.url(path) }));
        Tab {
            path,
            target,
            session,
        }
    }

    /// `expression`'s value in the tab.
    fn eval(&self, chrome: &mut Chrome, expression: &str) -> Value {
        chrome.call_on(
            &self.session,
            "Runtime.evaluate",
            json!({ "expression": expression, "returnByValue": true }),
        )["result"]["value"]
            .clone()
    }

    /// Wait until `expression` is true in the tab; `what` names it.
    fn until(&self, chrome: &mut Chrome, what: &str, expression: &str) {
        test_time::until_true(format!("{} {what}", self.path), BACKSTOP, || {
            self.eval(chrome, expression) == json!(true)
        })
        .unwrap_or_else(|stall| panic!("{stall}; the tab says {}", self.eval(chrome, STATE)));
    }
}

/// The stand-in with the data of every page [`TABS`] opens.
fn site_for_every_page() -> Site {
    let site = site();
    let content = look_parity::responses();
    for path in [
        "/api/document",
        "/api/section",
        "/api/task-page",
        "/api/task-history",
        "/api/asks",
    ] {
        site.set_json(path, &content[path]);
    }
    site
}

/// Wait until the stand-in holds `streams` feed streams open, `sockets` of them on a WebSocket.
fn holding(site: &Site, streams: usize, sockets: usize) {
    test_time::until_true(
        format!("{streams} feed streams, {sockets} on a WebSocket"),
        BACKSTOP,
        || site.feeds() == streams && site.socket_feeds() == sockets,
    )
    .unwrap_or_else(|stall| {
        panic!(
            "{stall}: {} streams open, {} on a WebSocket, {} asked for",
            site.feeds(),
            site.socket_feeds(),
            site.feed_requests()
        )
    });
}

/// One event a board at the fixture's version takes: one more card waiting on the owner, which a
/// home page in the background tells them of. Its version, the cards then waiting and the event,
/// written by hand, as a change names its region before its data (a `json!` object sorts its
/// keys).
fn more_waiting() -> (u64, u64, String) {
    let board: Value = serde_json::from_str(&common::fixture("board.json")).unwrap();
    let version = board["version"].as_u64().unwrap() + 1;
    let waiting = board["counts"]["waiting"].as_u64().unwrap() + 1;
    let mut counts = board["counts"].clone();
    counts["waiting"] = json!(waiting);
    let event =
        format!(r#"{{"version":{version},"changes":[{{"region":"counts","data":{counts}}}]}}"#);
    (version, waiting, event)
}

#[test]
fn seven_tabs_of_one_server_each_draw_follow_the_feed_hear_one_event_and_let_go_when_closed() {
    let site = site_for_every_page();
    let Some(mut chrome) = chrome() else { return };
    let mut tabs = Vec::new();
    for (path, drawn) in &TABS[..6] {
        let tab = Tab::open(&mut chrome, &site, path);
        tab.until(
            &mut chrome,
            "draws its data",
            &format!("document.querySelector({}) !== null", json!(drawn)),
        );
        assert_eq!(
            tab.eval(&mut chrome, "typeof window.__heard"),
            json!("object"),
            "{path} runs without the script that counts what it hears"
        );
        tabs.push(tab);
    }
    // The seventh, the tab the test drives, reads, draws and follows as the six behind it do.
    chrome.call("Page.bringToFront", json!({}));
    chrome.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": HEARD }),
    );
    let (path, drawn) = TABS[6];
    chrome.open(&site.url(path));
    assert!(
        chrome.wait_for(
            &format!("document.querySelector({}) !== null", json!(drawn)),
            SHOWN
        ),
        "the seventh tab, {path}, never drew its data: {} streams open, {} on a WebSocket",
        site.feeds(),
        site.socket_feeds()
    );
    // Every tab follows the feed, each on a WebSocket.
    holding(&site, 7, 7);

    // One event reaches every tab, and the home page behind the others tells the owner of it.
    let (version, waiting, event) = more_waiting();
    site.push(version, &event);
    let heard = "(window.__heard?.delta ?? 0) >= 1";
    for tab in &tabs {
        tab.until(&mut chrome, "hears the event", heard);
    }
    assert!(
        chrome.wait_for(heard, SHOWN),
        "the seventh tab missed the event"
    );
    let home = &tabs[0];
    assert_eq!(home.eval(&mut chrome, "document.hidden"), json!(true));
    home.until(
        &mut chrome,
        "tells the owner from the background",
        &format!(
            "window.__notified.includes({})",
            json!(format!("{waiting} cards wait on you"))
        ),
    );

    // A tab closed stops counting.
    chrome.browser_call("Target.closeTarget", json!({ "targetId": tabs[1].target }));
    holding(&site, 6, 6);
}
