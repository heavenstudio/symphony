//! Provider-neutral, bounded execution operations. This contract supplies no dispatcher, durable
//! attempt store, credential verifier, capacity reservation or runtime fencing implementation.
//! A host persists intent before I/O and rechecks current authority on every operation. Jobs
//! outlive these calls and any process RPC. Notification [`crate::Transport`] is independent.
//!
//! Public data is constructed through constructors/defaults; structs and enums leave room for
//! additive Rust API changes. Wire changes require the separately negotiated [`wire::VERSION`].

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

mod identity;
mod outcome;
mod request;
pub mod wire;

pub use identity::{
    AttemptId, AttemptRef, Authorization, ContentRef, ConversationId, Input, Principal,
    PrincipalKind, Resource, ResourceId, Scope,
};
pub use outcome::{ExternalLink, Observation, Outcome, Receipt, Rejection, RuntimeState, Unknown};
pub use request::{
    Call, Capabilities, Operation, OperationIdentity, Request, ResumeFrom, ResumeMode, StopMode,
};

use crate::protocol::Version;

/// A pinned provider configuration and its negotiated contract/capability snapshot. Configuration
/// identifies stable account/environment scope; secrets stay in the adapter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Profile {
    pub scope: Scope,
    pub configuration: String,
    pub contract: Version,
    pub capabilities: Capabilities,
}

impl Profile {
    pub fn new(scope: Scope, configuration: String, capabilities: Capabilities) -> Self {
        Self {
            scope,
            configuration,
            contract: wire::VERSION,
            capabilities,
        }
    }
}

/// The largest per-call budget. This bounds adapter I/O, never the execution's lifetime.
pub const MAX_CALL_MS: u32 = 30_000;

/// Nonzero bounded relative budget on the wire. Each host also bounds its transport end to end;
/// a receiver must not turn retransmissions into an extension of the sender's original deadline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct CallLimit(u32);

impl CallLimit {
    pub fn new(milliseconds: u32) -> Result<Self, String> {
        if milliseconds == 0 || milliseconds > MAX_CALL_MS {
            return Err(format!(
                "execution call budget must be 1..={MAX_CALL_MS} ms"
            ));
        }
        Ok(Self(milliseconds))
    }

    pub fn milliseconds(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for CallLimit {
    type Error = String;
    fn try_from(value: u32) -> Result<Self, String> {
        Self::new(value)
    }
}

impl From<CallLimit> for u32 {
    fn from(value: CallLimit) -> Self {
        value.0
    }
}

/// Project-owned trusted code. `execute` MUST return by `deadline`, bounding all its I/O and
/// returning a receipt/observation without waiting for the job. Timeout/contact loss is Unknown.
/// Duplicate keys with identical input converge; mismatched input is rejected. The adapter must
/// honor its advertised retention/reconciliation/generation guarantees; this trait cannot kill
/// arbitrary linked code or implement a provider's idempotency. Host transports enforce their
/// own deadline and reconcile an abandoned call, never tying a job's life to an open pipe.
pub trait Adapter: Send + Sync {
    fn execute(&self, call: &Call, authorization: &Authorization, deadline: Instant) -> Outcome;
}

/// Validate a bounded call, then invoke the adapter. A late answer is ambiguous, even if it says
/// accepted. The host still owns persistence, fresh authorization and the pinned profile's origin.
pub fn invoke(
    adapter: &dyn Adapter,
    profile: &Profile,
    authorization: &Authorization,
    call: &Call,
    deadline: Instant,
) -> Outcome {
    invoke_with_clock(
        adapter,
        profile,
        authorization,
        call,
        deadline,
        Instant::now,
    )
}

fn invoke_with_clock(
    adapter: &dyn Adapter,
    profile: &Profile,
    authorization: &Authorization,
    call: &Call,
    deadline: Instant,
    mut now: impl FnMut() -> Instant,
) -> Outcome {
    let started = now();
    if started >= deadline
        || deadline.duration_since(started) > Duration::from_millis(u64::from(MAX_CALL_MS))
    {
        return Outcome::Unknown(Unknown::Deadline);
    }
    if let Err(reason) = validate(profile, authorization, call) {
        return Outcome::Rejected(reason);
    }
    if !supported(profile, &call.request) {
        return Outcome::Unsupported(call.request.operation());
    }
    let outcome = adapter.execute(call, authorization, deadline);
    if now() >= deadline {
        return Outcome::Unknown(Unknown::Deadline);
    }
    if valid_outcome(profile, call, &outcome) {
        outcome
    } else {
        Outcome::Unknown(Unknown::InvalidReply)
    }
}

fn same_resource_scope(resource: &Resource, identity: &OperationIdentity) -> bool {
    resource.attempt == identity.attempt
        && resource.configuration == identity.configuration
        && !resource.id.value().is_empty()
        && !resource.generation.is_empty()
}

fn validate(profile: &Profile, auth: &Authorization, call: &Call) -> Result<(), Rejection> {
    if !auth.permits(call)
        || auth.principal().issuer.is_empty()
        || auth.principal().subject.is_empty()
        || auth.policy_revision().is_empty()
    {
        return Err(Rejection::Unauthorized);
    }
    let id = &call.identity;
    if profile.scope != id.attempt.scope || profile.configuration != id.configuration {
        return Err(Rejection::Scope);
    }
    if wire::negotiate(profile.contract).ok() != Some(profile.contract)
        || id.key.is_empty()
        || id.input_digest.is_empty()
        || id.configuration.is_empty()
        || id.attempt.task.is_empty()
        || id.attempt.scope.project.is_empty()
        || id.attempt.scope.board.is_empty()
        || (call.request.operation() != Operation::Reconcile && id.kind != call.request.operation())
    {
        return Err(Rejection::Identity);
    }
    match &call.request {
        Request::Reconcile { .. } if id.kind == Operation::Resume => return Err(Rejection::Resume),
        Request::ReconcileResume { .. } if id.kind != Operation::Resume => {
            return Err(Rejection::Identity);
        }
        _ => {}
    }
    if let Some(resource) = call.request.resource()
        && !same_resource_scope(resource, id)
    {
        return Err(Rejection::Scope);
    }
    match &call.request {
        Request::Start { input } | Request::Resume { input, .. }
            if input.digest != id.input_digest
                || input.task_revision.is_empty()
                || input.repository_revision.is_empty()
                || input.manifest.reference.is_empty()
                || input.manifest.digest.is_empty() =>
        {
            return Err(Rejection::Input);
        }
        _ => {}
    }
    if let Some((from, _)) = call.request.resume() {
        let previous = &from.resource;
        if previous.attempt.scope != id.attempt.scope
            || previous.attempt.task != id.attempt.task
            || previous.configuration != id.configuration
            || previous.attempt.id == id.attempt.id
            || previous.id.value().is_empty()
            || previous.generation.is_empty()
            || from.quiescence.is_empty()
            || (from.checkpoint.is_none() && from.conversation.is_none())
            || from.checkpoint.as_ref().is_some_and(|checkpoint| {
                checkpoint.reference.is_empty() || checkpoint.digest.is_empty()
            })
            || from
                .conversation
                .as_ref()
                .is_some_and(|conversation| conversation.value().is_empty())
        {
            return Err(Rejection::Resume);
        }
    }
    Ok(())
}

fn valid_receipt(call: &Call, receipt: &Receipt) -> bool {
    if receipt.identity != call.identity
        || !same_resource_scope(&receipt.resource, &call.identity)
        || receipt.evidence.is_empty()
    {
        return false;
    }
    if let Some(resource) = call.request.resource()
        && resource != &receipt.resource
    {
        return false;
    }
    if let Some((from, mode)) = call.request.resume() {
        return match mode {
            ResumeMode::NewResource => receipt.resource.id != from.resource.id,
            ResumeMode::ReuseWithNewGeneration => {
                receipt.resource.id == from.resource.id
                    && receipt.resource.generation != from.resource.generation
            }
        };
    }
    true
}

fn valid_outcome(profile: &Profile, call: &Call, outcome: &Outcome) -> bool {
    let operation = call.request.operation();
    if !supported(profile, &call.request) {
        return matches!(outcome, Outcome::Unsupported(unsupported) if *unsupported == operation);
    }
    match outcome {
        Outcome::Accepted(receipt) => {
            matches!(
                operation,
                Operation::Start | Operation::Cancel | Operation::Stop | Operation::Resume
            ) && valid_receipt(call, receipt)
        }
        Outcome::Found(receipt) => {
            operation == Operation::Reconcile && valid_receipt(call, receipt)
        }
        Outcome::Absent { evidence } => {
            operation == Operation::Reconcile
                && profile.capabilities.authoritative_absence
                && !evidence.is_empty()
        }
        // Conflicting resources remain scoped to this operation and are quarantined, never promoted.
        Outcome::Conflicting { matches } => {
            operation == Operation::Reconcile
                && matches.len() > 1
                && matches.iter().all(|receipt| {
                    receipt.identity == call.identity
                        && same_resource_scope(&receipt.resource, &call.identity)
                        && !receipt.evidence.is_empty()
                })
        }
        Outcome::Observed(observation) => {
            operation == Operation::Observe
                && call.request.resource() == Some(&observation.resource)
        }
        Outcome::Link(link) => {
            operation == Operation::ExternalLink
                && call.request.resource() == Some(&link.resource)
                && !link.url.is_empty()
                && link.valid_for_ms > 0
                && link.valid_for_ms <= u64::from(MAX_CALL_MS)
        }
        Outcome::Unsupported(unsupported) => *unsupported == operation,
        Outcome::Rejected(_) | Outcome::Unknown(_) => true,
    }
}

fn supported(profile: &Profile, request: &Request) -> bool {
    request.since() <= profile.contract.minor && profile.capabilities.supports(request)
}

#[cfg(test)]
mod tests;
