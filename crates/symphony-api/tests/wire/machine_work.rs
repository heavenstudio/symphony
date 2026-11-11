//! Machine evidence remains compatible with the existing wire contract.
use serde_json::json;
use symphony_api::{Agents, MachineWork, TaskListing, TaskPage, WorkFreshness, WorkKind};

#[test]
fn legacy_snapshots_default_machine_evidence_to_unknown() {
    let agents: Agents = serde_json::from_value(json!({
        "generated": 1.5, "probes": {"herdr": "ok", "slots": "ok"}, "agents": []
    }))
    .unwrap();
    let listing: TaskListing = serde_json::from_value(json!({"tasks": [], "next": null})).unwrap();
    let page: TaskPage = serde_json::from_value(json!({
        "task": {"slug": "tools/check", "title": "Check", "body": "", "status": "review",
            "priority": 2, "parent": null, "created_by": "owner", "created": 0.0, "updated": 0.0,
            "links": [], "depends": [], "roles": [], "instruction": false},
        "blocks": [], "children": [], "children_count": 0
    }))
    .unwrap();
    assert_eq!(agents.machine_work, MachineWork::default());
    assert!(agents.machine_work.guidance.is_empty());
    assert_eq!(listing.machine_work.freshness, WorkFreshness::Unknown);
    assert_eq!(
        listing.machine_work.machines,
        Vec::<symphony_api::MachineObservation>::new()
    );
    assert_eq!(page.machine_work.freshness, WorkFreshness::Unknown);
    assert_eq!(page.task.status, "review");
}

#[test]
fn reservation_and_stale_operations_keep_their_wire_meaning() {
    let work: MachineWork = serde_json::from_value(json!({
        "freshness": "unknown", "unassigned": [], "guidance":{"machine-b":{"build":2,"test":1}}, "machines": [{
            "machine_id": "machine-b", "label": "builder", "producer_id": "publisher",
            "sequence": 7, "age_ms": 5000, "freshness": "stale", "complete": true,
            "reason": "observation expired", "counts": null, "operations": [{
                "machine_id": "machine-b", "operation_id": "room-1", "parent_id": "verify-1",
                "handle": "worker", "task": "tools/check", "checkout": "/same/path",
                "recipe": "cargo test", "kind": "reservation", "state": "running",
                "resource": "cargo_room", "reason": "disk reservation", "phase_since": 100.0
            }]
        }]
    }))
    .unwrap();
    assert_eq!(work.machines[0].freshness, WorkFreshness::Stale);
    assert_eq!(work.guidance["machine-b"].build, Some(2));
    assert_eq!(work.machines[0].counts, None);
    assert_eq!(work.machines[0].operations[0].kind, WorkKind::Reservation);
    let sent = serde_json::to_value(&work).unwrap();
    assert_eq!(
        sent["machines"][0]["operations"][0]["parent_id"],
        "verify-1"
    );
    assert_eq!(sent["machines"][0]["counts"], serde_json::Value::Null);
    assert_eq!(serde_json::from_value::<MachineWork>(sent).unwrap(), work);
}
