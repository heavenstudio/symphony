//! The operational reader without a board: claim edges, staffing, lost holders, slots, finished
//! work and prerequisites that ended without success, on a workflow with a review handoff.

use super::*;
use crate::workflow::activate::tests::custom;
use crate::{Health, Role as Held};
use serde_json::json;
use symphony_api::workflow::Role;

/// Work is built (claimed by an executor, blocked under a hold that keeps its slot), then verified
/// (claimed by a fresh reviewer, approved, accepted by the owner); changes send it back to build,
/// and work not started is cancelled by the owner or the board's recovery service.
pub(crate) const REVIEWED: &str = r#"
[workflow]
format = 1
initial = { phase = "build", state = "available" }
task_kinds = ["task"]
phases = [
  { id = "build", label = "Build", order = 0, phone_order = 0, column = true },
  { id = "verify", label = "Verify", order = 1, phone_order = 1, column = true },
]
states = [
  { id = "available", label = "Available" }, { id = "working", label = "Working" },
  { id = "blocked", label = "Blocked" }, { id = "awaiting", label = "Awaiting review" },
  { id = "reviewing", label = "Reviewing" }, { id = "approved", label = "Approved" },
  { id = "accepted", label = "Accepted" }, { id = "cancelled", label = "Cancelled" },
]
capabilities = ["owner", "recovery"]
adapters = []
guards = []
positions = [
  { at = { phase = "build", state = "available" }, slot = "release", dependencies = "done", unsuccessful_dependency = "cancel", accepts_work = true, terminal = false, legacy = "ready", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "build", state = "working" }, staffing = "executor", slot = "acquire", dependencies = "done", unsuccessful_dependency = "block", accepts_work = true, terminal = false, legacy = "in_progress", satisfies_dependencies = false, completion = "none", recovery = { release = { target = { phase = "build", state = "available" }, manual = false, slot = "release", release_role = true, release_reservation = false }, lost_holder = { target = { phase = "build", state = "available" }, manual = false, slot = "release", release_role = true, release_reservation = false }, attempt_failure = { manual = true, slot = "retain", release_role = false, release_reservation = false }, cancellation_acknowledged = { manual = true, slot = "retain", release_role = false, release_reservation = false } } },
  { at = { phase = "build", state = "blocked" }, staffing = "executor", slot = "retain", dependencies = "done", unsuccessful_dependency = "manual", accepts_work = false, terminal = false, legacy = "blocked", satisfies_dependencies = false, completion = "none", recovery = { release = { target = { phase = "build", state = "available" }, manual = false, slot = "release", release_role = true, release_reservation = false }, lost_holder = { manual = true, slot = "retain", release_role = false, release_reservation = false }, attempt_failure = { manual = true, slot = "retain", release_role = false, release_reservation = false }, cancellation_acknowledged = { manual = true, slot = "retain", release_role = false, release_reservation = false } } },
  { at = { phase = "verify", state = "awaiting" }, slot = "release", dependencies = "done", unsuccessful_dependency = "block", accepts_work = false, terminal = false, legacy = "review", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "verify", state = "reviewing" }, staffing = "reviewer", slot = "acquire", dependencies = "done", unsuccessful_dependency = "block", accepts_work = false, terminal = false, legacy = "review", satisfies_dependencies = false, completion = "none", recovery = { release = { target = { phase = "verify", state = "awaiting" }, manual = false, slot = "release", release_role = true, release_reservation = false }, lost_holder = { target = { phase = "verify", state = "awaiting" }, manual = false, slot = "release", release_role = true, release_reservation = false }, attempt_failure = { manual = true, slot = "retain", release_role = false, release_reservation = false }, cancellation_acknowledged = { manual = true, slot = "retain", release_role = false, release_reservation = false } } },
  { at = { phase = "verify", state = "approved" }, slot = "release", dependencies = "done", unsuccessful_dependency = "block", accepts_work = false, terminal = false, legacy = "done", satisfies_dependencies = true, completion = "none" },
  { at = { phase = "verify", state = "accepted" }, slot = "release", dependencies = "done", unsuccessful_dependency = "block", accepts_work = false, terminal = true, legacy = "done", outcome = "success", satisfies_dependencies = true, completion = "covered_close" },
  { at = { phase = "build", state = "cancelled" }, slot = "release", dependencies = "done", unsuccessful_dependency = "block", accepts_work = false, terminal = true, legacy = "cancelled", outcome = "cancellation", satisfies_dependencies = false, completion = "none" },
]
edges = [
  { id = "claim", label = "Claim", from = { phase = "build", state = "available" }, to = { phase = "build", state = "working" }, operation = "claim", alias = "claim", authority = [{ kind = "task_role", role = "executor" }], inputs = [], dependencies = "done", guards = [], effects = [{ kind = "acquire_role", role = "executor" }, { kind = "slot", policy = "acquire" }], automatic = false, human_wait = false },
  { id = "block", label = "Block", from = { phase = "build", state = "working" }, to = { phase = "build", state = "blocked" }, operation = "block", authority = [{ kind = "task_role", role = "executor" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "block", save_resume = true }, { kind = "slot", policy = "retain" }], automatic = false, human_wait = false },
  { id = "resume", label = "Resume", from = { phase = "build", state = "blocked" }, to = { phase = "build", state = "working" }, operation = "resume", authority = [{ kind = "task_role", role = "executor" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "clear_block" }, { kind = "slot", policy = "retain" }], automatic = false, human_wait = false },
  { id = "submit", label = "Submit", from = { phase = "build", state = "working" }, to = { phase = "verify", state = "awaiting" }, operation = "request_review", authority = [{ kind = "task_role", role = "executor" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "release_role", role = "executor" }, { kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  { id = "review", label = "Review", from = { phase = "verify", state = "awaiting" }, to = { phase = "verify", state = "reviewing" }, operation = "claim", alias = "claim", authority = [{ kind = "task_role", role = "reviewer" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "acquire_role", role = "reviewer" }, { kind = "slot", policy = "acquire" }], automatic = false, human_wait = false },
  { id = "approve", label = "Approve", from = { phase = "verify", state = "reviewing" }, to = { phase = "verify", state = "approved" }, operation = "approve", authority = [{ kind = "task_role", role = "reviewer" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "review", approved = true }, { kind = "release_role", role = "reviewer" }, { kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  { id = "changes", label = "Changes", from = { phase = "verify", state = "reviewing" }, to = { phase = "build", state = "available" }, operation = "changes", authority = [{ kind = "task_role", role = "reviewer" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "review", approved = false }, { kind = "release_role", role = "reviewer" }, { kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  { id = "accept", label = "Accept", from = { phase = "verify", state = "approved" }, to = { phase = "verify", state = "accepted" }, operation = "close_covered", authority = [{ kind = "project_capability", capability = "owner" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "outcome", outcome = "success", satisfies_dependencies = true }, { kind = "slot", policy = "release" }], automatic = false, human_wait = true },
  { id = "cancel", label = "Cancel", from = { phase = "build", state = "available" }, to = { phase = "build", state = "cancelled" }, operation = "cancel", authority = [{ kind = "project_capability", capability = "owner" }, { kind = "system_capability", capability = "recovery" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "outcome", outcome = "cancellation", satisfies_dependencies = false }, { kind = "slot", policy = "release" }], automatic = false, human_wait = false },
]
"#;

/// [`REVIEWED`] as the board reads it once it is active.
pub(crate) fn reviewed() -> Operating {
    let custom = custom(REVIEWED);
    Operating {
        definition: custom.definition.definition().clone(),
        revision: custom.revision,
        generation: 1,
        accepted: Vec::new(),
    }
}

/// A task the workflow holds at `phase/state`, in the status its position projects to (ready at a
/// pair [`REVIEWED`] does not declare), filed by the owner, with no roles.
pub(crate) fn held(slug: &str, phase: &str, state: &str) -> Task {
    let w = reviewed();
    let status = (w.definition.positions.iter())
        .find(|p| p.at.phase == phase && p.at.state == state)
        .map_or_else(
            || "ready".to_string(),
            |p| crate::workflow::activate::projection(p.legacy),
        );
    let mut task = legacy(slug, &status);
    task.fields.entry("_board".into()).or_default().insert(
        "workflow".into(),
        json!({
            "phase": phase, "state": state, "generation": 1, "revision": 3,
            "digest": reviewed().revision.digest,
        }),
    );
    task
}

/// A task the board's own writers keep, in `status`.
pub(crate) fn legacy(slug: &str, status: &str) -> Task {
    Task {
        branch: None,
        id: 1,
        slug: slug.into(),
        title: slug.into(),
        body: String::new(),
        status: status.into(),
        priority: 2,
        parent: None,
        created_by: "owner".into(),
        created: 0.0,
        updated: 0.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: Vec::new(),
        instruction: false,
        fields: Default::default(),
    }
}

pub(crate) fn holding(mut task: Task, handle: &str, role: &str, health: Health) -> Task {
    task.roles.push(Held {
        handle: handle.into(),
        role: role.into(),
        since: 0.0,
        expires: None,
        health,
    });
    task
}

#[test]
fn a_task_no_workflow_holds_and_one_at_an_undeclared_pair_are_not_placed() {
    let w = reviewed();
    assert_eq!(w.placed(&legacy("sim/a", "ready")), None);
    assert_eq!(w.placed(&held("sim/a", "build", "nowhere")), None);
    let mut broken = legacy("sim/a", "ready");
    broken
        .fields
        .entry("_board".into())
        .or_default()
        .insert("workflow".into(), json!({"phase": "build"}));
    assert_eq!(
        w.placed(&broken),
        None,
        "a position not in the board's shape"
    );
    assert_eq!(w.finished(&held("sim/a", "build", "nowhere")), None);
    assert_eq!(w.holds_slot(&legacy("sim/a", "in_progress")), None);
}

#[test]
fn claims_are_the_edges_leaving_the_pair_that_give_a_role() {
    let w = reviewed();
    let claims = |phase, state| {
        let task = held("sim/a", phase, state);
        let placed = w.placed(&task).unwrap();
        (placed.claims().iter())
            .map(|c| (c.edge.id.clone(), c.role))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        claims("build", "available"),
        [("claim".to_string(), Role::Executor)]
    );
    assert_eq!(
        claims("verify", "awaiting"),
        [("review".to_string(), Role::Reviewer)]
    );
    for (phase, state) in [
        ("build", "working"),
        ("build", "blocked"),
        ("verify", "reviewing"),
        ("verify", "approved"),
        ("verify", "accepted"),
    ] {
        assert!(claims(phase, state).is_empty(), "{phase}/{state}");
    }
    let task = held("sim/a", "build", "available");
    let cancels: Vec<&str> = (w.placed(&task).unwrap().cancels().iter())
        .map(|e| e.id.as_str())
        .collect();
    assert_eq!(cancels, ["cancel"]);
    let task = held("sim/a", "verify", "awaiting");
    assert!(w.placed(&task).unwrap().cancels().is_empty());
}

#[test]
fn an_automatic_edge_is_no_claim() {
    let mut w = reviewed();
    for edge in &mut w.definition.edges {
        edge.automatic = true;
    }
    let task = held("sim/a", "build", "available");
    let placed = w.placed(&task).unwrap();
    assert!(placed.claims().is_empty());
    assert!(placed.cancels().is_empty());
}

#[test]
fn working_is_a_session_there_holding_the_role_the_position_staffs() {
    let w = reviewed();
    let task = held("sim/a", "build", "working");
    assert!(!w.placed(&task).unwrap().working(&task), "nobody holds it");
    for (health, working) in [
        (Health::Live, true),
        (Health::Stale, true),
        (Health::Unknown, true),
        (Health::Lost, false),
        (Health::Left, false),
    ] {
        let task = holding(
            held("sim/a", "build", "working"),
            "amber",
            "executor",
            health,
        );
        assert_eq!(
            w.placed(&task).unwrap().working(&task),
            working,
            "{health:?}"
        );
    }
    // A reviewer is no executor; a position nobody works at is never worked.
    let task = holding(
        held("sim/a", "build", "working"),
        "amber",
        "reviewer",
        Health::Live,
    );
    assert!(!w.placed(&task).unwrap().working(&task));
    let task = holding(
        held("sim/a", "verify", "awaiting"),
        "amber",
        "reviewer",
        Health::Live,
    );
    assert!(!w.placed(&task).unwrap().working(&task));
}

#[test]
fn the_lost_are_the_staffed_roles_holders_gone_while_nobody_there_holds_it() {
    let w = reviewed();
    let task = holding(
        held("sim/a", "build", "working"),
        "amber",
        "executor",
        Health::Lost,
    );
    let task = holding(task, "birch", "executor", Health::Left);
    let task = holding(task, "cedar", "reserved", Health::Lost);
    assert_eq!(w.placed(&task).unwrap().lost(&task), ["amber", "birch"]);
    let task = holding(task, "dune", "executor", Health::Stale);
    assert!(
        w.placed(&task).unwrap().lost(&task).is_empty(),
        "a holder there keeps the work going"
    );
    let task = holding(
        held("sim/a", "verify", "awaiting"),
        "amber",
        "executor",
        Health::Lost,
    );
    assert!(
        w.placed(&task).unwrap().lost(&task).is_empty(),
        "a role the position does not staff is no lost work"
    );
}

#[test]
fn a_manual_recovery_is_recorded_once_for_its_holder() {
    let w = reviewed();
    let mut task = held("sim/a", "build", "blocked");
    task.fields.get_mut("_board").unwrap().insert(
        "workflow".into(),
        json!({
            "phase": "build", "state": "blocked", "generation": 1, "revision": 4,
            "digest": w.revision.digest,
            "manual": {"trigger": "lost_holder", "holder": "amber", "role": "executor", "manual": true},
        }),
    );
    let placed = w.placed(&task).unwrap();
    assert!(placed.waits_on_person_for("amber"));
    assert!(!placed.waits_on_person_for("birch"));
    let task = held("sim/a", "build", "blocked");
    assert!(!w.placed(&task).unwrap().waits_on_person_for("amber"));
}

#[test]
fn a_slot_is_held_by_policy_and_finished_is_a_terminal_position() {
    let w = reviewed();
    for (phase, state, slot, finished) in [
        ("build", "available", false, false),
        ("build", "working", true, false),
        ("build", "blocked", true, false),
        ("verify", "awaiting", false, false),
        ("verify", "reviewing", true, false),
        ("verify", "approved", false, false),
        ("verify", "accepted", false, true),
        ("build", "cancelled", false, true),
    ] {
        let task = held("sim/a", phase, state);
        assert_eq!(w.holds_slot(&task), Some(slot), "{phase}/{state}");
        assert_eq!(w.finished(&task), Some(finished), "{phase}/{state}");
    }
    // Approved work projects to done, and is not finished: its acceptance is still to come.
    assert_eq!(held("sim/a", "verify", "approved").status, "done");
    for (status, finished) in [("done", true), ("cancelled", true), ("review", false)] {
        assert_eq!(
            w.finished(&legacy("sim/a", status)),
            Some(finished),
            "{status}"
        );
    }
}

#[test]
fn unsuccessful_prerequisites_are_read_from_outcomes_and_the_boards_cancelled() {
    let w = reviewed();
    let mut task = held("sim/a", "build", "available");
    task.depends = [
        "sim/gone",
        "sim/cancelled",
        "sim/c-held",
        "sim/accepted",
        "sim/done",
        "sim/approved",
    ]
    .map(String::from)
    .to_vec();
    let board = [
        legacy("sim/cancelled", "cancelled"),
        held("sim/c-held", "build", "cancelled"),
        held("sim/accepted", "verify", "accepted"),
        legacy("sim/done", "done"),
        held("sim/approved", "verify", "approved"),
    ];
    let find = |slug: &str| board.iter().find(|t| t.slug == slug);
    assert_eq!(w.unsuccessful(&task, find), ["sim/cancelled", "sim/c-held"]);
    // A task the workflow holds projects to cancelled only by its position: a held task whose row
    // reads cancelled at a pair with no such outcome ended no way the board can read.
    let mut odd = held("sim/odd", "build", "available");
    odd.status = "cancelled".into();
    let mut task = held("sim/a", "build", "available");
    task.depends = vec!["sim/odd".into()];
    assert!(w.unsuccessful(&task, |_| Some(&odd)).is_empty());
}

#[test]
fn a_done_predicate_is_met_by_satisfying_positions_and_the_boards_done() {
    let w = reviewed();
    let claim = (w.definition.edges.iter())
        .find(|e| e.id == "claim")
        .unwrap()
        .clone();
    let board = [
        legacy("sim/done", "done"),
        held("sim/approved", "verify", "approved"),
        held("sim/accepted", "verify", "accepted"),
        held("sim/reviewing", "verify", "reviewing"),
        legacy("sim/ready", "ready"),
    ];
    let find = |slug: &str| board.iter().find(|t| t.slug == slug);
    let needing = |depends: &[&str]| {
        let mut task = held("sim/a", "build", "available");
        task.depends = depends.iter().map(|s| s.to_string()).collect();
        task
    };
    assert!(w.dependencies_met(&needing(&[]), &claim, find));
    assert!(w.dependencies_met(
        &needing(&["sim/done", "sim/approved", "sim/accepted"]),
        &claim,
        find
    ));
    for unmet in ["sim/reviewing", "sim/ready", "sim/gone"] {
        assert!(
            !w.dependencies_met(&needing(&["sim/done", unmet]), &claim, find),
            "{unmet}"
        );
    }
    let mut none = claim.clone();
    none.dependencies = Dependency::None;
    assert!(w.dependencies_met(&needing(&["sim/gone"]), &none, find));
    let mut landed = claim.clone();
    landed.dependencies = Dependency::Landed;
    assert!(!w.dependencies_met(&needing(&[]), &landed, find));

    // `accepted_output` is met by the output standing for each prerequisite at the revision the
    // board's read has it at (each held task here is at revision 3), never by a position that
    // satisfies dependants alone, an output read at another revision, one recorded for a task now
    // where none stands, or the board's own done.
    let mut takes = claim.clone();
    takes.dependencies = Dependency::AcceptedOutput;
    let w = Operating {
        accepted: vec![
            ("sim/approved".into(), 3, "sim/approved@3".into()),
            ("sim/accepted".into(), 2, "sim/accepted@2".into()),
            ("sim/reviewing".into(), 3, "sim/reviewing@3".into()),
        ],
        ..w
    };
    assert!(w.dependencies_met(&needing(&[]), &takes, find));
    assert!(w.dependencies_met(&needing(&["sim/approved"]), &takes, find));
    for unmet in [
        "sim/accepted",
        "sim/reviewing",
        "sim/done",
        "sim/ready",
        "sim/gone",
    ] {
        assert!(
            !w.dependencies_met(&needing(&["sim/approved", unmet]), &takes, find),
            "{unmet}"
        );
    }
}

#[test]
fn finished_is_a_terminal_position_for_held_work_and_done_or_cancelled_for_the_rest() {
    let w = reviewed();
    for (task, finished) in [
        (held("sim/a", "verify", "approved"), false),
        (held("sim/a", "verify", "accepted"), true),
        (held("sim/a", "build", "cancelled"), true),
        (legacy("sim/a", "done"), true),
        (legacy("sim/a", "cancelled"), true),
        (legacy("sim/a", "review"), false),
    ] {
        assert_eq!(
            super::finished(Some(&w), &task),
            finished,
            "{:?}",
            task.fields
        );
    }
    // No workflow read: a held task is never finished by the status it projects to.
    assert!(!super::finished(None, &held("sim/a", "verify", "accepted")));
    assert!(super::finished(None, &legacy("sim/a", "done")));
}
