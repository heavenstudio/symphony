//! The copy of a whole board on the engine the board runs on, a scratch home's `surrealkv://`: a
//! generated board about a quarter the size of the live one on 2026-11-04 (11000 messages, 8250
//! deliveries), in `Store::copy`'s one transaction. Each row's `board_changed` event used to cost
//! time in proportion to everything the transaction had written before it (a quarter of that day's
//! board took 26 s against 3.9 s, the whole of it outlasted the client's 5 min read); the copy
//! still succeeds or changes nothing.

use crate::common::Scratch;
use crate::store::{NOW, session, task};
use serde_json::{Value, json};
use symphony_board::Store;
use symphony_board::rows::{
    DeliveryRow, Dump, LinkRow, MessageRow, RoleRow, TaskRow, TaskSessionRow,
};
use symphony_board::rpc::Client;
use symphony_board::server::Storage;

const TASKS: usize = 480;
const SESSIONS: usize = 560;
const MESSAGES: usize = 11_000;
/// One delivery each for the first 8250 messages.
const DELIVERIES: usize = 8_250;

fn slug(n: usize) -> String {
    format!("area/task-{n:04}")
}

fn handle(n: usize) -> String {
    format!("handle-{n:04}")
}

/// `count` words drawn from a vocabulary of 4000 by `seed`: text the full-text index takes term by
/// term, as it takes a thread's.
fn words(seed: usize, count: usize) -> String {
    let mut state = seed as u64;
    (0..count)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            format!("w{}", (state >> 33) % 4000)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every table of a board, each in the order its dump reads it back.
fn quarter_board() -> Dump {
    let tasks = (0..TASKS)
        .map(|n| TaskRow {
            id: n as i64 + 1,
            body: words(n, 200),
            ..task(&slug(n), "ready")
        })
        .collect();
    let task_links = (1..TASKS)
        .map(|n| LinkRow {
            slug: slug(n),
            kind: "dep".into(),
            target: slug(n - 1),
        })
        .collect();
    let sessions = (0..SESSIONS).map(|n| session(&handle(n))).collect();
    let mut roles: Vec<RoleRow> = (0..TASKS)
        .flat_map(|n| {
            [
                (handle(n % SESSIONS), "executor"),
                (handle((n + 7) % SESSIONS), "reviewer"),
            ]
            .map(|(handle, role)| RoleRow {
                slug: slug(n),
                handle,
                role: role.into(),
                since: NOW,
                expires: None,
            })
        })
        .collect();
    roles.sort_by(|a, b| (&a.slug, &a.handle, &a.role).cmp(&(&b.slug, &b.handle, &b.role)));
    let messages = (0..MESSAGES)
        .map(|n| MessageRow {
            id: n as i64 + 1,
            slug: slug(n % TASKS),
            sender: handle(n % SESSIONS),
            recipient: (n % 3 == 0).then(|| "@orchestrator".into()),
            kind: "msg".into(),
            text: words(TASKS + n, 60),
            ts: NOW + n as f64,
            person: false,
        })
        .collect();
    let deliveries = (0..DELIVERIES)
        .map(|n| DeliveryRow {
            message: n as i64 + 1,
            handle: handle(n % SESSIONS),
            acked: NOW + n as f64,
        })
        .collect();
    let task_sessions = (0..200)
        .map(|n| TaskSessionRow {
            slug: slug(n),
            handle: handle(n),
            role: "executor".into(),
            claude_session: format!("c-{n}"),
            worktree: format!("/w/{n}"),
            branch: slug(n),
            pane: format!("p{n}"),
            model: "m".into(),
            started: NOW,
            closed: None,
            closed_why: String::new(),
            restore: String::new(),
        })
        .collect();
    Dump {
        tasks,
        task_links,
        sessions,
        roles,
        messages,
        deliveries,
        barred_keys: Vec::new(),
        task_sessions,
        delivery_keys: Vec::new(),
        asks: Vec::new(),
        usages: Vec::new(),
        ..Dump::default()
    }
}

/// The rows whose writes advance the combined marker: every table's but the barred keys'.
fn event_rows(dump: &Dump) -> usize {
    dump.tasks.len()
        + dump.task_links.len()
        + dump.sessions.len()
        + dump.roles.len()
        + dump.messages.len()
        + dump.deliveries.len()
        + dump.task_sessions.len()
}

fn board(s: &Scratch) -> Store {
    let running = s.home().ensure().unwrap();
    Store::open(Client::new(&running.addr, "sf", "tasks")).unwrap()
}

fn one(store: &Store, sql: &str) -> Value {
    store.client().query(sql, json!({})).unwrap().remove(0)
}

/// The board's tables, staging tables among them.
fn tables(store: &Store) -> Vec<String> {
    let found = one(store, "RETURN object::keys((INFO FOR DB).tables);");
    serde_json::from_value(found).unwrap()
}

#[test]
fn a_quarter_of_the_live_board_copies_whole_on_its_engine() {
    let s = Scratch::new("copy-whole");
    let store = board(&s);
    let dump = quarter_board();
    store.copy(&dump).unwrap();
    assert_eq!(store.dump().unwrap(), dump);
    assert_eq!(
        one(
            &store,
            &format!("RETURN {};", symphony_board::schema::BOARD_MARKER)
        ),
        json!(event_rows(&dump)),
        "each row's write advances the board's marker once"
    );
    assert!(
        tables(&store).iter().all(|t| !t.starts_with("copy_")),
        "{:?}",
        tables(&store)
    );
}

#[test]
fn a_refused_copy_of_a_quarter_board_leaves_the_board_and_its_index_as_they_were() {
    let s = Scratch::new("copy-refused");
    let store = board(&s);
    let small = Dump {
        tasks: vec![task("sim/jump", "ready")],
        sessions: vec![session("amber-heron")],
        ..Dump::default()
    };
    store.copy(&small).unwrap();
    store
        .client()
        .query(
            "CREATE task_chunk SET slug = 'sim/jump', source = 'brief', source_hash = 'old', text_hash = 'old', model = 'fake', text = 'cached board text', embedding = $vector;",
            json!({"vector": vec![1.0_f32; 384]}),
        )
        .unwrap();
    let marker = one(
        &store,
        &format!("RETURN {};", symphony_board::schema::BOARD_MARKER),
    );
    let mut bad = quarter_board();
    bad.messages.last_mut().unwrap().kind = "shout".into();
    match store.copy(&bad) {
        Err(symphony_board::Error::Db(why)) => assert!(why.contains("shout"), "{why}"),
        other => panic!("expected the server to refuse the copy, got {other:?}"),
    }
    assert_eq!(store.dump().unwrap(), small);
    assert_eq!(
        json!(store.raw().table("task_chunk").count().unwrap()),
        json!(1)
    );
    assert_eq!(
        one(
            &store,
            &format!("RETURN {};", symphony_board::schema::BOARD_MARKER)
        ),
        marker
    );
    assert!(
        tables(&store).iter().all(|t| !t.starts_with("copy_")),
        "{:?}",
        tables(&store)
    );
}

/// A copy larger than its server's memtable, which the server does not commit: the store says so
/// in one line, with the rows and bytes it staged and the setting, and the board is as it was,
/// nothing staged left behind.
#[test]
fn a_copy_larger_than_its_servers_memtable_is_refused_in_one_line_and_leaves_the_board() {
    let s = Scratch::new("copy-unfit");
    let small_memtable = Storage {
        memtable: 4 << 20,
        ..Storage::BOARD
    };
    let running = s.home().storage(small_memtable).ensure().unwrap();
    let store = Store::open(Client::new(&running.addr, "sf", "tasks")).unwrap();
    let small = Dump {
        tasks: vec![task("sim/jump", "ready")],
        sessions: vec![session("amber-heron")],
        ..Dump::default()
    };
    store.copy(&small).unwrap();
    let generation = store.recovery_generation().unwrap();
    let why = match store.copy(&quarter_board()) {
        Err(symphony_board::Error::Db(why)) => why,
        other => panic!("expected the server not to commit the copy, got {other:?}"),
    };
    assert!(
        why.starts_with(
            "the server did not commit the copy's one transaction, and the board is as it was: \
             the copy staged "
        ),
        "{why}"
    );
    for said in [
        format!("{TASKS} of task"),
        format!("{MESSAGES} of message"),
        "SURREAL_SURREALKV_MAX_MEMTABLE_SIZE".to_string(),
    ] {
        assert!(why.contains(&said), "{why}");
    }
    assert!(!why.contains('\n'), "{why}");
    assert_eq!(store.dump().unwrap(), small);
    assert_eq!(store.recovery_generation().unwrap(), generation);
    assert!(
        tables(&store).iter().all(|t| !t.starts_with("copy_")),
        "{:?}",
        tables(&store)
    );
}
