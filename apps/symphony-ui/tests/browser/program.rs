//! The program (src/split.rs): the task pages with `/agents`, Reports, the chat, the menus and the
//! task timeline share one script and main module, each kind with a chunk of its own. A move
//! between its pages fetches the page's chunk alone, never the script or the main module again.
//! The page shown before stays until the chunk is in; a chunk that does not load ends in the shell's
//! `gone` state (assets/shell.js) with that page under its notice, never an empty page or one left
//! waiting.

use serde_json::json;

use super::{SHOWN, STALL, Site, assets::module, chrome, chrome::Chrome, common, dist};

/// The stand-in with what a task page reads.
fn site() -> Site {
    let site = super::site();
    let content = super::look_parity::responses();
    for path in ["/api/task-page", "/api/task-history", "/api/asks"] {
        site.set_json(path, &content[path]);
    }
    site.set_file(
        symphony_api::TASK_TIMELINE_PATH,
        common::fixture("task-timeline.json").into_bytes(),
        "application/json",
    );
    site
}

/// Follow a link to `path` as a click does: the shell moves to it in place.
fn follow(chrome: &mut Chrome, path: &str) {
    chrome.eval(&format!(
        "(() => {{ const link = document.createElement('a'); link.href = {}; document.body.append(link); link.click(); link.remove(); }})()",
        json!(path)
    ));
}

/// Wait until `selector` is drawn and the shell's navigation is done.
#[track_caller]
fn shows(chrome: &mut Chrome, selector: &str) {
    assert!(
        chrome.wait_for(
            &format!(
                "document.querySelector({}) !== null && document.documentElement.dataset.navigation === 'ready'",
                json!(selector)
            ),
            SHOWN
        ),
        "{selector} is not shown: {:?}",
        chrome.eval("document.body.innerText.slice(0, 2000)")
    );
}

/// Whether an element `selector` names is drawn, none of its parents hidden.
fn drawn(chrome: &mut Chrome, selector: &str) -> bool {
    chrome.eval(&format!(
        "[...document.querySelectorAll({})].some(element => element.getClientRects().length > 0)",
        json!(selector)
    )) == json!(true)
}

/// How many times the stand-in was asked for each of `files`.
fn asks(site: &Site, files: &[&String]) -> Vec<usize> {
    let asked = site.requests();
    files
        .iter()
        .map(|file| asked.iter().filter(|path| path == file).count())
        .collect()
}

#[test]
fn a_move_between_the_program_s_pages_fetches_the_page_s_chunk_and_never_the_program_again() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    let (tasks, reports) = (module(dist(), "tasks"), module(dist(), "reports"));
    assert_eq!(
        (&reports.script, &reports.wasm),
        (&tasks.script, &tasks.wasm),
        "the task pages and Reports are one program"
    );
    let files = [&tasks.script, &tasks.wasm, tasks.chunk(), reports.chunk()];
    chrome.open(&site.url("/tasks"));
    shows(&mut chrome, "[data-slug='tools/example']");
    assert_eq!(
        asks(&site, &files[2..]),
        [0, 0],
        "the board is a module of its own"
    );
    follow(&mut chrome, "/task/visual/in-progress");
    shows(&mut chrome, "article.task");
    assert_eq!(
        asks(&site, &files),
        [1, 1, 1, 0],
        "a task page: the program's script and main module, and the task pages' chunk"
    );
    follow(&mut chrome, "/reports");
    shows(&mut chrome, "[data-ready=reports]");
    assert_eq!(
        asks(&site, &files),
        [1, 1, 1, 1],
        "Reports: its chunk alone"
    );
    follow(&mut chrome, "/agents");
    shows(&mut chrome, ".agents");
    follow(&mut chrome, "/reports");
    shows(&mut chrome, "[data-ready=reports]");
    assert_eq!(
        asks(&site, &files),
        [1, 1, 1, 1],
        "back to the pages it has: nothing fetched again"
    );
}

#[test]
fn a_move_into_a_program_page_keeps_the_page_before_shown_until_its_chunk_is_in() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/tasks"));
    shows(&mut chrome, "[data-slug='tools/example']");
    // Each kind the program's loader is asked for, in turn, as the program's script installs it.
    chrome.eval(
        "(() => { let loader; Object.defineProperty(globalThis, 'reviewChunk', { configurable: true, get: () => loader, set: load => { loader = kind => { (window.__asked ||= []).push(kind); return load(kind); }; } }); })()",
    );
    let chunk = module(dist(), "tasks").chunk().clone();
    site.hold_route(&chunk, true);
    follow(&mut chrome, "/task/visual/in-progress");
    // Asked for the task pages' chunk, which is held, the move waits on it alone: the board stays
    // shown, its header with it, until the chunk is in.
    assert!(
        chrome.wait_for("window.__asked?.includes('tasks') === true", SHOWN),
        "the task pages' chunk is never asked for: {:?}",
        chrome.eval("window.__asked")
    );
    assert_eq!(
        chrome.eval("document.documentElement.dataset.navigation"),
        json!("loading")
    );
    assert!(
        drawn(&mut chrome, "[data-slug='tools/example']"),
        "the board is hidden before the task page can draw"
    );
    assert!(drawn(&mut chrome, ".app-header"), "the header is gone");
    assert!(!drawn(&mut chrome, "article.task"));
    site.hold_route(&chunk, false);
    shows(&mut chrome, "article.task");
    assert!(
        !drawn(&mut chrome, "[data-slug='tools/example']"),
        "the board is hidden once the task page draws"
    );
}

/// The page's own shell, as its script names it.
fn shell(chrome: &mut Chrome) -> String {
    chrome
        .eval("document.querySelector(\"script[src*='/assets/symphony-shell-']\").getAttribute('src')")
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn a_page_whose_chunk_does_not_load_ends_in_the_gone_state_never_left_waiting() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/task/visual/in-progress"));
    shows(&mut chrome, "article.task");
    chrome.eval("window.__kept = true");
    let shell = shell(&mut chrome);
    let loads = |site: &Site| {
        site.requests()
            .iter()
            .filter(|path| *path == "/reports")
            .count()
    };
    // Its build still served, a chunk that does not load is said, and the page stays.
    site.retire(module(dist(), "reports").chunk());
    follow(&mut chrome, "/reports");
    assert!(
        chrome.wait_for(
            "document.documentElement.dataset.navigation === 'failed'",
            SHOWN
        ),
        "the move is left waiting: {:?}",
        chrome.eval("document.documentElement.dataset.navigation")
    );
    assert_eq!(
        chrome.eval("document.querySelector('#module-error[role=alert]')?.textContent"),
        json!("This page could not be loaded. Retry")
    );
    assert_eq!(chrome.eval("window.__kept === true"), json!(true));
    assert_eq!(loads(&site), 0, "{:?}", site.requests());
    // What it leaves on screen: the task page and its header, under the notice.
    assert!(
        drawn(&mut chrome, "article.task"),
        "the task page is hidden under the notice"
    );
    assert!(drawn(&mut chrome, ".app-header"), "the header is gone");
    assert!(!drawn(&mut chrome, "[data-ready=reports]"));
    // Its build gone from the server, the page loads again, onto the build served now.
    site.retire(&shell);
    chrome.eval("document.querySelector('#module-error button').click()");
    if let Err(stall) =
        test_time::until_true("a page whose chunk is gone loaded again", STALL, || {
            loads(&site) >= 1
        })
    {
        panic!("{stall}: {:?}", site.requests());
    }
}

#[test]
fn a_chat_whose_chunk_does_not_load_says_so_with_its_retry() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    // The first open fetches the chat; no idle prefetch has it in the cache already.
    chrome.save_data();
    chrome.open(&site.url("/"));
    super::wait_for_board(&mut chrome);
    site.retire(module(dist(), "panel").chunk());
    chrome.eval("document.querySelector('#quick-open').click()");
    assert!(
        chrome.wait_for(
            "document.querySelector('#panel-error[role=alert]') !== null && document.getElementById('panel-loading') === null",
            SHOWN
        ),
        "the chat is left loading: {:?}",
        chrome.eval("document.body.innerHTML.slice(-2000)")
    );
    assert_eq!(
        chrome.eval("document.querySelector('#panel-error')?.textContent"),
        json!("Conversation could not be loaded. Retry loading conversation")
    );
    assert_eq!(
        chrome.eval("window.reviewPanelReady === true"),
        json!(false)
    );
}
