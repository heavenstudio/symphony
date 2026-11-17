//! The feed's keyed positions retain lexical order and the last duplicate without cloning items.
use super::{diff_items, keyed};
use serde_json::json;

#[test]
fn keyed_positions_borrow_in_key_order_and_keep_the_last_duplicate() {
    struct Item(&'static str, usize);
    let items = [
        Item("z", 1),
        Item("猫", 2),
        Item("", 3),
        Item("a", 4),
        Item("z", 5),
    ];
    let mapped = keyed(&items, |item| item.0.into());
    assert_eq!(
        mapped.keys().map(String::as_str).collect::<Vec<_>>(),
        ["", "a", "z", "猫"]
    );
    assert_eq!(items[mapped["z"]].1, 5);
    assert!(std::ptr::eq(&items[mapped["z"]], &items[4]));
    assert!(keyed::<Item>(&[], |item| item.0.into()).is_empty());
}

#[test]
fn changed_lists_keep_their_exact_upserts_removals_and_predecessors() {
    let old = [json!({"id":"z", "value":1}), json!({"id":"a", "value":2})];
    let new = [json!({"id":"a", "value":3}), json!({"id":"m", "value":0})];
    let mut events = vec![];
    let key = |item: &serde_json::Value| item["id"].as_str().unwrap().to_owned();
    diff_items(&mut events, "row", &old, &new, key);
    assert_eq!(
        events,
        vec![
            json!({"region":"row", "data":{"key":"a", "value":new[0]}}),
            json!({"region":"row", "data":{"key":"m", "value":new[1]}}),
            json!({"region":"row", "data":{"key":"z", "removed":true}}),
            json!({"region":"row-order", "data":{"key":"a", "after":null}}),
            json!({"region":"row-order", "data":{"key":"m", "after":"a"}}),
        ]
    );
    events.clear();
    diff_items(&mut events, "row", &new, &new, key);
    assert!(events.is_empty());
}

#[test]
fn keyed_envelopes_keep_exact_json_bytes_for_scalar_and_nested_values() {
    for value in [
        json!(null),
        json!(true),
        json!(i64::MIN),
        json!(u64::MAX),
        json!(-0.0),
        json!("é\n\"😀"),
        json!([1, {"nested": [false, null]}]),
    ] {
        assert_eq!(
            super::region_event("row", value.clone()).to_string(),
            json!({"region":"row", "data":value}).to_string(),
        );
        let expected = json!({"region":"row", "data":{"key":"猫", "value":value}});
        assert_eq!(
            super::keyed_event("row", "猫", "value", value).to_string(),
            expected.to_string(),
        );
    }
    for after in [None, Some(""), Some("é\n😀")] {
        assert_eq!(
            super::keyed_event("row-order", "", "after", after.into()).to_string(),
            json!({"region":"row-order", "data":{"key":"", "after":after}}).to_string(),
        );
    }
    assert_eq!(
        super::keyed_event("row", "z", "removed", true.into()).to_string(),
        r#"{"region":"row","data":{"key":"z","removed":true}}"#,
    );
}

#[test]
fn reordering_borrowed_items_sends_only_the_changed_predecessors() {
    let old = ["猫", "", "a"];
    let new = ["a", "猫", ""];
    let mut events = vec![];
    diff_items(&mut events, "row", &old, &new, |s| (*s).to_owned());
    assert_eq!(
        events,
        vec![
            json!({"region":"row-order", "data":{"key":"a", "after":null}}),
            json!({"region":"row-order", "data":{"key":"猫", "after":"a"}}),
        ]
    );
    events.clear();
    diff_items(&mut events, "row", &[] as &[&str], &[], |s| (*s).to_owned());
    assert!(events.is_empty());
}
