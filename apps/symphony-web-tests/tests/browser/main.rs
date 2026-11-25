//! The board view in a real browser: the client's release build (apps/symphony-web/build.sh),
//! served by a stand-in for the Symphony server (`site`), opened in headless Chrome (`chrome`).
//!
//! The budgets are the owner's (tools/python-zero/ui-board), and a miss is reported, never
//! loosened. They are absolute; the measurement they were set against is in
//! docs/engineering/analysis/python-zero.md ("Today's app, measured for mobile").
//! - a first visit loads at most 115 KB gzipped to `/`, 150 KB to `/tasks` and 145 KB to a task's
//!   page (each lowered from the owner's 300 KB as the pages and the chat moved to React),
//!   everything the page needs (shell, the React client's scripts, CSS, API data, the feed's head
//!   and messages), each independently.
//!   The feed's keep-alive pings, which keep arriving while the page is open, are reported
//!   beside the total. Deferred page kinds and the whole build are reported separately; a
//!   visit pays for the resources it actually loads;
//! - a return visit loads at most 20 KB (the build is cached by its hashed names);
//! - one feed event changes one card in place, and the page fetches nothing for it;
//! - a page the browser keeps for back and forward holds no feed stream, and a kept page shown
//!   again reads what changed (`kept_pages`).
//!
//! Without Chrome on the machine the browser tests say so and pass: the one capability a test
//! may lack. The build is made once per run of this binary, into `build/symphony-ui/` of the tree.

mod agents;
mod assets;
mod binaries;
mod board_identity;
mod board_place;
mod board_search;
mod board_status;
mod cache_ahead;
mod cache_documents;
mod cache_pages;
mod cache_store;
mod cache_tasks;
mod chrome;
#[path = "../common/mod.rs"]
mod common;
mod components;
mod desktop_stub;
mod document_cards;
mod document_gallery;
mod document_landing;
mod document_moved;
mod document_references;
mod feed_deltas;
#[path = "../../../../tools/test-support/script_std.rs"]
mod fixture_script;
mod floating_controls;
mod group_decides;
mod group_queues;
mod header;
mod held_on_documents;
mod held_words;
mod html_documents;
mod kept_pages;
mod landmarks;
mod layout_shift;
mod look_parity;
mod notice_layout;
mod owner_answer;
mod owner_controls;
mod owner_note;
mod owner_save;
mod panel_open;
mod people;
mod persistent;
mod phone;
mod phone_board;
mod phone_labels;
mod polish;
mod preload;
mod projects;
mod reports;
mod reports_landed;
mod restored;
mod run_records;
mod runs;
mod server_notices;
mod settings;
mod shot;
mod site;
mod skeleton;
mod tabs;
mod task_attempts;
mod task_diff;
mod task_gates;
mod task_posts;
mod timeline;
mod token_costs;
mod touch;
mod waiting_board;
mod workflow_board;

use std::time::Duration;

use assets::dist;
use chrome::Chrome;
use serde_json::json;
use site::Site;

/// Home's first visit: lowered from the owner's 300,000 to 125,000 when home moved to the web
/// client, and to 115,000 once the chat moved and no kind was left to move (112,551 bytes on the
/// fixture then); never raised.
const HOME_FIRST_VISIT_BYTES: f64 = 115_000.0;
/// The task board's first-visit cap (`/tasks`): lowered from the owner's 300,000 to 150,000 when
/// the board moved to the React client, and to 125,000 once the chat moved and no kind was left
/// to move (121,492 bytes on the fixture then); raised to 150,000 by the owner on 2026-11-19
/// ("raise budget to 150"), with 44 bytes left and a custom workflow's board to draw.
const TASKS_FIRST_VISIT_BYTES: f64 = 150_000.0;
/// A task's page's first-visit cap (`/task/SLUG`), its timeline loaded after it and reported
/// beside it: lowered from the owner's 300,000 to 160,000 when the page moved to the React client
/// (546,351 bytes on main's Dioxus build at f56bde5), and to 145,000 once the chat moved and no
/// kind was left to move (142,175 bytes on the fixture then); never raised.
const TASK_FIRST_VISIT_BYTES: f64 = 145_000.0;
const RETURN_VISIT_BYTES: f64 = 20_000.0;
/// How long a page may take to show the board at all before a test gives up on it.
const SHOWN: Duration = Duration::from_secs(30);
/// The wall-clock backstop of a test's own wait on a page or a stand-in whose allowance is
/// [`SHOWN`]: six times it, which only a stall reaches (RULES.md `tests/no-test-fails-on-load`).
/// The driver's own waits keep the same rule (`crates/chrome-devtools`).
const STALL: Duration = test_time::backstop(SHOWN);

/// What a server said on `said` once it is up (its address), looked for until it says it: a
/// server whose output ends first (it exited) never will, and one that says nothing by the
/// backstop ([`STALL`]) stalled, which the error says with the load average. `what` names it.
fn announced(what: &str, said: &std::sync::mpsc::Receiver<String>) -> Result<String, String> {
    use std::sync::mpsc::TryRecvError;
    test_time::until(format!("{what} saying it is up"), STALL, || {
        match said.try_recv() {
            Ok(address) => Some(Ok(address)),
            Err(TryRecvError::Disconnected) => Some(Err(format!(
                "{what}'s output ended before it said it was up"
            ))),
            Err(TryRecvError::Empty) => None,
        }
    })
    .unwrap_or_else(|stall| Err(stall.to_string()))
}
/// A page script that leaves the page no IndexedDB, as a browser without one: the client cache's
/// store misses every read and keeps nothing (`shell/cache.ts`), so a return visit to the board
/// or the queue draws from the server under its ghost, as one the device has no copy for does.
const NO_STORE: &str = "Object.defineProperty(window, 'indexedDB', { value: undefined });";

/// The width the window leaves a page beside its scrollbar, as a script's expression: the window's
/// (`innerWidth`) less a scrollbar's thickness, probed on a box of its own that scrolls. The page
/// keeps that room whether or not it scrolls (`scrollbar-gutter: stable` on its root,
/// styles/board.css), and so do its fixed controls, so what fills the window is this wide. Chrome
/// draws a classic scrollbar, 15 px of every width, or an overlay one, none, as the machine's
/// settings choose (macOS's automatic scroll bars are classic while a mouse is attached; a phone's
/// emulation draws overlay ones), so it is never `innerWidth` alone; nor
/// `document.documentElement.clientWidth`, which leaves the gutter out only while the page
/// scrolls. It is read from the window, not from the root's own box, so a rule that narrows the
/// root (a `max-width` or a margin on `html`) narrows what fills the window and not this.
const PAGE_WIDTH: &str = "(() => { const probe = document.createElement('div'); \
    probe.style.cssText = 'position:absolute;top:0;left:0;width:100px;height:100px;overflow:scroll;visibility:hidden'; \
    document.documentElement.append(probe); const bar = probe.offsetWidth - probe.clientWidth; \
    probe.remove(); return innerWidth - bar; })()";

/// `script`, a page function of the window's width, called with [`PAGE_WIDTH`], as a script's
/// expression.
fn at_page_width(script: &str) -> String {
    format!("({script})({PAGE_WIDTH})")
}

/// Hold, from the page's first script on, every timer of the asks' retry the page sets
/// ([`symphony_web_tests::RETRY_MS`]: a failed read of a document's questions or of the chat's
/// thread asked again), none of them ever run, so a case says whether a read that answered left
/// one waiting, with no wait of its own: `window.askPoll` maps each held timer the page has not
/// cleared to its callback. One the page clears is let go, as it waits for nothing any more: a
/// deadline of that length its event ended, such as a page's `Settle` bound (`SETTLE_MS`, 3 s as
/// well) once its first screen is in, is no poll. The page's other timers run as they are. Call it
/// before `open`.
fn hold_ask_polls(browser: &mut Chrome) {
    hold_timers(browser, "askPoll", symphony_web_tests::RETRY_MS);
}

/// Hold, from the page's first script on, every timer of `ms` the page sets, none of them run by
/// the browser: `window[name]` maps each held timer the page has not cleared to its callback, for
/// the test to run when its case has come, the page's clock in the test's hands. One the page
/// clears is let go. The page's timers of any other length run as they are. Call it before `open`.
fn hold_timers(browser: &mut Chrome, name: &str, ms: i32) {
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": format!(
            r#"window[{name}] = new Map();
            (() => {{
                const kept = window[{name}], set = window.setTimeout, clear = window.clearTimeout;
                let held = 0;
                window.setTimeout = function (fn, ms, ...args) {{
                    if (ms !== {ms}) return set(fn, ms, ...args);
                    held -= 1;
                    kept.set(held, () => fn(...args));
                    return held;
                }};
                window.clearTimeout = function (id) {{ if (!kept.delete(id)) clear(id) }};
            }})();"#,
            name = json!(name),
        )}),
    );
}

/// [`hold_ask_polls`] keeps a retry's timer the page leaves set, with what it would run, lets go
/// one the page clears, and leaves a timer of another length to the browser, its id the browser's.
#[test]
fn the_ask_poll_hold_keeps_a_retry_left_waiting_and_lets_a_cleared_one_go() {
    let Some(mut browser) = chrome() else { return };
    hold_ask_polls(&mut browser);
    browser.open("data:text/html,held");
    let retry = symphony_web_tests::RETRY_MS;
    assert_eq!(
        browser.eval(&format!(
            "(() => {{
                const left = setTimeout(word => {{ window.ran = word }}, {retry}, 'held');
                clearTimeout(setTimeout(() => {{}}, {retry}));
                const other = setTimeout(() => {{}}, {retry} + 1);
                clearTimeout(other);
                askPoll.get(left)?.();
                return [askPoll.size, askPoll.has(left), left < 0, other > 0, window.ran ?? null];
            }})()"
        )),
        json!([1, true, true, true, "held"])
    );
}

/// A page script that keeps every idle callback the page asks for, none run by the browser, until
/// the test runs them ([`RUN_IDLE`]): `window.idle` holds them. The shell asks the idle browser
/// for its warm-up and its read-ahead in the task that marks the page ready
/// (`data-navigation=ready`, shell.ts), and for nothing where the browser saves data or its link
/// is too slow: so a page that is ready has asked for every one it will, and one that asked for
/// none warms and reads nothing, with no window to wait through to see it.
const HOLD_IDLE: &str =
    "window.idle = []; window.requestIdleCallback = callback => idle.push(callback);";

/// Runs every idle callback [`HOLD_IDLE`] kept, and says how many there were.
const RUN_IDLE: &str = "idle.splice(0).map(callback => callback()).length";

/// True once the shell has marked the page ready, its idle work asked for in that task.
const READY: &str = "document.documentElement.dataset.navigation === 'ready'";

/// A page script under which, once the test sets `window.__lateTasks` to a count of frames, every
/// task of React's scheduler (a `MessageChannel` message, one posted before as well) runs that many
/// frames after it comes, as a slow phone's or a loaded machine's renderer runs its frames before
/// a task waiting for it. `window.__channels` counts the channels whose messages it takes, React's
/// scheduler's the client's one. A page that draws a feed event within the event (`flushSync`, as
/// the board, the agents page and a task's page do) starts the shell's hold of the reader's place
/// from the change in the page, however late these run.
const LATE_TASKS: &str = r#"window.__lateTasks = 0;
window.__channels = 0;
(() => {
    const Channel = window.MessageChannel;
    const receive = Object.getOwnPropertyDescriptor(MessagePort.prototype, "onmessage");
    window.MessageChannel = class extends Channel {
        constructor() {
            super();
            const port = this.port1;
            let handler = null;
            Object.defineProperty(port, "onmessage", {
                get: () => handler,
                set: given => {
                    handler = given;
                    if (typeof given !== "function") return receive.set.call(port, given);
                    window.__channels += 1;
                    receive.set.call(port, event => {
                        let left = window.__lateTasks;
                        (function wait() { if (left-- > 0) requestAnimationFrame(wait); else given.call(port, event); })();
                    });
                },
            });
        }
    };
})();"#;

/// Whether React's scheduler takes its tasks through [`LATE_TASKS`]'s channel, so that a
/// `window.__lateTasks` the test sets makes them late: a page that took none would run none late.
const LATE_TASKS_TAKEN: &str = "window.__channels > 0";

fn site() -> Site {
    Site::start(dist(), &common::fixture("board.json"))
}

/// Chrome, or `None` when this machine has none (said once per test).
fn chrome() -> Option<Chrome> {
    match chrome::find() {
        Some(binary) => Some(Chrome::launch(&binary)),
        None => {
            eprintln!(
                "no Chrome on this machine (SYMPHONY_CHROME names one): browser test not run"
            );
            None
        }
    }
}

/// Wait for the board to be drawn; panics naming what the page shows instead.
fn wait_for_board(chrome: &mut Chrome) {
    // The page mounts its main element while the board request is still pending.
    // Wait for the rendered content before reading counts or measuring its requests.
    if !chrome.wait_for(
        "document.querySelector('[data-ready=board] .hero-title') !== null",
        SHOWN,
    ) {
        let body =
            chrome.eval("document.body ? document.body.innerHTML.slice(0, 2000) : '(no body)'");
        panic!("the board was not drawn within {SHOWN:?}; the page shows: {body}");
    }
}

/// A script true once `selector` names an element the reader can see: drawn, and out from under
/// its page's skeleton (`viewport::Settle` keeps a task or a document hidden until its first
/// screen is in, and a hidden control takes no focus).
fn visible(selector: &str) -> String {
    format!(
        "document.querySelector({})?.checkVisibility({{visibilityProperty: true}}) === true",
        json!(selector)
    )
}

/// Whether the focus is on the element `selector` names, once two frames and a task have passed
/// (Chrome takes the focus from a button disabled under it at its next rendering update, not at
/// once); else what holds it, for the failure.
fn focus_on(chrome: &mut Chrome, selector: &str) -> serde_json::Value {
    chrome.eval(&format!(
        "new Promise(done => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(done)))).then(() => \
         document.activeElement === document.querySelector({}) || (document.activeElement?.outerHTML ?? 'nothing').slice(0, 120))",
        json!(selector)
    ))
}

/// A script true once the element `selector` names is drawn as the shared sheet draws a disabled
/// button (`.dx-button:disabled`: half its opacity, which the button's transition reaches in its
/// own time, and no pointer events).
fn drawn_disabled(selector: &str) -> String {
    format!(
        "(style => style.opacity === '0.5' && style.pointerEvents === 'none')(getComputedStyle(document.querySelector({})))",
        json!(selector)
    )
}

/// A finger put down at (`x`, `from`), drawn `by` px up the screen in ten steps and lifted: the
/// page scrolls on by the swipe (510 px for 500, every time), and by no more. Each touch carries its own time, a frame apart and
/// the lift half a second after the last move, so the finger rests before it leaves: Chrome reads
/// a swipe's speed from the times of its touches, and with the times at which a loaded machine
/// let it take them the lift flung the page on by anything from nothing to twice the swipe.
fn swipe(chrome: &mut Chrome, x: u32, from: u32, by: u32) {
    let start = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_secs_f64();
    let mut touch = |kind: &str, at: f64, points: serde_json::Value| {
        chrome.call(
            "Input.dispatchTouchEvent",
            json!({"type": kind, "touchPoints": points, "timestamp": start + at}),
        );
    };
    let point = |y: u32| json!([{"x": x, "y": y}]);
    touch("touchStart", 0.0, point(from));
    for step in 1..=10 {
        touch(
            "touchMove",
            0.016 * f64::from(step),
            point(from - by * step / 10),
        );
    }
    touch("touchEnd", 0.66, json!([]));
}

/// The wire's samples (`wire.cases.json`'s `samples`).
fn samples() -> serde_json::Value {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("../../../symphony-web/src/wire.cases.json"))
            .expect("the wire's cases");
    cases["samples"].clone()
}

/// A page of `width` (a phone's at 760 px or under, 844 px high; else 900), over the stand-in's
/// `path`.
fn open(chrome: &mut Chrome, site: &Site, path: &str, width: u32) {
    let phone = width <= 760;
    chrome.viewport(width, if phone { 844 } else { 900 }, phone);
    chrome.open(&site.url(path));
}

/// The text of every element `selector` matches, in document order.
fn texts(chrome: &mut Chrome, selector: &str) -> Vec<String> {
    let value = chrome.eval(&format!(
        "Array.from(document.querySelectorAll({})).map(e => e.textContent.trim())",
        json!(selector)
    ));
    serde_json::from_value(value).unwrap_or_default()
}

#[test]
fn entry_pages_fit_the_first_visit_budget_and_deferred_kinds_are_reported() {
    let site = site();
    site.set_json("/api/task-board", &site::example_listing("ready"));
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
    site.set_file(
        symphony_api::TASK_TIMELINE_PATH,
        common::fixture("task-timeline.json").into_bytes(),
        "application/json",
    );
    let sizes = site.build_sizes();
    assert!(sizes.iter().any(|(path, _)| path == "/index.html"));
    assert!(sizes.iter().all(|(path, _)| !path.starts_with("/api/")));
    let board = gzip_len(common::fixture("board.json").as_bytes());
    let total = sizes.iter().map(|(_, size)| *size).sum::<usize>() + board;
    let listing: Vec<String> = sizes
        .iter()
        .map(|(path, size)| format!("{size:>8} {path}"))
        .collect();
    eprintln!(
        "the published build, gzipped (including retained asset hashes):\n{}\n{board:>8} the board's JSON\n{total:>8} in all",
        listing.join("\n")
    );
    let mut entries = Vec::new();
    let Some(mut chrome) = chrome() else { return };
    // Each kind gets a fresh browser context, including when kinds share CSS. A cached
    // sibling must not make an oversized entry pass. Keep that context for the return visit.
    for (path, kind, ready, cap) in [
        ("/", "review", ".hero-title", Some(HOME_FIRST_VISIT_BYTES)),
        (
            "/tasks",
            "board",
            "[data-slug='tools/example']",
            Some(TASKS_FIRST_VISIT_BYTES),
        ),
        ("/agents", "agents", ".agents", None),
        ("/docs/", "docs", "[data-ready=folder]", None),
        ("/queue", "queue", "[data-ready=queue]", None),
        ("/reports", "reports", "[data-ready=reports]", None),
        ("/runs", "runs", "[data-ready=runs]", None),
        ("/settings", "settings", "[data-ready=settings]", None),
        ("/people", "people", "[data-ready=people]", None),
        (
            "/task/visual/in-progress",
            "task",
            "article.task",
            Some(TASK_FIRST_VISIT_BYTES),
        ),
        ("/docs/look-parity.md", "docs", ".doc-card", None),
    ] {
        preload::serve_page(&site, path, kind);
        chrome.fresh_context();
        if kind == "board" {
            // The fixture is Ready; the phone now mounts only its selected Review stage.
            // Measure the same loaded lifecycle board as the established byte baseline.
            chrome.viewport(1440, 900, false);
        }
        chrome.open(&site.url(path));
        assert!(
            chrome.wait_for(
                &format!("document.querySelector({}) !== null", json!(ready)),
                SHOWN
            ),
            "{path} did not show {ready}: {:?}",
            chrome.eval("document.body.innerText")
        );
        // A task page's timeline is a chunk of its own (apps/symphony-web/src/timeline.tsx), loaded
        // into its box once the page has drawn and is idle: reported beside the first visit, which
        // never waits for it.
        if path.starts_with("/task/") {
            assert!(
                chrome.wait_for(
                    "document.getElementById('timeline-root') !== null && document.querySelector('.tl-drawn') !== null",
                    SHOWN
                ),
                "{path} drew no timeline"
            );
        }
        let (timeline, requests): (Vec<_>, Vec<_>) = completed_requests(&mut chrome)
            .into_iter()
            .partition(timeline_file);
        if path.starts_with("/task/") {
            eprintln!(
                "{path}'s timeline, loaded after its first visit: {} bytes\n{timeline:#?}",
                needed_bytes(&timeline)
            );
        } else {
            assert!(
                timeline.is_empty(),
                "{path} loaded the task timeline: {timeline:#?}"
            );
        }
        assert!(
            requests.iter().any(|r| r.kind == "Script"
                && r.url.contains(&format!("/assets/{kind}-"))
                && r.status == 200
                && !r.from_cache),
            "{path} did not fetch its {kind} module: {requests:#?}"
        );
        // A kind is its script and the chunks it imports: no page fetches wasm.
        assert!(
            requests.iter().all(|r| !r.url.ends_with(".wasm")),
            "{path} fetched wasm: {requests:#?}"
        );
        let bytes = needed_bytes(&requests);
        eprintln!(
            "{path} first visit: {bytes} bytes; the feed's keep-alive pings beside it: {} bytes; \
             warmed at idle so far: {} bytes\n{requests:#?}",
            ping_bytes(&requests),
            prefetched_bytes(&requests)
        );
        chrome.open("about:blank");
        chrome.open(&site.url(path));
        assert!(chrome.wait_for(
            &format!("document.querySelector({}) !== null", json!(ready)),
            SHOWN
        ));
        let returned: Vec<_> = completed_requests(&mut chrome)
            .into_iter()
            .filter(|request| !timeline_file(request))
            .collect();
        let again = needed_bytes(&returned);
        eprintln!(
            "{path} return visit: {again} bytes; keep-alive pings beside it: {} bytes",
            ping_bytes(&returned)
        );
        if let Some(cap) = cap {
            entries.push((path, cap, bytes, again));
        }
    }
    chrome.fresh_context();
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    let (_, idle) = panel_open::settled(&mut chrome);
    let before = needed_bytes(&idle);
    chrome.eval("document.querySelector('#quick-open').click()");
    assert!(
        chrome.wait_for("window.reviewPanelReady === true", SHOWN),
        "the deferred panel never loaded"
    );
    let with_panel = needed_bytes(&completed_requests(&mut chrome));
    eprintln!(
        "warmed at idle, outside the first visit: {} bytes (the chat's and the menus' modules); \
         the chat's first open then: {} additional bytes; home with the chat: {with_panel} bytes",
        prefetched_bytes(&idle),
        with_panel - before
    );
    for (path, cap, first, again) in entries {
        assert!(
            first <= cap,
            "{path} first visit loads {first} bytes, over {cap}"
        );
        assert!(
            again <= RETURN_VISIT_BYTES,
            "{path} return visit loads {again} bytes, over {RETURN_VISIT_BYTES}"
        );
    }
}

/// Whether `request` is the task timeline's: its chunk, which its box imports once the page is
/// idle and which carries its style, or its read. The chunk is no kind, so the shell's manifest
/// never names it: it is the published `timeline-HASH.js` beside the kinds' scripts.
fn timeline_file(request: &chrome::Request) -> bool {
    let path = request
        .url
        .split_once("://")
        .map_or(request.url.as_str(), |(_, rest)| {
            rest.find('/').map_or("", |at| &rest[at..])
        });
    path.starts_with(symphony_api::TASK_TIMELINE_PATH)
        || path
            .strip_prefix("/assets/timeline-")
            .is_some_and(|rest| rest.ends_with(".js") && !rest.contains('/'))
}

/// Include styles and API replies still in flight when the view first appeared. A live feed
/// intentionally never completes; its head and messages still count in `needed_bytes`, all but
/// its keep-alive pings.
fn completed_requests(chrome: &mut Chrome) -> Vec<chrome::Request> {
    test_time::until("the page's resources finished", STALL, || {
        let requests = chrome.requests();
        requests
            .iter()
            .all(|r| r.finished || r.streams() || r.kind == "Image")
            .then_some(requests)
    })
    .unwrap_or_else(|stall| panic!("{stall}: {:#?}", chrome.requests()))
}

/// What the board needed over the wire: every request but pictures, which are the cards'
/// content, not the app; idle prefetches, which are a later click's, reported apart
/// ([`prefetched_bytes`]); and the feed's keep-alive pings, which arrive every `ping_ms` for as
/// long as the page is open (the owner's, 2026-11-11). The feed's head and its content messages
/// count; the pings are reported beside the total ([`ping_bytes`]).
fn needed_bytes(requests: &[chrome::Request]) -> f64 {
    loaded(requests)
        .map(|request| request.bytes - pings_of(request))
        .sum()
}

/// The keep-alive pings' bytes, left out of [`needed_bytes`].
fn ping_bytes(requests: &[chrome::Request]) -> f64 {
    loaded(requests).map(pings_of).sum()
}

fn loaded(requests: &[chrome::Request]) -> impl Iterator<Item = &chrome::Request> {
    requests
        .iter()
        .filter(|request| request.kind != "Image" && !request.from_cache && !request.prefetch)
}

/// The bytes of `request`'s pings: each an event of an event stream, or a frame of a WebSocket.
fn pings_of(request: &chrome::Request) -> f64 {
    let ping = if request.kind == chrome::WEB_SOCKET {
        site::ping().frame().len()
    } else {
        site::ping().sse().len()
    };
    (request.pings * ping) as f64
}

/// What the page warmed at idle for a later click: never its first paint's.
fn prefetched_bytes(requests: &[chrome::Request]) -> f64 {
    requests
        .iter()
        .filter(|request| request.prefetch && !request.from_cache)
        .fold(0.0, |bytes, request| bytes + request.bytes)
}

#[test]
fn navigation_preserves_an_early_scroll_and_resets_an_untouched_next_page() {
    for when in ["before-shell", "during-navigation"] {
        assert_navigation_scroll(when);
    }
}

#[test]
fn superseded_navigation_preserves_the_new_page_scroll_until_ready() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.call("Page.addScriptToEvaluateOnNewDocument", json!({"source": r#"
        window.readyPages = [];
        window.navigationEvents = 0;
        const observeReady = () => {
            if (!document.documentElement) return;
            bootstrap.disconnect();
            new MutationObserver(records => {
                if (document.documentElement.dataset.navigation !== 'ready') return;
                const board = document.getElementById('review-root-board');
                records.forEach(() => {
                    readyPages.push({path: location.pathname, mounted: Boolean(board && !board.hidden && board.childElementCount)});
                });
            }).observe(document.documentElement, {attributes:true, attributeFilter:['data-navigation']});
        };
        const bootstrap = new MutationObserver(observeReady);
        bootstrap.observe(document, {childList:true});
        observeReady();
        addEventListener('review:navigate', () => {
            window.navigationEvents++;
            if (navigationEvents === 1) document.querySelector('header a[href="/tasks"]').click();
            else if (navigationEvents === 2) {
                document.body.style.minHeight = '30000px';
                scrollTo(0, 1200);
                window.earlyScroll = scrollY;
            }
        });
    "#}));
    chrome.open(&site.url("/"));
    assert!(chrome.wait_for(
        &format!("location.pathname === '/tasks' && {READY} && document.querySelector('.kanban') !== null"),
        SHOWN
    ));
    assert_eq!(chrome.eval("window.navigationEvents"), json!(2));
    assert_eq!(chrome.eval("window.earlyScroll"), json!(1200));
    assert_eq!(chrome.eval("scrollY"), json!(1200));
    assert_eq!(
        chrome.eval("window.readyPages"),
        json!([{"path":"/tasks","mounted":true}]),
        "a superseded callback announced ready before the new page mounted"
    );
}

fn assert_navigation_scroll(when: &str) {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    let setup = if when == "before-shell" {
        "const observer = new MutationObserver(() => { if (document.body) { scrollEarly(); observer.disconnect(); } }); observer.observe(document, {childList:true,subtree:true});"
    } else {
        "addEventListener('review:navigate', scrollEarly, {once:true});"
    };
    chrome.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": format!(r#"
            const scrollEarly = () => {{
                document.body.style.minHeight = '30000px';
                scrollTo(0, 1200);
                window.earlyScroll = scrollY;
            }};
            {setup}
        "#)}),
    );
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    assert!(chrome.wait_for(READY, SHOWN));
    assert_eq!(
        chrome.eval("window.earlyScroll"),
        json!(1200),
        "{when}: the page was never scrolled"
    );
    assert_eq!(
        chrome.eval("scrollY"),
        json!(1200),
        "{when}: initial navigation undid the scroll"
    );
    // Both mounts must exceed the viewport: otherwise the new page clamps to zero and
    // passes even when the shell incorrectly compares against the old page's position.
    chrome.eval(
        r#"
        const style = document.createElement('style');
        style.textContent = '#review-root-board { min-height: 3000px }';
        document.head.append(style);
        document.body.style.minHeight = '';
        document.getElementById('review-root-review').style.minHeight = '6000px';
        scrollTo(0,5000);
        addEventListener('review:navigate', () => { window.mountedScroll = scrollY; }, {once:true});
    "#,
    );
    assert_eq!(chrome.eval("scrollY"), json!(5000));
    chrome.eval("document.querySelector('header a[href=\"/tasks\"]').click()");
    assert!(chrome.wait_for(
            &format!("location.pathname === '/tasks' && {READY} && document.querySelector('.kanban') !== null"),
            SHOWN
        ));
    assert_eq!(
        chrome.eval("mountedScroll > 0 && mountedScroll < 5000"),
        json!(true),
        "{when}: the destination must clamp above zero before the shell resets it"
    );
    assert_eq!(
        chrome.eval("scrollY"),
        json!(0),
        "{when}: the next page kept the old position"
    );
}

#[test]
fn same_kind_navigation_resets_scroll_clamped_by_the_new_page() {
    let site = site();
    for slug in ["tools/example", "tools/second"] {
        site.set_json(
            &format!("/api/task-page?slug={}", slug.replace('/', "%2F")),
            &json!({"task":{"slug":slug,"title":slug,"body":"","status":"ready",
                "priority":1,"parent":null,"created_by":"owner","created":0.0,"updated":0.0,
                "links":[],"depends":[],"roles":[],"instruction":false},
                "blocks":[],"children":[],"children_count":0}),
        );
    }
    site.set_json(
        "/api/task-history",
        &json!({"entries":[],"before":null,"after":null}),
    );
    // A task page to another: `/agents` is a kind of its own (the web client's), whose move from a
    // task page resets the scroll as every move between kinds does (above).
    let destination = "/task/tools/second";
    let Some(mut chrome) = chrome() else { return };
    chrome.call("Profiler.enable", json!({}));
    chrome.call(
        "Profiler.startPreciseCoverage",
        json!({"callCount":true,"detailed":true}),
    );
    chrome.open(&site.url("/task/tools/example"));
    assert!(chrome.wait_for(
        &format!(
            "{READY} && document.querySelector('.task[data-slug=\"tools/example\"]') !== null"
        ),
        SHOWN
    ));
    chrome.eval(
        r#"
        const mount = document.getElementById('review-root-task');
        mount.style.minHeight = '6000px';
        scrollTo(0,5000);
        addEventListener('review:navigate', () => {
            window.beforeClamp = scrollY;
            mount.style.minHeight = '3000px';
            window.afterClamp = scrollY;
        }, {once:true});
    "#,
    );
    assert_eq!(chrome.eval("scrollY"), json!(5000));
    chrome.eval(&format!(
        "const next = document.createElement('a'); next.href = {}; document.body.append(next); next.click(); next.remove()",
        json!(destination)
    ));
    assert!(chrome.wait_for(READY, SHOWN));
    assert_eq!(chrome.eval("window.beforeClamp"), json!(5000));
    assert_eq!(
        chrome.eval("afterClamp > 0 && afterClamp < beforeClamp"),
        json!(true),
        "{destination}: navigation must shrink the page after the shell's baseline"
    );
    assert_eq!(chrome.eval("scrollY"), json!(0), "{destination}");
    // Keep the fixture honest: the second route must render its own page, rather than
    // passing the scroll check with a missing API response or the first task still shown.
    let selector = ".task[data-slug=\"tools/second\"]";
    assert!(chrome.wait_for(
        &format!("document.querySelector({}) !== null", json!(selector)),
        SHOWN
    ));
    let coverage = chrome.call("Profiler.takePreciseCoverage", json!({}));
    let shell = coverage["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|script| {
            script["url"]
                .as_str()
                .is_some_and(|url| url.contains("/symphony-shell-"))
        })
        .expect("navigation shell coverage");
    let ranges: Vec<_> = shell["functions"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|function| function["ranges"].as_array().unwrap())
        .collect();
    let used = ranges
        .iter()
        .filter(|range| range["count"].as_u64().unwrap_or(0) > 0)
        .count();
    assert!(
        used > 0,
        "the shell coverage instrument returned no executed blocks"
    );
    eprintln!(
        "NAVIGATION COVERAGE {destination}: {used}/{} executed shell blocks",
        ranges.len()
    );
}

#[test]
fn document_and_queue_loads_preserve_a_scroll_made_on_the_mounted_page() {
    let site = site();
    restored::document(
        &site,
        vec![json!({"type":"paragraph","spans":[{"type":"text","text":"Loaded document"}]})],
    );
    for (path, ready) in [("/docs/PROBE.md", "document"), ("/queue", "queue")] {
        let Some(mut chrome) = chrome() else { return };
        chrome.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source": r#"
            // Hold the data until the new module is mounted, so its load effect always
            // follows the early scroll. No delay or rendering-frame assumption is needed.
            let mounted = false;
            const mount = new Promise(resolve => addEventListener('review:navigate', () => {
                mounted = true;
                // The reconnect notice can disappear during hydration. Isolate explicit
                // navigation resets from the browser's own layout anchoring adjustment.
                document.documentElement.style.overflowAnchor = 'none';
                document.body.style.minHeight = '30000px';
                // Keep the first document section within its 600px visibility margin so
                // hydration, including its load-time anchor callback, actually completes.
                scrollTo(0,400);
                window.earlyScroll = scrollY;
                resolve();
            }, {once:true}));
            const fetchData = window.fetch;
            window.fetch = async (...args) => {
                const response = await fetchData(...args);
                const url = typeof args[0] === 'string' ? args[0] : args[0].url;
                if (/\/api\/(document|queue)(\?|$)/.test(url)) {
                    await mount;
                    window.dataAfterMount = mounted;
                }
                return response;
            };
        "#}),
        );
        chrome.open(&site.url(path));
        assert!(
            chrome.wait_for(
                &format!("{READY} && document.querySelector('[data-ready={ready}]') !== null"),
                SHOWN
            ),
            "{path}: {}",
            chrome.eval("document.body.innerText")
        );
        assert_eq!(chrome.eval("window.dataAfterMount"), json!(true), "{path}");
        assert_eq!(chrome.eval("window.earlyScroll"), json!(400), "{path}");
        assert_eq!(
            chrome.eval("scrollY"),
            json!(400),
            "{path}: loading undid the early scroll"
        );
    }
}

#[test]
fn old_page_scrolling_during_navigation_does_not_keep_the_new_page_offset() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    assert!(chrome.wait_for(READY, SHOWN));
    chrome.eval("document.body.style.minHeight = '30000px'; scrollTo(0,5000)");
    assert_eq!(chrome.eval("scrollY"), json!(5000));
    chrome.eval("document.querySelector('header a[href=\"/tasks\"]').click(); window.oldPageShown = !document.getElementById('review-root-review').hidden; scrollTo(0,4000); window.oldPageScroll = scrollY");
    assert_eq!(chrome.eval("window.oldPageShown"), json!(true));
    assert_eq!(chrome.eval("window.oldPageScroll"), json!(4000));
    assert!(chrome.wait_for(
        &format!("location.pathname === '/tasks' && {READY} && document.querySelector('.kanban') !== null"),
        SHOWN
    ));
    assert_eq!(chrome.eval("scrollY"), json!(0));
    chrome.eval("scrollTo(0,5000)");
    assert_eq!(chrome.eval("scrollY"), json!(5000));
    chrome.eval("history.back()");
    assert!(chrome.wait_for(
        &format!(
            "location.pathname === '/' && {READY} && document.querySelector('.hero-title') !== null"
        ),
        SHOWN
    ));
    assert_eq!(chrome.eval("scrollY"), json!(0));
}

#[test]
fn a_failed_navigation_reports_failed_with_the_page_error() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    assert!(chrome.wait_for(READY, SHOWN));
    chrome.call(
        "Network.setBlockedURLs",
        json!({"urls":["*/assets/board-*.js"]}),
    );
    chrome.eval("document.querySelector('header a[href=\"/tasks\"]').click()");
    assert!(chrome.wait_for(
        "document.documentElement.dataset.navigation === 'failed'",
        SHOWN
    ));
    assert_eq!(chrome.eval("location.pathname"), json!("/tasks"));
    assert_eq!(
        chrome.eval("document.querySelector('#module-error[role=alert]')?.textContent"),
        json!("This page could not be loaded. Retry")
    );
}

#[test]
fn a_page_whose_build_is_gone_loads_again_once_and_one_still_served_says_the_failure() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    assert!(chrome.wait_for(READY, SHOWN));
    chrome.eval("window.__kept = true");
    let shell = chrome.eval(
        "document.querySelector(\"script[src*='/assets/symphony-shell-']\").getAttribute('src')",
    );
    let shell = shell.as_str().unwrap().to_owned();
    // The board's module, which this page has not loaded.
    let script = std::fs::read_to_string(dist().join(shell.trim_start_matches('/'))).unwrap();
    let modules: serde_json::Value = serde_json::from_str(
        script
            .split("const modules = ")
            .nth(1)
            .and_then(|rest| rest.split(";\n").next())
            .unwrap(),
    )
    .unwrap();
    let board = modules["board"]["script"].as_str().unwrap();
    let loads = |site: &Site| {
        site.requests()
            .iter()
            .filter(|path| *path == "/tasks")
            .count()
    };
    // Its build still served, a module that does not load is said, and the page stays.
    site.retire(board);
    chrome.eval("document.querySelector('header a[href=\"/tasks\"]').click()");
    assert!(chrome.wait_for(
        "document.documentElement.dataset.navigation === 'failed'",
        SHOWN
    ));
    assert_eq!(
        chrome.eval("document.querySelector('#module-error[role=alert]')?.textContent"),
        json!("This page could not be loaded. Retry")
    );
    assert_eq!(chrome.eval("window.__kept === true"), json!(true));
    assert_eq!(loads(&site), 0, "{:?}", site.requests());
    // What it leaves on screen: the page before and its header, under the notice, never an empty
    // page or the board half drawn.
    assert_eq!(
        chrome.eval(
            "['.hero-title', '.app-header'].every(selector => document.querySelector(selector)?.getClientRects().length > 0) && document.querySelector('.kanban') === null"
        ),
        json!(true),
        "the page before is hidden under the notice: {:?}",
        chrome.eval("document.body.innerText.slice(0, 2000)")
    );
    // Its build gone from the server, the page loads again, onto the build served now.
    site.retire(&shell);
    chrome.eval("document.querySelector('#module-error button').click()");
    if let Err(stall) =
        test_time::until_true("a page whose build is gone loaded again", STALL, || {
            loads(&site) >= 1
        })
    {
        panic!("{stall}: {:?}", site.requests());
    }
    // This stand-in serves no other build, so the page loaded again finds its build gone too: it
    // loads no more.
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a test holds for a length of time to show the page loads no more"
    )]
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(loads(&site), 1, "{:?}", site.requests());
    assert_eq!(chrome.eval("window.__kept === undefined"), json!(true));
}

/// Waits until the page has asked the stand-in for a URL holding `query`, pumping the page's
/// events between looks; a stall past [`STALL`] fails, saying `what` and the URLs it asked for.
#[track_caller]
fn fetched(chrome: &mut Chrome, site: &Site, query: &str, what: &str) {
    let asked = || site.targets().iter().any(|url| url.contains(query));
    if let Err(stall) = test_time::until_true(what, STALL, || {
        asked() || {
            chrome.pump(Duration::from_millis(25));
            false
        }
    }) {
        panic!("{stall}: {:?}", site.targets());
    }
}

/// The window of finished tasks already chosen, chosen again, reads the board again as the Dioxus
/// page did: its columns never wait empty for the next event.
#[test]
fn task_controls_read_the_board_again_for_the_window_already_chosen() {
    let site = site();
    site.set_json("/api/task-board", &site::example_listing("ready"));
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport(1440, 900, false);
    chrome.open(&site.url("/tasks"));
    let shown = "document.querySelector('[data-slug=\"tools/example\"]') !== null && document.querySelector('.column[aria-busy=true]') === null";
    assert!(
        chrome.wait_for(shown, SHOWN),
        "the board never drew its card"
    );
    let reads = |site: &Site| {
        site.targets()
            .iter()
            .filter(|url| url.contains("status=ready"))
            .count()
    };
    let before = reads(&site);
    components::board_filters(&mut chrome);
    components::choose(
        &mut chrome,
        ".filters button[aria-label='Finished tasks']",
        "Last 24 hours",
    );
    if let Err(stall) = test_time::until_true("the column read again", STALL, || {
        reads(&site) > before || {
            chrome.pump(Duration::from_millis(25));
            false
        }
    }) {
        panic!("{stall}: {:?}", site.targets());
    }
    assert!(
        chrome.wait_for(shown, SHOWN),
        "the column was left without its card: {:?}",
        chrome.eval("document.querySelector('.column[data-status=ready]')?.outerHTML")
    );
}

#[test]
fn task_controls_keep_their_values_and_revealed_rows_after_scrolling() {
    let site = site();
    site.set_json("/api/task-board", &site::example_listing("ready"));
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport(1440, 900, false);
    chrome.call("Page.addScriptToEvaluateOnNewDocument", json!({"source":
        "const observer = new MutationObserver(() => { if (document.head) { const style = document.createElement('style'); style.textContent = '.column { margin-top: 20000px }'; document.head.append(style); observer.disconnect(); } }); observer.observe(document, {childList: true, subtree: true});"
    }));
    chrome.open(&site.url("/tasks"));
    assert!(chrome.wait_for("document.querySelector('.kanban') !== null", SHOWN));
    assert!(chrome.wait_for(READY, SHOWN));
    assert_eq!(
        chrome.eval("document.querySelector('[data-slug]') === null"),
        json!(true)
    );
    assert!(site.targets().iter().any(|url| url.contains("counts=1")));
    assert!(
        !site
            .targets()
            .iter()
            .any(|url| url.contains("status=ready"))
    );
    // The waiting columns follow the page; approaching their reserved boxes reveals them.
    chrome.eval("scrollTo(0, 20000)");
    assert!(
        chrome.wait_for(
            "document.querySelector('[data-slug=\"tools/example\"]') !== null",
            SHOWN
        ),
        "the scrolled-to row never appeared: {:?}; {:?}",
        site.targets(),
        chrome.eval("(b => ({scrollTop: b.scrollTop, scrollHeight: b.scrollHeight, clientHeight: b.clientHeight}))(document.scrollingElement)")
    );
    chrome.eval("scrollTo(0, 0)");
    assert!(chrome.wait_for("scrollY === 0", SHOWN));
    assert_eq!(
        chrome.eval("document.querySelectorAll('[data-slug=\"tools/example\"]').length"),
        json!(1)
    );
    components::board_filters(&mut chrome);
    // The real control is deliberately not focused. Read its node, not activeElement, and
    // preserve the latest synchronous edit when the request's debounce runs later.
    chrome.eval("const decoy = document.createElement('input'); decoy.value = 'wrong'; document.body.prepend(decoy); decoy.focus(); const input = document.querySelector('input[type=search]'); input.value = 'alpha'; input.dispatchEvent(new Event('input', {bubbles:true})); input.value = 'beta'; input.dispatchEvent(new Event('input', {bubbles:true}));");
    assert!(chrome.wait_for(
        "document.querySelector('input[type=search]').value === 'beta'",
        SHOWN
    ));
    fetched(
        &mut chrome,
        &site,
        "counts=1&q=beta&days=1",
        "the search fetched beta",
    );
    components::choose(
        &mut chrome,
        ".filters button[aria-label='Finished tasks']",
        "Last week",
    );
    fetched(
        &mut chrome,
        &site,
        "counts=1&q=beta&days=7",
        "the window selector kept the search",
    );
    assert!(!site.targets().iter().any(|url| url.contains("q=wrong")));
    // A new filter replaces rows, so its new visibility latches must be approached again.
    // The earlier assertion checked that scrolling alone preserves the revealed row.
    chrome.eval("scrollTo(0, 20000)");
    assert!(
        chrome.wait_for(
            "document.querySelector('[data-slug=\"tools/example\"]') !== null",
            SHOWN
        ),
        "the filtered row never appeared: {:?}; {:?}",
        site.targets(),
        chrome.eval("document.body.innerHTML")
    );
}

#[test]
fn a_revealed_column_keeps_its_rows_look_ahead_before_it_reaches_the_screen() {
    let site = site();
    site.set_json(
        "/api/task-board",
        &json!({
            "tasks": [{"slug": "tools/blocked-example", "title": "Blocked example", "body": "",
                "status": "ready", "priority": 1, "parent": null, "created_by": "owner",
                "created": 0.0, "updated": 0.0, "links": [], "depends": [], "roles": [],
                "instruction": false}],
            "statuses": ["ready"], "counts": [["ready", 1]], "signoff": [], "next": null
        }),
    );
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport(1280, 800, false);
    chrome.call("Page.addScriptToEvaluateOnNewDocument", json!({"source":
        "const observer = new MutationObserver(() => { if (document.head) { const style = document.createElement('style'); style.textContent = '.column[data-status=ready] { margin-top: 20000px }'; document.head.append(style); observer.disconnect(); } }); observer.observe(document, {childList: true, subtree: true});"
    }));
    chrome.open(&site.url("/tasks"));
    assert!(chrome.wait_for(
        &format!("{READY} && document.querySelector('#column-ready') !== null"),
        SHOWN
    ));
    assert!(
        !site
            .targets()
            .iter()
            .any(|url| url.contains("status=ready"))
    );
    // The offline columns scroll with the page. Stop with the column's slot 300 px below the
    // screen: its own look-ahead reveals it, and its row's look-ahead, which reaches 300 px
    // into the screen, must then reveal the card while the column is still off screen.
    chrome.eval("const column = document.querySelector('#column-ready'); scrollTo(0, column.getBoundingClientRect().top + scrollY - innerHeight - 300)");
    assert!(
        chrome.wait_for(
            "document.querySelector('[data-slug=\"tools/blocked-example\"]') !== null",
            SHOWN
        ),
        "the row's look-ahead was clipped: {}",
        chrome.eval("JSON.stringify({innerHeight, column: document.querySelector('#column-ready').getBoundingClientRect().top, row: document.querySelector('[id=\"row-tools/blocked-example\"]')?.outerHTML})")
    );
    assert_eq!(
        chrome.eval("document.querySelector('#column-ready').getBoundingClientRect().top >= innerHeight + 299"),
        json!(true),
        "the column reached the screen, so the case no longer tests the row's look-ahead"
    );
}

#[test]
fn an_unrevealed_mockup_replaced_by_a_picture_attaches_to_its_new_node() {
    let site = site();
    let mut board: serde_json::Value =
        serde_json::from_str(&common::fixture("board.json")).unwrap();
    board["next"][0]["pictures"] = json!([{"src":"/mockup.html", "caption":"Before"}]);
    site.set_board(&board.to_string());
    let Some(mut chrome) = chrome() else { return };
    chrome.call("Page.addScriptToEvaluateOnNewDocument", json!({"source":
        "const observer = new MutationObserver(() => { if (document.head) { const style = document.createElement('style'); style.textContent = '.hero .pics { margin-top: 20000px }'; document.head.append(style); observer.disconnect(); } }); observer.observe(document, {childList: true, subtree: true});"
    }));
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    assert!(chrome.wait_for(
        "document.querySelector('.hero .mockup-shell') !== null",
        SHOWN
    ));
    assert_eq!(
        chrome.eval("document.querySelector('.hero iframe') === null"),
        json!(true)
    );
    wait_for_feed(&site).expect("the page never followed the feed");
    board["next"][0]["pictures"] = json!([{"src":"/replacement.png", "caption":"After"}]);
    let event = format!(
        r#"{{"version":4183,"changes":[{{"region":"card","data":{}}}]}}"#,
        board["next"][0]
    );
    site.push(4183, &event);
    assert!(chrome.wait_for(
        "document.querySelector('.hero .picture-shell') !== null",
        SHOWN
    ));
    assert_eq!(
        chrome.eval("document.querySelector('.hero img') === null"),
        json!(true)
    );
    chrome.eval("document.querySelector('.hero .picture-shell').scrollIntoView({block:'center'})");
    assert!(
        chrome.wait_for(
            "document.querySelector('.hero img')?.getAttribute('src') === '/replacement.png'",
            SHOWN
        ),
        "the replacement picture never revealed: {:?}",
        chrome.eval("document.querySelector('.hero').outerHTML")
    );
}

/// Home's pictures open in the overlay's picture viewer, its arrows moving through the next card's
/// pictures alone (a later card's thumbnail is a link to its card) and Escape closing it; its
/// mockup's frame is scaled to its box. Each wait is on the page, never on a clock.
#[test]
fn home_opens_its_pictures_in_the_viewer_and_scales_its_mockup() {
    let site = site();
    let mut board: serde_json::Value =
        serde_json::from_str(&common::fixture("board.json")).unwrap();
    board["next"][0]["pictures"] = json!([
        {"src":"/first.svg", "caption":"First"},
        {"src":"/mockup.html", "caption":"Mockup"},
        {"src":"/second.svg", "caption":"Second"},
    ]);
    site.set_board(&board.to_string());
    for name in ["first", "second"] {
        site.set_file(
            &format!("/{name}.svg"),
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200"/>"#.to_vec(),
            "image/svg+xml",
        );
    }
    site.set_file(
        "/mockup.html",
        b"<!doctype html><title>Mockup</title><p>Mockup</p>".to_vec(),
        "text/html",
    );
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    // The pictures come in as they near the screen, and the test's window leaves them under it.
    chrome.eval("document.querySelector('.hero .pics').scrollIntoView({block:'center'})");
    assert!(
        chrome.wait_for(
            "document.querySelectorAll('.hero img[data-zoom-ready][role=button][tabindex=\"0\"]').length === 2 \
             && document.querySelector('.hero .mockup-frame iframe')?.getAttribute('style')?.startsWith('transform:scale(') === true",
            SHOWN
        ),
        "{}",
        chrome.eval("document.querySelector('.hero').outerHTML")
    );
    assert_eq!(
        chrome.eval("document.querySelector('.hero img[alt=Second]').getAttribute('aria-label')"),
        json!("Open picture Second")
    );
    chrome.eval("document.querySelector('.hero img[alt=Second]').click()");
    assert!(
        chrome.wait_for(
            "document.querySelector('#media-lightbox[data-state=open] img')?.getAttribute('src') === '/second.svg'",
            SHOWN
        ),
        "the viewer never opened: {}",
        chrome.eval("document.querySelector('#overlay-root')?.outerHTML ?? 'no overlay root'")
    );
    // The next card's two pictures, its mockup no picture of the set: the second of two.
    assert_eq!(
        chrome.eval("document.querySelector('#media-lightbox .lightbox-caption').textContent"),
        json!("Second · 2 / 2")
    );
    chrome.eval(
        "document.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true}))",
    );
    assert!(chrome.wait_for(
        "document.querySelector('#media-lightbox img')?.getAttribute('src') === '/first.svg'",
        SHOWN
    ));
    chrome.eval(
        "document.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowLeft',bubbles:true}))",
    );
    assert!(chrome.wait_for(
        "document.querySelector('#media-lightbox img')?.getAttribute('src') === '/second.svg'",
        SHOWN
    ));
    components::key(&mut chrome, "Escape", "Escape", 27);
    assert!(chrome.wait_for(
        "document.querySelector('#media-lightbox[data-state=open]') === null",
        SHOWN
    ));
    // The keyboard opens it too, on the picture it is on.
    chrome.eval("document.querySelector('.hero img[alt=First]').focus()");
    components::key(&mut chrome, "Enter", "Enter", 13);
    assert!(chrome.wait_for(
        "document.querySelector('#media-lightbox[data-state=open] img')?.getAttribute('src') === '/first.svg'",
        SHOWN
    ));
}

#[test]
fn tasks_and_their_controls_work_without_a_visibility_observer() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source":
            "window.IntersectionObserver = undefined;"
        }),
    );
    chrome.open(&site.url("/tasks"));
    assert_eq!(
        chrome.eval("typeof IntersectionObserver"),
        json!("undefined")
    );
    assert!(chrome.wait_for(
        "document.querySelector('[data-slug=\"tools/example\"]') !== null",
        SHOWN
    ));
    chrome.eval("document.querySelector('.wait-chip.k-nobody').click()");
    assert!(
        chrome.wait_for(
            "document.querySelector('.column[data-status=waiting] .taskcard-title')?.textContent === 'Example task'",
            SHOWN
        ),
        "the fallback left task controls unusable: {:?}",
        chrome.eval("document.body.innerText")
    );
    assert!(site.targets().iter().any(|url| url.contains("wait=nobody")));
}

/// A task the server sends with its plugins' board fields draws on the board as one without them
/// does, whatever their values: the board draws no field, so its guard (`readTask` in the web
/// client's `task.ts`) checks only what a card draws and leaves the fields as `JSON.parse` read
/// them, not even refusing a value the fields' own reader refuses (a boolean:
/// `symphony_blocks::FieldValue` is a number or text). That is the client's to accept: a page one
/// build behind a server whose plugins write a new kind of value still draws the board, where a
/// reader of the fields would lose the whole listing to the one value.
#[test]
fn a_task_with_plugin_board_fields_draws_on_the_board() {
    let site = site();
    site.set_json(
        "/api/task-board",
        &json!({
            "tasks": [{"slug": "game/jump", "title": "Jump", "body": "", "status": "review",
                "priority": 2, "parent": null, "created_by": "owner", "created": 0.0,
                "updated": 0.0, "links": [], "depends": [], "roles": [], "instruction": false,
                "fields": {"game": {"effort": 3.0, "risk": "high", "done": true}}}],
            "statuses": ["review"], "counts": [["review", 1]], "signoff": [], "next": null
        }),
    );
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/tasks"));
    assert!(
        chrome.wait_for(
            "document.querySelector('[data-slug=\"game/jump\"]')?.textContent.includes('Jump') === true",
            SHOWN
        ),
        "the task with board fields was not drawn: {:?}",
        chrome.eval("document.body.innerText")
    );
}

#[test]
fn queued_visibility_callbacks_are_drained_when_a_page_unmounts() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    // IntersectionObserver queues entries independently of disconnect. Hold notification so
    // teardown deterministically happens before a queued callback would be delivered.
    chrome.call("Page.addScriptToEvaluateOnNewDocument", json!({"source": r#"
        window.heldObservers = [];
        window.IntersectionObserver = class {
            constructor(callback) { this.callback = callback; this.records = []; heldObservers.push(this); }
            observe(target) { this.records.push({target, isIntersecting: true}); }
            disconnect() {}
            takeRecords() { return this.records.splice(0); }
        };
    "#}));
    chrome.open(&site.url("/tasks"));
    assert!(chrome.wait_for("document.querySelector('.kanban') !== null", SHOWN));
    assert_eq!(
        chrome.eval("heldObservers.some(o => o.records.length > 0)"),
        json!(true)
    );
    chrome.eval("window.oldObservers = heldObservers.slice(); document.querySelector('header a[href=\"/\"]').click()");
    wait_for_board(&mut chrome);
    assert_eq!(
        chrome.eval("document.querySelector('#review-root-board').childElementCount"),
        json!(0)
    );
    let errors = chrome.eval(r#"(() => {
        const errors = [];
        for (const observer of oldObservers) {
            const records = observer.takeRecords();
            if (records.length) {
                try { observer.callback(records, observer); } catch (error) { errors.push(String(error)); }
            }
        }
        return errors;
    })()"#);
    assert_eq!(errors, json!([]), "a queued callback outlived its page");
}

#[test]
fn the_transfer_budget_counts_api_failures_and_open_streams_but_not_images_cache_prefetch_or_pings()
{
    let requests = [
        chrome::Request {
            kind: "Other".into(),
            bytes: 9_000.0,
            finished: true,
            prefetch: true,
            ..Default::default()
        },
        chrome::Request {
            kind: "Other".into(),
            bytes: 7_000.0,
            from_cache: true,
            finished: true,
            prefetch: true,
            ..Default::default()
        },
        chrome::Request {
            kind: "Fetch".into(),
            bytes: 120.0,
            finished: true,
            ..Default::default()
        },
        chrome::Request {
            kind: "Fetch".into(),
            bytes: 60.0,
            status: 404,
            finished: true,
            ..Default::default()
        },
        chrome::Request {
            kind: "EventSource".into(),
            bytes: 42.0 + 2.0 * 19.0,
            pings: 2,
            ..Default::default()
        },
        chrome::Request {
            kind: chrome::WEB_SOCKET.into(),
            bytes: 129.0 + 3.0 * 7.0,
            pings: 3,
            ..Default::default()
        },
        chrome::Request {
            kind: "Image".into(),
            bytes: 500.0,
            finished: true,
            ..Default::default()
        },
        chrome::Request {
            kind: "Script".into(),
            bytes: 800.0,
            from_cache: true,
            finished: true,
            ..Default::default()
        },
    ];
    // A ping is 19 bytes as an event, and 7 as a WebSocket's frame: its head and `ping\n`.
    assert_eq!(site::ping().sse().len(), 19);
    assert_eq!(site::ping().frame(), b"\x81\x05ping\n");
    assert_eq!(needed_bytes(&requests), 222.0 + 129.0);
    assert_eq!(ping_bytes(&requests), 38.0 + 21.0);
    assert_eq!(
        prefetched_bytes(&requests),
        9_000.0,
        "a prefetch served from the cache cost nothing"
    );
}

#[test]
fn the_board_shows_the_next_card_the_four_after_it_the_counts_and_progress() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    assert_eq!(
        texts(&mut chrome, ".hero-title"),
        ["North · pendant picture"]
    );
    assert_eq!(
        texts(&mut chrome, ".nextcard strong"),
        [
            "Should a rock's yield fall off with each pass of the beam, or stay flat until it is spent?",
            "West · chandelier picture",
            "The dock menu lists repair before trade.",
            "East lamps keep the stepped prow across every size class.",
        ]
    );
    assert_eq!(
        texts(&mut chrome, ".tile b"),
        ["37", "4", "6", "12 · 5", "14"]
    );
    assert_eq!(texts(&mut chrome, ".bar .bar-name").len(), 6);
    let href = chrome.eval("document.querySelector('.hero .go').getAttribute('href')");
    assert_eq!(href, json!("/docs/lamps/north.md#north-pendant-picture"));
}

/// Waits for the page to hold a feed stream open, which a quiet machine has within 10 s; a stall
/// past its backstop ([`test_time::backstop`]) is the error.
fn wait_for_feed(site: &Site) -> Result<(), test_time::Stall> {
    test_time::until_true(
        "the page holding a feed stream open",
        test_time::backstop(Duration::from_secs(10)),
        || site.feeds() > 0,
    )
}

/// The board fixture as a server at `version`, with `waiting` cards on the owner and pinging its
/// feed every `ping_ms`, answers it.
fn board_at(version: u64, waiting: u32, ping_ms: u32) -> String {
    let mut board: serde_json::Value =
        serde_json::from_str(&common::fixture("board.json")).unwrap();
    board["version"] = json!(version);
    board["counts"]["waiting"] = json!(waiting);
    board["ping_ms"] = json!(ping_ms);
    board.to_string()
}

/// The servers' ping interval here, the longest the contract allows (the fixture's).
const PING_MS: u32 = symphony_api::PING_MAX_MS;

/// The ping of a board whose test waits its silence out: the shortest such a test names, the
/// stand-in's pings and the page's timers being a loaded machine's, so the page allows 5 s. A
/// stream that falls silent but stays open is given up within two and a half pings: the owner
/// reads the board from a phone over the tailnet, where that happens on every network change
/// (ruled on tools/python-zero/ui-board).
const SHORT_PING_MS: u32 = 2_000;

/// Have the page record whether it ever says its board may be out of date: the notice shows only
/// while the page heals. [`stale_seen`] reads the record.
fn record_stale(chrome: &mut Chrome) {
    chrome.eval(
        "window.__stale = false; new MutationObserver(() => { \
         if (document.querySelector('[data-feed=stale] .stale')) window.__stale = true }) \
         .observe(document.body, {subtree: true, childList: true, attributes: true}); true",
    );
}

fn stale_seen(chrome: &mut Chrome) -> bool {
    chrome.eval("window.__stale") == json!(true)
}

/// Wait for the board to show `waiting` cards on the owner, up to [`HEALED`]; whether it does.
fn shows_waiting(chrome: &mut Chrome, waiting: u32) -> bool {
    chrome.wait_for(
        &format!("document.querySelector('.tile b').textContent.trim() === '{waiting}'"),
        HEALED,
    )
}

/// The event of delta-card.json at `version`, on one line as an event's data must be, and in the
/// server's key order: a change names its region before its data (crates/symphony-api), which a
/// serde_json::Value would sort away.
fn card_event(version: u64) -> String {
    let delta = common::fixture("delta-card.json");
    let at = "\"version\": 4183,";
    assert!(delta.contains(at), "delta-card.json moved");
    delta
        .replacen(at, &format!("\"version\": {version},"), 1)
        .lines()
        .map(str::trim)
        .collect()
}

/// The page shows delta-card.json's card.
const CARD_CHANGED: &str = "Array.from(document.querySelectorAll('.nextcard strong')).some(e => e.textContent.includes('refuel before both'))";
/// How long a page may take to heal a lost feed: the client's first waits are 1 s and 2 s.
const HEALED: Duration = Duration::from_secs(15);

#[test]
fn one_feed_event_changes_one_card_in_place_with_no_page_fetch() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    wait_for_feed(&site).expect("the page never followed the feed");
    assert_eq!(site.feeds(), 1, "the page follows the feed on one stream");
    // A mark on the page: a reload or a redraw of the whole board would lose it.
    chrome.eval("document.querySelector('.hero').dataset.mark = 'kept'; window.__kept = true");
    // The board's own pictures are fetched after it is drawn; on a loaded machine they reached
    // the site after the snapshot, which charged them to the event.
    assert!(
        chrome.wait_for(
            "Array.from(document.images).every(image => image.complete)",
            SHOWN
        ),
        "the board's pictures never finished loading"
    );
    // Once the board has its data and is idle, the shell warms the chat's and the menus' files,
    // once in the page's life and never for an event; on a loaded machine that warm-up reached
    // the site after the snapshot, which charged it to the event. The window opens once it is
    // over, so whatever the site sees in it, the event asked for.
    panel_open::settled(&mut chrome);
    let before = site.requests();
    site.push(4183, &card_event(4183));
    assert!(
        chrome.wait_for(CARD_CHANGED, Duration::from_secs(10)),
        "the card did not change: {:?}",
        texts(&mut chrome, ".nextcard strong")
    );
    chrome.pump(Duration::from_millis(500));
    assert_eq!(site.requests(), before, "the event fetched something");
    assert_eq!(
        chrome.eval("window.__kept === true && document.querySelector('.hero').dataset.mark"),
        json!("kept")
    );
    assert_eq!(
        texts(&mut chrome, ".nextcard strong")[0],
        "Should a rock's yield fall off with each pass of the beam, or stay flat until it is spent?",
        "the other cards stay as they were"
    );
    assert_eq!(texts(&mut chrome, ".tile b")[0], "37");
}

#[test]
fn a_feed_answered_500_once_heals_from_a_fresh_board_shown_stale_meanwhile() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    wait_for_feed(&site).expect("the page never followed the feed");
    record_stale(&mut chrome);
    // The server stops, and comes back with a board that moved meanwhile, answering the feed's
    // first request 500: a stream the browser will not open again by itself.
    site.fail_feeds(1);
    site.set_board(&board_at(4190, 99, PING_MS));
    site.drop_feeds();
    assert!(
        shows_waiting(&mut chrome, 99),
        "the board was never fetched again: {:?}",
        texts(&mut chrome, ".tile b")
    );
    assert!(
        chrome.wait_for(
            "document.querySelector('[data-feed=live]') !== null",
            HEALED
        ),
        "the page never followed the feed again after {} feed requests",
        site.feed_requests()
    );
    wait_for_feed(&site).expect("the page never held its feed again");
    assert!(
        stale_seen(&mut chrome),
        "the page never said its board may be out of date while it healed"
    );
    assert!(
        site.feed_requests() >= 3,
        "{} feed requests: the first, the 500 and the one that held",
        site.feed_requests()
    );
    site.push(4191, &card_event(4191));
    assert!(
        chrome.wait_for(CARD_CHANGED, Duration::from_secs(10)),
        "the next event after the feed healed was lost"
    );
}

#[test]
fn a_restarted_server_counting_versions_from_one_again_is_followed_from_its_board() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    wait_for_feed(&site).expect("the page never followed the feed");
    // The page is at version 4182; the server restarts at version 1.
    site.set_board(&board_at(1, 99, PING_MS));
    site.drop_feeds();
    assert!(
        shows_waiting(&mut chrome, 99),
        "the restarted server's board was never shown: {:?}",
        texts(&mut chrome, ".tile b")
    );
    assert!(
        chrome.wait_for(
            "document.querySelector('[data-feed=live]') !== null",
            HEALED
        ),
        "the page never followed the restarted server's feed"
    );
    wait_for_feed(&site).expect("the page never held the restarted server's feed");
    site.push(2, &card_event(2));
    assert!(
        chrome.wait_for(CARD_CHANGED, Duration::from_secs(10)),
        "the restarted server's next event was ignored"
    );
}

#[test]
fn an_event_the_client_cannot_read_is_a_lost_stream_healed_from_a_fresh_board() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    wait_for_feed(&site).expect("the page never followed the feed");
    record_stale(&mut chrome);
    // Each event carried a change the page cannot read; the board has it.
    site.set_board(&board_at(4190, 99, PING_MS));
    site.push(4190, "{not json");
    assert!(
        shows_waiting(&mut chrome, 99),
        "an event JSON.parse refuses was dropped and the board never fetched again: {:?}",
        texts(&mut chrome, ".tile b")
    );
    assert!(
        stale_seen(&mut chrome),
        "the page never showed it was stale"
    );
    assert!(
        chrome.wait_for(
            "document.querySelector('[data-feed=live]') !== null",
            HEALED
        ),
        "the page never followed the feed again"
    );
    wait_for_feed(&site).expect("the page never held its feed again");
    site.set_board(&board_at(4191, 98, PING_MS));
    site.push(4191, r#"{"version":"4191","changes":"none"}"#);
    assert!(
        shows_waiting(&mut chrome, 98),
        "an event that is no delta was skipped and the board never fetched again: {:?}",
        texts(&mut chrome, ".tile b")
    );
}

#[test]
fn a_stream_that_falls_silent_is_lost_and_healed_while_one_that_pings_holds() {
    let site = site();
    // The board names a shorter ping than the fixture's, so the silence the page allows, two and
    // a half of it, is shorter to hold past and to wait out: 5 s. That it is 12.5 s at the longest
    // ping the contract allows is the client's unit test's (apps/symphony-web/src/live.test.tsx).
    let silence = Duration::from_millis(u64::from(SHORT_PING_MS) * 5 / 2);
    let mut board: serde_json::Value =
        serde_json::from_str(&common::fixture("board.json")).unwrap();
    board["ping_ms"] = json!(SHORT_PING_MS);
    site.set_board(&board.to_string());
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    wait_for_feed(&site).expect("the page never followed the feed");
    record_stale(&mut chrome);
    // The server pings as its board says: the stream holds past the longest silence allowed.
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a test holds past the feed's longest silence to show the stream stays"
    )]
    std::thread::sleep(silence + Duration::from_secs(2));
    assert_eq!(site.feed_requests(), 1, "a stream that pings was given up");
    assert!(!stale_seen(&mut chrome), "a stream that pings showed stale");
    // The server falls silent with the socket open, its board moved meanwhile: given up within
    // the bound, the page waits its first second and shows the board fetched again.
    site.fall_silent();
    site.set_board(&board_at(4190, 99, SHORT_PING_MS));
    let healed = silence + Duration::from_secs(5);
    assert!(
        chrome.wait_for(
            "document.querySelector('.tile b').textContent.trim() === '99'",
            healed
        ),
        "a stream silent for {healed:?} was never given up: {} feed requests, {} board fetches",
        site.feed_requests(),
        site.board_requests()
    );
    assert!(
        stale_seen(&mut chrome),
        "the page never showed it was stale"
    );
}

#[test]
fn a_client_healed_onto_another_build_loads_the_page_again_once() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    wait_for_feed(&site).expect("the page never followed the feed");
    chrome.eval("window.__kept = true");
    let shells = |site: &Site| site.requests().iter().filter(|path| *path == "/").count();
    assert_eq!(shells(&site), 1);
    // The server is deployed again with another client build, and restarts.
    site.deploy("/assets/symphony-shell-0000000000000000.js");
    site.drop_feeds();
    if let Err(stall) = test_time::until_true(
        "a client healed onto another build loaded the page again",
        STALL,
        || shells(&site) >= 2,
    ) {
        panic!("{stall}: {:?}", site.requests());
    }
    // This stand-in still serves the old files, so the page loaded again is not the build the
    // board names either: it loads no more.
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a test holds for a length of time to show the page loads no more"
    )]
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        shells(&site),
        2,
        "the page loaded again {} times for one build",
        shells(&site) - 1
    );
    wait_for_board(&mut chrome);
    assert_eq!(
        chrome.eval("window.__kept === undefined"),
        json!(true),
        "the page shown is a new one"
    );
}

#[test]
fn a_stream_that_ends_as_soon_as_it_opens_is_tried_again_ever_later() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    wait_for_feed(&site).expect("the page never followed the feed");
    site.end_feeds_on_open();
    let before = site.board_requests();
    let feeds_before = site.feed_requests();
    site.drop_feeds();
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a test holds for a length of time to count the page's reads after its feeds drop"
    )]
    std::thread::sleep(Duration::from_secs(8));
    let fetches = site.board_requests() - before;
    let feed_retries = site.feed_requests() - feeds_before;
    assert!(
        fetches > 0 && feed_retries > 0,
        "the lost stream was never followed again: {fetches} board fetches and {feed_retries} feed requests in 8 s"
    );
    assert!(
        fetches <= 3,
        "{fetches} board fetches in 8 s for streams that end as they open; waits of 1, 2 and 4 s allow 3"
    );
}

/// Chrome keeps a page that was left for Back (its back/forward cache) and opens at most six
/// connections to one server. A page kept there lets go of its feed's stream, or five pages loaded
/// in one tab, with the page shown, hold all six, and the page shown can read nothing more: not
/// its board's columns, not the chat's module (tests/polish-buttons-test-red). Shown again, it
/// follows the feed again from a fresh board.
#[test]
fn a_page_kept_for_back_lets_go_of_its_feed_and_follows_it_again_when_shown() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    // Whether a page went into the back/forward cache when it was left, and came back from it.
    chrome.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": "addEventListener('pagehide', event => { if (event.persisted) sessionStorage.setItem('kept', Number(sessionStorage.getItem('kept')) + 1); });\
            addEventListener('pageshow', event => { if (event.persisted) sessionStorage.setItem('shown', 'cache'); });"}),
    );
    // Six visits, each a page of its own: Chrome replaces, rather than keeps, a page left for its
    // own URL.
    for visit in 1..=6 {
        chrome.open(&site.url(&format!("/?visit={visit}")));
        wait_for_board(&mut chrome);
        test_time::until_true(format!("visit {visit} following the feed"), STALL, || {
            site.feed_requests() >= visit || {
                chrome.pump(Duration::from_millis(50));
                false
            }
        })
        .expect("a visit never followed the feed");
    }
    assert_eq!(
        chrome.eval("sessionStorage.getItem('kept')"),
        json!("5"),
        "every page left went into the back/forward cache, or this case tests nothing"
    );
    chrome.eval(
        "window.__read = null; fetch('/api/activity').then(response => window.__read = response.status)",
    );
    assert!(
        chrome.wait_for("window.__read === 200", SHOWN),
        "the page shown could not read from the server with five pages kept for Back"
    );
    let feeds = site.feed_requests();
    chrome.eval("history.back()");
    assert!(
        chrome.wait_for(
            "location.search === '?visit=5' && sessionStorage.getItem('shown') === 'cache'",
            SHOWN
        ),
        "Back did not show the page kept for it: {}",
        chrome.eval("[location.href, sessionStorage.getItem('shown')]")
    );
    test_time::until_true(
        "the page shown again asking for the feed again",
        test_time::backstop(HEALED),
        || {
            site.feed_requests() > feeds || {
                chrome.pump(Duration::from_millis(50));
                false
            }
        },
    )
    .expect("the page shown again never asked for the feed again: it let go of its stream, so it follows nothing");
    assert!(
        chrome.wait_for(
            "document.querySelector('[data-feed=live]') !== null",
            HEALED
        ),
        "the page shown again never followed the feed again: {}",
        chrome.eval("document.querySelector('main')?.dataset.feed")
    );
    site.push(4183, &card_event(4183));
    assert!(
        chrome.wait_for(CARD_CHANGED, Duration::from_secs(10)),
        "the page shown again missed the feed's next event"
    );
}

fn gzip_len(bytes: &[u8]) -> usize {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap().len()
}
