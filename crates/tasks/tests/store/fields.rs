//! A plugin's board fields, on the real store and a scratch database for each case: what a write
//! keeps, what it refuses, and boards of the schema before the field (9) meeting this one's.

use super::common::{Fixture, RawWrite, Seed, refused};
use serde_json::{Value, json};

/// The task's raw record, as a reader of any version reads it.
fn record(f: &Fixture, slug: &str) -> Value {
    f.other_client()
        .query(
            "SELECT * OMIT id FROM ONLY type::record('task', $slug);",
            json!({"slug": slug}),
        )
        .unwrap()
        .pop()
        .unwrap()
}

fn defines_fields(f: &Fixture) -> bool {
    let found = f
        .other_client()
        .query("RETURN (INFO FOR TABLE task).fields.fields;", json!({}))
        .unwrap();
    found.last().is_some_and(|definition| !definition.is_null())
}

fn schema_version(f: &Fixture) -> Value {
    f.other_client()
        .query(
            "RETURN (SELECT VALUE version FROM ONLY meta:schema);",
            json!({}),
        )
        .unwrap()
        .pop()
        .unwrap()
}

#[test]
fn a_field_is_kept_under_its_plugin_beside_the_others_and_said_on_the_thread() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    f.task("sim/jump");
    let other = f.task("sim/dock");
    let version = record(&f, "sim/jump")["version"].as_i64().unwrap();
    f.advance(1.0);
    let set = f
        .store
        .set_field("sim/jump", "game", "effort", &json!(3), "brisk-finch")
        .unwrap();
    assert_eq!(
        set.fields,
        [("game".into(), [("effort".into(), json!(3))].into())].into()
    );
    assert_eq!(record(&f, "sim/jump")["version"], json!(version + 1));
    f.store
        .set_field("sim/jump", "game", "risk", &json!("high"), "brisk-finch")
        .unwrap();
    f.store
        .set_field("sim/jump", "other-plugin", "owner_x", &json!(1.5), "owner")
        .unwrap();
    let last = f
        .store
        .set_field("sim/jump", "game", "effort", &json!(5), "brisk-finch")
        .unwrap();
    assert_eq!(
        serde_json::to_value(&last.fields).unwrap(),
        json!({"game": {"effort": 5, "risk": "high"}, "other-plugin": {"owner_x": 1.5}})
    );
    assert_eq!(last, f.store.get("sim/jump").unwrap());
    assert_eq!(
        record(&f, "sim/jump")["fields"],
        json!({"game": {"effort": 5, "risk": "high"}, "other-plugin": {"owner_x": 1.5}})
    );
    let said = f.said("sim/jump");
    assert_eq!(
        said[said.len() - 4..],
        [
            "brisk-finch set game.effort to 3",
            "brisk-finch set game.risk to high",
            "owner set other-plugin.owner_x to 1.5",
            "brisk-finch set game.effort to 5",
        ]
    );
    assert_eq!(f.store.get("sim/dock").unwrap(), other);
}

#[test]
fn a_task_never_given_a_field_carries_none_for_a_reader_from_before_it() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    let made = f.task("sim/jump");
    assert!(made.fields.is_empty());
    let raw = record(&f, "sim/jump");
    assert!(raw.get("fields").is_none(), "{raw}");
    f.store.set_priority("sim/jump", 0, "brisk-finch").unwrap();
    assert!(record(&f, "sim/jump").get("fields").is_none());
    let dumped = serde_json::to_value(f.store.dump().unwrap()).unwrap();
    assert!(dumped["tasks"][0].get("fields").is_none(), "{dumped}");
}

#[test]
fn bad_names_values_tasks_and_actors_leave_the_board_unchanged() {
    let f = Fixture::new(
        Seed::default()
            .session("brisk-finch")
            .session("amber-heron"),
    );
    f.task("sim/jump");
    f.store.leave("amber-heron").unwrap();
    let before = f.store.dump().unwrap();
    f.advance(1.0);
    for (plugin, name, why) in [
        (
            "",
            "effort",
            "a plugin name is letters, digits, - and _, not ''",
        ),
        (
            "game.x",
            "effort",
            "a plugin name is letters, digits, - and _, not 'game.x'",
        ),
        (
            "game",
            "",
            "a field name is letters, digits, - and _, not ''",
        ),
        (
            "game",
            "a b",
            "a field name is letters, digits, - and _, not 'a b'",
        ),
    ] {
        assert_eq!(
            refused(
                f.store
                    .set_field("sim/jump", plugin, name, &json!(1), "brisk-finch")
            ),
            why
        );
    }
    for value in [json!(null), json!(true), json!([1]), json!({"a": 1})] {
        assert_eq!(
            refused(
                f.store
                    .set_field("sim/jump", "game", "effort", &value, "brisk-finch")
            ),
            format!("a field holds a number or text, not {value}")
        );
    }
    for (slug, actor, why) in [
        ("sim/missing", "brisk-finch", "no task 'sim/missing'"),
        (
            "sim/jump",
            "ghost-owl",
            "no session 'ghost-owl'; attach first (just tasks-attach)",
        ),
        (
            "sim/jump",
            "amber-heron",
            "session amber-heron has left; attach again",
        ),
    ] {
        assert_eq!(
            refused(f.store.set_field(slug, "game", "effort", &json!(1), actor)),
            why
        );
    }
    assert_eq!(f.store.dump().unwrap(), before);
}

#[test]
fn a_board_of_schema_nine_gains_the_field_when_this_schema_opens_it() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    f.task("sim/jump");
    f.other_client()
        .write(
            "BEGIN TRANSACTION; REMOVE FIELD fields ON task; UPSERT meta:schema SET version = 9; COMMIT TRANSACTION;",
            json!({}),
        )
        .unwrap();
    assert!(!defines_fields(&f));
    let reopened = f.other();
    assert!(defines_fields(&f));
    assert_eq!(schema_version(&f), json!(12));
    let set = reopened
        .set_field("sim/jump", "game", "effort", &json!("4"), "brisk-finch")
        .unwrap();
    assert_eq!(
        serde_json::to_value(&set.fields).unwrap(),
        json!({"game": {"effort": "4"}})
    );
}

/// A schema-9 open runs its own definitions, every one `IF NOT EXISTS`, and names version 9: the
/// field's definition and a task's fields stay, and the next open of this schema names 11 again.
#[test]
fn an_open_from_schema_nine_keeps_the_field_and_its_values() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    f.task("sim/jump");
    f.store
        .set_field("sim/jump", "game", "effort", &json!(2), "brisk-finch")
        .unwrap();
    f.other_client()
        .write(
            "BEGIN TRANSACTION;
DEFINE TABLE IF NOT EXISTS task SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS version ON task TYPE int DEFAULT 0;
UPSERT meta:schema SET version = 9;
COMMIT TRANSACTION;",
            json!({}),
        )
        .unwrap();
    assert!(defines_fields(&f));
    assert_eq!(
        record(&f, "sim/jump")["fields"],
        json!({"game": {"effort": 2}})
    );
    let reopened = f.other();
    assert_eq!(schema_version(&f), json!(12));
    assert_eq!(
        serde_json::to_value(reopened.get("sim/jump").unwrap().fields).unwrap(),
        json!({"game": {"effort": 2}})
    );
}
