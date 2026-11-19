//! An attempt's rules, decided in the process on the state a write read: what its worker does
//! next ([`next`]), what an answer makes of it ([`apply`]), and what a cancel or a stop asked for
//! does ([`cancel`], [`stop`]). Nothing here reads the store or a clock: `now` is the database's,
//! as the worker's claim read it.
//!
//! The answers that settle nothing (lost contact, a deadline, a provider that does not know yet)
//! never fail an attempt and never start another: the operation is reconciled by its key, with a
//! backoff, and after [`Bounds::unknowns`] of them in a row, or once the provider's key retention
//! has run out, the attempt is held for a person.

use super::keys;
use super::record::{
    Asked, Binding, Cancellation, Dispatch, Ending, Fact, Identity, Pending, State,
};
use symphony_plugin::execution::{
    AttemptRef, Operation, Outcome, Principal, Receipt, Rejection, Request, Resource, ResourceId,
    RuntimeState, StopMode, Unknown,
};

/// How far recovery goes on its own, and how often it looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Bounds {
    /// Answers in a row that settle nothing, after which the attempt is held.
    pub unknowns: u32,
    /// The first wait after one, doubled after each, up to `backoff_cap_ms`.
    pub backoff_ms: u64,
    pub backoff_cap_ms: u64,
    /// How often a running attempt is observed.
    pub observe_ms: u64,
}

impl Default for Bounds {
    fn default() -> Bounds {
        Bounds {
            unknowns: 8,
            backoff_ms: 1_000,
            backoff_cap_ms: 60_000,
            observe_ms: 5_000,
        }
    }
}

impl Bounds {
    /// The wait after `unknowns` answers in a row that settled nothing.
    pub(crate) fn backoff(&self, unknowns: u32) -> u64 {
        let doubled = self
            .backoff_ms
            .saturating_mul(1u64 << unknowns.saturating_sub(1).min(20));
        doubled.min(self.backoff_cap_ms)
    }
}

/// What the worker does for an attempt now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Next {
    /// Nothing: it ended, or it waits on a person or on the host.
    Wait,
    /// Send this operation, recorded as sent first: one not sent before, or a new one (a cancel or
    /// a stop asked for), which the write that records it as sent also records as owed.
    Send { pending: Pending, new: bool },
    /// Reconcile this operation, which may have reached the provider.
    Reconcile(Pending),
    /// Observe the bound resource.
    Observe(Binding),
    /// Hold the attempt for a person, for this reason, with no I/O.
    Hold(String),
}

/// What an answer, a request or a hold makes of an attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Applied {
    pub state: State,
    pub facts: Vec<Fact>,
    /// The capacity scope whose reservation it gives back: it is known that nothing of it runs.
    pub release: Option<String>,
    /// When its worker should look again (ms from now), or none: it waits on a person or the host.
    pub due_in: Option<u64>,
}

impl Applied {
    pub(super) fn from_state(state: State) -> Applied {
        Applied {
            state,
            facts: Vec::new(),
            release: None,
            due_in: None,
        }
    }
}

/// The resource `binding` of attempt `identity` as the kit names it.
pub(crate) fn resource(identity: &Identity, attempt: &AttemptRef, binding: &Binding) -> Resource {
    Resource::new(
        attempt.clone(),
        identity.configuration.clone(),
        ResourceId::new(binding.id.clone()),
        binding.generation.clone(),
    )
}

/// What the worker does for attempt `attempt` (`identity`, in `state`) at `now`.
pub(crate) fn next(
    identity: &Identity,
    attempt: &AttemptRef,
    hex: &str,
    state: &State,
    now: i64,
) -> Next {
    if matches!(state.dispatch, Dispatch::Held | Dispatch::Terminal) {
        return Next::Wait;
    }
    let caps = &identity.capabilities;
    if let Some(pending) = &state.pending {
        if !pending.sent {
            return Next::Send {
                pending: pending.clone(),
                new: false,
            };
        }
        if !caps.operations.contains(&Operation::Reconcile) {
            return Next::Hold(format!(
                "its {} may have reached the provider, which cannot reconcile it",
                name(pending.kind)
            ));
        }
        if let (Some(retention), Some(first)) = (caps.idempotency_retention_ms, pending.first_sent)
            && now.saturating_sub(first) > i64::try_from(retention).unwrap_or(i64::MAX)
        {
            return Next::Hold(format!(
                "its {} is unsettled and the provider's key retention has run out",
                name(pending.kind)
            ));
        }
        return Next::Reconcile(pending.clone());
    }
    let Some(binding) = &state.resource else {
        return Next::Wait;
    };
    let control = |kind: Operation, request: Request, initiator: &Principal| {
        let sequence = state.sequence + 1;
        Next::Send {
            pending: Pending {
                sequence,
                kind,
                key: keys::operation(
                    &identity.project,
                    &identity.board,
                    &identity.configuration,
                    hex,
                    kind,
                    sequence,
                ),
                digest: keys::digest(&request),
                request,
                initiator: initiator.clone(),
                sent: false,
                first_sent: None,
            },
            new: true,
        }
    };
    let bound = resource(identity, attempt, binding);
    if let Some(asked) = &state.cancel {
        // A provider that cannot cancel may still terminate: a hard stop is the cancel then.
        if !caps.operations.contains(&Operation::Cancel)
            && caps.operations.contains(&Operation::Stop)
            && caps.stop_modes.contains(&StopMode::Terminate)
        {
            let request = Request::Stop {
                resource: bound,
                mode: StopMode::Terminate,
            };
            return control(Operation::Stop, request, &asked.initiator);
        }
        let request = Request::Cancel { resource: bound };
        return control(Operation::Cancel, request, &asked.initiator);
    }
    if let Some(Asked {
        initiator,
        mode: Some(mode),
    }) = &state.stop
    {
        let request = Request::Stop {
            resource: bound,
            mode: *mode,
        };
        return control(Operation::Stop, request, initiator);
    }
    Next::Observe(binding.clone())
}

/// Attempt `state` held for a person, for `why`.
pub(crate) fn hold(state: &State, why: String) -> Applied {
    let mut applied = Applied::from_state(state.clone());
    applied.state.dispatch = Dispatch::Held;
    applied.state.held = Some(why.clone());
    applied.facts.push(Fact::Held { why });
    applied
}

/// What `outcome`, the answer to `sent` (the operation owed, sent or reconciled, or none for an
/// observation of the bound resource), makes of `state`. `answer` is the sequence number of the
/// fact that records the answer itself, which an ending names as its quiescence.
pub(crate) fn apply(
    identity: &Identity,
    state: &State,
    sent: Option<&Pending>,
    reconciled: bool,
    outcome: &Outcome,
    answer: u64,
    bounds: &Bounds,
) -> Applied {
    match sent {
        None => observed(identity, state, outcome, answer, bounds),
        Some(pending) => answered(state, pending, reconciled, outcome, answer, bounds),
    }
}

/// An answer to the owed operation `pending`.
fn answered(
    state: &State,
    pending: &Pending,
    reconciled: bool,
    outcome: &Outcome,
    answer: u64,
    bounds: &Bounds,
) -> Applied {
    let starts = matches!(pending.kind, Operation::Start | Operation::Resume);
    match outcome {
        Outcome::Accepted(receipt) if !reconciled => accepted(state, pending, receipt, bounds),
        Outcome::Found(receipt) if reconciled => accepted(state, pending, receipt, bounds),
        Outcome::Absent { .. } if reconciled => {
            let mut applied = Applied::from_state(state.clone());
            applied.state.unknowns = 0;
            if starts && state.cancellation == Cancellation::Requested {
                // Asked to cease, and the start never happened: nothing to stop.
                applied.state.pending = None;
                return ended(applied, Ending::Cancelled, answer);
            }
            // It never reached the provider, and the provider guarantees it never will under its
            // key: the same operation is sent again, by the same key.
            if let Some(owed) = applied.state.pending.as_mut() {
                owed.sent = false;
            }
            applied.due_in = Some(0);
            applied
        }
        Outcome::Conflicting { matches } if reconciled => hold(
            state,
            format!(
                "the provider holds {} resources for its {}: quarantined for a person",
                matches.len(),
                name(pending.kind)
            ),
        ),
        Outcome::Rejected(_) | Outcome::Unsupported(_) if reconciled => {
            let why = match outcome {
                Outcome::Rejected(rejection) => rejected(rejection),
                _ => "the provider cannot reconcile".to_string(),
            };
            hold(
                state,
                format!("its {} could not be reconciled: {why}", name(pending.kind)),
            )
        }
        Outcome::Rejected(_) | Outcome::Unsupported(_) => {
            // A definite refusal: nothing was made, or the control was not taken.
            let mut applied = Applied::from_state(state.clone());
            applied.state.pending = None;
            applied.state.unknowns = 0;
            if starts {
                return ended(applied, Ending::Rejected, answer);
            }
            if pending.kind == Operation::Cancel || state.cancel.is_some() {
                applied.state.cancel = None;
                applied.state.cancellation = Cancellation::Unconfirmed;
            }
            if pending.kind == Operation::Stop {
                applied.state.stop = None;
            }
            applied.due_in = Some(bounds.observe_ms);
            applied
        }
        Outcome::Unknown(Unknown::KeyExpired) => hold(
            state,
            format!(
                "its {} is unsettled and the provider's key has expired",
                name(pending.kind)
            ),
        ),
        _ => {
            // Unknown, a deadline, or an answer the kit found invalid: the operation may have
            // reached the provider. It is reconciled by its key, never sent anew.
            let mut applied = Applied::from_state(state.clone());
            if starts && applied.state.resource.is_none() {
                applied.state.dispatch = Dispatch::Unknown;
            }
            unsettled(applied, bounds, pending.kind)
        }
    }
}

/// One more answer that settled nothing: looked at again after a backoff, or held past the bound.
fn unsettled(mut applied: Applied, bounds: &Bounds, kind: Operation) -> Applied {
    applied.state.unknowns += 1;
    if applied.state.unknowns > bounds.unknowns {
        let why = format!(
            "{} answers in a row to its {} settled nothing",
            applied.state.unknowns,
            name(kind)
        );
        let held = hold(&applied.state, why);
        return Applied {
            facts: [applied.facts, held.facts].concat(),
            ..held
        };
    }
    applied.due_in = Some(bounds.backoff(applied.state.unknowns));
    applied
}

/// The owed operation `pending` taken by the provider, as `receipt` says.
fn accepted(state: &State, pending: &Pending, receipt: &Receipt, bounds: &Bounds) -> Applied {
    let mut applied = Applied::from_state(state.clone());
    applied.state.pending = None;
    applied.state.unknowns = 0;
    match pending.kind {
        Operation::Start | Operation::Resume => {
            let binding = Binding {
                id: receipt.resource.id.value().clone(),
                generation: receipt.resource.generation.clone(),
            };
            match &state.resource {
                Some(bound) if *bound != binding => {
                    return hold(
                        state,
                        format!(
                            "the provider bound resource {} at {} where {} at {} was bound",
                            binding.id, binding.generation, bound.id, bound.generation
                        ),
                    );
                }
                Some(_) => {}
                None => {
                    applied.facts.push(Fact::Bound {
                        resource: Some(binding.clone()),
                        conversation: None,
                    });
                    applied.state.resource = Some(binding);
                }
            }
            if matches!(
                applied.state.dispatch,
                Dispatch::Pending | Dispatch::Unknown
            ) {
                applied.state.dispatch = Dispatch::Starting;
            }
        }
        // A cancel or a stop taken is no proof that it ceased: it is observed until it has.
        Operation::Cancel => applied.state.cancel = None,
        Operation::Stop => {
            applied.state.stop = None;
            if state.cancel.is_some() {
                applied.state.cancel = None;
            }
        }
        _ => {}
    }
    applied.due_in = Some(
        if applied.state.cancel.is_some() || applied.state.stop.is_some() {
            0
        } else {
            bounds.observe_ms
        },
    );
    applied
}

/// An observation of the bound resource.
fn observed(
    identity: &Identity,
    state: &State,
    outcome: &Outcome,
    answer: u64,
    bounds: &Bounds,
) -> Applied {
    let mut applied = Applied::from_state(state.clone());
    let observation = match outcome {
        Outcome::Observed(observation) => observation,
        Outcome::Rejected(_) | Outcome::Unsupported(_) => {
            return hold(state, "the provider cannot observe it".to_string());
        }
        _ => return unsettled(applied, bounds, Operation::Observe),
    };
    if state.dispatch == Dispatch::Terminal {
        return applied;
    }
    if let (Some(seen), Some(last)) = (&observation.cursor, &state.cursor) {
        let order = |cursor: &String| (cursor.len(), cursor.clone());
        if seen == last || (identity.capabilities.ordered_observations && order(seen) < order(last))
        {
            // A replay, or one older than what it said already: nothing new.
            applied.due_in = Some(bounds.observe_ms);
            return applied;
        }
    }
    let next = &mut applied.state;
    if observation.cursor.is_some() {
        next.cursor.clone_from(&observation.cursor);
    }
    if let Some(conversation) = observation.conversation.as_ref().map(|c| c.value().clone())
        && next.conversation.as_ref() != Some(&conversation)
    {
        next.conversation = Some(conversation.clone());
        applied.facts.push(Fact::Bound {
            resource: None,
            conversation: Some(conversation),
        });
    }
    let next = &mut applied.state;
    for (field, value) in [
        (&mut next.checkpoint, &observation.checkpoint),
        (&mut next.completion, &observation.completion),
        (&mut next.manifest, &observation.artifacts),
    ] {
        if value.is_some() {
            field.clone_from(value);
        }
    }
    next.unknowns = 0;
    match observation.state {
        RuntimeState::Provisioning => {
            if matches!(next.dispatch, Dispatch::Pending | Dispatch::Unknown) {
                next.dispatch = Dispatch::Starting;
            }
        }
        RuntimeState::Running => next.dispatch = Dispatch::Running,
        RuntimeState::Paused => {
            next.dispatch = Dispatch::Paused;
            next.quiescent = Some(answer);
            if next
                .stop
                .as_ref()
                .is_some_and(|asked| asked.mode == Some(StopMode::Pause))
            {
                next.stop = None;
            }
        }
        RuntimeState::Succeeded => return ended(applied, Ending::Succeeded, answer),
        RuntimeState::Failed => return ended(applied, Ending::Failed, answer),
        RuntimeState::Terminated | RuntimeState::Destroyed => {
            let ending = if state.cancellation == Cancellation::Requested {
                Ending::Cancelled
            } else {
                Ending::Stopped
            };
            return ended(applied, ending, answer);
        }
        RuntimeState::Unknown => {
            // Lost contact is not a failure.
            return unsettled(applied, bounds, Operation::Observe);
        }
        _ => return unsettled(applied, bounds, Operation::Observe),
    }
    // A cancel or a stop asked for while it was being observed is sent at once.
    let asked = next.cancel.is_some() || next.stop.is_some();
    applied.due_in = Some(if asked { 0 } else { bounds.observe_ms });
    applied
}

/// `applied` ended as `ending`, known quiescent at fact `answer`: its reservation given back, a
/// cancel asked for confirmed, nothing more owed.
fn ended(mut applied: Applied, ending: Ending, answer: u64) -> Applied {
    let next = &mut applied.state;
    next.dispatch = Dispatch::Terminal;
    next.ending = Some(ending);
    next.quiescent = Some(answer);
    next.pending = None;
    next.cancel = None;
    next.stop = None;
    if matches!(
        next.cancellation,
        Cancellation::Requested | Cancellation::Unconfirmed
    ) {
        next.cancellation = Cancellation::Confirmed;
    }
    applied.release = next.reserved.take();
    applied.facts.push(Fact::Ended { ending });
    applied.due_in = None;
    applied
}

/// What a cancel asked for by `initiator` makes of `state`, or none when it was asked already.
/// `fact` is the sequence number the first fact it appends will take.
pub(crate) fn cancel(state: &State, initiator: &Principal, fact: u64) -> Option<Applied> {
    if state.cancellation != Cancellation::None {
        return None;
    }
    let mut applied = Applied::from_state(state.clone());
    applied.facts.push(Fact::Asked {
        initiator: initiator.clone(),
        cancel: true,
        mode: None,
    });
    let unsent_start = state.pending.as_ref().is_some_and(|pending| {
        !pending.sent && matches!(pending.kind, Operation::Start | Operation::Resume)
    });
    match state.dispatch {
        // It ended already: confirmed at once, and no result of it is eligible from now on.
        Dispatch::Terminal => applied.state.cancellation = Cancellation::Confirmed,
        // Nothing was sent: it ends here, with nothing provisioned.
        Dispatch::Pending if unsent_start => {
            applied.state.cancellation = Cancellation::Requested;
            applied.state.pending = None;
            return Some(ended(applied, Ending::Cancelled, fact));
        }
        _ => {
            applied.state.cancellation = Cancellation::Requested;
            applied.state.cancel = Some(Asked {
                initiator: initiator.clone(),
                mode: None,
            });
            applied.due_in = Some(0);
        }
    }
    Some(applied)
}

/// What a stop in `mode` asked for by `initiator` makes of `state`, or why it is refused.
pub(crate) fn stop(
    identity: &Identity,
    state: &State,
    initiator: &Principal,
    mode: StopMode,
) -> Result<Applied, String> {
    if state.dispatch == Dispatch::Terminal {
        return Err("it has ended".into());
    }
    if state.resource.is_none() {
        return Err("no resource is bound to it yet: cancel it instead".into());
    }
    let caps = &identity.capabilities;
    if !caps.operations.contains(&Operation::Stop) || !caps.stop_modes.contains(&mode) {
        return Err(format!("its provider cannot {}", stop_name(mode)));
    }
    let mut applied = Applied::from_state(state.clone());
    applied.state.stop = Some(Asked {
        initiator: initiator.clone(),
        mode: Some(mode),
    });
    applied.facts.push(Fact::Asked {
        initiator: initiator.clone(),
        cancel: false,
        mode: Some(mode),
    });
    applied.due_in = Some(0);
    Ok(applied)
}

/// The binding and the fact of quiescence a resume continues from, or why `state` cannot be
/// resumed: it must be known paused or ended, owe nothing, and have no cancel whose outcome is
/// still uncertain.
pub(crate) fn resumable(state: &State) -> Result<(Binding, u64), String> {
    if let Some(successor) = &state.successor {
        return Err(format!("attempt {successor} followed it already"));
    }
    if state.pending.is_some() || state.stop.is_some() || state.cancel.is_some() {
        return Err("an operation of it is not settled yet".into());
    }
    if matches!(
        state.cancellation,
        Cancellation::Requested | Cancellation::Unconfirmed
    ) {
        return Err("its cancellation is not confirmed".into());
    }
    let quiescent = match (state.dispatch, state.ending) {
        (Dispatch::Paused, _)
        | (
            Dispatch::Terminal,
            Some(Ending::Stopped | Ending::Succeeded | Ending::Failed | Ending::Cancelled),
        ) => state.quiescent,
        _ => None,
    };
    let (Some(binding), Some(quiescent)) = (&state.resource, quiescent) else {
        return Err("it is not known to be paused or ended with a resource".into());
    };
    Ok((binding.clone(), quiescent))
}

/// An operation's name in a reason.
fn name(kind: Operation) -> &'static str {
    match kind {
        Operation::Start => "start",
        Operation::Reconcile => "reconcile",
        Operation::Observe => "observation",
        Operation::Cancel => "cancel",
        Operation::Stop => "stop",
        Operation::Resume => "resume",
        Operation::ExternalLink => "link",
        _ => "operation",
    }
}

fn stop_name(mode: StopMode) -> &'static str {
    match mode {
        StopMode::Pause => "pause",
        StopMode::Terminate => "terminate",
        StopMode::HardStop => "hard-stop",
        _ => "stop",
    }
}

/// A refusal's reason.
fn rejected(rejection: &Rejection) -> String {
    match rejection {
        Rejection::Provider(why) => format!("the provider refused: {why}"),
        other => format!("refused before any I/O: {other:?}"),
    }
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
