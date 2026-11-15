//! A full page load in one tab, the page before it left kept by the browser for back and forward
//! (its back/forward cache). A kept page holds no feed stream: it would keep its socket, and the
//! server its subscription, for as long as it is kept. A page shown again from the cache reads what
//! changed while it was kept and follows the feed again.

use std::time::Duration;

use serde_json::{Value, json};

use super::chrome::Chrome;
use super::site::Site;
use super::{
    CARD_CHANGED, HEALED, PING_MS, SHOWN, STALL, board_at, card_event, chrome, shows_waiting, site,
    texts, wait_for_board,
};

/// What the page shows, for a failure's message.
const PAGE: &str = "document.body ? document.body.innerText.slice(0, 2000) : '(no body)'";

/// Wait until the page shown has opened a feed stream since the stand-in had served `before` feed
/// requests, and that stream is the only one open: the pages kept for back and forward hold none.
/// A quiet machine has it within 10 s; a stall past its backstop ([`test_time::backstop`]) is the
/// error.
fn follows_alone(site: &Site, before: usize) -> Result<(), test_time::Stall> {
    test_time::until_true(
        "the page shown following the feed on the only stream open",
        test_time::backstop(Duration::from_secs(10)),
        || site.feed_requests() > before && site.feeds() == 1,
    )
}

/// The main frame's navigations so far, as the browser reported them.
fn navigations(chrome: &mut Chrome) -> Vec<Value> {
    chrome
        .wait_for_protocol_events("Page.frameNavigated", 0, Duration::ZERO)
        .into_iter()
        .filter(|navigated| navigated["frame"]["parentId"].is_null())
        .collect()
}

/// Go back one entry, as the browser's Back does, and wait for the page; how the browser showed
/// it (`BackForwardCacheRestore` for a page it kept), and why it did not keep it, if it said.
fn go_back(chrome: &mut Chrome) -> (Value, Vec<Value>) {
    let history = chrome.call("Page.getNavigationHistory", json!({}));
    let current = history["currentIndex"].as_u64().expect("a current entry") as usize;
    let entry = history["entries"][current - 1]["id"].clone();
    let seen = navigations(chrome).len();
    chrome.call("Page.navigateToHistoryEntry", json!({ "entryId": entry }));
    let shown = test_time::until("going back showing a page", STALL, || {
        let navigated = navigations(chrome);
        (navigated.len() > seen).then(|| navigated[seen]["type"].clone())
    })
    .unwrap_or_else(|stall| panic!("{stall}; the history was {history}"));
    let not_kept =
        chrome.wait_for_protocol_events("Page.backForwardCacheNotUsed", 0, Duration::ZERO);
    (shown, not_kept)
}

#[test]
fn a_page_kept_for_back_closes_its_feed_stream_and_shows_what_changed_once_shown_again() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    follows_alone(&site, 0).expect("the board never followed the feed");
    assert_eq!(texts(&mut chrome, ".tile b")[0], "37");
    // A mark only this document holds: a page loaded again would not have it.
    chrome.eval("window.__kept = true");

    let before = site.feed_requests();
    chrome.open(&site.url("/tasks"));
    assert!(
        chrome.wait_for(
            "document.querySelector(\"[data-slug='tools/example']\") !== null",
            SHOWN
        ),
        "the tasks board never drew its data; the page shows {}",
        chrome.eval(PAGE)
    );
    if let Err(stall) = follows_alone(&site, before) {
        panic!(
            "{stall}: the kept board still holds its stream: {} open",
            site.feeds()
        );
    }

    // While the board is kept, it moves on.
    site.set_board(&board_at(4190, 99, PING_MS));
    let before = site.feed_requests();
    let (shown, not_kept) = go_back(&mut chrome);
    assert_eq!(
        shown,
        json!("BackForwardCacheRestore"),
        "the board was not kept: {not_kept:?}"
    );
    assert_eq!(
        chrome.eval("window.__kept === true && location.pathname"),
        json!("/"),
        "the board shown is not the page that was kept"
    );
    assert!(
        shows_waiting(&mut chrome, 99),
        "the board shown again still shows what it showed when it was left: {:?}",
        texts(&mut chrome, ".tile b")
    );
    assert!(
        chrome.wait_for(
            "document.querySelector('[data-feed=live]') !== null",
            HEALED
        ),
        "the board shown again does not follow the feed: {} open, {} asked for",
        site.feeds(),
        site.feed_requests()
    );
    if let Err(stall) = follows_alone(&site, before) {
        panic!(
            "{stall}: the board shown again does not follow the feed alone: {} open, {} asked for",
            site.feeds(),
            site.feed_requests()
        );
    }
    // It follows the feed from the board it read: the next event changes its card in place.
    site.push(4191, &card_event(4191));
    assert!(
        chrome.wait_for(CARD_CHANGED, Duration::from_secs(10)),
        "the next event after the board was shown again was lost: {:?}",
        texts(&mut chrome, ".nextcard strong")
    );
}
