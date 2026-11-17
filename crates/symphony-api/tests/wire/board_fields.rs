//! A task's board fields (`symphony_blocks::FieldValue`, each plugin's values) on the task the server
//! sends.
use serde_json::Value;
use std::collections::BTreeMap;
use symphony_api::{FieldValue, Role, Task};

fn task(fields: BTreeMap<String, BTreeMap<String, FieldValue>>) -> Task {
    Task {
        slug: "game/jump".into(),
        title: "Jump".into(),
        body: String::new(),
        status: "ready".into(),
        priority: 2,
        parent: None,
        created_by: "owner".into(),
        created: 1_762_912_345.25,
        updated: 1_762_912_400.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: vec![Role {
            handle: "amber-heron".into(),
            role: "executor".into(),
            since: 1_762_912_350.5,
            expires: None,
            health: "live".into(),
        }],
        instruction: false,
        fields,
        waits_on: None,
        review_rounds: None,
        from: None,
    }
}

/// The server writes a whole number as `3.0`, a float to `serde_json`: the task it sends reads
/// back from that text, as it does from a value.
#[test]
fn a_task_with_board_fields_reads_back_from_the_json_the_server_writes() {
    let task = task(BTreeMap::from([(
        "game".into(),
        BTreeMap::from([
            ("effort".into(), FieldValue::Number(3.0)),
            ("share".into(), FieldValue::Number(0.25)),
            ("risk".into(), FieldValue::Text("high".into())),
            ("doc".into(), FieldValue::Text("docs/A.md".into())),
        ]),
    )]));
    let text = serde_json::to_string(&task).unwrap();
    assert!(text.contains(r#""effort":3.0"#), "{text}");
    assert_eq!(serde_json::from_str::<Task>(&text).unwrap(), task);
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(serde_json::from_value::<Task>(value).unwrap(), task);
    assert_eq!(
        serde_json::from_value::<Task>(serde_json::to_value(&task).unwrap()).unwrap(),
        task
    );
}

#[test]
fn a_task_without_board_fields_writes_none_and_reads_with_none() {
    let task = task(BTreeMap::new());
    let text = serde_json::to_string(&task).unwrap();
    assert!(!text.contains("fields"), "{text}");
    assert_eq!(serde_json::from_str::<Task>(&text).unwrap(), task);
}
