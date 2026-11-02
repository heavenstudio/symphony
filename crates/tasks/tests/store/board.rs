//! The reads: the whole board in one round trip however big it is, tasks by priority then creation,
//! each with its links and holders.

use crate::common::{Fixture, NOW, RawWrite, Seed, refused, session};
use tasks::Health;
use tasks::health::LIVE_SECONDS;
use tasks::rows::{MessageRow, SessionRow, TaskRow};

fn a_board_of(tasks: usize) -> Seed {
    let mut seed = Seed::default()
        .session("amber-heron")
        .session("brisk-finch");
    for i in 0..tasks {
        seed = seed.task(&format!("sim/t{i:03}"), "ready").role(
            &format!("sim/t{i:03}"),
            "amber-heron",
            "orchestrator",
        );
        if i % 2 == 0 {
            seed = seed.link(&format!("sim/t{i:03}"), "doc", "docs/game/SPEC.md");
        }
    }
    seed
}

#[test]
fn the_board_is_one_round_trip_however_big_it_is() {
    for size in [1, 40] {
        let f = Fixture::new(a_board_of(size));
        let before = f.store.client().requests();
        let board = f.store.board().unwrap();
        assert_eq!(f.store.client().requests() - before, 1, "{size} tasks");
        assert_eq!(board.tasks.len(), size);
        assert_eq!(board.sessions.len(), 2);
    }
}

#[test]
fn a_task_read_is_one_round_trip_too() {
    let f = Fixture::new(a_board_of(5));
    let before = f.store.client().requests();
    f.store.get("sim/t003").unwrap();
    f.store.tasks(Some("ready")).unwrap();
    f.store.session("amber-heron").unwrap();
    assert_eq!(f.store.client().requests() - before, 3);
}

#[test]
fn tasks_come_by_priority_then_the_order_they_were_made_in() {
    let at = |slug: &str, id: i64, priority: i64| TaskRow {
        id,
        priority,
        ..crate::common::task(slug, "ready")
    };
    let f = Fixture::new(
        Seed::default()
            .task_row(at("z/first", 1, 2))
            .task_row(at("a/second", 2, 2))
            .task_row(at("m/urgent", 3, 0))
            .task_row(at("b/later", 4, 3)),
    );
    let slugs: Vec<String> = f
        .store
        .board()
        .unwrap()
        .tasks
        .into_iter()
        .map(|t| t.slug)
        .collect();
    assert_eq!(slugs, ["m/urgent", "z/first", "a/second", "b/later"]);
    let ids: Vec<i64> = f
        .store
        .tasks(None)
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(ids, [3, 1, 2, 4]);
}

#[test]
fn tasks_by_status_are_only_those() {
    let f = Fixture::new(
        Seed::default()
            .task("sim/a", "ready")
            .task("sim/b", "done")
            .task("sim/c", "ready"),
    );
    let ready: Vec<String> = f
        .store
        .tasks(Some("ready"))
        .unwrap()
        .into_iter()
        .map(|t| t.slug)
        .collect();
    assert_eq!(ready, ["sim/a", "sim/c"]);
    assert!(f.store.tasks(Some("review")).unwrap().is_empty());
}

#[test]
fn a_task_carries_its_fields_its_links_and_its_holders_with_their_health() {
    let row = TaskRow {
        id: 2,
        body: "Make it jump.\n\nTwice.".into(),
        priority: 1,
        parent: Some("sim".into()),
        updated: NOW + 3.0,
        ..crate::common::task("sim/jump", "in_progress")
    };
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .session_row(SessionRow {
                last_seen: NOW - LIVE_SECONDS - 1.0,
                ..session("brisk-finch")
            })
            .task("sim", "ready")
            .task_row(row)
            .task("sim/land", "done")
            .link("sim/jump", "doc", "docs/game/SPEC.md#Jumping")
            .link("sim/jump", "dep", "sim/land")
            .link("sim/jump", "doc", "docs/engineering/DECK.md")
            .role("sim/jump", "amber-heron", "executor")
            .role("sim/jump", "brisk-finch", "reviewer"),
    );
    let task = f.store.get("sim/jump").unwrap();
    assert_eq!(
        (
            task.id,
            task.title.as_str(),
            task.body.as_str(),
            task.priority,
            task.parent.as_deref(),
            task.created,
            task.updated
        ),
        (
            2,
            "Task sim/jump",
            "Make it jump.\n\nTwice.",
            1,
            Some("sim"),
            NOW,
            NOW + 3.0
        )
    );
    assert_eq!(
        task.links,
        ["docs/engineering/DECK.md", "docs/game/SPEC.md#Jumping"],
        "doc links, by target"
    );
    assert_eq!(task.depends, ["sim/land"]);
    let held: Vec<(&str, &str, Health)> = task
        .roles
        .iter()
        .map(|r| (r.handle.as_str(), r.role.as_str(), r.health))
        .collect();
    assert_eq!(
        held,
        [
            ("amber-heron", "executor", Health::Live),
            ("brisk-finch", "reviewer", Health::Stale)
        ],
        "by since, then handle"
    );
    assert!(!task.instruction);
    assert_eq!(
        f.store
            .board()
            .unwrap()
            .tasks
            .into_iter()
            .find(|t| t.slug == "sim/jump")
            .unwrap(),
        task,
        "the board reads it the same"
    );
}

#[test]
fn a_reservation_without_an_end_is_held_until_its_holder_is_gone() {
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .task("sim/jump", "ready")
            .reserved("sim/jump", "amber-heron", None),
    );
    assert_eq!(
        f.store.get("sim/jump").unwrap().holders("reserved"),
        ["amber-heron"]
    );
    f.advance(1e7);
    assert_eq!(
        f.store.get("sim/jump").unwrap().roles[0].health,
        Health::Left,
        "shown, but its holder has gone"
    );
}

#[test]
fn sessions_come_by_when_they_connected_then_handle() {
    let at = |handle: &str, connected: f64| SessionRow {
        connected,
        ..session(handle)
    };
    let f = Fixture::new(
        Seed::default()
            .session_row(at("zinc-moth", NOW - 5.0))
            .session_row(at("brisk-finch", NOW))
            .session_row(at("amber-heron", NOW)),
    );
    let handles: Vec<String> = f
        .store
        .board()
        .unwrap()
        .sessions
        .into_iter()
        .map(|s| s.row.handle)
        .collect();
    assert_eq!(handles, ["zinc-moth", "amber-heron", "brisk-finch"]);
}

#[test]
fn a_session_reads_back_every_column() {
    let row = SessionRow {
        handle: "amber-heron".into(),
        kind: "reviewer".into(),
        parent: Some("ochre-owl".into()),
        worktree: "/w/rust-s2".into(),
        branch: "tools/rust-port-s2".into(),
        model: "opus".into(),
        pid: Some(4242),
        host: "elsewhere".into(),
        affinity: "rust,tools".into(),
        connected: NOW - 100.0,
        last_seen: NOW - 1.0,
        left_at: None,
        agent: Some("agent-7".into()),
        busy_until: Some(NOW + 30.0),
        busy_call: Some("toolu_1".into()),
        last_tool: Some(NOW - 2.0),
    };
    let f = Fixture::new(Seed::default().session_row(row.clone()));
    assert_eq!(f.store.session("amber-heron").unwrap().row, row);
}

#[test]
fn the_newest_message_is_what_a_poller_compares() {
    let said = |id: i64| MessageRow {
        id,
        slug: "sim/jump".into(),
        sender: "owner".into(),
        kind: "msg".into(),
        text: format!("#{id}"),
        ts: NOW,
        ..Default::default()
    };
    let mut seed = Seed::default()
        .session("amber-heron")
        .task("sim/jump", "ready");
    seed.dump.messages = vec![said(3), said(17)];
    let f = Fixture::new(seed);
    assert_eq!(f.store.board().unwrap().latest_message, 17);
    f.store.claim("sim/jump", "amber-heron").unwrap();
    assert_eq!(
        f.store.board().unwrap().latest_message,
        18,
        "the next message continues the board's numbering"
    );
    let ids: Vec<i64> = f
        .store
        .thread("sim/jump")
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(ids, [3, 17, 18]);
}

#[test]
fn an_empty_board_reads_as_nothing() {
    let board = Fixture::new(Seed::default()).store.board().unwrap();
    assert_eq!(
        (
            board.tasks.len(),
            board.sessions.len(),
            board.latest_message
        ),
        (0, 0, 0)
    );
}

#[test]
fn an_unknown_task_is_refused() {
    let f = Fixture::new(Seed::default());
    assert_eq!(refused(f.store.get("sim/none")), "no task 'sim/none'");
    assert_eq!(refused(f.store.thread("sim/none")), "no task 'sim/none'");
}

#[test]
fn a_slug_is_data_never_part_of_a_statement() {
    let slug = "sim/x'); DELETE task; --";
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .task(slug, "ready")
            .task("sim/kept", "ready"),
    );
    assert_eq!(f.store.claim(slug, "amber-heron").unwrap().slug, slug);
    assert_eq!(f.store.tasks(None).unwrap().len(), 2);
    assert_eq!(f.store.thread(slug).unwrap().len(), 1);
}

#[test]
fn a_board_read_never_sees_half_a_write() {
    let mut seed = Seed::default().session("amber-heron");
    for i in 0..30 {
        seed = seed.task(&format!("sim/t{i:02}"), "ready");
    }
    let f = Fixture::new(seed);
    let writer = f.other();
    std::thread::scope(|scope| {
        let claims = scope.spawn(move || {
            for i in 0..30 {
                writer
                    .claim(&format!("sim/t{i:02}"), "amber-heron")
                    .unwrap();
            }
        });
        let mut reads = 0;
        while !claims.is_finished() || reads == 0 {
            let board = f.store.board().unwrap();
            let claimed = board
                .tasks
                .iter()
                .filter(|t| !t.holders("executor").is_empty())
                .count();
            for t in &board.tasks {
                assert_eq!(
                    t.status == "in_progress",
                    !t.holders("executor").is_empty(),
                    "{} in read {reads}",
                    t.slug
                );
            }
            assert_eq!(
                board.latest_message as usize, claimed,
                "one message per claim, read {reads}"
            );
            reads += 1;
        }
        claims.join().unwrap();
    });
}

/// `read` of a board holding a ready `sim/jump`, with another session's claim of it forced into the
/// middle (main's `SnapshotReadTest`): the read stalls on each task row it reads, after the tasks and
/// before the roles, for as long as `gate:read` says when it starts, and the claim lands in that stall.
fn claimed_mid_read<T>(read: impl FnOnce(&tasks::Store) -> T) -> T {
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .task("sim/jump", "ready"),
    );
    let (other, board) = (f.other(), f.other_client());
    board
        .write(
            "BEGIN TRANSACTION;
             DEFINE FIELD stall ON task COMPUTED sleep((SELECT VALUE wait FROM ONLY gate:read) ?? 0s);
             UPSERT gate:read SET wait = 2s;
             COMMIT TRANSACTION;",
            serde_json::json!({}),
        )
        .unwrap();
    let began = std::time::Instant::now();
    let (result, read, claimed) = std::thread::scope(|scope| {
        let claiming = scope.spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500)); // the read is in its stall
            board
                .write("UPSERT gate:read SET wait = 0s;", serde_json::json!({}))
                .unwrap(); // the claim's own read does not stall
            other.claim("sim/jump", "amber-heron").unwrap();
            began.elapsed()
        });
        let result = read(&f.store);
        let read = began.elapsed();
        (result, read, claiming.join().unwrap())
    });
    assert!(
        claimed < read,
        "the claim landed at {claimed:?}, after the read ended at {read:?}"
    );
    result
}

fn consistent(task: &tasks::Task) {
    assert_eq!(
        task.status == "ready",
        task.holders("executor").is_empty(),
        "{} is {} held by {:?}",
        task.slug,
        task.status,
        task.holders("executor")
    );
}

#[test]
fn tasks_is_one_snapshot() {
    let read = claimed_mid_read(|store| store.tasks(None).unwrap());
    assert_eq!(read.len(), 1);
    read.iter().for_each(consistent);
}

#[test]
fn get_is_one_snapshot() {
    consistent(&claimed_mid_read(|store| store.get("sim/jump").unwrap()));
}

#[test]
fn the_board_is_one_snapshot() {
    let board = claimed_mid_read(|store| store.board().unwrap());
    board.tasks.iter().for_each(consistent);
}

#[test]
fn a_thread_is_its_own_task_s_messages_in_order() {
    let said = |id: i64, slug: &str| MessageRow {
        id,
        slug: slug.into(),
        sender: "owner".into(),
        kind: "msg".into(),
        text: format!("#{id}"),
        ts: NOW,
        ..Default::default()
    };
    let mut seed = Seed::default()
        .task("a", "ready")
        .task("a/b", "ready")
        .task("a/bc", "ready")
        .task("a/b/c", "ready");
    seed.dump.messages = vec![
        said(1, "a/b"),
        said(2, "a"),
        said(3, "a/bc"),
        said(9, "a/b"),
        said(10, "a/b/c"),
        said(11, "a/b"),
    ];
    let f = Fixture::new(seed);
    let ids: Vec<i64> = f
        .store
        .thread("a/b")
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(ids, [1, 9, 11]);
    assert_eq!(f.store.thread("a").unwrap().len(), 1);
}

#[test]
fn threads_are_each_slug_s_thread_in_order_in_one_round_trip() {
    let said = |id: i64, slug: &str| MessageRow {
        id,
        slug: slug.into(),
        sender: "owner".into(),
        kind: "msg".into(),
        text: format!("#{id}"),
        ts: NOW,
        ..Default::default()
    };
    let mut seed = Seed::default()
        .task("a", "ready")
        .task("a/b", "ready")
        .task("c", "ready");
    seed.dump.messages = vec![
        said(1, "a/b"),
        said(2, "a"),
        said(9, "a/b"),
        said(11, "a/b"),
    ];
    let f = Fixture::new(seed);
    let before = f.store.client().requests();
    let threads = f
        .store
        .threads(&["a/b".into(), "a".into(), "c".into()])
        .unwrap();
    assert_eq!(f.store.client().requests() - before, 1);
    let ids = |slug: &str| -> Vec<i64> { threads[slug].iter().map(|m| m.id).collect() };
    assert_eq!(ids("a/b"), [1, 9, 11]);
    assert_eq!(ids("a"), [2]);
    assert_eq!(ids("c"), Vec::<i64>::new());
    assert_eq!(threads.len(), 3);
    assert!(f.store.threads(&[]).unwrap().is_empty());
}

#[test]
fn threads_refuse_a_slug_no_task_has() {
    let f = Fixture::new(Seed::default().task("a", "ready"));
    assert_eq!(
        refused(f.store.threads(&["a".into(), "sim/none".into()])),
        "no task 'sim/none'"
    );
}

/// The requests a call makes, and what it gave.
fn counted<T>(f: &Fixture, call: impl FnOnce() -> T) -> (usize, T) {
    let before = f.store.client().requests();
    let given = call();
    (f.store.client().requests() - before, given)
}

#[test]
fn a_rule_is_its_read_and_one_write_that_gives_back_the_task_as_written() {
    let f = Fixture::empty();
    let orch = f.session("orchestrator");
    let worker = f.session("worker");
    f.task("p/a");
    f.task("p/b");
    let (requests, claimed) = counted(&f, || f.store.claim("p/a", &worker).unwrap());
    assert_eq!((requests, claimed.status.as_str()), (2, "in_progress"));
    assert_eq!(claimed, f.store.get("p/a").unwrap());
    let (requests, moved) = counted(&f, || f.store.move_to("p/a", "review", &worker).unwrap());
    assert_eq!((requests, moved.status.as_str()), (2, "review"));
    assert_eq!(moved, f.store.get("p/a").unwrap());
    let (requests, held) = counted(&f, || {
        f.store.take_role("p/b", &orch, "orchestrator").unwrap()
    });
    assert_eq!(
        (requests, held.holders("orchestrator")),
        (2, vec![orch.as_str()])
    );
    assert_eq!(held, f.store.get("p/b").unwrap());
}

#[test]
fn a_new_task_is_one_round_trip_and_a_move_under_another_two() {
    let f = Fixture::empty();
    let orch = f.session("orchestrator");
    let (requests, made) = counted(&f, || {
        f.store
            .create(&tasks::NewTask::new("p/a", "A", &orch))
            .unwrap()
    });
    assert_eq!((requests, made.slug.as_str()), (1, "p/a"));
    assert_eq!(made, f.store.get("p/a").unwrap());
    f.task("p/b");
    let (requests, moved) = counted(&f, || {
        f.store.set_parent("p/b", Some("p/a"), &orch).unwrap()
    });
    assert_eq!((requests, moved.parent.as_deref()), (2, Some("p/a")));
    assert_eq!(moved, f.store.get("p/b").unwrap());
}

#[test]
fn a_new_session_is_two_round_trips() {
    let f = Fixture::empty();
    let (requests, made) = counted(&f, || {
        f.store.register(&tasks::NewSession::new("worker")).unwrap()
    });
    assert_eq!(requests, 2);
    assert_eq!(made, f.store.session(&made.row.handle).unwrap());
}

/// A schema definition failure leaves no partially created board or version marker.
/// A later open reports the failure too. Duplicate rows force a unique-index failure.
#[test]
fn a_definition_that_fails_leaves_no_board_taken_for_made() {
    let client = crate::common::client();
    let (addr, db) = (client.addr().to_string(), client.database().to_string());
    // The database is made as an open makes it, again when that meets another test's at once in the
    // shared namespace (the verify of 7ea29d1d: Conflict here). Its statements are no transaction, so
    // the rows go in after, once.
    let made =
        (0..100).find_map(
            |_| match client.define_then("RETURN NONE;", serde_json::json!({})) {
                Err(tasks::rpc::DbError::Conflict) => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    None
                }
                made => Some(made),
            },
        );
    made.expect("the database kept meeting other writes")
        .unwrap();
    client
        .query(
            "CREATE task:a SET sid = 1; CREATE task:b SET sid = 1;",
            serde_json::json!({}),
        )
        .unwrap();
    assert!(tasks::Store::open(client).is_err());
    for _ in 0..2 {
        let again = tasks::Store::open(tasks::rpc::Client::new(&addr, "sf", &db));
        assert!(
            again.is_err(),
            "a board missing a definition was taken for made"
        );
    }
    let fields = tasks::rpc::Client::new(&addr, "sf", &db)
        .query(
            "RETURN (INFO FOR DB).tables.session;",
            serde_json::json!({}),
        )
        .unwrap();
    assert_eq!(
        fields.last(),
        Some(&serde_json::Value::Null),
        "half a board was made"
    );
}

#[test]
fn opening_a_board_already_made_is_one_round_trip() {
    let first = crate::common::client();
    let (addr, db) = (first.addr().to_string(), first.database().to_string());
    tasks::Store::open(first).unwrap();
    let again = tasks::Store::open(tasks::rpc::Client::new(&addr, "sf", &db)).unwrap();
    assert_eq!(again.client().requests(), 1);
}
