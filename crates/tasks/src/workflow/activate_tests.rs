//! The activation planner and the host's support, without a board: where each task goes, and
//! every refusal that leaves the previous workflow active.

use super::*;
use crate::store_fixture::pair;
use crate::workflow::{Custom, Mapping, written};
use symphony_api::workflow::Source;

/// Two phases: work is built (available, worked by a held executor, or blocked), then verified
/// (awaiting, accepted by the owner). The server's tests read the same file.
pub(crate) const TWO: &str = concat!("\n", include_str!("../../tests/fixtures/two_workflow.toml"));

/// `custom` as the active workflow admission and recovery decide under, at generation 2.
pub(crate) fn context(custom: &Custom) -> crate::workflow::admit::Context<'_> {
    crate::workflow::admit::Context {
        definition: custom.definition.definition(),
        revision: &custom.revision,
        generation: 2,
    }
}

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

/// `text` as committed, its revision from a fixed commit and a file whose id is its bytes' own, as
/// git's is (here their SHA-256, as a repository of that object format names it): a text is its
/// blob, and another text another blob.
pub(crate) fn custom(text: &str) -> Custom {
    use sha2::{Digest, Sha256};
    let (definition, mappings) = written(text).unwrap();
    let definition = definition.expect("a [workflow]");
    let blob = (Sha256::digest(text.as_bytes()).iter())
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let revision = definition
        .revision(Source {
            commit: COMMIT.into(),
            project_blob: blob,
        })
        .unwrap();
    let authorities = crate::workflow::authorities(text, Some(&definition)).unwrap();
    Custom {
        definition,
        revision,
        mappings,
        project: (!authorities.is_empty())
            .then(|| crate::workflow::attested_project(text))
            .flatten(),
        authorities,
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
        accepted: None,
        attempts: Vec::new(),
        retired: 0,
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
    let landed = TWO.replacen(
        "dependencies = \"none\", unsuccessful_dependency = \"block\", accepts_work = true, terminal = false, legacy = \"ready\"",
        "dependencies = \"landed\", unsuccessful_dependency = \"block\", accepts_work = true, terminal = false, legacy = \"ready\"",
        1,
    );
    let definition = written(&landed).unwrap().0.unwrap();
    assert_eq!(
        supported(&definition, &Host::local()),
        Err("unsupported dependency at build/available: landed".into())
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
    let unattested = "unsupported guard: approved (its adapter approvals needs an approval \
                      authority committed in [[workflow_authorities]])";
    assert_eq!(supported(&definition, &host), Err(unattested.into()));
    host.adapters.push("approvals".into());
    assert_eq!(supported(&definition, &host), Err(unattested.into()));
    // An authority committed for its adapter makes the guard one this release answers.
    let attested = crate::workflow::guard::tests::guarded();
    assert_eq!(
        supported_with(&attested.definition, &Host::local(), &attested.authorities),
        Ok(())
    );
    // Its authorities bind the project the workflow names, and refuse a workflow naming none.
    assert_eq!(bound(&attested), Ok(()));
    for project in [None, Some(" ".to_string())] {
        let unbound = Custom {
            project,
            ..attested.clone()
        };
        assert!(bound(&unbound).unwrap_err().contains("names none"));
        assert_eq!(
            bound(&Custom {
                authorities: Vec::new(),
                ..unbound
            }),
            Ok(())
        );
    }
    let mut later = attested.authorities.clone();
    later[0].contract = 2;
    assert!(
        supported_with(&attested.definition, &Host::local(), &later)
            .unwrap_err()
            .contains("contract 2")
    );
    // A guarded pair a route reaches around its guard is refused before anything else runs.
    let mut around = attested.definition.definition().clone();
    let mut shortcut = around
        .edges
        .iter()
        .find(|e| e.id == "accept")
        .unwrap()
        .clone();
    shortcut.id = "shortcut".into();
    shortcut.from = Pair {
        phase: "build".into(),
        state: "working".into(),
    };
    shortcut.guards.clear();
    around.edges.push(shortcut);
    let around = Validated::new(around).expect("a valid graph, if not a supported one");
    assert!(
        supported_with(&around, &Host::local(), &attested.authorities)
            .unwrap_err()
            .starts_with("unguarded route: edge shortcut enters verify/accepted under no guard")
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
    let landed = Validated::new(definition).unwrap();
    assert_eq!(
        supported(&landed, &Host::local()),
        Err("unsupported edge accept: a dependency on landed".into())
    );
}

#[test]
fn a_guard_on_an_edge_its_adapter_prepares_or_finalizes_refuses_the_activation() {
    let attested = crate::workflow::guard::tests::guarded();
    let mut lander = Host::local();
    lander.adapters.push("lander".into());
    // `submit` prepares, the only edge into verify/awaiting, so guarding it guards the pair;
    // `accept` finalizes, keeping the outcome its terminal pair needs.
    for (id, operation, effect) in [
        (
            "submit",
            symphony_api::workflow::Operation::PrepareExternal,
            Effect::Prepare {
                adapter: "lander".into(),
            },
        ),
        (
            "accept",
            symphony_api::workflow::Operation::ConfirmExternal,
            Effect::Finalize {
                adapter: "lander".into(),
            },
        ),
    ] {
        let mut definition = attested.definition.definition().clone();
        definition.adapters.push("lander".into());
        // A confirm completes a pair that asks for no covered close.
        for position in &mut definition.positions {
            position.completion = symphony_api::workflow::Completion::None;
        }
        let edge = (definition.edges.iter_mut()).find(|e| e.id == id).unwrap();
        edge.guards = vec!["approved".into()];
        edge.operation = operation;
        edge.human_wait = false;
        edge.effects.push(effect);
        let adapted = Validated::new(definition).expect("a valid graph, if not a supported one");
        assert_eq!(
            supported_with(&adapted, &lander, &attested.authorities),
            Err(format!(
                "unsupported edge {id}: a guard on an edge its adapter prepares or finalizes, \
                 which takes no attestation yet"
            ))
        );
    }
    // The same edge with no adapter's effect stays supported.
    assert_eq!(
        supported_with(&attested.definition, &lander, &attested.authorities),
        Ok(())
    );
}

#[test]
fn a_definition_depending_on_accepted_outputs_is_supported_on_its_edges_and_positions() {
    let outputs = custom(&crate::workflow::admit::tests::outputs());
    let claim = (outputs.definition.definition().edges.iter())
        .find(|e| e.id == "claim")
        .unwrap();
    assert_eq!(claim.dependencies, Dependency::AcceptedOutput);
    assert_eq!(supported(&outputs.definition, &Host::local()), Ok(()));
    let mut definition = outputs.definition.definition().clone();
    for position in &mut definition.positions {
        position.dependencies = Dependency::AcceptedOutput;
    }
    for edge in &mut definition.edges {
        edge.dependencies = Dependency::AcceptedOutput;
    }
    let everywhere = Validated::new(definition).unwrap();
    assert_eq!(supported(&everywhere, &Host::local()), Ok(()));
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
            "unsupported edge rework: no authority of this automatic edge is a capability the \
             host grants the board's recovery service"
                .into()
        )
    );
    // A task role is never the board's.
    assert!(
        supports(&|d| {
            rework(d).authority = vec![Authority::TaskRole {
                role: symphony_api::workflow::Role::Executor,
            }];
        })
        .unwrap_err()
        .starts_with("unsupported edge rework: no authority of this automatic edge")
    );
    let mut owner_only = Host::local();
    owner_only.grants = Grants::none()
        .grant("owner", crate::workflow::local_owner())
        .grant("recovery", crate::workflow::local_owner());
    assert!(
        supported(&Validated::new(base.clone()).unwrap(), &owner_only)
            .unwrap_err()
            .starts_with("unsupported edge rework: no authority of this automatic edge")
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
    // Nor an adapter's effect, which admission refuses every taker, the board among them.
    assert_eq!(
        supports(&|d| {
            d.adapters.push("default_land".into());
            let edge = rework(d);
            edge.operation = symphony_api::workflow::Operation::PrepareExternal;
            edge.effects.push(Effect::Prepare {
                adapter: "default_land".into(),
            });
        }),
        Err(
            "unsupported edge rework: an automatic edge prepares or finalizes an effect, which \
             only its adapter takes"
                .into()
        )
    );
    // Its bound is exhausted off every cycle through it: a pair it leaves or enters, or one the
    // task passes on a lap (build/working, by claim and submit, back to verify/checking), would
    // restart the count of its takes every lap.
    let exhausted_to = |phase: &'static str, state: &'static str| {
        move |d: &mut Definition| {
            let edge = rework(d);
            edge.to = pair("verify", "stuck");
            edge.retry.as_mut().unwrap().exhausted = pair(phase, state);
        }
    };
    let on_a_lap = Err(
        "unsupported edge rework: an automatic edge's retry is exhausted to a pair on a cycle \
         through it"
            .to_string(),
    );
    for (phase, state) in [("verify", "checking"), ("verify", "stuck")] {
        assert_eq!(supports(&exhausted_to(phase, state)), on_a_lap);
    }
    // build/working, manual by its cancellation_acknowledged, which every lap passes.
    assert_eq!(
        supports(&|d| rework(d).retry.as_mut().unwrap().exhausted = pair("build", "working")),
        on_a_lap
    );
    // verify/stuck reached on a lap by a recovery's target: a lost executor sent there.
    assert_eq!(
        supports(&|d| {
            let working = position_mut(d, &pair("build", "working"));
            working.recovery.as_mut().unwrap().lost_holder.target = Some(pair("verify", "stuck"));
        }),
        on_a_lap
    );
    // The failure action of the edge's own source is the bound's to replace: no lap passes it.
    assert_eq!(
        supports(&|d| {
            let checking = position_mut(d, &pair("verify", "checking"));
            checking.recovery.as_mut().unwrap().attempt_failure.target =
                Some(pair("verify", "stuck"));
        }),
        Ok(())
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
            "unsupported edge again: another automatic edge leaves its pair, and the board could \
             not choose which to take"
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
            "unsupported edge unstick: a retry bound on an edge a caller takes: the board counts \
             its own takes"
                .into()
        )
    );
}

#[test]
fn a_live_attempt_keeps_its_task_from_moving_until_it_ends_or_is_cancelled() {
    let two = custom(&format!("{TWO}{MAPPED}"));
    let mut working = standing("sim/working", "in_progress");
    // An attempt a cancel was asked of is not read: its result no longer counts.
    working.attempts = vec!["bb22".into()];
    assert_eq!(
        planned(&two, None, &[working.clone()]),
        Err(
            "sim/working runs execution attempt bb22, and activation would change its position \
             or its staffing; an attempt drains first (let it end, or cancel it), and is never \
             rebound to another position"
                .into()
        )
    );
    // Drained or cancelled, nothing holds the task, and a cancelled attempt keeps its ending.
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
    working.attempts = vec!["bb22".into()];
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
             work finish), as it may be no attempt the board sees"
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
        authorities: Vec::new(),
        project: None,
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
        authorities: Vec::new(),
        project: None,
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
        authorities: Vec::new(),
        project: None,
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
fn an_activation_carries_an_accepted_output_supersedes_it_where_none_stands_and_accepts_none() {
    let old = custom(&format!(
        "{}{MAPPED}",
        crate::workflow::admit::tests::outputs()
    ));
    let mut approved = standing("sim/approved", "done");
    approved.position = Some(TaskPosition {
        phase: "verify".into(),
        state: "approved".into(),
        resume: None,
        generation: 1,
        revision: 4,
        digest: old.revision.digest.clone(),
        manual: None,
    });
    approved.accepted = Some("sim/approved@4".into());
    let replanned = |definition: Definition, mappings: Vec<Mapping>, task: &Standing| {
        let new = Custom {
            definition: Validated::new(definition).unwrap(),
            revision: old.revision.clone(),
            mappings,
            authorities: Vec::new(),
            project: None,
        };
        plan(
            &new,
            Some(old.definition.definition()),
            std::slice::from_ref(task),
            2,
            &by(),
            60.0,
        )
    };
    let only = |plan: Plan| {
        assert!(plan.kept.is_empty(), "{plan:?}");
        assert_eq!(plan.moved.len(), 1);
        plan.moved.into_iter().next().unwrap().transition
    };
    let definition = old.definition.definition().clone();

    // The same pair, satisfying dependants still: kept, its output standing.
    let again = replanned(definition.clone(), Vec::new(), &approved).unwrap();
    assert_eq!(again.kept, vec![("sim/approved".to_string(), 3)]);

    // Mapped to another pair that satisfies them: the output is carried there.
    let signed = pair("verify", "signed");
    let mut renamed = definition.clone();
    renamed.states.push(symphony_api::workflow::State {
        id: "signed".into(),
        label: "Signed".into(),
    });
    for position in &mut renamed.positions {
        if position.at == pair("verify", "approved") {
            position.at = signed.clone();
        }
    }
    for edge in &mut renamed.edges {
        for end in [&mut edge.from, &mut edge.to] {
            if *end == pair("verify", "approved") {
                *end = signed.clone();
            }
        }
    }
    renamed.states.retain(|s| s.id != "approved");
    let mapped = vec![Mapping {
        from: pair("verify", "approved"),
        to: signed.clone(),
    }];
    let carried = only(replanned(renamed, mapped, &approved).unwrap());
    assert_eq!(
        (carried.to, carried.accepted.as_deref()),
        (signed, Some("sim/approved@4"))
    );

    // The pair no longer satisfies dependants: a transition that moves nothing supersedes the
    // output, and a live holder, which nothing moves or restaffs, does not stop it.
    let mut unsatisfied = definition.clone();
    position_mut(&mut unsatisfied, &pair("verify", "approved")).satisfies_dependencies = false;
    let mut held = approved.clone();
    held.roles = vec![("amber".into(), "executor".into(), true)];
    let superseded = only(replanned(unsatisfied.clone(), Vec::new(), &held).unwrap());
    assert_eq!(superseded.from.as_ref(), Some(&superseded.to));
    assert_eq!(
        (superseded.status.as_str(), superseded.accepted),
        ("done", None)
    );
    // That transition leaves the task at another revision, where a live attempt's ending, pinned
    // to the one before, would answer nothing: it drains first, or is cancelled.
    let mut running = held.clone();
    running.attempts = vec!["bb22".into()];
    assert_eq!(
        replanned(unsatisfied.clone(), Vec::new(), &running),
        Err(
            "sim/approved runs execution attempt bb22, and activation would change its position \
             or its staffing; an attempt drains first (let it end, or cancel it), and is never \
             rebound to another position"
                .into()
        )
    );
    running.attempts.clear();
    assert_eq!(
        only(replanned(unsatisfied, Vec::new(), &running).unwrap()).accepted,
        None
    );

    // A pair kept that comes to satisfy dependants is refused, as ever: a mapping accepts nothing.
    let mut awaiting = standing("sim/awaiting", "review");
    awaiting.position = Some(TaskPosition {
        state: "awaiting".into(),
        ..approved.position.clone().unwrap()
    });
    awaiting.accepted = Some("sim/awaiting@2".into());
    let mut satisfying = definition;
    position_mut(&mut satisfying, &pair("verify", "awaiting")).satisfies_dependencies = true;
    assert!(
        replanned(satisfying, Vec::new(), &awaiting)
            .unwrap_err()
            .contains("its dependants satisfied")
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
        retired: 0,
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
            accepted: None,
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

/// TWO's pairs mapped back onto built-in v1.
pub(crate) const BACK: &str = r#"
[[workflow_mappings]]
from = { phase = "build", state = "available" }
to = { phase = "ready", state = "available" }
[[workflow_mappings]]
from = { phase = "build", state = "working" }
to = { phase = "in_progress", state = "working" }
[[workflow_mappings]]
from = { phase = "build", state = "blocked" }
to = { phase = "blocked", state = "waiting" }
[[workflow_mappings]]
from = { phase = "verify", state = "awaiting" }
to = { phase = "review", state = "awaiting_review" }
[[workflow_mappings]]
from = { phase = "verify", state = "accepted" }
to = { phase = "done", state = "completed" }
"#;

fn back() -> Vec<Mapping> {
    written(BACK).unwrap().1
}

/// A task TWO holds at `phase`/`state`, at workflow revision 2 of generation 1.
fn held_at(slug: &str, status: &str, phase: &str, state: &str) -> Standing {
    Standing {
        position: Some(TaskPosition {
            phase: phase.into(),
            state: state.into(),
            resume: None,
            generation: 1,
            revision: 2,
            digest: "d".repeat(64),
            manual: None,
        }),
        ..standing(slug, status)
    }
}

fn returned(mappings: &[Mapping], tasks: &[Standing]) -> Result<Plan, String> {
    let two = custom(TWO);
    returning(two.definition.definition(), mappings, tasks, 2, &by(), 60.0)
}

#[test]
fn a_return_sends_each_held_task_to_its_mapped_built_in_pair_and_status() {
    let mut blocked = held_at("sim/blocked", "blocked", "build", "blocked");
    blocked.position.as_mut().unwrap().resume = Some(pair("build", "working"));
    let mut reviewed = held_at("sim/reviewed", "review", "verify", "awaiting");
    reviewed.roles = vec![
        ("amber".into(), "executor".into(), true),
        ("birch".into(), "reviewer".into(), true),
    ];
    let tasks = [
        held_at("sim/ready", "ready", "build", "available"),
        held_at("sim/working", "in_progress", "build", "working"),
        blocked,
        reviewed,
        held_at("sim/accepted", "done", "verify", "accepted"),
        // An open task with no position is the board's writers' already: only fenced.
        standing("sim/stray", "ready"),
    ];
    let plan = returned(&back(), &tasks).unwrap();
    assert_eq!(plan.kept, vec![("sim/stray".to_string(), 3)]);
    let moved: Vec<(&str, Pair, Option<Pair>, &str)> = (plan.moved.iter())
        .map(|m| {
            let t = &m.transition;
            (
                t.slug.as_str(),
                t.to.clone(),
                t.resume.clone(),
                t.status.as_str(),
            )
        })
        .collect();
    assert_eq!(
        moved,
        vec![
            ("sim/ready", pair("ready", "available"), None, "ready"),
            (
                "sim/working",
                pair("in_progress", "working"),
                None,
                "in_progress"
            ),
            (
                "sim/blocked",
                pair("blocked", "waiting"),
                Some(pair("in_progress", "working")),
                "blocked"
            ),
            // Its reviewer holds it: the board reads it at built-in v1's held review.
            ("sim/reviewed", pair("review", "held"), None, "review"),
            ("sim/accepted", pair("done", "completed"), None, "done"),
        ]
    );
    let first = &plan.moved[0];
    assert_eq!((first.version, first.expected), (3, 2));
    let t = &first.transition;
    assert_eq!((t.id.as_str(), t.revision), ("sim/ready@3", 3));
    assert_eq!(t.from, Some(pair("build", "available")));
    assert_eq!((t.generation, t.activation), (2, Some(2)));
    assert_eq!(t.digest, crate::workflow::DEFAULT);
    assert_eq!((t.edge.as_deref(), t.at), (None, 60.0));
}

#[test]
fn a_return_keeps_a_pair_built_in_v1_declares_and_drops_a_resume_pair_off_a_block() {
    // A definition that kept v1's ready/available needs no mapping for it.
    let mut definition = custom(TWO).definition.definition().clone();
    let mut kept = definition.positions[0].clone();
    kept.at = pair("ready", "available");
    definition.positions.push(kept);
    let mut ready = held_at("sim/ready", "ready", "ready", "available");
    // A resume pair on a task that does not return blocked is dropped, unmapped or not.
    ready.position.as_mut().unwrap().resume = Some(pair("nowhere", "known"));
    let plan = returning(&definition, &[], &[ready], 2, &by(), 60.0).unwrap();
    let t = &plan.moved[0].transition;
    assert_eq!(
        (t.to.clone(), t.resume.clone()),
        (pair("ready", "available"), None)
    );
}

#[test]
fn a_return_is_refused_whole_on_a_pair_or_resume_pair_nothing_maps() {
    assert_eq!(
        returned(&[], &[held_at("sim/ready", "ready", "build", "available")]),
        Err(
            "workflow_mappings: sim/ready is at build/available, which the workflow no longer \
             declares; map it to a pair it declares"
                .into()
        )
    );
    let mut mappings = back();
    mappings.retain(|m| m.from != pair("build", "working"));
    let mut blocked = held_at("sim/blocked", "blocked", "build", "blocked");
    blocked.position.as_mut().unwrap().resume = Some(pair("build", "working"));
    assert_eq!(
        returned(
            &mappings,
            &[held_at("sim/a", "ready", "build", "available"), blocked]
        ),
        Err(
            "workflow_mappings: sim/blocked resumes to build/working, which the workflow no \
             longer declares; map it to a pair it declares"
                .into()
        )
    );
    // A position its own workflow does not declare is the board's fault, and said.
    assert_eq!(
        returned(&back(), &[held_at("sim/lost", "ready", "deploy", "queued")]),
        Err("sim/lost is at deploy/queued, which its workflow does not declare".into())
    );
}

#[test]
fn a_return_never_fabricates_an_outcome_satisfaction_or_completion() {
    let onto = |from: Pair, to: Pair| {
        let mut mappings = back();
        mappings.retain(|m| m.from != from);
        mappings.push(Mapping { from, to });
        mappings
    };
    let working = [held_at("sim/working", "in_progress", "build", "working")];
    assert_eq!(
        returned(
            &onto(pair("build", "working"), pair("done", "completed")),
            &working
        ),
        Err(
            "sim/working would stand at done/completed with the outcome Success that \
             build/working does not record; a mapping never fabricates one"
                .into()
        )
    );
    assert!(
        returned(
            &onto(pair("build", "working"), pair("done", "approved")),
            &working
        )
        .unwrap_err()
        .contains("with its dependants satisfied that build/working does not record")
    );
    assert!(
        returned(
            &onto(pair("build", "working"), pair("cancelled", "cancelled")),
            &working
        )
        .unwrap_err()
        .contains("with the outcome Cancellation")
    );
    // What the task has it keeps: accepted work returns as approved or completed.
    let accepted = held_at("sim/accepted", "done", "verify", "accepted");
    let plan = returned(
        &onto(pair("verify", "accepted"), pair("done", "approved")),
        &[accepted],
    )
    .unwrap();
    assert_eq!(plan.moved[0].transition.to, pair("done", "approved"));
}

#[test]
fn a_return_is_refused_for_a_live_holder_the_boards_writers_would_not_keep() {
    let mut ready = held_at("sim/ready", "ready", "build", "available");
    ready.roles = vec![
        ("idle".into(), "reserved".into(), true),
        ("olive".into(), "orchestrator".into(), true),
        ("amber".into(), "executor".into(), true),
    ];
    assert_eq!(
        returned(&back(), &[ready.clone()]),
        Err(
            "sim/ready is held by amber as its executor, and it would return to ready, where \
             the board's own writers keep no executor; a live holder stands in for a live \
             attempt, which drains first (release it, or let its work finish)"
                .into()
        )
    );
    // A holder that is lost holds nothing, and its row is the board's writers' to recover.
    ready.roles[2].2 = false;
    assert!(returned(&back(), &[ready]).is_ok());
    // A live executor returns with its work, a live reviewer with its review.
    let mut working = held_at("sim/working", "in_progress", "build", "working");
    working.roles = vec![("amber".into(), "executor".into(), true)];
    assert!(returned(&back(), &[working.clone()]).is_ok());
    working.roles = vec![("birch".into(), "reviewer".into(), true)];
    assert!(
        returned(&back(), &[working])
            .unwrap_err()
            .starts_with("sim/working is held by birch as its reviewer")
    );
    // A blocked task keeps the holder the status it resumes to keeps.
    let mut blocked = held_at("sim/blocked", "blocked", "build", "blocked");
    blocked.roles = vec![("amber".into(), "executor".into(), true)];
    assert!(returned(&back(), &[blocked.clone()]).is_ok());
    blocked.position.as_mut().unwrap().resume = Some(pair("build", "available"));
    assert!(
        returned(&back(), &[blocked])
            .unwrap_err()
            .contains("it would return to blocked, where the board's own writers keep no executor")
    );
}

#[test]
fn the_boards_writers_keep_an_executor_through_review_and_a_reviewer_in_review() {
    for (status, executor, reviewer) in [
        ("backlog", false, false),
        ("concept", true, false),
        ("design", true, false),
        ("ready", false, false),
        ("in_progress", true, false),
        ("review", true, true),
        ("done", false, false),
        ("cancelled", false, false),
    ] {
        assert_eq!(keeps(status, None, "executor"), executor, "{status}");
        assert_eq!(keeps(status, None, "reviewer"), reviewer, "{status}");
        assert!(keeps(status, None, "orchestrator") && keeps(status, None, "reserved"));
        assert_eq!(
            keeps("blocked", Some(status), "executor"),
            executor,
            "{status}"
        );
    }
    assert!(keeps("blocked", None, "executor") && keeps("blocked", None, "reviewer"));
}

#[test]
fn a_task_a_return_retired_continues_its_workflow_revisions() {
    let two = custom(&format!("{TWO}{MAPPED}"));
    let mut back_again = standing("sim/ready", "ready");
    back_again.retired = 3;
    let plan = planned(&two, None, &[back_again]).unwrap();
    let moved = &plan.moved[0];
    // It has no position, so its write fences on none; its revision is the next all the same.
    assert_eq!((moved.expected, moved.transition.revision), (0, 4));
    assert_eq!(moved.transition.id, "sim/ready@4");

    let roles = Vec::new();
    let task = Reopening {
        slug: "sim/a",
        left: "done",
        status: "review",
        roles: &roles,
        retired: 3,
    };
    let placed = placing(&[]).reopened(&task, &by(), 7.0).unwrap();
    assert_eq!((placed.id.as_str(), placed.revision), ("sim/a@4", 4));
}

/// TWO with its submit edge guarded by `approved`: verify/awaiting is a pair only that guard
/// lets a task into, and every open status mapped.
fn guarded_review() -> Custom {
    let attested = crate::workflow::guard::tests::guarded();
    let mut definition = attested.definition.definition().clone();
    for edge in definition.edges.iter_mut() {
        edge.guards = match edge.id.as_str() {
            "submit" => vec!["approved".into()],
            _ => Vec::new(),
        };
    }
    Custom {
        definition: Validated::new(definition).unwrap(),
        ..custom(&format!(
            "{}{MAPPED}",
            crate::workflow::guard::tests::guarded_text(7)
        ))
    }
}

#[test]
fn an_activation_never_places_or_maps_a_task_past_a_guard() {
    let guarded = guarded_review();
    let refused = planned(&guarded, None, &[standing("sim/review", "review")]).unwrap_err();
    assert_eq!(
        refused,
        "sim/review would stand at verify/awaiting, which only edges guarded by approved enter; \
         activation never takes a task past a guard: map it to a pair before it"
    );
    // A task elsewhere is placed as before.
    assert!(planned(&guarded, None, &[standing("sim/ready", "ready")]).is_ok());
    // A blocked task that would resume past the guard is refused as well.
    let mut definition = guarded.definition.definition().clone();
    for edge in definition.edges.iter_mut() {
        edge.guards = match edge.id.as_str() {
            "claim" => vec!["approved".into()],
            _ => Vec::new(),
        };
    }
    let resuming = Custom {
        definition: Validated::new(definition).unwrap(),
        ..guarded.clone()
    };
    let mut blocked = standing("sim/blocked", "blocked");
    blocked.blocked_from = Some("in_progress".into());
    let refused = planned(&resuming, None, &[blocked]).unwrap_err();
    assert!(
        refused.contains("would resume to build/working"),
        "{refused}"
    );
}

#[test]
fn a_task_already_past_a_guard_stays_there_only_under_the_same_guards() {
    let guarded = guarded_review();
    let at_awaiting = |digest: &str| TaskPosition {
        phase: "verify".into(),
        state: "awaiting".into(),
        resume: None,
        generation: 1,
        revision: 2,
        digest: digest.into(),
        manual: None,
    };
    let mut verify = standing("sim/verify", "review");
    verify.position = Some(at_awaiting(&guarded.revision.digest));
    // The same guards before and after: it stood there under them, and stays.
    let kept = plan(
        &guarded,
        Some(guarded.definition.definition()),
        std::slice::from_ref(&verify),
        2,
        &by(),
        60.0,
    )
    .unwrap();
    assert_eq!(kept.kept, vec![("sim/verify".to_string(), 3)]);
    // Under a previous workflow that did not guard it, it never met the guard: refused.
    let unguarded = custom(&format!("{TWO}{MAPPED}"));
    let refused = plan(
        &guarded,
        Some(unguarded.definition.definition()),
        &[verify],
        2,
        &by(),
        60.0,
    )
    .unwrap_err();
    assert!(
        refused.contains("would stand at verify/awaiting"),
        "{refused}"
    );
}

#[test]
fn a_reopened_task_is_never_placed_past_a_guard() {
    let guarded = guarded_review();
    let placing = Placing {
        definition: guarded.definition.definition().clone(),
        mappings: guarded.mappings.clone(),
        generation: 4,
        digest: "d".repeat(64),
    };
    let refused = reopened(&placing, "review", &[]).unwrap_err();
    assert!(
        refused.starts_with(
            "sim/a would reopen at verify/awaiting, which only edges guarded by approved enter"
        ),
        "{refused}"
    );
    assert!(reopened(&placing, "ready", &[]).is_ok());
}

#[test]
fn the_board_takes_no_guarded_edge_and_no_exhausted_retry_bound_passes_a_guard() {
    use crate::workflow::guard::tests::guarding_edge;
    use crate::workflow::recover::attempt_tests::attempts;
    let attesters = [crate::workflow::guard::tests::attester(7)];
    // A guard on the automatic edge: the board brings no attestation to meet it.
    let automatic_guarded = custom(&guarding_edge(&attempts(false), "rework"));
    let d = automatic_guarded.definition.definition();
    let rework = (d.edges.iter()).find(|e| e.id == "rework").unwrap();
    assert_eq!(
        automatic(d, rework, &Host::local()),
        Some("an automatic edge is guarded, and the board brings no attestation to meet its guard")
    );
    let definition = &automatic_guarded.definition;
    assert!(supported_with(definition, &Host::local(), &attesters).is_err());
    // A retry bound exhausted to a pair only a guarded edge enters reaches it unguarded: the board
    // sends the task there as a recovery, no edge taken. Here a person escalates a check to
    // verify/stuck under the guard, where the bound also sends it.
    let unstick = "  { id = \"unstick\",";
    let escalate = "  { id = \"escalate\", label = \"Escalate\", from = { phase = \"verify\", state = \
         \"checking\" }, to = { phase = \"verify\", state = \"stuck\" }, operation = \"advance\", \
         authority = [{ kind = \"project_capability\", capability = \"owner\" }], inputs = [], \
         dependencies = \"none\", guards = [], effects = [{ kind = \"slot\", policy = \"release\" }], \
         automatic = false, human_wait = false },\n";
    let escalating = attempts(false).replacen(unstick, &format!("{escalate}{unstick}"), 1);
    assert_ne!(escalating, attempts(false), "the escalate edge is added");
    let exhausting = custom(&guarding_edge(&escalating, "escalate"));
    assert_eq!(
        crate::workflow::guard::coverage(exhausting.definition.definition()),
        Err(
            "unguarded route: the retry bound of edge rework sends a task to verify/stuck once \
             exhausted, which edge escalate enters under approved"
                .into()
        )
    );
    // Exhausted to where it stood, past no guard, the bound passes.
    let staying = custom(&guarding_edge(&attempts(false), "accept"));
    assert_eq!(
        crate::workflow::guard::coverage(staying.definition.definition()),
        Ok(())
    );
}
