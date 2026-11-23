//! An execution attempt's ending answered without a board: the automatic edge within its retry
//! bound, the bound exhausted, the position's `attempt_failure` and `cancellation_acknowledged`
//! actions, and every ending refused.

use super::*;
use crate::store_fixture::pair;
use crate::workflow::activate::tests::{context, custom};
use crate::workflow::{Custom, TaskPosition, board_service};
use symphony_api::workflow::Role;

/// The workflow at `tests/store/execution/attempts.toml`: a failed check takes the automatic edge
/// `rework` within its bound of two; an executor's failed attempt moves its task back to available,
/// its role and reservations let go, or stays for a person where `stay`.
pub(crate) fn attempts(stay: bool) -> String {
    const TEXT: &str = include_str!("../../tests/store/execution/attempts.toml");
    let failure = if stay {
        r#"{ manual = true, slot = "retain", release_role = false, release_reservation = false }"#
    } else {
        r#"{ target = { phase = "build", state = "available" }, manual = false, slot = "release", release_role = true, release_reservation = true }"#
    };
    TEXT.replace("@FAILURE@", failure)
}

/// `sim/a` at `phase/state`, workflow revision 6, with `roles`.
fn at(custom: &Custom, phase: &str, state: &str, roles: &[(&str, &str)]) -> Facts {
    Facts {
        slug: "sim/a".into(),
        created_by: "owner".into(),
        position: Some(TaskPosition {
            phase: phase.into(),
            state: state.into(),
            resume: None,
            generation: 1,
            revision: 6,
            digest: custom.revision.digest.clone(),
            manual: None,
        }),
        roles: (roles.iter())
            .map(|(h, r)| (h.to_string(), r.to_string()))
            .collect(),
        ..Facts::default()
    }
}

/// Attempt `ab12`'s ending, admitted where `facts` stands, run by `amber` as its executor.
fn ending(facts: &Facts, trigger: Trigger) -> Observation {
    let position = facts.position.as_ref().unwrap();
    Observation {
        attempt: "ab12".into(),
        trigger,
        role: "executor".into(),
        handle: Some("amber".into()),
        revision: position.revision,
        at: position.at(),
    }
}

fn proposed(
    custom: &Custom,
    facts: &Facts,
    observation: &Observation,
    transitions: &[Transition],
) -> Result<Proposal, Refusal> {
    deciding(custom, facts, observation, transitions, false)
}

/// What `observation` proposes once admission refused the board's take of the automatic edge.
fn refused(
    custom: &Custom,
    facts: &Facts,
    observation: &Observation,
    transitions: &[Transition],
) -> Result<Proposal, Refusal> {
    deciding(custom, facts, observation, transitions, true)
}

fn deciding(
    custom: &Custom,
    facts: &Facts,
    observation: &Observation,
    transitions: &[Transition],
    refused: bool,
) -> Result<Proposal, Refusal> {
    let service = board_service("recovery");
    observed(
        context(custom),
        facts,
        observation,
        transitions,
        refused,
        &service,
        7.0,
    )
}

/// The workflow of [`attempts`] with `change` made to its definition.
fn changed(stay: bool, change: impl FnOnce(&mut Definition)) -> Custom {
    let mut definition = custom(&attempts(stay)).definition.definition().clone();
    change(&mut definition);
    Custom {
        definition: crate::workflow::Validated::new(definition).unwrap(),
        ..custom(&attempts(stay))
    }
}

/// `definition`'s position at `phase/state`.
fn position_at<'a>(definition: &'a mut Definition, phase: &str, state: &str) -> &'a mut Position {
    (definition.positions.iter_mut())
        .find(|p| p.at == pair(phase, state))
        .unwrap()
}

fn recovery(proposal: Result<Proposal, Refusal>) -> Recovery {
    match proposal {
        Ok(Proposal::Recovery { recovery, .. }) => *recovery,
        other => panic!("expected a recovery, got {other:?}"),
    }
}

fn answered(trigger: Trigger, manual: bool) -> Recovered {
    Recovered {
        trigger,
        holder: "amber".into(),
        role: "executor".into(),
        manual,
    }
}

fn dropped(rows: &[(&str, &str)]) -> Vec<(String, String)> {
    (rows.iter())
        .map(|(h, r)| (h.to_string(), r.to_string()))
        .collect()
}

/// A transition of sim/a at `revision`: the board's take of `edge` on an attempt's failure, or the
/// exhaustion of `exhausted`'s bound, which sends it to verify/stuck.
pub(crate) fn past(revision: u64, edge: Option<&str>, exhausted: Option<&str>) -> Transition {
    Transition {
        id: Transition::id_of("sim/a", revision),
        slug: "sim/a".into(),
        revision,
        generation: 1,
        digest: "d".into(),
        edge: edge.map(str::to_string),
        operation: None,
        activation: None,
        from: None,
        to: match exhausted {
            Some(_) => pair("verify", "stuck"),
            None => pair("build", "available"),
        },
        resume: None,
        status: "ready".into(),
        principal: board_service("recovery"),
        effects: Vec::new(),
        key: None,
        recovery: (edge.is_some() || exhausted.is_some())
            .then(|| answered(Trigger::AttemptFailure, exhausted.is_some())),
        accepted: None,
        at: 1.0,
    }
}

#[test]
fn an_executors_failed_attempt_moves_its_task_by_the_positions_action_on_the_record() {
    let custom = custom(&attempts(false));
    let facts = at(
        &custom,
        "build",
        "working",
        &[
            ("amber", "executor"),
            ("cedar", "reserved"),
            ("orch", "orchestrator"),
        ],
    );
    let failed = ending(&facts, Trigger::AttemptFailure);
    let moved = recovery(proposed(&custom, &facts, &failed, &[]));
    assert_eq!(
        moved.drop,
        dropped(&[("amber", "executor"), ("cedar", "reserved")])
    );
    assert!(!moved.acquires);
    assert_eq!(
        moved.transition.unwrap(),
        Transition {
            id: "sim/a@7".into(),
            slug: "sim/a".into(),
            revision: 7,
            generation: 2,
            digest: custom.revision.digest.clone(),
            edge: None,
            operation: None,
            activation: None,
            from: Some(pair("build", "working")),
            to: pair("build", "available"),
            resume: None,
            status: "ready".into(),
            principal: board_service("recovery"),
            effects: vec![
                Effect::ReleaseRole {
                    role: Role::Executor
                },
                Effect::Slot {
                    policy: Slot::Release
                },
            ],
            // The board answers the attempt once, by its key.
            key: Some("attempt/ab12".into()),
            recovery: Some(answered(Trigger::AttemptFailure, false)),
            accepted: None,
            at: 7.0,
        }
    );
}

#[test]
fn a_failure_or_an_acknowledged_cancel_that_stays_waits_on_a_person_keeping_its_roles() {
    let custom = custom(&attempts(true));
    let mut facts = at(&custom, "build", "working", &[("amber", "executor")]);
    facts.position.as_mut().unwrap().resume = Some(pair("build", "available"));
    for trigger in [Trigger::AttemptFailure, Trigger::CancellationAcknowledged] {
        let stay = recovery(proposed(&custom, &facts, &ending(&facts, trigger), &[]));
        assert!(stay.drop.is_empty());
        let transition = stay.transition.unwrap();
        assert_eq!(
            (&transition.from, &transition.to, &transition.resume),
            (
                &Some(pair("build", "working")),
                &pair("build", "working"),
                &Some(pair("build", "available"))
            )
        );
        assert_eq!(
            transition.effects,
            [Effect::Slot {
                policy: Slot::Retain
            }]
        );
        assert_eq!(
            transition.position().manual,
            Some(answered(trigger, true)),
            "{trigger:?}"
        );
    }
}

#[test]
fn a_failed_check_takes_the_automatic_edge_until_its_bound_is_exhausted() {
    let custom = custom(&attempts(false));
    let facts = at(&custom, "verify", "checking", &[("cedar", "reserved")]);
    let mut check = ending(&facts, Trigger::AttemptFailure);
    check.handle = None;
    check.role = "checker".into();
    let rework = |transitions: &[Transition]| proposed(&custom, &facts, &check, transitions);
    let edge = Proposal::Edge {
        edge: "rework".into(),
        answered: Recovered {
            holder: "ab12".into(),
            role: "checker".into(),
            ..answered(Trigger::AttemptFailure, false)
        },
    };
    // Taken once before: once more.
    let once = [past(3, Some("rework"), None), past(4, Some("claim"), None)];
    assert_eq!(rework(&once), Ok(edge.clone()));
    // Taken twice: the bound is exhausted, the failure's action sent to the bound's pair.
    let twice = [past(2, Some("rework"), None), past(5, Some("rework"), None)];
    let exhausted = recovery(rework(&twice));
    assert_eq!(exhausted.drop, Vec::<(String, String)>::new());
    let transition = exhausted.transition.unwrap();
    assert_eq!(
        (&transition.edge, &transition.to, &transition.status),
        (&None, &pair("verify", "stuck"), &"blocked".to_string())
    );
    assert_eq!(
        transition.recovery,
        Some(Recovered {
            holder: "ab12".into(),
            role: "checker".into(),
            ..answered(Trigger::AttemptFailure, true)
        })
    );
    assert_eq!(transition.key.as_deref(), Some("attempt/ab12"));
    assert!(matches!(
        rework(&twice),
        Ok(Proposal::Recovery { exhausted: Some(edge), .. }) if edge == "rework"
    ));
    assert_eq!(
        transition.effects,
        [Effect::Slot {
            policy: Slot::Retain
        }]
    );
    // An exhaustion since starts the count again; another edge's takes count nothing.
    let since = [
        past(2, Some("rework"), None),
        past(3, Some("rework"), None),
        past(4, None, Some("rework")),
        past(5, Some("unstick"), None),
        past(6, Some("rework"), None),
    ];
    assert_eq!(rework(&since), Ok(edge.clone()));
    let other = [past(2, None, Some("other")), past(3, Some("rework"), None)];
    assert_eq!(rework(&other), Ok(edge.clone()));
    // Whoever brought the task to the bound's pair starts the count again: a person, or an
    // activation that mapped it there.
    for arrived in [
        Transition {
            to: pair("verify", "stuck"),
            principal: crate::workflow::local_owner(),
            ..past(4, None, None)
        },
        Transition {
            to: pair("verify", "stuck"),
            activation: Some(2),
            ..past(4, None, None)
        },
    ] {
        let since = [
            past(2, Some("rework"), None),
            past(3, Some("rework"), None),
            arrived,
            past(5, Some("unstick"), None),
            past(6, Some("rework"), None),
        ];
        assert_eq!(rework(&since), Ok(edge.clone()));
    }
    // A caller's take of an edge of that id, where an earlier definition had it no automatic
    // edge, is no take of the board's.
    let caller = |revision| Transition {
        recovery: None,
        principal: crate::workflow::local_owner(),
        ..past(revision, Some("rework"), None)
    };
    assert_eq!(rework(&[caller(2), caller(3)]), Ok(edge.clone()));
    assert!(matches!(
        rework(&[
            caller(2),
            past(3, Some("rework"), None),
            past(4, Some("rework"), None)
        ]),
        Ok(Proposal::Recovery { .. })
    ));
}

#[test]
fn an_exhausted_bound_with_no_failure_action_moves_and_lets_nothing_go() {
    let custom = changed(false, |definition| {
        position_at(definition, "verify", "checking").recovery = None;
    });
    let facts = at(&custom, "verify", "checking", &[("cedar", "reserved")]);
    let failed = ending(&facts, Trigger::AttemptFailure);
    let twice = [past(2, Some("rework"), None), past(3, Some("rework"), None)];
    let exhausted = recovery(proposed(&custom, &facts, &failed, &twice));
    assert!(exhausted.drop.is_empty());
    let transition = exhausted.transition.unwrap();
    assert_eq!(transition.to, pair("verify", "stuck"));
    assert_eq!(
        transition.effects,
        [Effect::Slot {
            policy: Slot::Retain
        }]
    );
    // With no automatic edge either, a failure there has no answer, and the task stays.
    assert_eq!(
        proposed(
            &custom,
            &facts,
            &ending(&facts, Trigger::CancellationAcknowledged),
            &[]
        )
        .unwrap_err()
        .to_string(),
        "explicit_action_required: verify/checking declares no cancellation_acknowledged \
         recovery: sim/a stays as it was"
    );
}

#[test]
fn an_acknowledged_cancel_while_checking_goes_back_to_build() {
    let custom = custom(&attempts(false));
    let facts = at(&custom, "verify", "checking", &[]);
    let cancelled = recovery(proposed(
        &custom,
        &facts,
        &ending(&facts, Trigger::CancellationAcknowledged),
        &[past(2, Some("rework"), None), past(3, Some("rework"), None)],
    ));
    let transition = cancelled.transition.unwrap();
    assert_eq!(transition.to, pair("build", "available"));
    assert_eq!(
        transition.recovery,
        Some(answered(Trigger::CancellationAcknowledged, false))
    );
}

#[test]
fn an_ending_from_where_its_task_no_longer_stands_is_refused_as_stale() {
    let custom = custom(&attempts(false));
    let facts = at(&custom, "build", "working", &[("amber", "executor")]);
    let mut moved_on = ending(&facts, Trigger::AttemptFailure);
    moved_on.revision = 5;
    assert_eq!(
        proposed(&custom, &facts, &moved_on, &[])
            .unwrap_err()
            .to_string(),
        "stale_task: sim/a is at build/working (workflow revision 6), not at build/working \
         (revision 5) where attempt ab12 was admitted: its ending stays on the attempt and moves \
         nothing"
    );
    let mut elsewhere = ending(&facts, Trigger::AttemptFailure);
    elsewhere.at = pair("verify", "checking");
    assert_eq!(
        proposed(&custom, &facts, &elsewhere, &[]).unwrap_err().code,
        RefusalCode::StaleTask
    );
    let mut unheld = facts.clone();
    unheld.position = None;
    assert_eq!(
        proposed(&custom, &unheld, &moved_on, &[])
            .unwrap_err()
            .to_string(),
        "unknown_id: no custom workflow holds sim/a"
    );
    let nowhere = at(&custom, "verify", "nowhere", &[]);
    assert_eq!(
        proposed(
            &custom,
            &nowhere,
            &ending(&nowhere, Trigger::AttemptFailure),
            &[]
        )
        .unwrap_err()
        .code,
        RefusalCode::StaleWorkflow
    );
    for exit in [Trigger::Release, Trigger::LostHolder] {
        assert_eq!(
            proposed(&custom, &facts, &ending(&facts, exit), &[])
                .unwrap_err()
                .code,
            RefusalCode::UnknownId
        );
    }
}

#[test]
fn two_automatic_edges_from_the_failed_pair_are_refused_for_the_board_cannot_choose() {
    let custom = changed(false, |definition| {
        let mut again = (definition.edges.iter())
            .find(|e| e.id == "rework")
            .unwrap()
            .clone();
        again.id = "again".into();
        again.operation = Operation::Advance;
        again.retry = None;
        definition.edges.push(again);
    });
    let facts = at(&custom, "verify", "checking", &[]);
    assert_eq!(
        proposed(
            &custom,
            &facts,
            &ending(&facts, Trigger::AttemptFailure),
            &[]
        )
        .unwrap_err()
        .to_string(),
        "unsupported_activation: more than one automatic edge leaves verify/checking, rework \
         among them: the board cannot choose"
    );
}

#[test]
fn a_holders_exit_never_takes_an_attempts_action() {
    let custom = custom(&attempts(false));
    let facts = at(&custom, "build", "working", &[("amber", "executor")]);
    let service = board_service("recovery");
    for trigger in [Trigger::AttemptFailure, Trigger::CancellationAcknowledged] {
        assert_eq!(
            recover(
                context(&custom),
                &facts,
                "amber",
                None,
                trigger,
                &service,
                7.0
            )
            .unwrap_err()
            .code,
            RefusalCode::UnknownId
        );
    }
}

#[test]
fn a_stay_where_dependants_are_satisfied_keeps_the_output_accepted_there_and_a_move_supersedes_it()
{
    let satisfying = |stay: bool| {
        changed(stay, |definition| {
            position_at(definition, "build", "working").satisfies_dependencies = true;
        })
    };
    for (stay, kept) in [(true, Some("sim/a@4")), (false, None)] {
        let custom = satisfying(stay);
        let mut facts = at(&custom, "build", "working", &[("amber", "executor")]);
        facts.accepted = Some("sim/a@4".into());
        for trigger in [Trigger::AttemptFailure, Trigger::CancellationAcknowledged] {
            let recovered = recovery(proposed(&custom, &facts, &ending(&facts, trigger), &[]));
            let transition = recovered.transition.unwrap();
            let expected = if trigger == Trigger::CancellationAcknowledged {
                // The cancel stays in both, for a person.
                Some("sim/a@4")
            } else {
                kept
            };
            assert_eq!(
                transition.accepted.as_deref(),
                expected,
                "stay {stay}, {trigger:?}"
            );
        }
    }
}

#[test]
fn a_refused_take_answers_a_failure_by_the_positions_action_as_if_no_automatic_edge_left() {
    // verify/checking's own attempt_failure action stays there for a person.
    let custom = custom(&attempts(false));
    let facts = at(&custom, "verify", "checking", &[("cedar", "reserved")]);
    let mut check = ending(&facts, Trigger::AttemptFailure);
    check.handle = None;
    check.role = "checker".into();
    assert!(matches!(
        proposed(&custom, &facts, &check, &[]),
        Ok(Proposal::Edge { .. })
    ));
    let stay = match refused(&custom, &facts, &check, &[]) {
        Ok(Proposal::Recovery {
            recovery,
            exhausted: None,
        }) => *recovery,
        other => panic!("expected the position's action, got {other:?}"),
    };
    assert!(stay.drop.is_empty());
    let transition = stay.transition.unwrap();
    assert_eq!(
        (&transition.edge, &transition.from, &transition.to),
        (
            &None,
            &Some(pair("verify", "checking")),
            &pair("verify", "checking")
        )
    );
    assert_eq!(transition.key.as_deref(), Some("attempt/ab12"));
    assert_eq!(
        transition.position().manual,
        Some(Recovered {
            holder: "ab12".into(),
            role: "checker".into(),
            ..answered(Trigger::AttemptFailure, true)
        })
    );
    // One that moves moves the task where it says, the board taking no edge.
    let moving = changed(false, |definition| {
        let checking = position_at(definition, "verify", "checking");
        let actions = checking.recovery.as_mut().unwrap();
        actions.attempt_failure = RecoveryAction {
            target: Some(pair("build", "available")),
            manual: false,
            slot: Slot::Release,
            release_role: false,
            release_reservation: true,
        };
    });
    let moved = recovery(refused(&moving, &facts, &check, &[]));
    assert_eq!(moved.drop, dropped(&[("cedar", "reserved")]));
    let transition = moved.transition.unwrap();
    assert_eq!(
        (&transition.edge, &transition.to, transition.status.as_str()),
        (&None, &pair("build", "available"), "ready")
    );
    assert!(!transition.recovery.unwrap().manual);
}

#[test]
fn a_refused_take_at_a_position_with_no_failure_action_is_refused_as_undeclared() {
    let custom = changed(false, |definition| {
        position_at(definition, "verify", "checking").recovery = None;
    });
    let facts = at(&custom, "verify", "checking", &[]);
    let failed = ending(&facts, Trigger::AttemptFailure);
    assert_eq!(
        refused(&custom, &facts, &failed, &[])
            .unwrap_err()
            .to_string(),
        "explicit_action_required: verify/checking declares no attempt_failure recovery: sim/a \
         stays as it was"
    );
}

#[test]
fn a_refused_take_spends_no_retry_and_a_bound_already_exhausted_still_sends_the_task_on() {
    let custom = custom(&attempts(false));
    let facts = at(&custom, "verify", "checking", &[]);
    let failed = ending(&facts, Trigger::AttemptFailure);
    // Two failures answered by the position's action while the take was refused: no take of the
    // board's, so the edge is proposed again.
    let fallback = |revision| Transition {
        to: pair("verify", "checking"),
        ..past(revision, None, None)
    };
    let answered_by_hand = Transition {
        recovery: Some(answered(Trigger::AttemptFailure, true)),
        ..fallback(3)
    };
    let fell_back = [
        past(2, Some("rework"), None),
        answered_by_hand.clone(),
        Transition {
            revision: 4,
            ..answered_by_hand
        },
    ];
    assert!(matches!(
        proposed(&custom, &facts, &failed, &fell_back),
        Ok(Proposal::Edge { edge, .. }) if edge == "rework"
    ));
    // At the bound, a refused take changes nothing: the bound sends it to verify/stuck.
    let twice = [past(2, Some("rework"), None), past(3, Some("rework"), None)];
    assert!(matches!(
        refused(&custom, &facts, &failed, &twice),
        Ok(Proposal::Recovery { exhausted: Some(edge), .. }) if edge == "rework"
    ));
}

#[test]
fn a_refused_take_leaves_an_acknowledged_cancel_to_its_own_action() {
    let custom = custom(&attempts(false));
    let facts = at(&custom, "verify", "checking", &[]);
    let cancelled = ending(&facts, Trigger::CancellationAcknowledged);
    assert_eq!(
        refused(&custom, &facts, &cancelled, &[]),
        proposed(&custom, &facts, &cancelled, &[])
    );
}

#[test]
fn an_observation_records_the_handle_it_ran_as_or_the_attempt_where_none() {
    let custom = custom(&attempts(false));
    let facts = at(&custom, "build", "working", &[]);
    let mut observation = ending(&facts, Trigger::AttemptFailure);
    assert_eq!(
        observation.recovered(),
        answered(Trigger::AttemptFailure, false)
    );
    observation.handle = None;
    assert_eq!(observation.recovered().holder, "ab12");
}
