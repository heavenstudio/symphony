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
/// its running build, and leaves a task in review where it is, its run a chip on the card. Main
/// placed them so already; this holds it for the run records' inventory.
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

fn named(slug: &str, status: &str) -> Task {
    Task {
        slug: slug.into(),
        ..task(status)
    }
}

/// A run recorded for a subtask is its parents' work too: the board draws the parent's card, so
/// the parent moves, as the subtask does where it is drawn; the recorder's binding stays the
/// subtask, and a task beside it or above its parents does not move.
#[test]
fn a_subtasks_run_places_every_task_it_is_under_and_no_other() {
    let parents = |slug: &str| match slug {
        "feature/slice" => Some("feature"),
        "feature/slice/work" => Some("feature/slice"),
        _ => None,
    };
    let mut current = work(&[WorkOperation {
        task: Some("feature/slice/work".into()),
        ..operation("test", "running", "t")
    }]);
    assert_eq!(
        current
            .task_stage(&named("feature", "in_progress"), true)
            .as_str(),
        "in_progress",
        "an operation read without the board's tree names only its own task"
    );
    current.place_under(parents);
    let placed = &current.machines[0].operations[0];
    assert_eq!(placed.task.as_deref(), Some("feature/slice/work"));
    assert_eq!(placed.under, ["feature/slice", "feature"]);
    for slug in ["feature", "feature/slice", "feature/slice/work"] {
        assert_eq!(
            current.task_stage(&named(slug, "in_progress"), true),
            TaskStage::Test,
            "{slug}"
        );
    }
    for slug in ["feature/other", "feature/slice/work/deeper", "other"] {
        assert_eq!(
            current
                .task_stage(&named(slug, "in_progress"), true)
                .as_str(),
            "in_progress",
            "{slug}"
        );
    }
    // A parent in review keeps its card there, the run a chip on it.
    assert_eq!(
        current
            .task_stage(&named("feature", "review"), true)
            .as_str(),
        "review"
    );
}

/// What an operation read under no task says.
const UNDER_NOTHING: [&str; 0] = [];

#[test]
fn placing_under_reads_the_tree_again_and_stops_where_a_chain_would_repeat() {
    let mut current = work(&[
        WorkOperation {
            task: Some("a".into()),
            ..operation("build", "running", "looped")
        },
        WorkOperation {
            task: None,
            ..operation("build", "running", "unattributed")
        },
    ]);
    current.unassigned = vec![WorkOperation {
        task: Some("top".into()),
        ..operation("test", "queued", "loose")
    }];
    current.place_under(|slug| match slug {
        "a" | "c" => Some("b"),
        "b" => Some("c"),
        _ => None,
    });
    assert_eq!(current.machines[0].operations[0].under, ["b", "c"]);
    assert_eq!(current.machines[0].operations[1].under, UNDER_NOTHING);
    assert_eq!(current.unassigned[0].under, UNDER_NOTHING);
    // A task moved under another reads its new parents; a task back at the top reads none.
    current.place_under(|slug| (slug == "a").then_some("other"));
    assert_eq!(current.machines[0].operations[0].under, ["other"]);
    current.place_under(|_| None);
    assert_eq!(current.machines[0].operations[0].under, UNDER_NOTHING);
    // A task that is its own parent is under nothing.
    current.place_under(|slug| (slug == "a").then_some("a"));
    assert_eq!(current.machines[0].operations[0].under, UNDER_NOTHING);
}

#[test]
fn the_wire_leaves_out_an_empty_under_and_an_older_record_reads_as_under_nothing() {
    let older = operation("build", "queued", "b");
    assert_eq!(older.under, UNDER_NOTHING);
    let wire = serde_json::to_value(&older).unwrap();
    assert!(wire.get("under").is_none(), "{wire}");
    let placed = WorkOperation {
        under: vec!["parent".into()],
        ..older
    };
    let wire = serde_json::to_value(&placed).unwrap();
    assert_eq!(wire["under"], json!(["parent"]));
    assert_eq!(
        serde_json::from_value::<WorkOperation>(wire).unwrap(),
        placed
    );
    assert!(placed.is_for("feature") && placed.is_for("parent") && !placed.is_for("other"));
}

/// A machine's heartbeat moves its sequence, its age and what it does beside builds and tests every
/// second, and places no task anywhere else: works alike in every way a task's stage reads are
/// alike, however the rest of them moved.
#[test]
fn a_heartbeat_places_alike_and_so_does_what_places_no_task() {
    let current = work(&[
        operation("build", "running", "b"),
        operation("test", "queued", "t"),
    ]);
    let mut beat = current.clone();
    beat.freshness = WorkFreshness::Incomplete;
    let machine = &mut beat.machines[0];
    machine.sequence += 1;
    machine.age_ms = Some(900);
    machine.label = "Alpha, again".into();
    machine.complete = false;
    machine.freshness = WorkFreshness::Incomplete;
    machine.counts = Some(crate::WorkCounts::default());
    for operation in &mut machine.operations {
        operation.phase_since = Some(12.0);
        operation.reason = "still at it".into();
        operation.handle = Some("other-worker".into());
        operation.resource = "another resource".into();
    }
    // What can place no task: a command, a reservation, another machine's report on this one, an
    // operation with no id, a machine whose evidence is stale, and work assigned to no machine.
    machine
        .operations
        .push(operation("command", "running", "c"));
    machine
        .operations
        .push(operation("reservation", "queued", "r"));
    machine.operations.push(WorkOperation {
        machine_id: Some("foreign".into()),
        ..operation("build", "running", "f")
    });
    machine.operations.push(WorkOperation {
        operation_id: None,
        ..operation("test", "running", "")
    });
    let mut stale = beat.machines[0].clone();
    stale.machine_id = "beta".into();
    stale.freshness = WorkFreshness::Stale;
    for operation in &mut stale.operations {
        operation.machine_id = Some("beta".into());
    }
    beat.machines.push(stale);
    beat.unassigned = vec![operation("build", "running", "loose")];
    assert!(current.places_alike(&beat) && beat.places_alike(&current));
    assert!(MachineWork::default().places_alike(&work(&[operation("setup", "running", "s")])));
}

/// Anything a task's stage reads that differs is another placement: the board reads its counts and
/// columns again.
#[test]
fn a_work_that_could_move_a_task_does_not_place_alike() {
    fn first(work: &mut MachineWork) -> &mut WorkOperation {
        &mut work.machines[0].operations[0]
    }
    let current = work(&[operation("build", "queued", "b")]);
    let differs = |change: &dyn Fn(&mut MachineWork)| {
        let mut next = current.clone();
        change(&mut next);
        assert!(
            !current.places_alike(&next) && !next.places_alike(&current),
            "{next:?}"
        );
    };
    differs(&|work| first(work).state = WorkState::Running);
    differs(&|work| first(work).kind = WorkKind::Test);
    differs(&|work| first(work).task = Some("other".into()));
    differs(&|work| first(work).task = None);
    differs(&|work| first(work).under = vec!["parent".into()]);
    differs(&|work| work.machines[0].operations.clear());
    differs(&|work| {
        work.machines[0]
            .operations
            .push(operation("test", "running", "t"));
    });
    // A second report of the same operation that disagrees with it unplaces it.
    differs(&|work| {
        let disagreeing = WorkOperation {
            state: WorkState::Running,
            ..work.machines[0].operations[0].clone()
        };
        work.machines[0].operations.push(disagreeing);
    });
    for freshness in [
        WorkFreshness::Unknown,
        WorkFreshness::Stale,
        WorkFreshness::Disconnected,
        WorkFreshness::Conflict,
    ] {
        differs(&|work| work.machines[0].freshness = freshness.clone());
    }
    differs(&|work| work.machines.clear());
    assert!(!MachineWork::default().places_alike(&current));
}
