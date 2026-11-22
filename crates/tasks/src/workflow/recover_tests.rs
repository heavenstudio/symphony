//! Recovery without a board: which action answers which exit, what goes, where the task goes,
//! and what is refused.

use super::*;
use crate::store_fixture::pair;
use crate::workflow::activate::tests::{TWO, context, custom};
use crate::workflow::{Custom, TaskPosition, board_service};
use symphony_api::workflow::Role;

/// The reviewer's position TWO lacks: `verify/reviewing`, staffed by a reviewer taken from
/// `verify/awaiting` by the `review` edge, whose release moves back to `verify/awaiting` and whose
/// lost holder stays for a person, the role kept and every reservation let go.
const REVIEWING: &str = r#"{ at = { phase = "verify", state = "reviewing" }, staffing = "reviewer", slot = "retain", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = false, legacy = "review", satisfies_dependencies = false, completion = "none", recovery = { release = { target = { phase = "verify", state = "awaiting" }, manual = false, slot = "retain", release_role = true, release_reservation = false }, lost_holder = { manual = true, slot = "retain", release_role = false, release_reservation = true }, attempt_failure = { manual = true, slot = "retain", release_role = false, release_reservation = false }, cancellation_acknowledged = { manual = true, slot = "retain", release_role = false, release_reservation = false } } },
  "#;

const REVIEW: &str = r#"{ id = "review", label = "Review", from = { phase = "verify", state = "awaiting" }, to = { phase = "verify", state = "reviewing" }, operation = "claim", authority = [{ kind = "task_role", role = "reviewer" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "acquire_role", role = "reviewer" }, { kind = "slot", policy = "retain" }], automatic = false, human_wait = false },
  "#;

/// TWO with a second staffed position: the executor's `build/working` and the reviewer's
/// `verify/reviewing`, each with its own recovery.
pub(crate) fn staffed() -> String {
    TWO.replace(
        r#"{ id = "accepted", label = "Accepted" },"#,
        r#"{ id = "accepted", label = "Accepted" }, { id = "reviewing", label = "Reviewing" },"#,
    )
    .replace(
        r#"  { at = { phase = "verify", state = "accepted" }"#,
        &format!(r#"  {REVIEWING}{{ at = {{ phase = "verify", state = "accepted" }}"#),
    )
    .replace(
        r#"  { id = "accept","#,
        &format!(r#"  {REVIEW}{{ id = "accept","#),
    )
}

/// `sim/a` at `phase/state`, workflow revision 4, with `roles`.
fn at(custom: &Custom, phase: &str, state: &str, roles: &[(&str, &str)]) -> Facts {
    Facts {
        slug: "sim/a".into(),
        created_by: "owner".into(),
        position: Some(TaskPosition {
            phase: phase.into(),
            state: state.into(),
            resume: None,
            generation: 1,
            revision: 4,
            digest: custom.revision.digest.clone(),
            manual: None,
        }),
        roles: (roles.iter())
            .map(|(h, r)| (h.to_string(), r.to_string()))
            .collect(),
        ..Facts::default()
    }
}

fn recovering(
    custom: &Custom,
    facts: &Facts,
    holder: &str,
    role: Option<&str>,
    trigger: Trigger,
) -> Result<Recovery, Refusal> {
    let service = board_service("recovery");
    recover(context(custom), facts, holder, role, trigger, &service, 7.0)
}

fn dropped(rows: &[(&str, &str)]) -> Vec<(String, String)> {
    (rows.iter())
        .map(|(h, r)| (h.to_string(), r.to_string()))
        .collect()
}

#[test]
fn a_released_or_lost_executor_moves_its_task_to_the_declared_target_on_the_record() {
    let custom = custom(&staffed());
    let facts = at(
        &custom,
        "build",
        "working",
        &[("amber", "executor"), ("orch", "orchestrator")],
    );
    for trigger in [Trigger::Release, Trigger::LostHolder] {
        let recovery = recovering(&custom, &facts, "amber", None, trigger).unwrap();
        assert_eq!(recovery.drop, dropped(&[("amber", "executor")]));
        assert!(!recovery.acquires);
        let transition = recovery.transition.unwrap();
        assert_eq!(
            transition,
            Transition {
                id: "sim/a@5".into(),
                slug: "sim/a".into(),
                revision: 5,
                generation: 2,
                digest: custom.revision.digest.clone(),
                edge: None,
                operation: Some(Operation::Release),
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
                recovery: Some(Recovered {
                    trigger,
                    holder: "amber".into(),
                    role: "executor".into(),
                    manual: false,
                }),
                accepted: None,
                at: 7.0,
            }
        );
        assert_eq!(transition.position().manual, None);
    }
}

#[test]
fn a_lost_reviewer_stays_for_a_person_keeping_its_role_and_letting_reservations_go() {
    let custom = custom(&staffed());
    let mut facts = at(
        &custom,
        "verify",
        "reviewing",
        &[
            ("birch", "reviewer"),
            ("cedar", "reserved"),
            ("amber", "executor"),
        ],
    );
    // A stay keeps the pair a blocked task resumes to.
    facts.position.as_mut().unwrap().resume = Some(pair("build", "working"));
    let recovery = recovering(&custom, &facts, "birch", None, Trigger::LostHolder).unwrap();
    assert_eq!(recovery.drop, dropped(&[("cedar", "reserved")]));
    let transition = recovery.transition.unwrap();
    assert_eq!(
        (&transition.from, &transition.to, &transition.status),
        (
            &Some(pair("verify", "reviewing")),
            &pair("verify", "reviewing"),
            &"review".to_string()
        )
    );
    assert_eq!(transition.resume, Some(pair("build", "working")));
    assert_eq!(
        transition.effects,
        [Effect::Slot {
            policy: Slot::Retain
        }]
    );
    let manual = Recovered {
        trigger: Trigger::LostHolder,
        holder: "birch".into(),
        role: "reviewer".into(),
        manual: true,
    };
    assert_eq!(transition.recovery, Some(manual.clone()));
    assert_eq!(transition.position().manual, Some(manual));
    // Its own release goes back to awaiting a reviewer, the role let go.
    let released = recovering(&custom, &facts, "birch", None, Trigger::Release).unwrap();
    assert_eq!(released.drop, dropped(&[("birch", "reviewer")]));
    let moved = released.transition.unwrap();
    assert_eq!((moved.to, moved.resume), (pair("verify", "awaiting"), None));
}

#[test]
fn a_role_the_position_does_not_staff_goes_alone_and_moves_nothing() {
    let custom = custom(&staffed());
    let facts = at(
        &custom,
        "verify",
        "awaiting",
        &[
            ("amber", "executor"),
            ("amber", "reserved"),
            ("birch", "reserved"),
        ],
    );
    for trigger in [Trigger::Release, Trigger::LostHolder] {
        assert_eq!(
            recovering(&custom, &facts, "amber", None, trigger).unwrap(),
            Recovery {
                transition: None,
                drop: dropped(&[("amber", "executor"), ("amber", "reserved")]),
                acquires: false,
            }
        );
    }
    assert_eq!(
        recovering(&custom, &facts, "amber", Some("reserved"), Trigger::Release)
            .unwrap()
            .drop,
        dropped(&[("amber", "reserved")])
    );
}

#[test]
fn a_release_the_position_keeps_or_a_holder_of_nothing_there_is_refused() {
    let text = staffed().replace(
        r#"release = { target = { phase = "verify", state = "awaiting" }, manual = false, slot = "retain", release_role = true, release_reservation = false }"#,
        r#"release = { manual = true, slot = "retain", release_role = false, release_reservation = false }"#,
    );
    let custom = custom(&text);
    let facts = at(
        &custom,
        "verify",
        "reviewing",
        &[("birch", "reviewer"), ("orch", "orchestrator")],
    );
    let refused = |holder: &str, role: Option<&str>, trigger: Trigger| {
        recovering(&custom, &facts, holder, role, trigger)
            .unwrap_err()
            .to_string()
    };
    assert_eq!(
        refused("birch", None, Trigger::Release),
        "explicit_action_required: verify/reviewing keeps its reviewer when released (its \
         recovery's release keeps the role): take an edge out of it (`just tasks workflow \
         actions sim/a`)"
    );
    // The same holder lost keeps its role there, and the task waits on a person.
    let lost = recovering(&custom, &facts, "birch", None, Trigger::LostHolder).unwrap();
    assert!(lost.drop.is_empty());
    assert!(lost.transition.unwrap().position().manual.is_some());
    assert_eq!(
        refused("orch", None, Trigger::LostHolder),
        "unknown_id: orch holds no role on sim/a"
    );
    assert_eq!(
        refused("birch", Some("executor"), Trigger::Release),
        "unknown_id: birch holds no executor on sim/a"
    );
    let mut unheld = facts.clone();
    unheld.position = None;
    assert_eq!(
        recovering(&custom, &unheld, "birch", None, Trigger::Release)
            .unwrap_err()
            .to_string(),
        "unknown_id: no custom workflow holds sim/a"
    );
}

#[test]
fn a_recovery_that_acquires_a_slot_says_so_for_the_cap() {
    let text = staffed().replace(
        r#"lost_holder = { target = { phase = "build", state = "available" }, manual = false, slot = "release""#,
        r#"lost_holder = { target = { phase = "build", state = "available" }, manual = false, slot = "acquire""#,
    );
    let custom = custom(&text);
    let facts = at(&custom, "build", "working", &[("amber", "executor")]);
    let lost = recovering(&custom, &facts, "amber", None, Trigger::LostHolder).unwrap();
    assert!(lost.acquires);
    let released = recovering(&custom, &facts, "amber", None, Trigger::Release).unwrap();
    assert!(!released.acquires);
}

#[test]
fn a_task_at_a_pair_the_workflow_does_not_declare_is_stale() {
    let custom = custom(&staffed());
    let facts = at(&custom, "verify", "nowhere", &[("birch", "reviewer")]);
    assert_eq!(
        recovering(&custom, &facts, "birch", None, Trigger::LostHolder)
            .unwrap_err()
            .to_string(),
        "stale_workflow: sim/a is at verify/nowhere, which the active workflow does not declare"
    );
}

#[test]
fn a_stay_keeps_the_accepted_output_standing_and_a_move_away_supersedes_it() {
    let custom = custom(&staffed());
    let mut definition = custom.definition.definition().clone();
    (definition.positions.iter_mut())
        .find(|p| p.at == pair("verify", "reviewing"))
        .unwrap()
        .satisfies_dependencies = true;
    let context = Context {
        definition: &definition,
        revision: &custom.revision,
        generation: 2,
    };
    let mut facts = at(&custom, "verify", "reviewing", &[("birch", "reviewer")]);
    facts.accepted = Some("sim/a@3".into());
    let service = board_service("recovery");
    let after = |trigger| {
        let recovery = recover(context, &facts, "birch", None, trigger, &service, 7.0).unwrap();
        let transition = recovery.transition.unwrap();
        (transition.to, transition.accepted)
    };
    assert_eq!(
        after(Trigger::LostHolder),
        (pair("verify", "reviewing"), Some("sim/a@3".into()))
    );
    assert_eq!(after(Trigger::Release), (pair("verify", "awaiting"), None));
}
