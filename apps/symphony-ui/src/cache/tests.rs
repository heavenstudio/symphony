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
        (
            Item::document("docs/X.md", false),
            Kind::Document,
            "document:docs/X.md",
            false,
        ),
        (
            Item::document("@work~x/docs/", true),
            Kind::Document,
            "document:@work~x/docs/",
            true,
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
        Item::document("x", false),
        Item::document("task:x", false),
    ];
    for (at, item) in items.iter().enumerate() {
        for other in &items[at + 1..] {
            assert_ne!(item.id, other.id);
        }
    }
}

/// The store's caps by kind are the approved ones, under the names this side gives its kinds: the
/// tasks opened last, the documents weighed by their visits.
#[test]
fn the_stores_caps_are_the_approved_ones_by_this_sides_kind_names() {
    let caps = format!(
        "const CAPS = {{ {}: [30, 2 * MB, false], {}: [20, 6 * MB, true] }};",
        Kind::Task.name(),
        Kind::Document.name()
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

/// A section `key` of `hash`, its body one paragraph saying `text` (none drawn when empty).
fn section(key: &str, hash: &str, text: &str) -> symphony_api::Section {
    let blocks = if text.is_empty() {
        serde_json::json!([])
    } else {
        serde_json::json!([{"type": "paragraph", "spans": [{"type": "text", "text": text}]}])
    };
    serde_json::from_value(serde_json::json!({
        "start": 1, "end": 2, "key": key, "hash": hash, "part": null, "blocks": blocks
    }))
    .unwrap()
}

/// A document of `sections` (`(key, hash, text)`), its card `palette` with a thread saying `said`.
fn document_of(sections: &[(&str, &str, &str)], said: &str) -> symphony_api::Document {
    let mut doc: symphony_api::Document = serde_json::from_value(serde_json::json!({
        "version": 7, "path": "docs/DECISIONS.md", "title": "Decisions", "waiting": [],
        "crumbs": [], "headings": [], "sections": [],
        "cards": [{"slug": "palette", "state": "pending", "text": "Palette, whole", "group": "decide", "open": true}],
        "threads": {"palette": {"blocks": [{"type": "paragraph", "spans": [{"type": "text", "text": said}]}], "html": "", "media": []}},
        "tasks": []
    }))
    .unwrap();
    doc.sections = sections
        .iter()
        .map(|(key, hash, text)| section(key, hash, text))
        .collect();
    doc
}

/// The server's outline of `document`: no bodies, its card's thread by the hash `thread`.
fn outline_of(document: &symphony_api::Document, thread: &str) -> symphony_api::DocumentOutline {
    let mut document = document.clone();
    for section in &mut document.sections {
        section.blocks.clear();
    }
    document.threads.clear();
    symphony_api::DocumentOutline {
        document,
        anchors: Default::default(),
        thread_hashes: [("palette".to_string(), thread.to_string())].into(),
    }
}

fn keyset(keys: &[&str]) -> std::collections::BTreeSet<String> {
    keys.iter().map(|key| key.to_string()).collect()
}

const DOCUMENT: [(&str, &str, &str); 3] = [
    ("intro", "i1", "Intro"),
    ("card:palette", "p1", "Palette"),
    ("lamps", "l1", "Lamps"),
];

/// Every section of [`DOCUMENT`], as a first visit reads it.
fn all() -> std::collections::BTreeSet<String> {
    keyset(&["intro", "card:palette", "lamps"])
}

/// The bodies a first visit to `read` leaves taken: each read under the outline's validators, its
/// card's thread by the hash `thread`.
fn first_visit(read: &symphony_api::Document, thread: &str) -> document::Bodies {
    let mut bodies = document::Bodies::default();
    bodies.answered(&outline_of(read, thread), &keyset(&[]));
    bodies.read(&all(), &read.sections);
    bodies
}

#[test]
fn a_sections_validator_is_its_hash_a_cards_its_hash_and_threads_and_a_difference_has_none() {
    let threads = [("palette".to_string(), "t1".to_string())].into();
    assert_eq!(
        document::validator(&section("intro", "i1", ""), &threads).as_deref(),
        Some("i1")
    );
    assert_eq!(
        document::validator(&section("card:palette", "p1", ""), &threads).as_deref(),
        Some("p1/t1")
    );
    assert_eq!(
        document::validator(&section("card:bare", "b1", ""), &threads).as_deref(),
        Some("b1/"),
        "a card with no thread"
    );
    let mut differs = section("changed", "c1", "");
    differs.comparison =
        Some(serde_json::from_value(serde_json::json!({"kind": "changed", "before": []})).unwrap());
    assert_eq!(document::validator(&differs, &threads), None);
    let mut doc = document_of(&DOCUMENT, "Said");
    doc.sections.push(differs);
    let outline = symphony_api::DocumentOutline {
        document: doc,
        anchors: Default::default(),
        thread_hashes: threads,
    };
    assert_eq!(
        document::validators(&outline),
        [
            ("card:palette".to_string(), "p1/t1".to_string()),
            ("intro".to_string(), "i1".to_string()),
            ("lamps".to_string(), "l1".to_string()),
        ]
        .into(),
        "a section showing a difference is never kept"
    );
}

#[test]
fn bodies_drawn_from_the_device_are_those_current_by_the_outline_kept_with_them() {
    let read = document_of(&DOCUMENT, "Said");
    let mut kept = first_visit(&read, "t1").kept(&read);
    assert_eq!(kept.len(), 3, "{kept:#?}");
    assert_eq!(kept["card:palette"].validator, "p1/t1");
    assert_eq!(
        kept["card:palette"]
            .body
            .card
            .as_ref()
            .map(|c| c.text.as_str()),
        Some("Palette, whole")
    );
    assert_eq!(
        kept["card:palette"].body.thread.as_ref(),
        read.threads.get("palette")
    );
    // A body kept under an older validator, or under another key than its section's, is not drawn.
    kept.get_mut("lamps").unwrap().validator = "l0".into();
    let misplaced = kept["intro"].clone();
    kept.insert("other".into(), misplaced);
    let mut device = document::Bodies::default();
    let drawn = device.drawn(&outline_of(&read, "t1"), kept);
    assert_eq!(
        drawn
            .iter()
            .map(|body| body.section.key.as_str())
            .collect::<Vec<_>>(),
        ["card:palette", "intro"]
    );
    assert!(!device.confirmed(), "drawn, not yet confirmed");
    assert!(
        document::Bodies::default()
            .drawn(&outline_of(&read, "t1"), Default::default())
            .is_empty()
    );
}

#[test]
fn the_servers_outline_keeps_the_bodies_it_names_alike_and_leaves_the_rest_to_be_read() {
    let read = document_of(&DOCUMENT, "Said");
    let kept = first_visit(&read, "t1").kept(&read);
    // Since: intro was edited, and the card's thread answered; lamps is as it was.
    let now = document_of(
        &[("intro", "i2", "Intro, edited"), DOCUMENT[1], DOCUMENT[2]],
        "Said, answered",
    );
    let wanted = keyset(&["intro", "card:palette"]);
    let device = |wanted: &std::collections::BTreeSet<String>| {
        let mut device = document::Bodies::default();
        device.drawn(&outline_of(&read, "t1"), kept.clone());
        let current = device.answered(&outline_of(&now, "t2"), wanted);
        (device, current)
    };
    let (waiting, current) = device(&wanted);
    assert_eq!(current, keyset(&["lamps"]));
    assert!(
        !waiting.confirmed(),
        "intro and the card are wanted, and wait for their reads"
    );
    let (unwanted, _) = device(&keyset(&[]));
    assert!(
        unwanted.confirmed(),
        "a body drawn from the device that the page no longer wants is not waited for"
    );

    // The two read again: taken under the server's validators, and every drawn one confirmed.
    let (mut read_again, _) = device(&wanted);
    assert!(read_again.read(&wanted, &now.sections[..2]));
    assert!(read_again.confirmed());
    assert_eq!(
        read_again
            .kept(&now)
            .iter()
            .map(|(key, kept)| (key.as_str(), kept.validator.as_str()))
            .collect::<Vec<_>>(),
        [("card:palette", "p1/t2"), ("intro", "i2"), ("lamps", "l1")]
    );
    assert!(
        !read_again.read(&wanted, &now.sections[..2]),
        "the same bodies read again change nothing"
    );
    // A section asked for and not answered is gone: nothing of it is kept.
    assert!(read_again.read(&keyset(&["lamps"]), &[]));
    assert_eq!(read_again.kept(&now).len(), 2);
}

#[test]
fn a_body_the_feed_changed_is_not_kept_until_the_next_outline_names_it() {
    let read = document_of(&DOCUMENT, "Said");
    let mut bodies = first_visit(&read, "t1");
    // A reply on the card's thread, and lamps edited.
    let mut delta = symphony_api::DocumentChange {
        path: read.path.clone(),
        ..Default::default()
    };
    delta.threads.insert("palette".into(), None);
    delta.sections.push(section("lamps", "l2", ""));
    assert!(bodies.changed(&delta));
    assert_eq!(
        bodies.kept(&read).keys().collect::<Vec<_>>(),
        ["intro"],
        "only the body the feed left alone"
    );
    // Lamps read again before the next outline: no validator names it, so it is not kept.
    let edited = document_of(
        &[DOCUMENT[0], DOCUMENT[1], ("lamps", "l2", "Lamps, edited")],
        "Said",
    );
    assert!(!bodies.read(&keyset(&["lamps"]), &edited.sections[2..]));
    assert_eq!(bodies.kept(&edited).keys().collect::<Vec<_>>(), ["intro"]);
    // A card changed by its card alone, or a section deleted, is as changed.
    let mut delta = symphony_api::DocumentChange::default();
    delta.cards.push(symphony_api::ItemChange {
        key: "palette".into(),
        value: None,
        removed: true,
    });
    delta.deleted.push("intro".into());
    assert!(bodies.changed(&delta));
    assert!(bodies.kept(&edited).is_empty());
    assert!(!bodies.changed(&delta), "nothing more to change");
    // The next outline names them: read again, they are kept.
    bodies.answered(&outline_of(&edited, "t1"), &keyset(&[]));
    assert!(bodies.read(&all(), &edited.sections));
    assert_eq!(bodies.kept(&edited).len(), 3);
}

#[test]
fn the_device_is_given_the_bodies_again_only_once_one_is_taken_or_let_go() {
    let read = document_of(&DOCUMENT, "Said");
    let mut first = first_visit(&read, "t1");
    assert_eq!(
        first.unkept(),
        Some(
            [
                ("card:palette".to_string(), "p1/t1".to_string()),
                ("intro".to_string(), "i1".to_string()),
                ("lamps".to_string(), "l1".to_string()),
            ]
            .into()
        ),
        "each body under its validator"
    );
    assert_eq!(first.unkept(), None, "given already");

    // The next visit draws them and the server's outline names them alike: nothing to give.
    let mut device = document::Bodies::default();
    device.drawn(&outline_of(&read, "t1"), first.kept(&read));
    assert_eq!(device.unkept(), None, "drawn from the device");
    device.answered(&outline_of(&read, "t1"), &all());
    assert_eq!(device.unkept(), None, "every body confirmed");

    // A body the feed changed is not given: the next outline tells its copy out of date.
    let mut delta = symphony_api::DocumentChange::default();
    delta.sections.push(section("lamps", "l2", ""));
    device.changed(&delta);
    assert_eq!(device.unkept(), None);

    // An outline naming another validator lets a body go; a read takes one.
    let edited = document_of(
        &[("intro", "i2", "Intro, edited"), DOCUMENT[1], DOCUMENT[2]],
        "Said",
    );
    device.answered(&outline_of(&edited, "t1"), &all());
    let given = device.unkept().expect("intro let go");
    assert_eq!(given.keys().collect::<Vec<_>>(), ["card:palette"]);
    device.read(&keyset(&["intro"]), &edited.sections[..1]);
    let given = device.unkept().expect("intro taken");
    assert_eq!(given.keys().collect::<Vec<_>>(), ["card:palette", "intro"]);
    assert_eq!(given["intro"], "i2");
}

#[test]
fn a_kept_body_reads_back_as_kept_and_one_of_another_shape_does_not() {
    let read = document_of(&DOCUMENT, "Said");
    let kept = first_visit(&read, "t1").kept(&read);
    let json = serde_json::to_value(&kept).unwrap();
    // The keys of the body this client keeps around the wire's types, whose shapes the wire
    // crate's cache format pins (crates/symphony-api/tests/wire/cache_format.rs): a change here
    // lands with a new `symphony_api::CACHE_FORMAT`. The page writes a kept body under the names
    // the module gives (docs_live.rs, `keep_bodies`), and reads it as this type: the same.
    let keys = |value: &serde_json::Value| {
        let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    };
    assert_eq!(
        keys(&json["card:palette"]),
        [document::BODY, document::VALIDATOR]
    );
    assert_eq!(
        keys(&json["card:palette"]["body"]),
        [document::CARD, document::SECTION, document::THREAD]
    );
    assert_eq!(
        keys(&json["intro"]["body"]),
        [document::SECTION],
        "a section's body names no card or thread: {json}"
    );
    let back: std::collections::BTreeMap<String, document::KeptBody> =
        serde_json::from_value(json).unwrap();
    assert_eq!(back, kept);
    assert!(
        serde_json::from_value::<std::collections::BTreeMap<String, document::KeptBody>>(
            serde_json::json!({"intro": {"validator": "i1"}})
        )
        .is_err()
    );
}

/// The first part of [`DOCUMENT`] a page read ahead (`view=first`): the outline's head, the
/// leading sections `keys` whole, the card's thread when it is among them and its hash `thread`.
fn first_part(
    read: &symphony_api::Document,
    keys: &[&str],
    thread: &str,
) -> symphony_api::DocumentOutline {
    let mut first = outline_of(read, thread);
    first.document.sections = read
        .sections
        .iter()
        .filter(|section| keys.contains(&section.key.as_str()))
        .cloned()
        .collect();
    if keys.contains(&"card:palette") {
        first.document.threads = read.threads.clone();
    } else {
        first.thread_hashes.clear();
        first.document.cards.clear();
    }
    first
}

#[test]
fn a_document_read_ahead_draws_as_the_bodies_a_visit_would_have_kept() {
    let read = document_of(&DOCUMENT, "Said");
    let first = first_part(&read, &["intro", "card:palette"], "t1");
    assert!(document::read_ahead(&first));
    assert!(
        !document::read_ahead(&outline_of(&read, "t1")),
        "an outline carries no body"
    );
    let (outline, kept) = document::split(first);
    assert!(!document::read_ahead(&outline), "its bodies taken out");
    assert_eq!(
        outline
            .document
            .sections
            .iter()
            .map(|section| section.key.as_str())
            .collect::<Vec<_>>(),
        ["intro", "card:palette"],
        "the outline of its first part, no further"
    );
    assert_eq!(
        kept,
        first_visit(&read, "t1")
            .kept(&read)
            .into_iter()
            .filter(|(key, _)| key != "lamps")
            .collect(),
        "each body under the validator a visit would have kept it under"
    );
    // Drawn from the device as a visit's: current by the outline the part was split into.
    let mut device = document::Bodies::default();
    assert_eq!(device.drawn(&outline, kept.clone()).len(), 2);
    // Drawn as the device keeps a visit's, the bodies are kept already: nothing to keep again.
    device.answered(&outline_of(&read, "t1"), &keyset(&[]));
    assert_eq!(device.unkept(), None);
    // Drawn from a part read ahead, they are kept in its outline alone, which the visit's outline
    // replaces: once it answers, those still current are the device's to keep, under their
    // validators, and the one it no longer names is not.
    let mut ahead = document::Bodies::default();
    ahead.drawn(&outline, kept);
    ahead.drawn_ahead();
    let mut edited = outline_of(&read, "t1");
    assert_eq!(edited.document.sections[0].key, "intro");
    edited.document.sections[0].hash = "i2".into();
    ahead.answered(&edited, &keyset(&[]));
    assert_eq!(
        ahead.unkept(),
        Some([("card:palette".to_string(), "p1/t1".to_string())].into()),
        "the card, current; the intro edited since"
    );
    assert_eq!(ahead.unkept(), None, "given once");
    // A part with no card and no body of any section is no part read ahead.
    let empty = first_part(
        &document_of(&[("intro", "i1", "")], "Said"),
        &["intro"],
        "t1",
    );
    assert!(!document::read_ahead(&empty));
}
