//! What a restore reads beside the dump (`Store::carried`): the rows the board's commands keep for
//! sessions and programs, and who is on the board, and no copy derives, each with its key, in the
//! order of its key.

use crate::common::{Fixture, Seed};
use serde_json::{Value, json};
use symphony_board::CARRIED;

/// A row of every carried table as the board's commands leave them: a compound key, a number with
/// no fraction and one with, a nested object whose keys are out of order, an array, an empty text.
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
    // Who is on the board, a group and the log of the change that made it: the board's own word.
    raw.row("people", "ana")
        .create(json!({"name": "ana", "groups": ["staff"], "powers": ["decide"], "revision": 2}))
        .unwrap();
    raw.row("people_group", "staff")
        .create(json!({"name": "staff", "powers": ["speak"], "revision": 1}))
        .unwrap();
    raw.row("people_log", json!(1))
        .create(json!({
            "n": 1, "at": 1_800_000_004.0, "by": "amber-heron", "did": "group-add",
            "name": "staff", "powers": ["speak"],
        }))
        .unwrap();
    // The map of places to groups, a line of its log: the board's own word too.
    raw.row("ownership", json!(1))
        .create(json!({
            "n": 1, "at": 1_800_000_004.5, "by": "amber-heron", "did": "map",
            "place": "area:app", "name": "staff",
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

#[test]
fn a_row_of_every_carried_table_is_read_under_its_key() {
    let f = Fixture::new(Seed::default());
    seeded(&f);
    let carried = f.store.carried().unwrap();
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
    assert_eq!(carried["counter"][1]["id"], json!(["slots", "amber-heron"]));
    assert_eq!(
        carried["counter"][1]["last"],
        json!(4),
        "a fence at its count"
    );
    assert_eq!(carried["callback_state"][0]["id"], json!("amber-heron"));
    assert_eq!(
        carried["callback_lease"][0],
        json!({"id": "amber-heron", "until": 1_900_000_000.0, "revision": 12}),
        "a lease as it is given"
    );
    assert_eq!(
        f.store.carried().unwrap(),
        carried,
        "read again, the same rows"
    );
}

#[test]
fn an_empty_board_carries_no_row() {
    let f = Fixture::new(Seed::default());
    let none = f.store.carried().unwrap();
    for table in CARRIED {
        // An opening writes its own line among the writers.
        assert!(
            table == "writer" || rows(&none, table) == 0,
            "{table}: {none}"
        );
    }
}
