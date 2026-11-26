//! Execution attempts through the `Coordinator` a host runs, against a
//! deterministic provider ([`fake`]): admission once per request key and lane, intent recorded
//! before the provider hears of it, an ambiguous start reconciled by its key, a lease the
//! database's clock times, answers fenced on it, cancels that race a start, bounded recovery,
//! capacity held until nothing runs, resume at the next runtime epoch, evidence and cleanup, and
//! grants the host alone makes.

mod fake;
mod workflow;

use crate::common::{Fixture, Seed};
use fake::{Fake, Fault, Meanwhile, capabilities};
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};
use symphony_board::execution::{
    Act, Adapter, Admission, Artifact, Attempt, Cancellation, Cleanup, Collected, Collection,
    Continuation, Coordinator, Dispatch, Ending, Fact, Grant, Pacing, Refusal, Relation, Worked,
};
use symphony_plugin::execution::{
    ContentRef, Input, Operation, Outcome, Principal, PrincipalKind, Profile, ResumeMode,
    RuntimeState, Scope, StopMode,
};

const TASK: &str = "sim/jump";
const OTHER_TASK: &str = "sim/land";

fn scope() -> Scope {
    Scope::new("proj".into(), "board".into())
}

fn profile() -> Profile {
    Profile::new(scope(), "fake".into(), capabilities())
}

fn owner() -> Principal {
    Principal::new(PrincipalKind::Human, "local".into(), "owner".into())
}

/// Every act a person may do on the board.
fn owner_grant() -> Grant {
    let acts = [
        Act::Start,
        Act::Cancel,
        Act::Stop,
        Act::Resume,
        Act::Retry,
        Act::Read,
        Act::Link,
        Act::Collect,
        Act::Cleanup,
    ];
    Grant::from_host(owner(), scope(), &acts, "policy-1")
}

fn worker() -> Grant {
    let service = Principal::new(PrincipalKind::Service, "board".into(), "worker".into());
    Grant::from_host(service, scope(), &[Act::Work], "policy-1")
}

fn input(revision: &str) -> Input {
    Input::new(
        revision.into(),
        format!("digest-of-{revision}"),
        "commit-abc".into(),
        ContentRef::new("manifest".into(), "manifest-digest".into()),
    )
}

/// A board with the tasks attempts run for.
fn board() -> Fixture {
    let f = Fixture::new(Seed::default());
    f.task(TASK);
    f.task(OTHER_TASK);
    f
}

fn coordinator<'a>(f: &'a Fixture, adapter: &'a dyn Adapter) -> Coordinator<'a> {
    Coordinator::new(&f.store, profile(), adapter, worker()).expect("the worker may work")
}

fn asked(task: &str) -> Admission {
    Admission::new(task, "executor", "executor", input("r1")).handle("amber-heron")
}

fn admit(c: &Coordinator, key: &str) -> Attempt {
    c.admit(&owner_grant(), asked(TASK).request_key(key))
        .expect("admitted")
}

fn read(c: &Coordinator, id: &str) -> Attempt {
    c.attempt(&owner_grant(), id).expect("readable")
}

/// What one worker's step answered.
fn answered(worked: Result<Worked, Refusal>) -> (Operation, Outcome) {
    match worked.expect("a step") {
        Worked::Answered { operation, outcome } => (operation, outcome),
        other => panic!("expected an answer, got {other:?}"),
    }
}

/// The kinds of attempt `id`'s facts, in order.
fn kinds(c: &Coordinator, id: &str) -> Vec<String> {
    (c.facts(&owner_grant(), id).expect("facts"))
        .iter()
        .map(|recorded| {
            to_json(&recorded.fact)["kind"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

fn facts(c: &Coordinator, id: &str) -> Vec<Fact> {
    (c.facts(&owner_grant(), id).expect("facts"))
        .into_iter()
        .map(|recorded| recorded.fact)
        .collect()
}

fn to_json(value: &impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

/// Attempt `id`'s lease run out, as the database's clock would see it once its worker died.
fn age_lease(f: &Fixture, id: &str) {
    let row = f.store.raw().row("execution_attempt", id);
    let mut lease = row.field("lease").unwrap();
    assert!(lease.is_object(), "a lease is held: {lease}");
    lease["expires"] = json!(0);
    row.set(json!({ "lease": lease })).unwrap();
}

/// `step` run to the worker's death.
fn dies<T>(step: impl FnOnce() -> T) {
    let died = catch_unwind(AssertUnwindSafe(step));
    assert!(died.is_err(), "the worker died");
}

fn is_unknown(outcome: &Outcome) -> bool {
    matches!(outcome, Outcome::Unknown(_))
}

/// Attempt `id` run from its start to observed `state`.
fn started(c: &Coordinator, id: &str) -> String {
    let (operation, outcome) = answered(c.step(id));
    assert_eq!(operation, Operation::Start);
    assert!(matches!(outcome, Outcome::Accepted(_)), "{outcome:?}");
    read(c, id).state.resource.expect("bound").id
}

#[test]
fn an_attempt_is_admitted_once_per_request_key_and_once_per_lane() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake);
    let first = admit(&c, "k1");
    assert_eq!(
        admit(&c, "k1"),
        first,
        "the same key admits the same attempt"
    );
    assert_eq!(
        c.admit(&owner_grant(), asked(TASK).request_key("k2")),
        Err(Refusal::Busy(Some(first.id.clone())))
    );
    let other = Admission::new(TASK, "reviewer", "reviewer", input("r1")).request_key("k1");
    assert!(matches!(
        c.admit(&owner_grant(), other),
        Err(Refusal::Conflict(_))
    ));
    assert!(matches!(
        c.admit(&owner_grant(), asked("sim/none")),
        Err(Refusal::Invalid(why)) if why.contains("no task sim/none")
    ));

    assert_eq!(first.id.len(), 32);
    assert!(first.current);
    assert_eq!(first.state.dispatch, Dispatch::Pending);
    assert_eq!(first.identity.input, input("r1"));
    assert_eq!(first.identity.initiator, owner());
    assert_eq!(first.identity.handle.as_deref(), Some("amber-heron"));
    let pending = first.state.pending.clone().expect("its start owed");
    assert_eq!(
        (pending.kind, pending.sequence, pending.sent),
        (Operation::Start, 1, false)
    );
    assert_eq!(kinds(&c, &first.id), ["admitted", "intent"]);
    assert_eq!(
        f.store.live_attempts(TASK).unwrap(),
        std::slice::from_ref(&first.id)
    );
    assert!(fake.calls().is_empty(), "admission sends nothing");

    // Another lane of the task, or another task, is its own.
    let reviewer = Admission::new(TASK, "reviewer", "reviewer", input("r1"));
    assert!(c.admit(&owner_grant(), reviewer).is_ok());
    assert!(c.admit(&owner_grant(), asked(OTHER_TASK)).is_ok());
}

#[test]
fn the_worker_records_its_send_before_the_provider_hears_and_follows_it_to_its_end() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake);
    let attempt = admit(&c, "k1");
    let resource = started(&c, &attempt.id);
    assert_eq!(
        fake.keys(Operation::Start),
        [attempt.state.pending.unwrap().key]
    );
    assert_eq!(
        kinds(&c, &attempt.id),
        ["admitted", "intent", "sent", "answer", "bound"]
    );
    assert_eq!(read(&c, &attempt.id).state.dispatch, Dispatch::Starting);

    let (operation, _) = answered(c.step(&attempt.id));
    assert_eq!(operation, Operation::Observe);
    let running = read(&c, &attempt.id).state;
    assert_eq!(running.dispatch, Dispatch::Running);
    // Its session's link, kept with the attempt after its handle leaves.
    assert_eq!(
        running.conversation.as_deref(),
        Some(format!("conversation-of-{resource}").as_str())
    );

    fake.runtime(&resource, RuntimeState::Succeeded);
    answered(c.step(&attempt.id));
    let ended = read(&c, &attempt.id).state;
    assert_eq!(
        (ended.dispatch, ended.ending),
        (Dispatch::Terminal, Some(Ending::Succeeded))
    );
    assert!(f.store.live_attempts(TASK).unwrap().is_empty());
    assert_eq!(c.step(&attempt.id), Ok(Worked::Waited));
    assert!(
        c.run_due(10).unwrap().is_empty(),
        "nothing is due of an ended attempt"
    );
    assert_eq!(fake.resources(), 1);
}

#[test]
fn a_lost_response_is_reconciled_by_its_key_and_never_starts_twice() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake);
    let attempt = admit(&c, "k1");
    fake.script([Fault::Lost]);
    let (_, outcome) = answered(c.step(&attempt.id));
    assert!(is_unknown(&outcome));
    let unknown = read(&c, &attempt.id).state;
    assert_eq!(unknown.dispatch, Dispatch::Unknown);
    assert!(unknown.pending.as_ref().is_some_and(|owed| owed.sent));

    let (operation, outcome) = answered(c.step(&attempt.id));
    assert_eq!(operation, Operation::Reconcile);
    assert!(matches!(outcome, Outcome::Found(_)), "{outcome:?}");
    let bound = read(&c, &attempt.id).state;
    assert_eq!(bound.dispatch, Dispatch::Starting);
    assert_eq!(bound.pending, None);
    assert_eq!(fake.resources(), 1);
    assert_eq!(fake.keys(Operation::Reconcile), fake.keys(Operation::Start));
}

#[test]
fn a_start_that_never_reached_the_provider_is_sent_again_by_the_same_key() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake);
    let attempt = admit(&c, "k1");
    fake.script([Fault::Down]);
    assert!(is_unknown(&answered(c.step(&attempt.id)).1));
    assert!(matches!(
        answered(c.step(&attempt.id)).1,
        Outcome::Absent { .. }
    ));
    assert!(!read(&c, &attempt.id).state.pending.unwrap().sent);
    started(&c, &attempt.id);
    let key = attempt.state.pending.unwrap().key;
    assert_eq!(fake.keys(Operation::Start), std::slice::from_ref(&key));
    assert_eq!(fake.keys(Operation::Reconcile), [key]);
    assert_eq!(fake.resources(), 1);
}

#[test]
fn a_worker_that_dies_after_recording_its_send_is_taken_up_once_its_lease_runs_out() {
    let f = board();
    let fake = Fake::default();
    let (dead, next) = (coordinator(&f, &fake), coordinator(&f, &fake));
    let attempt = admit(&dead, "k1");
    fake.script([Fault::CrashBefore]);
    dies(|| dead.step(&attempt.id));
    assert!(
        read(&next, &attempt.id).state.pending.unwrap().sent,
        "its send was recorded first"
    );
    assert_eq!(
        next.step(&attempt.id),
        Ok(Worked::Leased),
        "the dead worker's lease holds"
    );

    age_lease(&f, &attempt.id);
    assert!(matches!(
        answered(next.step(&attempt.id)).1,
        Outcome::Absent { .. }
    ));
    started(&next, &attempt.id);
    assert_eq!(fake.resources(), 1);
    let epochs: Vec<(u64, bool)> = (facts(&next, &attempt.id).iter())
        .filter_map(|fact| match fact {
            Fact::Sent {
                epoch, reconcile, ..
            } => Some((*epoch, *reconcile)),
            _ => None,
        })
        .collect();
    // Each lease taken is the next epoch: the dead worker's, the reconcile's, the resend's,
    // each call recorded before it was made.
    assert_eq!(epochs, [(1, false), (2, true), (3, false)]);
}

#[test]
fn a_worker_that_dies_after_the_provider_acted_finds_what_it_made() {
    let f = board();
    let fake = Fake::default();
    let (dead, next) = (coordinator(&f, &fake), coordinator(&f, &fake));
    let attempt = admit(&dead, "k1");
    fake.script([Fault::CrashAfter]);
    dies(|| dead.step(&attempt.id));
    age_lease(&f, &attempt.id);
    assert!(matches!(
        answered(next.step(&attempt.id)).1,
        Outcome::Found(_)
    ));
    assert!(read(&next, &attempt.id).state.resource.is_some());
    assert_eq!(fake.resources(), 1);
}

#[test]
fn an_answer_after_its_lease_was_taken_over_is_kept_stale_and_changes_nothing() {
    let f = board();
    let fake = Fake::default();
    let next = coordinator(&f, &fake);
    let attempt = admit(&next, "k1");
    let id = attempt.id.clone();
    let (f_ref, next_ref, id_ref) = (&f, &next, id.as_str());
    let slow = Meanwhile::new(&fake, Operation::Start, move || {
        assert_eq!(
            next_ref.step(id_ref),
            Ok(Worked::Leased),
            "a second worker waits"
        );
        age_lease(f_ref, id_ref);
        let (operation, outcome) = answered(next_ref.step(id_ref));
        assert_eq!(operation, Operation::Reconcile);
        assert!(matches!(outcome, Outcome::Found(_)), "{outcome:?}");
    });
    let first = coordinator(&f, &slow);
    match first.step(&id) {
        Ok(Worked::Fenced {
            operation: Operation::Start,
            outcome: Outcome::Accepted(_),
        }) => {}
        other => panic!("expected the late answer fenced, got {other:?}"),
    }
    let facts = facts(&next, &id);
    let stale = facts
        .iter()
        .filter(|fact| matches!(fact, Fact::Answer { stale: true, .. }));
    assert_eq!(stale.count(), 1);
    let bound = facts.iter().filter(|fact| {
        matches!(
            fact,
            Fact::Bound {
                resource: Some(_),
                ..
            }
        )
    });
    assert_eq!(bound.count(), 1, "only the lease's holder bound it");
    assert_eq!(read(&next, &id).state.dispatch, Dispatch::Starting);
    assert_eq!(fake.resources(), 1);
}

#[test]
fn a_cancel_while_the_start_is_in_flight_cancels_what_it_made() {
    let f = board();
    let fake = Fake::default();
    let caller = coordinator(&f, &fake);
    let attempt = admit(&caller, "k1");
    let (caller_ref, id) = (&caller, attempt.id.as_str());
    let racing = Meanwhile::new(&fake, Operation::Start, move || {
        let asked = caller_ref.cancel(&owner_grant(), id).expect("asked");
        assert_eq!(asked.state.cancellation, Cancellation::Requested);
    });
    let c = coordinator(&f, &racing);
    let resource = started(&c, id);
    let (operation, outcome) = answered(c.step(id));
    assert_eq!(operation, Operation::Cancel);
    assert!(matches!(outcome, Outcome::Accepted(_)));
    assert_eq!(fake.runtime_of(&resource), RuntimeState::Terminated);
    assert_eq!(
        read(&c, id).state.dispatch,
        Dispatch::Starting,
        "taken, not yet seen to cease"
    );
    answered(c.step(id));
    let ended = read(&c, id).state;
    assert_eq!(ended.ending, Some(Ending::Cancelled));
    assert_eq!(ended.cancellation, Cancellation::Confirmed);
    assert!(!c.eligibility(&owner_grant(), id, "r1").unwrap().eligible);
}

#[test]
fn a_cancel_before_anything_was_sent_ends_it_with_nothing_provisioned() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake).with_capacity("pool", 1);
    let attempt = admit(&c, "k1");
    let cancelled = c.cancel(&owner_grant(), &attempt.id).unwrap();
    assert_eq!(cancelled.state.ending, Some(Ending::Cancelled));
    assert_eq!(
        c.cancel(&owner_grant(), &attempt.id).unwrap(),
        cancelled,
        "asked once"
    );
    assert_eq!(c.step(&attempt.id), Ok(Worked::Waited));
    assert!(fake.calls().is_empty());
    assert!(
        c.admit(&owner_grant(), asked(OTHER_TASK)).is_ok(),
        "its place was given back"
    );
}

#[test]
fn answers_that_settle_nothing_hold_the_attempt_for_a_person_who_alone_retries_it() {
    let f = board();
    let fake = Fake::default();
    let mut pacing = Pacing::default();
    pacing.unknowns = 2;
    let c = coordinator(&f, &fake).with_pacing(pacing).unwrap();
    let held = admit(&c, "k1");
    fake.script([Fault::Down, Fault::Down, Fault::Down]);
    for _ in 0..3 {
        assert!(is_unknown(&answered(c.step(&held.id)).1));
    }
    let state = read(&c, &held.id).state;
    assert_eq!(state.dispatch, Dispatch::Held);
    assert!(state.held.unwrap().contains("settled nothing"));
    assert!(matches!(c.step(&held.id), Ok(Worked::Waited)));
    assert_eq!(
        f.store.live_attempts(TASK).unwrap(),
        std::slice::from_ref(&held.id),
        "held is not ended"
    );
    assert!(matches!(
        c.admit(&owner_grant(), asked(TASK)),
        Err(Refusal::Busy(_))
    ));

    let agent = Principal::new(PrincipalKind::Agent, "local".into(), "amber-heron".into());
    let agents = Grant::from_host(agent, scope(), &[Act::Retry, Act::Read], "policy-1");
    assert!(matches!(
        c.retry(&agents, &held.id, "found it gone", None),
        Err(Refusal::Unauthorized(_))
    ));
    assert!(matches!(
        c.retry(&owner_grant(), &held.id, " ", None),
        Err(Refusal::Invalid(_))
    ));
    let retry = c
        .retry(
            &owner_grant(),
            &held.id,
            "the provider's console shows nothing",
            Some("r1"),
        )
        .expect("a person retries");
    assert_eq!(
        c.retry(
            &owner_grant(),
            &held.id,
            "the provider's console shows nothing",
            Some("r1")
        ),
        Ok(retry.clone()),
        "a repeat of the retry is the same attempt"
    );
    assert_eq!(
        retry.identity.predecessor.as_deref(),
        Some(held.id.as_str())
    );
    assert_eq!(retry.identity.relation, Some(Relation::Retry));
    assert_eq!(retry.identity.runtime_epoch, 1);
    let before = read(&c, &held.id).state;
    assert_eq!(before.ending, Some(Ending::Superseded));
    assert_eq!(before.successor.as_deref(), Some(retry.id.as_str()));
    let history = c.history(&owner_grant(), TASK).unwrap();
    let mut lineage: Vec<(&str, bool)> = history
        .iter()
        .map(|attempt| (attempt.id.as_str(), attempt.current))
        .collect();
    symphony_board::order::sort_by(&mut lineage, &Ord::cmp);
    let mut expected = vec![(held.id.as_str(), false), (retry.id.as_str(), true)];
    symphony_board::order::sort_by(&mut expected, &Ord::cmp);
    assert_eq!(lineage, expected);
    assert_eq!(
        kinds(&c, &held.id).last().map(String::as_str),
        Some("ended")
    );
}

#[test]
fn conflicting_resources_are_quarantined() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake);
    let attempt = admit(&c, "k1");
    fake.script([Fault::Lost, Fault::Twice]);
    answered(c.step(&attempt.id));
    assert!(matches!(
        answered(c.step(&attempt.id)).1,
        Outcome::Conflicting { .. }
    ));
    let state = read(&c, &attempt.id).state;
    assert_eq!(state.dispatch, Dispatch::Held);
    assert!(state.held.unwrap().contains("2 resources"));
    assert_eq!(state.resource, None, "neither is promoted");
}

#[test]
fn capacity_is_held_until_nothing_of_an_attempt_runs_even_across_a_dead_worker() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake).with_capacity("pool", 1);
    let first = admit(&c, "k1");
    let full = c.admit(&owner_grant(), asked(OTHER_TASK));
    assert_eq!(full, Err(Refusal::Full(vec![first.id.clone()])));

    fake.script([Fault::CrashAfter]);
    dies(|| c.step(&first.id));
    age_lease(&f, &first.id);
    assert!(matches!(
        c.admit(&owner_grant(), asked(OTHER_TASK)),
        Err(Refusal::Full(_))
    ));
    answered(c.step(&first.id));
    let resource = read(&c, &first.id).state.resource.unwrap().id;
    fake.runtime(&resource, RuntimeState::Failed);
    answered(c.step(&first.id));
    assert_eq!(read(&c, &first.id).state.ending, Some(Ending::Failed));
    let second = c
        .admit(&owner_grant(), asked(OTHER_TASK))
        .expect("its place was given back");

    // A start the provider refuses gives its place back too.
    fake.script([Fault::Refuse("quota")]);
    answered(c.step(&second.id));
    assert_eq!(read(&c, &second.id).state.ending, Some(Ending::Rejected));
    let reviewer = Admission::new(TASK, "reviewer", "reviewer", input("r1"));
    assert!(c.admit(&owner_grant(), reviewer).is_ok());
}

#[test]
fn a_paused_attempt_resumes_as_another_at_the_next_runtime_epoch() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake);
    let paused = admit(&c, "k1");
    let resource = started(&c, &paused.id);
    assert!(matches!(
        c.stop(&owner_grant(), &paused.id, StopMode::HardStop),
        Err(Refusal::Invalid(why)) if why.contains("hard-stop")
    ));
    c.stop(&owner_grant(), &paused.id, StopMode::Pause).unwrap();
    assert_eq!(answered(c.step(&paused.id)).0, Operation::Stop);
    answered(c.step(&paused.id));
    assert_eq!(read(&c, &paused.id).state.dispatch, Dispatch::Paused);

    let continuation = Continuation {
        checkpoint: None,
        conversation: Some(format!("conversation-of-{resource}")),
        mode: ResumeMode::ReuseWithNewGeneration,
    };
    let resumed = c
        .resume(
            &owner_grant(),
            &paused.id,
            continuation.clone(),
            Some("resume-1"),
        )
        .expect("resumed");
    assert_eq!(
        c.resume(
            &owner_grant(),
            &paused.id,
            continuation.clone(),
            Some("resume-1")
        ),
        Ok(resumed.clone()),
        "a repeat of the resume is the same attempt"
    );
    assert!(matches!(
        c.resume(&owner_grant(), &paused.id, continuation, Some("resume-2")),
        Err(Refusal::Invalid(why)) if why.contains("followed it already")
    ));
    assert_eq!(resumed.identity.runtime_epoch, 1);
    assert_eq!(resumed.identity.relation, Some(Relation::Resume));
    assert_eq!(
        resumed.state.pending.as_ref().unwrap().kind,
        Operation::Resume
    );
    let before = read(&c, &paused.id);
    assert_eq!(
        (before.state.ending, before.current),
        (Some(Ending::Superseded), false)
    );

    // The resume's answer is lost: it is reconciled as a resume, by its key, never sent anew.
    fake.script([Fault::Lost]);
    let (operation, outcome) = answered(c.step(&resumed.id));
    assert_eq!(operation, Operation::Resume);
    assert!(is_unknown(&outcome), "{outcome:?}");
    let (operation, outcome) = answered(c.step(&resumed.id));
    assert_eq!(operation, Operation::Reconcile);
    assert!(matches!(outcome, Outcome::Found(_)), "{outcome:?}");
    let bound = read(&c, &resumed.id).state.resource.unwrap();
    assert_eq!(
        (bound.id.as_str(), bound.generation.as_str()),
        (resource.as_str(), "g1+")
    );
    assert_eq!(fake.keys(Operation::Resume).len(), 1);
    assert_eq!(
        fake.keys(Operation::Reconcile),
        fake.keys(Operation::Resume)
    );
}

#[test]
fn a_lost_answer_from_a_provider_that_cannot_reconcile_is_held_unsent() {
    let f = board();
    let fake = Fake::default();
    let mut capabilities = capabilities();
    capabilities
        .operations
        .retain(|kind| *kind != Operation::Reconcile);
    let profile = Profile::new(scope(), "fake".into(), capabilities);
    let c = Coordinator::new(&f.store, profile, &fake, worker()).unwrap();
    let attempt = admit(&c, "k1");
    fake.script([Fault::Lost]);
    assert!(is_unknown(&answered(c.step(&attempt.id)).1));
    let Ok(Worked::Held(why)) = c.step(&attempt.id) else {
        panic!("held");
    };
    assert!(why.contains("cannot reconcile"), "{why}");
    let held = read(&c, &attempt.id).state;
    assert_eq!(
        (held.dispatch, held.held.as_deref()),
        (Dispatch::Held, Some(why.as_str()))
    );
    assert_eq!(
        c.step(&attempt.id),
        Ok(Worked::Waited),
        "a held attempt waits for a person"
    );
    assert_eq!(fake.keys(Operation::Start).len(), 1, "never sent twice");
    assert!(fake.keys(Operation::Reconcile).is_empty());
    assert_eq!(
        f.store.live_attempts(TASK).unwrap(),
        std::slice::from_ref(&attempt.id)
    );
}

#[test]
fn only_a_complete_collection_from_the_current_runtime_is_eligible_then_it_is_cleaned() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake);
    let attempt = admit(&c, "k1");
    let id = attempt.id.as_str();
    let resource = started(&c, id);
    let grant = owner_grant();
    assert!(
        matches!(c.begin_collection(&grant, id), Err(Refusal::Invalid(_))),
        "it runs"
    );
    fake.runtime(&resource, RuntimeState::Succeeded);
    answered(c.step(id));
    let reasons = c.eligibility(&grant, id, "r1").unwrap().reasons;
    assert_eq!(reasons, ["its artifacts are not all collected: NotStarted"]);
    assert!(
        matches!(c.begin_cleanup(&grant, id), Err(Refusal::Invalid(_))),
        "collect first"
    );

    c.begin_collection(&grant, id).unwrap();
    let artifact = |name: &str, digest: &str, generation: &str, epoch: u64| Artifact {
        name: name.into(),
        reference: format!("store://{name}"),
        digest: digest.into(),
        size: 12,
        generation: generation.into(),
        runtime_epoch: epoch,
        verified: true,
    };
    let report = artifact("logs/run.txt", "d1", "g1", 0);
    assert_eq!(
        c.record_artifact(&grant, id, report.clone()),
        Ok(Collected::Kept)
    );
    assert_eq!(
        c.record_artifact(&grant, id, report),
        Ok(Collected::Already)
    );
    assert!(matches!(
        c.record_artifact(&grant, id, artifact("logs/run.txt", "d2", "g1", 0)),
        Err(Refusal::Conflict(_))
    ));
    assert_eq!(
        c.record_artifact(&grant, id, artifact("old.txt", "d3", "g0", 0)),
        Ok(Collected::Stale)
    );
    assert_eq!(
        c.record_artifact(&grant, id, artifact("late.txt", "d4", "g1", 7)),
        Ok(Collected::Stale)
    );
    assert!(matches!(
        c.record_artifact(&grant, id, artifact("../up", "d5", "g1", 0)),
        Err(Refusal::Invalid(_))
    ));
    let kept: Vec<String> = c
        .artifacts(&grant, id)
        .unwrap()
        .into_iter()
        .map(|a| a.name)
        .collect();
    assert_eq!(kept, ["logs/run.txt"], "a stale artifact is a fact alone");

    c.end_collection(&grant, id, Collection::Complete, None)
        .unwrap();
    assert!(c.eligibility(&grant, id, "r1").unwrap().eligible);
    let other_revision = c.eligibility(&grant, id, "r2").unwrap();
    assert_eq!(
        other_revision.reasons,
        ["it ran on task revision r1, not r2"]
    );

    let key = c.begin_cleanup(&grant, id).unwrap();
    assert_eq!(
        c.begin_cleanup(&grant, id),
        Ok(key.clone()),
        "the same cleanup under way"
    );
    assert!(matches!(
        c.end_cleanup(&grant, id, "another", Cleanup::Complete, None),
        Err(Refusal::Conflict(_))
    ));
    assert!(read(&c, id).state.open());
    let cleaned = c
        .end_cleanup(&grant, id, &key, Cleanup::Complete, Some("gone"))
        .unwrap();
    assert!(!cleaned.state.open(), "nothing is owed for it");
    assert!(matches!(
        c.begin_cleanup(&grant, id),
        Err(Refusal::Invalid(_))
    ));
}

#[test]
fn a_partial_or_unverified_collection_is_not_eligible() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake);
    let grant = owner_grant();
    let attempt = admit(&c, "k1");
    let resource = started(&c, &attempt.id);
    fake.runtime(&resource, RuntimeState::Succeeded);
    answered(c.step(&attempt.id));
    c.begin_collection(&grant, &attempt.id).unwrap();
    assert!(matches!(
        c.end_collection(&grant, &attempt.id, Collection::Complete, None),
        Err(Refusal::Invalid(why)) if why.contains("no artifact")
    ));
    let unverified = Artifact {
        name: "report.json".into(),
        reference: "store://report".into(),
        digest: "d".into(),
        size: 1,
        generation: "g1".into(),
        runtime_epoch: 0,
        verified: false,
    };
    c.record_artifact(&grant, &attempt.id, unverified).unwrap();
    c.end_collection(&grant, &attempt.id, Collection::Partial, Some("one of two"))
        .unwrap();
    let reasons = c.eligibility(&grant, &attempt.id, "r1").unwrap().reasons;
    assert_eq!(
        reasons,
        [
            "its artifacts are not all collected: Partial",
            "artifact report.json is not verified"
        ]
    );
}

#[test]
fn a_grant_the_host_made_is_the_only_authority_and_no_attempt_crosses_its_project() {
    let f = board();
    let fake = Fake::default();
    let reader = Grant::from_host(owner(), scope(), &[Act::Read], "policy-1");
    assert!(matches!(
        Coordinator::new(&f.store, profile(), &fake, reader.clone()),
        Err(Refusal::Unauthorized(_))
    ));
    let elsewhere = Scope::new("another-project".into(), "board".into());
    let foreign = Grant::from_host(owner(), elsewhere.clone(), &[Act::Work], "policy-1");
    assert!(matches!(
        Coordinator::new(&f.store, profile(), &fake, foreign),
        Err(Refusal::Scope)
    ));
    let some_tasks = worker().on_tasks(&[TASK]);
    assert!(
        matches!(
            Coordinator::new(&f.store, profile(), &fake, some_tasks),
            Err(Refusal::Unauthorized(_))
        ),
        "a worker acts board-wide or not at all"
    );

    let c = coordinator(&f, &fake);
    assert!(matches!(
        c.admit(&reader, asked(TASK)),
        Err(Refusal::Unauthorized(_))
    ));
    let other_task =
        Grant::from_host(owner(), scope(), &[Act::Start], "policy-1").on_tasks(&[OTHER_TASK]);
    assert!(matches!(
        c.admit(&other_task, asked(TASK)),
        Err(Refusal::Unauthorized(_))
    ));
    assert!(c.admit(&other_task, asked(OTHER_TASK)).is_ok());
    let unrevised = Grant::from_host(owner(), scope(), &[Act::Start], "");
    assert!(matches!(
        c.admit(&unrevised, asked(TASK)),
        Err(Refusal::Unauthorized(_))
    ));
    let foreign_owner = Grant::from_host(owner(), elsewhere.clone(), &[Act::Start], "policy-1");
    assert!(matches!(
        c.admit(&foreign_owner, asked(TASK)),
        Err(Refusal::Scope)
    ));

    let attempt = admit(&c, "k1");
    started(&c, &attempt.id);
    assert!(matches!(
        c.cancel(&reader, &attempt.id),
        Err(Refusal::Unauthorized(_))
    ));
    assert!(matches!(
        c.external_link(&reader, &attempt.id),
        Err(Refusal::Unauthorized(_))
    ));
    let link = c
        .external_link(&owner_grant(), &attempt.id)
        .expect("a view");
    assert!(link.url.starts_with("https://provider.invalid/"));

    // Another project's coordinator on the same database neither reads nor steps it.
    let theirs_worker = Grant::from_host(owner(), elsewhere.clone(), &[Act::Work], "policy-1");
    let theirs = Coordinator::new(
        &f.store,
        Profile::new(elsewhere.clone(), "fake".into(), capabilities()),
        &fake,
        theirs_worker,
    )
    .unwrap();
    let theirs_owner = Grant::from_host(owner(), elsewhere, &[Act::Read, Act::Cancel], "policy-1");
    assert_eq!(
        theirs.attempt(&theirs_owner, &attempt.id),
        Err(Refusal::Scope)
    );
    assert_eq!(
        theirs.cancel(&theirs_owner, &attempt.id),
        Err(Refusal::Scope)
    );
    assert_eq!(theirs.step(&attempt.id), Err(Refusal::Scope));
    assert_eq!(
        c.attempt(&owner_grant(), "not-an-attempt"),
        Err(Refusal::Unknown("not-an-attempt".into()))
    );
}

#[test]
fn run_due_steps_what_is_due_under_its_configuration_alone() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake);
    let first = admit(&c, "k1");
    let second = c.admit(&owner_grant(), asked(OTHER_TASK)).unwrap();
    let other_configuration = Coordinator::new(
        &f.store,
        Profile::new(scope(), "another-configuration".into(), capabilities()),
        &fake,
        worker(),
    )
    .unwrap();
    assert!(other_configuration.run_due(10).unwrap().is_empty());
    let mut stepped: Vec<String> = c
        .run_due(10)
        .unwrap()
        .into_iter()
        .map(|(id, worked)| {
            assert!(matches!(
                worked,
                Ok(Worked::Answered {
                    operation: Operation::Start,
                    ..
                })
            ));
            id
        })
        .collect();
    symphony_board::order::sort_by(&mut stepped, &Ord::cmp);
    let mut admitted = vec![first.id, second.id];
    symphony_board::order::sort_by(&mut admitted, &Ord::cmp);
    assert_eq!(stepped, admitted);
    assert!(
        c.run_due(10).unwrap().is_empty(),
        "each is next due when it is observed"
    );
}

#[test]
fn a_copy_carries_attempts_and_their_fences_and_recovery_continues_on_it() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake).with_capacity("pool", 1);
    let attempt = admit(&c, "k1");
    fake.script([Fault::Lost]);
    answered(c.step(&attempt.id));
    let dump = f.store.dump().unwrap();
    assert_eq!(dump.execution_attempts.len(), 1);
    assert_eq!(
        dump.execution_fences.len(),
        3,
        "its request, lane and capacity"
    );

    let copy = Fixture::new(Seed { dump: dump.clone() });
    assert_eq!(copy.store.dump().unwrap(), dump);
    let there = coordinator(&copy, &fake).with_capacity("pool", 1);
    assert_eq!(
        admit(&there, "k1").id,
        attempt.id,
        "its request key holds on the copy"
    );
    assert!(matches!(
        there.admit(&owner_grant(), asked(OTHER_TASK)),
        Err(Refusal::Full(_))
    ));
    assert!(matches!(
        answered(there.step(&attempt.id)).1,
        Outcome::Found(_)
    ));
    assert_eq!(fake.resources(), 1);
}

/// A provider that gives no view and resumes only on a new resource.
fn viewless() -> Profile {
    let mut capabilities = capabilities();
    capabilities.external_view = false;
    capabilities.resume_modes = vec![ResumeMode::NewResource];
    Profile::new(scope(), "fake".into(), capabilities)
}

#[test]
fn pacing_profiles_and_admissions_are_checked_before_anything_is_written() {
    let f = Fixture::new(Seed::default().task(TASK, "ready").task("sim/done", "done"));
    let fake = Fake::default();
    let millis = std::time::Duration::from_millis;
    let paced = |change: &dyn Fn(&mut Pacing)| {
        let mut pacing = Pacing::default();
        change(&mut pacing);
        coordinator(&f, &fake).with_pacing(pacing).map(|_| ())
    };
    assert_eq!(paced(&|_| {}), Ok(()));
    for (why, change) in [
        (
            "no time to call",
            &(|p: &mut Pacing| p.call = millis(0)) as &dyn Fn(&mut Pacing),
        ),
        ("past the kit's bound", &|p: &mut Pacing| {
            p.call = millis(30_001)
        }),
        ("a lease shorter than its call", &|p: &mut Pacing| {
            p.lease = p.call
        }),
        ("never observed", &|p: &mut Pacing| p.observe = millis(0)),
        ("no backoff", &|p: &mut Pacing| p.backoff = millis(0)),
        ("a cap under the first wait", &|p: &mut Pacing| {
            p.backoff_cap = millis(1)
        }),
    ] {
        assert!(matches!(paced(change), Err(Refusal::Invalid(_))), "{why}");
    }
    let unnamed = Profile::new(scope(), String::new(), capabilities());
    assert!(matches!(
        Coordinator::new(&f.store, unnamed, &fake, worker()),
        Err(Refusal::Invalid(_))
    ));

    let c = coordinator(&f, &fake);
    let grant = owner_grant();
    let blank_lane = Admission::new(TASK, " ", "executor", input("r1"));
    let mut no_digest = input("r1");
    no_digest.digest = String::new();
    for refused in [
        blank_lane,
        Admission::new(TASK, "executor", "executor", no_digest),
        asked(TASK).request_key(""),
    ] {
        assert!(matches!(c.admit(&grant, refused), Err(Refusal::Invalid(_))));
    }
    assert!(matches!(
        c.admit(&grant, asked("sim/done")),
        Err(Refusal::Invalid(why)) if why.contains("is done")
    ));
    assert!(
        f.store.dump().unwrap().execution_attempts.is_empty(),
        "nothing was written"
    );
}

#[test]
fn controls_refuse_what_does_not_hold_for_the_attempt() {
    let f = board();
    let fake = Fake::default();
    let c = Coordinator::new(&f.store, viewless(), &fake, worker()).unwrap();
    let grant = owner_grant();
    let attempt = admit(&c, "k1");
    let id = attempt.id.as_str();
    assert!(matches!(
        c.stop(&grant, id, StopMode::Pause),
        Err(Refusal::Invalid(why)) if why.contains("cancel it instead")
    ));
    assert!(
        matches!(c.external_link(&grant, id), Err(Refusal::Invalid(_))),
        "nothing bound"
    );
    let resource = started(&c, id);
    assert!(matches!(
        c.external_link(&grant, id),
        Err(Refusal::Invalid(why)) if why.contains("no view")
    ));
    assert!(matches!(
        c.retry(&grant, id, "it is running", None),
        Err(Refusal::Invalid(why)) if why.contains("neither held nor ended")
    ));
    let nothing = Continuation {
        checkpoint: None,
        conversation: None,
        mode: ResumeMode::NewResource,
    };
    assert!(matches!(
        c.resume(&grant, id, nothing, None),
        Err(Refusal::Invalid(_))
    ));
    let running = Continuation {
        checkpoint: Some(ContentRef::new("checkpoint".into(), "cd".into())),
        conversation: None,
        mode: ResumeMode::NewResource,
    };
    assert!(matches!(
        c.resume(&grant, id, running, None),
        Err(Refusal::Invalid(why)) if why.contains("not known to be paused")
    ));

    fake.runtime(&resource, RuntimeState::Failed);
    answered(c.step(id));
    assert_eq!(
        c.stop(&grant, id, StopMode::Pause),
        Err(Refusal::Invalid("it has ended".into()))
    );
    let reused = Continuation {
        checkpoint: Some(ContentRef::new("checkpoint".into(), "cd".into())),
        conversation: None,
        mode: ResumeMode::ReuseWithNewGeneration,
    };
    assert!(matches!(
        c.resume(&grant, id, reused, None),
        Err(Refusal::Invalid(why)) if why.contains("cannot resume")
    ));
    // A retry after an ended attempt links it and leaves how it ended as it was.
    let retry = c.retry(&grant, id, "it failed on its own", None).unwrap();
    let before = read(&c, id).state;
    assert_eq!(before.ending, Some(Ending::Failed));
    assert_eq!(before.successor.as_deref(), Some(retry.id.as_str()));
    assert_eq!(kinds(&c, id).last().map(String::as_str), Some("followed"));
}

#[test]
fn collection_and_cleanup_refuse_reports_out_of_their_order() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake);
    let grant = owner_grant();
    let report = Artifact {
        name: "report.json".into(),
        reference: "store://report".into(),
        digest: "d".into(),
        size: 1,
        generation: "g1".into(),
        runtime_epoch: 0,
        verified: true,
    };

    // Cancelled before anything was sent: nothing bound, so nothing to clean.
    let never = admit(&c, "k1");
    c.cancel(&grant, &never.id).unwrap();
    assert!(matches!(
        c.begin_cleanup(&grant, &never.id),
        Err(Refusal::Invalid(why)) if why.contains("nothing to clean")
    ));

    let attempt = c.admit(&grant, asked(OTHER_TASK)).unwrap();
    let id = attempt.id.as_str();
    let resource = started(&c, id);
    fake.runtime(&resource, RuntimeState::Succeeded);
    answered(c.step(id));
    assert!(matches!(
        c.record_artifact(&grant, id, report.clone()),
        Err(Refusal::Invalid(why)) if why.contains("not under way")
    ));
    assert!(matches!(
        c.end_collection(&grant, id, Collection::Complete, None),
        Err(Refusal::Invalid(why)) if why.contains("NotStarted")
    ));
    assert!(matches!(
        c.end_collection(&grant, id, Collection::Collecting, None),
        Err(Refusal::Invalid(_))
    ));
    assert!(matches!(
        c.end_collection(&grant, id, Collection::None, Some("  ")),
        Err(Refusal::Invalid(why)) if why.contains("how that is known")
    ));
    let collecting = c.begin_collection(&grant, id).unwrap();
    assert_eq!(c.begin_collection(&grant, id), Ok(collecting), "begun once");
    let mut unnamed = report.clone();
    unnamed.digest = String::new();
    assert!(matches!(
        c.record_artifact(&grant, id, unnamed),
        Err(Refusal::Invalid(_))
    ));
    let ended = c
        .end_collection(&grant, id, Collection::None, Some("the run wrote nothing"))
        .unwrap();
    assert_eq!(
        c.end_collection(&grant, id, Collection::None, Some("again")),
        Ok(ended),
        "ended once"
    );
    assert!(matches!(
        c.end_collection(&grant, id, Collection::Failed, Some("other")),
        Err(Refusal::Invalid(_))
    ));
    assert!(matches!(
        c.begin_collection(&grant, id),
        Err(Refusal::Invalid(why)) if why.contains("ended already")
    ));

    let key = c.begin_cleanup(&grant, id).unwrap();
    assert!(matches!(
        c.end_cleanup(&grant, id, &key, Cleanup::Running, None),
        Err(Refusal::Invalid(_))
    ));
    let failed = c
        .end_cleanup(&grant, id, &key, Cleanup::Failed, Some("busy"))
        .unwrap();
    assert!(failed.state.open(), "a failed cleanup is still owed");
    assert_eq!(
        c.end_cleanup(&grant, id, &key, Cleanup::Failed, None),
        Ok(failed)
    );
    assert!(matches!(
        c.end_cleanup(&grant, id, &key, Cleanup::Complete, None),
        Err(Refusal::Conflict(why)) if why.contains("ended already")
    ));
    let again = c.begin_cleanup(&grant, id).unwrap();
    assert_ne!(again, key, "a new cleanup has a key of its own");
    assert!(
        matches!(
            c.end_cleanup(&grant, id, &key, Cleanup::Complete, None),
            Err(Refusal::Conflict(_))
        ),
        "the earlier cleanup's late report changes nothing"
    );
    c.end_cleanup(&grant, id, &again, Cleanup::Complete, None)
        .unwrap();
}

/// The attempts holding a place in capacity scope `scope`, as the board keeps them.
fn reserved(f: &Fixture, scope: &str) -> Vec<String> {
    let fences = f.store.dump().unwrap().execution_fences;
    let fence = fences
        .iter()
        .find(|fence| fence["key"] == json!(["capacity", scope]));
    let held = fence.and_then(|fence| fence["reserved"].as_array().cloned());
    (held.unwrap_or_default().iter())
        .map(|attempt| attempt.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_cancel_asked_while_a_pause_is_in_flight_is_sent_after_it_whatever_its_answer() {
    let f = board();
    let fake = Fake::default();
    let caller = coordinator(&f, &fake).with_capacity("pool", 1);
    let grant = owner_grant();
    let paused = admit(&caller, "k1");
    let id = paused.id.as_str();
    let resource = started(&caller, id);
    caller.stop(&grant, id, StopMode::Pause).unwrap();
    let caller_ref = &caller;
    let racing = Meanwhile::new(&fake, Operation::Stop, move || {
        caller_ref.cancel(&owner_grant(), id).expect("asked");
    });
    let c = coordinator(&f, &racing);
    let (operation, outcome) = answered(c.step(id));
    assert_eq!(operation, Operation::Stop);
    assert!(matches!(outcome, Outcome::Accepted(_)), "{outcome:?}");
    assert_eq!(fake.runtime_of(&resource), RuntimeState::Paused);
    let state = read(&c, id).state;
    assert_eq!(state.stop, None);
    assert!(
        state.cancel.is_some(),
        "the cancel asked meanwhile is not dropped"
    );
    assert_eq!(state.cancellation, Cancellation::Requested);
    assert_eq!(
        answered(c.step(id)).0,
        Operation::Cancel,
        "sent next, before any observation"
    );
    assert_eq!(fake.runtime_of(&resource), RuntimeState::Terminated);
    answered(c.step(id));
    let ended = read(&c, id).state;
    assert_eq!(
        (ended.ending, ended.cancellation),
        (Some(Ending::Cancelled), Cancellation::Confirmed)
    );
    assert!(ended.reserved.is_none());

    // A pause refused leaves the cancel asked, never unconfirmed: the provider was not asked.
    let second = caller
        .admit(&grant, asked(OTHER_TASK))
        .expect("the cancelled attempt's place was given back");
    let other = second.id.as_str();
    let resource = started(&caller, other);
    caller.stop(&grant, other, StopMode::Pause).unwrap();
    let racing = Meanwhile::new(&fake, Operation::Stop, move || {
        caller_ref.cancel(&owner_grant(), other).expect("asked");
    });
    let c = coordinator(&f, &racing);
    fake.script([Fault::Refuse("busy")]);
    assert!(matches!(answered(c.step(other)).1, Outcome::Rejected(_)));
    let state = read(&c, other).state;
    assert_eq!(
        (state.stop, state.cancellation),
        (None, Cancellation::Requested)
    );
    assert!(state.cancel.is_some());
    assert_eq!(answered(c.step(other)).0, Operation::Cancel);
    assert_eq!(fake.runtime_of(&resource), RuntimeState::Terminated);
    assert_eq!(fake.keys(Operation::Cancel).len(), 2);
}

#[test]
fn a_resume_or_a_retry_takes_its_predecessors_place_in_its_own_admission() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake).with_capacity("pool", 1);
    let grant = owner_grant();
    let paused = admit(&c, "k1");
    started(&c, &paused.id);
    c.stop(&grant, &paused.id, StopMode::Pause).unwrap();
    answered(c.step(&paused.id));
    answered(c.step(&paused.id));
    assert_eq!(read(&c, &paused.id).state.dispatch, Dispatch::Paused);
    assert_eq!(
        c.admit(&grant, asked(OTHER_TASK)),
        Err(Refusal::Full(vec![paused.id.clone()])),
        "a paused attempt holds its place"
    );

    let continuation = Continuation {
        checkpoint: None,
        conversation: Some("conversation-1".into()),
        mode: ResumeMode::NewResource,
    };
    let resumed = c
        .resume(&grant, &paused.id, continuation.clone(), None)
        .expect("a resume needs no free place");
    let before = read(&c, &paused.id).state;
    assert_eq!(before.ending, Some(Ending::Superseded));
    assert_eq!(before.successor.as_deref(), Some(resumed.id.as_str()));
    assert_eq!(before.reserved, None);
    assert_eq!(
        kinds(&c, &paused.id).last().map(String::as_str),
        Some("ended")
    );
    assert_eq!(
        reserved(&f, "pool"),
        std::slice::from_ref(&resumed.id),
        "handed over, not added"
    );
    assert_eq!(
        f.store.live_attempts(TASK).unwrap(),
        std::slice::from_ref(&resumed.id)
    );
    assert!(
        matches!(
            c.resume(&grant, &paused.id, continuation, None),
            Err(Refusal::Invalid(why)) if why.contains("followed it already")
        ),
        "a repeat without a request key finds it followed, nothing half-done"
    );

    // Held, then retried at capacity 1, its place handed over the same way.
    fake.script([Fault::Lost, Fault::Twice]);
    answered(c.step(&resumed.id));
    answered(c.step(&resumed.id));
    assert_eq!(read(&c, &resumed.id).state.dispatch, Dispatch::Held);
    let retry = c
        .retry(
            &grant,
            &resumed.id,
            "the provider shows both isolated",
            None,
        )
        .expect("a retry needs no free place");
    let held = read(&c, &resumed.id).state;
    assert_eq!(
        (held.ending, held.successor.as_deref()),
        (Some(Ending::Superseded), Some(retry.id.as_str()))
    );
    assert_eq!(reserved(&f, "pool"), std::slice::from_ref(&retry.id));
    assert!(matches!(
        c.retry(&grant, &resumed.id, "again", None),
        Err(Refusal::Invalid(why)) if why.contains("followed it already")
    ));

    // A predecessor that ended gives no place to hand over: a full scope refuses its retry,
    // and the refusal leaves the predecessor as it was.
    let resource = read(&c, &retry.id).state;
    assert!(resource.pending.is_some());
    answered(c.step(&retry.id));
    let bound = read(&c, &retry.id).state.resource.unwrap().id;
    fake.runtime(&bound, RuntimeState::Failed);
    answered(c.step(&retry.id));
    assert_eq!(read(&c, &retry.id).state.ending, Some(Ending::Failed));
    let other = c
        .admit(&grant, asked(OTHER_TASK))
        .expect("the failed retry gave its place back");
    let facts = kinds(&c, &retry.id);
    assert_eq!(
        c.retry(&grant, &retry.id, "it failed on its own", None),
        Err(Refusal::Full(vec![other.id.clone()]))
    );
    let after = read(&c, &retry.id).state;
    assert_eq!(after.successor, None);
    assert_eq!(
        kinds(&c, &retry.id),
        facts,
        "nothing written to the predecessor"
    );
}

/// Attempt `id`, started on its resource, paused by the stop it was asked: the resource.
fn paused_on(c: &Coordinator, id: &str) -> String {
    let resource = started(c, id);
    c.stop(&owner_grant(), id, StopMode::Pause).unwrap();
    answered(c.step(id));
    answered(c.step(id));
    assert_eq!(read(c, id).state.dispatch, Dispatch::Paused);
    resource
}

/// A resume of the attempt paused on `resource`, on that resource at a new generation.
fn reusing(resource: &str) -> Continuation {
    Continuation {
        checkpoint: None,
        conversation: Some(format!("conversation-of-{resource}")),
        mode: ResumeMode::ReuseWithNewGeneration,
    }
}

/// Attempt `id`'s collection gathered, with nothing in it.
fn collected(c: &Coordinator, id: &str) {
    c.begin_collection(&owner_grant(), id).unwrap();
    c.end_collection(
        &owner_grant(),
        id,
        Collection::None,
        Some("it wrote nothing"),
    )
    .unwrap();
}

/// What `admissions` answer, each sent from a store of its own on `f`'s board at the same moment,
/// under capacity scope `capacity` when one is named.
fn racing(
    f: &Fixture,
    fake: &Fake,
    admissions: Vec<Admission>,
    capacity: Option<(&str, u32)>,
) -> Vec<Result<Attempt, Refusal>> {
    let stores: Vec<_> = admissions.iter().map(|_| f.other()).collect();
    let barrier = std::sync::Barrier::new(admissions.len());
    std::thread::scope(|scope| {
        let racers: Vec<_> = (stores.iter().zip(admissions))
            .map(|(store, admission)| {
                let barrier = &barrier;
                scope.spawn(move || {
                    let mut c = Coordinator::new(store, profile(), fake, worker()).unwrap();
                    if let Some((scope, limit)) = capacity {
                        c = c.with_capacity(scope, limit);
                    }
                    barrier.wait();
                    c.admit(&owner_grant(), admission)
                })
            })
            .collect();
        racers
            .into_iter()
            .map(|racer| racer.join().unwrap())
            .collect()
    })
}

/// The one attempt `answers` admitted, every other refused as `refused` says of it.
fn one_admitted(
    answers: Vec<Result<Attempt, Refusal>>,
    refused: impl Fn(&Refusal, &Attempt) -> bool,
) -> Attempt {
    let (admitted, others): (Vec<_>, Vec<_>) = answers.into_iter().partition(Result::is_ok);
    assert_eq!(
        admitted.len(),
        1,
        "exactly one is admitted: {admitted:?} {others:?}"
    );
    let admitted = admitted.into_iter().next().unwrap().unwrap();
    for other in others {
        let refusal = other.unwrap_err();
        assert!(refused(&refusal, &admitted), "{refusal:?}");
    }
    admitted
}

const RACERS: usize = 6;

#[test]
fn a_cancel_after_an_authoritative_absence_ends_it_with_nothing_provisioned() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake).with_capacity("pool", 1);
    let attempt = admit(&c, "k1");
    let id = attempt.id.as_str();
    fake.script([Fault::Down]);
    assert!(is_unknown(&answered(c.step(id)).1));
    assert!(matches!(answered(c.step(id)).1, Outcome::Absent { .. }));
    assert_eq!(read(&c, id).state.dispatch, Dispatch::Unknown);

    let cancelled = c.cancel(&owner_grant(), id).unwrap().state;
    assert_eq!(
        (cancelled.dispatch, cancelled.ending),
        (Dispatch::Terminal, Some(Ending::Cancelled))
    );
    assert_eq!(cancelled.cancellation, Cancellation::Confirmed);
    assert_eq!(c.step(id), Ok(Worked::Waited));
    assert_eq!(
        fake.calls(),
        [Operation::Reconcile],
        "the start is never sent again"
    );
    assert_eq!(fake.resources(), 0);
    assert!(reserved(&f, "pool").is_empty(), "its place was given back");
    assert!(f.store.live_attempts(TASK).unwrap().is_empty());
    // The reconcile is recorded before it is made, as a send is.
    assert_eq!(
        kinds(&c, id),
        [
            "admitted", "intent", "sent", "answer", "sent", "answer", "asked", "ended"
        ]
    );
    let sent: Vec<bool> = (facts(&c, id).iter())
        .filter_map(|fact| match fact {
            Fact::Sent { reconcile, .. } => Some(*reconcile),
            _ => None,
        })
        .collect();
    assert_eq!(sent, [false, true]);
}

#[test]
fn a_resume_that_reuses_the_resource_takes_its_cleanup_from_its_predecessor() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake);
    let grant = owner_grant();
    let paused = admit(&c, "k1");
    let resource = paused_on(&c, &paused.id);
    let resumed = c
        .resume(&grant, &paused.id, reusing(&resource), None)
        .unwrap();
    let before = read(&c, &paused.id).state;
    assert_eq!(
        before.resource_handed_to.as_deref(),
        Some(resumed.id.as_str())
    );
    assert_eq!(
        before.resource.as_ref().map(|bound| bound.id.as_str()),
        Some(resource.as_str())
    );
    assert!(!before.open(), "its resource's cleanup is the resume's");
    collected(&c, &paused.id);
    let refused = |c: &Coordinator| {
        matches!(
            c.begin_cleanup(&grant, &paused.id),
            Err(Refusal::Invalid(why)) if why.contains(&format!("handed to attempt {}", resumed.id))
        )
    };
    assert!(refused(&c), "the resume may still take it");

    // The resume takes it at a new generation, then ends and cleans it as its own.
    let (operation, _) = answered(c.step(&resumed.id));
    assert_eq!(operation, Operation::Resume);
    let bound = read(&c, &resumed.id).state.resource.unwrap();
    assert_eq!(
        (bound.id.as_str(), bound.generation.as_str()),
        (resource.as_str(), "g1+")
    );
    assert!(refused(&c), "the resume holds it");
    fake.runtime(&resource, RuntimeState::Succeeded);
    answered(c.step(&resumed.id));
    assert_eq!(read(&c, &resumed.id).state.ending, Some(Ending::Succeeded));
    collected(&c, &resumed.id);
    let key = c
        .begin_cleanup(&grant, &resumed.id)
        .expect("the resume cleans it");
    c.end_cleanup(
        &grant,
        &resumed.id,
        &key,
        Cleanup::Complete,
        Some("deleted"),
    )
    .unwrap();
    assert!(
        refused(&c),
        "cleaned by the resume, never by its predecessor"
    );
    assert!(!read(&c, &paused.id).state.open());
    assert!(!read(&c, &resumed.id).state.open());
}

#[test]
fn a_resume_that_never_takes_the_resource_leaves_its_cleanup_to_its_predecessor() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake);
    let grant = owner_grant();
    // One resume is cancelled before it was sent, the other refused by the provider.
    for (task, cancelled) in [(TASK, true), (OTHER_TASK, false)] {
        let paused = c.admit(&grant, asked(task)).unwrap();
        let resource = paused_on(&c, &paused.id);
        let resumed = c
            .resume(&grant, &paused.id, reusing(&resource), None)
            .unwrap();
        if cancelled {
            c.cancel(&grant, &resumed.id).unwrap();
        } else {
            fake.script([Fault::Refuse("no capacity to resume")]);
            answered(c.step(&resumed.id));
        }
        let ended = read(&c, &resumed.id).state;
        let ending = if cancelled {
            Ending::Cancelled
        } else {
            Ending::Rejected
        };
        assert_eq!((ended.ending, ended.resource), (Some(ending), None));

        collected(&c, &paused.id);
        let key = c
            .begin_cleanup(&grant, &paused.id)
            .expect("its own to clean again");
        let before = read(&c, &paused.id).state;
        assert_eq!(before.resource_handed_to, None);
        assert!(before.open(), "owed until it is cleaned");
        let cleaned = c
            .end_cleanup(&grant, &paused.id, &key, Cleanup::Complete, None)
            .unwrap();
        assert!(!cleaned.state.open());
    }
    assert_eq!(fake.resources(), 2, "neither resume made anything");
}

#[test]
fn a_resource_whose_cleanup_has_begun_is_resumed_only_on_a_new_one() {
    let f = board();
    let fake = Fake::default();
    let c = coordinator(&f, &fake);
    let grant = owner_grant();
    let stopped = admit(&c, "k1");
    let id = stopped.id.as_str();
    let resource = started(&c, id);
    c.stop(&grant, id, StopMode::Terminate).unwrap();
    answered(c.step(id));
    answered(c.step(id));
    assert_eq!(read(&c, id).state.ending, Some(Ending::Stopped));
    collected(&c, id);
    c.begin_cleanup(&grant, id).unwrap();
    let facts = kinds(&c, id);
    assert!(matches!(
        c.resume(&grant, id, reusing(&resource), None),
        Err(Refusal::Invalid(why)) if why.contains("cleanup has begun")
    ));
    assert_eq!(kinds(&c, id), facts, "nothing written to it");
    let fresh = Continuation {
        mode: ResumeMode::NewResource,
        ..reusing(&resource)
    };
    let resumed = c
        .resume(&grant, id, fresh, None)
        .expect("resumed on a new resource");
    assert_eq!(
        read(&c, id).state.successor.as_deref(),
        Some(resumed.id.as_str())
    );
    assert_eq!(read(&c, id).state.resource_handed_to, None);
}

#[test]
fn a_held_attempt_refuses_a_stop_and_a_cancel_of_it_waits_for_a_person() {
    let f = board();
    let fake = Fake::default();
    let mut pacing = Pacing::default();
    pacing.unknowns = 1;
    let c = coordinator(&f, &fake)
        .with_pacing(pacing)
        .unwrap()
        .with_capacity("pool", 1);
    let grant = owner_grant();
    let held = admit(&c, "k1");
    let id = held.id.as_str();
    started(&c, id);
    fake.script([Fault::Down, Fault::Down]);
    assert!(is_unknown(&answered(c.step(id)).1));
    assert!(is_unknown(&answered(c.step(id)).1));
    let state = read(&c, id).state;
    assert_eq!(state.dispatch, Dispatch::Held);
    assert!(state.resource.is_some());

    for mode in [StopMode::Pause, StopMode::Terminate] {
        assert!(matches!(
            c.stop(&grant, id, mode),
            Err(Refusal::Invalid(why)) if why.contains("held for a person")
        ));
    }
    let cancelled = c.cancel(&grant, id).unwrap().state;
    assert_eq!(
        (cancelled.dispatch, cancelled.cancellation),
        (Dispatch::Held, Cancellation::Requested)
    );
    let calls = fake.calls();
    assert_eq!(c.step(id), Ok(Worked::Waited));
    assert_eq!(fake.calls(), calls, "nothing is sent for a held attempt");
    assert_eq!(
        f.store.live_attempts(TASK).unwrap(),
        std::slice::from_ref(&held.id)
    );
    assert_eq!(
        c.admit(&grant, asked(OTHER_TASK)),
        Err(Refusal::Full(vec![held.id.clone()])),
        "it keeps its place"
    );
    assert!(!c.eligibility(&grant, id, "r1").unwrap().eligible);

    let retry = c
        .retry(&grant, id, "the provider's console shows it gone", None)
        .unwrap();
    let superseded = read(&c, id).state;
    assert_eq!(superseded.ending, Some(Ending::Superseded));
    assert_eq!(superseded.cancel, None);
    assert_eq!(reserved(&f, "pool"), std::slice::from_ref(&retry.id));
}

#[test]
fn concurrent_admissions_admit_one_per_request_key_lane_and_capacity_place() {
    let f = board();
    f.task("sim/fly");
    let fake = Fake::default();

    // One request key: every racer is answered with the one attempt it admitted.
    let same = (0..RACERS).map(|_| asked(TASK).request_key("k1")).collect();
    let answers = racing(&f, &fake, same, None);
    let ids: Vec<String> = answers
        .into_iter()
        .map(|answer| answer.expect("admitted").id)
        .collect();
    assert!(ids.iter().all(|id| *id == ids[0]), "{ids:?}");
    assert_eq!(
        f.store.live_attempts(TASK).unwrap(),
        std::slice::from_ref(&ids[0])
    );

    // One lane: one admitted, every other busy with it.
    let lane = (0..RACERS)
        .map(|n| asked(OTHER_TASK).request_key(&format!("lane-{n}")))
        .collect();
    let held = one_admitted(racing(&f, &fake, lane, None), |refusal, admitted| {
        *refusal == Refusal::Busy(Some(admitted.id.clone()))
    });
    assert_eq!(
        f.store.live_attempts(OTHER_TASK).unwrap(),
        std::slice::from_ref(&held.id)
    );

    // One capacity scope with one place left: one admitted, every other full.
    let c = coordinator(&f, &fake).with_capacity("pool", 2);
    let first = c
        .admit(
            &owner_grant(),
            Admission::new("sim/fly", "first", "executor", input("r1")),
        )
        .unwrap();
    let places = (0..RACERS)
        .map(|n| Admission::new("sim/fly", &format!("lane-{n}"), "executor", input("r1")))
        .collect();
    let last = one_admitted(
        racing(&f, &fake, places, Some(("pool", 2))),
        |refusal, admitted| {
            let Refusal::Full(full) = refusal else {
                return false;
            };
            let mut full = full.clone();
            symphony_board::order::sort_by(&mut full, &Ord::cmp);
            let mut expected = vec![first.id.clone(), admitted.id.clone()];
            symphony_board::order::sort_by(&mut expected, &Ord::cmp);
            full == expected
        },
    );
    let mut holders = reserved(&f, "pool");
    symphony_board::order::sort_by(&mut holders, &Ord::cmp);
    let mut expected = vec![first.id.clone(), last.id.clone()];
    symphony_board::order::sort_by(&mut expected, &Ord::cmp);
    assert_eq!(holders, expected, "no place reserved twice");
    assert_eq!(f.store.live_attempts("sim/fly").unwrap().len(), 2);
}
