use serde::{Deserialize, Serialize};

use super::{AttemptRef, ContentRef, ConversationId, Input, Principal, Resource};

/// Separate capabilities: viewing does not grant control, local focus or local screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Operation {
    Start,
    Reconcile,
    Observe,
    Cancel,
    Stop,
    Resume,
    ExternalLink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum StopMode {
    Pause,
    Terminate,
    HardStop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ResumeMode {
    NewResource,
    ReuseWithNewGeneration,
}

/// Provider guarantees pinned with its configuration on admission. Defaults advertise nothing.
/// Retention is the provider's guarantee from first acceptance, not permission to retry forever.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(default)]
pub struct Capabilities {
    pub operations: Vec<Operation>,
    pub stop_modes: Vec<StopMode>,
    pub resume_modes: Vec<ResumeMode>,
    /// Exact composite operation identity (scope/configuration/attempt/kind/sequence/key/input).
    pub idempotency_retention_ms: Option<u64>,
    pub authoritative_absence: bool,
    pub ordered_observations: bool,
    pub artifact_enumeration: bool,
    pub artifact_read: bool,
    pub external_view: bool,
    pub local_focus: bool,
    pub local_screen: bool,
}

impl Capabilities {
    pub fn supports(&self, request: &Request) -> bool {
        self.operations.contains(&request.operation())
            && match request {
                Request::Stop { mode, .. } => self.stop_modes.contains(mode),
                Request::Resume { mode, .. } | Request::ReconcileResume { mode, .. } => {
                    self.resume_modes.contains(mode)
                }
                Request::ExternalLink { .. } => self.external_view,
                _ => true,
            }
    }
}

/// Persisted before I/O. A reconciliation reuses the original kind, sequence, key and input.
/// Worker fencing and runtime epoch are distinct; these values implement no persistence or lease.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OperationIdentity {
    pub attempt: AttemptRef,
    pub configuration: String,
    pub input_digest: String,
    pub key: String,
    pub kind: Operation,
    pub sequence: u64,
    pub fencing_epoch: u64,
    pub runtime_epoch: u64,
}

impl OperationIdentity {
    pub fn new(
        attempt: AttemptRef,
        configuration: String,
        input_digest: String,
        key: String,
        kind: Operation,
        sequence: u64,
    ) -> Self {
        Self {
            attempt,
            configuration,
            input_digest,
            key,
            kind,
            sequence,
            fencing_epoch: 0,
            runtime_epoch: 0,
        }
    }

    #[must_use]
    pub fn with_epochs(mut self, fencing_epoch: u64, runtime_epoch: u64) -> Self {
        self.fencing_epoch = fencing_epoch;
        self.runtime_epoch = runtime_epoch;
        self
    }
}

/// A predecessor whose stop/cancellation has been authoritatively settled by the coordinator.
/// `quiescence` references that durable evidence; it is not proof produced by this kit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ResumeFrom {
    pub resource: Resource,
    pub quiescence: String,
    pub checkpoint: Option<ContentRef>,
    pub conversation: Option<ConversationId>,
}

impl ResumeFrom {
    pub fn new(resource: Resource, quiescence: String) -> Self {
        Self {
            resource,
            quiescence,
            checkpoint: None,
            conversation: None,
        }
    }
}

/// One short provider operation. No request waits for a job to complete. A provider that creates
/// an environment and submits work separately records separately keyed steps with its coordinator.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    Start {
        input: Input,
    },
    Reconcile {
        resource: Option<Resource>,
    },
    /// Reconcile an original Resume with its persisted predecessor/mode intact. Since execution
    /// minor 1; a plain Reconcile cannot recover a Resume because it loses these constraints.
    ReconcileResume {
        resource: Option<Resource>,
        from: ResumeFrom,
        mode: ResumeMode,
    },
    Observe {
        resource: Resource,
        cursor: Option<String>,
    },
    Cancel {
        resource: Resource,
    },
    Stop {
        resource: Resource,
        mode: StopMode,
    },
    Resume {
        input: Input,
        from: ResumeFrom,
        mode: ResumeMode,
    },
    ExternalLink {
        resource: Resource,
        conversation: Option<ConversationId>,
    },
}

impl Request {
    /// Earliest minor of execution contract major 1 that can carry this request. A host must
    /// check it against its pinned negotiated minor before forwarding to another process.
    pub fn since(&self) -> u32 {
        match self {
            Self::ReconcileResume { .. } => 1,
            _ => 0,
        }
    }

    pub fn operation(&self) -> Operation {
        match self {
            Self::Start { .. } => Operation::Start,
            Self::Reconcile { .. } | Self::ReconcileResume { .. } => Operation::Reconcile,
            Self::Observe { .. } => Operation::Observe,
            Self::Cancel { .. } => Operation::Cancel,
            Self::Stop { .. } => Operation::Stop,
            Self::Resume { .. } => Operation::Resume,
            Self::ExternalLink { .. } => Operation::ExternalLink,
        }
    }

    pub(crate) fn resource(&self) -> Option<&Resource> {
        match self {
            Self::Start { .. } | Self::Resume { .. } => None,
            Self::Reconcile { resource } | Self::ReconcileResume { resource, .. } => {
                resource.as_ref()
            }
            Self::Observe { resource, .. }
            | Self::Cancel { resource }
            | Self::Stop { resource, .. }
            | Self::ExternalLink { resource, .. } => Some(resource),
        }
    }

    pub(crate) fn resume(&self) -> Option<(&ResumeFrom, ResumeMode)> {
        match self {
            Self::Resume { from, mode, .. } | Self::ReconcileResume { from, mode, .. } => {
                Some((from, *mode))
            }
            _ => None,
        }
    }
}

/// Initiator comes from the persisted intent; current effect authorization names the acting
/// principal separately. Neither an input manifest nor a claimed owner changes that authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Call {
    pub identity: OperationIdentity,
    pub initiator: Principal,
    pub request: Request,
}

impl Call {
    pub fn new(identity: OperationIdentity, initiator: Principal, request: Request) -> Self {
        Self {
            identity,
            initiator,
            request,
        }
    }
}
