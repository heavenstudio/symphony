use super::*;

use symphony_board::Role;
use symphony_board::health::Health;

fn board_task(slug: &str, status: &str) -> symphony_board::Task {
    symphony_board::Task {
        branch: None,
        id: 0,
        slug: slug.into(),
        title: "A task".into(),
        body: String::new(),
        status: status.into(),
        priority: 2,
        parent: None,
        created_by: "owner".into(),
        created: 10.0,
        updated: 20.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: Vec::new(),
        instruction: false,
        fields: BTreeMap::new(),
    }
}

fn role(handle: &str, role: &str, health: Health) -> Role {
    Role {
        handle: handle.into(),
        role: role.into(),
        since: 0.0,
        expires: None,
        health,
    }
}

fn board(tasks: Vec<symphony_board::Task>) -> symphony_board::Board {
    symphony_board::Board {
        tasks,
        sessions: Vec::new(),
        latest_message: 0,
        workflow_generation: None,
    }
}

/// `task` with `record` as its block's record in the board's own fields.
fn blocked_on(mut task: symphony_board::Task, record: serde_json::Value) -> symphony_board::Task {
    let fields = serde_json::json!({ symphony_board::red::FIELDS: { waits_on::BLOCKED: record } });
    task.fields = serde_json::from_value(fields).unwrap();
    task
}

/// What the listing sends of `task` on a board of `tasks`, with `stood` read of it.
fn filled(task: &symphony_board::Task, tasks: &[symphony_board::Task], stood: &Stood) -> Task {
    let look = Look::new(tasks, &[]);
    let mut wire = crate::taskview::task(task);
    fill(&mut wire, task, &look, stood);
    wire
}

fn stood_of(task: &symphony_board::Task, since: f64, from: Option<&str>) -> Stood {
    let mut stood = Stood::default();
    let read = InStatus {
        review_rounds: 0,
        since,
        from: from.map(str::to_string),
    };
    let key = (task.status.clone(), task.updated.to_bits(), read);
    stood.by_slug.insert(task.slug.clone(), key);
    stood
}

#[test]
fn an_open_task_says_what_it_waits_on_since_it_took_its_status() {
    let mut task = board_task("sim/jump", "in_progress");
    task.roles = vec![role("brisk-finch", "executor", Health::Live)];
    let stood = stood_of(&task, 15.0, Some("ready"));
    let wire = filled(&task, std::slice::from_ref(&task), &stood);
    assert_eq!(
        wire.waits_on,
        Some(WaitsOn {
            kind: "working".into(),
            who: "brisk-finch".into(),
            why: "executor".into(),
            since: 15.0,
        })
    );
    // Only a blocked task says where it stopped.
    assert_eq!((wire.status.as_str(), wire.from), ("in_progress", None));
}

#[test]
fn a_task_whose_status_or_write_moved_since_its_read_says_since_its_write() {
    let task = board_task("sim/jump", "ready");
    let stood = stood_of(&task, 15.0, None);
    let mut moved = task.clone();
    moved.updated = 30.0;
    let wire = filled(&moved, std::slice::from_ref(&moved), &stood);
    assert_eq!(wire.waits_on.unwrap().since, 30.0);
    let mut moved = task.clone();
    moved.status = "design".into();
    assert_eq!(stood.of(&moved), None);
}

#[test]
fn a_finished_task_waits_on_nothing() {
    let task = board_task("sim/jump", "done");
    let wire = filled(&task, std::slice::from_ref(&task), &Stood::default());
    assert_eq!((wire.waits_on, wire.from), (None, None));
}

#[test]
fn a_listing_counts_running_tests_present_as_pull_does() {
    let mut work = board_task("work", "in_progress");
    work.roles = vec![role("executor", "executor", Health::Lost)];
    let mut review = board_task("review", "review");
    review.roles = vec![role("reviewer", "reviewer", Health::Lost)];
    let mut ready = board_task("ready", "ready");
    ready.roles = vec![role("executor", "executor", Health::Lost)];
    let parent = board_task("parent", "in_progress");
    let mut child = board_task("child", "in_progress");
    child.parent = Some("parent".into());
    child.roles = vec![role("executor", "executor", Health::Lost)];
    let board = board(vec![work, review, ready, parent, child]);
    let busy = ["executor".into(), "reviewer".into()];
    let stood = Stood::default();
    let lane = BTreeMap::new();
    let (active, _) = crate::taskview::snapshot_board_with_busy(&board, &stood, &lane, &busy);
    let look = Look::new(&board.tasks, &busy);
    for (wire, task) in active.tasks.iter().zip(&board.tasks) {
        let expected = waits_on::line(task, &look, None).unwrap();
        let waits = wire.waits_on.as_ref().unwrap();
        assert_eq!(
            (waits.kind.as_str(), waits.who.as_str()),
            (expected.kind, expected.who.as_str())
        );
        assert_eq!(waits.kind, "working", "{}", wire.slug);
    }
    assert_eq!(active.unstarted, 0);
    let (quiet, _) = crate::taskview::snapshot_board(&board, &stood, &lane);
    assert_eq!(quiet.unstarted, 1);
    assert_ne!(quiet.version, active.version);
    assert!(
        quiet.tasks.iter().filter(|t| t.status != "ready").all(|t| t
            .waits_on
            .as_ref()
            .unwrap()
            .kind
            == "nobody")
    );
}

#[test]
fn a_block_that_names_its_thing_stays_blocked_and_says_where_it_stopped() {
    let task = blocked_on(
        board_task("sim/jump", "blocked"),
        serde_json::json!({"on": "decision:a-card", "by": "brisk-finch", "at": 18.0,
            "from": "review"}),
    );
    // The record's own from wins over the thread's.
    let stood = stood_of(&task, 18.0, Some("in_progress"));
    let wire = filled(&task, std::slice::from_ref(&task), &stood);
    assert_eq!(
        (wire.status.as_str(), wire.from.as_deref()),
        ("blocked", Some("review"))
    );
    let waits = wire.waits_on.unwrap();
    assert_eq!(
        (waits.kind.as_str(), waits.who.as_str(), waits.since),
        ("decision", "a-card", 18.0)
    );
}

#[test]
fn a_block_that_names_nothing_is_sent_in_the_stage_it_stopped_in() {
    let task = board_task("sim/jump", "blocked");
    let stood = stood_of(&task, 18.0, Some("review"));
    let wire = filled(&task, std::slice::from_ref(&task), &stood);
    assert_eq!(
        (wire.status.as_str(), wire.from.as_deref()),
        ("review", Some("review"))
    );
    let waits = wire.waits_on.unwrap();
    assert_eq!(
        (waits.kind.as_str(), waits.who.as_str(), waits.why.as_str()),
        ("nobody", "", waits_on::NO_REASON)
    );
    // Where it stopped is not known, or is no stage: it stays blocked.
    for from in [None, Some("blocked"), Some("done")] {
        let stood = stood_of(&task, 18.0, from);
        let wire = filled(&task, std::slice::from_ref(&task), &stood);
        assert_eq!(wire.status, "blocked", "from {from:?}");
    }
}

#[test]
fn a_cancelled_dependency_blocks_and_stays_blocked_on_the_wire() {
    let mut task = board_task("sim/jump", "blocked");
    task.depends = vec!["sim/old".into()];
    let old = board_task("sim/old", "cancelled");
    let tasks = [task.clone(), old];
    let stood = stood_of(&task, 18.0, Some("ready"));
    let wire = filled(&task, &tasks, &stood);
    assert_eq!(wire.status, "blocked");
    assert_eq!(wire.waits_on.unwrap().kind, "nobody");
}

#[test]
fn stood_reads_again_only_what_moved_and_keeps_finished_summaries() {
    use crate::taskview::page_tests::store_fixture::{Fixture, NOW};
    let f = Fixture::empty();
    let exe = f.session("worker");
    for slug in ["sim/jump", "sim/ready", "sim/gone"] {
        f.task(slug);
    }
    f.advance(30.0);
    f.store.claim("sim/jump", &exe).unwrap();
    let mut stood = Stood::default();
    stood.read(&f.store, &f.store.board().unwrap()).unwrap();
    let board = f.store.board().unwrap();
    let of = |stood: &Stood, slug: &str| {
        let task = board.tasks.iter().find(|t| t.slug == slug).unwrap();
        stood.of(task).cloned()
    };
    assert_eq!(
        of(&stood, "sim/jump"),
        Some(InStatus {
            review_rounds: 0,
            since: NOW + 30.0,
            from: Some("ready".into())
        })
    );
    assert_eq!(
        of(&stood, "sim/ready"),
        Some(InStatus {
            review_rounds: 0,
            since: NOW,
            from: None
        })
    );
    // A read the board did not move since is kept as it is, so its thread is read no more.
    let marked = InStatus {
        review_rounds: 0,
        since: -1.0,
        from: None,
    };
    for (_, _, read) in stood.by_slug.values_mut() {
        *read = marked.clone();
    }
    stood.read(&f.store, &board).unwrap();
    assert_eq!(of(&stood, "sim/ready"), Some(marked.clone()));
    // A task that moved is read again; one finished keeps its summary.
    f.advance(30.0);
    f.store.move_to("sim/gone", "cancelled", "owner").unwrap();
    f.store
        .move_on(
            "sim/jump",
            "blocked",
            &exe,
            None,
            Some(symphony_board::On {
                thing: "parked",
                why: None,
            }),
        )
        .unwrap();
    let board = f.store.board().unwrap();
    stood.read(&f.store, &board).unwrap();
    assert_eq!(
        stood.by_slug.keys().collect::<Vec<_>>(),
        ["sim/gone", "sim/jump", "sim/ready"]
    );
    let jump = board.tasks.iter().find(|t| t.slug == "sim/jump").unwrap();
    assert_eq!(
        stood.of(jump),
        Some(&InStatus {
            review_rounds: 0,
            since: NOW + 60.0,
            from: Some("in_progress".into())
        })
    );
    let ready = board.tasks.iter().find(|t| t.slug == "sim/ready").unwrap();
    assert_eq!(stood.of(ready), Some(&marked));
}

#[test]
fn the_orchestrators_are_those_of_open_tasks_with_their_health() {
    let mut open = board_task("sim/jump", "in_progress");
    open.roles = vec![
        role("sable-finch", "orchestrator", Health::Stale),
        role("brisk-finch", "executor", Health::Live),
    ];
    let mut other = board_task("sim/fall", "review");
    other.roles = vec![
        role("sable-finch", "orchestrator", Health::Stale),
        role("glacial-heron", "orchestrator", Health::Left),
    ];
    let mut done = board_task("sim/old", "done");
    done.roles = vec![role("amber-heron", "orchestrator", Health::Live)];
    assert_eq!(
        orchestrators(&board(vec![open, other, done])),
        [
            Orchestrator {
                handle: "glacial-heron".into(),
                health: "left".into()
            },
            Orchestrator {
                handle: "sable-finch".into(),
                health: "stale".into()
            },
        ]
    );
}

#[test]
fn a_task_in_the_sign_off_lane_waits_on_a_decision_never_a_person() {
    let mut tasks = vec![
        crate::taskview::task(&board_task("sim/cards", "review")),
        crate::taskview::task(&board_task("sim/gate", "review")),
        crate::taskview::task(&board_task("sim/other", "review")),
    ];
    tasks[0].waits_on = Some(WaitsOn {
        kind: "queued".into(),
        since: 15.0,
        ..WaitsOn::default()
    });
    let lane = BTreeMap::from([
        (
            "sim/cards".to_string(),
            vec!["a-card".into(), "b-card".into()],
        ),
        ("sim/gate".to_string(), Vec::new()),
    ]);
    decided(&mut tasks, &lane);
    assert_eq!(
        tasks[0].waits_on,
        Some(WaitsOn {
            kind: "decision".into(),
            who: "a-card, b-card".into(),
            why: "the owner's sign-off".into(),
            since: 15.0,
        })
    );
    assert_eq!(
        tasks[1].waits_on,
        Some(WaitsOn {
            kind: "decision".into(),
            who: SIGN_OFF.into(),
            why: "its owner gate holds it".into(),
            since: 20.0,
        })
    );
    assert_eq!(tasks[2].waits_on, None);
}

/// Seed historical moves through the raw interface: these tests exercise batched history reads,
/// not today's admission/reviewer rules (and include old review-child and reopened histories).
fn history_move(f: &crate::taskview::page_tests::store_fixture::Fixture, slug: &str, status: &str) {
    use crate::taskview::page_tests::store_fixture::NOW;
    use serde_json::json;
    let from = f.store.get(slug).unwrap().status;
    f.advance(1.0);
    let ts = f.store.now();
    let sid = (ts - NOW) as i64 + 100;
    f.store
        .raw()
        .row("message", json!([slug, sid]))
        .create(json!({
            "sid": sid, "slug": slug, "sender": "owner", "recipient": null,
            "kind": "system", "text": format!("owner moved it {from} → {status}"), "ts": ts,
        }))
        .unwrap();
    f.store
        .raw()
        .row("task", slug)
        .set(json!({"status": status, "updated": ts}))
        .unwrap();
}

#[test]
fn review_summaries_batch_closed_tasks_keep_reads_and_invalidate_changed_history() {
    use crate::taskview::page_tests::store_fixture::Fixture;
    let f = Fixture::empty();
    for slug in [
        "sim/reviewed",
        "sim/cancelled",
        "sim/ready",
        "sim/reviewed/review",
    ] {
        f.task(slug);
    }
    f.store
        .raw()
        .row("task", "sim/reviewed/review")
        .set(serde_json::json!({"parent": "sim/reviewed", "instruction": true}))
        .unwrap();
    f.advance(1.0);
    history_move(&f, "sim/reviewed", "review");
    history_move(&f, "sim/reviewed/review", "review");
    history_move(&f, "sim/cancelled", "cancelled");
    let board = f.store.board().unwrap();
    let mut stood = Stood::default();
    let before = f.store.database().requests();
    stood.read(&f.store, &board).unwrap();
    assert_eq!(
        f.store.database().requests() - before,
        1,
        "all histories in one request"
    );
    let counts = |board: &symphony_board::Board, stood: &Stood| {
        crate::taskview::snapshot_board(board, stood, &BTreeMap::new())
            .0
            .tasks
            .into_iter()
            .map(|t| (t.slug, t.review_rounds))
            .collect::<BTreeMap<_, _>>()
    };
    let first = counts(&board, &stood);
    assert_eq!(first["sim/reviewed"], Some(1));
    assert_eq!(first["sim/cancelled"], Some(0));
    assert_eq!(first["sim/ready"], Some(0));
    // A separate review task never contributes a round to the task it names.
    assert_eq!(first["sim/reviewed/review"], Some(1));
    let before = f.store.database().requests();
    for _ in 0..3 {
        stood.read(&f.store, &board).unwrap();
        assert_eq!(counts(&board, &stood), first);
    }
    assert_eq!(
        f.store.database().requests(),
        before,
        "kept snapshots read no histories"
    );
    f.advance(1.0);
    history_move(&f, "sim/reviewed", "in_progress");
    f.advance(1.0);
    history_move(&f, "sim/reviewed", "review");
    let changed = f.store.board().unwrap();
    assert_eq!(
        counts(&changed, &stood)["sim/reviewed"],
        None,
        "stale is not zero or an old count"
    );
    let ready_key = stood.by_slug["sim/ready"].clone();
    let before = f.store.database().requests();
    stood.read(&f.store, &changed).unwrap();
    assert_eq!(f.store.database().requests() - before, 1);
    assert_eq!(counts(&changed, &stood)["sim/reviewed"], Some(2));
    assert_eq!(stood.by_slug["sim/ready"], ready_key);
    f.advance(1.0);
    history_move(&f, "sim/reviewed", "done");
    let mut closed = f.store.board().unwrap();
    stood.read(&f.store, &closed).unwrap();
    assert_eq!(counts(&closed, &stood)["sim/reviewed"], Some(2));
    let before = f.store.database().requests();
    stood.read(&f.store, &closed).unwrap();
    assert_eq!(
        f.store.database().requests(),
        before,
        "closed histories are kept too"
    );
    closed.tasks.retain(|t| t.slug != "sim/reviewed");
    stood.read(&f.store, &closed).unwrap();
    assert!(
        !stood.by_slug.contains_key("sim/reviewed"),
        "removed tasks leave the cache"
    );
    let empty = Stood::default();
    assert!(counts(&closed, &empty).values().all(Option::is_none));
}

#[test]
fn review_summary_batch_counts_each_move_to_review() {
    use crate::taskview::page_tests::store_fixture::{Fixture, Seed};
    let f = Fixture::new(Seed::default());
    f.task("sim/rounds");
    let mut stood = Stood::default();
    for (status, count) in [
        ("review", 1),
        ("in_progress", 1),
        ("review", 2),
        ("blocked", 2),
        ("review", 3),
        ("done", 3),
        ("review", 4),
        ("cancelled", 4),
    ] {
        history_move(&f, "sim/rounds", status);
        let board = f.store.board().unwrap();
        stood.read(&f.store, &board).unwrap();
        let wire = crate::taskview::snapshot_board(&board, &stood, &BTreeMap::new()).0;
        assert_eq!(wire.tasks[0].review_rounds, Some(count), "{status}");
    }
}
