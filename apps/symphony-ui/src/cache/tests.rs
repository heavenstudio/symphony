use super::*;

const STORE: &str = include_str!("../../assets/cache.js");
const BRIDGE: &str = include_str!("../cache.rs");

#[test]
fn an_item_is_named_by_its_kind_and_scope() {
    for (item, kind, id, pinned) in [
        (Item::board(""), Kind::Board, "board:", false),
        (Item::board("work~x"), Kind::Board, "board:@work~x/", false),
        (Item::task("", "perf/x"), Kind::Task, "task:perf/x", false),
        (
            Item::task("work~x", "perf/x"),
            Kind::Task,
            "task:@work~x/perf/x",
            false,
        ),
    ] {
        assert_eq!(
            item,
            Item {
                kind,
                id: id.into(),
                pinned
            }
        );
    }
}

#[test]
fn items_of_other_kinds_or_scopes_never_share_a_name() {
    let items = [
        Item::board(""),
        Item::board("board"),
        Item::task("", "board:"),
        Item::task("", "x"),
        Item::task("x", "x"),
    ];
    for (at, item) in items.iter().enumerate() {
        for other in &items[at + 1..] {
            assert_ne!(item.id, other.id);
        }
    }
}

/// The store's caps by kind are the approved ones, under the names this side gives its kinds and
/// the web client gives its documents (`cache.ts`): the tasks opened last, the documents weighed by
/// their visits.
#[test]
fn the_stores_caps_are_the_approved_ones_by_this_sides_kind_names() {
    let caps = format!(
        "const CAPS = {{ {}: [30, 2 * MB, false], document: [20, 6 * MB, true] }};",
        Kind::Task.name(),
    );
    for line in [
        caps.as_str(),
        "const ORIGIN = 32 * MB, AGE = 14 * DAY;",
        "const MB = 1e6, DAY = 864e5;",
    ] {
        assert!(STORE.contains(line), "cache.js holds {line}");
    }
    assert!(
        !STORE.contains(&format!("{}: [", Kind::Board.name())),
        "the board's reads have no cap of their own"
    );
    assert!(
        !STORE.contains("PROJECT"),
        "a project's 8 MB is its kinds' caps together, the board outside them"
    );
}

/// A kept value is read as the page's type, keeping its tag and time; one this client cannot read
/// (a shape an older client wrote under the same format) is no value at all.
#[test]
fn a_kept_value_this_client_cannot_read_is_none() {
    let kept = |value: serde_json::Value| Kept {
        value,
        etag: "W/\"aa\"".to_string(),
        saved: 1_000.0,
    };
    let board = serde_json::from_str::<serde_json::Value>(include_str!(
        "../../../../crates/symphony-api/fixtures/board.json"
    ))
    .unwrap();
    let read = kept(board.clone())
        .readable(serde_json::from_value::<symphony_api::Board>)
        .expect("the fixture's board reads");
    assert_eq!((read.etag.as_str(), read.saved), ("W/\"aa\"", 1_000.0));
    assert_eq!(read.value, serde_json::from_value(board.clone()).unwrap());
    let mut renamed = board;
    renamed["cards"] = renamed["counts"].take();
    for unreadable in [
        renamed,
        serde_json::json!(null),
        serde_json::json!("a board"),
        serde_json::json!({}),
    ] {
        assert_eq!(
            kept(unreadable.clone()).readable(serde_json::from_value::<symphony_api::Board>),
            None,
            "{unreadable}"
        );
    }
}

/// The parameter names of the store's function `name` in cache.js, an arrow its object names or a
/// function of the store's own it names as it is.
fn store_parameters(name: &str) -> Vec<&'static str> {
    let list = [format!("{name}: ("), format!("function {name}(")]
        .into_iter()
        .find_map(|head| STORE.find(&head).map(|at| &STORE[at + head.len()..]))
        .unwrap_or_else(|| panic!("cache.js has {name}"));
    list[..list.find(')').unwrap()].split(", ").collect()
}

/// The parameter names of this side's binding to the store's `name`.
fn bridge_parameters(name: &str) -> Vec<&'static str> {
    let at = BRIDGE
        .find(&format!("fn store_{name}("))
        .unwrap_or_else(|| panic!("cache.rs binds {name}"));
    let list = &BRIDGE[at + name.len() + 10..];
    list[..list.find(')').unwrap()]
        .split(',')
        .filter_map(|parameter| parameter.split(':').next())
        .map(str::trim)
        .filter(|parameter| !parameter.is_empty())
        .collect()
}

/// wasm-bindgen passes arguments by position: each binding names the store's in its order.
#[test]
fn each_binding_passes_the_stores_parameters_in_its_order() {
    assert!(
        STORE.contains("        keep,\n"),
        "the store's object names its own keep"
    );
    for name in ["read", "keep", "visit", "forget"] {
        assert_eq!(bridge_parameters(name), store_parameters(name), "{name}");
        assert!(
            BRIDGE.contains(&format!("js_name = {name}, catch")),
            "{name} is bound by its name and never throws into the page"
        );
    }
    assert_eq!(store_parameters("forget"), ["format", "project", "id"]);
}

/// Every first-visit byte of the store is in the shell, which every page kind loads: the store
/// is the shell's first part, and nothing else in the client opens IndexedDB.
#[test]
fn the_shell_carries_the_store_and_no_page_kind_does() {
    let package = include_str!("../../package.sh");
    assert!(
        package.contains("cat assets/cache.js"),
        "package.sh puts the store in the shell"
    );
    assert!(STORE.is_ascii(), "coverage offsets count the store's bytes");
    assert!(
        !BRIDGE.contains("web_sys::Idb"),
        "no IndexedDB binding in Rust"
    );
}

#[test]
fn a_page_drawn_from_the_cache_shows_it_until_the_feed_is_open_and_every_drawn_read_answered() {
    let mut fresh = Freshness::default();
    assert_eq!(fresh.shown(), None, "nothing drawn from the device");
    fresh.drawn("counts", 2_000.0);
    fresh.drawn("column:ready", 1_000.0);
    fresh.drawn("column:ready", 3_000.0);
    assert_eq!(
        fresh.shown(),
        Some((1_000.0, false)),
        "the oldest copy drawn, drawn twice"
    );
    fresh.answered("counts");
    fresh.opened();
    assert_eq!(
        fresh.shown(),
        Some((1_000.0, false)),
        "a drawn column is still unanswered"
    );
    fresh.answered("column:ready");
    assert_eq!(
        fresh.shown(),
        None,
        "every drawn read answered, the feed open"
    );

    let mut fresh = Freshness::default();
    fresh.drawn("board", 5.0);
    fresh.answered("board");
    assert_eq!(
        fresh.shown(),
        Some((5.0, false)),
        "the feed is not open yet"
    );
    fresh.opened();
    assert_eq!(fresh.shown(), None);
    fresh.lost();
    assert_eq!(
        fresh.shown(),
        None,
        "once current, a lost feed is the feed's own stale state"
    );
}

#[test]
fn a_failing_read_says_the_server_is_not_reachable_until_it_answers() {
    let mut fresh = Freshness::default();
    fresh.failed("board");
    assert_eq!(fresh.shown(), None, "a page not drawn from the device");
    fresh.drawn("board", 7.0);
    assert_eq!(
        fresh.shown(),
        Some((7.0, true)),
        "a copy drawn after a read went unanswered"
    );
    fresh.drawn("queue", 9.0);
    fresh.failed("queue");
    fresh.failed("queue");
    fresh.answered("board");
    assert_eq!(
        fresh.shown(),
        Some((7.0, true)),
        "another read's answer: the queue's still fails"
    );
    fresh.answered("queue");
    assert_eq!(
        fresh.shown(),
        Some((7.0, false)),
        "every failing read answered, once each however often it failed: checking"
    );
    fresh.opened();
    assert_eq!(fresh.shown(), None);
}

/// With the feed open the server answers, so a read that fails is asked again and the page says
/// it is checking; once the feed is lost too, it says the server is not reachable.
#[test]
fn a_read_failing_while_the_feed_is_open_is_checking_until_the_feed_is_lost() {
    let mut fresh = Freshness::default();
    fresh.drawn("work", 4.0);
    fresh.drawn("counts", 5.0);
    fresh.answered("counts");
    fresh.opened();
    fresh.failed("work");
    assert_eq!(fresh.shown(), Some((4.0, false)), "the work is asked again");
    fresh.lost();
    assert_eq!(
        fresh.shown(),
        Some((4.0, true)),
        "no feed, and a read fails"
    );
    fresh.opened();
    assert_eq!(fresh.shown(), Some((4.0, false)));
    fresh.answered("work");
    assert_eq!(fresh.shown(), None);
    fresh.failed("work");
    assert_eq!(
        fresh.shown(),
        None,
        "once current, nothing is shown from the device"
    );
}

#[test]
fn a_read_drawn_after_the_page_was_current_shows_until_it_is_answered() {
    let mut fresh = Freshness::default();
    fresh.failed("board");
    fresh.opened();
    assert_eq!(fresh.shown(), None);
    fresh.answered("never drawn");
    assert_eq!(fresh.shown(), None);
    fresh.drawn("queue", 4.0);
    assert_eq!(
        fresh.shown(),
        Some((4.0, false)),
        "the feed opened since the read went unanswered"
    );
    fresh.answered("queue");
    assert_eq!(fresh.shown(), None);
}

#[test]
fn a_part_the_page_dropped_is_no_longer_shown_and_says_nothing_of_the_server() {
    let mut fresh = Freshness::default();
    fresh.drawn("counts", 3.0);
    fresh.drawn("column:ready", 2.0);
    fresh.failed("counts");
    fresh.failed("column:ready");
    fresh.gone("column:ready");
    assert_eq!(
        fresh.shown(),
        Some((2.0, true)),
        "the counts still wait, and their read still fails"
    );
    fresh.gone("never drawn");
    assert_eq!(fresh.shown(), Some((2.0, true)));
    fresh.answered("counts");
    assert_eq!(
        fresh.shown(),
        Some((2.0, false)),
        "the column dropped is no read failing"
    );
    fresh.opened();
    assert_eq!(fresh.shown(), None);
    fresh.drawn("column:review", 6.0);
    fresh.gone("column:review");
    assert_eq!(fresh.shown(), None, "the feed open, nothing left drawn");
}

#[test]
fn the_task_board_keeps_its_default_view_only() {
    assert!(board::kept_view("", 1, 1));
    for (query, days, activity) in [("x", 1, 1), ("", 7, 1), ("", 1, 0), (" ", 1, 1)] {
        assert!(
            !board::kept_view(query, days, activity),
            "{query:?} {days} {activity}"
        );
    }
    assert_eq!(board::column("ready"), "column:ready");
    let columns = [board::column("board"), board::column("")];
    let parts = [
        board::BOARD,
        board::QUEUE,
        board::COUNTS,
        board::WORK,
        &columns[0],
        &columns[1],
    ];
    for (at, part) in parts.iter().enumerate() {
        assert!(!parts[at + 1..].contains(part), "{part} is one part");
    }
}

#[test]
fn the_task_boards_kept_columns_are_its_statuses_and_the_stages_it_adds() {
    let named = |names: &[&str]| {
        names
            .iter()
            .map(|name| name.to_string())
            .collect::<Vec<_>>()
    };
    let counts = vec![("ready".to_string(), 2), ("done".to_string(), 0)];
    assert_eq!(
        board::columns(&named(&["ready", "build", "review"]), &counts),
        named(&["ready", "build", "review", "test", "signoff"]),
        "the listing's statuses, not its counts"
    );
    assert_eq!(
        board::columns(&[], &counts),
        named(&["ready", "done", "build", "test", "signoff"]),
        "a listing that names none: its counts'"
    );
    assert_eq!(
        board::columns(&named(&["ready", "ready", "signoff"]), &[]),
        named(&["ready", "signoff", "build", "test"]),
        "once each"
    );
    assert_eq!(
        board::columns(&[], &[]),
        named(&["build", "test", "signoff"])
    );
}

/// A message of the thread, `id`, saying `text`.
fn said(id: i64, text: &str) -> symphony_api::TaskEntry {
    symphony_api::TaskEntry {
        message: symphony_api::Message {
            id,
            slug: "perf/x".into(),
            sender: "calm".into(),
            recipient: None,
            kind: "message".into(),
            text: text.into(),
            ts: id as f64,
        },
        stamp: format!("{id}"),
        blocks: Vec::new(),
    }
}

fn texts(entries: &[symphony_api::TaskEntry]) -> Vec<(i64, &str)> {
    entries
        .iter()
        .map(|entry| (entry.message.id, entry.message.text.as_str()))
        .collect()
}

#[test]
fn a_task_keeps_its_page_and_its_threads_newest_page_apart_from_the_boards_parts() {
    assert_ne!(task::PAGE, task::THREAD);
    for part in [task::PAGE, task::THREAD] {
        assert!(
            ![board::BOARD, board::QUEUE, board::COUNTS, board::WORK].contains(&part),
            "{part}"
        );
    }
}

#[test]
fn the_threads_newest_page_replaces_what_this_device_drew_and_keeps_what_the_feed_brought() {
    // Drawn from the device: 3, 4, 5; the feed brought 7 meanwhile.
    let mut entries = vec![
        said(3, "three"),
        said(4, "four"),
        said(5, "five"),
        said(7, "seven"),
    ];
    task::newest(
        &mut entries,
        &[3, 4, 5],
        vec![
            said(4, "four, drawn anew"),
            said(6, "six"),
            said(7, "seven"),
        ],
    );
    assert_eq!(
        texts(&entries),
        [(4, "four, drawn anew"), (6, "six"), (7, "seven")],
        "3 fell off the newest page, read again as an earlier one; 5 is not there; 7 once"
    );

    let mut entries = vec![said(9, "nine"), said(2, "two")];
    task::newest(&mut entries, &[], vec![said(5, "five")]);
    assert_eq!(
        texts(&entries),
        [(2, "two"), (5, "five"), (9, "nine")],
        "nothing drawn from the device: every shown message stays, in id order"
    );

    let mut entries = vec![said(1, "one")];
    task::newest(&mut entries, &[1], Vec::new());
    assert!(entries.is_empty(), "a thread now empty shows nothing drawn");
}

#[test]
fn a_page_that_opens_another_route_no_longer_shows_what_it_drew_for_the_one_it_left() {
    let mut fresh = Freshness::default();
    fresh.drawn(task::PAGE, 3.0);
    fresh.drawn(task::THREAD, 2.0);
    fresh.failed(task::PAGE);
    fresh.failed(board::BOARD);
    assert!(fresh.waits(task::PAGE) && fresh.waits(task::THREAD));
    assert!(!fresh.waits(board::BOARD), "failed, never drawn");
    fresh.left(&[task::PAGE, task::THREAD]);
    assert!(!fresh.waits(task::PAGE) && !fresh.waits(task::THREAD));
    assert_eq!(
        fresh.shown(),
        None,
        "nothing drawn left, though the feed is not open"
    );
    fresh.drawn(task::PAGE, 8.0);
    assert_eq!(
        fresh.shown(),
        Some((8.0, true)),
        "the next task's copy, kept later than the one left; the board's read still fails"
    );

    let mut fresh = Freshness::default();
    fresh.drawn(board::COUNTS, 1.0);
    fresh.drawn(task::PAGE, 2.0);
    fresh.left(&[task::PAGE]);
    assert_eq!(
        fresh.shown(),
        Some((1.0, false)),
        "what is still drawn stays, saved when it was"
    );
    fresh.answered(task::PAGE);
    fresh.answered(board::COUNTS);
    fresh.opened();
    assert_eq!(fresh.shown(), None);
    fresh.left(&[task::PAGE]);
    assert_eq!(fresh.shown(), None, "nothing to leave");
}
