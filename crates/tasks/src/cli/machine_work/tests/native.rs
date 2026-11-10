//! Explicit native completion shares sample fences and cannot resurrect a finished nonce.

use super::*;
use crate::cli::machine_work::native::{Inventory, MAX_BYTES};
use serde_json::{Value, json};

const ONE: &str = "11111111111111111111111111111111";
const TWO: &str = "22222222222222222222222222222222";

fn row(id: &str, state: &str) -> Value {
    json!({"evidence": if state == "finished" { "finished" } else { "active" },
        "operation_id":id,"parent_id":null,"timing_run_id":"timing-only","handle":"worker",
        "checkout":"/same/path","recipe":"cargo build","kind":"build","state":state,
        "resource":"cargo-room","reason":"fixture","phase_since_epoch_ms":999999999999_u64,
        "owner_pid":123,"child_pid":null,"exit_code":if state == "finished" { Some(7) } else { None },
    })
}

fn bytes(rows: Vec<Value>) -> Vec<u8> {
    serde_json::to_vec(
        &json!({"version":1,"complete":false,"uncertain":false,"truncated":false,"records":rows}),
    )
    .unwrap()
}

fn inventory(machine: &str, rows: Vec<Value>) -> Inventory {
    Inventory::decode(&bytes(rows), Some(machine)).unwrap()
}

#[test]
fn unreadable_native_records_keep_unknown_work_without_suppressing_independent_verified_rows() {
    let mut receiver = Projection::new(2, 4, 100);
    receiver
        .observe_native(
            sample("a", 1, vec![]),
            inventory("a", vec![row(ONE, "running")]),
            1,
        )
        .unwrap();
    for (sequence, state, remaining) in [(2, "queued", 2), (3, "finished", 1)] {
        let input = json!({"version":1,"complete":false,"uncertain":true,"truncated":false,"records":[row(TWO,state)]});
        let decoded = Inventory::decode(&serde_json::to_vec(&input).unwrap(), Some("a")).unwrap();
        receiver
            .observe_native(sample("a", sequence, vec![]), decoded, sequence)
            .unwrap();
        let view = receiver.snapshot(sequence).machines.remove(0);
        assert_eq!(view.freshness, Freshness::Unknown);
        assert_eq!(view.operations.len(), remaining);
        let retained = view
            .operations
            .iter()
            .find(|operation| operation.operation_id.as_deref() == Some(ONE))
            .expect("original unreadable operation retained");
        assert_eq!(retained.state, State::Running);
        let independent = view
            .operations
            .iter()
            .find(|operation| operation.operation_id.as_deref() == Some(TWO));
        if state == "queued" {
            assert_eq!(
                independent.expect("verified queued operation").state,
                State::Queued
            );
        } else {
            assert_eq!(
                independent, None,
                "only the verified finished nonce retires"
            );
        }
        assert_eq!(view.counts, None);
        assert!(!view.complete);
    }
    assert_eq!(
        receiver.observe_native(sample("b", 1, vec![]), inventory("a", vec![]), 4),
        Err(Rejected::Identity)
    );
    assert_eq!(receiver.snapshot(4).machines.len(), 1);
    receiver.disconnect("a");
    receiver.scanned_native("a", &inventory("b", vec![]));
    assert_eq!(
        receiver.snapshot(4).machines[0].freshness,
        Freshness::Disconnected
    );
    receiver.scanned_native("b", &inventory("b", vec![]));
    assert_eq!(receiver.snapshot(4).machines.len(), 1);
    receiver.scanned_native("a", &inventory("a", vec![]));
    let retained = receiver.snapshot(4).machines.remove(0);
    assert_eq!(retained.freshness, Freshness::Incomplete);
    assert_eq!((retained.sequence, retained.age_ms), (3, Some(1)));
    assert_eq!(retained.operations[0].operation_id.as_deref(), Some(ONE));
    assert_eq!(retained.counts, None);
}

#[test]
fn native_completion_is_exact_atomic_partial_idempotent_and_machine_local() {
    let mut receiver = Projection::new(2, 4, 10);
    receiver
        .observe_native(
            sample("a", 1, vec![]),
            inventory("a", vec![row(ONE, "queued"), row(TWO, "running")]),
            10,
        )
        .unwrap();
    receiver
        .observe_native(
            sample("b", 1, vec![]),
            inventory("b", vec![row(ONE, "running")]),
            10,
        )
        .unwrap();
    let other = receiver.snapshot(10).machines[1].clone();
    for sequence in [2, 3] {
        receiver
            .observe_native(
                sample("a", sequence, vec![]),
                inventory("a", vec![row(ONE, "finished"), row(TWO, "running")]),
                11,
            )
            .unwrap();
        let view = receiver.snapshot(11);
        assert_eq!(view.machines[0].operations.len(), 1, "{view:?}");
        assert_eq!(
            view.machines[0].operations[0].operation_id.as_deref(),
            Some(TWO)
        );
        assert!(!view.machines[0].complete);
        assert_eq!(view.machines[0].freshness, Freshness::Incomplete);
        assert_eq!(view.machines[0].counts, None);
        assert_eq!(view.machines[1].operations, other.operations);
        assert_eq!(view.machines[1].sequence, other.sequence);
    }
    receiver
        .observe_native(
            sample("a", 4, vec![]),
            inventory("a", vec![row(TWO, "finished")]),
            12,
        )
        .unwrap();
    let finished = receiver.snapshot(12).machines.remove(0);
    assert!(finished.operations.is_empty());
    assert_eq!(finished.counts, None);
    assert!(!finished.complete);
    assert_eq!(
        receiver.observe_native(sample("a", 4, vec![]), inventory("a", vec![]), 20),
        Err(Rejected::OutOfOrder)
    );
    assert_eq!(receiver.snapshot(20).machines[0].age_ms, Some(8));
}

#[test]
fn terminal_evidence_cannot_bypass_identity_order_clock_or_ownership_fences() {
    let cases: [fn(&mut Sample); 5] = [
        |sample| sample.machine_id = "foreign".into(),
        |sample| sample.producer_id = "different".into(),
        |sample| sample.generation = 0,
        |sample| sample.sequence = 1,
        |sample| sample.label = "x".repeat(257),
    ];
    for change in cases {
        let mut receiver = Projection::new(2, 4, 10);
        receiver
            .observe_native(
                sample("a", 1, vec![]),
                inventory("a", vec![row(ONE, "running")]),
                10,
            )
            .unwrap();
        let mut next = sample("a", 2, vec![]);
        change(&mut next);
        assert!(
            receiver
                .observe_native(next, inventory("a", vec![row(ONE, "finished")]), 11)
                .is_err()
        );
        let retained = receiver.snapshot(11).machines.remove(0);
        assert_eq!(retained.operations.len(), 1);
        assert_eq!((retained.sequence, retained.age_ms), (1, Some(1)));
    }
    let mut receiver = Projection::new(1, 4, 10);
    receiver
        .observe_native(
            sample("a", 1, vec![]),
            inventory("a", vec![row(ONE, "running")]),
            10,
        )
        .unwrap();
    assert_eq!(
        receiver.observe_native(
            sample("a", 2, vec![]),
            inventory("a", vec![row(ONE, "finished")]),
            9
        ),
        Err(Rejected::ReceiverClock)
    );
    let mut changed = row(ONE, "finished");
    changed["handle"] = json!("imposter");
    assert_eq!(
        receiver.observe_native(sample("a", 2, vec![]), inventory("a", vec![changed]), 11),
        Err(Rejected::Ownership)
    );
    receiver
        .observe_native(
            sample("a", 2, vec![]),
            inventory("a", vec![row(TWO, "finished")]),
            11,
        )
        .unwrap();
    assert_eq!(
        receiver.snapshot(11).machines[0].operations[0]
            .operation_id
            .as_deref(),
        Some(ONE)
    );
}

#[test]
fn terminal_history_survives_restart_and_blocks_resurrection_and_owner_rebinding() {
    let mut receiver = Projection::new(1, 2, 10);
    receiver
        .observe_native(
            sample("a", 1, vec![]),
            inventory("a", vec![row(ONE, "running")]),
            1,
        )
        .unwrap();
    receiver
        .observe_native(
            sample("a", 2, vec![]),
            inventory("a", vec![row(ONE, "finished")]),
            2,
        )
        .unwrap();
    let mut next = sample("a", 1, vec![]);
    next.generation = 2;
    next.producer_id = "replacement".into();
    for changed_owner in [false, true] {
        let mut replay = row(ONE, "running");
        if changed_owner {
            replay["checkout"] = json!("/other");
        }
        assert_eq!(
            receiver.observe_native(next.clone(), inventory("a", vec![replay]), 3),
            Err(if changed_owner {
                Rejected::Ownership
            } else {
                Rejected::ConflictingOperation
            })
        );
        assert!(receiver.snapshot(3).machines[0].operations.is_empty());
        assert_eq!(receiver.snapshot(3).machines[0].sequence, 2);
    }
    receiver
        .observe_native(
            next.clone(),
            inventory("a", vec![row(ONE, "finished"), row(TWO, "finished")]),
            3,
        )
        .unwrap();
    next.sequence = 2;
    assert_eq!(
        receiver.observe_native(
            next.clone(),
            inventory(
                "a",
                vec![row("33333333333333333333333333333333", "finished")]
            ),
            4
        ),
        Err(Rejected::Limit)
    );
    next.sequence = 3;
    assert_eq!(
        receiver.observe_native(next, inventory("a", vec![]), 5),
        Err(Rejected::Limit)
    );
    assert_eq!(
        receiver.snapshot(5).machines[0].freshness,
        Freshness::Conflict
    );
}

#[test]
fn decoder_requires_exact_bounded_explicit_fields_and_separates_timing_setup_and_absence() {
    let original = row(ONE, "setup");
    let fields: Vec<_> = original.as_object().unwrap().keys().cloned().collect();
    for field in fields {
        let mut missing = original.clone();
        missing.as_object_mut().unwrap().remove(&field);
        assert!(
            Inventory::decode(&bytes(vec![missing]), Some("a")).is_err(),
            "accepted absent {field}"
        );
    }
    for (field, invalid) in [
        ("exit_code", json!(7)),
        ("handle", json!("")),
        ("parent_id", json!(ONE)),
        ("operation_id", json!("bad")),
        ("owner_pid", json!(0)),
        ("recipe", json!("x".repeat(4097))),
        ("state", json!("unknown")),
        ("kind", json!("reservation")),
    ] {
        let mut broken = original.clone();
        broken[field] = invalid;
        assert!(
            Inventory::decode(&bytes(vec![row(TWO, "finished"), broken]), Some("a")).is_err(),
            "accepted {field}"
        );
    }
    for (field, invalid) in [
        ("version", json!(2)),
        ("complete", json!(true)),
        ("truncated", json!(true)),
    ] {
        let mut value: Value = serde_json::from_slice(&bytes(vec![])).unwrap();
        value[field] = invalid;
        assert!(Inventory::decode(&serde_json::to_vec(&value).unwrap(), Some("a")).is_err());
    }
    assert!(Inventory::decode(b"{", Some("a")).is_err());
    assert!(
        Inventory::decode(
            &bytes(vec![row(ONE, "running"), row(ONE, "finished")]),
            Some("a")
        )
        .is_err()
    );
    let repeated = String::from_utf8(bytes(vec![row(ONE, "running")]))
        .unwrap()
        .replace(
            "\"parent_id\":null",
            "\"parent_id\":null,\"parent_id\":null",
        );
    assert!(Inventory::decode(repeated.as_bytes(), Some("a")).is_err());
    let mut missing_exit = row(ONE, "finished");
    missing_exit["exit_code"] = Value::Null;
    assert!(Inventory::decode(&bytes(vec![missing_exit]), Some("a")).is_err());
    assert!(Inventory::decode(&vec![b' '; MAX_BYTES + 1], Some("a")).is_err());
    let unassigned = Inventory::decode(&bytes(vec![original]), None)
        .unwrap()
        .unassigned();
    assert_eq!(unassigned.len(), 1);
    assert_eq!(unassigned[0].machine_id, None);
    assert_eq!(
        unassigned[0].parent_id, None,
        "timing label became ancestry"
    );
    assert_eq!(
        (unassigned[0].kind.clone(), unassigned[0].state.clone()),
        (Kind::Setup, State::Running)
    );
    assert_eq!(unassigned[0].phase_since, Some(999999999.999));
}
