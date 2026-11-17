//! Owned process-boundary data, independent of the UI plugin protocol. A host and worker must
//! explicitly negotiate this contract before forwarding; existing plugin processes advertise
//! no execution capability. This module starts no process/job and exposes no HTTP endpoint.
//!
//! Envelopes travel ONLY over an authenticated host channel. Deserializing Authorization is not
//! authentication. Unknown fields are ignored for additive minors; missing required fields and
//! unknown variants fail closed. A new required field/semantic change requires a new major.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{
    Adapter, Authorization, Call, CallLimit, OperationIdentity, Outcome, Profile, Unknown,
    invoke_with_clock, valid_outcome, validate,
};
use crate::protocol::Version;

/// Execution contract version, negotiated independently of `protocol::PROTOCOL`.
pub const VERSION: Version = Version::new(1, 0);

/// Negotiate a common execution minor, refusing incompatible majors before any operation.
pub fn negotiate(peer: Version) -> Result<Version, String> {
    if peer.major != VERSION.major {
        return Err(format!(
            "execution contract {peer} is incompatible with {VERSION}"
        ));
    }
    Ok(Version::new(VERSION.major, peer.minor.min(VERSION.minor)))
}

/// One bounded authenticated host request. An agent's input manifest is never this envelope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Envelope {
    pub contract: Version,
    pub authorization: Authorization,
    pub call: Call,
    pub limit: CallLimit,
}

/// Every answer, including absence/unknown/unsupported, is bound to the original operation and
/// worker/runtime epochs. A transport associates it with its outstanding request as well.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Answer {
    pub contract: Version,
    pub identity: OperationIdentity,
    pub outcome: Outcome,
}

impl Answer {
    pub fn new(identity: OperationIdentity, outcome: Outcome) -> Self {
        Self {
            contract: VERSION,
            identity,
            outcome,
        }
    }
}

impl Envelope {
    pub fn new(authorization: Authorization, call: Call, limit: CallLimit) -> Self {
        Self {
            contract: VERSION,
            authorization,
            call,
            limit,
        }
    }

    /// Dispatch after channel authentication. The sender also enforces its original deadline.
    /// Decode/transport failures are unknown to the sender, never definite provider rejection.
    pub fn dispatch(&self, adapter: &dyn Adapter, pinned: &Profile) -> Result<Answer, String> {
        self.dispatch_with_clock(adapter, pinned, Instant::now)
    }

    pub(super) fn dispatch_with_clock(
        &self,
        adapter: &dyn Adapter,
        pinned: &Profile,
        mut now: impl FnMut() -> Instant,
    ) -> Result<Answer, String> {
        let agreed = negotiate(self.contract)?;
        if self.contract != agreed || pinned.contract != agreed {
            return Err("execution request must use the negotiated pinned version".into());
        }
        let deadline = now() + Duration::from_millis(u64::from(self.limit.milliseconds()));
        let outcome = invoke_with_clock(
            adapter,
            pinned,
            &self.authorization,
            &self.call,
            deadline,
            now,
        );
        Ok(Answer::new(self.call.identity.clone(), outcome))
    }

    /// Read a response only within the sender's original transport deadline. Stale/wrong
    /// identities and incompatible outcomes remain ambiguous; they never authorize another start.
    pub fn accept(&self, pinned: &Profile, answer: Answer) -> Outcome {
        if answer.contract != self.contract
            || pinned.contract != self.contract
            || answer.identity != self.call.identity
            || validate(pinned, &self.authorization, &self.call).is_err()
            || !valid_outcome(pinned, &self.call, &answer.outcome)
        {
            return Outcome::Unknown(Unknown::InvalidReply);
        }
        answer.outcome
    }
}
