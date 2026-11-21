//! Home, the task board and the queue drawn from this device's cache on a return visit
//! (docs/analysis/client-cache.md, "A return visit, step by step" and "A stale draw"): with the
//! server's reads held back, the page draws what it kept before any answer, marked as possibly out
//! of date and its actions waiting, then asks whether each kept read changed, a `304` keeping the
//! copy and a `200` replacing it on the page and on the device. A store of another format, or a
//! value this client cannot read, draws nothing; a server that does not answer leaves the copy
//! drawn, saying so. A page whose feed reconnects holds its actions by the same rule. The release
//! build, on the private stand-in (`site`).

use std::time::Duration;

use serde_json::{Value, json};

use crate::chrome::Chrome;
use crate::site::{Answered, Site};
use crate::{SHOWN, STALL, common};

/// The reads a return visit checks, which the stand-in holds back to show what the page draws
/// before any answer.
const READS: [&str; 4] = [
    "/api/board",
    "/api/task-board",
    "/api/queue",
    "/api/machine-work",
];

/// This device's kept parts, `{ "ITEM PART": FIELD }` (`board:waiting counts`), each record's `FIELD`:
/// the store's records, read beside it, their keys' names read back without the project's.
macro_rules! kept_parts {
    ($field:literal) => {
        concat!(
            r#"(async () => {
    const db = await new Promise(resolve => {
        const request = indexedDB.open("symphony-cache");
        // No store yet: make none.
        request.onupgradeneeded = () => request.transaction.abort();
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => resolve(null);
    });
    if (!db) return {};
    const all = await new Promise(resolve => {
        const request = db.transaction("parts").objectStore("parts").getAll();
        request.onsuccess = () => resolve(request.result);
    });
    db.close();
    const names = key => {
        const names = [];
        for (let at = 0; at < key.length;) {
            const colon = key.indexOf(":", at), end = colon + 1 + Number(key.slice(at, colon));
            names.push(key.slice(colon + 1, end));
            at = end;
        }
        return names.slice(1).join(" ");
    };
    return Object.fromEntries(all.map(part => [names(part.key), part."#,
            $field,
            "]));\n})()"
        )
    };
}

/// When each part was kept, `{ "ITEM PART": saved }`.
pub(super) const PARTS: &str = kept_parts!("saved");
/// What each part holds, `{ "ITEM PART": value }`, as the page gave it to the store.
pub(super) const VALUES: &str = kept_parts!("value");

/// `work(stores)` in one read-write transaction over the store's records, beside the page's own
/// connection: what a client of another format, or of another shape, left there.
fn rewrite(chrome: &mut Chrome, work: &str) {
    let done = chrome.eval(&format!(
        r#"(async () => {{
            const db = await new Promise(resolve => {{
                const request = indexedDB.open("symphony-cache");
                request.onsuccess = () => resolve(request.result);
            }});
            await new Promise(resolve => {{
                const transaction = db.transaction(["meta", "parts"], "readwrite");
                const stores = {{ meta: transaction.objectStore("meta"), parts: transaction.objectStore("parts") }};
                transaction.oncomplete = resolve;
                ({work})(stores);
            }});
            db.close();
            return true;
        }})()"#
    ));
    assert_eq!(done, json!(true));
}

/// A page of the board's: its path, what shows it drawn, and the parts it draws from the device.
struct Page {
    path: &'static str,
    drawn: &'static str,
    parts: &'static [&'static str],
}

const HOME: Page = Page {
    path: "/",
    drawn: "[data-ready=board] .hero-title",
    parts: &["board: board"],
};
const TASKS: Page = Page {
    path: "/tasks",
    drawn: "[data-slug='tools/example']",
    parts: &[
        "board:waiting counts",
        "board:waiting work",
        "board:waiting column:ready",
    ],
};
const QUEUE: Page = Page {
    path: "/queue",
    drawn: "[data-ready=queue] [data-card] .postpone-controls button",
    parts: &["board: board", "board: queue"],
};

/// The stand-in, with a card waiting in the queue: its Later button is the queue's action.
fn site() -> Site {
    let site = crate::site();
    // Cache assertions name the Ready column explicitly.
    site.set_json("/api/task-board", &crate::site::example_listing("ready"));
    let board: Value = serde_json::from_str(&common::fixture("board.json")).unwrap();
    site.set_json("/api/queue", &queue(&board["next"][0]));
    site
}

fn queue(card: &Value) -> Value {
    json!({"waiting": [card], "spot_check": [], "agent": [], "branches": []})
}

/// `expression`, which must come true within [`SHOWN`]; the page's text when it does not.
#[track_caller]
pub(super) fn shows(chrome: &mut Chrome, expression: &str, what: &str) {
    if !chrome.wait_for(expression, SHOWN) {
        let text = chrome.eval("document.body ? document.body.innerText.slice(0, 2000) : ''");
        panic!("{what}: never {expression}; the page shows {text}");
    }
}

pub(super) fn kept(chrome: &mut Chrome) -> serde_json::Map<String, Value> {
    match chrome.eval(PARTS) {
        Value::Object(parts) => parts,
        other => panic!("the store's parts: {other}"),
    }
}

/// Wait until this device keeps each of `parts`.
#[track_caller]
pub(super) fn keeps(chrome: &mut Chrome, parts: &[&str]) {
    shows(
        chrome,
        &format!(
            "{PARTS}.then(kept => {}.every(part => part in kept))",
            json!(parts)
        ),
        "the visit's reads were never kept",
    );
}

/// This origin's store opened before a page's first visit, in a page of the site's without the
/// client (`/api/pending`, a read no test here counts): the store keeps a scratch item, asked again
/// until it answers, and forgets it. A fresh profile's first open makes the database against the
/// store's 500 ms deadline, which a loaded machine can miss, and the page then keeps nothing, as
/// it should (`shell/cache.ts`); with the database made, the page's own open makes nothing.
pub(super) fn store_opened(chrome: &mut Chrome, site: &Site) {
    chrome.open(&site.url("/api/pending"));
    // The store as the shell carries it, its types stripped.
    let store = crate::assets::store();
    chrome.eval(&format!("(async () => {{ {store}\n return null; }})()"));
    let format = symphony_api::CACHE_FORMAT;
    let keep = format!(
        r#"window.symphonyCache.keep({format}, "warm", "task:warm", "task", "page", "", {{}}, undefined, false)"#
    );
    // A quiet machine keeps it within seconds; only a stall reaches the browser target's backstop
    // (RULES.md `tests/no-test-fails-on-load`).
    let opened = test_time::until_true(
        "this origin's store keeping a scratch item",
        crate::STALL,
        || chrome.eval(&keep) == json!(true),
    );
    if let Err(stall) = opened {
        panic!("the store never opened its database: {stall}");
    }
    assert_eq!(
        chrome.eval(&format!(
            r#"window.symphonyCache.forget({format}, "warm", "task:warm")"#
        )),
        json!(true),
        "the store forgets the scratch item it kept"
    );
}

/// A first visit to `page`: drawn from the server, live, and its reads kept.
fn visit(chrome: &mut Chrome, site: &Site, page: &Page) {
    chrome.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":1440,"height":900,"deviceScaleFactor":1,"mobile":false}),
    );
    store_opened(chrome, site);
    chrome.open(&site.url(page.path));
    if page.drawn.starts_with("[data-status=build]") {
        shows(
            chrome,
            "document.querySelector('.column[data-status=build]') !== null",
            "Build column",
        );
        // Keep the initial lifecycle reads before scrolling to machine activity.
        let lifecycle: Vec<_> = page
            .parts
            .iter()
            .copied()
            .filter(|part| *part != "board:waiting column:build")
            .collect();
        keeps(chrome, &lifecycle);
        chrome.eval("document.querySelector('.column[data-status=build]').scrollIntoView({block:'nearest',inline:'center'})");
    }
    shows(chrome, &present(page.drawn), page.path);
    shows(chrome, LIVE, page.path);
    keeps(chrome, page.parts);
}

pub(super) fn present(selector: &str) -> String {
    format!("document.querySelector({}) !== null", json!(selector))
}

/// The page is current: its feed open, nothing shown from the device.
pub(super) const LIVE: &str = "(main => main?.dataset.feed === 'live' && !main.dataset.cache && !document.querySelector('.device-notice'))(document.querySelector('main'))";

/// What the notice says of a page drawn from copies the oldest of which was kept at `saved`, as
/// the browser's clock says the time.
pub(super) fn notice(chrome: &mut Chrome, saved: f64, unreachable: bool) -> String {
    let clock = chrome.eval(&format!(
        "(d => String(d.getHours()).padStart(2, '0') + ':' + String(d.getMinutes()).padStart(2, '0'))(new Date({saved}))"
    ));
    let clock = clock.as_str().unwrap();
    if unreachable {
        format!("Not reachable: shown from this device, saved {clock}")
    } else {
        format!("Shown from this device, saved {clock}. Checking for changes…")
    }
}

/// When the oldest of what `page` draws from the device was kept: its parts, and the task board's
/// every kept column.
fn oldest(chrome: &mut Chrome, page: &Page) -> f64 {
    kept(chrome)
        .iter()
        .filter(|(part, _)| {
            page.parts.contains(&part.as_str())
                || page.path == TASKS.path && part.starts_with("board:waiting column:")
        })
        .map(|(_, saved)| saved.as_f64().unwrap())
        .fold(f64::INFINITY, f64::min)
}

/// The page shows what this device kept, saying so, and its actions wait.
#[track_caller]
fn drawn_from_the_device(chrome: &mut Chrome, page: &Page, said: &str) {
    shows(chrome, &present(page.drawn), page.path);
    shows(
        chrome,
        &format!(
            "(main => main.dataset.feed === 'stale' && main.dataset.cache === 'device' && document.querySelector('.page-notices .device-notice')?.textContent === {})(document.querySelector('main'))",
            json!(said)
        ),
        page.path,
    );
    if page.path == QUEUE.path {
        assert_eq!(
            chrome.eval(&format!(
                "(b => [b.textContent, b.disabled])(document.querySelector({}))",
                json!(page.drawn)
            )),
            json!(["Checking…", true]),
            "the Later button waits"
        );
    }
}

/// How many answers the stand-in has sent to each of [`READS`].
fn answered(site: &Site) -> Vec<usize> {
    READS.iter().map(|read| site.answers(read).len()).collect()
}

/// The answers to `read` since `before` ([`answered`]).
fn since(site: &Site, before: &[usize], read: &str) -> Vec<Answered> {
    let at = READS.iter().position(|each| *each == read).unwrap();
    site.answers(read)[before[at]..].to_vec()
}

fn hold(site: &Site, held: bool) {
    for read in READS {
        site.hold_route(read, held);
    }
}

/// Open `page` again with every read held back; the answers counted before it.
fn return_held(chrome: &mut Chrome, site: &Site, page: &Page) -> Vec<usize> {
    let before = answered(site);
    hold(site, true);
    chrome.open("about:blank");
    chrome.open(&site.url(page.path));
    before
}

/// Each read of `answers` named a tag, was answered `304`, and asked once.
#[track_caller]
fn each_unchanged_once(answers: &[Answered], what: &str) {
    assert!(!answers.is_empty(), "{what}: no read");
    for (at, answer) in answers.iter().enumerate() {
        assert!(
            answer
                .named
                .as_deref()
                .is_some_and(|tag| tag.starts_with("W/\"")),
            "{what}: {answer:?} names the kept copy's tag"
        );
        assert_eq!(answer.status, 304, "{what}: {answer:?}");
        assert!(
            answers[at + 1..]
                .iter()
                .all(|other| other.target != answer.target),
            "{what}: {} asked twice: {answers:#?}",
            answer.target
        );
    }
}

#[test]
fn a_return_visit_draws_what_this_device_kept_before_any_answer_and_checks_it_with_one_304_each() {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    for page in [HOME, TASKS, QUEUE] {
        visit(&mut chrome, &site, &page);
        let saved = oldest(&mut chrome, &page);
        let before = return_held(&mut chrome, &site, &page);
        let said = notice(&mut chrome, saved, false);
        drawn_from_the_device(&mut chrome, &page, &said);
        assert_eq!(
            answered(&site),
            before,
            "{}: drawn before any read was answered",
            page.path
        );
        hold(&site, false);
        shows(&mut chrome, LIVE, page.path);
        if page.path == QUEUE.path {
            shows(
                &mut chrome,
                &format!(
                    "(b => b.textContent === 'Later' && !b.disabled)(document.querySelector({}))",
                    json!(page.drawn)
                ),
                "the Later button, once current",
            );
        }
        // The board is read fresh, its version starting the feed; every other kept read is asked
        // whether it changed, once.
        let board = since(&site, &before, "/api/board");
        assert_eq!(
            board
                .iter()
                .map(|answer| (answer.named.is_some(), answer.status))
                .collect::<Vec<_>>(),
            [(false, 200)],
            "{}: {board:#?}",
            page.path
        );
        let checked: Vec<Answered> = ["/api/task-board", "/api/queue"]
            .into_iter()
            .flat_map(|read| since(&site, &before, read))
            .collect();
        eprintln!("{} checked: {checked:#?}", page.path);
        if page.path == HOME.path {
            assert!(
                checked.is_empty(),
                "home reads only the board: {checked:#?}"
            );
        } else {
            each_unchanged_once(&checked, page.path);
        }
        if page.path == TASKS.path {
            // The counts and each kept column: one read each, all at once.
            let columns = kept(&mut chrome)
                .keys()
                .filter(|part| part.starts_with("board:waiting column:"))
                .count();
            assert_eq!(checked.len(), 1 + columns, "{checked:#?}");
        }
    }
}

#[test]
fn a_changed_read_redraws_and_its_copy_replaces_the_kept_one() {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    let board: Value = serde_json::from_str(&common::fixture("board.json")).unwrap();
    let mut card = board["next"][0].clone();
    card["title"] = json!("North · pendant picture, asked again");
    let mut listing = site_listing();
    listing["tasks"][0]["title"] = json!("Example task, renamed");
    for (page, read, changed, title) in [
        (TASKS, "/api/task-board", listing, "Example task, renamed"),
        (
            QUEUE,
            "/api/queue",
            queue(&card),
            "North · pendant picture, asked again",
        ),
    ] {
        let redrawn = format!(
            "document.querySelector('main').innerText.includes({})",
            json!(title)
        );
        visit(&mut chrome, &site, &page);
        let saved = oldest(&mut chrome, &page);
        let before = return_held(&mut chrome, &site, &page);
        let said = notice(&mut chrome, saved, false);
        drawn_from_the_device(&mut chrome, &page, &said);
        site.set_json(read, &changed);
        hold(&site, false);
        shows(&mut chrome, &redrawn, page.path);
        shows(&mut chrome, LIVE, page.path);
        let asked = since(&site, &before, read);
        assert!(
            !asked.is_empty()
                && asked
                    .iter()
                    .all(|answer| answer.named.is_some() && answer.status == 200),
            "{}: the kept copy's tag no longer names the read: {asked:#?}",
            page.path
        );
        // The new copy is the one kept: the next return visit draws it before any answer.
        let changed_part = page.parts[page.parts.len() - 1];
        shows(
            &mut chrome,
            &format!(
                "{PARTS}.then(kept => kept[{}] > {saved})",
                json!(changed_part)
            ),
            "the changed read kept",
        );
        let saved = oldest(&mut chrome, &page);
        return_held(&mut chrome, &site, &page);
        let said = notice(&mut chrome, saved, false);
        drawn_from_the_device(&mut chrome, &page, &said);
        shows(&mut chrome, &redrawn, page.path);
        hold(&site, false);
        shows(&mut chrome, LIVE, page.path);
    }
}

/// The stand-in's task board listing, as `site` serves it.
fn site_listing() -> Value {
    json!({
        "tasks": [{"slug": "tools/example", "title": "Example task", "body": "",
            "status": "ready", "priority": 1, "parent": null, "created_by": "owner",
            "created": 0.0, "updated": 0.0, "links": [], "depends": [], "roles": [],
            "instruction": false}],
        "statuses": ["ready"], "counts": [["ready", 1]], "signoff": [], "next": null
    })
}

/// The task board, opened with every read held: nothing drawn from the device, and once the reads
/// answer, drawn from the server and kept again.
#[track_caller]
fn draws_nothing_kept(chrome: &mut Chrome, site: &Site, what: &str) {
    shows(
        chrome,
        &format!("{PARTS}.then(kept => Object.keys(kept).length === 0)"),
        what,
    );
    assert_eq!(
        chrome.eval(&format!(
            "[document.querySelector('main')?.dataset.cache ?? null, document.body.innerText.includes('Loading tasks…'), {}]",
            present(TASKS.drawn)
        )),
        json!([null, true, false]),
        "{what}: nothing drawn from the device"
    );
    hold(site, false);
    shows(chrome, &present(TASKS.drawn), what);
    shows(chrome, LIVE, what);
    keeps(chrome, TASKS.parts);
}

#[test]
fn a_store_of_another_format_or_a_value_this_client_cannot_read_draws_nothing() {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit(&mut chrome, &site, &TASKS);
    // A client of another format opened the store last: this one clears it as it opens.
    rewrite(
        &mut chrome,
        &format!(
            "stores => stores.meta.put({}, 'format')",
            symphony_api::CACHE_FORMAT + 1
        ),
    );
    return_held(&mut chrome, &site, &TASKS);
    draws_nothing_kept(&mut chrome, &site, "another format");
    // A kept value of a shape this client cannot read: a miss, and its item is forgotten whole.
    rewrite(
        &mut chrome,
        r#"stores => stores.parts.openCursor().onsuccess = event => {
            const cursor = event.target.result;
            if (!cursor) return;
            if (cursor.key.endsWith("6:counts")) cursor.update({ ...cursor.value, value: "a listing" });
            cursor.continue();
        }"#,
    );
    return_held(&mut chrome, &site, &TASKS);
    draws_nothing_kept(&mut chrome, &site, "an unreadable value");
}

/// A visit left before its columns answered keeps the counts alone. The next draws the columns'
/// totals from the device, and once the board's version is read it reads each column as a first
/// visit does.
#[test]
fn counts_kept_without_their_columns_draw_and_then_read_each_column() {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit(&mut chrome, &site, &TASKS);
    rewrite(
        &mut chrome,
        r#"stores => stores.parts.openCursor().onsuccess = event => {
            const cursor = event.target.result;
            if (!cursor) return;
            if (cursor.key.includes("column:")) cursor.delete();
            cursor.continue();
        }"#,
    );
    let before = answered(&site);
    hold(&site, true);
    chrome.open("about:blank");
    chrome.open(&site.url(TASKS.path));
    shows(
        &mut chrome,
        "(main => main?.dataset.cache === 'device' && main.querySelector('[data-status=ready] .count')?.textContent === '1')(document.querySelector('main'))",
        "the counts drawn from the device",
    );
    hold(&site, false);
    shows(&mut chrome, &present(TASKS.drawn), "the column read");
    shows(&mut chrome, LIVE, "the column read");
    let columns = since(&site, &before, "/api/task-board")
        .into_iter()
        .filter(|answer| answer.target.contains("status=ready"))
        .collect::<Vec<_>>();
    assert_eq!(columns.len(), 1, "one read of the column: {columns:#?}");
    assert_eq!(
        (columns[0].named.as_deref(), columns[0].status),
        (None, 200),
        "no copy to name"
    );
    keeps(&mut chrome, TASKS.parts);
}

#[test]
fn a_server_that_does_not_answer_leaves_the_kept_copy_drawn_saying_so() {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    for page in [HOME, TASKS, QUEUE] {
        visit(&mut chrome, &site, &page);
        let saved = oldest(&mut chrome, &page);
        site.fail_route("/api/board", true);
        chrome.open("about:blank");
        chrome.open(&site.url(page.path));
        let said = notice(&mut chrome, saved, true);
        drawn_from_the_device(&mut chrome, &page, &said);
        site.fail_route("/api/board", false);
        shows(&mut chrome, LIVE, page.path);
    }
}

/// Wait until the stand-in has answered `read` `500` at least `times` times since `from` of its
/// answers.
#[track_caller]
fn fails(site: &Site, read: &str, from: usize, times: usize) {
    let failed = || {
        site.answers(read)[from..]
            .iter()
            .filter(|answer| answer.status == 500)
            .count()
            >= times
    };
    if let Err(stall) =
        test_time::until_true(format!("{read} failing {times} times"), STALL, failed)
    {
        panic!("{stall}: {:#?}", &site.answers(read)[from..]);
    }
}

/// Wait until, of the stand-in's requests since `from`, the feed has been asked for and then
/// `read` again. How often `read` has failed cannot say so: the task board's counts and each
/// column it drew are reads of one route, each tried once before the feed is asked for.
#[track_caller]
fn asked_again_after_the_feed(site: &Site, read: &str, from: usize) {
    let asked = || {
        let requests = site.requests();
        let feed = requests[from..]
            .iter()
            .position(|path| path == symphony_api::FEED_PATH);
        feed.is_some_and(|feed| requests[from + feed..].iter().any(|path| path == read))
    };
    if let Err(stall) = test_time::until_true(
        format!("{read} asked again once the feed was"),
        STALL,
        asked,
    ) {
        panic!("{stall}: {:#?}", &site.targets()[from..]);
    }
}

/// A check that fails is asked again until it answers, whichever read it is: the machine's work,
/// the task board's counts and columns, the queue. Meanwhile the feed is open, so the page says it
/// is checking, never that the server is not reachable, and its actions wait; once the read
/// answers the page is current, its Later buttons back.
#[test]
fn a_check_that_fails_is_asked_again_until_it_answers() {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    for (page, read) in [
        (TASKS, "/api/machine-work"),
        (TASKS, "/api/task-board"),
        (QUEUE, "/api/queue"),
    ] {
        visit(&mut chrome, &site, &page);
        let saved = oldest(&mut chrome, &page);
        let (asked, from) = (site.requests().len(), site.answers(read).len());
        site.fail_route(read, true);
        chrome.open("about:blank");
        chrome.open(&site.url(page.path));
        // Its first try fails before the feed opens, a second after.
        asked_again_after_the_feed(&site, read, asked);
        fails(&site, read, from, 2);
        let said = notice(&mut chrome, saved, false);
        drawn_from_the_device(&mut chrome, &page, &said);
        site.fail_route(read, false);
        shows(&mut chrome, LIVE, read);
        if page.path == QUEUE.path {
            shows(
                &mut chrome,
                &format!(
                    "(b => b.textContent === 'Later' && !b.disabled)(document.querySelector({}))",
                    json!(page.drawn)
                ),
                "the Later button, once the queue answers",
            );
        }
    }
}

/// The feed's `machine-work` event carries the machine's whole work: it answers for a read of the
/// work that keeps failing.
#[test]
fn the_feeds_machine_work_answers_for_a_failing_read_of_it() {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit(&mut chrome, &site, &TASKS);
    let saved = oldest(&mut chrome, &TASKS);
    let from = site.answers("/api/machine-work").len();
    site.fail_route("/api/machine-work", true);
    chrome.open("about:blank");
    chrome.open(&site.url(TASKS.path));
    fails(&site, "/api/machine-work", from, 2);
    let said = notice(&mut chrome, saved, false);
    drawn_from_the_device(&mut chrome, &TASKS, &said);
    let board: Value = serde_json::from_str(&common::fixture("board.json")).unwrap();
    let version = board["version"].as_u64().unwrap() + 1;
    let work = json!({"freshness": "unknown", "machines": [], "unassigned": []});
    site.push(
        version,
        &json!({"version": version, "changes": [{"region": "machine-work", "data": work}]})
            .to_string(),
    );
    shows(&mut chrome, LIVE, "the work, from the feed");
    site.fail_route("/api/machine-work", false);
}

/// A task board listing of in-progress `tasks`, `building` of them in the machine's Build.
fn in_progress(tasks: &[&str], building: usize) -> Value {
    let tasks: Vec<Value> = tasks
        .iter()
        .map(|slug| {
            json!({"slug": slug, "title": slug, "body": "", "status": "in_progress",
                "priority": 1, "parent": null, "created_by": "owner", "created": 0.0,
                "updated": 0.0, "links": [], "depends": [], "roles": [], "instruction": false})
        })
        .collect();
    json!({"tasks": tasks, "statuses": ["in_progress"],
        "counts": [["in_progress", 1], ["build", building]], "signoff": [], "next": null})
}

/// The machine's work: machine alpha, building each of `tasks`.
fn building(tasks: &[&str]) -> Value {
    let operations: Vec<Value> = tasks
        .iter()
        .map(|slug| {
            json!({"machine_id": "alpha", "operation_id": format!("build-{slug}"),
                "parent_id": null, "handle": "example", "task": slug, "checkout": "opaque",
                "recipe": "fixture", "kind": "build", "state": "running",
                "resource": "native resource", "reason": "fixture", "phase_since": 1.0})
        })
        .collect();
    json!({"freshness": "fresh", "unassigned": [], "machines": [{"machine_id": "alpha",
        "label": "alpha", "producer_id": "fixture", "sequence": 1, "age_ms": 0,
        "freshness": "fresh", "complete": true, "reason": "", "counts": null,
        "operations": operations}]})
}

/// The task board's default view's read of `query`.
fn listing_read(query: &str) -> String {
    format!("/api/task-board?view=waiting&{query}&days=1&activity=1&work=0")
}

/// A task drawn from the device in the machine's Build, which the server lists nowhere now and no
/// machine builds: it goes once every column drawn from the device has answered, whichever order
/// the column it is in now, its old one and the machine's work answer in. A task every answer
/// still lists stays.
#[test]
fn a_task_no_fresh_answer_lists_goes_whichever_order_the_answers_come_in() {
    const GHOST: &str = "[data-slug='probe/ghost']";
    const STAY: &str = "[data-slug='probe/stay']";
    let page = Page {
        path: "/tasks",
        drawn: "[data-status=build] [data-slug='probe/ghost']",
        parts: &[
            "board:waiting counts",
            "board:waiting work",
            "board:waiting column:in_progress",
            "board:waiting column:build",
        ],
    };
    let (column, build, work) = (
        listing_read("q=&status=in_progress"),
        listing_read("q=&status=build"),
        "/api/machine-work",
    );
    let orders = [
        [column.as_str(), work, build.as_str()],
        [work, column.as_str(), build.as_str()],
    ];
    for order in orders {
        let site = site();
        let Some(mut chrome) = crate::chrome() else {
            return;
        };
        // The first visit: probe/ghost in the machine's Build, probe/stay in progress.
        site.set_json("/api/task-board", &in_progress(&[], 1));
        site.set_json(&column, &in_progress(&["probe/stay"], 1));
        site.set_json(&build, &in_progress(&["probe/ghost"], 1));
        site.set_json(work, &building(&["probe/ghost"]));
        visit(&mut chrome, &site, &page);
        // Since: probe/ghost is listed nowhere, and nothing is built.
        site.set_json("/api/task-board", &in_progress(&[], 0));
        site.set_json(&column, &in_progress(&["probe/stay"], 0));
        site.set_json(&build, &in_progress(&[], 0));
        site.set_json(work, &building(&[]));
        for read in order {
            site.hold_route(read, true);
        }
        chrome.open("about:blank");
        chrome.open(&site.url(page.path));
        shows(
            &mut chrome,
            "document.querySelector('.column[data-status=build]') !== null",
            "kept Build column",
        );
        chrome.eval("document.querySelector('.column[data-status=build]').scrollIntoView({block:'nearest',inline:'center'})");
        shows(
            &mut chrome,
            &present(page.drawn),
            "drawn from the device in Build",
        );
        for read in order {
            let route = read.split('?').next().unwrap();
            let from = site
                .answers(route)
                .iter()
                .filter(|answer| answer.target == read)
                .count();
            site.hold_route(read, false);
            test_time::until_true(format!("{order:?}: {read} answered"), STALL, || {
                site.answers(route)
                    .iter()
                    .filter(|answer| answer.target == read)
                    .count()
                    > from
            })
            .expect("a read let go was never answered");
        }
        shows(&mut chrome, LIVE, "every read answered");
        chrome.eval("document.querySelector('.column[data-status=in_progress]').scrollIntoView({block:'nearest',inline:'center'})");
        shows(
            &mut chrome,
            &format!("!({}) && {}", present(GHOST), present(STAY)),
            &format!("{order:?}: the task no answer lists"),
        );
        chrome.pump(Duration::from_millis(1_000));
        assert_eq!(
            chrome.eval(&format!("[{}, {}]", present(GHOST), present(STAY))),
            json!([false, true]),
            "{order:?}: a second after"
        );
    }
}

/// The look-parity document's card verdict form and its new question: `[label, disabled]` for each
/// button that saves (the verdicts, Later, the message, Dismiss and Reset, then Ask), and
/// `[text, disabled]` for each draft (the card's, then the question's).
const FORMS: &str = r#"(() => {
    const card = document.querySelector('[data-doc-key="card:palette"] form.verdict');
    const ask = document.getElementById("new-ask")?.form;
    return card && ask ? [
        [...card.querySelectorAll(".verdict-choices button, button[type=submit], .v-dismissed, .reset"),
            ask.querySelector("button[type=submit]")].map(b => [b.textContent.trim(), b.disabled]),
        [card.querySelector("textarea"), ask.querySelector("textarea")].map(t => [t.value, t.disabled]),
    ] : null;
})()"#;

const CARD_DRAFT: &str = "[data-doc-key=\"card:palette\"] form.verdict textarea";
const ASK_DRAFT: &str = "#new-ask";

/// Type `text` into the draft `selector` as the owner does.
fn draft(chrome: &mut Chrome, selector: &str, text: &str) {
    chrome.eval(&format!(
        "(t => {{ t.focus(); t.value = {}; t.dispatchEvent(new Event('input', {{bubbles: true}})); return true; }})(document.querySelector({}))",
        json!(text),
        json!(selector)
    ));
}

/// [`FORMS`] comes to say `forms`.
#[track_caller]
fn forms_say(chrome: &mut Chrome, forms: &Value, what: &str) {
    shows(
        chrome,
        &format!("JSON.stringify({FORMS}) === {}", json!(forms.to_string())),
        what,
    );
}

/// The reconnecting rule is the same: a document whose feed is lost holds its verdicts, message,
/// Later, Dismiss, Reset and new question, each saying "Checking…", while their drafts stay
/// editable, and gives them back as they were once the feed is open again.
#[test]
fn a_documents_verdicts_and_questions_wait_while_its_feed_reconnects_and_their_drafts_do_not() {
    let site = crate::site();
    for (endpoint, response) in crate::look_parity::responses() {
        site.set_json(&endpoint, &response);
    }
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    chrome.open(&site.url("/docs/look-parity.md"));
    let feed = "document.querySelector('.document-page')?.dataset.feed";
    shows(
        &mut chrome,
        &format!(
            "{feed} === 'live' && {}",
            present("[data-ready=document] .asks-shell")
        ),
        "the document",
    );
    // The questions draw once their shell is in view; the card's form, opened after, grows above
    // it and would push it out again.
    chrome.eval("document.querySelector('.asks-shell').scrollIntoView({block: 'center'})");
    shows(&mut chrome, &present(ASK_DRAFT), "the new question");
    chrome.eval("document.querySelector('.doc-card .doc-actions button').click()");
    shows(&mut chrome, &format!("({FORMS}) !== null"), "the forms");
    draft(&mut chrome, CARD_DRAFT, "Keep the palette");
    draft(&mut chrome, ASK_DRAFT, "Is the palette final?");
    let live = chrome.eval(FORMS);
    let labels: Vec<String> = live[0]
        .as_array()
        .unwrap()
        .iter()
        .map(|save| {
            assert_eq!(save[1], json!(false), "{save} waits while the feed is open");
            save[0].as_str().unwrap().to_owned()
        })
        .collect();
    // The card is approved, so it has a verdict to reset.
    for label in [
        "✓ Approve",
        "✗ Reject",
        "? Question",
        "Send message",
        "⊘ Dismiss",
        "↺ Reset to pending",
        "Ask",
    ] {
        assert!(
            labels.iter().any(|each| each == label),
            "{label} in {labels:?}"
        );
    }
    let drafts = json!([
        ["Keep the palette", false],
        ["Is the palette final?", false]
    ]);
    assert_eq!(live[1], drafts);

    site.fail_feeds(usize::MAX);
    site.drop_feeds();
    shows(&mut chrome, &format!("{feed} === 'stale'"), "the lost feed");
    let waiting = vec![json!(["Checking…", true]); labels.len()];
    forms_say(&mut chrome, &json!([waiting, drafts]), "the saves wait");
    draft(
        &mut chrome,
        CARD_DRAFT,
        "Keep the palette, and its contrast",
    );
    draft(&mut chrome, ASK_DRAFT, "Is the palette final, in dark?");
    let edited = json!([
        ["Keep the palette, and its contrast", false],
        ["Is the palette final, in dark?", false]
    ]);
    forms_say(
        &mut chrome,
        &json!([waiting, edited]),
        "the drafts are edited while the page waits",
    );

    site.fail_feeds(0);
    shows(&mut chrome, &format!("{feed} === 'live'"), "the feed again");
    let back: Vec<Value> = labels.iter().map(|label| json!([label, false])).collect();
    forms_say(&mut chrome, &json!([back, edited]), "the saves come back");
}
