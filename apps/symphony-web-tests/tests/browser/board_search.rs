//! A search on the task board (the React client's `src/boardlive.tsx`): its counts and each
//! column on the screen are read once and drawn, however the feed moves meanwhile
//! (app/board-search-slow-to-fill). A loaded machine's work comes on the feed every second; each
//! one used to drop every column read in flight and read them all again, so a search's cards
//! stayed grey for 5 to 60 s.

use serde_json::{Value, json};

use crate::chrome::Chrome;
use crate::site::Site;
use crate::{SHOWN, STALL};

/// The word searched for.
const WORD: &str = "found";

/// The board's version, from which the page follows the feed: the fixture's.
fn version() -> u64 {
    let board: Value = serde_json::from_str(&crate::common::fixture("board.json")).unwrap();
    board["version"].as_u64().unwrap()
}

/// A listing of `tasks` (slug, status), counting `counts` by stage, as the server answers the
/// board with the machine's activity: every status and Build and Test.
fn listing(tasks: &[(&str, &str)], counts: &[(&str, usize)]) -> Value {
    let tasks: Vec<Value> = tasks
        .iter()
        .map(|(slug, status)| {
            json!({"slug": slug, "title": slug, "body": "", "status": status, "priority": 1,
                "parent": null, "created_by": "owner", "created": 0.0, "updated": 0.0,
                "links": [], "depends": [], "roles": [], "instruction": false})
        })
        .collect();
    json!({"tasks": tasks, "counts": counts, "signoff": [], "next": null,
        "statuses": ["backlog", "concept", "design", "ready", "in_progress", "review", "done",
            "cancelled", "blocked", "build", "test"]})
}

/// The read of the search's `query` (its counts, or one column's first page), as the page asks it.
fn search_read(query: &str) -> String {
    format!("/api/task-board?view=waiting&{query}&days=1&activity=1&work=0")
}

fn counts_read() -> String {
    search_read(&format!("counts=1&q={WORD}"))
}

fn column_read(status: &str) -> String {
    search_read(&format!("q={WORD}&status={status}"))
}

/// The machine's work as its `sequence`th sample says it, `said` the observation as a whole: one
/// fresh machine running a build for a task not on this board. Each sample moves its sequence and
/// age as a heartbeat does, and places every task alike.
fn work(sequence: u64, said: &str) -> Value {
    json!({"freshness": said, "unassigned": [], "machines": [{"machine_id": "alpha",
        "label": "alpha", "producer_id": "fixture", "sequence": sequence,
        "age_ms": sequence * 100, "freshness": "fresh", "complete": true, "reason": "",
        "counts": null, "operations": [{"machine_id": "alpha", "operation_id": "build-1",
            "parent_id": null, "handle": "example", "task": "elsewhere/task",
            "checkout": "opaque", "recipe": "fixture", "kind": "build", "state": "running",
            "resource": "native resource", "reason": "fixture", "phase_since": 1.0}]}]})
}

/// The stand-in serving a board whose ready and in-progress columns hold a task each, and a search
/// for [`WORD`] that finds two in each.
fn site() -> Site {
    let site = crate::site();
    let both = [("ready", 2), ("in_progress", 2)];
    site.set_json(
        "/api/task-board",
        &listing(
            &[("board/ready", "ready"), ("board/doing", "in_progress")],
            &[("ready", 1), ("in_progress", 1)],
        ),
    );
    site.set_json(&counts_read(), &listing(&[], &both));
    site.set_json(
        &column_read("ready"),
        &listing(
            &[("found/ready-a", "ready"), ("found/ready-b", "ready")],
            &both,
        ),
    );
    site.set_json(
        &column_read("in_progress"),
        &listing(
            &[
                ("found/doing-a", "in_progress"),
                ("found/doing-b", "in_progress"),
            ],
            &both,
        ),
    );
    site.set_json("/api/machine-work", &work(1, "fresh"));
    site
}

/// The board open on a wide screen, following the feed, its default view drawn, the search's
/// column reads held: what the stand-in has been asked so far, from which the search's reads count.
fn opened(site: &Site, chrome: &mut Chrome) -> usize {
    chrome.viewport(1440, 900, false);
    chrome.open(&site.url("/tasks"));
    assert!(
        chrome.wait_for(
            "document.querySelector('[data-status=in_progress] [data-slug=\"board/doing\"]') !== null",
            SHOWN
        ),
        "the board's default view was not drawn: {:?}",
        site.targets()
    );
    crate::wait_for_feed(site).expect("the page never followed the feed");
    site.hold_route(&column_read("ready"), true);
    site.hold_route(&column_read("in_progress"), true);
    site.targets().len()
}

/// Search for [`WORD`], as one keystroke would, and wait until the search's counts are answered and
/// both columns asked (and held).
fn search(site: &Site, chrome: &mut Chrome, from: usize) {
    chrome.eval(&format!(
        "const input = document.querySelector('input[type=search]'); input.value = '{WORD}'; \
         input.dispatchEvent(new Event('input', {{bubbles: true}})); true"
    ));
    until(
        chrome,
        site,
        "the search's counts answered and both columns asked",
        || {
            let asked = searched(site, from);
            site.answers("/api/task-board")
                .iter()
                .any(|answer| answer.target == counts_read() && answer.status == 200)
                && asked.contains(&column_read("ready"))
                && asked.contains(&column_read("in_progress"))
        },
    );
}

/// The search's reads the stand-in was asked since `from`, in order.
fn searched(site: &Site, from: usize) -> Vec<String> {
    site.targets()[from..]
        .iter()
        .filter(|target| {
            target.starts_with("/api/task-board?") && target.contains(&format!("q={WORD}&"))
        })
        .cloned()
        .collect()
}

/// How many times the search asked each of its reads since `from`: its counts, the ready column,
/// the in-progress column.
fn rounds(site: &Site, from: usize) -> [usize; 3] {
    let asked = searched(site, from);
    let times = |read: String| asked.iter().filter(|target| **target == read).count();
    [
        times(counts_read()),
        times(column_read("ready")),
        times(column_read("in_progress")),
    ]
}

/// Wait until `done`, the page let run meanwhile; a stall past the backstop fails naming `what`.
fn until(chrome: &mut Chrome, site: &Site, what: &str, mut done: impl FnMut() -> bool) {
    if let Err(stall) = test_time::until_true(what, STALL, || {
        done() || {
            chrome.pump(std::time::Duration::from_millis(25));
            false
        }
    }) {
        panic!("{stall}: {:#?}", site.targets());
    }
}

/// Both columns show the search's cards.
const FOUND: &str = "document.querySelector('[data-status=ready] [data-slug=\"found/ready-a\"]') !== null \
    && document.querySelector('[data-status=in_progress] [data-slug=\"found/doing-a\"]') !== null";

/// The machine's work keeps coming while a search's columns are read, every sample a heartbeat that
/// places no task anywhere else: the search reads its counts and each column once, and draws what
/// they answered. Before, each sample dropped the columns' answers and read everything again.
#[test]
fn a_search_reads_its_counts_and_columns_once_while_the_machines_heartbeat_comes() {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    chrome.call("Page.addScriptToEvaluateOnNewDocument",json!({"source": "const Socket = WebSocket; window.__machineSamples = []; window.WebSocket = class extends Socket { constructor(...args) { super(...args); this.addEventListener('message', event => { const data = JSON.parse(event.data.slice(event.data.indexOf(String.fromCharCode(10)) + 1)); for (const change of data.changes ?? []) if (change.region === 'machine-work') window.__machineSamples.push(change.data.freshness); }); } };"}));
    let from = opened(&site, &mut chrome);
    search(&site, &mut chrome, from);
    // Observe all three samples delivered while column reads are out. Keep the stage grid
    // mounted: opening machine details now deliberately changes the board to an activity list.
    let version = version();
    for (n, said) in ["incomplete", "fresh", "stale"].into_iter().enumerate() {
        let at = version + 1 + n as u64;
        site.push(
            at,
            &json!({"version": at, "changes": [{"region": "machine-work", "data": work(2 + n as u64, said)}]})
                .to_string(),
        );
    }
    assert!(
        chrome.wait_for(
            "JSON.stringify(window.__machineSamples) === JSON.stringify(['incomplete','fresh','stale'])",
            SHOWN
        ),
        "the page never had the machine's last sample"
    );
    site.hold_route(&column_read("ready"), false);
    site.hold_route(&column_read("in_progress"), false);
    assert!(
        chrome.wait_for(FOUND, SHOWN),
        "the search's cards were never drawn: {:#?}",
        site.targets()
    );
    assert_eq!(
        rounds(&site, from),
        [1, 1, 1],
        "the search read again: {:#?}",
        searched(&site, from)
    );
    assert_eq!(
        chrome.eval("document.querySelectorAll('[data-slug^=\"found/\"]').length"),
        json!(4)
    );
}

/// A change the feed brings while a search's columns are read asks the board to read them again,
/// and the page draws the answers it asked for meanwhile, not the grey of a page waiting on that
/// refresh: here the refresh's counts are held, and the cards are drawn all the same.
#[test]
fn a_search_draws_the_columns_it_read_while_the_refresh_a_message_asked_for_waits() {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    let from = opened(&site, &mut chrome);
    search(&site, &mut chrome, from);
    site.hold_route(&counts_read(), true);
    // A message on any thread may make a task match the search: the board reads it again.
    let at = version() + 1;
    site.push(
        at,
        &json!({"version": at, "changes": [{"region": "message", "data": {"slug": "found/ready-a"}}]})
            .to_string(),
    );
    until(&mut chrome, &site, "the refresh's counts asked", || {
        rounds(&site, from)[0] == 2
    });
    site.hold_route(&column_read("ready"), false);
    site.hold_route(&column_read("in_progress"), false);
    assert!(
        chrome.wait_for(FOUND, SHOWN),
        "the answers read before the message were dropped, the refresh held: {:#?}",
        site.targets()
    );
    assert_eq!(
        rounds(&site, from),
        [2, 1, 1],
        "{:#?}",
        searched(&site, from)
    );
    // The refresh goes on once its counts answer: each column read once more, its cards kept.
    site.hold_route(&counts_read(), false);
    until(&mut chrome, &site, "the refresh's columns answered", || {
        let answered = site.answers("/api/task-board");
        ["ready", "in_progress"].into_iter().all(|status| {
            answered
                .iter()
                .filter(|answer| answer.target == column_read(status))
                .count()
                == 2
        })
    });
    assert!(chrome.wait_for(FOUND, SHOWN));
    assert_eq!(
        rounds(&site, from),
        [2, 2, 2],
        "{:#?}",
        searched(&site, from)
    );
}
