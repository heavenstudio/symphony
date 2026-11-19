use serde::{Deserialize, Serialize};

use super::{ContentRef, ConversationId, Operation, OperationIdentity, Resource};

/// Acceptance of this operation only: not task delivery, completion, quiescence or a stamp.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Receipt {
    pub identity: OperationIdentity,
    pub resource: Resource,
    pub evidence: String,
}

impl Receipt {
    pub fn new(identity: OperationIdentity, resource: Resource, evidence: String) -> Self {
        Self {
            identity,
            resource,
            evidence,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RuntimeState {
    Provisioning,
    Running,
    Paused,
    Succeeded,
    Failed,
    Terminated,
    Destroyed,
    Unknown,
}

/// One bounded observation, not a stream or an artifact download. Cursors are opaque and ordered
/// only when advertised; the coordinator still checks current revision, generation and epochs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Observation {
    pub resource: Resource,
    pub state: RuntimeState,
    pub cursor: Option<String>,
    pub conversation: Option<ConversationId>,
    pub checkpoint: Option<ContentRef>,
    pub completion: Option<ContentRef>,
    pub artifacts: Option<ContentRef>,
}

impl Observation {
    pub fn new(resource: Resource, state: RuntimeState) -> Self {
        Self {
            resource,
            state,
            cursor: None,
            conversation: None,
            checkpoint: None,
            completion: None,
            artifacts: None,
        }
    }
}

/// A current-authorized view only, with a bounded lifetime. Never a control or provider token.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ExternalLink {
    pub resource: Resource,
    pub url: String,
    pub valid_for_ms: u64,
}

impl ExternalLink {
    pub fn new(resource: Resource, url: String, valid_for_ms: u64) -> Self {
        Self {
            resource,
            url,
            valid_for_ms,
        }
    }
}

/// Definite local refusal or provider rejection. Unknown I/O is never a rejection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Rejection {
    Unauthorized,
    Scope,
    Input,
    Identity,
    Resume,
    Provider(String),
}

/// Every ambiguous result requires reconciliation/intervention, never a fresh blind start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Unknown {
    Deadline,
    ContactLost,
    Visibility,
    KeyExpired,
    InvalidReply,
    Other(String),
}

/// Outcomes keep rejection, authoritative absence, ambiguity and conflicting matches distinct.
/// `Absent` guarantees this operation cannot later be accepted, and is only a reconcile answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Outcome {
    Accepted(Receipt),
    Found(Receipt),
    Absent { evidence: String },
    Conflicting { matches: Vec<Receipt> },
    Observed(Observation),
    Link(ExternalLink),
    Unsupported(Operation),
    Rejected(Rejection),
    Unknown(Unknown),
}
