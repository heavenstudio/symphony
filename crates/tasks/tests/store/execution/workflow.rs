//! An execution attempt under a custom workflow, on each database (docs/workflow/WORKFLOW.md, "Attempts"):
//! activation waits for a task's live attempt to drain or be cancelled, never rebinding it; the
//! attempt keeps where its task stood when it was admitted, and its ending, proposed by the
//! coordinator as the board's recovery service, moves the task from there alone: by the
//! position's `attempt_failure` or `cancellation_acknowledged` action, or the automatic edge out of
//! it within its retry bound, then to a person. A cancelled or superseded attempt's late result,
//! and a repeat of an observation, move nothing more.

use super::*;
use std::collections::BTreeMap;
use symphony_api::workflow::{
    Definition, Dependency, OperationRequest, Pair, RecoveryAction, Slot, Source,
};
use symphony_board::workflow::{
    Custom, Host, Mapping, Transition, Trigger, Validated, agent_session, board_service,
    local_owner, written,
};
use symphony_board::{NewTask, Store};

/// An executor's failed attempt moves its task back to available, its role let go.
const MOVE: &str = r#"{ target = { phase = "build", state = "available" }, manual = false, slot = "release", release_role = true, release_reservation = true }"#;
/// An executor's failed attempt keeps its task where it was, for a person.
const STAY: &str =
    r#"{ manual = true, slot = "retain", release_role = false, release_reservation = false }"#;

/// The task `OTHER_TASK` depends on in [`waiting`].
const PREREQUISITE: &str = "sim/base";

/// `attempts.toml`, an executor's failed attempt answered by `failure`.
fn attempts(failure: &str) -> Custom {
    let text = include_str!("attempts.toml").replace("@FAILURE@", failure);
    let (definition, mappings) = written(&text).unwrap();
    revised(definition.unwrap(), mappings)
}

fn revised(definition: Validated, mappings: Vec<Mapping>) -> Custom {
    let revision = definition
        .revision(Source {
            commit: "1".repeat(40),
            project_blob: "2".repeat(40),
        })
        .unwrap();
    Custom {
        definition,
        revision,
        mappings,
        authorities: Vec::new(),
        project: None,
    }
}

/// [`attempts`] with the automatic edge `rework` waiting on its task's prerequisites being done,
/// and a failed check at `verify/checking` answered by `checking` (none: it declares no recovery).
fn gated(failure: &str, checking: Option<RecoveryAction>) -> Custom {
    let Custom {
        definition,
        mappings,
        ..
    } = attempts(failure);
    let mut definition: Definition = definition.definition().clone();
    let rework = (definition.edges.iter_mut())
        .find(|e| e.id == "rework")
        .unwrap();
    rework.dependencies = Dependency::Done;
    let at = (definition.positions.iter_mut())
        .find(|p| p.at == pair("verify", "checking"))
        .unwrap();
    match checking {
        Some(action) => at.recovery.as_mut().unwrap().attempt_failure = action,
        None => at.recovery = None,
    }
    revised(Validated::new(definition).unwrap(), mappings)
}

/// A check's failure at `verify/checking` that sends its task back to build.
fn back_to_build() -> RecoveryAction {
    RecoveryAction {
        target: Some(pair("build", "available")),
        manual: false,
        slot: Slot::Release,
        release_role: false,
        release_reservation: false,
    }
}

/// A check's failure at `verify/checking` that stays there for a person, as `attempts.toml` says.
fn stays() -> RecoveryAction {
    RecoveryAction {
        target: None,
        manual: true,
        slot: Slot::Retain,
        release_role: false,
        release_reservation: false,
    }
}

/// The board the execution tests run on, `amber-heron` a session, and `custom` active.
fn under(backend: Backend, custom: &Custom) -> Fixture {
    let f = Fixture::on(backend, Seed::default().session("amber-heron"));
    f.task(TASK);
    f.task(OTHER_TASK);
    f.store.activate_workflow(custom, &Host::local()).unwrap();
    f
}

/// [`under`], with `OTHER_TASK` depending on [`PREREQUISITE`], open.
fn waiting(backend: Backend, custom: &Custom) -> Fixture {
    let f = Fixture::on(backend, Seed::default().session("amber-heron"));
    f.task(TASK);
    f.task(PREREQUISITE);
    f.task_with(NewTask {
        depends: vec![PREREQUISITE.into()],
        ..NewTask::new(OTHER_TASK, "Jump departure", "owner")
    });
    f.store.activate_workflow(custom, &Host::local()).unwrap();
    f
}

/// `slug` claimed and submitted by amber-heron, to be checked.
fn to_check(f: &Fixture, slug: &str) {
    let amber = agent_session("amber-heron");
    take(&f.store, slug, &amber, "claim");
    take(&f.store, slug, &amber, "submit");
    assert_eq!(at(f, slug), pair("verify", "checking"));
}

/// [`PREREQUISITE`] built and accepted: done, by its workflow.
fn prerequisite_done(f: &Fixture) {
    to_check(f, PREREQUISITE);
    take(&f.store, PREREQUISITE, &local_owner(), "accept");
}

/// The thread's lines that say nothing answers attempt `id`'s failure.
fn unanswered(f: &Fixture, slug: &str, id: &str) -> Vec<String> {
    (recoveries(f, slug).into_iter())
        .filter(|line| line.contains(&format!("attempt {id} ")) && line.contains("nothing answers"))
        .collect()
}

fn pair(phase: &str, state: &str) -> Pair {
    Pair {
        phase: phase.into(),
        state: state.into(),
    }
}

fn at(f: &Fixture, slug: &str) -> Pair {
    f.store.workflow_position(slug).unwrap().unwrap().at()
}

fn roles(f: &Fixture, slug: &str) -> Vec<(String, String)> {
    let mut roles: Vec<_> = (f.store.get(slug).unwrap().roles.iter())
        .map(|r| (r.handle.clone(), r.role.clone()))
        .collect();
    roles.sort();
    roles
}

fn transitions(f: &Fixture, slug: &str) -> Vec<Transition> {
    f.store.workflow_transitions(slug).unwrap()
}

/// `edge` taken on `slug` by `principal` from where it reads the task now.
fn take(store: &Store, slug: &str, principal: &Principal, edge: &str) {
    let next = (store.workflow_actions(slug, principal, &Host::local())).unwrap();
    let key = format!("{slug}:{edge}:{}", next.snapshot.task_revision);
    let request = OperationRequest {
        project: store.database().database().into(),
        task: slug.into(),
        edge: edge.into(),
        expected: next.snapshot,
        input_revisions: BTreeMap::new(),
        idempotency_key: key,
        inputs: BTreeMap::new(),
    };
    (store.apply_operation(principal, &Host::local(), &request)).expect("taken");
}

/// The coordinator over `fake`, proposing endings to the local host's workflow.
fn proposing<'a>(f: &'a Fixture, fake: &'a Fake) -> Coordinator<'a> {
    coordinator(f, fake).with_workflow(Host::local())
}

/// `slug` claimed by amber-heron, and its executor's attempt admitted under `key` and started.
fn working(f: &Fixture, c: &Coordinator, slug: &str, key: &str) -> (Attempt, String) {
    take(&f.store, slug, &agent_session("amber-heron"), "claim");
    let admission = asked(slug).request_key(key);
    let attempt = c.admit(&owner_grant(), admission).expect("admitted");
    let resource = started(c, &attempt.id);
    (attempt, resource)
}

/// A check of `slug`, an attempt no session holds, admitted under `key`, started and failed.
fn failed_check(c: &Coordinator, fake: &Fake, slug: &str, key: &str) -> Attempt {
    let admission = Admission::new(slug, "checker", "checker", input("r1")).request_key(key);
    let attempt = c.admit(&owner_grant(), admission).expect("admitted");
    let resource = started(c, &attempt.id);
    fake.runtime(&resource, RuntimeState::Failed);
    answered(c.step(&attempt.id));
    assert_eq!(read(c, &attempt.id).state.ending, Some(Ending::Failed));
    attempt
}

/// A check of `slug` admitted under `key`, whose start the provider refuses: it ends rejected.
fn rejected_check(c: &Coordinator, fake: &Fake, slug: &str, key: &str) -> Attempt {
    let admission = Admission::new(slug, "checker", "checker", input("r1")).request_key(key);
    let attempt = c.admit(&owner_grant(), admission).expect("admitted");
    fake.script([Fault::Refuse("no room")]);
    answered(c.step(&attempt.id));
    assert_eq!(read(c, &attempt.id).state.ending, Some(Ending::Rejected));
    attempt
}

/// The lines the board's recovery service put on `slug`'s thread.
fn recoveries(f: &Fixture, slug: &str) -> Vec<String> {
    (f.said(slug).into_iter())
        .filter(|line| line.contains("attempt "))
        .collect()
}

on_both! {
    fn activation_waits_for_a_live_attempt_to_end_or_be_cancelled_and_never_rebinds_it(backend: Backend) {
        let f = board(backend);
        let fake = Fake::default();
        let c = proposing(&f, &fake);
        let jump = c.admit(&owner_grant(), asked(TASK).request_key("j")).unwrap();
        let jumping = started(&c, &jump.id);
        let land = c.admit(&owner_grant(), asked(OTHER_TASK).request_key("l")).unwrap();
        let landing = started(&c, &land.id);
        let custom = attempts(MOVE);
        let refused = |store: &Store| crate::common::refused(store.activate_workflow(&custom, &Host::local()));
        assert_eq!(
            refused(&f.store),
            format!(
                "workflow: {TASK} runs execution attempt {}, and activation would \
                 change its position or its staffing; an attempt drains first (let it end, or cancel \
                 it), and is never rebound to another position",
                jump.id
            )
        );
        assert!(f.store.workflow_active().unwrap().is_none(), "nothing activated");
        // Cancelled, no result of it is eligible: invalidated, it no longer holds its task.
        c.cancel(&owner_grant(), &jump.id).unwrap();
        assert!(refused(&f.store).contains(&format!("{OTHER_TASK} runs execution attempt {}", land.id)));
        // Ended, it has drained.
        fake.runtime(&landing, RuntimeState::Succeeded);
        answered(c.step(&land.id));
        assert!(f.store.live_attempts(OTHER_TASK).unwrap().is_empty());
        f.store.activate_workflow(&custom, &Host::local()).unwrap();
        assert_eq!(at(&f, TASK), pair("build", "available"));
        assert_eq!(at(&f, OTHER_TASK), pair("build", "available"));
        // The cancelled attempt ends afterwards, admitted before any workflow held its task: its
        // ending stays on it, and its task where activation placed it.
        let (operation, _) = answered(c.step(&jump.id));
        assert_eq!(operation, Operation::Cancel);
        assert_eq!(fake.runtime_of(&jumping), RuntimeState::Terminated);
        answered(c.step(&jump.id));
        let ended = read(&c, &jump.id).state;
        assert_eq!((ended.ending, ended.cancellation), (Some(Ending::Cancelled), Cancellation::Confirmed));
        assert_eq!(f.store.observe_attempt(&jump.id, &Host::local()).unwrap(), None);
        assert_eq!(transitions(&f, TASK).len(), 1, "its placement alone");
    }

    fn a_failed_attempt_moves_its_task_by_the_failure_action_once_however_often_it_is_observed(backend: Backend) {
        let custom = attempts(MOVE);
        let f = under(backend, &custom);
        let fake = Fake::default();
        let c = proposing(&f, &fake);
        let (attempt, resource) = working(&f, &c, TASK, "k1");
        let pinned = f.store.workflow_position(TASK).unwrap().unwrap();
        assert_eq!(roles(&f, TASK), [("amber-heron".into(), "executor".into())]);
        // Running, it proposes nothing.
        assert_eq!(f.store.observe_attempt(&attempt.id, &Host::local()).unwrap(), None);
        fake.runtime(&resource, RuntimeState::Failed);
        answered(c.step(&attempt.id));
        assert_eq!(read(&c, &attempt.id).state.ending, Some(Ending::Failed));

        assert_eq!(at(&f, TASK), pair("build", "available"));
        assert_eq!(f.store.get(TASK).unwrap().status, "ready");
        assert!(roles(&f, TASK).is_empty(), "its executor let go");
        let all = transitions(&f, TASK);
        let moved = all.last().unwrap().clone();
        assert_eq!(moved.revision, pinned.revision + 1);
        assert_eq!((&moved.from, moved.edge.as_deref()), (&Some(pair("build", "working")), None));
        assert_eq!(moved.principal, board_service("recovery"));
        let answered = moved.recovery.clone().unwrap();
        assert_eq!(
            (answered.trigger, moved.key.clone(), answered.holder.as_str(), answered.manual),
            (Trigger::AttemptFailure, Some(format!("attempt/{}", attempt.id)), "amber-heron", false)
        );
        assert_eq!(
            recoveries(&f, TASK),
            [format!(
                "the board's recovery ({TASK}@{}): attempt {} of amber-heron's executor failed; \
                 build/working → build/available, workflow revision {}; let go: amber-heron executor",
                moved.revision, attempt.id, moved.revision
            )]
        );
        // A repeat, by the coordinator after a crash or by a host, answers the one transition.
        for _ in 0..2 {
            assert_eq!(f.store.observe_attempt(&attempt.id, &Host::local()).unwrap(), Some(moved.clone()));
        }
        assert_eq!(transitions(&f, TASK), all);
        assert_eq!(recoveries(&f, TASK).len(), 1);
        // A host that grants the board no recovery moves nothing.
        assert!(
            crate::common::refused(f.store.observe_attempt(&attempt.id, &Host::default()))
                .starts_with("unauthorized:")
        );
    }

    fn a_failed_attempt_whose_position_stays_waits_on_a_person_and_its_retry_starts_from_there(backend: Backend) {
        let custom = attempts(STAY);
        let f = under(backend, &custom);
        let fake = Fake::default();
        let c = proposing(&f, &fake);
        let (attempt, resource) = working(&f, &c, TASK, "k1");
        fake.runtime(&resource, RuntimeState::Failed);
        answered(c.step(&attempt.id));
        let stayed = f.store.workflow_position(TASK).unwrap().unwrap();
        assert_eq!(stayed.at(), pair("build", "working"));
        let manual = stayed.manual.clone().expect("a person decides");
        assert_eq!(manual.trigger, Trigger::AttemptFailure);
        let keyed = |f: &Fixture| transitions(f, TASK).last().unwrap().key.clone();
        assert_eq!(keyed(&f), Some(format!("attempt/{}", attempt.id)));
        assert_eq!(f.store.get(TASK).unwrap().status, "in_progress");
        assert_eq!(roles(&f, TASK), [("amber-heron".into(), "executor".into())], "its role kept");

        // The owner retries: the successor is admitted where the task stands now, and the
        // superseded attempt moves nothing more.
        let retry = c.retry(&owner_grant(), &attempt.id, "it failed on its own", Some("r1")).unwrap();
        assert_eq!(f.store.observe_attempt(&attempt.id, &Host::local()).unwrap(), None);
        let resource = started(&c, &retry.id);
        fake.runtime(&resource, RuntimeState::Failed);
        answered(c.step(&retry.id));
        let again = f.store.workflow_position(TASK).unwrap().unwrap();
        assert_eq!(again.revision, stayed.revision + 1);
        assert_eq!(again.manual.unwrap().trigger, Trigger::AttemptFailure);
        assert_eq!(keyed(&f), Some(format!("attempt/{}", retry.id)));
        assert_eq!(recoveries(&f, TASK).len(), 2);
    }

    fn a_late_failure_after_a_cancel_is_the_cancel_acknowledged_never_a_failure(backend: Backend) {
        let custom = attempts(MOVE);
        let f = under(backend, &custom);
        let fake = Fake::default();
        let c = proposing(&f, &fake);
        let (attempt, resource) = working(&f, &c, TASK, "k1");
        let before = transitions(&f, TASK).len();
        c.cancel(&owner_grant(), &attempt.id).unwrap();
        // The cancel reaches the provider, then the runtime reports it failed.
        let (operation, _) = answered(c.step(&attempt.id));
        assert_eq!(operation, Operation::Cancel);
        assert_eq!(transitions(&f, TASK).len(), before, "nothing moves while the cancel is unsettled");
        fake.runtime(&resource, RuntimeState::Failed);
        answered(c.step(&attempt.id));
        let ended = read(&c, &attempt.id).state;
        assert_eq!((ended.ending, ended.cancellation), (Some(Ending::Failed), Cancellation::Confirmed));
        // Not the failure's move to available: the cancel's stay, for a person.
        let position = f.store.workflow_position(TASK).unwrap().unwrap();
        assert_eq!(position.at(), pair("build", "working"));
        assert_eq!(position.manual.unwrap().trigger, Trigger::CancellationAcknowledged);
        assert_eq!(roles(&f, TASK), [("amber-heron".into(), "executor".into())]);
        assert_eq!(transitions(&f, TASK).len(), before + 1);
    }

    fn an_ending_from_where_its_task_no_longer_stands_stays_on_its_attempt(backend: Backend) {
        let custom = attempts(MOVE);
        let f = under(backend, &custom);
        let fake = Fake::default();
        let c = proposing(&f, &fake);
        let (attempt, resource) = working(&f, &c, TASK, "k1");
        // The executor submits while its attempt still runs: the task moved on without it.
        take(&f.store, TASK, &agent_session("amber-heron"), "submit");
        let submitted = transitions(&f, TASK);
        fake.runtime(&resource, RuntimeState::Failed);
        answered(c.step(&attempt.id));
        assert_eq!(read(&c, &attempt.id).state.ending, Some(Ending::Failed), "kept on its attempt");
        assert_eq!(at(&f, TASK), pair("verify", "checking"));
        assert_eq!(transitions(&f, TASK), submitted);
        let stale = crate::common::refused(f.store.observe_attempt(&attempt.id, &Host::local()));
        assert!(stale.starts_with(&format!("stale_task: {TASK} is at verify/checking")), "{stale}");
        // Once a later attempt holds its lane, it proposes nothing at all.
        c.admit(&owner_grant(), asked(TASK).request_key("k2")).unwrap();
        assert_eq!(f.store.observe_attempt(&attempt.id, &Host::local()).unwrap(), None);
    }

    fn a_success_or_a_stop_moves_nothing(backend: Backend) {
        let custom = attempts(MOVE);
        let f = under(backend, &custom);
        let fake = Fake::default();
        let c = proposing(&f, &fake);
        let (attempt, resource) = working(&f, &c, TASK, "k1");
        let claimed = transitions(&f, TASK);
        fake.runtime(&resource, RuntimeState::Succeeded);
        answered(c.step(&attempt.id));
        assert_eq!(read(&c, &attempt.id).state.ending, Some(Ending::Succeeded));
        assert_eq!(f.store.observe_attempt(&attempt.id, &Host::local()).unwrap(), None);
        // A stop with no cancel asked ends it stopped, which no position answers either.
        let stopped = c.admit(&owner_grant(), asked(TASK).request_key("k2")).unwrap();
        started(&c, &stopped.id);
        c.stop(&owner_grant(), &stopped.id, StopMode::Terminate).unwrap();
        while read(&c, &stopped.id).state.live() {
            answered(c.step(&stopped.id));
        }
        assert_eq!(read(&c, &stopped.id).state.ending, Some(Ending::Stopped));
        assert_eq!(f.store.observe_attempt(&stopped.id, &Host::local()).unwrap(), None);
        assert_eq!(transitions(&f, TASK), claimed, "the task stays where its claim put it");
        assert_eq!(roles(&f, TASK), [("amber-heron".into(), "executor".into())]);
    }

    fn a_cancel_that_ends_an_attempt_at_once_is_acknowledged_by_the_position(backend: Backend) {
        let custom = attempts(MOVE);
        let f = under(backend, &custom);
        let fake = Fake::default();
        let c = proposing(&f, &fake);
        take(&f.store, TASK, &agent_session("amber-heron"), "claim");
        take(&f.store, TASK, &agent_session("amber-heron"), "submit");
        // Nothing was sent for the check: the cancel ends it, and the position sends the task
        // back to build.
        let admission = Admission::new(TASK, "checker", "checker", input("r1"));
        let check = c.admit(&owner_grant(), admission).unwrap();
        let cancelled = c.cancel(&owner_grant(), &check.id).unwrap();
        assert_eq!(
            (cancelled.state.ending, cancelled.state.cancellation),
            (Some(Ending::Cancelled), Cancellation::Confirmed)
        );
        assert!(fake.calls().is_empty());
        assert_eq!(at(&f, TASK), pair("build", "available"));
        let acknowledged = transitions(&f, TASK).last().unwrap().clone();
        let answered = acknowledged.recovery.unwrap();
        assert_eq!(
            (answered.trigger, acknowledged.key, answered.manual),
            (Trigger::CancellationAcknowledged, Some(format!("attempt/{}", check.id)), false)
        );
        assert!(
            recoveries(&f, TASK).last().unwrap().contains(&format!(
                "checker attempt {} ended cancelled; verify/checking → build/available",
                check.id
            )),
            "{:?}",
            recoveries(&f, TASK)
        );
        // Asked again, nothing more.
        let moved = transitions(&f, TASK);
        c.cancel(&owner_grant(), &check.id).unwrap();
        assert_eq!(transitions(&f, TASK), moved);
    }

    fn a_failed_check_takes_the_automatic_edge_until_its_bound_sends_the_task_to_a_person(backend: Backend) {
        let custom = attempts(MOVE);
        let f = under(backend, &custom);
        let fake = Fake::default();
        let c = proposing(&f, &fake);
        let amber = agent_session("amber-heron");
        let to_check = || {
            take(&f.store, OTHER_TASK, &amber, "claim");
            take(&f.store, OTHER_TASK, &amber, "submit");
            assert_eq!(at(&f, OTHER_TASK), pair("verify", "checking"));
        };
        for round in 1..=2 {
            to_check();
            // A start the provider refuses fails the check as a failed run does.
            let check = if round == 1 {
                failed_check(&c, &fake, OTHER_TASK, "check-1")
            } else {
                rejected_check(&c, &fake, OTHER_TASK, "check-2")
            };
            assert_eq!(at(&f, OTHER_TASK), pair("build", "available"), "round {round}");
            let reworked = transitions(&f, OTHER_TASK).last().unwrap().clone();
            assert_eq!(reworked.edge.as_deref(), Some("rework"));
            assert_eq!(reworked.principal, board_service("recovery"));
            assert_eq!(reworked.key.as_deref(), Some(format!("attempt/{}", check.id).as_str()));
            assert_eq!(reworked.recovery.as_ref().unwrap().trigger, Trigger::AttemptFailure);
            assert!(
                f.said(OTHER_TASK).iter().any(|line| line.ends_with(&format!(
                    "verify/checking → build/available, workflow revision {}, on the failure of attempt {}",
                    reworked.revision, check.id
                ))),
                "{:?}",
                f.said(OTHER_TASK)
            );
            // A repeat finds its receipt: one transition.
            assert_eq!(f.store.observe_attempt(&check.id, &Host::local()).unwrap(), Some(reworked));
        }
        // Taken twice, its bound is exhausted: the third failure goes to a person.
        to_check();
        let check = failed_check(&c, &fake, OTHER_TASK, "check-3");
        let stuck = f.store.workflow_position(OTHER_TASK).unwrap().unwrap();
        assert_eq!(stuck.at(), pair("verify", "stuck"));
        assert_eq!(f.store.get(OTHER_TASK).unwrap().status, "blocked");
        let manual = stuck.manual.unwrap();
        assert_eq!((manual.trigger, manual.manual), (Trigger::AttemptFailure, true));
        let exhaustion = transitions(&f, OTHER_TASK).last().unwrap().clone();
        assert_eq!(exhaustion.key, Some(format!("attempt/{}", check.id)));
        let line = recoveries(&f, OTHER_TASK).last().unwrap().clone();
        assert!(
            line.contains(&format!("checker attempt {} failed; verify/checking → verify/stuck", check.id))
                && line.ends_with("; rework's retry bound is exhausted"),
            "{line}"
        );
        // The owner sends it back: the count starts again.
        take(&f.store, OTHER_TASK, &local_owner(), "unstick");
        to_check();
        failed_check(&c, &fake, OTHER_TASK, "check-4");
        assert_eq!(at(&f, OTHER_TASK), pair("build", "available"));
        // No caller is offered the board's edge, or takes it.
        to_check();
        let owner = local_owner();
        let next = f.store.workflow_actions(OTHER_TASK, &owner, &Host::local()).unwrap();
        assert!(next.actions.iter().all(|a| a.edge != "rework"), "{:?}", next.actions);
        let request = OperationRequest {
            project: f.store.database().database().into(),
            task: OTHER_TASK.into(),
            edge: "rework".into(),
            expected: next.snapshot,
            input_revisions: BTreeMap::new(),
            idempotency_key: "by-hand".into(),
            inputs: BTreeMap::new(),
        };
        assert_eq!(
            crate::common::refused(f.store.apply_operation(&owner, &Host::local(), &request)),
            "unsupported_writer: edge rework is automatic: the board takes it, never a caller"
        );
    }

    fn a_failed_check_whose_rework_waits_on_a_dependency_waits_on_a_person_and_spends_no_retry(backend: Backend) {
        let custom = gated(MOVE, Some(stays()));
        let f = waiting(backend, &custom);
        let fake = Fake::default();
        let c = proposing(&f, &fake);
        to_check(&f, OTHER_TASK);
        let check = failed_check(&c, &fake, OTHER_TASK, "check-1");
        // Admission refused the board's take of rework: the position's own action answers it.
        let stayed = f.store.workflow_position(OTHER_TASK).unwrap().unwrap();
        assert_eq!(stayed.at(), pair("verify", "checking"));
        let manual = stayed.manual.clone().expect("a person decides");
        assert_eq!((manual.trigger, manual.manual), (Trigger::AttemptFailure, true));
        let all = transitions(&f, OTHER_TASK);
        let answer = all.last().unwrap().clone();
        assert_eq!(
            (answer.edge.as_deref(), answer.key.clone(), answer.principal.clone()),
            (None, Some(format!("attempt/{}", check.id)), board_service("recovery"))
        );
        assert_eq!(
            recoveries(&f, OTHER_TASK),
            [format!(
                "the board's recovery ({OTHER_TASK}@{revision}): checker attempt {id} failed; stays at \
                 verify/checking, workflow revision {revision}; it waits on a person's recovery \
                 (`just tasks workflow actions {OTHER_TASK}`); the board could not take rework: \
                 dependency: {OTHER_TASK} depends on {PREREQUISITE}, which is at build/available",
                revision = answer.revision,
                id = check.id
            )]
        );
        // A repeat answers the one transition, and says nothing more.
        for _ in 0..2 {
            assert_eq!(f.store.observe_attempt(&check.id, &Host::local()).unwrap(), Some(answer.clone()));
        }
        assert_eq!(transitions(&f, OTHER_TASK), all);
        assert_eq!(recoveries(&f, OTHER_TASK).len(), 1);
        // The fallback was no take of the board's: once the prerequisite is done, its next two
        // failures each take rework within its bound of two.
        prerequisite_done(&f);
        for round in 2..=3 {
            if round == 3 {
                to_check(&f, OTHER_TASK);
            }
            let check = failed_check(&c, &fake, OTHER_TASK, &format!("check-{round}"));
            assert_eq!(at(&f, OTHER_TASK), pair("build", "available"), "round {round}");
            let reworked = transitions(&f, OTHER_TASK).last().unwrap().clone();
            assert_eq!(
                (reworked.edge.as_deref(), reworked.key),
                (Some("rework"), Some(format!("attempt/{}", check.id)))
            );
        }
    }

    fn a_failed_check_whose_rework_waits_on_a_dependency_moves_by_the_positions_action(backend: Backend) {
        let custom = gated(MOVE, Some(back_to_build()));
        let f = waiting(backend, &custom);
        let fake = Fake::default();
        let c = proposing(&f, &fake);
        to_check(&f, OTHER_TASK);
        let check = failed_check(&c, &fake, OTHER_TASK, "check-1");
        assert_eq!(at(&f, OTHER_TASK), pair("build", "available"));
        assert_eq!(f.store.get(OTHER_TASK).unwrap().status, "ready");
        let moved = transitions(&f, OTHER_TASK).last().unwrap().clone();
        assert_eq!((moved.edge.as_deref(), moved.from.clone()), (None, Some(pair("verify", "checking"))));
        assert_eq!(moved.recovery.as_ref().map(|r| (r.trigger, r.manual)), Some((Trigger::AttemptFailure, false)));
        let line = recoveries(&f, OTHER_TASK).last().unwrap().clone();
        assert!(
            line.contains(&format!("checker attempt {} failed; verify/checking → build/available", check.id))
                && line.ends_with("; the board could not take rework: dependency: sim/land depends on sim/base, which is at build/available"),
            "{line}"
        );
        assert_eq!(f.store.observe_attempt(&check.id, &Host::local()).unwrap(), Some(moved));
        assert_eq!(recoveries(&f, OTHER_TASK).len(), 1);
    }

    fn a_failure_nothing_can_answer_is_said_on_its_thread_once_and_that_answers_it(backend: Backend) {
        // verify/checking declares no recovery: with rework refused, nothing can answer a check.
        let custom = gated(MOVE, None);
        let f = waiting(backend, &custom);
        let fake = Fake::default();
        let c = proposing(&f, &fake);
        to_check(&f, OTHER_TASK);
        let checking = transitions(&f, OTHER_TASK);
        let check = failed_check(&c, &fake, OTHER_TASK, "check-1");
        assert_eq!(transitions(&f, OTHER_TASK), checking, "nothing moved");
        assert!(f.store.workflow_position(OTHER_TASK).unwrap().unwrap().manual.is_none());
        let said = [format!(
            "the board's recovery: checker attempt {id} failed, and nothing answers it: the board \
             could not take rework (dependency: {OTHER_TASK} depends on {PREREQUISITE}, which is at \
             build/available), nor its position's attempt_failure action (explicit_action_required: \
             verify/checking declares no attempt_failure recovery: {OTHER_TASK} stays as it was); a \
             person does (`just tasks workflow actions {OTHER_TASK}`)",
            id = check.id
        )];
        assert_eq!(unanswered(&f, OTHER_TASK, &check.id), said);
        // A repeat says nothing more, and the line stays its answer once rework could be taken.
        assert_eq!(f.store.observe_attempt(&check.id, &Host::local()).unwrap(), None);
        prerequisite_done(&f);
        assert_eq!(f.store.observe_attempt(&check.id, &Host::local()).unwrap(), None);
        assert_eq!(transitions(&f, OTHER_TASK), checking);
        assert_eq!(unanswered(&f, OTHER_TASK, &check.id), said);
    }

    fn a_failure_where_no_action_is_declared_is_said_once_and_a_cancel_there_stays_silent(backend: Backend) {
        let custom = attempts(MOVE);
        let f = under(backend, &custom);
        let fake = Fake::default();
        let c = proposing(&f, &fake);
        // build/available has no automatic edge and declares no recovery.
        let failed = c.admit(&owner_grant(), asked(TASK).request_key("k1")).unwrap();
        let resource = started(&c, &failed.id);
        fake.runtime(&resource, RuntimeState::Failed);
        answered(c.step(&failed.id));
        let said = [format!(
            "the board's recovery: attempt {id} of amber-heron's executor failed, and nothing answers \
             it: explicit_action_required: build/available declares no attempt_failure recovery: \
             {TASK} stays as it was; a person does (`just tasks workflow actions {TASK}`)",
            id = failed.id
        )];
        assert_eq!(unanswered(&f, TASK, &failed.id), said);
        assert_eq!(f.store.observe_attempt(&failed.id, &Host::local()).unwrap(), None);
        assert_eq!(unanswered(&f, TASK, &failed.id), said);
        assert_eq!(transitions(&f, TASK).len(), 1, "its placement alone");
        // A cancel that took there is no failure: refused as before, and said nowhere.
        let cancelled = c.admit(&owner_grant(), asked(TASK).request_key("k2")).unwrap();
        c.cancel(&owner_grant(), &cancelled.id).unwrap();
        assert!(
            crate::common::refused(f.store.observe_attempt(&cancelled.id, &Host::local()))
                .starts_with("explicit_action_required: build/available declares no cancellation_acknowledged")
        );
        assert!(recoveries(&f, TASK).iter().all(|line| !line.contains(&cancelled.id)));
    }

    fn a_refused_take_whose_task_moved_meanwhile_moves_nothing_and_says_nothing(backend: Backend) {
        let custom = gated(MOVE, Some(stays()));
        let f = waiting(backend, &custom);
        let fake = Fake::default();
        // A host that grants the board no recovery: the coordinator's own proposal moves nothing.
        let c = coordinator(&f, &fake).with_workflow(Host::default());
        to_check(&f, OTHER_TASK);
        let check = failed_check(&c, &fake, OTHER_TASK, "check-1");
        let checking = transitions(&f, OTHER_TASK);
        // The owner accepts the task between the board's decision and its take of rework.
        let other = f.other();
        let late = f.late(move || take(&other, OTHER_TASK, &local_owner(), "accept"));
        let pinned = checking.last().unwrap().revision;
        // Admission's own read refuses the take as stale: no fallback, no line.
        assert_eq!(
            crate::common::refused(late.observe_attempt(&check.id, &Host::local())),
            format!(
                "stale_task: {OTHER_TASK} is at verify/accepted (workflow revision {}), not \
                 verify/checking (revision {pinned})",
                pinned + 1
            )
        );
        assert_eq!(at(&f, OTHER_TASK), pair("verify", "accepted"));
        assert_eq!(transitions(&f, OTHER_TASK).len(), checking.len() + 1, "the accept alone");
        assert!(recoveries(&f, OTHER_TASK).is_empty(), "{:?}", recoveries(&f, OTHER_TASK));
        // Asked again, the decision itself finds its pin stale: still nothing.
        let again = crate::common::refused(f.store.observe_attempt(&check.id, &Host::local()));
        assert!(again.contains(&format!("where attempt {} was admitted", check.id)), "{again}");
        assert!(recoveries(&f, OTHER_TASK).is_empty());
    }
}
