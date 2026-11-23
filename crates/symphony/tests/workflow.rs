//! Candidate syntax, graph invariants and legacy parity; no database or activation fixtures.

use std::path::Path;
use symphony::project::{Project, workflow::*};
use symphony_api::workflow::*;
#[path = "common/layouts.rs"]
mod layouts;
#[path = "common/tree.rs"]
mod tree;

fn source() -> Source {
    Source {
        commit: "a".repeat(40),
        project_blob: "b".repeat(40),
    }
}

fn invalid(mut change: impl FnMut(&mut Definition), expected: &str) {
    let mut d = builtin();
    change(&mut d);
    let error = Validated::new(d).unwrap_err();
    assert!(
        error.contains(expected),
        "expected {expected:?}, got {error:?}"
    );
}

fn pair(phase: &str, state: &str) -> Pair {
    Pair {
        phase: phase.into(),
        state: state.into(),
    }
}

#[test]
fn default_is_valid_and_omission_preserves_project_and_definition() {
    let default = Validated::new(builtin()).unwrap();
    assert_eq!(candidate("").unwrap(), default);
    assert_eq!(candidate("[project]\nname = 'demo'\n").unwrap(), default);
    assert_eq!(
        Project::parse(Path::new("/tmp/demo"), "").unwrap(),
        Project::plain(Path::new("/tmp/demo"))
    );
    assert_eq!(DEFAULT_VERSION, 1);
    let encoded = serde_json::to_string(default.definition()).unwrap();
    assert_eq!(
        Validated::new(serde_json::from_str(&encoded).unwrap()).unwrap(),
        default
    );
    let d = default.definition();
    assert_eq!(d.initial, pair("ready", "available"));
    let mut columns: Vec<_> = d.phases.iter().filter(|p| p.column).collect();
    columns.sort_by_key(|p| p.order);
    assert_eq!(
        columns.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        symphony_board::PIPELINE
            .into_iter()
            .filter(|p| *p != "backlog")
            .collect::<Vec<_>>()
    );
    let mut phone: Vec<_> = d
        .phases
        .iter()
        .filter(|p| !["blocked", "cancelled"].contains(&p.id.as_str()))
        .collect();
    phone.sort_by_key(|p| p.phone_order);
    let current_phone = include_str!("../../../apps/symphony-web/src/boardwait.tsx")
        .split_once("export const PHONE_STAGES = [")
        .unwrap()
        .1
        .split_once(']')
        .unwrap()
        .0
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix('"')?
                .split_once('"')
                .map(|(value, _)| value)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        phone.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        current_phone
    );
    assert!(
        d.positions
            .iter()
            .any(|p| p.at == pair("done", "approved") && !p.terminal && p.outcome.is_none())
    );
    assert!(d.positions.iter().any(|p| p.at == pair("done", "completed")
        && p.terminal
        && p.outcome == Some(Outcome::Success)));
}

#[test]
fn default_move_routes_match_every_legacy_pair() {
    let d = builtin();
    for from in symphony_board::STATUSES {
        for to in symphony_board::STATUSES {
            let route = d.edges.iter().find(|e| e.id == format!("{from}_to_{to}"));
            assert_eq!(
                route.is_some(),
                symphony_board::can_transition(from, to),
                "{from} -> {to}"
            );
            if let Some(route) = route {
                assert_eq!(route.from.phase, from);
                assert_eq!(route.to.phase, to);
                assert_eq!(route.guards, ["legacy_board"]);
            }
        }
    }
    for op in [
        Operation::Claim,
        Operation::Release,
        Operation::Block,
        Operation::Resume,
        Operation::RequestReview,
        Operation::Approve,
        Operation::Changes,
        Operation::CompleteSubtask,
        Operation::PrepareLand,
        Operation::ConfirmLand,
    ] {
        assert!(d.edges.iter().any(|e| e.operation == op), "{op:?}");
    }
    let work = d
        .positions
        .iter()
        .find(|p| p.at.phase == "in_progress")
        .unwrap();
    assert_eq!(
        work.recovery.as_ref().unwrap().lost_holder.target,
        Some(pair("ready", "available"))
    );
    let hold = d.edges.iter().find(|e| e.id == "hold_review").unwrap();
    assert!(hold.effects.contains(&Effect::Slot {
        policy: Slot::Retain
    }));
    assert!(hold.effects.contains(&Effect::Block { save_resume: true }));
    let movement = d
        .edges
        .iter()
        .find(|e| e.id == "ready_to_in_progress")
        .unwrap();
    assert_eq!(movement.operation, Operation::Advance);
    assert!(
        !movement
            .effects
            .iter()
            .any(|e| matches!(e, Effect::AcquireRole { .. }))
    );
    assert!(movement.effects.contains(&Effect::Slot {
        policy: Slot::Acquire
    }));
    let review = d
        .edges
        .iter()
        .find(|e| e.id == "in_progress_to_review")
        .unwrap();
    assert!(
        !review
            .effects
            .iter()
            .any(|e| matches!(e, Effect::ReleaseRole { .. }))
    );
    let block = d
        .edges
        .iter()
        .find(|e| e.id == "review_to_blocked")
        .unwrap();
    assert!(block.effects.contains(&Effect::Slot {
        policy: Slot::Release
    }));
}

#[test]
fn default_covered_close_routes_match_every_legacy_position() {
    let d = builtin();
    for position in &d.positions {
        let legacy = serde_json::to_value(position.legacy).unwrap();
        let open = !symphony_board::health::FINISHED.contains(&legacy.as_str().unwrap());
        let routes: Vec<_> = d
            .edges
            .iter()
            .filter(|edge| edge.from == position.at && edge.operation == Operation::CloseCovered)
            .collect();
        assert_eq!(routes.len(), usize::from(open), "{:?}", position.at);
        for edge in routes {
            assert_eq!(edge.to, pair("done", "completed"));
            assert_eq!(edge.guards, ["legacy_board"]);
            assert_eq!(
                edge.authority,
                [Authority::ProjectCapability {
                    capability: "legacy_actor".into()
                }]
            );
            assert!(edge.effects.contains(&Effect::Outcome {
                outcome: Outcome::Success,
                satisfies_dependencies: true
            }));
            assert!(edge.effects.contains(&Effect::Slot {
                policy: Slot::Release
            }));
        }
    }
}

#[test]
fn strict_ids_duplicates_and_pairs() {
    for id in ["", "Build", "1build", "à", "a.b", "a/b", "a b"] {
        invalid(|d| d.phases[0].id = id.into(), "invalid workflow ID");
    }
    invalid(|d| d.phases.push(d.phases[0].clone()), "duplicate phase");
    invalid(|d| d.states.push(d.states[0].clone()), "duplicate state");
    invalid(|d| d.edges.push(d.edges[0].clone()), "duplicate edge");
    invalid(
        |d| d.positions.push(d.positions[0].clone()),
        "duplicate position",
    );
    invalid(
        |d| d.positions[0].at.phase = "unknown".into(),
        "unknown phase",
    );
    invalid(
        |d| d.positions[0].at.state = "unknown".into(),
        "unknown state",
    );
    invalid(
        |d| d.initial = pair("ready", "working"),
        "invalid phase/state pair",
    );
    invalid(
        |d| d.edges[0].to = pair("ready", "working"),
        "invalid phase/state pair",
    );
    invalid(|d| d.format = FORMAT + 1, "unsupported workflow format");
    invalid(|d| d.task_kinds.clear(), "requires task kinds");
    invalid(|d| d.phases[0].label = " ".into(), "phase labels");
    invalid(
        |d| d.phases[0].order = d.phases[1].order,
        "unique display/phone order",
    );
    invalid(
        |d| d.phases[0].phone_order = d.phases[1].phone_order,
        "unique display/phone order",
    );
    invalid(|d| d.states[0].label.clear(), "state label");
}

#[test]
fn ambiguous_aliases_and_routes_are_refused() {
    invalid(
        |d| {
            let mut e = d.edges[0].clone();
            e.id = "other".into();
            d.edges.push(e);
        },
        "ambiguous legacy alias",
    );
    invalid(
        |d| {
            let mut e = d.edges[0].clone();
            e.id = "other".into();
            e.alias = None;
            d.edges[0].alias = None;
            d.edges.push(e);
        },
        "ambiguous operation route",
    );
    invalid(
        |d| d.edges[0].alias = Some("Move".into()),
        "invalid workflow ID",
    );
    invalid(
        |d| d.edges[0].label.clear(),
        "requires a label and authority",
    );
    invalid(
        |d| d.edges[0].authority.clear(),
        "requires a label and authority",
    );
}

#[test]
fn capabilities_guards_and_effects_cannot_supply_authority() {
    invalid(
        |d| {
            let a = d.edges[0].authority[0].clone();
            d.edges[0].authority.push(a);
        },
        "duplicate authority",
    );
    invalid(
        |d| {
            d.edges[0].authority = vec![Authority::ProjectCapability {
                capability: "owner".into(),
            }]
        },
        "unknown capability",
    );
    invalid(
        |d| {
            d.edges[0].authority = vec![Authority::SystemCapability {
                capability: "reaper".into(),
            }]
        },
        "unknown capability",
    );
    invalid(
        |d| d.guards[0].adapter = "missing".into(),
        "unknown guard adapter",
    );
    invalid(
        |d| d.edges[0].guards.push("missing".into()),
        "unknown guard",
    );
    invalid(
        |d| d.edges[0].effects.push(Effect::Review { approved: true }),
        "cannot impersonate effect",
    );
    invalid(
        |d| {
            d.edges
                .iter_mut()
                .find(|e| e.operation == Operation::Approve)
                .unwrap()
                .authority = vec![Authority::ProjectCapability {
                capability: "legacy_actor".into(),
            }]
        },
        "independent task reviewer",
    );
    invalid(
        |d| {
            d.edges
                .iter_mut()
                .find(|e| e.operation == Operation::Claim)
                .unwrap()
                .effects
                .retain(|e| !matches!(e, Effect::AcquireRole { .. }))
        },
        "missing operation effect",
    );
    invalid(|d| d.edges[0].effects.clear(), "explicit slot effect");
    invalid(
        |d| {
            d.edges
                .iter_mut()
                .find(|e| e.operation == Operation::PrepareLand)
                .unwrap()
                .effects = vec![Effect::Prepare {
                adapter: "missing".into(),
            }]
        },
        "unknown effect adapter",
    );
    let d = Validated::new(builtin()).unwrap();
    assert!(
        prerequisites(&d, &[], &["legacy_board"])
            .unwrap_err()
            .contains("capability")
    );
    assert!(
        prerequisites(&d, &["legacy_actor"], &[])
            .unwrap_err()
            .contains("adapter")
    );
    prerequisites(&d, &["legacy_actor"], &["legacy_board"]).unwrap();
}

#[test]
fn all_operation_effect_requirements_are_checked() {
    let mut external = builtin();
    for e in &mut external.edges {
        e.operation = match e.operation {
            Operation::PrepareLand => Operation::PrepareExternal,
            Operation::ConfirmLand => Operation::ConfirmExternal,
            other => other,
        };
    }
    Validated::new(external).unwrap();
    for op in [
        Operation::Claim,
        Operation::Release,
        Operation::Approve,
        Operation::Changes,
        Operation::Block,
        Operation::Resume,
        Operation::Cancel,
        Operation::CompleteSubtask,
        Operation::CloseCovered,
        Operation::PrepareLand,
        Operation::ConfirmLand,
    ] {
        invalid(
            |d| {
                d.edges
                    .iter_mut()
                    .find(|e| e.operation == op)
                    .unwrap()
                    .effects = vec![Effect::Slot {
                    policy: Slot::Retain,
                }]
            },
            "missing operation effect",
        );
    }
    for effect in [
        Effect::ParentIntegration,
        Effect::Finalize {
            adapter: "legacy_board".into(),
        },
        Effect::Prepare {
            adapter: "legacy_board".into(),
        },
        Effect::Outcome {
            outcome: Outcome::Success,
            satisfies_dependencies: true,
        },
    ] {
        invalid(
            |d| d.edges[0].effects.push(effect.clone()),
            "cannot impersonate effect",
        );
    }
}

#[test]
fn terminal_routes_and_conflicting_effects_cannot_fabricate_outcomes() {
    for completion in [
        Completion::Landing,
        Completion::CoveredClose,
        Completion::ParentIntegration,
    ] {
        invalid(
            |d| {
                d.positions
                    .iter_mut()
                    .find(|p| p.at == pair("done", "completed"))
                    .unwrap()
                    .completion = completion
            },
            "completion requirement",
        );
    }
    invalid(
        |d| {
            let e = d
                .edges
                .iter_mut()
                .find(|e| e.id == "confirm_land_done")
                .unwrap();
            e.effects.retain(|e| !matches!(e, Effect::Outcome { .. }));
        },
        "terminal route requires matching outcome",
    );
    invalid(
        |d| {
            d.edges[0].effects.push(Effect::Slot {
                policy: Slot::Acquire,
            })
        },
        "exactly one explicit slot",
    );
    invalid(
        |d| {
            let e = d
                .edges
                .iter_mut()
                .find(|e| e.operation == Operation::Approve)
                .unwrap();
            e.effects.push(Effect::Review { approved: true });
        },
        "conflicting repeated effect",
    );
    invalid(
        |d| d.edges[0].effects.push(Effect::ClearBlock),
        "cannot impersonate effect",
    );
    invalid(
        |d| {
            let e = d
                .edges
                .iter_mut()
                .find(|e| e.operation == Operation::Cancel)
                .unwrap();
            e.effects.iter_mut().for_each(|e| {
                if let Effect::Outcome {
                    satisfies_dependencies,
                    ..
                } = e
                {
                    *satisfies_dependencies = true;
                }
            });
        },
        "cannot impersonate effect",
    );
}

#[test]
fn staffed_positions_and_recovery_are_explicit() {
    invalid(
        |d| {
            d.positions
                .iter_mut()
                .find(|p| p.staffing.is_some())
                .unwrap()
                .recovery = None
        },
        "missing holder recovery",
    );
    invalid(
        |d| {
            d.positions[0].recovery.as_mut().unwrap().lost_holder.target =
                Some(pair("missing", "working"))
        },
        "invalid phase/state pair",
    );
    invalid(
        |d| d.positions[0].recovery.as_mut().unwrap().release.manual = false,
        "stay requires explicit manual",
    );
    invalid(
        |d| {
            d.positions
                .iter_mut()
                .find(|p| p.terminal)
                .unwrap()
                .accepts_work = true
        },
        "terminal position",
    );
    invalid(
        |d| d.positions.iter_mut().find(|p| p.terminal).unwrap().outcome = None,
        "terminal position",
    );
    invalid(
        |d| d.initial = pair("cancelled", "cancelled"),
        "initial position is terminal",
    );
    invalid(
        |d| d.edges[0].from = pair("cancelled", "cancelled"),
        "ordinary edge from terminal",
    );
    invalid(
        |d| d.edges.retain(|e| e.to != pair("done", "completed")),
        "unreachable completion",
    );
    invalid(
        |d| {
            d.edges.retain(|e| e.from.phase != "backlog");
            d.positions
                .iter_mut()
                .find(|p| p.at.phase == "backlog")
                .unwrap()
                .recovery = None;
        },
        "no outcome or manual recovery",
    );
}

#[test]
fn all_recovery_events_require_typed_completion_routes() {
    let completed = pair("done", "completed");
    for completion in [
        Completion::None,
        Completion::Landing,
        Completion::CoveredClose,
        Completion::ParentIntegration,
    ] {
        let mut baseline = builtin();
        baseline
            .positions
            .iter_mut()
            .find(|p| p.at == completed)
            .unwrap()
            .completion = completion;
        baseline.edges.retain(|edge| {
            edge.to != completed
                || match completion {
                    Completion::None => true,
                    Completion::Landing => edge.operation == Operation::ConfirmLand,
                    Completion::CoveredClose => edge.operation == Operation::CloseCovered,
                    Completion::ParentIntegration => edge.operation == Operation::CompleteSubtask,
                }
        });
        Validated::new(baseline.clone()).unwrap();
        for event in [
            "release",
            "lost_holder",
            "attempt_failure",
            "cancellation_acknowledged",
        ] {
            for manual in [false, true] {
                let mut definition = baseline.clone();
                let recovery = definition
                    .positions
                    .iter_mut()
                    .find(|p| p.at == pair("in_progress", "working"))
                    .unwrap()
                    .recovery
                    .as_mut()
                    .unwrap();
                let action = match event {
                    "release" => &mut recovery.release,
                    "lost_holder" => &mut recovery.lost_holder,
                    "attempt_failure" => &mut recovery.attempt_failure,
                    "cancellation_acknowledged" => &mut recovery.cancellation_acknowledged,
                    _ => unreachable!(),
                };
                action.target = Some(completed.clone());
                action.manual = manual;
                let error = Validated::new(definition).unwrap_err();
                assert!(
                    error.contains("recovery target requires a typed completion edge"),
                    "{event}, {completion:?}, manual={manual}: {error}"
                );
            }
        }
    }
    // Nonterminal completion/dependency facts cannot be manufactured by recovery either.
    for fact in ["outcome", "dependencies", "completion"] {
        invalid(
            |d| {
                let ready = d
                    .positions
                    .iter_mut()
                    .find(|p| p.at == pair("ready", "available"))
                    .unwrap();
                match fact {
                    "outcome" => ready.outcome = Some(Outcome::Failure),
                    "dependencies" => ready.satisfies_dependencies = true,
                    "completion" => ready.completion = Completion::Landing,
                    _ => unreachable!(),
                }
            },
            "recovery target requires a typed completion edge",
        );
    }
}

#[test]
fn retry_exhaustion_cannot_bypass_typed_completion_with_a_manual_terminal() {
    let mut definition = builtin();
    let manual = definition
        .positions
        .iter()
        .find(|p| p.at == pair("blocked", "waiting"))
        .unwrap()
        .recovery
        .clone();
    definition
        .positions
        .iter_mut()
        .find(|p| p.at == pair("done", "completed"))
        .unwrap()
        .recovery = manual;
    // A manual stay in an already-completed position introduces no completion by itself.
    Validated::new(definition.clone()).unwrap();
    for event in [
        "release",
        "lost_holder",
        "attempt_failure",
        "cancellation_acknowledged",
    ] {
        let mut reopened = definition.clone();
        let recovery = reopened
            .positions
            .iter_mut()
            .find(|p| p.at == pair("done", "completed"))
            .unwrap()
            .recovery
            .as_mut()
            .unwrap();
        let action = match event {
            "release" => &mut recovery.release,
            "lost_holder" => &mut recovery.lost_holder,
            "attempt_failure" => &mut recovery.attempt_failure,
            "cancellation_acknowledged" => &mut recovery.cancellation_acknowledged,
            _ => unreachable!(),
        };
        action.target = Some(pair("ready", "available"));
        assert!(
            Validated::new(reopened)
                .unwrap_err()
                .contains("terminal position cannot have recovery routes"),
            "{event}"
        );
    }
    let edge = definition
        .edges
        .iter_mut()
        .find(|e| e.id == "prepare_land_in_progress")
        .unwrap();
    edge.automatic = true;
    edge.retry = Some(RetryBound {
        maximum: 1,
        exhausted: pair("done", "completed"),
    });
    assert!(
        Validated::new(definition)
            .unwrap_err()
            .contains("recovery target requires a typed completion edge")
    );
}

#[test]
fn deliberate_rework_is_allowed_automatic_cycles_need_bounded_manual_exit() {
    let mut d = builtin();
    let index = d
        .edges
        .iter()
        .position(|e| e.id == "prepare_land_in_progress")
        .unwrap();
    d.edges[index].automatic = true;
    assert!(
        Validated::new(d.clone())
            .unwrap_err()
            .contains("finite retry bound")
    );
    d.edges[index].retry = Some(RetryBound {
        maximum: 2,
        exhausted: pair("blocked", "waiting"),
    });
    Validated::new(d.clone()).unwrap();
    d.edges[index].retry.as_mut().unwrap().maximum = 0;
    assert!(
        Validated::new(d.clone())
            .unwrap_err()
            .contains("finite positive maximum")
    );
    d.edges[index].retry.as_mut().unwrap().maximum = 1;
    d.edges[index].retry.as_mut().unwrap().exhausted = pair("cancelled", "cancelled");
    assert!(
        Validated::new(d)
            .unwrap_err()
            .contains("manual exhaustion destination")
    );
    let mut d = builtin();
    for e in &mut d.edges {
        if ["concept_to_design", "design_to_concept"].contains(&e.id.as_str()) {
            e.automatic = true;
        }
    }
    assert!(
        Validated::new(d)
            .unwrap_err()
            .contains("finite retry bound")
    );
}

#[test]
fn human_wait_needs_host_person_capability_and_no_automatic_retry() {
    let mut d = builtin();
    d.edges[0].human_wait = true;
    Validated::new(d.clone()).unwrap();
    d.edges[0].authority = vec![Authority::TaskRole {
        role: Role::Executor,
    }];
    assert!(
        Validated::new(d.clone())
            .unwrap_err()
            .contains("human wait")
    );
    d.edges[0].authority = vec![Authority::SystemCapability {
        capability: "legacy_actor".into(),
    }];
    assert!(
        Validated::new(d.clone())
            .unwrap_err()
            .contains("human wait")
    );
    d.edges[0].authority = vec![Authority::ProjectCapability {
        capability: "legacy_actor".into(),
    }];
    d.edges[0].automatic = true;
    assert!(Validated::new(d).unwrap_err().contains("human wait"));
}

#[test]
fn human_wait_rejects_machine_alternatives_even_beside_project_capabilities() {
    for alternative in [
        Authority::TaskRole {
            role: Role::Executor,
        },
        Authority::SystemCapability {
            capability: "legacy_actor".into(),
        },
    ] {
        invalid(
            |d| {
                d.edges[0].human_wait = true;
                d.edges[0].authority.push(alternative.clone());
            },
            "human wait requires only project capability alternatives",
        );
    }
    let mut definition = builtin();
    definition.capabilities.push("other_person".into());
    definition.edges[0].human_wait = true;
    definition.edges[0]
        .authority
        .push(Authority::ProjectCapability {
            capability: "other_person".into(),
        });
    Validated::new(definition).unwrap();
}

#[test]
fn revision_normalizes_sets_and_fences_content_format_and_source() {
    let d = Validated::new(builtin()).unwrap();
    let revision = d.revision(source()).unwrap();
    d.check_revision(&revision).unwrap();
    let mut reordered = builtin();
    reordered.edges.reverse();
    reordered.positions.reverse();
    reordered.phases.reverse();
    reordered.states.reverse();
    assert_eq!(
        Validated::new(reordered)
            .unwrap()
            .revision(source())
            .unwrap(),
        revision
    );
    let mut changed = builtin();
    changed.phases[0].label.push('!');
    let changed = Validated::new(changed).unwrap();
    assert!(
        changed
            .check_revision(&revision)
            .unwrap_err()
            .contains("stale_workflow")
    );
    let mut changed = builtin();
    changed.phases.swap(0, 1);
    let a = changed.phases[0].order;
    changed.phases[0].order = changed.phases[1].order;
    changed.phases[1].order = a;
    assert_ne!(
        Validated::new(changed).unwrap().revision(source()).unwrap(),
        revision
    );
    let mut other = source();
    other.commit = "c".repeat(40);
    assert_ne!(d.revision(other).unwrap(), revision);
    let mut other = source();
    other.project_blob = "c".repeat(64);
    assert_ne!(d.revision(other).unwrap(), revision);
    let mut forged = revision.clone();
    forged.format += 1;
    assert!(d.check_revision(&forged).is_err());
    let mut forged = revision.clone();
    forged.source.project_blob = "invalid".into();
    assert!(
        d.check_revision(&forged)
            .unwrap_err()
            .contains("project_blob")
    );
    for bad in ["", "abc", &"A".repeat(40), &"g".repeat(64)] {
        let mut other = source();
        other.commit = bad.into();
        assert!(d.revision(other).is_err());
    }
}

#[test]
fn normalization_orders_requirements_but_preserves_effect_order() {
    let mut definition = builtin();
    definition.guards.push(Guard {
        id: "extra_guard".into(),
        adapter: "legacy_board".into(),
    });
    definition.capabilities.push("extra_capability".into());
    let claim = definition
        .edges
        .iter_mut()
        .find(|e| e.id == "claim_ready")
        .unwrap();
    claim.guards.push("extra_guard".into());
    claim.inputs = vec!["second".into(), "first".into()];
    claim.authority.extend([
        Authority::SystemCapability {
            capability: "extra_capability".into(),
        },
        Authority::TaskRole {
            role: Role::Executor,
        },
    ]);
    let expected = Validated::new(definition.clone())
        .unwrap()
        .revision(source())
        .unwrap();
    definition.guards.reverse();
    definition.capabilities.reverse();
    let claim = definition
        .edges
        .iter_mut()
        .find(|e| e.id == "claim_ready")
        .unwrap();
    claim.guards.reverse();
    claim.inputs.reverse();
    claim.authority.reverse();
    assert_eq!(
        Validated::new(definition.clone())
            .unwrap()
            .revision(source())
            .unwrap(),
        expected
    );
    definition
        .edges
        .iter_mut()
        .find(|e| e.id == "claim_ready")
        .unwrap()
        .effects
        .reverse();
    assert_ne!(
        Validated::new(definition)
            .unwrap()
            .revision(source())
            .unwrap(),
        expected
    );
}

const CUSTOM: &str = r#"
[workflow]
format = 1
initial = { phase = "build", state = "available" }
task_kinds = ["task"]
phases = [{ id = "build", label = "Build", order = 0, phone_order = 0, column = true }]
states = [{ id = "available", label = "Available" }, { id = "accepted", label = "Accepted" }]
capabilities = ["accept"]
adapters = []
guards = []
positions = [
  { at = { phase = "build", state = "available" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = true, terminal = false, legacy = "ready", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "build", state = "accepted" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = true, legacy = "done", outcome = "success", satisfies_dependencies = true, completion = "covered_close" }
]
edges = [{ id = "accept", label = "Accept", from = { phase = "build", state = "available" }, to = { phase = "build", state = "accepted" }, operation = "close_covered", authority = [{ kind = "project_capability", capability = "accept" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "outcome", outcome = "success", satisfies_dependencies = true }, { kind = "slot", policy = "release" }], automatic = false, human_wait = true }]
"#;

#[test]
fn candidate_is_strict_and_activation_takes_what_the_local_host_supports() {
    let d = candidate(CUSTOM).unwrap();
    assert_eq!(d.definition().phases[0].id, "build");
    assert_eq!(
        Project::parse(Path::new("/tmp/demo"), CUSTOM).unwrap_err(),
        "symphony.toml: unsupported_activation: unsupported capability: accept"
    );
    let owned = CUSTOM
        .replace("\"accept\"]", "\"owner\"]")
        .replace("capability = \"accept\"", "capability = \"owner\"");
    assert!(Project::parse(Path::new("/tmp/demo"), &owned).is_ok());
    for (from, to) in [
        ("format = 1", "format = 1\nunknown = true"),
        ("close_covered", "deploy_shell"),
        ("accepts_work", "accept_work"),
        ("project_capability", "owner"),
        ("slot = \"release\"", "slot = \"unlimited\""),
    ] {
        assert!(candidate(&CUSTOM.replace(from, to)).is_err(), "{to}");
    }
    assert!(candidate("[workflow]").is_err());
    assert!(
        candidate(&CUSTOM.replace("format = 1", "format = 2"))
            .unwrap_err()
            .contains("unsupported workflow format")
    );
    assert!(
        Project::parse(
            Path::new("/tmp/demo"),
            &CUSTOM.replace("format = 1", "format = 2")
        )
        .unwrap_err()
        .contains("unsupported workflow format")
    );
}

#[test]
fn project_of_reports_custom_workflow_instead_of_silently_ignoring_it() {
    let tree = tree::Tree::new();
    tree.write("symphony.toml", CUSTOM);
    Project::of(&tree.root);
    assert!(
        Project::problem(&tree.root)
            .unwrap()
            .contains("unsupported_activation")
    );
}

#[test]
fn committed_reader_ignores_dirty_and_staged_configuration() {
    let tree = tree::Tree::new();
    layouts::git(&tree.root, &["init", "-q"]);
    tree.write("symphony.toml", CUSTOM);
    layouts::git(&tree.root, &["add", "symphony.toml"]);
    layouts::git(&tree.root, &["commit", "-qm", "workflow fixture"]);
    let (definition, revision) = committed(&tree.root, "HEAD").unwrap();
    definition.check_revision(&revision).unwrap();
    tree.write("symphony.toml", "[workflow]\nformat = 999");
    layouts::git(&tree.root, &["add", "symphony.toml"]);
    assert_eq!(
        committed(&tree.root, "HEAD").unwrap(),
        (definition, revision.clone())
    );
    layouts::git(&tree.root, &["commit", "-qm", "invalid candidate"]);
    assert!(
        committed(&tree.root, "HEAD")
            .unwrap_err()
            .contains("workflow")
    );
    assert_eq!(
        committed(&tree.root, &revision.source.commit).unwrap().1,
        revision
    );
    assert!(committed(&tree.root, "missing_ref").is_err());
    assert!(committed(&tree.root, "HEAD:symphony.toml").is_err());
}

#[test]
fn committed_reader_refuses_missing_nonregular_and_non_utf8_project_files() {
    let tree = tree::Tree::new();
    assert!(committed(&tree.root, "HEAD").is_err());
    layouts::git(&tree.root, &["init", "-q"]);
    layouts::git(
        &tree.root,
        &["commit", "--allow-empty", "-qm", "empty fixture"],
    );
    assert!(
        committed(&tree.root, "HEAD")
            .unwrap_err()
            .contains("missing")
    );
    std::os::unix::fs::symlink("elsewhere", tree.path("symphony.toml")).unwrap();
    layouts::git(&tree.root, &["add", "symphony.toml"]);
    layouts::git(&tree.root, &["commit", "-qm", "linked fixture"]);
    assert!(
        committed(&tree.root, "HEAD")
            .unwrap_err()
            .contains("regular blob")
    );
    std::fs::remove_file(tree.path("symphony.toml")).unwrap();
    std::fs::write(tree.path("symphony.toml"), [0xff]).unwrap();
    layouts::git(&tree.root, &["add", "symphony.toml"]);
    layouts::git(&tree.root, &["commit", "-qm", "binary fixture"]);
    assert!(committed(&tree.root, "HEAD").unwrap_err().contains("utf-8"));
}

#[test]
fn authority_set_collection_preserves_variants_full_results_and_refusal_precedence() {
    use std::collections::BTreeSet;
    let mut base = Validated::new(builtin()).unwrap().definition().clone();
    let capability = "fixture_authority";
    assert!(!base.capabilities.iter().any(|name| name == capability));
    base.capabilities.push(capability.into());
    base.capabilities.sort();
    let index = base
        .edges
        .iter()
        .position(|edge| edge.operation == Operation::Claim && !edge.human_wait)
        .unwrap();
    let edge_id = base.edges[index].id.clone();
    let unique = vec![
        Authority::TaskRole {
            role: Role::Executor,
        },
        Authority::TaskRole {
            role: Role::Reviewer,
        },
        Authority::TaskRole {
            role: Role::Orchestrator,
        },
        Authority::ProjectCapability {
            capability: capability.into(),
        },
        Authority::SystemCapability {
            capability: capability.into(),
        },
    ];
    let mut early = unique.clone();
    early.insert(0, unique[0].clone());
    let mut late = unique.clone();
    late.push(unique[3].clone());
    for authorities in [
        unique.clone(),
        vec![unique[4].clone(), unique[3].clone()],
        early,
        late,
        vec![],
        vec![unique[1].clone(), unique[1].clone()],
        vec![unique[4].clone(), unique[4].clone()],
    ] {
        let standard_len = authorities.iter().collect::<BTreeSet<_>>().len();
        let mut definition = base.clone();
        definition.edges[index].authority = authorities.clone();
        let actual = Validated::new(definition.clone());
        if authorities.is_empty() {
            assert_eq!(
                actual.unwrap_err(),
                format!("edge {edge_id} requires a label and authority")
            );
        } else if standard_len != authorities.len() {
            assert_eq!(
                actual.unwrap_err(),
                format!("duplicate authority: {edge_id}")
            );
        } else {
            definition.edges[index].authority.sort();
            assert_eq!(
                actual.unwrap().definition(),
                &definition,
                "all fields stay unchanged except existing normalization"
            );
        }
    }
    let duplicate = vec![unique[0].clone(), unique[0].clone()];
    let mut blank_label = base.clone();
    blank_label.edges[index].label = " ".into();
    blank_label.edges[index].authority = duplicate.clone();
    assert_eq!(
        Validated::new(blank_label).unwrap_err(),
        format!("edge {edge_id} requires a label and authority")
    );
    let unknown = Authority::ProjectCapability {
        capability: "missing_authority_fixture".into(),
    };
    let mut multiple_invalid = base.clone();
    multiple_invalid.edges[index].authority = [duplicate, vec![unknown.clone()]].concat();
    multiple_invalid.edges[index].alias = Some("?".into());
    multiple_invalid.edges[index].human_wait = true;
    assert_eq!(
        Validated::new(multiple_invalid).unwrap_err(),
        format!("duplicate authority: {edge_id}")
    );
    let mut unknown_only = base.clone();
    unknown_only.edges[index].authority = vec![unknown];
    assert_eq!(
        Validated::new(unknown_only).unwrap_err(),
        "unknown capability: missing_authority_fixture"
    );
    let mut wrong_human_wait = base;
    wrong_human_wait.edges[index].authority = unique;
    wrong_human_wait.edges[index].human_wait = true;
    assert_eq!(
        Validated::new(wrong_human_wait).unwrap_err(),
        format!(
            "human wait requires only project capability alternatives and deliberate action: {edge_id}"
        )
    );
}

#[test]
fn authority_insert_all_matches_the_standard_set_length_and_reports_cost() {
    use std::collections::BTreeSet;
    let authorities: Vec<_> = (0..512)
        .map(|index| match index % 5 {
            0 => Authority::TaskRole {
                role: Role::Executor,
            },
            1 => Authority::TaskRole {
                role: Role::Reviewer,
            },
            2 => Authority::TaskRole {
                role: Role::Orchestrator,
            },
            3 => Authority::ProjectCapability {
                capability: format!("cap_{}", index % 73),
            },
            _ => Authority::SystemCapability {
                capability: format!("cap_{}", index % 73),
            },
        })
        .collect();
    let (expected, old_cpu) = test_time::cpu::spent(|| authorities.iter().collect::<BTreeSet<_>>());
    let (actual, new_cpu) = test_time::cpu::spent(|| {
        let mut set = BTreeSet::new();
        for authority in &authorities {
            set.insert(authority);
        }
        set
    });
    assert_eq!(actual.len(), expected.len());
    assert_eq!(actual, expected);
    eprintln!(
        "authority set: input={}, unique={}, standard_thread_cpu={old_cpu:?}, insert_all_thread_cpu={new_cpu:?}; insert-all uses tree nodes without collection Vec/stable-sort scratch; only length is observed by validation",
        authorities.len(),
        actual.len()
    );
}

#[test]
fn workflow_mappings_are_a_known_key_checked_against_the_workflow() {
    let mapped = "[[workflow_mappings]]\nfrom = { phase = \"ready\", state = \"available\" }\nto = { phase = \"build\", state = \"available\" }\n";
    // With no [workflow] the mappings map onto built-in v1, the board's own writers: a pair it
    // declares is never remapped, and one it does not maps to one it does.
    assert!(
        Project::parse(Path::new("/tmp/demo"), mapped)
            .unwrap_err()
            .contains("workflow_mappings: ready/available is still declared")
    );
    let back = "[[workflow_mappings]]\nfrom = { phase = \"build\", state = \"available\" }\nto = { phase = \"ready\", state = \"available\" }\n";
    assert!(Project::parse(Path::new("/tmp/demo"), back).is_ok());
    assert!(
        Project::parse(
            Path::new("/tmp/demo"),
            &back.replace("\"ready\"", "\"deploy\"")
        )
        .unwrap_err()
        .contains("names a destination the workflow does not declare")
    );
    assert!(
        Project::parse(Path::new("/tmp/demo"), &format!("{CUSTOM}{mapped}"))
            .unwrap_err()
            .contains("unsupported_activation")
    );
    assert!(
        Project::parse(
            Path::new("/tmp/demo"),
            &format!("{CUSTOM}{}", mapped.replace("\"build\"", "\"verify\""))
        )
        .unwrap_err()
        .contains("names a destination the workflow does not declare")
    );
    let tree = tree::Tree::new();
    tree.write("symphony.toml", mapped);
    Project::of(&tree.root);
    let problem = Project::problem(&tree.root).unwrap();
    assert!(
        problem.contains("ready/available is still declared"),
        "{problem}"
    );
    assert!(!problem.contains("unknown"), "{problem}");
}

/// The Ed25519 base point: a key that reads, whose secret no test needs.
const AUTHORITY_KEY: &str =
    "ed25519:5866666666666666666666666666666666666666666666666666666666666666";

#[test]
fn workflow_authorities_are_a_known_key_committed_for_a_guarded_workflow_and_its_project() {
    let owned = CUSTOM
        .replace("capabilities = [\"accept\"]", "capabilities = [\"owner\"]")
        .replace("capability = \"accept\"", "capability = \"owner\"");
    let root = Path::new("/tmp/demo");
    Project::parse(root, &owned).expect("the owner's workflow is supported");
    let guarded = owned
        .replacen("adapters = []", "adapters = [\"approvals\"]", 1)
        .replacen(
            "guards = []",
            "guards = [{ id = \"approved\", adapter = \"approvals\" }]",
            1,
        )
        .replacen(
            "dependencies = \"none\", guards = []",
            "dependencies = \"none\", guards = [\"approved\"]",
            1,
        );
    let authority = |adapter: &str, key: &str| {
        format!(
            "[[workflow_authorities]]\nadapter = \"{adapter}\"\nissuer = \"ledger\"\n\
             contract = 1\nkey = \"{key}\"\n"
        )
    };
    let project = "[project]\nid = \"demo\"\n";
    let unattested = Project::parse(root, &guarded).unwrap_err();
    assert!(
        unattested.contains("unsupported_activation: unsupported guard: approved"),
        "{unattested}"
    );
    let anonymous = format!("{guarded}{}", authority("approvals", AUTHORITY_KEY));
    assert!(
        Project::parse(root, &anonymous)
            .unwrap_err()
            .contains("[project] id names none")
    );
    let committed = format!(
        "{guarded}{project}{}",
        authority("approvals", AUTHORITY_KEY)
    );
    Project::parse(root, &committed).expect("a guard with its authority committed is supported");
    let elsewhere = format!(
        "{guarded}{project}{}",
        authority("elsewhere", AUTHORITY_KEY)
    );
    assert!(
        Project::parse(root, &elsewhere)
            .unwrap_err()
            .contains("elsewhere is no adapter the workflow declares")
    );
    let unread = format!("{guarded}{project}{}", authority("approvals", "rsa:00"));
    assert!(
        Project::parse(root, &unread)
            .unwrap_err()
            .contains("not ed25519:HEX")
    );
    let alone = authority("approvals", AUTHORITY_KEY);
    assert!(
        Project::parse(root, &alone)
            .unwrap_err()
            .contains("workflow_authorities: no [workflow] to guard")
    );
    let tree = tree::Tree::new();
    tree.write("symphony.toml", &alone);
    Project::of(&tree.root);
    let problem = Project::problem(&tree.root).unwrap();
    assert!(problem.contains("no [workflow] to guard"), "{problem}");
    assert!(!problem.contains("unknown"), "{problem}");
}

#[test]
fn the_project_file_reads_its_workflow_as_the_board_does_and_names_what_it_refuses() {
    let parse = |text: &str| Project::parse(Path::new("/tmp/demo"), text);
    let unknown = parse(&CUSTOM.replace("format = 1", "format = 1\nunknown = true")).unwrap_err();
    assert!(
        unknown.starts_with("symphony.toml: workflow: unknown field `unknown`"),
        "{unknown}"
    );
    let typed = parse(&CUSTOM.replace("order = 0,", "order = \"first\",")).unwrap_err();
    assert!(
        typed.starts_with("symphony.toml: workflow: invalid type"),
        "{typed}"
    );
    // The values the file's one reading left: a datetime is a table to the board's reader there
    // too, which no field of the workflow takes.
    let dated =
        parse(&CUSTOM.replace("label = \"Build\"", "label = 1979-05-27T07:32:00Z")).unwrap_err();
    assert!(
        dated.starts_with("symphony.toml: workflow: invalid type: map, expected a string"),
        "{dated}"
    );
    assert!(
        parse("workflow = 1\n")
            .unwrap_err()
            .starts_with("symphony.toml: workflow: ")
    );
    let mappings = parse("workflow_mappings = \"none\"\n").unwrap_err();
    assert!(
        mappings.starts_with("symphony.toml: workflow_mappings: "),
        "{mappings}"
    );
    let authorities = parse("workflow_authorities = \"none\"\n").unwrap_err();
    assert!(
        authorities.starts_with("symphony.toml: workflow_authorities: "),
        "{authorities}"
    );
    // Each is a setting the Symphony knows, whatever it holds.
    for text in [
        "workflow = 1\n",
        "workflow_mappings = []\n",
        "workflow_authorities = 1\n",
    ] {
        let refused = parse(text).err().unwrap_or_default();
        assert!(!refused.contains("the Symphony knows"), "{refused}");
    }
    assert!(parse("workflow_mappings = []\n").is_ok());
    assert!(parse("workflow_authorities = []\n").is_ok());
}
