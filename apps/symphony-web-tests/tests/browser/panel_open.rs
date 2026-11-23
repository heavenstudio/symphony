//! The chat's and the activity list's first open. The sheet a click opens is drawn in the frame
//! after the click, loading; the panel's script, the chunks it imports and its styles are
//! requested together; once a page is idle the persistent modules' files are warmed into the HTTP
//! cache, outside the first visit's transfer, each once and none the page has; an open chat reads
//! its thread when the page's feed says it moved, never on a timer. On the board, whose page has
//! no media layer, the chat's own takes its thread's media up, from the files the chat came with,
//! and on the queue, Reports and Runs, which have none either, it scales the chat's mockup.

use super::{
    READY, SHOWN, assets::module, chrome, chrome::Chrome, completed_requests, dist, needed_bytes,
    prefetched_bytes, site, wait_for_board, wait_for_feed,
};
use serde_json::{Value, json};
use std::time::Duration;

/// Waits, pumping the page's events between looks, until `counted` is `count` or more, and says
/// it then: a count past `count` is said at once, for the caller's assertion. A quiet machine
/// reaches it within 10 s; a stall past the backstop ([`test_time::backstop`]) fails, saying
/// `what` and the count.
#[track_caller]
fn counted(browser: &mut Chrome, what: &str, count: usize, counted: impl Fn() -> usize) -> usize {
    let reached = test_time::until_true(
        format!("{what} reaching {count}"),
        test_time::backstop(Duration::from_secs(10)),
        || {
            counted() >= count || {
                browser.pump(Duration::from_millis(50));
                false
            }
        },
    );
    if let Err(stall) = reached {
        panic!("{stall}: it is {}", counted());
    }
    counted()
}

/// How many times the site was asked for `path`, once that count is `count` or more.
pub(super) fn asked(browser: &mut Chrome, site: &super::Site, path: &str, count: usize) -> usize {
    counted(browser, &format!("the asks for {path}"), count, || {
        site.requests()
            .iter()
            .filter(|asked| *asked == path)
            .count()
    })
}

/// How many times the open chat read thread 7, once that count is `count` or more.
fn chat_reads(browser: &mut Chrome, site: &super::Site, count: usize) -> usize {
    counted(browser, "the chat's reads of thread 7", count, || {
        site.targets()
            .iter()
            .filter(|target| target.starts_with("/api/ask?format=blocks&id=7"))
            .count()
    })
}

/// Click `opener`, then report what the next animation frame (the one painted after the click)
/// shows of `sheet`, and the words its status line said in the frames up to the third.
fn first_frame(browser: &mut Chrome, opener: &str, sheet: &str) -> Value {
    browser.eval(&format!(
        r#"new Promise(done => {{
            document.querySelector({opener}).click();
            const said = [];
            let first;
            const look = () => {{
                const sheet = document.querySelector({sheet});
                const status = sheet?.querySelector('[role=status]');
                said.push(status?.textContent ?? null);
                if (!first) {{
                    const box = sheet?.getBoundingClientRect();
                    first = {{
                        loading: sheet !== null && sheet.closest('#panel-loading') !== null,
                        busy: status?.closest('[aria-busy=true]') !== null,
                        drawn: !!box && box.width > 0 && box.height > 0 && getComputedStyle(sheet).visibility === 'visible',
                    }};
                }}
                if (said.length < 3) requestAnimationFrame(look); else done({{...first, said}});
            }};
            requestAnimationFrame(look);
        }})"#,
        opener = json!(opener),
        sheet = json!(sheet),
    ))
}

#[test]
fn a_click_draws_its_sheet_loading_in_the_next_frame_and_asks_for_the_panel_s_files_together() {
    for (opener, sheet, status, ready) in [
        (
            "#quick-open",
            "#quick-sheet",
            "Loading conversation…",
            "document.querySelector('#quick-text') !== null",
        ),
        (
            "#bell",
            "#activity-panel",
            "Loading activity…",
            "document.querySelector('#activity-panel [aria-label=\"Close activity\"]') !== null",
        ),
    ] {
        let site = site();
        let Some(mut browser) = chrome() else { return };
        // The first open fetches the panel; no idle prefetch has it in the cache already.
        browser.save_data();
        browser.open(&site.url("/"));
        wait_for_board(&mut browser);
        let panel = module(dist(), "panel");
        let fresh = not_asked(&site, &panel.imports);
        assert!(
            !fresh.is_empty(),
            "the page has every chunk the chat imports: {:?}",
            site.requests()
        );
        site.hold_route(&panel.script, true);
        // A live region announces what changes in it once it is in the page: the status is
        // drawn empty with the sheet, outside any busy container, and says its words a frame on.
        assert_eq!(
            first_frame(&mut browser, opener, sheet),
            json!({"loading": true, "busy": false, "drawn": true, "said": ["", status, status]}),
            "{opener} draws its sheet, loading, in the frame after the click"
        );
        let styles = json!(panel.styles);
        assert!(
            browser.wait_for(
                &format!("{styles}.every(href => document.querySelector(`link[rel=stylesheet][href=\"${{href}}\"]`))"),
                SHOWN
            ),
            "the panel's styles are requested while its script is still held"
        );
        assert_eq!(asked(&mut browser, &site, &panel.script, 1), 1);
        for &file in &fresh {
            assert_eq!(
                asked(&mut browser, &site, file, 1),
                1,
                "{file} is asked for beside the held script, not after it"
            );
        }
        site.hold_route(&panel.script, false);
        assert!(
            browser.wait_for(
                &format!(
                    "window.reviewPanelReady === true && {ready} && document.getElementById('panel-loading') === null"
                ),
                SHOWN
            ),
            "{opener}: the panel replaces its loading sheet: {}",
            browser.eval("document.body.innerHTML.slice(-3000)")
        );
        assert_eq!(
            browser.eval(&format!(
                "document.querySelectorAll({}).length",
                json!(sheet)
            )),
            json!(1),
            "one sheet once the panel is up"
        );
        for &file in &fresh {
            assert_eq!(
                asked(&mut browser, &site, file, 1),
                1,
                "the script's imports took the preloaded {file}, no second request"
            );
        }
    }
}

/// Those of `files` the site has not been asked for yet.
fn not_asked<'a>(site: &super::Site, files: &'a [String]) -> Vec<&'a String> {
    let asked = site.requests();
    files.iter().filter(|file| !asked.contains(file)).collect()
}

#[test]
fn closing_a_sheet_while_the_panel_loads_takes_its_loading_sheet_down() {
    let site = site();
    let Some(mut browser) = chrome() else { return };
    browser.save_data();
    browser.open(&site.url("/"));
    wait_for_board(&mut browser);
    let panel = module(dist(), "panel");
    site.hold_route(&panel.script, true);
    assert_eq!(
        first_frame(&mut browser, "#bell", "#activity-panel")["loading"],
        json!(true)
    );
    browser.eval("document.querySelector('#bell').click()");
    assert!(
        browser.wait_for(
            "document.getElementById('panel-loading') === null && document.getElementById('bell').getAttribute('aria-expanded') === 'false'",
            SHOWN
        ),
        "the bell closes its own loading sheet"
    );
    site.hold_route(&panel.script, false);
    assert!(browser.wait_for("window.reviewPanelReady === true", SHOWN));
    assert_eq!(
        browser.eval("document.getElementById('activity-panel') === null"),
        json!(true),
        "the panel that loads after the close stays closed"
    );
}

/// Every file the shell has asked the browser to warm, once its idle callbacks ran.
fn warmed(browser: &mut Chrome) -> Vec<String> {
    // An idle callback asked for after the shell's runs after it.
    browser.eval("new Promise(done => requestIdleCallback(done, {timeout: 10000}))");
    serde_json::from_value(browser.eval(
        "[...document.querySelectorAll('link[rel=prefetch]')].map(link => link.getAttribute('href'))",
    ))
    .unwrap()
}

#[test]
fn an_idle_page_warms_the_chat_and_the_menus_outside_its_first_visit_and_their_first_use_fetches_nothing_again()
 {
    let site = site();
    let Some(mut browser) = chrome() else { return };
    browser.open(&site.url("/"));
    wait_for_board(&mut browser);
    let (panel, overlay) = (module(dist(), "panel"), module(dist(), "overlay"));
    let (warmed, requests) = settled(&mut browser);
    // Each module's script, the chunks it imports and its styles, the chat's first: each once, and
    // none the page fetched itself (the shared sheet, the media layer's sheet home shows media
    // with, the chunks home imports).
    let page = |file: &String| {
        requests
            .iter()
            .any(|request| !request.prefetch && request.url.ends_with(file.as_str()))
    };
    let mut files: Vec<&String> = Vec::new();
    for module in [&panel, &overlay] {
        let all = std::iter::once(&module.script)
            .chain(&module.imports)
            .chain(&module.styles);
        for file in all {
            if !page(file) && !files.contains(&file) {
                files.push(file);
            }
        }
    }
    assert_eq!(
        warmed.iter().collect::<Vec<_>>(),
        files,
        "the warm-up is the chat's files and then the menus', each once and none the page has"
    );
    let media = panel
        .styles
        .iter()
        .find(|style| style.starts_with("/assets/media-layer-"))
        .expect("the chat's media layer's sheet");
    assert!(page(media), "home shows no media: {requests:#?}");
    for file in &files {
        assert!(
            requests
                .iter()
                .any(|request| request.url.ends_with(file.as_str()) && request.prefetch),
            "{file} was not fetched as a prefetch: {requests:#?}"
        );
    }
    let prefetched = prefetched_bytes(&requests);
    assert!(prefetched > 0.0);
    eprintln!(
        "first visit {} bytes; warmed at idle, outside it: {prefetched} bytes",
        needed_bytes(&requests)
    );
    for &file in &files {
        assert_eq!(
            asked(&mut browser, &site, file, 1),
            1,
            "the warm-up fetched {file} once"
        );
    }
    browser.eval("document.querySelector('#quick-open').click()");
    assert!(browser.wait_for(
        "window.reviewPanelReady === true && document.querySelector('#quick-text') !== null",
        SHOWN
    ));
    let chat = std::iter::once(&panel.script)
        .chain(&panel.imports)
        .chain(&panel.styles);
    for file in chat {
        assert_eq!(
            asked(&mut browser, &site, file, 1),
            1,
            "the first open took {file} from the cache"
        );
    }
    // The menus' first use, a picture shown in their viewer, takes their warmed files too.
    browser.eval(
        "window.reviewOverlay({kind: 'picture', owner: 'warmed', photos: [{src: '/first.svg', caption: 'First'}], at: 0, callback() {}})",
    );
    assert!(browser.wait_for(
        "window.reviewOverlayReady === true && document.querySelector('#media-lightbox[data-state=open]') !== null",
        SHOWN
    ));
    let menus = std::iter::once(&overlay.script)
        .chain(&overlay.imports)
        .chain(&overlay.styles);
    for file in menus {
        assert_eq!(
            asked(&mut browser, &site, file, 1),
            1,
            "the menus' first use took {file} from the cache"
        );
    }
}

/// Whether the page links each of `files` as a prefetch.
fn linked(browser: &mut Chrome, files: &[&String]) -> Vec<bool> {
    files
        .iter()
        .map(|file| {
            browser.eval(&format!(
                "document.querySelector(`link[rel=prefetch][href=\"${{{}}}\"]`) !== null",
                json!(file)
            )) == json!(true)
        })
        .collect()
}

#[test]
fn an_idle_page_warms_the_menus_only_once_the_chat_s_script_is_in() {
    let site = site();
    let Some(mut browser) = chrome() else { return };
    let (panel, overlay) = (module(dist(), "panel"), module(dist(), "overlay"));
    let own = panel.styles.last().expect("the chat's own sheet");
    site.hold_route(&panel.script, true);
    browser.open(&site.url("/"));
    wait_for_board(&mut browser);
    assert_eq!(
        asked(&mut browser, &site, &panel.script, 1),
        1,
        "the chat's script is warmed"
    );
    browser.pump(Duration::from_millis(500));
    assert_eq!(
        linked(&mut browser, &[&panel.script, own, &overlay.script]),
        [true, true, false],
        "the chat's files are warmed together, the menus' script waits for the chat's script"
    );
    assert_eq!(asked(&mut browser, &site, &overlay.script, 0), 0);
    site.hold_route(&panel.script, false);
    let (warmed, _) = settled(&mut browser);
    assert!(
        warmed.contains(&overlay.script),
        "the menus' script is not warmed: {warmed:?}"
    );
    assert_eq!(asked(&mut browser, &site, &overlay.script, 1), 1);
    assert_eq!(
        asked(&mut browser, &site, own, 1),
        1,
        "the chat's own sheet is warmed once"
    );
}

#[test]
fn a_click_before_the_idle_warm_up_holds_the_menus_until_the_chat_is_up() {
    let site = site();
    let Some(mut browser) = chrome() else { return };
    // The page's idle callbacks are kept until the test runs them, after the click.
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": "window.idle = []; window.requestIdleCallback = callback => idle.push(callback);"}),
    );
    let (panel, overlay) = (module(dist(), "panel"), module(dist(), "overlay"));
    site.hold_route(&panel.script, true);
    browser.open(&site.url("/"));
    wait_for_board(&mut browser);
    assert!(browser.wait_for(&format!("{READY} && idle.length > 0"), SHOWN));
    browser.eval("document.querySelector('#quick-open').click(); idle.splice(0).forEach(callback => callback())");
    browser.pump(Duration::from_millis(500));
    assert_eq!(
        browser.eval("document.querySelectorAll('link[rel=prefetch]').length"),
        json!(0),
        "the chat's own load is under way and the menus wait for it"
    );
    assert_eq!(asked(&mut browser, &site, &overlay.script, 0), 0);
    site.hold_route(&panel.script, false);
    assert!(browser.wait_for("window.reviewPanelReady === true", SHOWN));
    assert!(
        browser.wait_for(
            &format!(
                "document.querySelector(`link[rel=prefetch][href=\"${{{}}}\"]`) !== null",
                json!(overlay.script)
            ),
            SHOWN
        ),
        "the menus are warmed once the chat is up"
    );
    assert_eq!(asked(&mut browser, &site, &overlay.script, 1), 1);
    assert_eq!(
        asked(&mut browser, &site, &panel.script, 1),
        1,
        "the chat's script is fetched once, by its load, not again by a warm-up"
    );
}

const PREFETCHING: &str = "document.querySelector('link[rel=prefetch]') !== null";

#[test]
fn a_page_warms_on_a_fast_link_and_never_when_it_saves_data_or_is_on_2g() {
    let panel = module(dist(), "panel");
    for (save_data, link, warms) in [
        (false, "4g", true),
        (true, "4g", false),
        (false, "2g", false),
        (false, "slow-2g", false),
    ] {
        let site = site();
        let Some(mut browser) = chrome() else { return };
        browser.connection(save_data, link);
        // The page's idle callbacks are kept until the test runs them: a ready page has asked
        // for every one it will, so one that asked for none never warms.
        browser.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": super::HOLD_IDLE }),
        );
        browser.open(&site.url("/"));
        wait_for_board(&mut browser);
        assert!(
            browser.wait_for(READY, SHOWN),
            "saveData {save_data} on {link}: the page is ready"
        );
        let idle = browser.eval("idle.length").as_u64().unwrap_or(u64::MAX);
        assert_eq!(
            browser.eval(PREFETCHING),
            json!(false),
            "saveData {save_data} on {link} warms nothing before the browser is idle"
        );
        if warms {
            assert!(
                idle > 0,
                "a {link} link that keeps its data asks the idle browser for its warm-up"
            );
            browser.eval(super::RUN_IDLE);
            assert!(
                browser.wait_for(PREFETCHING, SHOWN),
                "a {link} link that keeps its data warms the chat"
            );
            assert_eq!(asked(&mut browser, &site, &panel.script, 1), 1);
        } else {
            assert_eq!(
                idle, 0,
                "saveData {save_data} on {link} asks the idle browser for nothing, a warm-up among it"
            );
            browser.eval(super::RUN_IDLE);
            assert_eq!(
                browser.eval(PREFETCHING),
                json!(false),
                "saveData {save_data} on {link} warms nothing"
            );
            assert_eq!(
                asked(&mut browser, &site, &panel.script, 0),
                0,
                "saveData {save_data} on {link}: nothing fetched the panel before its use"
            );
        }
    }
}

/// Open /tasks and wait for its page to be up and its board's columns, read last, asked for.
fn tasks_reading_columns(browser: &mut Chrome, site: &super::Site) {
    browser.open(&site.url("/tasks"));
    assert!(browser.wait_for(READY, SHOWN));
    asked(browser, site, "/api/task-board", 1);
}

#[test]
fn the_warm_up_waits_for_the_page_s_own_data() {
    let site = site();
    let Some(mut browser) = chrome() else { return };
    // The page is up and idle without its columns. A loaded machine can keep the test past the
    // read's 15 s limit: a read that ran out of time holds the warm-up as well, and the board
    // reads its columns again once its lost feed is back.
    site.hold_route("/api/task-board", true);
    tasks_reading_columns(&mut browser, &site);
    browser.pump(Duration::from_secs(2));
    assert_eq!(
        browser.eval(PREFETCHING),
        json!(false),
        "nothing is warmed while the page's own data is on its way"
    );
    site.hold_route("/api/task-board", false);
    site.drop_feeds();
    assert!(browser.wait_for(
        "document.querySelector('[data-slug=\"tools/example\"]') !== null",
        SHOWN
    ));
    assert!(
        browser.wait_for(PREFETCHING, SHOWN),
        "the warm-up follows the page's data"
    );
}

/// Holds the page's lookups in the device's cache (shell/cache.ts, `read`) until the test calls
/// `window.__answer()`, as a store slow to open and answer is on a loaded machine: set before the
/// shell takes the store's `read` as one of the page's reads. `window.__lookups` counts them.
const SLOW_STORE: &str = r"
let store, held = [];
window.__lookups = 0;
window.__answer = () => { const go = held; held = null; go.forEach(answer => answer()); };
Object.defineProperty(window, 'symphonyCache', {
    configurable: true,
    get: () => store,
    set(value) {
        const read = value.read;
        value.read = (...args) => {
            window.__lookups++;
            if (held === null) return read(...args);
            return new Promise(done => held.push(() => done(read(...args))));
        };
        store = value;
    },
});
";

#[test]
fn the_warm_up_waits_for_the_page_s_lookups_in_the_device_s_cache() {
    let site = site();
    let Some(mut browser) = chrome() else { return };
    // The board has answered and the page waits on what this device kept of it before it asks for
    // its counts and columns: no read of the server is in flight, and the page has no data yet.
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": SLOW_STORE }),
    );
    browser.open(&site.url("/tasks"));
    assert!(browser.wait_for(&format!("{READY} && window.__lookups > 0"), SHOWN));
    asked(&mut browser, &site, "/api/board", 1);
    browser.pump(Duration::from_secs(2));
    assert_eq!(
        browser.eval(PREFETCHING),
        json!(false),
        "nothing is warmed while the page waits on its lookup in the device's cache"
    );
    assert_eq!(
        asked(&mut browser, &site, "/api/task-board", 0),
        0,
        "the page asks for its counts only once its lookup answers"
    );
    browser.eval("window.__answer()");
    assert!(browser.wait_for(
        "document.querySelector('[data-slug=\"tools/example\"]') !== null",
        SHOWN
    ));
    assert!(
        browser.wait_for(PREFETCHING, SHOWN),
        "the warm-up follows the page's data"
    );
}

/// Wraps `fetch` before the shell does: the page's reads in flight are `window.__reads` (each
/// one's path), and `window.__ended` the path of each read that ended, in order, and whether it
/// was answered.
const READS: &str = r"
window.__reads = new Map(); window.__ended = [];
let read = 0;
const fetched = window.fetch;
window.fetch = (...args) => {
    const id = read++;
    const path = new URL(args[0] instanceof Request ? args[0].url : String(args[0]), location.href).pathname;
    window.__reads.set(id, path);
    return fetched(...args).then(
        response => (window.__ended.push([path, true]), response),
        error => { window.__ended.push([path, false]); throw error; },
    ).finally(() => window.__reads.delete(id));
};
";

/// Pumps the page's events until `expression` holds, failing past the browser target's backstop
/// with what `what` names and the page's reads.
#[track_caller]
fn reads_until(browser: &mut Chrome, what: &str, expression: &str) {
    let held = test_time::until_true(what, super::STALL, || {
        browser.eval(expression) == json!(true) || {
            browser.pump(Duration::from_millis(50));
            false
        }
    });
    if let Err(stall) = held {
        panic!(
            "{stall}: in flight {}, ended {}",
            browser.eval("JSON.stringify([...window.__reads.values()])"),
            browser.eval("JSON.stringify(window.__ended)")
        );
    }
}

#[test]
fn a_read_with_no_answer_holds_the_warm_up_until_one_answers() {
    let site = site();
    let Some(mut browser) = chrome() else { return };
    // The shell waits after a read that got no answer until any read answers. The columns are the
    // page's last reads, but a loaded machine can end another read after them, which rightly ends
    // the wait: so the columns' reads are held until every other read has ended, then dropped.
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": READS }),
    );
    site.drop_route("/api/task-board", true);
    site.hold_route("/api/task-board", true);
    tasks_reading_columns(&mut browser, &site);
    reads_until(
        &mut browser,
        "every read but the columns' ended",
        "[...window.__reads.values()].every(path => path.endsWith('/api/task-board'))",
    );
    site.hold_route("/api/task-board", false);
    reads_until(
        &mut browser,
        "the columns' reads dropped",
        "window.__reads.size === 0",
    );
    browser.pump(Duration::from_secs(2));
    assert_eq!(
        browser.eval(PREFETCHING),
        json!(false),
        "nothing is warmed after the page's read got no answer: the reads ended {}",
        browser.eval("JSON.stringify(window.__ended)")
    );
    // The board reads its columns again once its lost feed is back.
    site.drop_route("/api/task-board", false);
    site.drop_feeds();
    assert!(browser.wait_for(
        "document.querySelector('[data-slug=\"tools/example\"]') !== null",
        SHOWN
    ));
    assert!(
        browser.wait_for(PREFETCHING, SHOWN),
        "the warm-up follows the read that answers"
    );
}

#[test]
fn a_read_the_page_gave_up_holds_no_warm_up() {
    let site = site();
    let Some(mut browser) = chrome() else { return };
    // A read the page gives up, as a page does by its signal, ends after every other read has
    // answered: no read answers after it, so a warm-up that took it for a read with no answer
    // would wait for ever.
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": READS }),
    );
    site.hold_route("/api/given-up", true);
    site.hold_route("/api/task-board", true);
    tasks_reading_columns(&mut browser, &site);
    browser.eval(
        "window.__giveUp = new AbortController(); \
         fetch('/api/given-up', { signal: window.__giveUp.signal }).catch(() => {}); true",
    );
    site.hold_route("/api/task-board", false);
    assert!(browser.wait_for(
        "document.querySelector('[data-slug=\"tools/example\"]') !== null",
        SHOWN
    ));
    reads_until(
        &mut browser,
        "every read but the one to be given up ended",
        "[...window.__reads.values()].every(path => path === '/api/given-up')",
    );
    browser.eval("window.__giveUp.abort()");
    reads_until(
        &mut browser,
        "the read given up ended",
        "window.__reads.size === 0",
    );
    assert_eq!(
        browser.eval("JSON.stringify(window.__ended.at(-1))"),
        json!(r#"["/api/given-up",false]"#),
        "the read given up ended last"
    );
    assert!(
        browser.wait_for(PREFETCHING, SHOWN),
        "the warm-up waits on a read the page gave up: the reads ended {}",
        browser.eval("JSON.stringify(window.__ended)")
    );
}

/// A page at desktop width, where an open chat hides `+`, that keeps the data it is given and
/// whose panel's script is held, so a sheet the keyboard opens stays loading; `opener` focused.
fn desktop_loading(opener: &str) -> Option<(super::Site, Chrome, super::assets::Module)> {
    let site = site();
    let mut browser = chrome()?;
    browser.viewport(1280, 800, false);
    browser.save_data();
    browser.open(&site.url("/"));
    wait_for_board(&mut browser);
    let panel = module(dist(), "panel");
    site.hold_route(&panel.script, true);
    browser.eval(&format!(
        "document.querySelector({}).focus()",
        json!(opener)
    ));
    Some((site, browser, panel))
}

fn focused(selector: &str) -> String {
    format!(
        "document.activeElement?.matches({}) === true",
        json!(selector)
    )
}

#[test]
fn the_keyboard_opens_a_loading_sheet_into_its_focus_and_the_panel_takes_it_over() {
    for (opener, sheet, control) in [
        ("#quick-open", "#quick-sheet", "#quick-text"),
        (
            "#bell",
            "#activity-panel",
            "[aria-label=\"Close activity\"]",
        ),
    ] {
        let Some((site, mut browser, panel)) = desktop_loading(opener) else {
            return;
        };
        super::components::key(&mut browser, "Enter", "Enter", 13);
        assert!(
            browser.wait_for(&focused(&format!("#panel-loading {sheet}")), SHOWN),
            "{opener}: Enter puts the focus in the loading sheet, not on {}",
            browser.eval("document.activeElement?.outerHTML.slice(0, 200)")
        );
        if opener == "#quick-open" {
            assert_eq!(
                browser.eval("getComputedStyle(document.querySelector('#quick-open')).display"),
                json!("none"),
                "at this width the open chat hides +, which can keep no focus"
            );
        }
        site.hold_route(&panel.script, false);
        assert!(
            browser.wait_for(
                &format!(
                    "window.reviewPanelReady === true && document.getElementById('panel-loading') === null && {}",
                    focused(&format!("#quick-root {sheet} {control}"))
                ),
                SHOWN
            ),
            "{opener}: the panel's own sheet takes the focus over, not {}",
            browser.eval("document.activeElement?.outerHTML.slice(0, 200)")
        );
    }
}

#[test]
fn escape_closes_a_loading_sheet_and_gives_the_focus_back_to_its_opener() {
    for (opener, closed) in [
        (
            "#quick-open",
            "document.querySelector('#quick-root #quick-sheet')?.hidden !== false",
        ),
        (
            "#bell",
            "document.getElementById('activity-panel') === null",
        ),
    ] {
        let Some((site, mut browser, panel)) = desktop_loading(opener) else {
            return;
        };
        super::components::key(&mut browser, "Enter", "Enter", 13);
        assert!(browser.wait_for(&focused("#panel-loading aside"), SHOWN));
        super::components::key(&mut browser, "Escape", "Escape", 27);
        assert!(
            browser.wait_for(
                &format!(
                    "document.getElementById('panel-loading') === null && {} && document.querySelector({}).getAttribute('aria-expanded') === 'false'",
                    focused(opener),
                    json!(opener)
                ),
                SHOWN
            ),
            "{opener}: Escape closes the loading sheet and focuses its opener, not {}",
            browser.eval("document.activeElement?.outerHTML.slice(0, 200)")
        );
        site.hold_route(&panel.script, false);
        assert!(browser.wait_for("window.reviewPanelReady === true", SHOWN));
        assert_eq!(
            browser.eval(closed),
            json!(true),
            "{opener}: the panel that loads after Escape stays closed"
        );
    }
}

/// The chat's reply for ask 7, its messages saying `texts`.
pub(super) fn chat(texts: &[&str]) -> Value {
    let messages: Vec<_> = texts
        .iter()
        .enumerate()
        .map(|(seq, text)| {
            json!({"seq": seq + 1, "kind": if seq == 0 { "question" } else { "answer" }, "by": "owner",
                "text": text, "at": 1.0,
                "blocks": [{"type": "paragraph", "spans": [{"type": "text", "text": text}]}]})
        })
        .collect();
    json!({
        "ask": {"id": 7, "path": "", "anchor": "", "quote": "", "question": texts[0], "asked": 1.0,
            "answers": [], "status": "open", "updated": 1.0, "messages": messages},
        "recipient": "fixture"
    })
}

#[test]
fn an_open_chat_reads_its_thread_when_the_feed_says_it_moved_never_on_a_timer() {
    let site = site();
    site.set_json("/api/ask", &chat(&["First question"]));
    let Some(mut browser) = chrome() else { return };
    // The draft names thread 7; every timer of the chat's retry the page sets is held here, so a
    // read that answered and left one waiting shows. The page's news of the asks is kept from its
    // start.
    super::hold_ask_polls(&mut browser);
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": r#"localStorage.setItem('review-quick-draft', JSON.stringify({open: false, task: false, ask: 7}));
            window.asksNews = []; addEventListener('review:asks', event => asksNews.push(event.moved));"#}),
    );
    browser.open(&site.url("/"));
    wait_for_board(&mut browser);
    wait_for_feed(&site).expect("the page holds its feed open");
    assert!(
        browser.wait_for("asksNews.includes(true)", SHOWN),
        "an opened stream tells every reader to read once"
    );
    browser.eval("document.querySelector('#quick-open').click()");
    let said = |texts: &[&str]| {
        format!(
            "JSON.stringify([...document.querySelectorAll('#quick-sheet .quick-message p:not(.by)')].map(p => p.textContent)) === {}",
            json!(serde_json::to_string(texts).unwrap())
        )
    };
    assert!(
        browser.wait_for(&said(&["First question"]), SHOWN),
        "the chat opens on its thread: {}",
        browser.eval("document.getElementById('quick-sheet')?.innerText ?? null")
    );
    assert_eq!(chat_reads(&mut browser, &site, 1), 1);
    assert_eq!(
        browser.eval("askPoll.size"),
        json!(0),
        "a read that answered waits for the feed"
    );
    site.set_json("/api/ask", &chat(&["First question", "An answer"]));
    browser.eval("window.asksHeard = 0; addEventListener('review:asks', () => asksHeard++)");
    site.push(
        4183,
        r#"{"version":4183,"changes":[{"region":"asks","data":{"threads":[{"id":8,"path":"docs/OTHER.md"}]}}]}"#,
    );
    assert!(browser.wait_for("asksHeard === 1", SHOWN));
    site.push(
        4184,
        r#"{"version":4184,"changes":[{"region":"asks","data":{"threads":[{"id":7,"path":""}]}}]}"#,
    );
    assert!(
        browser.wait_for(&said(&["First question", "An answer"]), SHOWN),
        "the thread's move is read"
    );
    assert_eq!(
        chat_reads(&mut browser, &site, 2),
        2,
        "another thread's move read nothing"
    );
    site.push(
        4185,
        r#"{"version":4185,"changes":[{"region":"asks","data":{"recipient":"another"}}]}"#,
    );
    assert_eq!(
        chat_reads(&mut browser, &site, 3),
        3,
        "a new recipient is read once"
    );
    assert_eq!(browser.eval("askPoll.size"), json!(0));
}

/// The build's files the site was asked for.
fn build_files(site: &super::Site) -> Vec<String> {
    site.requests()
        .into_iter()
        .filter(|path| path.starts_with("/assets/"))
        .collect()
}

#[test]
fn the_board_s_chat_takes_its_thread_s_media_up_with_the_files_it_came_with() {
    let site = site();
    site.set_json("/api/ask", &chat(&["First question"]));
    site.set_file(
        "/mockup.html",
        b"<!doctype html><title>Mockup</title>".to_vec(),
        "text/html",
    );
    let Some(mut browser) = chrome() else { return };
    // Each file is fetched by what shows it: no idle prefetch has the chat's in already.
    browser.save_data();
    // The board's own page has no media layer: the chat's panel holds one there.
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": "localStorage.setItem('review-quick-draft', JSON.stringify({open: false, task: false, ask: 7}));"}),
    );
    browser.open(&site.url("/tasks"));
    assert!(
        browser.wait_for(
            "document.querySelector(\"[data-slug='tools/example']\") !== null",
            SHOWN
        ),
        "the board is never drawn"
    );
    wait_for_feed(&site).expect("the page holds its feed open");
    browser.eval("document.querySelector('#quick-open').click()");
    assert!(
        browser.wait_for(
            "[...document.querySelectorAll('#quick-sheet .quick-message p:not(.by)')].some(p => p.textContent === 'First question')",
            SHOWN
        ),
        "the chat opens on its thread: {}",
        browser.eval("document.getElementById('quick-sheet')?.innerText ?? null")
    );
    assert_eq!(chat_reads(&mut browser, &site, 1), 1);
    assert!(browser.wait_for("window.reviewPanelReady === true", SHOWN));
    let files = build_files(&site);
    // The thread moves on to a mockup, which the open chat reads and shows.
    let mut thread = chat(&["First question", "A mockup"]);
    thread["ask"]["messages"][1]["blocks"] =
        json!([{"type":"media","media":{"kind":"mockup","src":"/mockup.html","label":"Mockup"}}]);
    site.set_json("/api/ask", &thread);
    site.push(
        4184,
        r#"{"version":4184,"changes":[{"region":"asks","data":{"threads":[{"id":7,"path":""}]}}]}"#,
    );
    // The mockup shows once it is scrolled to in the thread, as a reader would.
    assert!(
        browser.wait_for(
            "document.querySelector('#quick-root .mockup-shell') !== null",
            SHOWN
        ),
        "the thread's move is never read: {}",
        browser.eval("document.getElementById('quick-sheet')?.innerText ?? null")
    );
    browser.eval(
        "document.querySelector('#quick-root .mockup-shell').scrollIntoView({block:'center'})",
    );
    assert!(
        browser.wait_for(
            "document.querySelector('#quick-root .mockup-frame iframe')?.getAttribute('style')?.includes('transform:scale(') === true",
            SHOWN
        ),
        "the media layer never scales the chat's mockup: {:?}; {:?}",
        browser.eval("document.querySelector('#quick-root .mockup-shell')?.outerHTML ?? null"),
        site.requests()
    );
    assert_eq!(
        build_files(&site),
        files,
        "the chat's media layer is in the files it came with: nothing more is fetched for it"
    );
}

#[test]
fn the_chat_s_mockup_is_scaled_on_the_pages_with_no_media_layer_of_their_own() {
    let site = site();
    let mut thread = chat(&["First question", "A mockup"]);
    thread["ask"]["messages"][1]["blocks"] =
        json!([{"type":"media","media":{"kind":"mockup","src":"/mockup.html","label":"Mockup"}}]);
    site.set_json("/api/ask", &thread);
    site.set_file(
        "/mockup.html",
        b"<!doctype html><title>Mockup</title>".to_vec(),
        "text/html",
    );
    let Some(mut browser) = chrome() else { return };
    // Each page opens on the chat's thread 7, its sheet closed until "+" is clicked.
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": "localStorage.setItem('review-quick-draft', JSON.stringify({open: false, task: false, ask: 7}));"}),
    );
    // No kind of these pages mounts a media layer: the chat's panel runs their views.
    for route in ["/queue", "/reports", "/runs"] {
        browser.open(&site.url(route));
        assert!(
            browser.wait_for(
                &format!("{READY} && document.querySelector('#quick-open') !== null"),
                SHOWN
            ),
            "{route}: the page is never drawn"
        );
        browser.eval("document.querySelector('#quick-open').click()");
        assert!(
            browser.wait_for(
                "document.querySelector('#quick-root .mockup-shell') !== null",
                SHOWN
            ),
            "{route}: the chat never draws its thread: {}",
            browser.eval("document.getElementById('quick-sheet')?.innerText ?? null")
        );
        // The mockup shows once it is scrolled to in the thread, as a reader would.
        browser.eval(
            "document.querySelector('#quick-root .mockup-shell').scrollIntoView({block:'center'})",
        );
        assert!(
            browser.wait_for(
                "document.querySelector('#quick-root .mockup-frame iframe')?.getAttribute('style')?.includes('transform:scale(') === true",
                SHOWN
            ),
            "{route}: no media layer scales the chat's mockup: {:?}",
            browser.eval("document.querySelector('#quick-root .mockup-shell')?.outerHTML ?? null"),
        );
    }
}

/// Wait for the page's idle warm-up to start (once its own reads are over; up to 10 s, for a
/// build that has none) and to end, every request it made, its prefetches included, finished,
/// and no further file linked after that (the menus' follow the chat's script): the files warmed,
/// and the requests.
pub(super) fn settled(browser: &mut Chrome) -> (Vec<String>, Vec<super::chrome::Request>) {
    browser.wait_for(PREFETCHING, Duration::from_secs(10));
    let mut files = warmed(browser);
    let settled = test_time::until("the warm-up settled", super::STALL, || {
        let requests = completed_requests(browser);
        if requests.iter().filter(|request| request.prefetch).count() < files.len() {
            return None;
        }
        // A finished prefetch's load event reaches the page a moment after the network.
        browser.pump(Duration::from_millis(100));
        let again = warmed(browser);
        if again.len() == files.len() {
            return Some(requests);
        }
        files = again;
        None
    });
    match settled {
        Ok(requests) => (files, requests),
        Err(stall) => panic!("{stall}: {files:?}"),
    }
}

/// Chrome DevTools' "Fast 3G" preset as it applies it: 562.5 ms of latency, 180,000 bytes a
/// second down and 84,375 up.
const FAST_3G: (f64, f64, f64) = (562.5, 180_000.0, 84_375.0);

/// Click "+" and time, from the click, the first frame showing a chat sheet and the first frame
/// whose composer can be typed in, both in milliseconds.
const TIME_FIRST_OPEN: &str = r#"new Promise(done => {
    const start = performance.now();
    let sheet = null;
    const look = () => {
        const now = performance.now() - start;
        if (sheet === null && document.querySelector('#quick-sheet')) sheet = now;
        const text = document.querySelector('#quick-text');
        if (window.reviewPanelReady === true && text && !text.disabled) { done({sheet, usable: now}); return; }
        requestAnimationFrame(look);
    };
    document.querySelector('#quick-open').click();
    requestAnimationFrame(look);
})"#;

#[test]
#[ignore = "a meter, run by hand: cold first opens of the chat, by link and moment, of this build or the one SYMPHONY_UI_DIST names"]
fn first_open_meter() {
    let runs: usize = std::env::var("SYMPHONY_PANEL_METER_RUNS")
        .ok()
        .and_then(|runs| runs.parse().ok())
        .unwrap_or(5);
    let load = || {
        std::process::Command::new("sysctl")
            .args(["-n", "vm.loadavg"])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .unwrap_or_default()
    };
    eprintln!("build {}; load before: {}", dist().display(), load());
    for (link, conditions) in [("loopback", None), ("Fast 3G", Some(FAST_3G))] {
        for moment in ["at once", "after idle"] {
            let (mut sheets, mut usable) = (Vec::new(), Vec::new());
            for _ in 0..runs {
                let site = site();
                let Some(mut browser) = chrome() else { return };
                if let Some((latency, down, up)) = conditions {
                    browser.call(
                        "Network.emulateNetworkConditions",
                        json!({"offline": false, "latency": latency, "downloadThroughput": down, "uploadThroughput": up}),
                    );
                }
                browser.open(&site.url("/"));
                wait_for_board(&mut browser);
                if moment == "after idle" {
                    settled(&mut browser);
                }
                let timing = browser.eval(TIME_FIRST_OPEN);
                sheets.push(timing["sheet"].as_f64().unwrap_or(f64::NAN).round());
                usable.push(timing["usable"].as_f64().unwrap().round());
            }
            let median = |values: &[f64]| {
                let mut sorted = values.to_vec();
                sorted.sort_by(f64::total_cmp);
                sorted[sorted.len() / 2]
            };
            eprintln!(
                "{link}, {moment}: sheet drawn {sheets:?} ms (median {}), composer usable {usable:?} ms (median {})",
                median(&sheets),
                median(&usable)
            );
        }
    }
    eprintln!("load after: {}", load());
}
