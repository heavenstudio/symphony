//! What an attempt's row holds: its [`Identity`], written once at admission and never again, and
//! its [`State`], a projection every write replaces under the row's revision; and the [`Fact`]s
//! appended beside it, which no write changes or removes.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use symphony_plugin::execution::{
    Capabilities, ContentRef, Input, Operation, Outcome, Principal, Request, ResumeMode, StopMode,
};
use symphony_plugin::protocol::Version;

/// How an attempt follows the one before it in its lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    /// A new execution of the same input, after a person settled the one before.
    Retry,
    /// A continuation of a stopped or paused predecessor, from its checkpoint or conversation.
    Resume,
}

/// An attempt as it was admitted. Nothing writes it again: a changed input, a retry or a resume
/// is another attempt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub project: String,
    pub board: String,
    pub task: String,
    /// The lane it holds while it may run: one attempt at a time per task and lane.
    pub lane: String,
    pub role: String,
    /// The agent handle it runs as, when it has one.
    pub handle: Option<String>,
    /// The attempt before it in its lane, and how it follows it.
    pub predecessor: Option<String>,
    pub relation: Option<Relation>,
    /// What a person decided to admit a retry: the evidence that its predecessor was found or
    /// isolated.
    pub decision: Option<String>,
    /// The task's input, pinned: its revision, digest, repository revision and manifest.
    pub input: Input,
    /// The provider configuration it runs under, and the contract and capabilities pinned then.
    pub configuration: String,
    pub contract: Version,
    pub capabilities: Capabilities,
    /// Who asked for it, as the host authenticated them, and the policy that let them.
    pub initiator: Principal,
    pub policy_revision: String,
    /// The caller's request key, when it gave one: the same key admits the same attempt.
    pub request_key: Option<String>,
    /// The runtime's epoch: a resume runs at its predecessor's next, so the predecessor's runtime
    /// cannot write for it.
    pub runtime_epoch: u64,
    /// The capacity scope it reserved at admission, if any.
    pub capacity: Option<String>,
}

/// Where its execution is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dispatch {
    /// Admitted; nothing sent yet.
    #[default]
    Pending,
    /// The provider accepted it, or reports it provisioning.
    Starting,
    /// What the provider did is not known yet: it is being reconciled.
    Unknown,
    Running,
    Paused,
    /// Waiting on a person: an ambiguity recovery could not settle within its bounds.
    Held,
    /// Ended, with an [`Ending`].
    Terminal,
}

/// How a terminal attempt ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ending {
    Succeeded,
    Failed,
    Cancelled,
    Stopped,
    /// The provider refused to start it: no resource was made.
    Rejected,
    /// Another attempt took its place while it was held or paused: a retry a person admitted
    /// once they found it or isolated it, or a resume of it.
    Superseded,
}

/// Whether it was asked to cease, and whether it did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cancellation {
    #[default]
    None,
    /// Asked: from then on it publishes no result, whatever the provider says.
    Requested,
    /// It is known to have ended.
    Confirmed,
    /// The provider could not be asked, or refused: it may still be running.
    Unconfirmed,
}

/// Its artifacts' collection, which the project's storage does and reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Collection {
    #[default]
    NotStarted,
    Collecting,
    Complete,
    /// Some artifacts collected, not all.
    Partial,
    Failed,
    /// Nothing to collect, with evidence that there was nothing.
    None,
}

impl Collection {
    /// Whether collection has come to an end, so the compute may be cleaned up.
    pub fn settled(self) -> bool {
        matches!(
            self,
            Collection::Complete | Collection::Partial | Collection::Failed | Collection::None
        )
    }
}

/// Its resource's cleanup, which the project's hook does and reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cleanup {
    #[default]
    NotDue,
    Running,
    Complete,
    Failed,
    Unknown,
}

/// A resource bound to the attempt, at its generation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub id: String,
    pub generation: String,
}

/// An operation the attempt owes the provider, recorded before any I/O for it: performed while
/// not `sent`, reconciled by its key once it may have been.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub sequence: u64,
    pub kind: Operation,
    pub key: String,
    /// The request as first made; a repeat sends it again whole, with the same key.
    pub request: Request,
    pub digest: String,
    /// Who asked for it.
    pub initiator: Principal,
    pub sent: bool,
    /// When it was first sent, by the database's clock (ms): the provider's key retention runs
    /// from then.
    pub first_sent: Option<i64>,
    /// A stop that stands in for the cancel asked, on a provider that cannot cancel: taken, it
    /// settles the cancel; any other stop leaves a cancel asked meanwhile to be sent after it.
    #[serde(default)]
    pub cancels: bool,
}

impl Pending {
    /// Whether it is the cancel asked, or the stop that stands in for it.
    pub fn cancels(&self) -> bool {
        self.kind == Operation::Cancel || self.cancels
    }
}

/// A stop asked for, which the worker sends once a resource is bound.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asked {
    pub initiator: Principal,
    pub mode: Option<StopMode>,
}

/// An attempt's current state: each dimension apart, so no one flag means all of them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub dispatch: Dispatch,
    pub ending: Option<Ending>,
    pub cancellation: Cancellation,
    pub collection: Collection,
    pub cleanup: Cleanup,
    pub resource: Option<Binding>,
    pub conversation: Option<String>,
    pub cursor: Option<String>,
    pub checkpoint: Option<ContentRef>,
    pub completion: Option<ContentRef>,
    pub manifest: Option<ContentRef>,
    /// The last operation sequence given out.
    pub sequence: u64,
    pub pending: Option<Pending>,
    /// Answers in a row that settled nothing: past the coordinator's bound, the attempt is held.
    pub unknowns: u32,
    /// Why it waits on a person.
    pub held: Option<String>,
    /// The cancellation asked for, by whom, until the worker sends it.
    pub cancel: Option<Asked>,
    /// The stop asked for, until the worker sends it.
    pub stop: Option<Asked>,
    /// The fact that recorded the attempt quiescent (paused or ended), which a resume names.
    pub quiescent: Option<u64>,
    /// The capacity scope it still holds a reservation in.
    pub reserved: Option<String>,
    /// The key of the cleanup under way.
    pub cleanup_key: Option<String>,
    /// The cleanups begun, for the next key.
    pub cleanups: u64,
    /// The attempt that followed it in its lane, a resume or a retry.
    pub successor: Option<String>,
    /// The resume its resource was handed to, which reuses it at a new generation: the
    /// resource's cleanup is that attempt's, unless it ended without taking it.
    #[serde(default)]
    pub resource_handed_to: Option<String>,
}

impl State {
    /// Whether its runtime may still be alive: it has not ended.
    pub fn live(&self) -> bool {
        self.dispatch != Dispatch::Terminal
    }

    /// Whether anything is still owed for it: it is live, its collection is under way, or a
    /// resource bound to it, and not handed to a resume, is not known to be cleaned up.
    pub fn open(&self) -> bool {
        self.live()
            || self.collection == Collection::Collecting
            || (self.resource.is_some()
                && self.resource_handed_to.is_none()
                && self.cleanup != Cleanup::Complete)
    }
}

/// The lease a worker holds on an attempt while it acts for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub owner: String,
    pub epoch: u64,
    /// When it runs out, by the database's clock (ms).
    pub expires: i64,
}

/// An attempt's row as the store holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Row {
    pub attempt: String,
    pub task: String,
    pub created: i64,
    pub identity: Identity,
    pub state: State,
    pub revision: i64,
    pub open: bool,
    pub live: bool,
    #[serde(default)]
    pub due: Option<i64>,
    #[serde(default)]
    pub lease: Option<Lease>,
    pub epoch: u64,
    pub facts: i64,
    /// Where the workflow held its task when it was admitted, none where no workflow held it: the
    /// position its ending is proposed from, and only while its task still stands there
    /// (`crate::Store::observe_attempt`).
    #[serde(default)]
    pub workflow: Option<Pin>,
}

/// The pair and revision of the workflow position an attempt's task stood at when it was
/// admitted: all its ending's proposal is fenced on, and no more of the position, so a position
/// that gains a field never refuses the read of an attempt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Pin {
    pub phase: String,
    pub state: String,
    pub revision: u64,
}

impl Row {
    pub(crate) fn of(value: &Value) -> Result<Row, crate::Error> {
        crate::objects::read(value).map_err(|e| {
            crate::Error::Db(format!(
                "an execution attempt not in the board's shape: {e}"
            ))
        })
    }
}

/// One thing that happened to an attempt, appended and never changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Fact {
    /// Admitted, by whom, under which request key.
    Admitted {
        initiator: Principal,
        request_key: Option<String>,
    },
    /// An operation owed the provider, recorded before its I/O.
    Intent {
        sequence: u64,
        operation: Operation,
        key: String,
        digest: String,
        initiator: Principal,
    },
    /// A worker is about to send operation `sequence` under its lease's epoch, or to reconcile it
    /// by its key when `reconcile`.
    Sent {
        sequence: u64,
        worker: Principal,
        epoch: u64,
        reconcile: bool,
    },
    /// What the provider answered for operation `sequence` (or an observation, with none).
    /// `stale` is an answer that came after its worker's lease was gone: kept, and acted on by
    /// no one.
    Answer {
        sequence: Option<u64>,
        operation: Operation,
        outcome: Box<Outcome>,
        worker: Principal,
        epoch: u64,
        stale: bool,
    },
    /// A resource or conversation bound to the attempt: its session's links, kept after its
    /// handle leaves.
    Bound {
        resource: Option<Binding>,
        conversation: Option<String>,
    },
    /// Cancellation or a stop asked for, by whom.
    Asked {
        initiator: Principal,
        cancel: bool,
        mode: Option<StopMode>,
    },
    /// The attempt waits on a person, and why.
    Held { why: String },
    /// Its execution ended, how.
    Ended { ending: Ending },
    /// A resume or a retry followed it, as attempt `by`.
    Followed { by: String, relation: Relation },
    /// Collection begun, or ended as `collection`, by whom.
    Collection {
        collection: Collection,
        by: Principal,
        evidence: Option<String>,
    },
    /// An artifact reported, kept as current, or as stale when its producer was not the
    /// attempt's current generation or runtime.
    Artifact {
        name: String,
        digest: String,
        stale: bool,
    },
    /// A cleanup begun under `key`, or ended as `cleanup`.
    Cleanup {
        key: String,
        cleanup: Cleanup,
        by: Principal,
        evidence: Option<String>,
    },
}

/// An artifact as the project's storage reports it, collected for an attempt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// Its name within the attempt's manifest: a relative path that leads nowhere above it.
    pub name: String,
    /// Where the project's storage keeps it, and the digest of its content.
    pub reference: String,
    pub digest: String,
    pub size: u64,
    /// The resource generation and runtime epoch that produced it.
    pub generation: String,
    pub runtime_epoch: u64,
    /// Whether the project's storage verified its digest and size.
    pub verified: bool,
}

/// A resume's continuation: a checkpoint, a conversation, or both, and the mode the provider
/// resumes in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Continuation {
    pub checkpoint: Option<ContentRef>,
    pub conversation: Option<String>,
    pub mode: ResumeMode,
}
