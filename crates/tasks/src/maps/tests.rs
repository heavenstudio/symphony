use super::{Collect as _, of};
use std::collections::BTreeMap;

/// Pairs out of key order, a key given twice with different values.
fn pairs() -> Vec<(String, u32)> {
    [("c", 1), ("a", 2), ("b", 3), ("a", 4), ("c", 5), ("d", 6)]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

#[test]
fn a_map_collected_through_a_vec_is_the_map_collect_makes() {
    let collected: BTreeMap<String, u32> = pairs().into_iter().collect();
    assert_eq!(pairs().into_iter().collect_map(), collected);
    assert_eq!(of(pairs()), collected);
}

#[test]
fn a_key_given_twice_keeps_its_last_value_and_the_keys_are_in_order() {
    let map = pairs().into_iter().collect_map();
    let keys: Vec<&str> = map.keys().map(String::as_str).collect();
    assert_eq!(keys, ["a", "b", "c", "d"]);
    assert_eq!((map["a"], map["c"]), (4, 5));
}

#[test]
fn no_pairs_make_an_empty_map() {
    assert!(std::iter::empty::<(String, u32)>().collect_map().is_empty());
    assert!(of(Vec::<(String, u32)>::new()).is_empty());
}

#[test]
fn an_adapted_iterator_is_collected_as_its_pairs() {
    let pairs = pairs();
    let map = (pairs.iter())
        .filter(|(_, value)| value % 2 == 0)
        .map(|(key, value)| (key.as_str(), value * 10))
        .collect_map();
    assert_eq!(map, BTreeMap::from([("a", 40), ("d", 60)]));
}
