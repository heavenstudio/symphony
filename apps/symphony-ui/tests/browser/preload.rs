//! A first visit to a kind of the web client (apps/symphony-web) asks for its entry and the chunks
//! that entry imports together: the shell preloads them beside the entry (`imports` in its
//! manifest, assets/shell.js `initialize`), so React and the frame do not wait for the entry to
//! come and be read first, a round trip a phone over a tailnet pays. The entry's import then
//! takes each preloaded chunk: no chunk is asked for twice. A move into a kind keeps the page
//! before shown, its header with it, until the kind's entry is in.

use serde_json::json;

use super::{
    SHOWN, STALL,
    assets::{module, web},
    chrome, common, dist, site,
};

/// Each kind of the web client, the page that shows it and what the page draws once it is in (a
/// task's page, the stand-in having no page of its task, says so once it has asked for it).
const KINDS: [(&str, &str, &str); 8] = [
    ("review", "/", ".hero-title"),
    ("board", "/tasks", "[data-slug='tools/example']"),
    ("agents", "/agents", "[data-ready=agents]"),
    ("docs", "/docs/", "[data-ready=folder]"),
    ("queue", "/queue", "[data-ready=queue]"),
    ("reports", "/reports", "[data-ready=reports]"),
    ("runs", "/runs", "[data-ready=runs]"),
    (
        "task",
        "/task/tools/example",
        ".task-page[data-ready=missing]",
    ),
];

#[test]
fn a_web_kind_s_first_visit_asks_for_the_chunks_its_entry_imports_while_the_entry_is_held() {
    for (kind, path, ready) in KINDS {
        assert!(web(dist(), kind), "{kind} is a web kind");
        let entry = module(dist(), kind).script;
        let source = std::fs::read_to_string(dist().join(entry.trim_start_matches('/')))
            .expect("the kind's entry");
        // Read from the entry itself, not from the manifest the shell preloads by.
        let chunks = common::imported(&entry, &source);
        assert!(!chunks.is_empty(), "{kind}'s entry imports no chunk");
        // A profile of its own, so no chunk is in the browser's cache from another kind.
        let site = site();
        let Some(mut chrome) = chrome() else { return };
        site.hold_route(&entry, true);
        chrome.open(&site.url(path));
        let asked = |file: &String| site.requests().iter().filter(|p| *p == file).count();
        // The entry is never answered meanwhile: what is asked for is asked without it.
        if let Err(stall) = test_time::until_true(
            format!("{kind}'s chunks asked for while its entry is held"),
            STALL,
            || asked(&entry) > 0 && chunks.iter().all(|chunk| asked(chunk) > 0),
        ) {
            panic!("{stall}: {entry} and {chunks:#?} of {:#?}", site.requests());
        }
        site.hold_route(&entry, false);
        assert!(
            chrome.wait_for(
                &format!("document.querySelector({}) !== null", json!(ready)),
                SHOWN
            ),
            "{path} did not show {ready}: {:?}",
            chrome.eval("document.body.innerText")
        );
        assert_eq!(asked(&entry), 1, "{kind}'s entry");
        for chunk in &chunks {
            assert_eq!(
                asked(chunk),
                1,
                "{kind}'s entry took the preloaded {chunk}, no second request"
            );
        }
    }
}

/// Whether an element `selector` names is drawn, none of its parents hidden.
fn drawn(chrome: &mut super::chrome::Chrome, selector: &str) -> bool {
    chrome.eval(&format!(
        "[...document.querySelectorAll({})].some(element => element.getClientRects().length > 0)",
        json!(selector)
    )) == json!(true)
}

#[test]
fn a_move_into_a_web_kind_keeps_the_page_before_shown_until_its_entry_is_in() {
    let (path, ready) = ("/task/tools/example", ".task-page[data-ready=missing]");
    let board = "[data-slug='tools/example']";
    let entry = module(dist(), "task").script;
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    // No read ahead has the task's entry in already.
    chrome.save_data();
    chrome.open(&site.url("/tasks"));
    assert!(
        chrome.wait_for(
            &format!(
                "document.querySelector({}) !== null && document.documentElement.dataset.navigation === 'ready'",
                json!(board)
            ),
            SHOWN
        ),
        "the board is never shown"
    );
    site.hold_route(&entry, true);
    chrome.eval(&format!(
        "(() => {{ const link = document.createElement('a'); link.href = {}; document.body.append(link); link.click(); link.remove(); }})()",
        json!(path)
    ));
    // Asked for the task's entry, which is held, the move waits on it alone: the board stays shown,
    // its header with it, until the entry is in.
    if let Err(stall) = test_time::until_true("the task's entry asked for", STALL, || {
        site.requests().contains(&entry)
    }) {
        panic!("{stall}: {entry} of {:#?}", site.requests());
    }
    assert_eq!(
        chrome.eval("document.documentElement.dataset.navigation"),
        json!("loading")
    );
    assert!(
        drawn(&mut chrome, board),
        "the board is hidden before the task's page can draw"
    );
    assert!(drawn(&mut chrome, ".app-header"), "the header is gone");
    assert!(!drawn(&mut chrome, ".task-page"));
    site.hold_route(&entry, false);
    assert!(
        chrome.wait_for(
            &format!(
                "document.querySelector({}) !== null && document.documentElement.dataset.navigation === 'ready'",
                json!(ready)
            ),
            SHOWN
        ),
        "{path} did not show {ready}: {:?}",
        chrome.eval("document.body.innerText")
    );
    assert!(
        !drawn(&mut chrome, board),
        "the board is hidden once the task's page draws"
    );
}
