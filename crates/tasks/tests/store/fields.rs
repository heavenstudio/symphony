//! A plugin's board fields, on the real store and a scratch database for each case: what a write
//! keeps, and what it refuses.

use super::common::{Fixture, Seed, refused};
use serde_json::{Value, json};
use symphony_board::Raw;

/// The task's raw record, as a reader of any version reads it.
fn record(f: &Fixture, slug: &str) -> Value {
    let db = f.other_database();
    Raw::of(&db).row("task", slug).get().unwrap()
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
    let dumped = serde_json::to_value(f.rows()).unwrap();
    let task = dumped["task"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["slug"] == "sim/jump")
        .expect("the task is present");
    assert!(task.get("fields").is_none(), "{task}");
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
    let before = f.rows();
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
    let ghost = f.no_session("ghost-owl");
    for (slug, actor, why) in [
        ("sim/missing", "brisk-finch", "no task 'sim/missing'"),
        ("sim/jump", "ghost-owl", ghost.as_str()),
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
    assert_eq!(f.rows(), before);
}
