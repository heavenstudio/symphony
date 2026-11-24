//! A task's execution attempts read with no provider at hand (`execution::Reader`), as a page and
//! `tasks api` read them: a history kept whole across a retry, each attempt's lane holder said,
//! and a read refused to a grant of another task, act or board, and for an attempt of another
//! project however its id came.

use crate::common::{Fixture, Seed};
use std::time::Instant;
use symphony_board::execution::{
    Act, Adapter, Admission, Coordinator, Grant, Reader, Refusal, board_scope,
};
use symphony_plugin::execution::{
    Authorization, Call, Capabilities, ContentRef, Input, Operation, Outcome, Principal,
    PrincipalKind, Profile, ResumeMode, Scope, Unknown,
};

const TASK: &str = "sim/jump";
const OTHER: &str = "sim/land";

/// A provider no read reaches: nothing here sends an operation.
struct Unreached;

impl Adapter for Unreached {
    fn execute(&self, _: &Call, _: &Authorization, _: Instant) -> Outcome {
        Outcome::Unknown(Unknown::Other("no read reaches the provider".into()))
    }
}

fn owner() -> Principal {
    Principal::new(PrincipalKind::Human, "local".into(), "owner".into())
}

fn grant(scope: &Scope, acts: &[Act]) -> Grant {
    Grant::from_host(owner(), scope.clone(), acts, "policy-1")
}

fn input() -> Input {
    Input::new(
        "r1".into(),
        "digest-of-r1".into(),
        "commit-abc".into(),
        ContentRef::new("manifest".into(), "manifest-digest".into()),
    )
}

fn capabilities() -> Capabilities {
    let mut capabilities = Capabilities::default();
    capabilities.operations = vec![Operation::Start, Operation::Cancel, Operation::Observe];
    capabilities
}

fn coordinator<'a>(f: &'a Fixture, scope: &Scope, adapter: &'a dyn Adapter) -> Coordinator<'a> {
    let profile = Profile::new(scope.clone(), "cloud".into(), capabilities());
    let worker = Principal::new(PrincipalKind::Service, "board".into(), "worker".into());
    let worker = Grant::from_host(worker, scope.clone(), &[Act::Work], "policy-1");
    Coordinator::new(&f.store, profile, adapter, worker).expect("the worker may work")
}

fn everything(scope: &Scope) -> Grant {
    grant(
        scope,
        &[Act::Start, Act::Cancel, Act::Retry, Act::Read, Act::Link],
    )
}

#[test]
fn a_history_is_read_whole_with_no_provider_and_each_lane_holder_said() {
    let f = Fixture::new(Seed::default());
    f.task(TASK);
    f.task(OTHER);
    let scope = board_scope("proj", &f.store);
    assert_eq!(scope.board, f.store.database().database());
    let c = coordinator(&f, &scope, &Unreached);
    let all = everything(&scope);
    let asked = || Admission::new(TASK, "executor/amber", "executor", input()).handle("amber");
    let first = c.admit(&all, asked()).unwrap();
    // Cancelled before anything was sent, it ends at once, and a person retries it.
    c.cancel(&all, &first.id).unwrap();
    let second = c
        .retry(&all, &first.id, "nothing was provisioned", None)
        .unwrap();
    let other = c
        .admit(
            &all,
            Admission::new(OTHER, "executor/amber", "executor", input()),
        )
        .unwrap();
    assert_eq!(f.store.attempt_count(TASK).unwrap(), 2);
    assert_eq!(f.store.attempt_count(OTHER).unwrap(), 1);
    assert_eq!(f.store.attempt_count("sim/never").unwrap(), 0);

    let reader = Reader::new(&f.store, scope.clone());
    let read = grant(&scope, &[Act::Read]).on_tasks(&[TASK]);
    let history = reader.history(&read, TASK).unwrap();
    let ids: Vec<_> = history.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, [first.id.as_str(), second.id.as_str()]);
    assert_eq!(
        history.iter().map(|a| a.current).collect::<Vec<_>>(),
        [false, true],
        "the retry holds the lane"
    );
    let wired = history[0].wire();
    assert_eq!(
        (
            wired.dispatch.as_str(),
            wired.ending.as_deref(),
            wired.cancellation.as_str()
        ),
        ("terminal", Some("cancelled"), "confirmed")
    );
    assert_eq!(wired.successor.as_deref(), Some(second.id.as_str()));
    assert_eq!(wired.initiator, "human local/owner");
    assert_eq!(wired.handle.as_deref(), Some("amber"));
    let retried = history[1].wire();
    assert_eq!(
        (retried.predecessor.as_deref(), retried.relation.as_deref()),
        (Some(first.id.as_str()), Some("retry"))
    );
    assert_eq!(retried.dispatch, "pending");
    assert_eq!(retried.controls, symphony_api::AttemptControls::default());
    for mode in [ResumeMode::NewResource, ResumeMode::ReuseWithNewGeneration] {
        assert!(
            history[0].resumable(mode).is_err(),
            "nothing was ever bound to it"
        );
    }
    // The reader and the coordinator read the same.
    assert_eq!(c.history(&all, TASK).unwrap(), history);
    assert_eq!(reader.attempt(&read, &second.id).unwrap(), history[1]);
    let kinds: Vec<_> = reader
        .facts(&read, &first.id)
        .unwrap()
        .iter()
        .map(|recorded| serde_json::to_value(&recorded.fact).unwrap()["kind"].clone())
        .collect();
    assert_eq!(kinds.first(), Some(&serde_json::json!("admitted")));
    assert!(kinds.contains(&serde_json::json!("followed")), "{kinds:?}");

    // A grant on one task reads no other, nor anything board-wide, and a grant without Read
    // reads nothing.
    let refused = |result: Result<_, Refusal>| match result {
        Err(Refusal::Unauthorized(_)) => {}
        other => panic!("expected unauthorized, got {other:?}"),
    };
    refused(reader.history(&read, OTHER).map(|_| ()));
    refused(reader.attempt(&read, &other.id).map(|_| ()));
    refused(reader.facts(&read, &other.id).map(|_| ()));
    refused(
        reader
            .history(&grant(&scope, &[Act::Cancel]), TASK)
            .map(|_| ()),
    );
    // An id that is no attempt is unknown, read as text or as hex.
    for id in ["../x", "0123456789abcdef0123456789abcdef", ""] {
        assert!(
            matches!(reader.attempt(&read, id), Err(Refusal::Unknown(_))),
            "{id:?}"
        );
    }
}

#[test]
fn an_attempt_of_another_project_is_refused_however_its_id_came() {
    let f = Fixture::new(Seed::default());
    f.task(TASK);
    let ours = board_scope("proj", &f.store);
    let theirs = board_scope("another-project", &f.store);
    let elsewhere = coordinator(&f, &theirs, &Unreached);
    let foreign = elsewhere
        .admit(
            &everything(&theirs),
            Admission::new(TASK, "executor/amber", "executor", input()),
        )
        .unwrap();
    let reader = Reader::new(&f.store, ours.clone());
    let read = grant(&ours, &[Act::Read]);
    assert!(matches!(
        reader.attempt(&read, &foreign.id),
        Err(Refusal::Scope)
    ));
    assert!(matches!(
        reader.facts(&read, &foreign.id),
        Err(Refusal::Scope)
    ));
    assert!(matches!(reader.history(&read, TASK), Err(Refusal::Scope)));
    // A grant of the other project reads nothing through this board's reader.
    assert!(matches!(
        reader.attempt(&grant(&theirs, &[Act::Read]), &foreign.id),
        Err(Refusal::Scope)
    ));
    assert!(matches!(
        reader.history(&grant(&theirs, &[Act::Read]), TASK),
        Err(Refusal::Scope)
    ));
}
