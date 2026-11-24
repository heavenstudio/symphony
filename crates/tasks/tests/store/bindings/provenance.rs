//! Proven full-session ownership survives stale bind decisions and in-process callers.

use super::*;
use crate::callbacks::Listener;
use serde_json::json;

mod freshness;

#[test]
fn legacy_bindings_without_provenance_can_be_classified_once_and_retried() {
    for full in [false, true] {
        let f = Fixture::new(crate::common::Seed::default());
        let h = f.session("worker");
        // A supported legacy row carries its agent id before binding metadata existed.
        (f.store.raw().row("session", h.as_str()))
            .set(json!({"agent": "legacy-key"}))
            .unwrap();
        let unknown = (f.store.raw())
            .row("callback_state", json!(["binding", "legacy-key"]))
            .field("in_process")
            .unwrap();
        assert_eq!(unknown, serde_json::Value::Null);
        let last_seen = f.store.session(&h).unwrap().row.last_seen;
        f.advance(10.0);
        assert!(f.store.bind_agent("legacy-key", &h, full).unwrap());
        let classified = binding_of(&f, &h);
        assert_eq!(classified.agent, "legacy-key");
        assert_eq!(classified.in_process, !full, "{classified:?}");
        assert_eq!(
            f.store.session(&h).unwrap().row.last_seen,
            last_seen,
            "classification adds no heartbeat, full={full}"
        );
        f.advance(10.0);
        assert!(f.store.bind_agent("legacy-key", &h, full).unwrap());
        assert_eq!(
            binding_of(&f, &h),
            classified,
            "full={full}: retry is stable"
        );
        assert_eq!(
            f.store.session(&h).unwrap().row.last_seen,
            last_seen,
            "idempotent retry adds no heartbeat, full={full}"
        );
    }
}

#[test]
fn a_proven_full_session_key_cannot_move_or_downgrade_to_another_handle() {
    let f = Fixture::new(crate::common::Seed::default());
    let (full, other) = (f.session("worker"), f.session("worker"));
    assert!(f.store.bind_agent("full-key", &full, true).unwrap());
    let before = binding_of(&f, &full);
    assert!(!before.in_process);
    assert!(!f.store.bind_agent("full-key", &other, false).unwrap());
    assert_eq!(binding_of(&f, &full), before);
    assert_eq!(f.store.session(&other).unwrap().row.agent, None);
    assert!(f.store.bind_agent("full-key", &full, true).unwrap());
    assert_eq!(
        binding_of(&f, &full),
        before,
        "a full-session retry is valid"
    );
}

#[test]
fn a_target_proven_full_after_the_snapshot_rejects_takeover_transactionally() {
    let f = Fixture::new(crate::common::Seed::default());
    let h = f.session("worker");
    let (protect, handle) = (f.other(), h.clone());
    let late = f.late(move || {
        assert!(protect.bind_agent("full-key", &handle, true).unwrap());
    });
    assert!(!late.bind_agent("sub-key", &h, false).unwrap());
    let binding = binding_of(&f, &h);
    assert_eq!(binding.agent, "full-key");
    assert!(!binding.in_process, "{binding:?}");
    assert_eq!(f.store.handle_for_agent("sub-key").unwrap(), None);
    let leaked = (f.store.raw())
        .row("callback_state", json!(["binding", "sub-key"]))
        .field("in_process")
        .unwrap();
    assert_eq!(
        leaked,
        serde_json::Value::Null,
        "rejected transaction left metadata"
    );
    let counter = (f.store.raw().row("counter", json!(["agent", "sub-key"])))
        .exists()
        .unwrap();
    assert!(
        !counter,
        "the rejected transaction rolled back its guard write"
    );
}

#[test]
fn an_incoming_key_proven_full_after_the_snapshot_keeps_its_original_handle() {
    let f = Fixture::new(crate::common::Seed::default());
    let (anchor, target) = (f.session("worker"), f.session("worker"));
    let (protect, original) = (f.other(), anchor.clone());
    let late = f.late(move || {
        assert!(protect.bind_agent("racing-key", &original, true).unwrap());
    });
    assert!(!late.bind_agent("racing-key", &target, false).unwrap());
    let binding = binding_of(&f, &anchor);
    assert_eq!(binding.agent, "racing-key");
    assert!(!binding.in_process, "{binding:?}");
    assert_eq!(f.store.session(&target).unwrap().row.agent, None);
    assert_eq!(
        f.store.handle_for_agent("racing-key").unwrap(),
        Some(anchor)
    );
}

#[test]
fn a_native_callback_without_binding_provenance_still_prevents_takeover() {
    let f = Fixture::new(crate::common::Seed::default());
    let h = f.session("worker");
    let listener = Listener::new(f.store.database().database());
    let address = format!("uds:{}", listener.path.display());
    f.store.callback(&h, Some(&address), false, false).unwrap();
    assert!(f.store.agent_bindings().unwrap().is_empty());
    let channel = || {
        f.store
            .raw()
            .row("callback_state", h.as_str())
            .get()
            .unwrap()
    };
    let before = channel();
    assert!(!f.store.bind_agent("sub-key", &h, false).unwrap());
    assert_eq!(
        channel(),
        before,
        "rejection preserves the native registration"
    );
    assert_eq!(f.store.session(&h).unwrap().row.agent, None);
    f.store.callback(&h, None, false, false).unwrap();
    assert!(f.store.bind_agent("sub-key", &h, false).unwrap());
    assert!(
        binding_of(&f, &h).in_process,
        "removing the sole fence permits binding"
    );
}

#[test]
fn a_native_callback_registered_after_the_snapshot_is_retained_on_retry() {
    let f = Fixture::new(crate::common::Seed::default());
    let h = f.session("worker");
    let listener = Listener::new(f.store.database().database());
    let address = format!("uds:{}", listener.path.display());
    struct Registration {
        store: symphony_board::Store,
        handle: String,
        address: String,
    }
    // The delayed call owns another Store connection and the registration inputs;
    // the test retains its original handle/address for the final assertions.
    let registration = Registration {
        store: f.other(),
        handle: h.clone(),
        address: address.clone(),
    };
    let late = f.late(move || {
        registration
            .store
            .callback(
                &registration.handle,
                Some(&registration.address),
                false,
                false,
            )
            .unwrap();
    });
    assert!(!late.bind_agent("sub-key", &h, false).unwrap());
    assert_eq!(f.store.session(&h).unwrap().row.agent, None);
    let channel = (f.store.raw().row("callback_state", h.as_str()))
        .field("address")
        .unwrap();
    assert_eq!(channel, json!(address));
    assert!(f.store.agent_bindings().unwrap().is_empty());
}
