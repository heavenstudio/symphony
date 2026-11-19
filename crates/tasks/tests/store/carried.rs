//! What a move of the board to another engine carries beside the dump (`Store::carried`,
//! `Store::carry`): the rows the board's commands keep for sessions and programs and no copy
//! derives, read on one engine and written on the other under the same keys, row for row.

use crate::common::{Backend, Fixture, Seed};
use serde_json::{Value, json};
use symphony_board::{CARRIED, Error};

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
    // A dispatcher's lease, which no move carries: on the new board a holder of this revision
    // finds no lease of its own to write under, and the next one taken is past it.
    raw.row("callback_lease", "amber-heron")
        .create(json!({"until": 1_900_000_000.0, "revision": 12}))
        .unwrap();
    // A fence of a command's own and the copy's counters, which no move carries.
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
        let held = if table == "writer" { 2 } else { 1 };
        assert_eq!(rows(&carried, table), held, "{table}: {carried}");
    }
    assert_eq!(carried["counter"][0]["id"], json!(["hold", "tools/one"]));
    assert_eq!(carried["callback_state"][0]["id"], json!("amber-heron"));

    let target = Fixture::on(to, Seed::default());
    // What the target held is replaced, its own fences and the copy's counters left alone.
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
    assert_eq!(
        raw.row("counter", json!(["slots", "kept-fence"]))
            .field("last")
            .unwrap(),
        json!(2)
    );
    assert!(
        !raw.row("counter", json!(["slots", "amber-heron"]))
            .exists()
            .unwrap(),
        "a fence is not carried"
    );
    assert_eq!(raw.row("counter", "message").get().unwrap(), counters);
    assert_eq!(
        raw.table("callback_lease").count().unwrap(),
        0,
        "no lease is carried"
    );
    assert_eq!(
        raw.row("callback_state", "amber-heron")
            .field("revision")
            .unwrap(),
        json!(12),
        "the revision the next lease is taken past"
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
