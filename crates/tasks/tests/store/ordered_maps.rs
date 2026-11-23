//! Complete public reads on each engine through the three ordered-map collectors.
use crate::common::{Fixture, HOST, NOW, Seed, session};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn ordered_collectors_keep_empty_complete_values_lexical_keys_and_chat_projection() {
    let z = symphony_board::rows::SessionRow {
        kind: "orchestrator".into(),
        ..session("z")
    };
    let a = symphony_board::rows::SessionRow {
        kind: "orchestrator".into(),
        ..session("a")
    };
    let f = Fixture::new(Seed::default().session_row(z).session_row(a));
    assert!(f.store.agent_bindings().unwrap().is_empty());
    assert!(f.store.last_messages().unwrap().is_empty());
    assert!(f.store.callbacks().unwrap().is_empty());
    assert!(f.store.bind_agent("agent-z", "z", true).unwrap());
    assert!(f.store.bind_agent("agent-a", "a", true).unwrap());
    let bindings = f.store.agent_bindings().unwrap();
    assert_eq!(
        bindings,
        BTreeMap::from([
            (
                "a".into(),
                symphony_board::Binding {
                    agent: "agent-a".into(),
                    ..symphony_board::Binding::default()
                }
            ),
            (
                "z".into(),
                symphony_board::Binding {
                    agent: "agent-z".into(),
                    ..symphony_board::Binding::default()
                }
            ),
        ])
    );
    assert_eq!(
        bindings.keys().map(String::as_str).collect::<Vec<_>>(),
        ["a", "z"]
    );
    f.task("work");
    f.store.say("work", "z", "earlier", None).unwrap();
    let a_word = f.store.say("work", "a", "A word", None).unwrap();
    let z_word = f.store.say("work", "z", "latest", None).unwrap();
    assert_eq!(
        f.store.last_messages().unwrap(),
        BTreeMap::from([("z".into(), z_word), ("a".into(), a_word)])
    );
    // Raw fixture channels exercise the read without contacting a real native conversation.
    for handle in ["z", "a"] {
        f.store
            .raw()
            .row("callback_state", format!("fixture-{handle}"))
            .put(json!({
                "handle":handle,"address":format!("codex:fixture-{handle}"),"host":HOST,
                "chat":false,"confirmed_until":NOW+600.0
            }))
            .unwrap();
    }
    let project = |rows: BTreeMap<String, symphony_board::Callback>| {
        rows.into_iter()
            .map(|(key, row)| {
                (
                    key,
                    (row.handle, row.address, row.host, row.chat, row.terminal),
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = |chat: &str| {
        ["a", "z"]
            .into_iter()
            .map(|handle| {
                (
                    handle.to_owned(),
                    (
                        handle.to_owned(),
                        format!("codex:fixture-{handle}"),
                        HOST.to_owned(),
                        handle == chat,
                        None,
                    ),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        project(f.store.callbacks().unwrap()),
        expected("a"),
        "lexical live orchestrator fallback"
    );
    f.store
        .raw()
        .row("callback_state", "fixture-z")
        .set(json!({"chat":true}))
        .unwrap();
    assert_eq!(
        project(f.store.callbacks().unwrap()),
        expected("z"),
        "explicit chat beats lexical fallback"
    );
    f.store
        .raw()
        .row("callback_state", "fixture-z")
        .delete()
        .unwrap();
    let only = project(f.store.callbacks().unwrap());
    assert_eq!(only, expected("a")[..1]);
    f.store
        .raw()
        .row("callback_state", "fixture-a")
        .delete()
        .unwrap();
    assert!(f.store.callbacks().unwrap().is_empty());
}

#[test]
fn outer_threads_keep_complete_metadata_duplicates_empty_unknown_and_one_read() {
    let f = Fixture::new(
        Seed::default()
            .task("a", "ready")
            .task("z", "ready")
            .task("empty", "ready"),
    );
    let a = f.store.say("a", "owner", "Exact A", None).unwrap();
    let z = f.store.say("z", "owner", "Exact Z", None).unwrap();
    f.advance(1.0);
    let reply = f
        .store
        .say("z", "owner", "Exact later Z", Some("@orchestrator"))
        .unwrap();
    let expected = BTreeMap::from([
        ("a".into(), vec![a]),
        ("z".into(), vec![z, reply]),
        ("empty".into(), vec![]),
    ]);
    let before = f.store.database().requests();
    let actual = f
        .store
        .threads(&["z".into(), "a".into(), "z".into(), "empty".into()])
        .unwrap();
    assert_eq!(f.store.database().requests() - before, 1);
    assert_eq!(actual, expected);
    assert_eq!(
        actual.keys().map(String::as_str).collect::<Vec<_>>(),
        ["a", "empty", "z"]
    );
    assert!(f.store.threads(&[]).unwrap().is_empty());
    assert_eq!(
        crate::common::refused(f.store.threads(&[
            "missing-a".into(),
            "a".into(),
            "missing-z".into()
        ])),
        "no task 'missing-a'"
    );
}
