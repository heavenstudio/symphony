use super::*;
use serde_json::json;

fn task(status: &str) -> Task {
    serde_json::from_value(json!({"slug":"feature","title":"Feature","body":"",
        "status":status,"priority":1,"parent":null,"created_by":"owner","created":0.0,
        "updated":0.0,"links":[],"depends":[],"roles":[],"instruction":false}))
    .unwrap()
}

fn operation(kind: &str, state: &str, id: &str) -> WorkOperation {
    serde_json::from_value(
        json!({"machine_id":"alpha","operation_id":id,"parent_id":null,
        "handle":"worker","task":"feature","checkout":"opaque","recipe":"fixture",
        "kind":kind,"state":state,"resource":"native","reason":"","phase_since":null}),
    )
    .unwrap()
}

fn work(operations: &[WorkOperation]) -> MachineWork {
    serde_json::from_value(json!({"freshness":"fresh","machines":[{
        "machine_id":"alpha","label":"Alpha","producer_id":"fixture","sequence":1,
        "age_ms":0,"freshness":"fresh","complete":true,"reason":"","counts":null,
        "operations":operations}],"unassigned":[]}))
    .unwrap()
}

#[test]
fn only_explicit_current_build_test_evidence_projects_active_lifecycle_work() {
    assert_eq!(TaskStage::Build.as_str(), "build");
    assert_eq!(TaskStage::Test.as_str(), "test");
    let active = task("in_progress");
    let build = operation("build", "queued", "build");
    let current = work(std::slice::from_ref(&build));
    assert_eq!(current.task_stage(&active, true), TaskStage::Build);
    assert_eq!(current.task_stage(&active, false).as_str(), "in_progress");
    assert_eq!(active.status, "in_progress");
    assert!(!active.is_finished());
    for state in [
        "ready",
        "review",
        "done",
        "blocked",
        "cancelled",
        "future_state",
    ] {
        assert_eq!(current.task_stage(&task(state), true).as_str(), state);
    }
    for freshness in [
        WorkFreshness::Unknown,
        WorkFreshness::Stale,
        WorkFreshness::Disconnected,
        WorkFreshness::Conflict,
    ] {
        let mut uncertain = current.clone();
        uncertain.machines[0].freshness = freshness;
        assert_eq!(uncertain.task_stage(&active, true).as_str(), "in_progress");
    }
    let mut partial = current.clone();
    partial.machines[0].complete = false;
    assert_eq!(partial.task_stage(&active, true), TaskStage::Build);
    partial.machines[0].freshness = WorkFreshness::Incomplete;
    assert_eq!(partial.task_stage(&active, true), TaskStage::Build);
    assert_eq!(
        partial.machines[0].counts, None,
        "positive task evidence does not invent machine capacity"
    );
    assert_eq!(
        MachineWork::default().task_stage(&active, true).as_str(),
        "in_progress"
    );
    for invalid in [
        WorkOperation {
            task: None,
            ..build.clone()
        },
        WorkOperation {
            task: Some("other".into()),
            ..build.clone()
        },
        WorkOperation {
            machine_id: Some("foreign".into()),
            ..build.clone()
        },
        WorkOperation {
            operation_id: None,
            ..build.clone()
        },
        WorkOperation {
            operation_id: Some(String::new()),
            ..build.clone()
        },
        operation("command", "running", "command"),
        operation("setup", "running", "setup"),
        operation("reservation", "queued", "hold"),
    ] {
        assert_eq!(
            work(&[invalid]).task_stage(&active, true).as_str(),
            "in_progress"
        );
    }
    let unassigned = MachineWork {
        unassigned: vec![build],
        ..MachineWork::default()
    };
    assert_eq!(unassigned.task_stage(&active, true).as_str(), "in_progress");
}

#[test]
fn overlap_has_one_order_independent_stage_and_conflicting_nonces_do_not_claim_activity() {
    let active = task("in_progress");
    for (build, test, expected) in [
        ("queued", "queued", TaskStage::Test),
        ("running", "queued", TaskStage::Build),
        ("queued", "running", TaskStage::Test),
        ("running", "running", TaskStage::Test),
    ] {
        let mut current = work(&[operation("build", build, "b"), operation("test", test, "t")]);
        assert_eq!(current.task_stage(&active, true), expected);
        current.machines[0].operations.reverse();
        assert_eq!(current.task_stage(&active, true), expected);
    }
    let build = operation("build", "running", "same");
    assert_eq!(
        work(&[build.clone(), build.clone()]).task_stage(&active, true),
        TaskStage::Build
    );
    let conflict = WorkOperation {
        state: WorkState::Queued,
        ..build.clone()
    };
    assert_eq!(
        work(&[build, conflict]).task_stage(&active, true).as_str(),
        "in_progress"
    );
}

/// The run records' inventory is partial: a running test places an in-progress task in Test, over
/// its running build, and leaves a task in review where it is, its run a chip on the card.
#[test]
fn a_partial_inventorys_running_test_places_work_in_progress_and_never_review() {
    let mut current = work(&[
        operation("build", "running", "b"),
        operation("test", "running", "t"),
    ]);
    current.machines[0].freshness = WorkFreshness::Incomplete;
    current.machines[0].complete = false;
    assert_eq!(
        current.task_stage(&task("in_progress"), true),
        TaskStage::Test
    );
    assert_eq!(current.task_stage(&task("review"), true).as_str(), "review");
    current.machines[0].operations.remove(1);
    assert_eq!(
        current.task_stage(&task("in_progress"), true),
        TaskStage::Build
    );
    current.machines[0].operations.clear();
    assert_eq!(
        current.task_stage(&task("in_progress"), true).as_str(),
        "in_progress"
    );
}
