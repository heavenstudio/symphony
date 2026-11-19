//! An execution attempt's ending answered without a board: the automatic edge within its retry
//! bound, the bound exhausted, the position's `attempt_failure` and `cancellation_acknowledged`
//! actions, and every ending refused.

use super::*;
use crate::workflow::activate::tests::custom;
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

fn pair(phase: &str, state: &str) -> Pair {
    Pair {
        phase: phase.into(),
        state: state.into(),
    }
}

fn context(custom: &Custom) -> Context<'_> {
    Context {
        definition: custom.definition.definition(),
        revision: &custom.revision,
        generation: 2,
    }
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
    let service = board_service("recovery");
    observed(
        context(custom),
        facts,
        observation,
        transitions,
        &service,
        7.0,
    )
}

fn recovery(proposal: Result<Proposal, Refusal>) -> Recovery {
    match proposal {
        Ok(Proposal::Recovery(recovery)) => *recovery,
        other => panic!("expected a recovery, got {other:?}"),
    }
}

fn answered(trigger: Trigger, manual: bool) -> Recovered {
    Recovered {
        trigger,
        holder: "amber".into(),
        role: "executor".into(),
        manual,
        attempt: Some("ab12".into()),
        exhausted: None,
    }
}

fn dropped(rows: &[(&str, &str)]) -> Vec<(String, String)> {
    (rows.iter())
        .map(|(h, r)| (h.to_string(), r.to_string()))
        .collect()
}

/// A transition of sim/a at `revision`: by `edge`, or the exhaustion of `exhausted`'s bound.
fn past(revision: u64, edge: Option<&str>, exhausted: Option<&str>) -> Transition {
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
        to: pair("build", "available"),
        resume: None,
        status: "ready".into(),
        principal: board_service("recovery"),
        effects: Vec::new(),
        key: None,
        recovery: exhausted.map(|edge| Recovered {
            exhausted: Some(edge.into()),
            ..answered(Trigger::AttemptFailure, true)
        }),
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
            key: None,
            recovery: Some(answered(Trigger::AttemptFailure, false)),
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
            exhausted: Some("rework".into()),
            ..answered(Trigger::AttemptFailure, true)
        })
    );
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
    assert_eq!(rework(&other), Ok(edge));
}

#[test]
fn an_exhausted_bound_with_no_failure_action_moves_and_lets_nothing_go() {
    let mut definition = custom(&attempts(false)).definition.definition().clone();
    let checking = (definition.positions.iter_mut())
        .find(|p| p.at == pair("verify", "checking"))
        .unwrap();
    checking.recovery = None;
    let validated = crate::workflow::Validated::new(definition).unwrap();
    let revision = custom(&attempts(false)).revision;
    let custom = Custom {
        definition: validated,
        revision,
        mappings: Vec::new(),
    };
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
    let mut definition = custom(&attempts(false)).definition.definition().clone();
    let mut again = (definition.edges.iter())
        .find(|e| e.id == "rework")
        .unwrap()
        .clone();
    again.id = "again".into();
    again.operation = Operation::Advance;
    again.retry = None;
    definition.edges.push(again);
    let custom = Custom {
        definition: crate::workflow::Validated::new(definition).unwrap(),
        ..custom(&attempts(false))
    };
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
