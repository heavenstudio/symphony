//! A first visit to a kind of the web client (apps/symphony-web) asks for its entry and the chunks
//! that entry imports together: the shell preloads them beside the entry (`imports` in its
//! manifest, shell/shell.ts `initialize`), so React and the frame do not wait for the entry to
//! come and be read first, a round trip a phone over a tailnet pays. The entry's import then
//! takes each preloaded chunk: no chunk is asked for twice. A move into a kind keeps the page
//! before shown, its header with it, until the kind's entry is in, and preloads only the chunks no
//! earlier kind asked for.

use serde_json::json;

use super::{READY, SHOWN, STALL, assets::module, chrome, common, dist, site};

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
    let Some(mut chrome) = chrome() else { return };
    for (kind, path, ready) in KINDS {
        let entry = module(dist(), kind).script;
        let source = std::fs::read_to_string(dist().join(entry.trim_start_matches('/')))
            .expect("the kind's entry");
        // Read from the entry itself, not from the manifest the shell preloads by.
        let chunks = common::imported(&entry, &source);
        assert!(!chunks.is_empty(), "{kind}'s entry imports no chunk");
        // A context of its own, so no chunk is in the browser's cache from another kind.
        let site = site();
        chrome.fresh_context();
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

/// Follow a link to `path` as a click does: the shell moves to it in place.
fn follow(chrome: &mut super::chrome::Chrome, path: &str) {
    chrome.eval(&format!(
        "(() => {{ const link = document.createElement('a'); link.href = {}; document.body.append(link); link.click(); link.remove(); }})()",
        json!(path)
    ));
}

/// Wait until `selector` is drawn and the shell's navigation is done.
#[track_caller]
fn shows(chrome: &mut super::chrome::Chrome, selector: &str) {
    assert!(
        chrome.wait_for(
            &format!(
                "document.querySelector({}) !== null && {READY}",
                json!(selector)
            ),
            SHOWN
        ),
        "{selector} is not shown: {:?}",
        chrome.eval("document.body.innerText.slice(0, 2000)")
    );
}

/// The chunks the published entry `entry` imports.
fn chunks_of(entry: &str) -> Vec<String> {
    let source = std::fs::read_to_string(dist().join(entry.trim_start_matches('/')))
        .expect("the kind's entry");
    common::imported(entry, &source)
}

/// The board and the agents page share chunks (React and the frame, the board's feed and keep, the
/// task card): a move from one to the other preloads none of them again, so each is asked for once
/// and named by one link. A second link for a module Chrome already holds fetches nothing, so the
/// links are counted as well as the requests.
#[test]
fn a_move_between_two_web_kinds_asks_for_each_chunk_once_and_preloads_it_by_one_link() {
    let (board, agents) = (
        module(dist(), "board").script,
        module(dist(), "agents").script,
    );
    let (left, entered) = (chunks_of(&board), chunks_of(&agents));
    assert!(
        entered.iter().any(|chunk| left.contains(chunk)),
        "the board's {left:#?} and the agents page's {entered:#?} share no chunk"
    );
    let mut chunks = [left, entered].concat();
    chunks.sort();
    chunks.dedup();
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    // No module warmed and nothing read ahead: the requests are the two kinds' own.
    chrome.save_data();
    chrome.open(&site.url("/tasks"));
    shows(&mut chrome, "[data-slug='tools/example']");
    follow(&mut chrome, "/agents");
    shows(&mut chrome, "[data-ready=agents]");
    let requests = site.requests();
    let asked = |file: &String| requests.iter().filter(|path| *path == file).count();
    for file in [&board, &agents].into_iter().chain(&chunks) {
        assert_eq!(asked(file), 1, "{file} of {requests:#?}");
    }
    let links: Vec<String> = serde_json::from_value(chrome.eval(
        "[...document.querySelectorAll('link[rel=modulepreload]')].map(link => new URL(link.href).pathname)",
    ))
    .expect("the module preloads' paths");
    for chunk in &chunks {
        assert_eq!(
            links.iter().filter(|link| *link == chunk).count(),
            1,
            "{chunk} preloaded by one link: {links:#?}"
        );
    }
    let mut unique = links.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        unique.len(),
        links.len(),
        "a chunk preloaded twice: {links:#?}"
    );
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
                "document.querySelector({}) !== null && {READY}",
                json!(board)
            ),
            SHOWN
        ),
        "the board is never shown"
    );
    site.hold_route(&entry, true);
    follow(&mut chrome, path);
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
                "document.querySelector({}) !== null && {READY}",
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

/// Serve this route with the initial imports the native server takes from its shell's manifest.
/// The native router tests hold manifest/build selection; these cases hold browser reuse.
pub(super) fn serve_page(site: &super::site::Site, path: &str, kind: &str) {
    let shell = std::fs::read_to_string(dist().join("index.html")).unwrap();
    let links: String = module(dist(), kind)
        .imports
        .iter()
        .map(|href| format!("<link rel=\"modulepreload\" href=\"{href}\">"))
        .collect();
    let html = common::served_shell(&shell.replace("</head>", &format!("{links}</head>")));
    site.set_file(path, html.into_bytes(), "text/html; charset=utf-8");
}

#[test]
fn html_preloads_start_every_kinds_chunks_while_its_shell_is_held_and_are_reused() {
    let Some(mut chrome) = chrome() else { return };
    for (kind, path, ready) in KINDS {
        let site = site();
        serve_page(&site, path, kind);
        let html = std::fs::read_to_string(dist().join("index.html")).unwrap();
        let shell = html
            .split("src=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let spec = module(dist(), kind);
        site.hold_route(shell, true);
        chrome.fresh_context();
        chrome.save_data();
        chrome.call("Page.navigate", json!({"url": site.url(path)}));
        if let Err(stall) = test_time::until_true(
            format!("{kind}'s HTML preloads requested before the shell answers"),
            STALL,
            || {
                let requests = site.requests();
                requests.contains(&shell.to_string())
                    && spec.imports.iter().all(|file| requests.contains(file))
            },
        ) {
            panic!("{stall}: {:#?}", site.requests());
        }
        assert!(
            !site.requests().contains(&spec.script),
            "the shell still owns its entry import"
        );
        site.hold_route(shell, false);
        shows(&mut chrome, ready);
        let requests = site.requests();
        for file in &spec.imports {
            assert_eq!(
                requests.iter().filter(|path| *path == file).count(),
                1,
                "{kind}: {file}"
            );
        }
        let links: Vec<String> = serde_json::from_value(chrome.eval(
            "[...document.querySelectorAll('link[rel=modulepreload]')].map(link => link.getAttribute('href'))",
        )).unwrap();
        assert_eq!(
            links, spec.imports,
            "{kind}: the shell reuses every HTML preload without another link"
        );
    }
}
