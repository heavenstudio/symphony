//! The modules every page mounts over its own, the web client's persistent kinds: the chat
//! (`panel`, apps/symphony-web/src/panel.tsx) and the menus (`overlay`). The chat opened on one page
//! and then on another fetches its script, the chunks it imports and its styles once, and none of
//! the menus'. A page takes its own pictures up with its own layer, fetching nothing of the chat's,
//! and the chat's pictures open in the menus' viewer on every page (#6183). A chat whose script is
//! held keeps the page shown, its sheet loading over it; one whose script does not load says so with
//! its retry, the page under its notice, or loads the page again when its build is gone. A media
//! layer is in the chunks of the chat and of each page that shows media, so it loads, or fails, with
//! them: no file of its own (panel_open.rs). The other kinds keep the same: preload.rs (a move into
//! a kind keeps the page before shown while its entry is held) and main.rs (a page whose module
//! does not load).

use serde_json::json;

use super::{READY, SHOWN, STALL, Site, assets::module, chrome, chrome::Chrome, common, dist};

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
                "document.querySelector({}) !== null && {READY}",
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

/// The board, as these tests wait for it.
const BOARD: &str = "[data-slug='tools/example']";

/// The chat, drawn and up: its sheet open and its composer in it, the loading sheet gone.
const CHAT_UP: &str = "window.reviewPanelReady === true && document.querySelector('#quick-root #quick-sheet')?.hidden === false && document.querySelector('#quick-root #quick-sheet #quick-text') !== null && document.getElementById('panel-loading') === null";

/// Open the chat from the header's `+` and wait until it is up.
#[track_caller]
fn open_chat(chrome: &mut Chrome) {
    chrome.eval("document.querySelector('#quick-open').click()");
    assert!(
        chrome.wait_for(CHAT_UP, SHOWN),
        "the chat never comes up: {:?}",
        chrome.eval("document.body.innerHTML.slice(-2000)")
    );
}

#[test]
fn the_chat_opened_on_one_page_and_then_on_another_fetches_its_files_once() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    // Each file is fetched by what shows it: no idle prefetch has the chat's in already.
    chrome.save_data();
    let (chat, menus) = (module(dist(), "panel"), module(dist(), "overlay"));
    // The chat's script, the chunks it imports (those the board shares with it the board's load
    // fetched) and its styles (the shared sheet the board's).
    let files: Vec<&String> = std::iter::once(&chat.script)
        .chain(&chat.imports)
        .chain(&chat.styles)
        .collect();
    let once = vec![1; files.len()];
    chrome.open(&site.url("/tasks"));
    shows(&mut chrome, BOARD);
    assert_eq!(
        asks(&site, &[&chat.script, &menus.script]),
        [0, 0],
        "the board is a module of its own, and the chat's script comes with the chat"
    );
    open_chat(&mut chrome);
    assert_eq!(
        asks(&site, &files),
        once,
        "the chat: its script, its chunks and its styles, each once: {files:?}"
    );
    // Closed, as a reader closes it, from within.
    assert!(chrome.wait_for(
        "document.querySelector('#quick-sheet')?.contains(document.activeElement) === true",
        SHOWN
    ));
    super::components::key(&mut chrome, "Escape", "Escape", 27);
    assert!(
        chrome.wait_for(
            "document.querySelector('#quick-root #quick-sheet')?.hidden !== false",
            SHOWN
        ),
        "Escape leaves the chat open"
    );
    // On a task's page the chat opens again from what the board's had in.
    follow(&mut chrome, "/task/visual/in-progress");
    shows(&mut chrome, "article.task");
    open_chat(&mut chrome);
    assert_eq!(
        asks(&site, &files),
        once,
        "the chat on a task's page: nothing fetched again"
    );
    // Away to pages of other modules (Reports and the agents page) and back, the chat open.
    follow(&mut chrome, "/reports");
    shows(&mut chrome, "[data-ready=reports]");
    follow(&mut chrome, "/agents");
    shows(&mut chrome, ".agents");
    follow(&mut chrome, "/task/visual/in-progress");
    shows(&mut chrome, "article.task");
    assert!(
        drawn(&mut chrome, "#quick-root #quick-sheet"),
        "the chat closed"
    );
    assert_eq!(
        asks(&site, &files),
        once,
        "back to the pages it has: nothing fetched again"
    );
    assert_eq!(
        asks(&site, &[&menus.script]),
        [0],
        "nothing the chat did needed the menus"
    );
}

/// A picture of 200 by 200 pixels.
const PICTURE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200"/>"#;

/// The stand-in with a task whose brief is a picture, `/near.svg`, below the page's head.
fn picture_site() -> Site {
    let site = site();
    let mut page = super::look_parity::responses()["/api/task-page"].clone();
    page["blocks"] = json!([{"type":"image","image":{"src":"/near.svg","alt":"near","presentation":{"attributes":{"width":"200","height":"200"}}}}]);
    site.set_json("/api/task-page", &page);
    site.set_file("/near.svg", PICTURE.to_vec(), "image/svg+xml");
    site
}

/// A task's page takes the brief's picture up with its own media layer, which opens it in the
/// menus' picture viewer: nothing of the chat's is fetched for it.
#[test]
fn a_web_page_takes_its_pictures_up_itself_and_opens_them_in_the_menus_viewer() {
    let site = picture_site();
    let Some(mut chrome) = chrome() else { return };
    // No idle prefetch fetches the chat's files: only what the page shows asks for its files.
    chrome.save_data();
    let chat = module(dist(), "panel");
    chrome.open(&site.url("/task/visual/in-progress"));
    // The brief is a part of its own, drawn when its read answers, after the article shows.
    shows(&mut chrome, "main img[alt=near]");
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
    assert_eq!(asks(&site, &[&chat.script]), [0], "{:?}", site.requests());
}

/// The chat's thread, ask 7, opened by the page's draft, its answer a picture, `/chat.svg`.
fn chat_picture_site(chrome: &mut Chrome) -> Site {
    let site = site();
    let mut thread = super::panel_open::chat(&["First question", "A picture"]);
    thread["ask"]["messages"][1]["blocks"] = json!([{"type":"image","image":{"src":"/chat.svg","alt":"chat picture","presentation":{"attributes":{"width":"200","height":"200"}}}}]);
    site.set_json("/api/ask", &thread);
    site.set_file("/chat.svg", PICTURE.to_vec(), "image/svg+xml");
    chrome.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": "localStorage.setItem('review-quick-draft', JSON.stringify({open: false, task: false, ask: 7}));"}),
    );
    site
}

/// The chat's picture.
const CHAT_PICTURE: &str = "#quick-root #quick-sheet .quick-message img[alt='chat picture']";

#[test]
fn a_chat_picture_opens_in_the_viewer_on_every_page() {
    let Some(mut chrome) = chrome() else { return };
    let site = chat_picture_site(&mut chrome);
    chrome.open(&site.url("/"));
    super::wait_for_board(&mut chrome);
    open_chat(&mut chrome);
    // Pages with a media layer of their own (home, a task's page) and without one (the board, the
    // agents page, Reports, the queue), the chat open over each.
    for (path, ready) in [
        ("/", ".hero-title"),
        ("/tasks", BOARD),
        ("/reports", "[data-ready=reports]"),
        ("/task/visual/in-progress", "article.task"),
        ("/agents", ".agents"),
        ("/queue", "[data-ready=queue]"),
    ] {
        if path != "/" {
            follow(&mut chrome, path);
            shows(&mut chrome, ready);
        }
        // Scrolled to in the thread, as a reader would, once the conversation's read has drawn
        // it there: the picture loads as it comes into view.
        shows(&mut chrome, CHAT_PICTURE);
        chrome.eval(&format!(
            "document.querySelector({}).scrollIntoView({{block:'center'}})",
            json!(CHAT_PICTURE)
        ));
        assert!(
            chrome.wait_for(
                &format!(
                    "document.querySelector({})?.naturalWidth === 200",
                    json!(format!("{CHAT_PICTURE}[data-zoom-ready]"))
                ),
                SHOWN
            ),
            "{path}: the chat's picture is never taken up: {:?}",
            chrome.eval(&format!(
                "document.querySelector({})?.outerHTML ?? document.getElementById('quick-sheet')?.innerText ?? null",
                json!(CHAT_PICTURE)
            ))
        );
        chrome.eval(&format!(
            "document.querySelector({}).click()",
            json!(CHAT_PICTURE)
        ));
        assert!(
            chrome.wait_for(
                "document.querySelector('#media-lightbox[data-state=open] img')?.getAttribute('src') === '/chat.svg'",
                SHOWN
            ),
            "{path}: the chat's picture never opens in the viewer: {:?}",
            chrome.eval("document.getElementById('media-lightbox')?.outerHTML ?? null")
        );
        chrome.eval("document.querySelector('#media-lightbox .lightbox-controls button').click()");
        assert!(
            chrome.wait_for("document.getElementById('media-lightbox') === null", SHOWN),
            "{path}: the viewer never closes"
        );
        assert!(
            drawn(&mut chrome, "#quick-root #quick-sheet"),
            "{path}: closing the viewer closed the chat"
        );
    }
}

#[test]
fn a_chat_whose_script_is_held_keeps_the_page_shown_and_its_sheet_loading_until_it_is_in() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    // The first open fetches the chat; no idle prefetch has it in the cache already.
    chrome.save_data();
    chrome.open(&site.url("/tasks"));
    shows(&mut chrome, BOARD);
    let script = module(dist(), "panel").script;
    site.hold_route(&script, true);
    chrome.eval("document.querySelector('#quick-open').click()");
    // Asked for the chat's script, which is held, the chat waits on it alone: the board stays shown
    // and usable, its header with it, and the chat's sheet says it loads.
    super::panel_open::asked(&mut chrome, &site, &script, 1);
    assert_eq!(
        chrome.eval("document.documentElement.dataset.navigation"),
        json!("ready"),
        "the page waits on the chat"
    );
    assert!(drawn(&mut chrome, BOARD), "the board is hidden");
    assert!(drawn(&mut chrome, ".app-header"), "the header is gone");
    assert!(
        drawn(&mut chrome, "#panel-loading #quick-sheet"),
        "the chat's sheet does not say it loads"
    );
    assert_eq!(
        chrome.eval("window.reviewPanelReady === true"),
        json!(false)
    );
    site.hold_route(&script, false);
    assert!(
        chrome.wait_for(CHAT_UP, SHOWN),
        "the chat never comes up once its script is in: {:?}",
        chrome.eval("document.body.innerHTML.slice(-2000)")
    );
    assert!(
        drawn(&mut chrome, BOARD),
        "the board is hidden once the chat is up"
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

/// The chat's notice when it does not load.
const CHAT_FAILED: &str = "document.querySelector('#panel-error[role=alert]') !== null && document.getElementById('panel-loading') === null";

#[test]
fn a_chat_whose_script_does_not_load_says_so_with_its_retry_and_leaves_the_page_shown() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    // The first open fetches the chat; no idle prefetch has it in the cache already.
    chrome.save_data();
    chrome.open(&site.url("/"));
    super::wait_for_board(&mut chrome);
    chrome.eval("window.__kept = true");
    let script = module(dist(), "panel").script;
    // Its build still served, the chat's script does not load: the page stays, and says so.
    site.retire(&script);
    chrome.eval("document.querySelector('#quick-open').click()");
    assert!(
        chrome.wait_for(CHAT_FAILED, SHOWN),
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
    // What it leaves on screen: the page and its header, under the notice.
    assert_eq!(chrome.eval("window.__kept === true"), json!(true));
    assert!(drawn(&mut chrome, ".hero-title"), "the page is hidden");
    assert!(drawn(&mut chrome, ".app-header"), "the header is gone");
    // A later load works: the retry asks for the script again, and the chat comes up.
    site.restore(&script);
    chrome.eval("document.querySelector('#panel-error button').click()");
    assert!(
        chrome.wait_for(
            "window.reviewPanelReady === true && document.getElementById('panel-error') === null",
            SHOWN
        ),
        "the retry never loads the chat: {:?}",
        site.requests()
    );
    assert_eq!(asks(&site, &[&script]), [2], "{:?}", site.requests());
    assert_eq!(chrome.eval("window.__kept === true"), json!(true));
}

#[test]
fn a_chat_whose_build_is_gone_loads_the_page_again() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.save_data();
    chrome.open(&site.url("/"));
    super::wait_for_board(&mut chrome);
    assert_eq!(loads(&site, "/"), 1, "{:?}", site.requests());
    // Its build gone from the server, the chat's script with it: the chat opened, the page loads
    // again, onto the build served now, and says nothing of a chat that could not load.
    site.retire(&module(dist(), "panel").script);
    site.retire(&shell(&mut chrome));
    chrome.eval("document.querySelector('#quick-open').click()");
    if let Err(stall) =
        test_time::until_true("a page whose chat is gone loaded again", STALL, || {
            loads(&site, "/") >= 2
        })
    {
        panic!("{stall}: {:?}", site.requests());
    }
}
