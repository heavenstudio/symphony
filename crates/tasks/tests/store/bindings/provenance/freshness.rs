//! A same-key classification against a legacy binding: either serial order of a full-session bind
//! and an in-process one ends with the full binding, and the legacy classification's cleanup is
//! done once, its matching retry writing nothing.

use crate::common::Fixture;
use serde_json::{Value, json};
use symphony_board::{Binding, Store};

const KEY: &str = "legacy-key";
const CHILD: &str = "new-child";

struct Legacy {
    fixture: Fixture,
    handle: String,
}

impl Legacy {
    fn new() -> Self {
        let fixture = Fixture::empty();
        let handle = fixture.session("worker");
        let raw = fixture.store.raw();
        (raw.row("session", handle.as_str()))
            .set(json!({"agent": KEY}))
            .unwrap();
        (raw.row("callback_state", handle.as_str()))
            .put(json!({"marker": "keep"}))
            .unwrap();
        (raw.row("callback_event", json!([handle, "chat-test"])))
            .put(json!({
                "handle": handle,
                "key": "chat-test",
                "delivered": false,
                "text": "keep",
            }))
            .unwrap();
        (raw.row("counter", json!(["agent", KEY])))
            .put(json!({"last": 17}))
            .unwrap();
        let metadata = (raw.row("callback_state", json!(["binding", KEY])))
            .field("in_process")
            .unwrap();
        assert_eq!(metadata, Value::Null);
        Self { fixture, handle }
    }

    fn binding(&self) -> Binding {
        self.fixture
            .store
            .agent_bindings()
            .unwrap()
            .remove(&self.handle)
            .unwrap()
    }
}

fn state(store: &Store) -> Vec<Value> {
    let rows = |table: &str| store.raw().table(table).get().unwrap();
    vec![json!({
        "sessions": rows("session"),
        "channels": rows("callback_state"),
        "events": rows("callback_event"),
        "leases": rows("callback_lease"),
        "counters": rows("counter"),
    })]
}

#[test]
fn same_key_classification_matches_both_serial_orders() {
    for full_first in [true, false] {
        let legacy = Legacy::new();
        let store = &legacy.fixture.store;
        if full_first {
            let full = store.bind_agent(KEY, &legacy.handle, true).unwrap();
            assert!(full, "serial full-first: full={full}");
            let before = state(store);
            let child = store.bind_agent(CHILD, &legacy.handle, false).unwrap();
            assert!(!child, "serial full-first: child={child}");
            assert_eq!(
                state(store),
                before,
                "serial full-first rejection writes nothing"
            );
        } else {
            let child = store.bind_agent(CHILD, &legacy.handle, false).unwrap();
            assert!(child, "serial child-first: child={child}");
            assert_eq!(legacy.binding().agent, CHILD);
            let full = store.bind_agent(KEY, &legacy.handle, true).unwrap();
            assert!(full, "serial child-first: full={full}");
        }
        let binding = legacy.binding();
        assert_eq!(binding.agent, KEY, "serial full_first={full_first}");
        assert!(
            !binding.in_process,
            "serial full_first={full_first}: {binding:?}"
        );
    }
}

#[test]
fn a_legacy_classification_cleans_up_once_and_its_matching_retry_writes_nothing() {
    let legacy = Legacy::new();
    let last_seen = legacy
        .fixture
        .store
        .session(&legacy.handle)
        .unwrap()
        .row
        .last_seen;
    legacy.fixture.advance(10.0);
    let classified = legacy
        .fixture
        .store
        .bind_agent(KEY, &legacy.handle, false)
        .unwrap();
    assert!(classified, "legacy classification: classified={classified}");
    let raw = legacy.fixture.store.raw();
    let channel = (raw.row("callback_state", legacy.handle.as_str()))
        .exists()
        .unwrap();
    let event = raw.row("callback_event", json!([legacy.handle, "chat-test"]));
    let cleanup = json!({
        "channel": channel,
        "delivered": event.field("delivered").unwrap(),
        "text": event.field("text").unwrap(),
    });
    assert_eq!(
        cleanup,
        json!({"channel":false, "delivered":true, "text":""})
    );
    assert_eq!(
        legacy
            .fixture
            .store
            .session(&legacy.handle)
            .unwrap()
            .row
            .last_seen,
        last_seen,
        "classification adds no heartbeat"
    );
    let before = state(&legacy.fixture.store);
    legacy.fixture.advance(10.0);
    let retried = legacy
        .fixture
        .store
        .bind_agent(KEY, &legacy.handle, false)
        .unwrap();
    assert!(retried, "legacy retry: retried={retried}");
    assert_eq!(
        state(&legacy.fixture.store),
        before,
        "matching retry changes no row or counter"
    );
}
