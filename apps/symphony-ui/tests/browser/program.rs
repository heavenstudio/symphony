//! The program (src/split.rs): `/agents`, the chat and the menus share one script and main
//! module, each kind with a chunk of its own. A move between its pages fetches the page's chunk
//! alone, never the script or the main module again. The media layer is a chunk of its own, which a
//! page fetches only once it shows media: the board's chat, once its thread shows a mockup, here. A
//! page of the web client's takes its pictures up with its own layer and opens them in the
//! program's picture viewer, fetching no media chunk. The page shown before stays until the chunk
//! is in; a chunk that does not load ends in the shell's `gone` state (assets/shell.js) with that
//! page under its notice, never an empty page or one left waiting. A media layer that does not load
//! leaves its page shown under a notice whose retry loads it, or loads the page again when its build
//! is gone.

use serde_json::json;

use super::{
    SHOWN, STALL, Site,
    assets::{media_chunk, module},
    chrome,
    chrome::Chrome,
    common, dist,
};

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

/// How many times the stand-in was asked for the page at `path`.
fn loads(site: &Site, path: &str) -> usize {
    site.requests()
        .iter()
        .filter(|asked| *asked == path)
        .count()
}

/// The board, as the program's tests wait for it.
const BOARD: &str = "[data-slug='tools/example']";

#[test]
fn a_move_between_the_program_s_pages_fetches_the_page_s_chunk_and_never_the_program_again() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    // Each chunk is fetched by what shows it: no idle prefetch has the chat's in already.
    chrome.save_data();
    let (agents, chat) = (module(dist(), "tasks"), module(dist(), "panel"));
    assert_eq!(
        (&chat.script, &chat.wasm),
        (&agents.script, &agents.wasm),
        "/agents and the chat are one program"
    );
    let files = [&agents.script, &agents.wasm, agents.chunk(), chat.chunk()];
    chrome.open(&site.url("/tasks"));
    shows(&mut chrome, BOARD);
    assert_eq!(
        asks(&site, &files[2..]),
        [0, 0],
        "the board is a module of its own"
    );
    follow(&mut chrome, "/agents");
    shows(&mut chrome, ".agents");
    assert_eq!(
        asks(&site, &files),
        [1, 1, 1, 0],
        "/agents: the program's script and main module, and its page's chunk"
    );
    chrome.eval("document.querySelector('#quick-open').click()");
    assert!(
        chrome.wait_for("window.reviewPanelReady === true", SHOWN),
        "the chat never loaded"
    );
    assert_eq!(
        asks(&site, &files),
        [1, 1, 1, 1],
        "the chat: its chunk alone"
    );
    // Away to pages of other modules (Reports and a task's, the web client's) and back.
    follow(&mut chrome, "/reports");
    shows(&mut chrome, "[data-ready=reports]");
    follow(&mut chrome, "/task/visual/in-progress");
    shows(&mut chrome, "article.task");
    follow(&mut chrome, "/agents");
    shows(&mut chrome, ".agents");
    assert_eq!(
        asks(&site, &files),
        [1, 1, 1, 1],
        "back to the pages it has: nothing fetched again"
    );
}

/// How many times the stand-in was asked for the media layer's chunk ([`media_chunk`]).
pub(super) fn media_asked(site: &Site) -> usize {
    asks(site, &[&media_chunk(dist())])[0]
}

/// The stand-in with a task whose brief is a picture, `/near.svg`, below the page's head.
fn picture_site() -> Site {
    let site = site();
    let mut page = super::look_parity::responses()["/api/task-page"].clone();
    page["blocks"] = json!([{"type":"image","image":{"src":"/near.svg","alt":"near","presentation":{"attributes":{"width":"200","height":"200"}}}}]);
    site.set_json("/api/task-page", &page);
    site.set_file(
        "/near.svg",
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200"/>"#.to_vec(),
        "image/svg+xml",
    );
    site
}

/// A task's page is the web client's: its own media layer takes the brief's picture up, which
/// opens in the program's picture viewer, and the program's media layer is never fetched, with a
/// picture or without one.
#[test]
fn a_web_page_takes_its_pictures_up_itself_and_opens_them_in_the_program_s_viewer() {
    let site = site();
    {
        let Some(mut chrome) = chrome() else { return };
        chrome.open(&site.url("/task/visual/in-progress"));
        shows(&mut chrome, "article.task");
        // The timeline comes once the page has drawn its data and is idle.
        assert!(chrome.wait_for(
            "document.getElementById('timeline-root') !== null && document.querySelector('.tl-drawn') !== null",
            SHOWN
        ));
        assert_eq!(media_asked(&site), 0, "{:?}", site.requests());
    }
    // The same task, its brief now a picture, in a browser of its own.
    let site = picture_site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/task/visual/in-progress"));
    shows(&mut chrome, "article.task");
    chrome.eval("document.querySelector('main img[alt=near]').scrollIntoView({block:'center'})");
    assert!(
        chrome.wait_for(
            "document.querySelector('main img[alt=near][data-zoom-ready]')?.naturalWidth === 200",
            SHOWN
        ),
        "the picture is never taken up: {:?}; {:?}",
        chrome.eval("[...document.querySelectorAll('main img')].map(image => image.outerHTML)"),
        site.requests()
    );
    chrome.eval("document.querySelector('main img[alt=near]').click()");
    assert!(
        chrome.wait_for(
            "document.querySelector('#media-lightbox[data-state=open] img')?.getAttribute('src') === '/near.svg'",
            SHOWN
        ),
        "the picture never opens in the viewer: {:?}",
        chrome.eval("document.getElementById('media-lightbox')?.outerHTML")
    );
    assert_eq!(media_asked(&site), 0, "{:?}", site.requests());
}

/// The stand-in with the chat's thread, ask 7, showing a mockup, `/mockup.html`.
fn mockup_site() -> Site {
    let site = site();
    let mut thread = super::panel_open::chat(&["First question", "A mockup"]);
    thread["ask"]["messages"][1]["blocks"] =
        json!([{"type":"media","media":{"kind":"mockup","src":"/mockup.html","label":"Mockup"}}]);
    site.set_json("/api/ask", &thread);
    site.set_file(
        "/mockup.html",
        b"<!doctype html><title>Mockup</title>".to_vec(),
        "text/html",
    );
    site
}

/// Open the board, its chat's draft on ask 7, and mark the page (`__kept`, which a load of the
/// page again drops).
fn open_board(chrome: &mut Chrome, site: &Site) {
    chrome.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": "localStorage.setItem('review-quick-draft', JSON.stringify({open: false, task: false, ask: 7}));"}),
    );
    chrome.open(&site.url("/tasks"));
    shows(chrome, BOARD);
    chrome.eval("window.__kept = true");
}

/// Open the chat and scroll its thread's mockup into view, as a reader would: it shows, which asks
/// for the media layer.
fn show_mockup(chrome: &mut Chrome) {
    chrome.eval("document.querySelector('#quick-open').click()");
    assert!(
        chrome.wait_for(
            "document.querySelector('#quick-root .mockup-shell') !== null",
            SHOWN
        ),
        "the chat's thread is never drawn: {}",
        chrome.eval("document.getElementById('quick-sheet')?.innerText ?? null")
    );
    chrome.eval(
        "document.querySelector('#quick-root .mockup-shell').scrollIntoView({block:'center'})",
    );
}

/// The media layer took the mockup up: it scales the mockup's frame once its chunk is in.
const TAKEN_UP: &str = "document.querySelector('#quick-root .mockup-frame iframe')?.getAttribute('style')?.includes('transform:scale(') === true";

/// The media layer's notice, up.
const NOTICE: &str = "document.querySelector('#media-error[role=alert]') !== null";

#[test]
fn a_media_layer_whose_chunk_does_not_load_says_so_and_its_retry_loads_it() {
    let site = mockup_site();
    let media = media_chunk(dist());
    // Its build still served, the media layer's chunk does not load: the page stays, and says so.
    site.retire(&media);
    let Some(mut chrome) = chrome() else { return };
    open_board(&mut chrome, &site);
    show_mockup(&mut chrome);
    assert!(
        chrome.wait_for(NOTICE, SHOWN),
        "a media layer that does not load is silent: {:?}",
        site.requests()
    );
    assert_eq!(
        chrome.eval("document.querySelector('#media-error')?.textContent"),
        json!("Media could not be loaded. Retry loading media")
    );
    assert_eq!(chrome.eval("window.__kept === true"), json!(true));
    assert!(drawn(&mut chrome, BOARD), "the board is gone");
    assert!(
        drawn(&mut chrome, "#quick-root .mockup-shell"),
        "the mockup is gone"
    );
    assert_eq!(chrome.eval(TAKEN_UP), json!(false));
    assert_eq!(media_asked(&site), 1, "{:?}", site.requests());
    // A later load works: the retry shows the media again, which asks for the chunk again.
    site.restore(&media);
    chrome.eval("document.querySelector('#media-error button').click()");
    assert!(
        chrome.wait_for(TAKEN_UP, SHOWN),
        "the retry never loads the media layer: {:?}",
        site.requests()
    );
    assert_eq!(media_asked(&site), 2, "{:?}", site.requests());
    assert_eq!(
        chrome.eval("document.getElementById('media-error') === null && window.__kept === true"),
        json!(true),
        "the notice stays up, or the page was loaded again"
    );
}

/// The media layer's notice comes down once the page moves on, as the shell's own notice of a page
/// that did not load does: the page it named is no longer shown.
#[test]
fn a_media_notice_comes_down_once_the_page_moves_on() {
    let site = mockup_site();
    site.retire(&media_chunk(dist()));
    let Some(mut chrome) = chrome() else { return };
    open_board(&mut chrome, &site);
    show_mockup(&mut chrome);
    assert!(
        chrome.wait_for(NOTICE, SHOWN),
        "a media layer that does not load is silent: {:?}",
        site.requests()
    );
    // To a page that shows no media of its own (the chat's goes with the board's layer).
    follow(&mut chrome, "/reports");
    shows(&mut chrome, "[data-ready=reports]");
    assert_eq!(
        chrome.eval("document.getElementById('media-error') === null && window.__kept === true"),
        json!(true),
        "the notice stays up on the reports, or the page was loaded again"
    );
}

#[test]
fn a_media_layer_whose_build_is_gone_loads_the_page_again() {
    let site = mockup_site();
    let Some(mut chrome) = chrome() else { return };
    open_board(&mut chrome, &site);
    assert_eq!(loads(&site, "/tasks"), 1, "{:?}", site.requests());
    // Its build gone from the server, the media layer's chunk with it: once the mockup shows, the
    // page loads again, onto the build served now.
    site.retire(&media_chunk(dist()));
    site.retire(&shell(&mut chrome));
    show_mockup(&mut chrome);
    if let Err(stall) =
        test_time::until_true("a page whose media is gone loaded again", STALL, || {
            loads(&site, "/tasks") >= 2
        })
    {
        panic!("{stall}: {:?}", site.requests());
    }
    assert_eq!(media_asked(&site), 1, "{:?}", site.requests());
}

#[test]
fn a_move_into_a_program_page_keeps_the_page_before_shown_until_its_chunk_is_in() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/tasks"));
    shows(&mut chrome, BOARD);
    // Each kind the program's loader is asked for, in turn, as the program's script installs it.
    chrome.eval(
        "(() => { let loader; Object.defineProperty(globalThis, 'reviewChunk', { configurable: true, get: () => loader, set: load => { loader = kind => { (window.__asked ||= []).push(kind); return load(kind); }; } }); })()",
    );
    let chunk = module(dist(), "tasks").chunk().clone();
    site.hold_route(&chunk, true);
    follow(&mut chrome, "/agents");
    // Asked for /agents' chunk, which is held, the move waits on it alone: the board stays shown,
    // its header with it, until the chunk is in.
    assert!(
        chrome.wait_for("window.__asked?.includes('tasks') === true", SHOWN),
        "/agents' chunk is never asked for: {:?}",
        chrome.eval("window.__asked")
    );
    assert_eq!(
        chrome.eval("document.documentElement.dataset.navigation"),
        json!("loading")
    );
    assert!(
        drawn(&mut chrome, BOARD),
        "the board is hidden before /agents can draw"
    );
    assert!(drawn(&mut chrome, ".app-header"), "the header is gone");
    assert!(!drawn(&mut chrome, ".agents"));
    site.hold_route(&chunk, false);
    shows(&mut chrome, ".agents");
    assert!(
        !drawn(&mut chrome, BOARD),
        "the board is hidden once /agents draws"
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
    chrome.open(&site.url("/reports"));
    shows(&mut chrome, "[data-ready=reports]");
    chrome.eval("window.__kept = true");
    let shell = shell(&mut chrome);
    // Its build still served, a chunk that does not load is said, and the page stays.
    site.retire(module(dist(), "tasks").chunk());
    follow(&mut chrome, "/agents");
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
    assert_eq!(loads(&site, "/agents"), 0, "{:?}", site.requests());
    // What it leaves on screen: the reports and their header, under the notice.
    assert!(
        drawn(&mut chrome, "[data-ready=reports]"),
        "the reports are hidden under the notice"
    );
    assert!(drawn(&mut chrome, ".app-header"), "the header is gone");
    assert!(!drawn(&mut chrome, ".agents"));
    // Its build gone from the server, the page loads again, onto the build served now.
    site.retire(&shell);
    chrome.eval("document.querySelector('#module-error button').click()");
    if let Err(stall) =
        test_time::until_true("a page whose chunk is gone loaded again", STALL, || {
            loads(&site, "/agents") >= 1
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
