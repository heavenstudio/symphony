//! The feed's keyed views retain lexical order and the last duplicate without cloning items.
use super::{diff_items, keyed};
use serde_json::json;

#[test]
fn keyed_views_borrow_in_key_order_and_keep_the_last_duplicate() {
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
    assert_eq!(mapped["z"].1, 5);
    assert!(std::ptr::eq(mapped["z"], &items[4]));
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
