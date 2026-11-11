//! Heartbeats invalidate cached health without writing the structural change counter.

use super::common::{Fixture, GONE, Seed, refused};
use serde_json::json;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use symphony_board::{Beat, Health, Store, schema};

fn markers(f: &Fixture) -> (u64, u64) {
    let value = f
        .store
        .client()
        .query(
            &format!(
                "RETURN [counter:board_structure.last ?? 0, {}];",
                schema::BOARD_MARKER
            ),
            json!({}),
        )
        .unwrap()
        .remove(0);
    serde_json::from_value(value).unwrap()
}

#[test]
fn beats_refresh_cached_health_without_the_shared_counter_board_write() {
    let f = Fixture::new(
        Seed::default()
            .session("worker")
            .task("tools/held", "in_progress")
            .role("tools/held", "worker", "executor"),
    );
    let bound = f.store.bind_agent("agent", "worker", true).unwrap();
    assert!(bound, "agent binding to worker: {bound}");
    let reader = f.other();
    assert_eq!(
        reader.clock_board().unwrap().sessions[0].health,
        Health::Live
    );
    f.advance(GONE);
    let before = markers(&f);
    assert_eq!(
        reader.clock_board().unwrap().sessions[0].health,
        Health::Lost
    );
    assert_eq!(
        f.store
            .agent_beat("agent", &Beat::default())
            .unwrap()
            .as_deref(),
        Some("worker")
    );
    let after = markers(&f);
    assert_eq!(after, (before.0, before.1 + 1));
    assert_eq!(
        f.store
            .client()
            .query("RETURN counter:board.last;", json!({}))
            .unwrap(),
        vec![json!(i64::MIN)],
        "a beat never rewrites the retired scalar"
    );
    let board = reader.clock_board().unwrap();
    assert_eq!(board.sessions[0].health, Health::Live);
    assert_eq!(board.sessions[0].row.last_tool, Some(f.now()));
    f.store.touch("worker").unwrap();
    assert_eq!(
        markers(&f),
        after,
        "an authorization fence alone changes no cached input"
    );
    let requests = reader.client().requests();
    assert_eq!(reader.clock_board().unwrap(), board);
    assert_eq!(
        reader.client().requests() - requests,
        1,
        "unchanged marker reuses the snapshot"
    );
    assert_eq!(f.store.client().query(
        "BEGIN TRANSACTION; UPDATE session:worker SET last_seen += 1; THROW 'rolled-back-beat'; COMMIT TRANSACTION;",
        json!({}),
    ), Err(symphony_board::rpc::DbError::Thrown("rolled-back-beat".into())));
    assert_eq!(
        markers(&f),
        after,
        "a rolled-back beat publishes no revision"
    );
    assert_eq!(reader.clock_board().unwrap(), board);
}

#[test]
fn deleting_and_recreating_sessions_never_reuses_a_cached_marker() {
    let seed = Seed::default().session("worker");
    let f = Fixture::empty();
    let reader = f.other();
    assert_eq!(markers(&f), (0, 0));
    let empty = reader.clock_board().unwrap();
    assert!(
        empty.sessions.is_empty(),
        "sessions on an empty board: {:?}",
        empty.sessions
    );
    f.store.copy(&seed.dump).unwrap();
    let initial = reader.clock_board().unwrap();
    let before = markers(&f);
    f.store
        .client()
        .query("DELETE session:worker;", json!({}))
        .unwrap();
    let deleted = markers(&f);
    assert_eq!(deleted, (before.0, before.1 + 1));
    let after_delete = reader.clock_board().unwrap();
    assert!(
        after_delete.sessions.is_empty(),
        "sessions after deletion: {:?}",
        after_delete.sessions
    );
    f.store.copy(&seed.dump).unwrap();
    let recreated = markers(&f);
    assert_eq!(recreated, (deleted.0, deleted.1 + 1));
    assert_eq!(reader.clock_board().unwrap(), initial);
    f.store.leave("worker").unwrap();
    assert_eq!(
        reader.clock_board().unwrap().sessions[0].health,
        Health::Left
    );
}

#[test]
fn an_upgraded_legacy_actor_can_write_before_its_first_sharded_beat() {
    let f = Fixture::new(
        Seed::default()
            .session("legacy")
            .task("tools/held", "ready"),
    );
    let before = markers(&f).1;
    f.store
        .client()
        .query(
            "BEGIN TRANSACTION;
         UPDATE counter:board SET last = $marker;
         DELETE counter:board_structure;
         REMOVE TABLE session_revision;
         UPDATE meta:schema SET version = 14;
         COMMIT TRANSACTION;",
            json!({"marker": before}),
        )
        .unwrap();
    let upgraded = f.other();
    assert_eq!(
        markers(&f).1,
        before,
        "opening the new schema changes no board input"
    );
    let task = upgraded
        .set_field("tools/held", "game", "race", &json!("started"), "legacy")
        .unwrap();
    assert_eq!(task.fields["game"]["race"], json!("started"));
    let revision = f
        .store
        .client()
        .query(
            "RETURN SELECT last, writes FROM ONLY session_revision:legacy;",
            json!({}),
        )
        .unwrap()
        .remove(0);
    assert_eq!(
        revision,
        json!({"last": 0, "writes": 1}),
        "the first guard creates its fence without fabricating a session mutation"
    );
}

/// As the session-hold race in closing.rs: the server pauses the real write after its guards,
/// while a second client commits. Checking elapsed time proves that the write reached the pause.
fn while_paused<T: Send>(
    f: &Fixture,
    act: impl FnOnce(&Store) -> T + Send,
    meanwhile: impl FnOnce(),
) -> T {
    let (sending, sent) = mpsc::channel();
    let writer = f.late(move || {
        sending.send(()).unwrap();
    });
    let (result, elapsed) = std::thread::scope(|scope| {
        let running = scope.spawn(|| {
            let began = Instant::now();
            (act(&writer), began.elapsed())
        });
        sent.recv_timeout(Duration::from_secs(30)).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        meanwhile();
        running.join().unwrap()
    });
    assert!(
        elapsed >= Duration::from_secs(2),
        "the guard must precede the competing commit: {elapsed:?}"
    );
    result
}

#[test]
fn a_beat_committing_after_the_lost_holder_guard_prevents_takeover() {
    let f = Fixture::new(
        Seed::default()
            .session("old")
            .task("tools/held", "in_progress")
            .role("tools/held", "old", "executor"),
    );
    let bound = f.store.bind_agent("old-agent", "old", true).unwrap();
    assert!(bound, "old-agent binding: {bound}");
    f.advance(GONE);
    let actor = f.session("worker");
    f.store.client().query(
        "DEFINE EVENT paused ON role WHEN $event = 'DELETE' AND $before.handle = 'old' THEN { sleep(2s) };",
        json!({}),
    ).unwrap();
    let result = while_paused(
        &f,
        |s| s.claim("tools/held", &actor),
        || {
            assert_eq!(
                f.store
                    .agent_beat("old-agent", &Beat::default())
                    .unwrap()
                    .as_deref(),
                Some("old")
            );
        },
    );
    assert_eq!(refused(result), "tools/held is held by old (live)");
    assert_eq!(
        f.store.get("tools/held").unwrap().holders("executor"),
        ["old"]
    );
    let thread = f.store.thread("tools/held").unwrap();
    assert!(
        thread.is_empty(),
        "the losing takeover left a log: {thread:?}"
    );
}

#[test]
fn a_departure_committing_after_the_incoming_holder_guard_prevents_handover() {
    let f = Fixture::new(
        Seed::default()
            .session("from")
            .session("target")
            .task("tools/held", "ready")
            .role("tools/held", "from", "orchestrator"),
    );
    f.store.client().query(
        "DEFINE EVENT paused ON role WHEN $event = 'CREATE' AND $after.handle = 'target' THEN { sleep(2s) };",
        json!({}),
    ).unwrap();
    let result = while_paused(
        &f,
        |s| s.hand_over("tools/held", "from", "target"),
        || {
            f.store.leave("target").unwrap();
        },
    );
    assert_eq!(
        refused(result),
        "target is left; hand tools/held to a session still here"
    );
    assert_eq!(
        f.store.get("tools/held").unwrap().holders("orchestrator"),
        ["from"]
    );
    let thread = f.store.thread("tools/held").unwrap();
    assert!(
        thread.is_empty(),
        "the losing handover left a log: {thread:?}"
    );
}

#[test]
fn a_departure_committing_after_the_actor_guard_prevents_a_field_write() {
    let f = Fixture::new(Seed::default().session("actor").task("tools/held", "ready"));
    f.store.client().query(
        "DEFINE EVENT paused ON task WHEN $after.fields.game.race = 'started' THEN { sleep(2s) };",
        json!({}),
    ).unwrap();
    let result = while_paused(
        &f,
        |s| s.set_field("tools/held", "game", "race", &json!("started"), "actor"),
        || {
            f.store.leave("actor").unwrap();
        },
    );
    assert_eq!(refused(result), "session actor has left; attach again");
    let task = f.store.get("tools/held").unwrap();
    assert!(
        task.fields.is_empty(),
        "fields after refused write: {:?}",
        task.fields
    );
    let thread = f.store.thread("tools/held").unwrap();
    assert!(
        thread.is_empty(),
        "the losing field write left a log: {thread:?}"
    );
}

#[test]
fn a_notification_fences_its_actor_without_fabricating_a_heartbeat() {
    let f = Fixture::new(
        Seed::default()
            .session("actor")
            .session("recipient")
            .task("tools/held", "ready"),
    );
    let actor = f.store.session("actor").unwrap().row;
    f.advance(60.0);
    let notice = f
        .store
        .notify("tools/held", "actor", "recipient", "before")
        .unwrap();
    assert_eq!(notice.sender, "board");
    assert_eq!(
        f.store.session("actor").unwrap().row,
        actor,
        "a machine notice is no tool call or heartbeat"
    );
    let before = f.store.thread("tools/held").unwrap();
    assert_eq!(before.len(), 1);
    f.store.client().query(
        "DEFINE EVENT paused ON message WHEN $after.text = 'notification-race' THEN { sleep(2s) };",
        json!({}),
    ).unwrap();
    let result = while_paused(
        &f,
        |s| s.notify("tools/held", "actor", "recipient", "notification-race"),
        || {
            f.store.leave("actor").unwrap();
        },
    );
    assert_eq!(refused(result), "session actor has left; attach again");
    assert_eq!(
        f.store.thread("tools/held").unwrap(),
        before,
        "the losing notice must leave no line"
    );
}

#[test]
fn a_departure_after_the_direct_message_recipient_guard_refuses_the_message() {
    let f = Fixture::new(Seed::default().session("actor").session("recipient"));
    f.store.client().query(
        "CREATE callback_state:recipient CONTENT { handle: 'recipient', address: 'codex:recipient-probe' };
         DEFINE EVENT paused ON callback_event WHEN $event = 'CREATE' THEN { sleep(2s) };",
        json!({}),
    ).unwrap();
    f.advance(1.0);
    let result = while_paused(
        &f,
        |s| s.send_agent("actor", "recipient", "recipient-race"),
        || {
            f.store.leave("recipient").unwrap();
        },
    );
    assert_eq!(refused(result), "session recipient has left");
    let queued = f.store.callback_inbox("recipient").unwrap();
    assert!(
        queued.is_empty(),
        "the losing send must leave no outbox entry: {queued:?}"
    );
}

#[test]
fn same_key_classification_rechecks_a_concurrently_remapped_target() {
    let f = Fixture::new(Seed::default().session("target"));
    f.store.client().query(
        "UPDATE session:target SET agent = 'old-key';
         CREATE callback_state:['binding', 'new-key'] CONTENT { in_process: false };
         DEFINE EVENT paused ON counter WHEN $after.id = counter:['agent', 'old-key'] THEN { sleep(2s) };",
        json!({}),
    ).unwrap();
    let before = markers(&f);
    let result = while_paused(
        &f,
        |s| {
            s.session("target").unwrap();
            s.bind_agent("old-key", "target", false)
        },
        || {
            f.store
                .client()
                .query("UPDATE session:target SET agent = 'new-key';", json!({}))
                .unwrap();
            let after = markers(&f);
            assert_eq!(
                after,
                (before.0, before.1 + 1),
                "the remapping must not be fenced accidentally by the structural counter"
            );
        },
    );
    assert!(!result.unwrap(), "a full-session key may not be replaced");
    let target = f.store.session("target").unwrap().row;
    assert_eq!(target.agent.as_deref(), Some("new-key"));
    let provenance = f
        .store
        .client()
        .query(
            "RETURN type::record('callback_state', ['binding', 'old-key']).in_process;",
            json!({}),
        )
        .unwrap();
    assert_eq!(
        provenance,
        vec![serde_json::Value::Null],
        "losing classification must not publish stale provenance"
    );
}
