//! Every JSON read carries a weak validator of its content, keeps `no-store`, and answers `304`
//! with no body to a request that names it (docs/engineering/analysis/client-cache.md, "How freshness is
//! checked").
//!
//! Some reads move on their own while the fixture is quiet: a view carrying the machine's work
//! carries the sequence and age of the board tick's last scan of it, which every tick (a second)
//! moves ([`ticking`]). On a loaded machine two requests may take longer than a tick every time,
//! so those reads are never asked to hold still: what holds whatever the tick does is checked of
//! them. Any other read may still move while the server takes in a change, so a check that needs it
//! to hold still between two requests says what it saw when it did not, and asks again until it
//! does: a `200` to a request naming a tag is only ever sent under another tag, and two reads with
//! the same content always share theirs.

use super::client::{Fixture, Response};
use super::{DOCUMENT, LIMIT, SCOPE, write};

/// The fixture's task, which `task_pages` makes before this phase runs.
const TASK: &str = "fixture%2Flong-brief";

/// The tag `response` carries, checked to be weak and 16 lowercase hex digits.
#[track_caller]
fn tag(response: &Response, route: &str) -> String {
    let tag = response.header("etag");
    let hex = tag
        .strip_prefix("W/\"")
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_else(|| panic!("{route}: {tag} is not a weak tag"));
    assert!(
        hex.len() == 16 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "{route}: {tag}"
    );
    tag.into()
}

fn encoded(text: &str) -> String {
    text.bytes().map(|byte| format!("%{byte:02X}")).collect()
}

fn get(f: &Fixture, route: &str, accept: &[(&str, &str)]) -> Response {
    f.request("GET", route, accept, &[]).status(200)
}

/// Looks at `condition` until it gives a value, which it returns: `what` of `route` fails as a
/// stall once the fixture's backstop, which only a stall reaches, has passed with none.
#[track_caller]
fn until<T>(route: &str, what: &str, condition: impl FnMut() -> Option<T>) -> T {
    test_time::until(format!("{route}: {what}"), LIMIT, condition)
        .unwrap_or_else(|stall| panic!("{stall}"))
}

/// Whether `route`'s view carries the machine's work as the board tick last scanned it: the
/// machine work, the agents' views, a task's page and a task listing that does not leave the work
/// out (`work=0`). Each tick moves it.
fn ticking(route: &str) -> bool {
    let path = route.split_once('?').map_or(route, |(path, _)| path);
    match path {
        "/api/machine-work" | "/api/agents" | "/api/agent-page" | "/api/task-page" => true,
        "/api/task-board" => !route.contains("work=0"),
        _ => false,
    }
}

/// `route` asked naming `named`, by `method`, with gzip offered: `304`, no body, the tag and
/// `no-store` kept, varying by the encoding as the gzipped `200` does, nothing encoded or typed;
/// or, when it is not, the answer as it came.
fn conditional(
    f: &Fixture,
    method: &str,
    route: &str,
    accept: &[(&str, &str)],
    named: &str,
) -> Result<(), Response> {
    let mut headers = vec![("If-None-Match", named), ("Accept-Encoding", "gzip")];
    headers.extend_from_slice(accept);
    let cached = f.request(method, route, &headers, &[]);
    if cached.status != 304 {
        return Err(cached);
    }
    assert!(cached.body.is_empty(), "{method} {route}");
    assert_eq!(
        cached.header("cache-control"),
        "no-store",
        "{method} {route}"
    );
    assert_eq!(cached.header("vary"), "accept-encoding", "{method} {route}");
    for absent in ["content-encoding", "content-type"] {
        assert!(
            !cached.headers.contains_key(absent),
            "{method} {route}: {absent} on a 304: {cached:?}"
        );
    }
    Ok(())
}

/// A read that holds still, asked naming `named`: `304`.
#[track_caller]
fn unchanged(f: &Fixture, method: &str, route: &str, named: &str) {
    if let Err(answer) = conditional(f, method, route, &[], named) {
        panic!("{method} {route} naming {named}: {answer:?}");
    }
}

/// `route` asked naming its current tag, written by `named`: `304`. A `200` may only come under
/// another tag, the content having moved between the two requests; then it asks again.
#[track_caller]
fn current(
    f: &Fixture,
    method: &str,
    route: &str,
    accept: &[(&str, &str)],
    named: impl Fn(&str) -> String,
) {
    until(route, "never held still for a conditional read", || {
        let read = get(f, route, accept);
        let now = tag(&read, route);
        let Err(answer) = conditional(f, method, route, accept, &named(&now)) else {
            return Some(());
        };
        assert_eq!(answer.status, 200, "{method} {route}: {answer:?}");
        assert_ne!(
            answer.header("etag"),
            now,
            "{method} {route}: the body was sent under the very tag it was asked by"
        );
        None
    });
}

/// Two reads of `route` with the same content, which share their tag: a tag is a function of the
/// content, whatever order the server wrote it in. The second of them.
#[track_caller]
fn settled(f: &Fixture, route: &str, accept: &[(&str, &str)]) -> Response {
    let mut last = get(f, route, accept);
    until(route, "never read the same twice", || {
        let next = get(f, route, accept);
        if next.json() == last.json() {
            assert_eq!(
                tag(&next, route),
                tag(&last, route),
                "{route}: the same content under two tags"
            );
            return Some(next);
        }
        last = next;
        None
    })
}

/// `probe` made between two reads of `route` that agree, and their tag.
fn between(
    f: &Fixture,
    route: &str,
    accept: &[(&str, &str)],
    probe: impl Fn() -> Response,
) -> (Response, String) {
    until(route, "never held still around a probe", || {
        let before = tag(&get(f, route, accept), route);
        let probed = probe();
        (tag(&get(f, route, accept), route) == before).then_some((probed, before))
    })
}

pub(super) fn json_reads(f: &Fixture) {
    let document = get(f, DOCUMENT, &[]).json();
    let key = document["sections"][0]["key"].as_str().unwrap();
    let anchor = document["sections"][0]["anchor"]
        .as_str()
        .unwrap_or_default();
    let keys = encoded(&serde_json::to_string(&[key]).unwrap());
    let reads = [
        DOCUMENT.to_string(),
        format!("{DOCUMENT}&view=outline"),
        format!("{DOCUMENT}&view=first"),
        format!("{SCOPE}{DOCUMENT}"),
        "/api/folder?path=docs/".into(),
        format!("/api/section?path=docs/TOPIC.md&format=blocks&keys={keys}"),
        format!("/api/section?path=docs/TOPIC.md&format=blocks&anchor={anchor}"),
        "/api/history?path=docs/TOPIC.md".into(),
        "/api/versions?path=docs/TOPIC.md".into(),
        "/api/asks?path=docs/TOPIC.md&format=blocks".into(),
        "/api/board".into(),
        format!("{SCOPE}/api/board"),
        "/api/queue".into(),
        "/api/postponed".into(),
        "/api/pending".into(),
        "/api/activity".into(),
        "/api/tasks".into(),
        "/api/task-board?counts=1".into(),
        "/api/task-board?status=ready".into(),
        "/api/task-board?counts=1&activity=1&work=0".into(),
        "/api/task-board?status=build&activity=1&work=0".into(),
        "/api/machine-work".into(),
        format!("/api/task-page?slug={TASK}"),
        format!("/api/task-brief?slug={TASK}&from=0"),
        format!("/api/task-history?slug={TASK}"),
        format!("/api/task-thread?slug={TASK}"),
        "/api/agents".into(),
        "/api/agent-page".into(),
    ];
    let json = [("Accept", "application/json")];
    let reads = reads
        .iter()
        .map(|route| (route.as_str(), &[][..]))
        .chain([("/queue", &json[..])]);
    for (route, accept) in reads {
        let read = if ticking(route) {
            get(f, route, accept)
        } else {
            settled(f, route, accept)
        };
        assert_eq!(read.header("cache-control"), "no-store", "{route}");
        assert!(
            read.header("content-type").starts_with("application/json"),
            "{route}"
        );
        tag(&read, route);
        if !ticking(route) {
            let (gzipped, plain) = between(f, route, accept, || {
                get(f, route, &[accept, &[("Accept-Encoding", "gzip")]].concat())
            });
            assert_eq!(
                tag(&gzipped, route),
                plain,
                "{route}: the tag names the JSON, not its encoding"
            );
        }
        for method in ["GET", "HEAD"] {
            if !ticking(route) {
                current(f, method, route, accept, str::to_string);
                current(f, method, route, accept, |tag| {
                    tag.strip_prefix("W/").unwrap().to_string()
                });
                current(f, method, route, accept, |tag| format!("\"other\", {tag}"));
            }
            current(f, method, route, accept, |_| "*".to_string());
            let stale = f.request(
                method,
                route,
                &[accept, &[("If-None-Match", "W/\"0000000000000000\"")]].concat(),
                &[],
            );
            assert_eq!(stale.status, 200, "{method} {route}: {stale:?}");
            tag(&stale, route);
            assert_eq!(stale.body.is_empty(), method == "HEAD", "{method} {route}");
        }
    }
    // A miss is never a read to keep: no tag, and naming every tag does not turn it into a 304.
    for route in [
        "/api/task-page?slug=fixture%2Fno-such-task",
        "/api/document?path=docs/NOPE.md",
        "/api/document",
    ] {
        let miss = f.request("GET", route, &[("If-None-Match", "*")], &[]);
        assert!(matches!(miss.status, 400 | 404), "{route}: {miss:?}");
        assert!(!miss.headers.contains_key("etag"), "{route}: {miss:?}");
        assert_eq!(miss.header("cache-control"), "no-store", "{route}");
    }
    // The document's reads as they are before the changes below, none of which touch it.
    let documents: Vec<(String, Response)> = [
        DOCUMENT.to_string(),
        format!("{DOCUMENT}&view=outline"),
        format!("{DOCUMENT}&view=first"),
        format!("/api/section?path=docs/TOPIC.md&format=blocks&keys={keys}"),
    ]
    .into_iter()
    .map(|route| {
        let read = get(f, &route, &[]);
        (route, read)
    })
    .collect();
    let board = get(f, "/api/board", &[]);
    changed(f, "/api/document?path=docs/OTHER.md", || {
        write(&f.root, "docs/OTHER.md", "# Other, edited\n")
    });
    changed(f, &format!("/api/task-history?slug={TASK}"), || {
        let store = symphony_board::cli::connect(&symphony_board::cli::Env::of_process()).unwrap();
        store
            .say("fixture/long-brief", "owner", "A new message", None)
            .unwrap();
    });
    cursor_is_not_content(f, &documents, &board);
}

/// After `change`, `route` asked by its old tag is sent the new body under a new tag, which a
/// read naming it is then answered `304` for. The server sees a document's edit through its
/// watcher, so the read is repeated until it does.
fn changed(f: &Fixture, route: &str, change: impl FnOnce()) {
    let before = get(f, route, &[]);
    let old = tag(&before, route);
    change();
    let after = until(route, "never changed", || {
        let read = f.request("GET", route, &[("If-None-Match", &old)], &[]);
        if read.status == 200 {
            return Some(read);
        }
        assert_eq!(read.status, 304, "{route}: {read:?}");
        None
    });
    let new = tag(&after, route);
    assert_ne!(new, old, "{route}");
    assert_ne!(after.body, before.body, "{route}");
    unchanged(f, "GET", route, &new);
}

/// The feed's cursor moved with those changes, and so did the board's tag, whose `version` is
/// what the page opens its feed from. A document's body carries the moved cursor too, but its
/// tag names its content alone, which did not change: it is still answered `304`.
fn cursor_is_not_content(f: &Fixture, documents: &[(String, Response)], board: &Response) {
    let moved = until("/api/board", "the board's cursor never moved", || {
        let now = get(f, "/api/board", &[]);
        (now.json()["version"] != board.json()["version"]).then_some(now)
    });
    assert_ne!(moved.header("etag"), board.header("etag"));
    let cursor = |read: &Response| {
        let json = read.json();
        json.get("document").unwrap_or(&json)["version"].clone()
    };
    // The cursor a read was taken at, as its header names it: on a `304`, which has no body, the
    // one place it is.
    let named = |read: &Response| {
        read.header(symphony_api::VERSION_HEADER)
            .parse::<u64>()
            .unwrap_or_else(|why| panic!("{why}: {read:?}"))
    };
    for (route, before) in documents {
        assert_eq!(serde_json::json!(named(before)), cursor(before), "{route}");
        let now = get(f, route, &[]);
        assert_ne!(cursor(&now), cursor(before), "{route}: the cursor moved");
        assert_eq!(serde_json::json!(named(&now)), cursor(&now), "{route}");
        assert_eq!(now.header("etag"), before.header("etag"), "{route}");
        unchanged(f, "GET", route, before.header("etag"));
        let cached = f
            .request(
                "GET",
                route,
                &[("If-None-Match", before.header("etag"))],
                &[],
            )
            .status(304);
        assert!(
            named(&cached) >= cursor(&now).as_u64().unwrap(),
            "{route}: a 304 names the cursor it was read at: {cached:?}"
        );
    }
    // Every other JSON read names the cursor too, read before its answer was: the board's own
    // version is at it or after it, and a `304` names it as a `200` does.
    let now = get(f, "/api/board", &[]);
    assert!(named(board) < named(&now), "the cursor moved: {now:?}");
    assert!(
        named(&now) <= now.json()["version"].as_u64().unwrap(),
        "a read's cursor is read before its answer: {now:?}"
    );
    let route = format!("/api/task-history?slug={TASK}");
    let thread = get(f, &route, &[]);
    let cached = f
        .request(
            "GET",
            &route,
            &[("If-None-Match", thread.header("etag"))],
            &[],
        )
        .status(304);
    assert!(
        named(&cached) >= named(&thread),
        "{route}: a 304 names the cursor it was read at: {cached:?}"
    );
}
