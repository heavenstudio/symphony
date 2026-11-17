//! Additive records, independent of the current Task/TaskFields and attempt lifecycle.
use serde_json::{Value, json};
use std::collections::BTreeMap;
use symphony_api::workflow::*;

fn snapshot() -> PositionSnapshot {
    PositionSnapshot {
        workflow: Revision {
            format: FORMAT,
            digest: "c".repeat(64),
            source: Source {
                commit: "a".repeat(40),
                project_blob: "b".repeat(40),
            },
        },
        activation_generation: 2,
        task_revision: 7,
        at: Pair {
            phase: "verify".into(),
            state: "awaiting_review".into(),
        },
    }
}

#[test]
fn request_round_trips_and_cannot_carry_a_trusted_principal() {
    let request = OperationRequest {
        project: "demo".into(),
        task: "area/task".into(),
        edge: "claim_review".into(),
        expected: snapshot(),
        input_revisions: [("candidate".into(), "sha256:abc".into())].into(),
        idempotency_key: "opaque".into(),
        inputs: [
            ("reason".into(), InputValue::Text("ready".into())),
            (
                "candidate".into(),
                InputValue::Revision("sha256:abc".into()),
            ),
            ("resume".into(), InputValue::Pair(snapshot().at)),
            ("hold".into(), InputValue::Flag(true)),
        ]
        .into(),
    };
    let mut wire = serde_json::to_value(&request).unwrap();
    assert_eq!(
        serde_json::from_value::<OperationRequest>(wire.clone()).unwrap(),
        request
    );
    assert_eq!(
        wire["expected"]["at"],
        json!({"phase":"verify", "state":"awaiting_review"})
    );
    assert!(wire.get("actor").is_none());
    wire["actor"] = json!({"capability":"owner"});
    assert!(serde_json::from_value::<OperationRequest>(wire).is_err());
}

#[test]
fn result_reports_effects_and_unavailable_actions_without_authorizing_them() {
    let result = OperationResult {
        transition: "event-1".into(),
        effects: vec![
            Effect::ReleaseRole {
                role: Role::Executor,
            },
            Effect::Slot {
                policy: Slot::Release,
            },
        ],
        next: NextActions {
            snapshot: snapshot(),
            actions: vec![NextAction {
                edge: "accept".into(),
                operation: Operation::PrepareExternal,
                destination: Pair {
                    phase: "deploy".into(),
                    state: "available".into(),
                },
                label: "Accept package".into(),
                responsible: vec![Authority::ProjectCapability {
                    capability: "accept_package".into(),
                }],
                required_inputs: vec!["package_revision".into()],
                unavailable: vec![RefusalCode::HumanWait, RefusalCode::GuardUnavailable],
            }],
        },
    };
    let value = serde_json::to_value(&result).unwrap();
    assert_eq!(
        value["next"]["actions"][0]["unavailable"],
        json!(["human_wait", "guard_unavailable"])
    );
    assert_eq!(
        serde_json::from_value::<OperationResult>(value).unwrap(),
        result
    );
}

#[test]
fn tagged_effect_parameters_and_unknown_variants_are_strict() {
    for invalid in [
        json!({"kind":"acquire_role"}),
        json!({"kind":"prepare"}),
        json!({"kind":"review", "approved":true, "override_independence":true}),
        json!({"kind":"shell", "command":"true"}),
    ] {
        assert!(serde_json::from_value::<Effect>(invalid).is_err());
    }
    assert!(
        serde_json::from_value::<Authority>(json!({"kind":"task_role", "role":"owner"})).is_err()
    );
    assert!(serde_json::from_value::<Operation>(json!("execute_provider")).is_err());
    assert!(
        serde_json::from_value::<PositionSnapshot>(json!({"phase":"Build", "state":"running"}))
            .is_err()
    );
}

#[test]
fn request_requires_every_fencing_coordinate() {
    let request = OperationRequest {
        project: "demo".into(),
        task: "area/task".into(),
        edge: "accept".into(),
        expected: snapshot(),
        input_revisions: BTreeMap::new(),
        idempotency_key: "key".into(),
        inputs: BTreeMap::new(),
    };
    for key in ["workflow", "activation_generation", "task_revision", "at"] {
        let mut value = serde_json::to_value(&request).unwrap();
        value["expected"].as_object_mut().unwrap().remove(key);
        assert!(
            serde_json::from_value::<OperationRequest>(value).is_err(),
            "{key}"
        );
    }
    let value = serde_json::to_value(&request).unwrap();
    let old_reader: Value = serde_json::from_str(&value.to_string()).unwrap();
    assert_eq!(old_reader["expected"]["task_revision"], 7);
}
