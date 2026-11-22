//! What the board's store may hold, how a key names its setting, and how its entries are read.

use super::*;
use serde_json::json;

fn entry(key: &str, value: Option<Json>) -> Entry {
    Entry {
        key: key.into(),
        value,
        revision: 1,
        changes: Vec::new(),
    }
}

#[test]
fn a_key_is_its_tables_and_its_key_joined_by_dots() {
    assert_eq!(
        path("limits.low_volume"),
        Ok((vec!["limits"], "low_volume"))
    );
    assert_eq!(
        path("agents.launch.model"),
        Ok((vec!["agents", "launch"], "model"))
    );
    for none in ["", "limits", ".low_volume", "limits.", "agents..model", "."] {
        let why = path(none).unwrap_err();
        assert!(why.contains("names no setting"), "{none}: {why}");
    }
    assert_eq!(named("limits.low_volume"), "[limits] low_volume");
    assert_eq!(named("agents.models.reviewer"), "[agents.models] reviewer");
    // The launch's keys as the project file's lines name them.
    assert_eq!(named("agents.launch.model"), "[agents] launch model");
    assert_eq!(named("agents.panes"), "[agents] panes");
    assert_eq!(named("limits"), "limits");
}

#[test]
fn a_command_lines_text_is_a_whole_number_for_a_limit_and_text_for_any_other() {
    assert_eq!(value_of("limits.low_volume", " 3 "), Value::Integer(3));
    assert_eq!(value_of("limits.low_volume", "-1"), Value::Integer(-1));
    assert_eq!(
        value_of("limits.low_volume", "3.5"),
        Value::String("3.5".into())
    );
    assert_eq!(
        value_of("limits.low_volume", "many"),
        Value::String("many".into())
    );
    // A model named by a number stays text, and so does a key that names no setting.
    assert_eq!(
        value_of("agents.models.reviewer", "5"),
        Value::String("5".into())
    );
    assert_eq!(value_of("limits", "5"), Value::String("5".into()));
}

#[test]
fn a_value_is_kept_as_json_and_read_back_as_the_file_would_write_it() {
    for value in [
        Value::Integer(3),
        Value::String("opus".into()),
        Value::Boolean(true),
        Value::Float(1.5),
    ] {
        assert_eq!(written(&kept(&value)), Some(value));
    }
    // What no setting holds is kept as its kind, and never read back.
    assert_eq!(kept(&Value::Array(Vec::new())), json!("array"));
    for none in [json!(null), json!([1]), json!({"a": 1})] {
        assert_eq!(written(&none), None, "{none}");
    }
    assert_eq!(shown(&json!("opus")), "\"opus\"");
    assert_eq!(shown(&json!(3)), "3");
}

#[test]
fn a_value_is_judged_by_the_local_files_rules_in_the_stores_words() {
    let limit = judged("limits.low_volume", &Value::Integer(0)).unwrap();
    assert_eq!(limit.limits.get("low_volume"), Some(&0));
    let panes = judged("agents.panes", &Value::String("none".into())).unwrap();
    assert_eq!(panes.panes.as_deref(), Some("none"));
    let model = judged("agents.models.executor", &Value::String(String::new())).unwrap();
    assert_eq!(model.model_of("executor"), Some(""));
    for (key, value, why) in [
        (
            "limits.low_volume",
            Value::Integer(-1),
            "the board's store: [limits] low_volume is -1; it is at least 0",
        ),
        (
            "limits.machine_agents",
            Value::Boolean(true),
            "the board's store: [limits] machine_agents is true; not a whole number",
        ),
        (
            "agents.models.reviewer",
            Value::Integer(4),
            "the board's store: [agents.models] reviewer is 4; not text",
        ),
        (
            "agents.panes",
            Value::Integer(4),
            "the board's store: [agents] panes is 4; not text",
        ),
        (
            "releases.every_landings",
            Value::Integer(4),
            "the board's store: [releases] every_landings is not kept in the board's store: it \
             holds a command or a program the Symphony runs, or decides what lands, and a place \
             no commit records must not change that; it stays in symphony.toml",
        ),
        (
            "project.name",
            Value::String("x".into()),
            "the board's store: [project] name is not a setting the board's store keeps: it \
             holds [agents] launch, [agents.models], [agents] panes and [limits] alone",
        ),
        (
            "agents.launch.default.more",
            Value::String("x".into()),
            "the board's store: [agents.launch] default is table; not text",
        ),
    ] {
        assert_eq!(judged(key, &value).unwrap_err(), why, "{key}");
    }
}

#[test]
fn every_setting_the_local_file_protects_is_refused_by_the_store_from_the_same_lists() {
    let value = Value::String("x".into());
    let keys = (crate::local::PROTECTED
        .iter()
        .map(|table| format!("{table}.any")))
    .chain(
        (crate::local::PROTECTED_KEYS.iter())
            .flat_map(|(table, keys)| keys.iter().map(move |key| format!("{table}.{key}"))),
    );
    let mut refused = 0;
    for key in keys {
        let why = judged(&key, &value).unwrap_err();
        assert!(
            why.contains("is not kept in the board's store"),
            "{key}: {why}"
        );
        refused += 1;
    }
    assert_eq!(refused, 8 + 2 + 6);
}

#[test]
fn each_entry_is_judged_alone_and_one_refused_is_said_and_left_out() {
    let stored = Stored::of(&[
        entry("limits.low_volume", Some(json!(1))),
        entry("land.check", Some(json!(false))),
        entry("limits.machine_agents", None),
        entry("agents.models.reviewer", Some(json!("opus"))),
        entry("limits.orchestrator_slots", Some(json!([2]))),
        entry("agents.panes", Some(json!("hc"))),
    ]);
    assert_eq!(
        stored.keys,
        [
            "limits.low_volume",
            "agents.models.reviewer",
            "agents.panes"
        ]
    );
    assert!(stored.holds("agents.panes") && !stored.holds("land.check"));
    assert!(
        !stored.holds("limits.machine_agents"),
        "a removed entry holds nothing"
    );
    assert!(!stored.is_empty());
    assert_eq!(stored.held.limits.len(), 1);
    assert_eq!(stored.held.model_of("reviewer"), Some("opus"));
    assert_eq!(stored.held.panes.as_deref(), Some("hc"));
    assert_eq!(stored.problems.len(), 2);
    assert!(stored.problems[0].starts_with("the board's store: [land] check is not kept"));
    assert_eq!(
        stored.problems[1],
        "the board's store: [limits] orchestrator_slots is of no kind a setting holds; its entry \
         is not read, the files' value holds"
    );
    assert!(Stored::of(&[]).is_empty());
    let unread = Stored::unread("no answer");
    assert!(unread.is_empty());
    assert_eq!(
        unread.problems,
        [
            "the board's store cannot be read (no answer); none of it is read, the files' settings hold"
        ]
    );
    assert_eq!(
        Stored::written("agents.launch.model").said(),
        "the board's store [agents] launch model"
    );
}

#[test]
fn a_change_records_what_it_held_before_and_after_and_who_made_it_when() {
    let first = entry("limits.low_volume", None).changed(Some(json!(2)), 10.0, "amber-heron");
    let removed = first.changed(None, 20.0, "the page");
    assert_eq!((removed.value.clone(), removed.revision), (None, 3));
    assert_eq!(
        removed.changes,
        [
            Change {
                before: None,
                after: Some(json!(2)),
                at: 10.0,
                by: "amber-heron".into()
            },
            Change {
                before: Some(json!(2)),
                after: None,
                at: 20.0,
                by: "the page".into()
            },
        ]
    );
    // A row is written with no null in it, and read back as it was.
    let row = serde_json::to_value(&removed).unwrap();
    assert_eq!(row.get("value"), None);
    assert_eq!(row["changes"][0].get("before"), None);
    assert_eq!(serde_json::from_value::<Entry>(row).unwrap(), removed);
}
