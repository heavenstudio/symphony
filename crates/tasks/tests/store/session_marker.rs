//! Heartbeats invalidate cached health without writing the structural change counter.
use crate::common::Backend;

use super::common::{Fixture, GONE, Seed, refused};
use super::gate::Gate;
use serde_json::json;
use symphony_board::{Beat, Health, Store};

fn markers(f: &Fixture) -> (u64, u64) {
    let structure = f
        .store
        .raw()
        .row("counter", "board_structure")
        .field("last")
        .unwrap()
        .as_u64()
        .unwrap_or(0);
    (structure, f.store.marker().unwrap())
}

on_both! {
fn beats_refresh_cached_health_without_the_shared_counter_board_write(backend: Backend) {
    let f = Fixture::on(backend,
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
        (f.store.raw().row("counter", "board"))
            .field("last")
            .unwrap(),
        match backend { Backend::SurrealDb => json!(i64::MIN), Backend::PostgreSql => serde_json::Value::Null },
        "a beat neither rewrites SurrealDB's retired scalar nor creates one on PostgreSQL"
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
    let requests = reader.database().requests();
    assert_eq!(reader.clock_board().unwrap(), board);
    assert_eq!(
        reader.database().requests() - requests,
        1,
        "unchanged marker reuses the snapshot"
    );
    match f.store.database() {
        symphony_board::Database::SurrealDb(client) => assert_eq!(client.query(
            "BEGIN TRANSACTION; UPDATE session:worker SET last_seen += 1; THROW 'rolled-back-beat'; COMMIT TRANSACTION;", json!({})), Err(symphony_board::db::DbError::Thrown("rolled-back-beat".into()))),
        symphony_board::Database::PostgreSql(client) => {
            let why = client.simple("BEGIN; UPDATE session SET last_seen = last_seen + 1 WHERE handle = 'worker'; DO $test$ BEGIN RAISE EXCEPTION 'rolled-back-beat'; END $test$; COMMIT;").unwrap_err();
            assert_eq!(why, symphony_board::db::DbError::Thrown("rolled-back-beat".into()));
            client.simple("ROLLBACK").unwrap();
        }
    }
    assert_eq!(
        markers(&f),
        after,
        "a rolled-back beat publishes no revision"
    );
    assert_eq!(reader.clock_board().unwrap(), board);
}
}

on_both! {
fn deleting_and_recreating_sessions_never_reuses_a_cached_marker(backend: Backend) {
    let seed = crate::common::session("worker");
    let f = Fixture::on(backend, crate::common::Seed::default());
    let reader = f.other();
    assert_eq!(markers(&f), (0, 0));
    let empty = reader.clock_board().unwrap();
    assert!(
        empty.sessions.is_empty(),
        "sessions on an empty board: {:?}",
        empty.sessions
    );
    f.store.raw().row("session", "worker").put(json!(seed)).unwrap();
    let initial = reader.clock_board().unwrap();
    let before = markers(&f);
    f.store.raw().row("session", "worker").delete().unwrap();
    let deleted = markers(&f);
    assert_eq!(deleted, (before.0, before.1 + 1));
    let after_delete = reader.clock_board().unwrap();
    assert!(
        after_delete.sessions.is_empty(),
        "sessions after deletion: {:?}",
        after_delete.sessions
    );
    f.store.raw().row("session", "worker").put(json!(seed)).unwrap();
    let recreated = markers(&f);
    assert_eq!(recreated, (deleted.0, deleted.1 + 1));
    assert_eq!(reader.clock_board().unwrap(), initial);
    f.store.leave("worker").unwrap();
    assert_eq!(
        reader.clock_board().unwrap().sessions[0].health,
        Health::Left
    );
}
}

// SurrealDB alone: reconstructs the pre-shard schema-14 session marker.
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
    let revision = (f.store.raw().row("session_revision", "legacy"))
        .fields(&["last", "writes"])
        .unwrap();
    assert_eq!(
        revision,
        json!({"last": 0, "writes": 1}),
        "the first guard creates its fence without fabricating a session mutation"
    );
}

/// The event that pauses a write past its guards at `gate`: `paused` on `table` when `when` holds.
fn paused_at(f: &Fixture, gate: &Gate, table: &str, when: &str) {
    let pause = format!(
        "DEFINE EVENT paused ON {table} WHEN {when} THEN {{ {} }};",
        gate.wait_here()
    );
    f.store.client().query(&pause, json!({})).unwrap();
}

/// As the session-hold race in closing.rs: the server pauses the real write after its guards, at
/// `gate`, while a second client commits; the write goes on once that has. The gate reached proves
/// the guard preceded the competing commit.
fn while_paused<T: Send>(
    f: &Fixture,
    gate: &Gate,
    act: impl FnOnce(&Store) -> T + Send,
    meanwhile: impl FnOnce(),
) -> T {
    let writer = f.other();
    std::thread::scope(|scope| {
        let opening = gate.opening();
        let running = scope.spawn(|| act(&writer));
        gate.reached();
        meanwhile();
        assert!(
            !running.is_finished(),
            "the write went on before the gate opened"
        );
        drop(opening);
        running.join().unwrap()
    })
}

// SurrealDB alone: an HTTP event gate pauses its transaction at the actor fence.
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
    let gate = Gate::new();
    paused_at(
        &f,
        &gate,
        "role",
        "$event = 'DELETE' AND $before.handle = 'old'",
    );
    let result = while_paused(
        &f,
        &gate,
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

// SurrealDB alone: an HTTP event gate pauses its transaction at the actor fence.
#[test]
fn a_departure_committing_after_the_incoming_holder_guard_prevents_handover() {
    let f = Fixture::new(
        Seed::default()
            .session("from")
            .session("target")
            .task("tools/held", "ready")
            .role("tools/held", "from", "orchestrator"),
    );
    let gate = Gate::new();
    paused_at(
        &f,
        &gate,
        "role",
        "$event = 'CREATE' AND $after.handle = 'target'",
    );
    let result = while_paused(
        &f,
        &gate,
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

// SurrealDB alone: an HTTP event gate pauses its transaction at the actor fence.
#[test]
fn a_departure_committing_after_the_actor_guard_prevents_a_field_write() {
    let f = Fixture::new(Seed::default().session("actor").task("tools/held", "ready"));
    let gate = Gate::new();
    paused_at(&f, &gate, "task", "$after.fields.game.race = 'started'");
    let result = while_paused(
        &f,
        &gate,
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

// SurrealDB alone: an HTTP event gate pauses its transaction at the actor fence.
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
    let gate = Gate::new();
    paused_at(&f, &gate, "message", "$after.text = 'notification-race'");
    let result = while_paused(
        &f,
        &gate,
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

// SurrealDB alone: an HTTP event gate pauses its transaction at the actor fence.
#[test]
fn a_departure_after_the_direct_message_recipient_guard_refuses_the_message() {
    let f = Fixture::new(Seed::default().session("actor").session("recipient"));
    (f.store.raw().row("callback_state", "recipient"))
        .create(json!({"handle": "recipient", "address": "codex:recipient-probe"}))
        .unwrap();
    let gate = Gate::new();
    paused_at(&f, &gate, "callback_event", "$event = 'CREATE'");
    f.advance(1.0);
    let result = while_paused(
        &f,
        &gate,
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

// SurrealDB alone: an HTTP event gate pauses its transaction at the actor fence.
#[test]
fn same_key_classification_rechecks_a_concurrently_remapped_target() {
    let f = Fixture::new(Seed::default().session("target"));
    (f.store.raw().row("session", "target"))
        .set(json!({"agent": "old-key"}))
        .unwrap();
    (f.store
        .raw()
        .row("callback_state", json!(["binding", "new-key"])))
    .create(json!({"in_process": false}))
    .unwrap();
    let gate = Gate::new();
    paused_at(
        &f,
        &gate,
        "counter",
        "$after.id = counter:['agent', 'old-key']",
    );
    let before = markers(&f);
    let result = while_paused(
        &f,
        &gate,
        |s| {
            s.session("target").unwrap();
            s.bind_agent("old-key", "target", false)
        },
        || {
            (f.store.raw().row("session", "target"))
                .set(json!({"agent": "new-key"}))
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
    let provenance = (f.store.raw())
        .row("callback_state", json!(["binding", "old-key"]))
        .field("in_process")
        .unwrap();
    assert_eq!(
        provenance,
        serde_json::Value::Null,
        "losing classification must not publish stale provenance"
    );
}
