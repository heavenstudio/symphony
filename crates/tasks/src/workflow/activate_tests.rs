//! The activation planner and the host's support, without a board: where each task goes, and
//! every refusal that leaves the previous workflow active.

use super::*;
use crate::workflow::{Custom, Mapping, written};
use symphony_api::workflow::Source;

/// Two phases: work is built (available, worked by a held executor, or blocked), then verified
/// (awaiting, accepted by the owner).
pub(crate) const TWO: &str = r#"
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
  { id = "blocked", label = "Blocked" }, { id = "awaiting", label = "Awaiting" },
  { id = "accepted", label = "Accepted" },
]
capabilities = ["owner"]
adapters = []
guards = []
positions = [
  { at = { phase = "build", state = "available" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = true, terminal = false, legacy = "ready", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "build", state = "working" }, staffing = "executor", slot = "acquire", dependencies = "none", unsuccessful_dependency = "block", accepts_work = true, terminal = false, legacy = "in_progress", satisfies_dependencies = false, completion = "none", recovery = { release = { target = { phase = "build", state = "available" }, manual = false, slot = "release", release_role = true, release_reservation = false }, lost_holder = { target = { phase = "build", state = "available" }, manual = false, slot = "release", release_role = true, release_reservation = false }, attempt_failure = { manual = true, slot = "retain", release_role = false, release_reservation = false }, cancellation_acknowledged = { manual = true, slot = "retain", release_role = false, release_reservation = false } } },
  { at = { phase = "build", state = "blocked" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = false, legacy = "blocked", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "verify", state = "awaiting" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = false, legacy = "review", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "verify", state = "accepted" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = true, legacy = "done", outcome = "success", satisfies_dependencies = true, completion = "covered_close" },
]
edges = [
  { id = "claim", label = "Claim", from = { phase = "build", state = "available" }, to = { phase = "build", state = "working" }, operation = "claim", authority = [{ kind = "task_role", role = "executor" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "acquire_role", role = "executor" }, { kind = "slot", policy = "acquire" }], automatic = false, human_wait = false },
  { id = "submit", label = "Submit", from = { phase = "build", state = "working" }, to = { phase = "verify", state = "awaiting" }, operation = "request_review", authority = [{ kind = "task_role", role = "executor" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  { id = "block", label = "Block", from = { phase = "build", state = "available" }, to = { phase = "build", state = "blocked" }, operation = "block", authority = [{ kind = "project_capability", capability = "owner" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "block", save_resume = true }, { kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  { id = "resume", label = "Resume", from = { phase = "build", state = "blocked" }, to = { phase = "build", state = "available" }, operation = "resume", authority = [{ kind = "project_capability", capability = "owner" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "clear_block" }, { kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  { id = "accept", label = "Accept", from = { phase = "verify", state = "awaiting" }, to = { phase = "verify", state = "accepted" }, operation = "close_covered", authority = [{ kind = "project_capability", capability = "owner" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "outcome", outcome = "success", satisfies_dependencies = true }, { kind = "slot", policy = "release" }], automatic = false, human_wait = true },
]
"#;

/// Every open board status mapped onto TWO.
pub(crate) const MAPPED: &str = r#"
[[workflow_mappings]]
from = { phase = "backlog", state = "available" }
to = { phase = "build", state = "available" }
[[workflow_mappings]]
from = { phase = "concept", state = "available" }
to = { phase = "build", state = "available" }
[[workflow_mappings]]
from = { phase = "design", state = "available" }
to = { phase = "build", state = "available" }
[[workflow_mappings]]
from = { phase = "ready", state = "available" }
to = { phase = "build", state = "available" }
[[workflow_mappings]]
from = { phase = "in_progress", state = "working" }
to = { phase = "build", state = "working" }
[[workflow_mappings]]
from = { phase = "blocked", state = "waiting" }
to = { phase = "build", state = "blocked" }
[[workflow_mappings]]
from = { phase = "review", state = "awaiting_review" }
to = { phase = "verify", state = "awaiting" }
"#;

const COMMIT: &str = "1111111111111111111111111111111111111111";
const BLOB: &str = "2222222222222222222222222222222222222222";

/// `text` as committed, its revision from a fixed commit.
pub(crate) fn custom(text: &str) -> Custom {
    let (definition, mappings) = written(text).unwrap();
    let definition = definition.expect("a [workflow]");
    let revision = definition
        .revision(Source {
            commit: COMMIT.into(),
            project_blob: BLOB.into(),
        })
        .unwrap();
    Custom {
        definition,
        revision,
        mappings,
    }
}

fn pair(phase: &str, state: &str) -> Pair {
    Pair {
        phase: phase.into(),
        state: state.into(),
    }
}

fn standing(slug: &str, status: &str) -> Standing {
    Standing {
        slug: slug.into(),
        version: 3,
        status: status.into(),
        position: None,
        blocked_from: None,
        roles: Vec::new(),
        attempts: Vec::new(),
    }
}

fn position_mut<'a>(definition: &'a mut Definition, at: &Pair) -> &'a mut Position {
    (definition.positions.iter_mut())
        .find(|p| p.at == *at)
        .expect("the fixture declares it")
}

fn by() -> Principal {
    crate::workflow::board_service("activation")
}

fn planned(custom: &Custom, old: Option<&Definition>, tasks: &[Standing]) -> Result<Plan, String> {
    plan(custom, old, tasks, 1, &by(), 50.0)
}

#[test]
fn the_local_host_supports_a_definition_of_its_own_capabilities_and_nothing_more() {
    let two = custom(&format!("{TWO}{MAPPED}"));
    assert_eq!(supported(&two.definition, &Host::local()), Ok(()));
    let mut accept = Host::default();
    assert_eq!(
        supported(&two.definition, &accept),
        Err("unsupported capability: owner".into())
    );
    accept.grants = Grants::none().grant("owner", crate::workflow::agent_session("amber"));
    assert_eq!(supported(&two.definition, &accept), Ok(()));
}

#[test]
fn a_definition_needing_what_this_release_cannot_run_is_unsupported() {
    // An attempt's ending is the board's recovery service's to answer (`Store::observe_attempt`).
    for (from, to) in [
        (
            "attempt_failure = { manual = true, slot = \"retain\"",
            "attempt_failure = { target = { phase = \"build\", state = \"available\" }, manual = false, slot = \"release\"",
        ),
        (
            "cancellation_acknowledged = { manual = true, slot = \"retain\"",
            "cancellation_acknowledged = { target = { phase = \"build\", state = \"available\" }, manual = false, slot = \"release\"",
        ),
    ] {
        let definition = written(&TWO.replacen(from, to, 1)).unwrap().0.unwrap();
        assert_eq!(supported(&definition, &Host::local()), Ok(()), "{to}");
    }
    let accepted_output = TWO.replacen(
        "dependencies = \"none\", unsuccessful_dependency = \"block\", accepts_work = true, terminal = false, legacy = \"ready\"",
        "dependencies = \"accepted_output\", unsuccessful_dependency = \"block\", accepts_work = true, terminal = false, legacy = \"ready\"",
        1,
    );
    let definition = written(&accepted_output).unwrap().0.unwrap();
    assert_eq!(
        supported(&definition, &Host::local()),
        Err("unsupported dependency at build/available: accepted_output (tools/workflow-accepted-output)".into())
    );
    let guarded = TWO
        .replacen("adapters = []", "adapters = [\"approvals\"]", 1)
        .replacen(
            "guards = []",
            "guards = [{ id = \"approved\", adapter = \"approvals\" }]",
            1,
        );
    let definition = written(&guarded).unwrap().0.unwrap();
    let mut host = Host::local();
    assert_eq!(
        supported(&definition, &host),
        Err("unsupported adapter: approvals".into())
    );
    host.adapters.push("approvals".into());
    assert!(
        supported(&definition, &host)
            .unwrap_err()
            .starts_with("unsupported guard: approved")
    );
}

#[test]
fn parent_integration_and_adapters_are_unsupported() {
    let mut definition = custom(TWO).definition.definition().clone();
    definition.edges[0].effects.push(Effect::ParentIntegration);
    let integrating = Validated::new(definition.clone());
    if let Ok(integrating) = integrating {
        assert!(
            supported(&integrating, &Host::local())
                .unwrap_err()
                .starts_with("unsupported edge accept: parent integration")
        );
    }
    definition.edges[0].effects.pop();
    let mut prepared = definition.clone();
    prepared.adapters = vec!["lander".into()];
    let mut prepare = prepared.edges[0].clone();
    prepare.id = "prepare".into();
    prepare.from = Pair {
        phase: "build".into(),
        state: "working".into(),
    };
    prepare.to = Pair {
        phase: "verify".into(),
        state: "awaiting".into(),
    };
    prepare.operation = symphony_api::workflow::Operation::PrepareExternal;
    prepare.human_wait = false;
    prepare.effects = vec![
        Effect::Prepare {
            adapter: "lander".into(),
        },
        Effect::Slot {
            policy: symphony_api::workflow::Slot::Release,
        },
    ];
    prepared.edges.push(prepare);
    let prepared = Validated::new(prepared).unwrap();
    let mut lander = Host::local();
    lander.adapters.push("lander".into());
    // An adapter the host registers takes its edges; one it does not refuses the activation.
    assert_eq!(supported(&prepared, &lander), Ok(()));
    assert_eq!(
        supported(&prepared, &Host::local()),
        Err("unsupported adapter: lander".into())
    );
    definition.edges[0].dependencies = Dependency::Landed;
    let landed = Validated::new(definition.clone()).unwrap();
    assert_eq!(
        supported(&landed, &Host::local()),
        Err("unsupported edge accept: a dependency on landed".into())
    );
    definition.edges[0].dependencies = Dependency::AcceptedOutput;
    let output = Validated::new(definition.clone()).unwrap();
    assert!(
        supported(&output, &Host::local())
            .unwrap_err()
            .starts_with("unsupported edge accept: a dependency on accepted_output")
    );
}

fn rework(d: &mut Definition) -> &mut Edge {
    d.edges.iter_mut().find(|e| e.id == "rework").unwrap()
}

#[test]
fn an_automatic_edge_is_one_the_boards_recovery_service_alone_can_take() {
    use crate::workflow::recover::attempt_tests::attempts;
    let base = custom(&attempts(false)).definition.definition().clone();
    let supports = |change: &dyn Fn(&mut Definition)| {
        let mut definition = base.clone();
        change(&mut definition);
        supported(&Validated::new(definition).unwrap(), &Host::local())
    };
    assert_eq!(supports(&|_| {}), Ok(()));
    // Not a capability the host grants the board's recovery service.
    assert_eq!(
        supports(&|d| {
            rework(d).authority = vec![Authority::ProjectCapability {
                capability: "owner".into(),
            }];
        }),
        Err(
            "unsupported edge rework: an automatic edge no authority of which this host grants \
             the board's recovery service, which takes it (grant it `recovery`, and name that \
             capability)"
                .into()
        )
    );
    let mut owner_only = Host::local();
    owner_only.grants = Grants::none()
        .grant("owner", crate::workflow::local_owner())
        .grant("recovery", crate::workflow::local_owner());
    assert!(
        supported(&Validated::new(base.clone()).unwrap(), &owner_only)
            .unwrap_err()
            .starts_with("unsupported edge rework: an automatic edge no authority")
    );
    // Nothing the board cannot give.
    assert_eq!(
        supports(&|d| rework(d).inputs = vec!["reason".into()]),
        Err(
            "unsupported edge rework: an automatic edge asks for inputs, which the board has \
             none of to give"
                .into()
        )
    );
    assert!(
        supports(&|d| {
            rework(d).effects.push(Effect::AcquireRole {
                role: symphony_api::workflow::Role::Executor,
            });
        })
        .unwrap_err()
        .starts_with("unsupported edge rework: an automatic edge gives a role")
    );
    // One automatic edge out of a pair, so the board never chooses.
    let twice = supports(&|d| {
        let mut again = rework(d).clone();
        again.id = "again".into();
        again.operation = symphony_api::workflow::Operation::Advance;
        again.retry = None;
        d.edges.push(again);
    });
    assert_eq!(
        twice,
        Err(
            "unsupported edge again: an automatic edge, as is rework from verify/checking: the \
             board takes one on an attempt's failure there, and could not choose"
                .into()
        )
    );
    // The board counts only its own takes.
    let bounded = supports(&|d| {
        let unstick = d.edges.iter_mut().find(|e| e.id == "unstick").unwrap();
        unstick.retry = Some(symphony_api::workflow::RetryBound {
            maximum: 2,
            exhausted: pair("verify", "stuck"),
        });
    });
    assert_eq!(
        bounded,
        Err(
            "unsupported edge unstick: a retry bound on an edge a caller takes; the board counts \
             the takes of an automatic edge alone"
                .into()
        )
    );
}

#[test]
fn a_live_attempt_keeps_its_task_from_moving_until_it_ends_or_is_cancelled() {
    let two = custom(&format!("{TWO}{MAPPED}"));
    let mut working = standing("sim/working", "in_progress");
    working.attempts = vec![
        Live {
            attempt: "aa11".into(),
            cancelled: true,
        },
        Live {
            attempt: "bb22".into(),
            cancelled: false,
        },
    ];
    assert_eq!(
        planned(&two, None, &[working.clone()]),
        Err(
            "sim/working runs execution attempt bb22, and activation would move it or change its \
             staffing; an attempt drains first (let it end, or cancel it), and is never rebound \
             to another position"
                .into()
        )
    );
    // Cancelled, no result of it counts: the task moves, and the attempt keeps its ending.
    working.attempts[1].cancelled = true;
    assert!(planned(&two, None, &[working.clone()]).is_ok());
    // Drained, nothing holds the task.
    working.attempts.clear();
    assert!(planned(&two, None, &[working]).is_ok());
}

#[test]
fn a_task_activation_keeps_where_it_stands_keeps_its_live_attempt() {
    let two = custom(&format!("{TWO}{MAPPED}"));
    let mut working = standing("sim/working", "in_progress");
    working.position = Some(TaskPosition {
        phase: "build".into(),
        state: "working".into(),
        resume: None,
        generation: 1,
        revision: 4,
        digest: two.revision.digest.clone(),
        manual: None,
    });
    working.roles = vec![("amber".into(), "executor".into(), true)];
    working.attempts = vec![Live {
        attempt: "bb22".into(),
        cancelled: false,
    }];
    let definition = two.definition.definition();
    let placed = plan(&two, Some(definition), &[working.clone()], 2, &by(), 60.0).unwrap();
    assert_eq!(
        placed.kept,
        vec![("sim/working".to_string(), working.version)]
    );
    assert!(placed.moved.is_empty());
    // Restaffed in place, it is moved under the attempt, and refused.
    let mut restaffed = definition.clone();
    position_mut(&mut restaffed, &pair("build", "working")).staffing =
        Some(symphony_api::workflow::Role::Reviewer);
    let restaffed = Custom {
        definition: Validated::new(restaffed).unwrap(),
        ..custom(&format!("{TWO}{MAPPED}"))
    };
    assert!(
        plan(&restaffed, Some(definition), &[working], 2, &by(), 60.0)
            .unwrap_err()
            .starts_with("sim/working runs execution attempt bb22")
    );
}

#[test]
fn a_first_activation_places_every_open_task_from_its_built_in_pair() {
    let two = custom(&format!("{TWO}{MAPPED}"));
    let mut blocked = standing("sim/blocked", "blocked");
    blocked.blocked_from = Some("in_progress".into());
    let mut idle = standing("sim/idle", "in_progress");
    idle.roles = vec![("amber".into(), "executor".into(), false)];
    let tasks = [
        standing("sim/ready", "ready"),
        standing("sim/review", "review"),
        blocked,
        idle,
    ];
    let plan = planned(&two, None, &tasks).unwrap();
    assert!(plan.kept.is_empty());
    let placed: Vec<(&str, Pair, Option<Pair>, &str, u64)> = (plan.moved.iter())
        .map(|m| {
            let t = &m.transition;
            (
                t.slug.as_str(),
                t.to.clone(),
                t.resume.clone(),
                t.status.as_str(),
                t.revision,
            )
        })
        .collect();
    assert_eq!(
        placed,
        vec![
            ("sim/ready", pair("build", "available"), None, "ready", 1),
            ("sim/review", pair("verify", "awaiting"), None, "review", 1),
            (
                "sim/blocked",
                pair("build", "blocked"),
                Some(pair("build", "working")),
                "blocked",
                1
            ),
            ("sim/idle", pair("build", "working"), None, "in_progress", 1),
        ]
    );
    let first = &plan.moved[0];
    assert_eq!((first.version, first.expected), (3, 0));
    let t = &first.transition;
    assert_eq!(t.id, "sim/ready@1");
    assert_eq!(t.from, Some(pair("ready", "available")));
    assert_eq!(
        (t.generation, t.activation, t.edge.as_deref()),
        (1, Some(1), None)
    );
    assert_eq!(t.digest, two.revision.digest);
    assert_eq!(t.principal, by());
    assert_eq!(t.at, 50.0);
}

#[test]
fn a_pair_or_resume_pair_without_a_mapping_refuses_the_whole_activation() {
    let two = custom(TWO);
    assert_eq!(
        planned(&two, None, &[standing("sim/ready", "ready")]),
        Err(
            "workflow_mappings: sim/ready is at ready/available, which the workflow no longer \
             declares; map it to a pair it declares"
                .into()
        )
    );
    let only_blocked = custom(&format!(
        "{TWO}[[workflow_mappings]]\nfrom = {{ phase = \"blocked\", state = \"waiting\" }}\nto = {{ phase = \"build\", state = \"blocked\" }}\n"
    ));
    let mut blocked = standing("sim/blocked", "blocked");
    blocked.blocked_from = Some("design".into());
    assert_eq!(
        planned(&only_blocked, None, &[blocked.clone()]),
        Err(
            "workflow_mappings: sim/blocked resumes to design/available, which the workflow no \
             longer declares; map it to a pair it declares"
                .into()
        )
    );
    // A blocked task the board recorded no source for resumes to nothing, and needs no mapping.
    blocked.blocked_from = None;
    let plan = planned(&only_blocked, None, &[blocked]).unwrap();
    assert_eq!(plan.moved[0].transition.resume, None);
    // A held concept, design or review task starts from its held pair.
    let mut held = standing("sim/held", "review");
    held.roles = vec![("brisk".into(), "reviewer".into(), false)];
    assert_eq!(
        planned(&custom(&format!("{TWO}{MAPPED}")), None, &[held]),
        Err(
            "workflow_mappings: sim/held is at review/held, which the workflow no longer \
             declares; map it to a pair it declares"
                .into()
        )
    );
}

#[test]
fn a_mapping_never_fabricates_an_outcome_satisfaction_or_completion() {
    let onto_accepted = custom(&format!(
        "{TWO}[[workflow_mappings]]\nfrom = {{ phase = \"review\", state = \"awaiting_review\" }}\nto = {{ phase = \"verify\", state = \"accepted\" }}\n"
    ));
    assert_eq!(
        planned(&onto_accepted, None, &[standing("sim/review", "review")]),
        Err(
            "sim/review would stand at verify/accepted with the outcome Success that \
             review/awaiting_review does not record; a mapping never fabricates one"
                .into()
        )
    );
    let mut definition = custom(TWO).definition.definition().clone();
    let blocked = position_mut(&mut definition, &pair("build", "blocked"));
    blocked.satisfies_dependencies = true;
    let satisfying = Custom {
        definition: Validated::new(definition.clone()).unwrap(),
        ..custom(&format!("{TWO}{MAPPED}"))
    };
    assert!(
        planned(&satisfying, None, &[standing("sim/blocked", "blocked")])
            .unwrap_err()
            .contains("with its dependants satisfied that blocked/waiting does not record")
    );
    let blocked = position_mut(&mut definition, &pair("build", "blocked"));
    blocked.satisfies_dependencies = false;
    blocked.completion = Completion::Landing;
    let completing = Custom {
        definition: Validated::new(definition).unwrap(),
        ..custom(&format!("{TWO}{MAPPED}"))
    };
    assert!(
        planned(&completing, None, &[standing("sim/blocked", "blocked")])
            .unwrap_err()
            .contains("with the completion Landing")
    );
}

#[test]
fn a_live_holder_keeps_its_task_from_moving_until_its_work_drains() {
    let two = custom(&format!("{TWO}{MAPPED}"));
    let mut working = standing("sim/working", "in_progress");
    working.roles = vec![
        ("idle".into(), "reserved".into(), true),
        ("amber".into(), "executor".into(), true),
    ];
    assert_eq!(
        planned(&two, None, &[working.clone()]),
        Err(
            "sim/working is held by amber as its executor, and activation would move it or \
             change its staffing; a live holder's session drains first (release it, or let its \
             work finish), as it is no attempt the board sees yet"
                .into()
        )
    );
    // Lost, its holder no longer stands in for a live attempt.
    working.roles[1].2 = false;
    assert!(planned(&two, None, &[working]).is_ok());
}

#[test]
fn a_reactivation_moves_only_what_changes_and_fences_the_rest() {
    let old = custom(&format!("{TWO}{MAPPED}"));
    let at = |phase: &str, state: &str, revision: u64| TaskPosition {
        phase: phase.into(),
        state: state.into(),
        resume: None,
        generation: 1,
        revision,
        digest: old.revision.digest.clone(),
        manual: None,
    };
    let mut kept = standing("sim/kept", "ready");
    kept.position = Some(at("build", "available", 4));
    let mut verify = standing("sim/verify", "review");
    verify.position = Some(at("verify", "awaiting", 2));
    // The new definition drops verify/awaiting, maps it back to build, and projects build's
    // working pair to another status.
    let mut definition = old.definition.definition().clone();
    let awaiting = pair("verify", "awaiting");
    definition.positions.retain(|p| p.at != awaiting);
    let mut accept_working = (definition.edges.iter())
        .find(|e| e.id == "accept")
        .unwrap()
        .clone();
    accept_working.id = "accept_working".into();
    accept_working.from = pair("build", "working");
    definition
        .edges
        .retain(|e| e.from != awaiting && e.to != awaiting);
    definition.edges.push(accept_working);
    let mut text_mapping = vec![Mapping {
        from: pair("verify", "awaiting"),
        to: pair("build", "available"),
    }];
    let new = Custom {
        definition: Validated::new(definition.clone()).unwrap(),
        revision: old.revision.clone(),
        mappings: text_mapping.clone(),
    };
    let placed = plan(
        &new,
        Some(old.definition.definition()),
        &[kept.clone(), verify.clone()],
        2,
        &by(),
        60.0,
    )
    .unwrap();
    assert_eq!(placed.kept, vec![("sim/kept".to_string(), 3)]);
    assert_eq!(placed.moved.len(), 1);
    let moved = &placed.moved[0];
    assert_eq!((moved.expected, moved.transition.revision), (2, 3));
    assert_eq!(moved.transition.from, Some(pair("verify", "awaiting")));
    assert_eq!(moved.transition.to, pair("build", "available"));
    assert_eq!(moved.transition.status, "ready");
    assert_eq!(moved.transition.id, "sim/verify@3");

    // A pair whose projection changes moves its task, though its pair stays.
    let mut projected = definition.clone();
    position_mut(&mut projected, &pair("build", "available")).legacy = LegacyStatus::Backlog;
    let new = Custom {
        definition: Validated::new(projected).unwrap(),
        revision: old.revision.clone(),
        mappings: text_mapping.clone(),
    };
    let projected = plan(
        &new,
        Some(old.definition.definition()),
        &[kept.clone()],
        2,
        &by(),
        60.0,
    )
    .unwrap();
    let transition = &projected.moved[0].transition;
    assert_eq!(transition.status, "backlog");
    assert_eq!(transition.from, Some(transition.to.clone()));

    // A staffing change refuses a live holder, though the task stays where it is.
    let mut restaffed = definition.clone();
    let recovery = position_mut(&mut restaffed, &pair("build", "working"))
        .recovery
        .clone();
    let available = position_mut(&mut restaffed, &pair("build", "available"));
    available.staffing = Some(symphony_api::workflow::Role::Reviewer);
    available.recovery = recovery;
    kept.roles = vec![("amber".into(), "reviewer".into(), true)];
    text_mapping.clear();
    text_mapping.push(Mapping {
        from: pair("verify", "awaiting"),
        to: pair("build", "available"),
    });
    let new = Custom {
        definition: Validated::new(restaffed).unwrap(),
        revision: old.revision.clone(),
        mappings: text_mapping,
    };
    assert!(
        plan(
            &new,
            Some(old.definition.definition()),
            &[kept],
            2,
            &by(),
            60.0
        )
        .unwrap_err()
        .starts_with("sim/kept is held by amber as its reviewer")
    );

    // A position with no workflow active is the board's error, not a placement.
    assert_eq!(
        planned(&old, None, &[verify]),
        Err("sim/verify has a workflow position, and no workflow is active".into())
    );
}

#[test]
fn a_status_that_is_no_board_status_is_refused() {
    let two = custom(&format!("{TWO}{MAPPED}"));
    assert_eq!(
        planned(&two, None, &[standing("sim/odd", "waiting")]),
        Err("sim/odd has the status waiting, which is no board status".into())
    );
}

fn placing(mappings: &[(Pair, Pair)]) -> Placing {
    let two = custom(&format!("{TWO}{MAPPED}"));
    let mut placing = Placing {
        definition: two.definition.definition().clone(),
        mappings: two.mappings,
        generation: 4,
        digest: "d".repeat(64),
    };
    for (from, to) in mappings {
        placing.mappings.retain(|m| m.from != *from);
        placing.mappings.push(Mapping {
            from: from.clone(),
            to: to.clone(),
        });
    }
    placing
}

fn reopened(placing: &Placing, status: &str, roles: &[(&str, &str)]) -> Result<Transition, String> {
    let roles: Vec<(String, String, bool)> = (roles.iter())
        .map(|(h, r)| (h.to_string(), r.to_string(), true))
        .collect();
    let task = Reopening {
        slug: "sim/a",
        left: "done",
        status,
        roles: &roles,
    };
    placing.reopened(&task, &by(), 7.0)
}

#[test]
fn a_reopened_task_is_placed_where_the_activation_maps_its_status_as_its_first_transition() {
    let placed = reopened(&placing(&[]), "review", &[]).unwrap();
    assert_eq!(
        placed,
        Transition {
            id: "sim/a@1".into(),
            slug: "sim/a".into(),
            revision: 1,
            generation: 4,
            digest: "d".repeat(64),
            edge: None,
            operation: None,
            activation: None,
            from: Some(pair("done", "approved")),
            to: pair("verify", "awaiting"),
            resume: None,
            status: "review".into(),
            principal: by(),
            effects: Vec::new(),
            key: None,
            recovery: None,
            at: 7.0,
        }
    );
}

#[test]
fn a_reopening_the_active_workflow_cannot_place_is_refused_naming_the_pair() {
    // Its reviewer holds it: built-in v1's held review, which nothing maps.
    assert_eq!(
        reopened(&placing(&[]), "review", &[("birch", "reviewer")]).unwrap_err(),
        "workflow_mappings: sim/a reopens at review/held, which the workflow no longer declares; \
         map it to a pair it declares"
    );
    // A mapping to a pair of another status.
    let elsewhere = placing(&[(pair("ready", "available"), pair("verify", "awaiting"))]);
    assert_eq!(
        reopened(&elsewhere, "ready", &[]).unwrap_err(),
        "workflow_mappings: sim/a reopens at verify/awaiting, which stands for review, not ready, \
         the status its move asks for"
    );
    // A mapping that would accept it.
    let accepted = placing(&[(
        pair("review", "awaiting_review"),
        pair("verify", "accepted"),
    )]);
    assert!(
        reopened(&accepted, "review", &[])
            .unwrap_err()
            .starts_with("sim/a would stand at verify/accepted with the outcome")
    );
    assert_eq!(
        reopened(&placing(&[]), "nowhere", &[]).unwrap_err(),
        "sim/a has the status nowhere, which is no board status"
    );
}
