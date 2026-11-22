//! What a move of the board to another engine carries beside the dump (`Store::carried`,
//! `Store::carry`): the rows the board's commands keep for sessions and programs and no copy
//! derives, read on one engine and written on the other under the same keys, row for row.

use crate::common::{Backend, Fixture, Seed};
use serde_json::{Value, json};
use symphony_board::{CARRIED, Error, NewSession, leases_ended};

/// A row of every carried table as the board's commands leave them, with what an engine could
/// read otherwise than the other: a compound key, a number with no fraction and one with, a
/// nested object whose keys are out of order, an array, an empty text.
fn seeded(f: &Fixture) {
    let raw = f.store.raw();
    raw.row("callback_state", "amber-heron")
        .create(json!({
            "address": "uds:/tmp/cc-socks/1.sock",
            "seen": {"tools/two": 7, "tools/one": 3.5},
            "statuses": {"tools/one": "review"},
            "pull_said": ["a", "b"],
            "revision": 12,
        }))
        .unwrap();
    raw.row("callback_event", "amber-heron-41")
        .create(json!({
            "handle": "amber-heron", "key": "amber-heron-41", "sequence": 41,
            "text": "MSG #9 tools/one from owner", "delivered": false,
        }))
        .unwrap();
    raw.row("chat_event", "7")
        .create(json!({"key": "7", "text": "", "delivered": true}))
        .unwrap();
    raw.row("writer", json!(["this-host", "/opt/bin/tasks"]))
        .create(json!({
            "program": "tasks", "host": "this-host", "path": "/opt/bin/tasks",
            "schema": 22, "seen": 1_800_000_000.25,
        }))
        .unwrap();
    raw.row("unanswered_restore", json!(["tools/one", "amber-heron", 9]))
        .create(json!({"at": 1_800_000_001.0}))
        .unwrap();
    raw.row("counter", json!(["hold", "tools/one"]))
        .create(json!({"last": 0, "ts": 1_800_000_002.5}))
        .unwrap();
    // A stored preference with its record: a project's own word, which no file holds.
    raw.row("setting", "limits.low_volume")
        .create(json!({
            "key": "limits.low_volume", "value": 2, "revision": 1,
            "changes": [{"after": 2, "at": 1_800_000_003.5, "by": "amber-heron"}],
        }))
        .unwrap();
    // A dispatcher's lease, carried as a row like any other: a move ends it first.
    raw.row("callback_lease", "amber-heron")
        .create(json!({"until": 1_900_000_000.0, "revision": 12}))
        .unwrap();
    // A fence counts up and is carried; the copy's own counters and the marker are not.
    raw.row("counter", json!(["slots", "amber-heron"]))
        .create(json!({"last": 4}))
        .unwrap();
    assert!(
        f.store
            .artifact_ledger_cas(
                "tenant",
                "project",
                0,
                &json!({"format": 1, "n": [null, 1e18]})
            )
            .unwrap()
    );
}

/// How many rows `carried` holds of `table`.
fn rows(carried: &Value, table: &str) -> usize {
    carried[table].as_array().unwrap().len()
}

fn carried_across(from: Backend, to: Backend) {
    let source = Fixture::on(from, Seed::default());
    seeded(&source);
    let carried = source.store.carried().unwrap();
    for table in CARRIED {
        // The store that opened the board keeps its own line among the writers.
        let held = if table == "writer" || table == "counter" {
            2
        } else {
            1
        };
        assert_eq!(rows(&carried, table), held, "{table}: {carried}");
    }
    assert_eq!(carried["counter"][0]["id"], json!(["hold", "tools/one"]));
    assert_eq!(carried["callback_state"][0]["id"], json!("amber-heron"));

    let target = Fixture::on(to, Seed::default());
    // What the target held is replaced, the copy's counters and its marker left alone.
    let raw = target.store.raw();
    raw.row("callback_state", "stale-one")
        .create(json!({"address": "gone"}))
        .unwrap();
    raw.row("counter", json!(["hold", "tools/stale"]))
        .create(json!({"last": 0}))
        .unwrap();
    raw.row("counter", json!(["slots", "kept-fence"]))
        .create(json!({"last": 2}))
        .unwrap();
    let counters = raw.row("counter", "message").get().unwrap();
    target.store.carry(&carried).unwrap();
    assert_eq!(
        target.store.carried().unwrap(),
        carried,
        "{from:?} to {to:?}"
    );
    assert_eq!(
        serde_json::to_string(&target.store.carried().unwrap()).unwrap(),
        serde_json::to_string(&carried).unwrap(),
        "one text on both"
    );
    assert!(
        !raw.row("counter", json!(["slots", "kept-fence"]))
            .exists()
            .unwrap(),
        "a counter the source did not hold is gone"
    );
    assert_eq!(
        raw.row("counter", json!(["slots", "amber-heron"]))
            .field("last")
            .unwrap(),
        json!(4),
        "a fence is carried at its count"
    );
    assert_eq!(raw.row("counter", "message").get().unwrap(), counters);
    assert_eq!(
        raw.row("callback_lease", "amber-heron").get().unwrap(),
        json!({"until": 1_900_000_000.0, "revision": 12}),
        "a lease is carried as it is given"
    );
    // Carried again, it is the same board: nothing doubled.
    target.store.carry(&carried).unwrap();
    assert_eq!(target.store.carried().unwrap(), carried);
    // The ledger reads as its writer left it.
    assert_eq!(
        target.store.artifact_ledger("tenant", "project").unwrap(),
        source.store.artifact_ledger("tenant", "project").unwrap()
    );
}

#[test]
fn surrealdbs_rows_are_postgresqls_row_for_row() {
    carried_across(Backend::SurrealDb, Backend::PostgreSql);
}

#[test]
fn postgresqls_rows_are_surrealdbs_row_for_row() {
    carried_across(Backend::PostgreSql, Backend::SurrealDb);
}

on_both! {
    fn an_empty_board_carries_no_row_and_a_table_no_move_carries_is_refused(backend: Backend) {
        let f = Fixture::on(backend, Seed::default());
        let none = f.store.carried().unwrap();
        for table in CARRIED {
            // An opening writes its own line among the writers.
            assert!(table == "writer" || rows(&none, table) == 0, "{table}: {none}");
        }
        let refused = f.store.carry(&json!({"task": []})).unwrap_err();
        assert!(
            matches!(&refused, Error::Refused(why) if why.contains("task is no table a move carries")),
            "{refused:?}"
        );
    }
}

/// `from`'s board moved to `to` as a move does it: the dump copied, the carried tables written
/// with their leases ended, and read back as they were written.
fn moved(from: &Fixture, to: Backend) -> Fixture {
    let target = Fixture::on(
        to,
        Seed {
            dump: from.store.dump().unwrap(),
        },
    );
    let carried = leases_ended(from.store.carried().unwrap());
    target.store.carry(&carried).unwrap();
    let there = target.store.carried().unwrap();
    for table in ["callback_lease", "callback_event", "counter"] {
        let (mut read, mut written) = (there[table].clone(), carried[table].clone());
        counted(&mut read);
        counted(&mut written);
        assert_eq!(read, written, "{table}");
    }
    target
}

/// The keys of the lines waiting in `handle`'s outbox, in order.
fn waiting(f: &Fixture, handle: &str) -> Vec<String> {
    let rows = f.store.carried().unwrap();
    (rows["callback_event"].as_array().unwrap().iter())
        .filter(|row| row["id"][0] == handle)
        .map(|row| row["id"][1].as_str().unwrap().to_string())
        .collect()
}

fn a_send_and_a_lease_meet_the_board_as_it_was(from: Backend, to: Backend) {
    let source = Fixture::on(from, Seed::default());
    let register = || {
        (source.store.register(&NewSession::new("worker")).unwrap())
            .row
            .handle
    };
    let (sender, worker, host) = (register(), register(), "this-host");
    // The worker's channel answers throughout, so a line is held back by a lease alone.
    let listener =
        crate::callbacks::Listener::new(&format!("{}-carried", source.store.database().database()));
    let address = format!("uds:{}", listener.path.display());
    (source.store.callback(&worker, Some(&address), false, false)).unwrap();
    // Each lease held when the board is moved, as a dispatcher, a host and a close hold them.
    let raw = source.store.raw();
    raw.row("callback_lease", worker.as_str())
        .put(json!({"until": source.now() + 600.0, "revision": 7}))
        .unwrap();
    raw.row("callback_lease", json!(["dispatch", host]))
        .put(json!({"until": source.now() + 600.0, "revision": 3}))
        .unwrap();
    let closing = source.store.take_session(&worker, "closing").unwrap();
    // The line waits: another holds the worker's lease.
    assert!(
        !source
            .store
            .send_agent(&sender, &worker, "before the move")
            .unwrap()
    );
    assert_eq!(waiting(&source, &worker), ["agent-1"]);

    let there = moved(&source, to);
    let lease = |f: &Fixture, id: Value| f.store.raw().row("callback_lease", id).get().unwrap();
    let place = |f: &Fixture| {
        (f.store.raw())
            .row("counter", json!(["callback-event", worker]))
            .field("last")
            .unwrap()
    };
    // No lease stands, and each revision is past the one its old holder writes under.
    for (id, revision) in [(json!(worker), 8), (json!(["dispatch", host]), 4)] {
        let carried = lease(&there, id.clone());
        assert_eq!(carried["until"].as_f64(), Some(0.0), "{id}");
        assert_eq!(carried["revision"], json!(revision), "{id}");
    }
    assert_eq!(
        lease(&there, json!(["session", worker]))["until"].as_f64(),
        Some(0.0)
    );
    assert!(
        !there.store.renew_session(&worker, closing).unwrap(),
        "the close that held the session before the move no longer holds it"
    );
    let taken = there.store.take_session(&worker, "restoring").unwrap();
    assert!(taken > closing + 1, "{taken} is past the old {closing}");
    assert!(!there.store.renew_session(&worker, closing).unwrap());
    assert!(there.store.renew_session(&worker, taken).unwrap());
    // The next lines are keyed past the one carried, delivered now or not.
    assert_eq!(place(&there), json!(1));
    for text in ["after the move", "and another"] {
        there.store.send_agent(&sender, &worker, text).unwrap();
    }
    assert_eq!(
        place(&there),
        json!(3),
        "the outbox's place, which a pane's events are keyed by too"
    );
    let keys = waiting(&there, &worker);
    assert!(keys.iter().all(|key| key.starts_with("agent-")), "{keys:?}");

    // And back: the tokens of both boards before are refused, and sends go on.
    let back = moved(&there, from);
    assert_eq!(lease(&back, json!(worker))["until"].as_f64(), Some(0.0));
    for old in [closing, taken] {
        assert!(!back.store.renew_session(&worker, old).unwrap(), "{old}");
    }
    assert!(back.store.take_session(&worker, "closing").unwrap() > taken + 1);
    back.store
        .send_agent(&sender, &worker, "back again")
        .unwrap();
    assert_eq!(place(&back), json!(4));
}

#[test]
fn a_send_after_a_move_to_postgresql_and_back_is_keyed_past_the_lines_carried() {
    a_send_and_a_lease_meet_the_board_as_it_was(Backend::SurrealDb, Backend::PostgreSql);
}

#[test]
fn a_send_after_a_move_to_surrealdb_and_back_is_keyed_past_the_lines_carried() {
    a_send_and_a_lease_meet_the_board_as_it_was(Backend::PostgreSql, Backend::SurrealDb);
}

/// Every number of `value` as what it counts: one engine writes a whole number of seconds
/// `3.0` where the other writes `3`.
fn counted(value: &mut Value) {
    match value {
        Value::Number(number) => *value = json!(number.as_f64()),
        Value::Array(items) => items.iter_mut().for_each(counted),
        Value::Object(fields) => fields.values_mut().for_each(counted),
        _ => {}
    }
}
