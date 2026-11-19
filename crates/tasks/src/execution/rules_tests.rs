use super::*;
use crate::execution::record::{Collection, Ending, Relation};
use symphony_plugin::execution::{
    AttemptId, Capabilities, ContentRef, ConversationId, Input, Observation, OperationIdentity,
    PrincipalKind, Scope,
};

fn principal(subject: &str) -> Principal {
    Principal::new(PrincipalKind::Human, "local".into(), subject.into())
}

fn capabilities(operations: &[Operation]) -> Capabilities {
    let mut capabilities = Capabilities::default();
    capabilities.operations = operations.to_vec();
    capabilities.stop_modes = vec![StopMode::Pause, StopMode::Terminate];
    capabilities
}

fn every_operation() -> Capabilities {
    capabilities(&[
        Operation::Start,
        Operation::Reconcile,
        Operation::Observe,
        Operation::Cancel,
        Operation::Stop,
        Operation::Resume,
    ])
}

fn identity(capabilities: Capabilities) -> Identity {
    Identity {
        project: "p".into(),
        board: "b".into(),
        task: "t".into(),
        lane: "executor".into(),
        role: "executor".into(),
        handle: None,
        predecessor: None,
        relation: None::<Relation>,
        decision: None,
        input: Input::new(
            "r1".into(),
            "d1".into(),
            "abc".into(),
            ContentRef::new("m".into(), "md".into()),
        ),
        configuration: "c".into(),
        contract: symphony_plugin::execution::wire::VERSION,
        capabilities,
        initiator: principal("owner"),
        policy_revision: "1".into(),
        request_key: None,
        runtime_epoch: 0,
        capacity: Some("pool".into()),
    }
}

const HEX: &str = "00112233445566778899aabbccddeeff";

fn attempt() -> AttemptRef {
    AttemptRef::new(
        Scope::new("p".into(), "b".into()),
        "t".into(),
        AttemptId::new([0; 16]),
    )
}

fn start(sent: bool) -> Pending {
    let request = Request::Start {
        input: identity(every_operation()).input,
    };
    Pending {
        sequence: 1,
        kind: Operation::Start,
        key: "k1".into(),
        digest: keys::digest(&request),
        request,
        initiator: principal("owner"),
        sent,
        first_sent: sent.then_some(1_000),
        cancels: false,
    }
}

fn binding() -> Binding {
    Binding {
        id: "res".into(),
        generation: "g1".into(),
    }
}

fn running() -> State {
    State {
        dispatch: Dispatch::Running,
        resource: Some(binding()),
        sequence: 1,
        reserved: Some("pool".into()),
        ..State::default()
    }
}

fn admitted(sent: bool) -> State {
    State {
        sequence: 1,
        pending: Some(start(sent)),
        reserved: Some("pool".into()),
        ..State::default()
    }
}

fn receipt(id: &str, generation: &str) -> Receipt {
    let identity = OperationIdentity::new(
        attempt(),
        "c".into(),
        "d1".into(),
        "k1".into(),
        Operation::Start,
        1,
    );
    let resource = Resource::new(
        attempt(),
        "c".into(),
        ResourceId::new(id.into()),
        generation.into(),
    );
    Receipt::new(identity, resource, "evidence".into())
}

fn observed_as(state: RuntimeState) -> Outcome {
    let resource = resource(&identity(every_operation()), &attempt(), &binding());
    Outcome::Observed(Observation::new(resource, state))
}

fn bounds() -> Bounds {
    Bounds {
        unknowns: 2,
        backoff_ms: 100,
        backoff_cap_ms: 350,
        observe_ms: 5_000,
    }
}

/// The answer `outcome` to the owed operation of `state`.
fn answer(state: &State, reconciled: bool, outcome: Outcome) -> Applied {
    let id = identity(every_operation());
    apply(
        &id,
        state,
        state.pending.as_ref(),
        reconciled,
        &outcome,
        7,
        &bounds(),
    )
}

/// An observation `outcome` of `state`'s resource.
fn observe(state: &State, outcome: Outcome) -> Applied {
    apply(
        &identity(every_operation()),
        state,
        None,
        false,
        &outcome,
        7,
        &bounds(),
    )
}

fn next_of(capabilities: Capabilities, state: &State, now: i64) -> Next {
    next(&identity(capabilities), &attempt(), HEX, state, now)
}

#[test]
fn backoff_doubles_up_to_its_cap() {
    let bounds = bounds();
    let waits: Vec<u64> = (1..=5).map(|n| bounds.backoff(n)).collect();
    assert_eq!(waits, [100, 200, 350, 350, 350]);
    assert_eq!(bounds.backoff(0), 100);
    assert_eq!(bounds.backoff(u32::MAX), 350);
}

#[test]
fn a_held_or_ended_attempt_waits() {
    for dispatch in [Dispatch::Held, Dispatch::Terminal] {
        let state = State {
            dispatch,
            pending: Some(start(false)),
            ..running()
        };
        assert_eq!(next_of(every_operation(), &state, 0), Next::Wait);
    }
    // Nothing owed and nothing bound: nothing to do.
    assert_eq!(next_of(every_operation(), &State::default(), 0), Next::Wait);
}

#[test]
fn an_owed_operation_is_sent_until_it_may_have_been_then_reconciled() {
    assert_eq!(
        next_of(every_operation(), &admitted(false), 0),
        Next::Send {
            pending: start(false),
            new: false
        }
    );
    assert_eq!(
        next_of(every_operation(), &admitted(true), 0),
        Next::Reconcile(start(true))
    );
}

#[test]
fn an_unsettled_operation_is_held_when_it_cannot_be_reconciled() {
    let no_reconcile = capabilities(&[Operation::Start, Operation::Observe]);
    let Next::Hold(why) = next_of(no_reconcile, &admitted(true), 0) else {
        panic!("held");
    };
    assert!(why.contains("cannot reconcile"), "{why}");

    let mut retained = every_operation();
    retained.idempotency_retention_ms = Some(500);
    // Sent at 1000: reconciled within the retention, held once it has run out.
    assert_eq!(
        next_of(retained.clone(), &admitted(true), 1_500),
        Next::Reconcile(start(true))
    );
    let Next::Hold(why) = next_of(retained, &admitted(true), 1_501) else {
        panic!("held");
    };
    assert!(why.contains("retention"), "{why}");
}

#[test]
fn a_bound_resource_is_observed_and_a_cancel_or_stop_asked_is_sent_as_the_next_operation() {
    assert_eq!(
        next_of(every_operation(), &running(), 0),
        Next::Observe(binding())
    );

    let asked = Asked {
        initiator: principal("canceller"),
        mode: None,
    };
    let cancelling = State {
        cancel: Some(asked.clone()),
        ..running()
    };
    let Next::Send { pending, new } = next_of(every_operation(), &cancelling, 0) else {
        panic!("a cancel sent");
    };
    assert!(new);
    assert_eq!((pending.kind, pending.sequence), (Operation::Cancel, 2));
    assert_eq!(pending.initiator, principal("canceller"));
    assert_eq!(
        pending.key,
        keys::operation("p", "b", "c", HEX, Operation::Cancel, 2)
    );
    assert!(matches!(pending.request, Request::Cancel { .. }));
    assert!(!pending.sent);

    // A provider without cancel but with a terminating stop is stopped instead.
    let no_cancel = capabilities(&[Operation::Observe, Operation::Stop]);
    let Next::Send { pending, .. } = next_of(no_cancel, &cancelling, 0) else {
        panic!("a stop sent");
    };
    assert_eq!(pending.kind, Operation::Stop);
    assert!(matches!(
        pending.request,
        Request::Stop {
            mode: StopMode::Terminate,
            ..
        }
    ));

    let pausing = State {
        stop: Some(Asked {
            initiator: principal("owner"),
            mode: Some(StopMode::Pause),
        }),
        ..running()
    };
    let Next::Send { pending, .. } = next_of(every_operation(), &pausing, 0) else {
        panic!("a pause sent");
    };
    assert!(matches!(
        pending.request,
        Request::Stop {
            mode: StopMode::Pause,
            ..
        }
    ));
}

#[test]
fn an_accepted_start_binds_its_resource() {
    let applied = answer(
        &admitted(true),
        false,
        Outcome::Accepted(receipt("res", "g1")),
    );
    assert_eq!(applied.state.dispatch, Dispatch::Starting);
    assert_eq!(applied.state.resource, Some(binding()));
    assert_eq!(applied.state.pending, None);
    assert_eq!(
        applied.facts,
        [Fact::Bound {
            resource: Some(binding()),
            conversation: None
        }]
    );
    assert_eq!(applied.due_in, Some(5_000));
    assert_eq!(applied.release, None);
    // Found on reconciling it is the same.
    let found = answer(&admitted(true), true, Outcome::Found(receipt("res", "g1")));
    assert_eq!(found.state, applied.state);
}

#[test]
fn a_start_accepted_after_a_cancel_was_asked_is_cancelled_at_once() {
    let state = State {
        cancellation: Cancellation::Requested,
        cancel: Some(Asked {
            initiator: principal("owner"),
            mode: None,
        }),
        ..admitted(true)
    };
    let applied = answer(&state, true, Outcome::Found(receipt("res", "g1")));
    assert_eq!(applied.due_in, Some(0));
    let next = next(
        &identity(every_operation()),
        &attempt(),
        HEX,
        &applied.state,
        0,
    );
    assert!(matches!(next, Next::Send { pending, .. } if pending.kind == Operation::Cancel));
}

#[test]
fn a_receipt_for_another_resource_than_the_one_bound_is_held() {
    let state = State {
        resource: Some(binding()),
        ..admitted(true)
    };
    let applied = answer(&state, true, Outcome::Found(receipt("other", "g1")));
    assert_eq!(applied.state.dispatch, Dispatch::Held);
    assert!(applied.state.held.unwrap().contains("other"));
}

#[test]
fn an_absent_operation_is_sent_again_by_the_same_key_unless_a_cancel_was_asked() {
    let applied = answer(
        &admitted(true),
        true,
        Outcome::Absent {
            evidence: "none".into(),
        },
    );
    let owed = applied.state.pending.expect("still owed");
    assert_eq!((owed.key.as_str(), owed.sent), ("k1", false));
    assert_eq!(applied.due_in, Some(0));

    let cancelled = State {
        cancellation: Cancellation::Requested,
        ..admitted(true)
    };
    let applied = answer(
        &cancelled,
        true,
        Outcome::Absent {
            evidence: "none".into(),
        },
    );
    assert_eq!(applied.state.dispatch, Dispatch::Terminal);
    assert_eq!(applied.state.ending, Some(Ending::Cancelled));
    assert_eq!(applied.state.cancellation, Cancellation::Confirmed);
    assert_eq!(applied.release.as_deref(), Some("pool"));
    assert_eq!(applied.state.reserved, None);
}

#[test]
fn conflicting_matches_are_quarantined() {
    let matches = vec![receipt("a", "g"), receipt("b", "g")];
    let applied = answer(&admitted(true), true, Outcome::Conflicting { matches });
    assert_eq!(applied.state.dispatch, Dispatch::Held);
    assert!(applied.state.held.unwrap().contains("2 resources"));
    assert_eq!(applied.release, None, "a held attempt keeps its place");
}

#[test]
fn a_refused_start_ends_rejected_and_gives_its_place_back() {
    for outcome in [
        Outcome::Rejected(Rejection::Provider("quota".into())),
        Outcome::Unsupported(Operation::Start),
    ] {
        let applied = answer(&admitted(true), false, outcome);
        assert_eq!(applied.state.ending, Some(Ending::Rejected));
        assert_eq!(applied.release.as_deref(), Some("pool"));
        assert_eq!(
            applied.facts,
            [Fact::Ended {
                ending: Ending::Rejected
            }]
        );
    }
}

#[test]
fn a_refused_cancel_leaves_the_cancellation_unconfirmed() {
    let request = Request::Cancel {
        resource: resource(&identity(every_operation()), &attempt(), &binding()),
    };
    let state = State {
        cancellation: Cancellation::Requested,
        cancel: Some(Asked {
            initiator: principal("owner"),
            mode: None,
        }),
        pending: Some(Pending {
            sequence: 2,
            kind: Operation::Cancel,
            key: "k2".into(),
            digest: keys::digest(&request),
            request,
            initiator: principal("owner"),
            sent: true,
            first_sent: Some(1),
            cancels: false,
        }),
        ..running()
    };
    let applied = answer(
        &state,
        false,
        Outcome::Rejected(Rejection::Provider("no".into())),
    );
    assert_eq!(applied.state.cancellation, Cancellation::Unconfirmed);
    assert_eq!(applied.state.cancel, None);
    assert_eq!(applied.state.dispatch, Dispatch::Running);
    // Accepted, it is observed until the runtime is seen to cease.
    let applied = answer(&state, false, Outcome::Accepted(receipt("res", "g1")));
    assert_eq!(applied.state.cancellation, Cancellation::Requested);
    assert_eq!(applied.state.cancel, None);
    assert_eq!(applied.state.dispatch, Dispatch::Running);
}

#[test]
fn a_failed_reconcile_holds_the_attempt() {
    let applied = answer(
        &admitted(true),
        true,
        Outcome::Rejected(Rejection::Provider("gone".into())),
    );
    assert_eq!(applied.state.dispatch, Dispatch::Held);
    assert!(
        applied
            .state
            .held
            .unwrap()
            .contains("could not be reconciled")
    );
}

#[test]
fn a_reconcile_refused_or_unsupported_holds_the_attempt_saying_why() {
    let refused = answer(
        &admitted(true),
        true,
        Outcome::Rejected(Rejection::Provider("gone".into())),
    );
    let why = refused.state.held.unwrap();
    assert!(why.contains("its start could not be reconciled"), "{why}");
    assert!(why.contains("the provider refused: gone"), "{why}");
    let unsupported = answer(
        &admitted(true),
        true,
        Outcome::Unsupported(Operation::Reconcile),
    );
    assert_eq!(unsupported.state.dispatch, Dispatch::Held);
    let why = unsupported.state.held.unwrap();
    assert!(why.contains("the provider cannot reconcile"), "{why}");
    assert_eq!(
        unsupported.state.pending,
        admitted(true).pending,
        "still owed"
    );
    assert_eq!(
        unsupported.release, None,
        "its place is kept while it may run"
    );
}

#[test]
fn a_start_found_again_on_the_resource_already_bound_binds_nothing_new() {
    let state = State {
        dispatch: Dispatch::Unknown,
        resource: Some(binding()),
        ..admitted(true)
    };
    let found = answer(&state, true, Outcome::Found(receipt("res", "g1")));
    assert_eq!(found.state.resource, Some(binding()));
    assert_eq!(found.state.dispatch, Dispatch::Starting);
    assert_eq!(found.state.pending, None);
    assert!(found.facts.is_empty(), "{:?}", found.facts);
}

/// `state` with the operation the worker would send next marked sent, as its step writes it.
fn sending(capabilities: Capabilities, state: &State) -> State {
    let Next::Send { mut pending, .. } = next_of(capabilities, state, 2_000) else {
        panic!("an operation to send");
    };
    pending.sent = true;
    pending.first_sent = Some(2_000);
    State {
        sequence: pending.sequence,
        pending: Some(pending),
        ..state.clone()
    }
}

/// The answer `outcome` to `state`'s owed operation, on a provider of `capabilities`.
fn answer_on(capabilities: Capabilities, state: &State, outcome: Outcome) -> Applied {
    apply(
        &identity(capabilities),
        state,
        state.pending.as_ref(),
        false,
        &outcome,
        7,
        &bounds(),
    )
}

/// A provider that cannot cancel, only terminate.
fn no_cancel() -> Capabilities {
    capabilities(&[
        Operation::Start,
        Operation::Reconcile,
        Operation::Observe,
        Operation::Stop,
    ])
}

#[test]
fn a_cancel_asked_while_a_stop_is_in_flight_is_sent_after_it_whatever_its_answer() {
    let owner = principal("owner");
    let id = identity(every_operation());
    let pausing = stop(&id, &running(), &owner, StopMode::Pause)
        .unwrap()
        .state;
    let in_flight = sending(every_operation(), &pausing);
    let owed = in_flight.pending.as_ref().unwrap();
    assert_eq!((owed.kind, owed.cancels()), (Operation::Stop, false));
    let asked = cancel(&in_flight, &owner, 5).unwrap().state;
    assert!(
        asked.cancel.is_some() && asked.pending.is_some(),
        "the stop is still owed"
    );

    let paused = answer(&asked, false, Outcome::Accepted(receipt("res", "g1")));
    assert_eq!(paused.state.stop, None);
    assert!(paused.state.cancel.is_some(), "the cancel is not dropped");
    assert_eq!(paused.state.cancellation, Cancellation::Requested);
    assert_eq!(paused.due_in, Some(0), "sent at once");
    let Next::Send { pending, .. } = next_of(every_operation(), &paused.state, 2_000) else {
        panic!("the cancel is sent");
    };
    assert_eq!((pending.kind, pending.cancels()), (Operation::Cancel, true));
    // On a provider that cannot cancel, a terminate stands in for it.
    let Next::Send { pending, .. } = next_of(no_cancel(), &paused.state, 2_000) else {
        panic!("its stand-in is sent");
    };
    assert_eq!((pending.kind, pending.cancels()), (Operation::Stop, true));
    assert!(matches!(
        pending.request,
        Request::Stop {
            mode: StopMode::Terminate,
            ..
        }
    ));

    // A pause refused leaves the cancel asked, never unconfirmed: the provider was not asked.
    let refused = answer(
        &asked,
        false,
        Outcome::Rejected(Rejection::Provider("busy".into())),
    );
    assert_eq!(refused.state.stop, None);
    assert_eq!(refused.state.pending, None);
    assert!(refused.state.cancel.is_some());
    assert_eq!(refused.state.cancellation, Cancellation::Requested);
    assert_eq!(refused.due_in, Some(0));
    assert!(matches!(
        next_of(every_operation(), &refused.state, 2_000),
        Next::Send { pending, .. } if pending.kind == Operation::Cancel
    ));
}

#[test]
fn the_terminate_standing_in_for_a_cancel_settles_it_taken_or_refused() {
    let owner = principal("owner");
    let pausing = stop(&identity(no_cancel()), &running(), &owner, StopMode::Pause)
        .unwrap()
        .state;
    // A cancel asked before the pause was sent goes first, as a terminate, and settles both.
    let asked = cancel(&pausing, &owner, 5).unwrap().state;
    let terminating = sending(no_cancel(), &asked);
    assert!(terminating.pending.as_ref().unwrap().cancels());
    let taken = answer_on(
        no_cancel(),
        &terminating,
        Outcome::Accepted(receipt("res", "g1")),
    );
    assert_eq!((taken.state.cancel, taken.state.stop), (None, None));
    assert_eq!(
        taken.state.cancellation,
        Cancellation::Requested,
        "observed until it ceased"
    );
    assert_eq!(taken.due_in, Some(5_000));

    let refused = answer_on(
        no_cancel(),
        &terminating,
        Outcome::Rejected(Rejection::Provider("no".into())),
    );
    assert_eq!(refused.state.cancel, None);
    assert_eq!(refused.state.cancellation, Cancellation::Unconfirmed);
    assert!(
        refused.state.stop.is_some(),
        "the pause asked is still to be sent"
    );
    assert_eq!(refused.due_in, Some(0));
}

#[test]
fn answers_that_settle_nothing_back_off_and_are_held_past_the_bound() {
    let mut state = admitted(true);
    let mut waits = Vec::new();
    for _ in 0..2 {
        let applied = answer(&state, true, Outcome::Unknown(Unknown::ContactLost));
        assert_eq!(applied.state.dispatch, Dispatch::Unknown);
        assert!(applied.state.pending.is_some(), "never sent anew");
        waits.push(applied.due_in);
        state = applied.state;
    }
    assert_eq!(waits, [Some(100), Some(200)]);
    let applied = answer(&state, true, Outcome::Unknown(Unknown::Deadline));
    assert_eq!(applied.state.dispatch, Dispatch::Held);
    assert_eq!(applied.state.unknowns, 3);
    assert_eq!(applied.due_in, None);
    assert!(matches!(applied.facts.as_slice(), [Fact::Held { .. }]));
}

#[test]
fn an_expired_key_is_held_at_once() {
    let applied = answer(&admitted(true), true, Outcome::Unknown(Unknown::KeyExpired));
    assert_eq!(applied.state.dispatch, Dispatch::Held);
    assert!(applied.state.held.unwrap().contains("expired"));
}

#[test]
fn an_observed_ending_ends_the_attempt_and_gives_its_place_back() {
    let applied = observe(&running(), observed_as(RuntimeState::Succeeded));
    assert_eq!(applied.state.dispatch, Dispatch::Terminal);
    assert_eq!(applied.state.ending, Some(Ending::Succeeded));
    assert_eq!(applied.state.quiescent, Some(7));
    assert_eq!(applied.release.as_deref(), Some("pool"));
    assert_eq!(applied.due_in, None);

    assert_eq!(
        observe(&running(), observed_as(RuntimeState::Failed))
            .state
            .ending,
        Some(Ending::Failed)
    );
    assert_eq!(
        observe(&running(), observed_as(RuntimeState::Terminated))
            .state
            .ending,
        Some(Ending::Stopped)
    );
    let cancelled = State {
        cancellation: Cancellation::Requested,
        ..running()
    };
    let applied = observe(&cancelled, observed_as(RuntimeState::Destroyed));
    assert_eq!(applied.state.ending, Some(Ending::Cancelled));
    assert_eq!(applied.state.cancellation, Cancellation::Confirmed);
}

#[test]
fn a_pause_is_quiescent_and_settles_the_pause_asked() {
    let state = State {
        stop: Some(Asked {
            initiator: principal("owner"),
            mode: Some(StopMode::Pause),
        }),
        ..running()
    };
    let applied = observe(&state, observed_as(RuntimeState::Paused));
    assert_eq!(applied.state.dispatch, Dispatch::Paused);
    assert_eq!(applied.state.quiescent, Some(7));
    assert_eq!(applied.state.stop, None);
    assert_eq!(applied.release, None, "a paused resource keeps its place");
}

#[test]
fn an_observation_records_its_cursor_conversation_and_evidence_refs() {
    let mut observation = Observation::new(
        resource(&identity(every_operation()), &attempt(), &binding()),
        RuntimeState::Running,
    );
    observation.cursor = Some("5".into());
    observation.conversation = Some(ConversationId::new("conv".into()));
    observation.completion = Some(ContentRef::new("done".into(), "dd".into()));
    let applied = observe(&running(), Outcome::Observed(observation.clone()));
    assert_eq!(applied.state.cursor.as_deref(), Some("5"));
    assert_eq!(applied.state.conversation.as_deref(), Some("conv"));
    assert_eq!(
        applied.state.completion,
        Some(ContentRef::new("done".into(), "dd".into()))
    );
    assert_eq!(
        applied.facts,
        [Fact::Bound {
            resource: None,
            conversation: Some("conv".into())
        }]
    );
    // The same conversation again binds nothing new.
    let again = observe(&applied.state, Outcome::Observed(observation));
    assert!(again.facts.is_empty());
}

#[test]
fn a_replayed_or_older_observation_changes_nothing() {
    let mut caps = every_operation();
    caps.ordered_observations = true;
    let id = identity(caps);
    let state = State {
        cursor: Some("10".into()),
        ..running()
    };
    for cursor in ["10", "9"] {
        let mut observation = Observation::new(
            resource(&id, &attempt(), &binding()),
            RuntimeState::Succeeded,
        );
        observation.cursor = Some(cursor.into());
        let applied = apply(
            &id,
            &state,
            None,
            false,
            &Outcome::Observed(observation),
            7,
            &bounds(),
        );
        assert_eq!(applied.state, state, "cursor {cursor}");
        assert!(applied.facts.is_empty());
    }
}

#[test]
fn lost_contact_on_an_observation_is_no_failure() {
    let applied = observe(&running(), observed_as(RuntimeState::Unknown));
    assert_eq!(applied.state.dispatch, Dispatch::Running);
    assert_eq!(applied.state.unknowns, 1);
    assert_eq!(applied.due_in, Some(100));
    let applied = observe(&running(), Outcome::Unknown(Unknown::ContactLost));
    assert_eq!(applied.state.dispatch, Dispatch::Running);
    assert_eq!(applied.state.ending, None);
}

#[test]
fn an_observation_the_provider_refuses_holds_the_attempt() {
    for outcome in [
        Outcome::Unsupported(Operation::Observe),
        Outcome::Rejected(Rejection::Provider("no".into())),
    ] {
        let applied = observe(&running(), outcome);
        assert_eq!(applied.state.dispatch, Dispatch::Held);
        assert_eq!(
            applied.state.held.as_deref(),
            Some("the provider cannot observe it")
        );
        assert_eq!(applied.release, None);
    }
}

#[test]
fn a_provisioning_observation_of_an_unsettled_start_shows_it_starting() {
    let unknown = State {
        dispatch: Dispatch::Unknown,
        unknowns: 2,
        ..running()
    };
    let applied = observe(&unknown, observed_as(RuntimeState::Provisioning));
    assert_eq!(
        (applied.state.dispatch, applied.state.unknowns),
        (Dispatch::Starting, 0)
    );
    let applied = observe(&running(), observed_as(RuntimeState::Provisioning));
    assert_eq!(
        applied.state.dispatch,
        Dispatch::Running,
        "never back to starting"
    );
}

#[test]
fn an_observation_of_an_ended_attempt_changes_nothing() {
    let ended = State {
        dispatch: Dispatch::Terminal,
        ending: Some(Ending::Succeeded),
        ..running()
    };
    let applied = observe(&ended, observed_as(RuntimeState::Failed));
    assert_eq!(applied.state, ended);
}

#[test]
fn a_cancel_asked_while_observing_is_sent_at_once() {
    let state = State {
        cancel: Some(Asked {
            initiator: principal("owner"),
            mode: None,
        }),
        cancellation: Cancellation::Requested,
        ..running()
    };
    assert_eq!(
        observe(&state, observed_as(RuntimeState::Running)).due_in,
        Some(0)
    );
}

#[test]
fn a_cancel_before_anything_was_sent_ends_the_attempt_with_nothing_provisioned() {
    let applied = cancel(&admitted(false), &principal("owner"), 3).expect("asked");
    assert_eq!(applied.state.dispatch, Dispatch::Terminal);
    assert_eq!(applied.state.ending, Some(Ending::Cancelled));
    assert_eq!(applied.state.cancellation, Cancellation::Confirmed);
    assert_eq!(applied.state.pending, None);
    assert_eq!(applied.state.quiescent, Some(3));
    assert_eq!(applied.release.as_deref(), Some("pool"));
    assert!(matches!(
        applied.facts.as_slice(),
        [
            Fact::Asked { cancel: true, .. },
            Fact::Ended {
                ending: Ending::Cancelled
            }
        ]
    ));
}

#[test]
fn a_cancel_of_a_start_in_flight_waits_for_its_answer() {
    let applied = cancel(&admitted(true), &principal("owner"), 3).expect("asked");
    assert_eq!(applied.state.cancellation, Cancellation::Requested);
    assert_eq!(applied.state.pending, Some(start(true)));
    assert!(applied.state.cancel.is_some());
    assert_eq!(applied.due_in, Some(0));
    // Asked again: nothing changes.
    assert_eq!(cancel(&applied.state, &principal("owner"), 4), None);
}

#[test]
fn a_cancel_of_an_ended_attempt_bars_its_result() {
    let ended = State {
        dispatch: Dispatch::Terminal,
        ending: Some(Ending::Succeeded),
        ..running()
    };
    let applied = cancel(&ended, &principal("owner"), 3).expect("asked");
    assert_eq!(applied.state.cancellation, Cancellation::Confirmed);
    assert_eq!(applied.state.ending, Some(Ending::Succeeded));
    assert_eq!(applied.due_in, None);
    assert_eq!(applied.release, None);
}

#[test]
fn a_stop_needs_a_bound_running_resource_and_a_mode_its_provider_offers() {
    let id = identity(every_operation());
    let owner = principal("owner");
    assert!(stop(&id, &admitted(true), &owner, StopMode::Pause).is_err());
    let ended = State {
        dispatch: Dispatch::Terminal,
        ..running()
    };
    assert!(stop(&id, &ended, &owner, StopMode::Pause).is_err());
    let why = stop(&id, &running(), &owner, StopMode::HardStop).unwrap_err();
    assert!(why.contains("hard-stop"), "{why}");
    let applied = stop(&id, &running(), &owner, StopMode::Pause).expect("asked");
    assert_eq!(applied.state.stop.unwrap().mode, Some(StopMode::Pause));
    assert_eq!(applied.due_in, Some(0));
}

#[test]
fn only_a_quiescent_settled_attempt_with_its_resource_is_resumable() {
    let paused = State {
        dispatch: Dispatch::Paused,
        quiescent: Some(9),
        ..running()
    };
    assert_eq!(resumable(&paused), Ok((binding(), 9)));
    let stopped = State {
        dispatch: Dispatch::Terminal,
        ending: Some(Ending::Stopped),
        ..paused.clone()
    };
    assert!(resumable(&stopped).is_ok());

    for (refused, why) in [
        (running(), "not known to be paused"),
        (
            State {
                pending: Some(start(true)),
                ..paused.clone()
            },
            "not settled",
        ),
        (
            State {
                cancellation: Cancellation::Unconfirmed,
                ..paused.clone()
            },
            "not confirmed",
        ),
        (
            State {
                successor: Some("next".into()),
                ..paused.clone()
            },
            "followed it already",
        ),
        (
            State {
                ending: Some(Ending::Superseded),
                ..stopped.clone()
            },
            "not known to be paused",
        ),
        (
            State {
                resource: None,
                ..paused.clone()
            },
            "not known to be paused",
        ),
    ] {
        let error = resumable(&refused).unwrap_err();
        assert!(error.contains(why), "{error}");
    }
}

#[test]
fn an_attempt_stays_open_while_anything_is_owed_for_it() {
    assert!(running().open());
    let ended = State {
        dispatch: Dispatch::Terminal,
        ..running()
    };
    assert!(ended.open(), "its resource is not cleaned up");
    let cleaned = State {
        cleanup: crate::execution::record::Cleanup::Complete,
        ..ended.clone()
    };
    assert!(!cleaned.open());
    let collecting = State {
        collection: Collection::Collecting,
        ..cleaned.clone()
    };
    assert!(collecting.open());
    let rejected = State {
        dispatch: Dispatch::Terminal,
        ..State::default()
    };
    assert!(!rejected.open(), "nothing was bound to clean");
}
