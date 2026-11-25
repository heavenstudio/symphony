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

#[test]
fn sorted_reversed_and_repeated_large_inputs_match_collection() {
    for len in [1, 32, 1024] {
        let forward: Vec<_> = (0..len).map(|n| (format!("key-{n:04}"), n)).collect();
        for pairs in [
            forward.clone(),
            forward.iter().rev().cloned().collect(),
            forward
                .iter()
                .cloned()
                .chain(
                    forward
                        .iter()
                        .rev()
                        .map(|(key, value)| (key.clone(), value + len)),
                )
                .collect(),
        ] {
            let expected: BTreeMap<_, _> = pairs.clone().into_iter().collect();
            assert_eq!(of(pairs), expected);
        }
    }
}

#[test]
fn equal_path_keys_keep_the_same_representation_as_collection() {
    use std::path::PathBuf;

    let pairs = vec![
        (PathBuf::from("folder/./file"), 1),
        (PathBuf::from("folder/file"), 2),
    ];
    let expected: BTreeMap<_, _> = pairs.clone().into_iter().collect();
    let map = of(pairs);
    let (key, value) = map.first_key_value().unwrap();
    let (expected_key, expected_value) = expected.first_key_value().unwrap();
    assert_eq!(key.as_os_str(), expected_key.as_os_str());
    assert_eq!(value, expected_value);
}

#[test]
fn ordered_store_payloads_keep_last_values_and_report_collection_cost() {
    fn compare<V: Clone>(name: &str, pairs: Vec<(String, V)>, fingerprint: impl Fn(&V) -> String) {
        for reversed in [false, true] {
            let mut input = pairs.clone();
            crate::order::sort_by(&mut input, &|a, b| a.0.cmp(&b.0));
            if reversed {
                input.reverse();
            }
            let copied = input.clone();
            let (expected, old_cpu) =
                test_time::cpu::spent(|| copied.into_iter().collect::<BTreeMap<_, _>>());
            let (actual, new_cpu) = test_time::cpu::spent(|| input.into_iter().collect_map());
            let fields = |map: &BTreeMap<String, V>| {
                map.iter()
                    .map(|(key, value)| (key.clone(), fingerprint(value)))
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                fields(&actual),
                fields(&expected),
                "{name}, reversed={reversed}"
            );
            eprintln!(
                "{name} collector: pairs={}, unique={}, pair_bytes={}, reversed={reversed}, standard_thread_cpu={old_cpu:?}, insertion_thread_cpu={new_cpu:?}; existing pair Vec reused, tree nodes replace bulk construction; no stable-sort scratch in insertion helper",
                pairs.len(),
                actual.len(),
                std::mem::size_of::<(String, V)>()
            );
        }
    }
    let bindings = (0..512)
        .map(|index| {
            (
                (index % 73).to_string(),
                crate::Binding {
                    agent: index.to_string(),
                    branch: format!("branch-{index}"),
                    worktree: "/fixture".into(),
                    ..crate::Binding::default()
                },
            )
        })
        .collect();
    compare("Binding", bindings, |value| format!("{value:?}"));
    let messages = (0..512)
        .map(|index| {
            (
                (index % 73).to_string(),
                crate::Message {
                    id: index,
                    slug: "work".into(),
                    sender: format!("sender-{index}"),
                    recipient: None,
                    kind: "msg".into(),
                    text: format!("message-{index}"),
                    ts: 1.0,
                },
            )
        })
        .collect();
    compare("Message", messages, |value| format!("{value:?}"));
    let callbacks: Vec<(String, crate::Callback)> =
        (0..512)
            .map(|index| {
                ((index % 73).to_string(),
        serde_json::from_value(serde_json::json!({
            "handle":format!("handle-{index}"),"address":format!("codex:fixture-{index}"),
            "host":"fixture","chat":index%3==0,"session":crate::rows::SessionRow::default()
        })).unwrap()
    )
            })
            .collect();
    compare("Callback", callbacks, |value| {
        format!(
            "{:?}",
            (
                &value.handle,
                &value.address,
                &value.host,
                value.chat,
                &value.terminal
            )
        )
    });
}

#[test]
fn outer_thread_slot_and_descendant_payload_maps_keep_complete_values_and_report_cost() {
    fn compare<K: Clone + Ord + std::fmt::Debug, V: Clone + PartialEq + std::fmt::Debug>(
        label: &str,
        pairs: Vec<(K, V)>,
    ) {
        for count in [0, 1, 32, 512] {
            for reversed in [false, true] {
                let mut input = pairs[..count].to_vec();
                crate::order::sort_by(&mut input, &|a, b| a.0.cmp(&b.0));
                if reversed {
                    input.reverse();
                }
                let old_input = input.clone();
                let (old, old_cpu) =
                    test_time::cpu::spent(|| old_input.into_iter().collect::<BTreeMap<_, _>>());
                let (new, new_cpu) = test_time::cpu::spent(|| input.into_iter().collect_map());
                assert_eq!(new, old, "{label} n={count} reversed={reversed}");
                eprintln!(
                    "{label} n={count},reversed={reversed}: old_cpu={old_cpu:?},insert_cpu={new_cpu:?}; pair={}bytes, existing pair Vec retained, no stable-sort scratch",
                    std::mem::size_of::<(K, V)>()
                );
            }
        }
    }
    compare(
        "threads",
        (0..512)
            .map(|i| {
                (
                    format!("task-{}", i % 73),
                    vec![crate::Message {
                        id: i,
                        slug: format!("task-{i}"),
                        sender: "owner".into(),
                        recipient: Some("worker".into()),
                        kind: "msg".into(),
                        text: format!("Exact metadata {i}"),
                        ts: 1.0,
                    }],
                )
            })
            .collect(),
    );
    compare(
        "slot holdings",
        (0..512)
            .map(|i| {
                (
                    format!("owner-{}", i % 73),
                    std::collections::BTreeSet::from([format!("slot-{i}"), "shared".into()]),
                )
            })
            .collect(),
    );
    compare(
        "latest descendant",
        (0..512)
            .map(|i| {
                (
                    (format!("handle-{}", i % 7), format!("task-{}", i % 13)),
                    i64::from(i),
                )
            })
            .collect(),
    );
}
