//! Heartbeats invalidate cached health without writing the structural change counter.

use super::common::{Fixture, GONE, Seed};
use serde_json::json;
use symphony_board::{Beat, Health};

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
        (f.store.raw().row("counter", "board"))
            .field("last")
            .unwrap(),
        serde_json::Value::Null,
        "a beat creates no board-wide scalar counter"
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
    let client = f.postgresql();
    let why = client.simple("BEGIN; UPDATE session SET last_seen = last_seen + 1 WHERE handle = 'worker'; DO $test$ BEGIN RAISE EXCEPTION 'rolled-back-beat'; END $test$; COMMIT;").unwrap_err();
    assert_eq!(
        why,
        symphony_board::db::DbError::Thrown("rolled-back-beat".into())
    );
    client.simple("ROLLBACK").unwrap();
    assert_eq!(
        markers(&f),
        after,
        "a rolled-back beat publishes no revision"
    );
    assert_eq!(reader.clock_board().unwrap(), board);
}

#[test]
fn deleting_and_recreating_sessions_never_reuses_a_cached_marker() {
    let seed = crate::common::session("worker");
    let f = Fixture::new(crate::common::Seed::default());
    let reader = f.other();
    assert_eq!(markers(&f), (0, 0));
    let empty = reader.clock_board().unwrap();
    assert!(
        empty.sessions.is_empty(),
        "sessions on an empty board: {:?}",
        empty.sessions
    );
    f.store
        .raw()
        .row("session", "worker")
        .put(json!(seed))
        .unwrap();
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
    f.store
        .raw()
        .row("session", "worker")
        .put(json!(seed))
        .unwrap();
    let recreated = markers(&f);
    assert_eq!(recreated, (deleted.0, deleted.1 + 1));
    assert_eq!(reader.clock_board().unwrap(), initial);
    f.store.leave("worker").unwrap();
    assert_eq!(
        reader.clock_board().unwrap().sessions[0].health,
        Health::Left
    );
}
