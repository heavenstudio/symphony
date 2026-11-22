//! What `settings list` prints of the store's entries.

use super::*;
use crate::settings::Change;

#[test]
fn an_empty_store_says_every_value_is_the_files() {
    assert_eq!(
        lines(&[]),
        ["the board's store holds no setting: every value is the files' (just tasks limits)"]
    );
}

#[test]
fn each_entry_is_listed_with_its_record_and_one_not_read_says_why() {
    let change = |before: Option<Value>, after: Option<Value>, at: f64, by: &str| Change {
        before,
        after,
        at,
        by: by.into(),
    };
    let (first, second) = (1_800_000_000.0, 1_800_000_060.0);
    let entries = [
        Entry {
            key: "agents.models.reviewer".into(),
            value: Some(json!("opus")),
            revision: 2,
            changes: vec![
                change(None, Some(json!("sonnet")), first, "amber-heron"),
                change(
                    Some(json!("sonnet")),
                    Some(json!("opus")),
                    second,
                    "teal-ibis",
                ),
            ],
        },
        Entry {
            key: "limits.low_volume".into(),
            value: None,
            revision: 2,
            changes: vec![
                change(None, Some(json!(2)), first, "amber-heron"),
                change(Some(json!(2)), None, second, "amber-heron"),
            ],
        },
        Entry {
            key: "land.check".into(),
            value: Some(json!(false)),
            revision: 1,
            changes: Vec::new(),
        },
    ];
    let (first, second) = (local_minute(first), local_minute(second));
    let listed = lines(&entries);
    assert_eq!(
        listed[..6],
        [
            "[agents.models] reviewer  \"opus\"".to_string(),
            format!("  {first}  amber-heron: none -> \"sonnet\""),
            format!("  {second}  teal-ibis: \"sonnet\" -> \"opus\""),
            "[limits] low_volume  removed: the files' value holds".to_string(),
            format!("  {first}  amber-heron: none -> 2"),
            format!("  {second}  amber-heron: 2 -> none"),
        ]
    );
    assert_eq!(listed.len(), 7);
    assert!(
        listed[6].starts_with(
            "[land] check  false  NOT READ: the board's store: [land] check is not kept in the \
             board's store"
        ),
        "{}",
        listed[6]
    );
}
