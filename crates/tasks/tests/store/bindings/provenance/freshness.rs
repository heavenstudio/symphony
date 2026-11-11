//! Classification must recheck its target after a real binding replaces the decision snapshot.

use crate::common::{Fixture, HOST, NOW};
use crate::relay::{forward, message, statements};
use serde_json::{Value, json};
use std::io::{BufReader, Write};
use std::net::{Shutdown, TcpListener};
use symphony_board::{Binding, Store, rpc::Client};

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
        fixture
            .store
            .client()
            .query(
                "UPDATE type::record('session', $handle) SET agent = $key RETURN NONE;
             UPSERT type::record('callback_state', $handle) SET marker = 'keep' RETURN NONE;
             UPSERT type::record('callback_event', [$handle, 'chat-test']) SET handle = $handle,
                 key = 'chat-test', delivered = false, text = 'keep' RETURN NONE;
             UPSERT type::record('counter', ['agent', $key]) SET last = 17 RETURN NONE;",
                json!({"handle":handle, "key":KEY}),
            )
            .unwrap();
        let metadata = fixture
            .store
            .client()
            .query(
                "RETURN type::record('callback_state', ['binding', $key]).in_process;",
                json!({"key":KEY}),
            )
            .unwrap();
        assert_eq!(metadata, vec![Value::Null]);
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
    store
        .client()
        .query(
            "RETURN {
        sessions: (SELECT * FROM session ORDER BY id),
        channels: (SELECT * FROM callback_state ORDER BY id),
        events: (SELECT * FROM callback_event ORDER BY id),
        leases: (SELECT * FROM callback_lease ORDER BY id),
        counters: (SELECT * FROM counter ORDER BY id)
    };",
            json!({}),
        )
        .unwrap()
}

fn body(raw: &[u8]) -> Value {
    let offset = raw.windows(4).position(|part| part == b"\r\n\r\n").unwrap() + 4;
    symphony_board::cbor::decode(&raw[offset..]).unwrap()
}

enum Caller {
    FullSession,
    InProcess,
}

/// Intervene before the first classification write; its preceding real snapshot already ended.
/// After the stale transaction ends, inspect all state before forwarding any fresh retry.
fn delayed_classification(
    legacy: &Legacy,
    caller: Caller,
    intervene: impl FnOnce(&Store, &str) + Send + 'static,
) -> bool {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let upstream = legacy.fixture.store.client().addr().to_string();
    // The worker owns its separate real Store and target; the caller retains the fixture and
    // handle to inspect final state after closing the proxied client and joining the worker.
    let other = legacy.fixture.other();
    let handle = legacy.handle.clone();
    let worker = std::thread::spawn(move || {
        let (mut caller, _) = listener.accept().unwrap();
        let mut requests = BufReader::new(caller.try_clone().unwrap());
        let mut connection = None;
        let mut intervene = Some(intervene);
        let mut snapshots = 0;
        while let Some(request) = message(&mut requests) {
            let sql = statements(&request);
            if sql.contains("held_in_process:") {
                snapshots += 1;
            }
            let before =
                if sql.contains("UPSERT type::record('callback_state', ['binding', $agent])") {
                    intervene.take().map(|intervene| {
                        assert_eq!(
                            snapshots, 1,
                            "intervention follows the initial real snapshot"
                        );
                        intervene(&other, &handle);
                        state(&other)
                    })
                } else {
                    None
                };
            let answer = forward(&mut connection, &upstream, &request).unwrap();
            if let Some(before) = before {
                let reply = body(&answer);
                assert!(
                    reply.to_string().contains("stale"),
                    "stale attempt reply: {reply}"
                );
                assert_eq!(
                    state(&other),
                    before,
                    "the stale attempt rolls back channel/event/session/lease/guard-counter writes"
                );
            }
            caller.write_all(&answer).unwrap();
        }
        if let Some(mut server) = connection {
            let _ = server.get_mut().shutdown(Shutdown::Both);
        }
        let _ = caller.shutdown(Shutdown::Both);
        let intervened = intervene.is_none();
        assert!(
            intervened,
            "classification write was never reached: intervened={intervened}"
        );
        assert_eq!(
            snapshots, 2,
            "stale classification retries from a fresh decision"
        );
    });
    let proxied = Store::with(
        Client::new(&address, "sf", legacy.fixture.store.client().database()),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
    )
    .unwrap();
    let accepted = proxied.bind_agent(KEY, &legacy.handle, matches!(caller, Caller::FullSession));
    drop(proxied);
    worker.join().unwrap();
    accepted.unwrap()
}

#[test]
fn replaced_same_key_classification_retries_and_matches_both_real_serial_orders() {
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

    let legacy = Legacy::new();
    let accepted = delayed_classification(&legacy, Caller::FullSession, |other, handle| {
        let child = other.bind_agent(CHILD, handle, false).unwrap();
        assert!(child, "intervening child bind: child={child}");
        let binding = other.agent_bindings().unwrap().remove(handle).unwrap();
        assert_eq!(binding.agent, CHILD);
        assert!(
            binding.in_process,
            "the completed intervening bind owns H: {binding:?}"
        );
    });
    assert!(
        accepted,
        "the fresh full binding can replace the in-process key: accepted={accepted}"
    );
    let binding = legacy.binding();
    assert_eq!(
        binding.agent, KEY,
        "success owns the target after the fresh retry"
    );
    assert!(!binding.in_process, "{binding:?}");
    assert_eq!(
        legacy.fixture.store.handle_for_agent(KEY).unwrap(),
        Some(legacy.handle)
    );
    let counter = legacy
        .fixture
        .store
        .client()
        .query(
            "RETURN type::record('counter', ['agent', $key]).last;",
            json!({"key":KEY}),
        )
        .unwrap();
    assert_eq!(
        counter,
        vec![json!(18)],
        "only the successful retry advances the key guard"
    );
}

#[test]
fn fresh_same_key_full_proof_rejects_without_writes_and_legacy_cleanup_stays_idempotent() {
    let protected = Legacy::new();
    let accepted = delayed_classification(&protected, Caller::InProcess, |other, handle| {
        let full = other.bind_agent(KEY, handle, true).unwrap();
        assert!(full, "intervening full proof: full={full}");
    });
    assert!(
        !accepted,
        "fresh full-session proof rejects in-process classification: accepted={accepted}"
    );
    let binding = protected.binding();
    assert_eq!(binding.agent, KEY);
    assert!(!binding.in_process, "{binding:?}");

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
    let cleanup = legacy
        .fixture
        .store
        .client()
        .query(
            "RETURN {
        channel: record::exists(type::record('callback_state', $handle)),
        delivered: type::record('callback_event', [$handle, 'chat-test']).delivered,
        text: type::record('callback_event', [$handle, 'chat-test']).text
    };",
            json!({"handle":legacy.handle}),
        )
        .unwrap();
    assert_eq!(
        cleanup,
        vec![json!({"channel":false, "delivered":true, "text":""})]
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
