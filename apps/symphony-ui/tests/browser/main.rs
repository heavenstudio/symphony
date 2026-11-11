//! The board view in a real browser: the client's release build (apps/symphony-ui/bundle.sh),
//! served by a stand-in for the Symphony server (`site`), opened in headless Chrome (`chrome`).
//!
//! The budgets are the owner's (tools/python-zero/ui-board), and a miss is reported, never
//! loosened. They are absolute; the measurement they were set against is in
//! docs/engineering/analysis/python-zero.md ("Today's app, measured for mobile").
//! - a first visit loads at most 300 KB gzipped, everything the board needs (shell, JS glue,
//!   wasm, CSS, API data, the feed's head and messages), independently for `/` and `/tasks`.
//!   The feed's keep-alive pings, which keep arriving while the page is open, are reported
//!   beside the total. Deferred page kinds and the whole build are reported separately; a
//!   visit pays for the resources it actually loads;
//! - a return visit loads at most 20 KB (the build is cached by its hashed names);
//! - on an emulated mid phone over a tailnet link (CPU slowed 4x, 10 Mbit/s, 40 ms round trip)
//!   the board is interactive within 2 s of wall time, not charged at the machine's load: most
//!   of it is the network and the server, which a loaded machine does not slow, and a board
//!   answered late must fail it whatever the load (the wall is 0.4-0.5 s at load 8.5);
//! - one feed event changes one card in place, and the page fetches nothing for it.
//!
//! Without Chrome on the machine the browser tests say so and pass: the one capability a test
//! may lack. The build is made once per run of this binary, into `build/symphony-ui/` of the tree.

mod assets;
mod binaries;
mod board_status;
mod chrome;
#[path = "../common/mod.rs"]
mod common;
mod components;
#[path = "../../../../tools/test-support/script_std.rs"]
mod fixture_script;
mod look_parity;
mod notice_layout;
mod owner_controls;
mod owner_save;
mod phone;
mod projects;
mod restored;
mod run_records;
mod server_notices;
mod site;

use std::time::Duration;

use assets::dist;
use chrome::Chrome;
use serde_json::json;
use site::Site;

const FIRST_VISIT_BYTES: f64 = 300_000.0;
const RETURN_VISIT_BYTES: f64 = 20_000.0;
/// How long a page may take to show the board at all before a test gives up on it.
const SHOWN: Duration = Duration::from_secs(30);

fn site() -> Site {
    Site::start(dist(), &common::fixture("board.json"))
}

/// Chrome, or `None` when this machine has none (said once per test).
fn chrome() -> Option<Chrome> {
    match chrome::find() {
        Some(binary) => Some(Chrome::launch(&binary)),
        None => {
            eprintln!("no Chrome on this machine (SF_CHROME names one): browser test not run");
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
    // Each kind gets a fresh browser profile, including when kinds share CSS. A cached
    // sibling must not make an oversized entry pass. Keep that profile for the return visit.
    for (path, kind, ready, budgeted) in [
        ("/", "review", ".hero-title", true),
        ("/tasks", "board", "[data-slug='tools/example']", true),
        ("/agents", "tasks", ".agents", false),
        ("/docs/", "docs", "[data-ready=folder]", false),
        ("/queue", "queue", "[data-ready=queue]", false),
        ("/task/visual/in-progress", "tasks", "article.task", false),
        ("/docs/look-parity.md", "docs", ".doc-card", false),
    ] {
        let Some(mut chrome) = chrome() else { return };
        chrome.open(&site.url(path));
        assert!(
            chrome.wait_for(
                &format!("document.querySelector({}) !== null", json!(ready)),
                SHOWN
            ),
            "{path} did not show {ready}: {:?}",
            chrome.eval("document.body.innerText")
        );
        let requests = completed_requests(&mut chrome);
        assert!(
            requests.iter().any(|r| r.kind == "Script"
                && r.url.contains(&format!("/assets/{kind}-"))
                && r.status == 200
                && !r.from_cache),
            "{path} did not fetch its {kind} module: {requests:#?}"
        );
        assert!(
            requests
                .iter()
                .any(|r| r.url.ends_with(".wasm") && r.bytes > 0.0 && !r.from_cache),
            "{path} fetched no WASM: {requests:#?}"
        );
        let bytes = needed_bytes(&requests);
        eprintln!(
            "{path} first visit: {bytes} bytes; the feed's keep-alive pings beside it: {} bytes\n{requests:#?}",
            ping_bytes(&requests)
        );
        chrome.open("about:blank");
        chrome.open(&site.url(path));
        assert!(chrome.wait_for(
            &format!("document.querySelector({}) !== null", json!(ready)),
            SHOWN
        ));
        let returned = completed_requests(&mut chrome);
        let again = needed_bytes(&returned);
        eprintln!(
            "{path} return visit: {again} bytes; keep-alive pings beside it: {} bytes",
            ping_bytes(&returned)
        );
        if budgeted {
            entries.push((path, bytes, again));
        }
    }
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    let before = needed_bytes(&completed_requests(&mut chrome));
    chrome.eval("document.querySelector('#quick-open').click()");
    assert!(
        chrome.wait_for("window.reviewPanelReady === true", SHOWN),
        "the deferred panel never loaded"
    );
    let with_panel = needed_bytes(&completed_requests(&mut chrome));
    eprintln!(
        "deferred panel: {} additional bytes; home with panel: {with_panel} bytes",
        with_panel - before
    );
    for (path, first, again) in entries {
        assert!(
            first <= FIRST_VISIT_BYTES,
            "{path} first visit loads {first} bytes, over {FIRST_VISIT_BYTES}"
        );
        assert!(
            again <= RETURN_VISIT_BYTES,
            "{path} return visit loads {again} bytes, over {RETURN_VISIT_BYTES}"
        );
    }
}

/// Include styles and API replies still in flight when the view first appeared. A live feed
/// intentionally never completes; its head and messages still count in `needed_bytes`, all but
/// its keep-alive pings.
fn completed_requests(chrome: &mut Chrome) -> Vec<chrome::Request> {
    let started = std::time::Instant::now();
    loop {
        let requests = chrome.requests();
        if requests
            .iter()
            .all(|r| r.finished || r.kind == "EventSource" || r.kind == "Image")
        {
            return requests;
        }
        assert!(
            started.elapsed() < SHOWN,
            "resources never finished: {requests:#?}"
        );
    }
}

#[test]
fn a_first_visit_and_a_return_visit_fit_their_budgets() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    let url = site.url("/");
    chrome.open(&url);
    wait_for_board(&mut chrome);
    let first = chrome.requests();
    let first_bytes = needed_bytes(&first);
    chrome.open("about:blank");
    chrome.open(&url);
    wait_for_board(&mut chrome);
    let again = chrome.requests();
    let again_bytes = needed_bytes(&again);
    eprintln!(
        "first visit: {first_bytes} bytes; return visit: {again_bytes} bytes; keep-alive pings beside them: {} and {} bytes\n{first:#?}\n{again:#?}",
        ping_bytes(&first),
        ping_bytes(&again)
    );
    assert!(
        first_bytes <= FIRST_VISIT_BYTES,
        "a first visit loads {first_bytes} bytes, over {FIRST_VISIT_BYTES}: {first:#?}"
    );
    assert!(
        again_bytes <= RETURN_VISIT_BYTES,
        "a return visit loads {again_bytes} bytes, over {RETURN_VISIT_BYTES}: {again:#?}"
    );
}

/// What the board needed over the wire: every request but pictures, which are the cards'
/// content, not the app, and the feed's keep-alive pings, which arrive every `ping_ms` for as
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
        .filter(|request| request.kind != "Image" && !request.from_cache)
}

fn pings_of(request: &chrome::Request) -> f64 {
    (request.pings * site::ping().len()) as f64
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
        "location.pathname === '/tasks' && document.documentElement.dataset.navigation === 'ready' && document.querySelector('.kanban') !== null",
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
    assert!(chrome.wait_for(
        "document.documentElement.dataset.navigation === 'ready'",
        SHOWN
    ));
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
            "location.pathname === '/tasks' && document.documentElement.dataset.navigation === 'ready' && document.querySelector('.kanban') !== null",
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
    for destination in ["/task/tools/second", "/agents"] {
        let Some(mut chrome) = chrome() else { return };
        chrome.call("Profiler.enable", json!({}));
        chrome.call(
            "Profiler.startPreciseCoverage",
            json!({"callCount":true,"detailed":true}),
        );
        chrome.open(&site.url("/task/tools/example"));
        assert!(chrome.wait_for(
            "document.documentElement.dataset.navigation === 'ready' && document.querySelector('.task[data-slug=\"tools/example\"]') !== null",
            SHOWN
        ));
        chrome.eval(
            r#"
            const mount = document.getElementById('review-root-tasks');
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
        assert!(chrome.wait_for(
            "document.documentElement.dataset.navigation === 'ready'",
            SHOWN
        ));
        assert_eq!(chrome.eval("window.beforeClamp"), json!(5000));
        assert_eq!(
            chrome.eval("afterClamp > 0 && afterClamp < beforeClamp"),
            json!(true),
            "{destination}: navigation must shrink the page after the shell's baseline"
        );
        assert_eq!(chrome.eval("scrollY"), json!(0), "{destination}");
        // Keep the fixture honest: the second route must render its own page, rather than
        // passing the scroll check with a missing API response or the first task still shown.
        let selector = if destination == "/agents" {
            ".agents"
        } else {
            ".task[data-slug=\"tools/second\"]"
        };
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
        assert!(chrome.wait_for(
            &format!("document.documentElement.dataset.navigation === 'ready' && document.querySelector('[data-ready={ready}]') !== null"),
            SHOWN
        ), "{path}: {}", chrome.eval("document.body.innerText"));
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
    assert!(chrome.wait_for(
        "document.documentElement.dataset.navigation === 'ready'",
        SHOWN
    ));
    chrome.eval("document.body.style.minHeight = '30000px'; scrollTo(0,5000)");
    assert_eq!(chrome.eval("scrollY"), json!(5000));
    chrome.eval("document.querySelector('header a[href=\"/tasks\"]').click(); window.oldPageShown = !document.getElementById('review-root-review').hidden; scrollTo(0,4000); window.oldPageScroll = scrollY");
    assert_eq!(chrome.eval("window.oldPageShown"), json!(true));
    assert_eq!(chrome.eval("window.oldPageScroll"), json!(4000));
    assert!(chrome.wait_for(
        "location.pathname === '/tasks' && document.documentElement.dataset.navigation === 'ready' && document.querySelector('.kanban') !== null",
        SHOWN
    ));
    assert_eq!(chrome.eval("scrollY"), json!(0));
    chrome.eval("scrollTo(0,5000)");
    assert_eq!(chrome.eval("scrollY"), json!(5000));
    chrome.eval("history.back()");
    assert!(chrome.wait_for(
        "location.pathname === '/' && document.documentElement.dataset.navigation === 'ready' && document.querySelector('.hero-title') !== null",
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
    assert!(chrome.wait_for(
        "document.documentElement.dataset.navigation === 'ready'",
        SHOWN
    ));
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
fn task_controls_keep_their_values_and_revealed_rows_after_scrolling() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.call("Page.addScriptToEvaluateOnNewDocument", json!({"source":
        "const observer = new MutationObserver(() => { if (document.head) { const style = document.createElement('style'); style.textContent = '.column { margin-top: 20000px }'; document.head.append(style); observer.disconnect(); } }); observer.observe(document, {childList: true, subtree: true});"
    }));
    chrome.open(&site.url("/tasks"));
    assert!(chrome.wait_for("document.querySelector('.kanban') !== null", SHOWN));
    assert!(chrome.wait_for(
        "document.documentElement.dataset.navigation === 'ready'",
        SHOWN
    ));
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
    // The pipeline's columns live in the board's own bounded scroller, which clips them from
    // the page: the pushed column is reached by scrolling that scroller, never the window.
    chrome.eval("document.querySelector('.board-scroll').scrollTo(0, 20000)");
    assert!(
        chrome.wait_for(
            "document.querySelector('[data-slug=\"tools/example\"]') !== null",
            SHOWN
        ),
        "the scrolled-to row never appeared: {:?}; {:?}",
        site.targets(),
        chrome.eval("(b => ({scrollTop: b.scrollTop, scrollHeight: b.scrollHeight, clientHeight: b.clientHeight}))(document.querySelector('.board-scroll'))")
    );
    chrome.eval("document.querySelector('.board-scroll').scrollTo(0, 0)");
    assert!(chrome.wait_for(
        "document.querySelector('.board-scroll').scrollTop === 0",
        SHOWN
    ));
    assert_eq!(
        chrome.eval("document.querySelectorAll('[data-slug=\"tools/example\"]').length"),
        json!(1)
    );
    // The real control is deliberately not focused. Read its node, not activeElement, and
    // preserve the latest synchronous edit when the request's debounce runs later.
    chrome.eval("const decoy = document.createElement('input'); decoy.value = 'wrong'; document.body.prepend(decoy); decoy.focus(); const input = document.querySelector('input[type=search]'); input.value = 'alpha'; input.dispatchEvent(new Event('input', {bubbles:true})); input.value = 'beta'; input.dispatchEvent(new Event('input', {bubbles:true}));");
    assert!(chrome.wait_for(
        "document.querySelector('input[type=search]').value === 'beta'",
        SHOWN
    ));
    let started = std::time::Instant::now();
    while !site
        .targets()
        .iter()
        .any(|url| url.contains("counts=1&q=beta&days=1"))
    {
        assert!(
            started.elapsed() < SHOWN,
            "the search did not fetch beta: {:?}",
            site.targets()
        );
        chrome.pump(Duration::from_millis(25));
    }
    components::choose(
        &mut chrome,
        ".filters button[aria-label='Finished tasks']",
        "Last week",
    );
    let started = std::time::Instant::now();
    while !site
        .targets()
        .iter()
        .any(|url| url.contains("counts=1&q=beta&days=7"))
    {
        assert!(
            started.elapsed() < SHOWN,
            "the window selector lost the search: {:?}",
            site.targets()
        );
        chrome.pump(Duration::from_millis(25));
    }
    assert!(!site.targets().iter().any(|url| url.contains("q=wrong")));
    // A new filter replaces rows, so its new visibility latches must be approached again.
    // The earlier assertion checked that scrolling alone preserves the revealed row.
    chrome.eval("document.querySelector('.board-scroll').scrollTo(0, 20000)");
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
                "status": "blocked", "priority": 1, "parent": null, "created_by": "owner",
                "created": 0.0, "updated": 0.0, "links": [], "depends": [], "roles": [],
                "instruction": false}],
            "statuses": ["blocked"], "counts": [["blocked", 1]], "signoff": [], "next": null
        }),
    );
    let Some(mut chrome) = chrome() else { return };
    chrome.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":1280,"height":800,"deviceScaleFactor":1,"mobile":false}),
    );
    chrome.call("Page.addScriptToEvaluateOnNewDocument", json!({"source":
        "const observer = new MutationObserver(() => { if (document.head) { const style = document.createElement('style'); style.textContent = '.offline { margin-top: 20000px }'; document.head.append(style); observer.disconnect(); } }); observer.observe(document, {childList: true, subtree: true});"
    }));
    chrome.open(&site.url("/tasks"));
    assert!(chrome.wait_for("document.documentElement.dataset.navigation === 'ready' && document.querySelector('#column-blocked') !== null", SHOWN));
    assert!(
        !site
            .targets()
            .iter()
            .any(|url| url.contains("status=blocked"))
    );
    // The offline columns scroll with the page. Stop with the column's slot 300 px below the
    // screen: its own look-ahead reveals it, and its row's look-ahead, which reaches 300 px
    // into the screen, must then reveal the card while the column is still off screen.
    chrome.eval("const column = document.querySelector('#column-blocked'); scrollTo(0, column.getBoundingClientRect().top + scrollY - innerHeight - 300)");
    assert!(
        chrome.wait_for(
            "document.querySelector('[data-slug=\"tools/blocked-example\"]') !== null",
            SHOWN
        ),
        "the row's look-ahead was clipped: {}",
        chrome.eval("JSON.stringify({innerHeight, column: document.querySelector('#column-blocked').getBoundingClientRect().top, row: document.querySelector('[id=\"row-tools/blocked-example\"]')?.outerHTML})")
    );
    assert_eq!(
        chrome.eval("document.querySelector('#column-blocked').getBoundingClientRect().top >= innerHeight + 299"),
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
    assert!(wait_for_feed(&site));
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
    chrome.eval("Array.from(document.querySelectorAll('button')).find(e => e.textContent === 'Subtasks').click()");
    assert!(
        chrome.wait_for(
            "document.querySelector('ul li a')?.textContent === 'Example task'",
            SHOWN
        ),
        "the fallback left task controls unusable: {:?}",
        chrome.eval("document.body.innerText")
    );
    assert!(site.targets().iter().any(|url| url.contains("roots=1")));
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
fn the_transfer_budget_counts_api_failures_and_open_streams_but_not_images_cache_or_pings() {
    let requests = [
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
    assert_eq!(site::ping().len(), 19);
    assert_eq!(needed_bytes(&requests), 222.0);
    assert_eq!(ping_bytes(&requests), 38.0);
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

/// Wait for the page to hold a feed stream open, up to 10 s; whether it does.
fn wait_for_feed(site: &Site) -> bool {
    for _ in 0..200 {
        if site.feeds() > 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
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

/// The longest a stream that falls silent but stays open may hold: the owner reads the board from
/// a phone over the tailnet, where that happens on every network change (ruled on
/// tools/python-zero/ui-board).
const SILENCE_GIVEN_UP: Duration = Duration::from_millis(12_500);

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
    wait_for_feed(&site);
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
    assert!(wait_for_feed(&site), "the page never followed the feed");
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
        ) && wait_for_feed(&site),
        "the page never followed the feed again after {} feed requests",
        site.feed_requests()
    );
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
    assert!(wait_for_feed(&site), "the page never followed the feed");
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
        ) && wait_for_feed(&site),
        "the page never followed the restarted server's feed"
    );
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
    assert!(wait_for_feed(&site), "the page never followed the feed");
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
        ) && wait_for_feed(&site),
        "the page never followed the feed again"
    );
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
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    assert!(wait_for_feed(&site), "the page never followed the feed");
    record_stale(&mut chrome);
    // The server pings as its board says: the stream holds past the longest silence allowed.
    std::thread::sleep(SILENCE_GIVEN_UP + Duration::from_secs(2));
    assert_eq!(site.feed_requests(), 1, "a stream that pings was given up");
    assert!(!stale_seen(&mut chrome), "a stream that pings showed stale");
    // The server falls silent with the socket open, its board moved meanwhile: given up within
    // the bound, the page waits its first second and shows the board fetched again.
    site.fall_silent();
    site.set_board(&board_at(4190, 99, PING_MS));
    let healed = SILENCE_GIVEN_UP + Duration::from_secs(5);
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
    assert!(wait_for_feed(&site), "the page never followed the feed");
    chrome.eval("window.__kept = true");
    let shells = |site: &Site| site.requests().iter().filter(|path| *path == "/").count();
    assert_eq!(shells(&site), 1);
    // The server is deployed again with another client build, and restarts.
    site.deploy("/assets/symphony-shell-0000000000000000.js");
    site.drop_feeds();
    let reloaded = (0..300).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        shells(&site) >= 2
    });
    assert!(
        reloaded,
        "a client healed onto another build never loaded the page again: {:?}",
        site.requests()
    );
    // This stand-in still serves the old files, so the page loaded again is not the build the
    // board names either: it loads no more.
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
    assert!(wait_for_feed(&site), "the page never followed the feed");
    site.end_feeds_on_open();
    let before = site.board_requests();
    let feeds_before = site.feed_requests();
    site.drop_feeds();
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

fn gzip_len(bytes: &[u8]) -> usize {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap().len()
}
