//! Additive delivery-workflow contract. These are data, not an admission service: a label,
//! serialized authority or next-action preview grants no permission. Execution attempts have
//! their own identity and lifecycle and never occupy a delivery state on that account alone.

use serde::{Deserialize, Serialize};

/// The definition format understood by this release, independent of board schema/API versions.
pub const FORMAT: u32 = 1;
/// The immutable built-in definition version. Omission keeps the existing board implementation.
pub const DEFAULT_VERSION: u32 = 1;

/// A closed list of `snake_case` words; `as` names it in the web client's wire types, where each
/// workflow type is named `Workflow…` beside the board's own (`Role`, `Task`).
macro_rules! vocabulary {
    ($name:ident as $ts:literal { $($variant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[cfg_attr(feature = "typescript", derive(ts_rs::TS), ts(rename = $ts))]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
    };
}

vocabulary!(Role as "WorkflowRole" {
    Executor,
    Reviewer,
    Orchestrator
});
vocabulary!(Slot as "WorkflowSlot" {
    Retain,
    Acquire,
    Release
});
vocabulary!(Dependency as "WorkflowDependency" {
    None,
    Done,
    Landed,
    AcceptedOutput
});
vocabulary!(UnsuccessfulDependency as "WorkflowUnsuccessfulDependency" {
    Block,
    Cancel,
    Manual
});
vocabulary!(Outcome as "WorkflowOutcome" {
    Success,
    Cancellation,
    Failure
});
vocabulary!(Completion as "WorkflowCompletion" {
    None,
    Landing,
    CoveredClose,
    ParentIntegration
});
vocabulary!(Operation as "WorkflowOperation" {
    Create,
    Claim,
    Release,
    Advance,
    RequestReview,
    Approve,
    Changes,
    Block,
    Resume,
    Retry,
    Cancel,
    CompleteSubtask,
    CloseCovered,
    PrepareLand,
    ConfirmLand,
    PrepareExternal,
    ConfirmExternal
});
vocabulary!(LegacyStatus as "WorkflowLegacyStatus" {
    Backlog,
    Concept,
    Design,
    Ready,
    InProgress,
    Review,
    Done,
    Blocked,
    Cancelled
});

/// Stable IDs are case-sensitive lowercase ASCII, beginning with a letter. Validation checks
/// every ID and reference; these public records also allow an editor to represent invalid input.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS), ts(rename = "WorkflowPair"))]
#[serde(deny_unknown_fields)]
pub struct Pair {
    pub phase: String,
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowDefinition")
)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub format: u32,
    pub initial: Pair,
    pub task_kinds: Vec<String>,
    pub phases: Vec<Phase>,
    pub states: Vec<State>,
    pub positions: Vec<Position>,
    pub edges: Vec<Edge>,
    pub capabilities: Vec<String>,
    pub adapters: Vec<String>,
    pub guards: Vec<Guard>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowPhase")
)]
#[serde(deny_unknown_fields)]
pub struct Phase {
    pub id: String,
    pub label: String,
    pub order: u32,
    pub phone_order: u32,
    /// False for the default's off-pipeline backlog/blocked/cancelled groups.
    pub column: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowState")
)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowPosition")
)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub at: Pair,
    /// None is explicit in JSON; TOML omits it. Phase/state names never imply staffing.
    pub staffing: Option<Role>,
    pub slot: Slot,
    pub dependencies: Dependency,
    pub unsuccessful_dependency: UnsuccessfulDependency,
    pub accepts_work: bool,
    pub terminal: bool,
    pub legacy: LegacyStatus,
    pub outcome: Option<Outcome>,
    pub satisfies_dependencies: bool,
    pub completion: Completion,
    pub recovery: Option<Recovery>,
}

/// All holder exits must be explicit, including intentional stays for manual recovery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowRecovery")
)]
#[serde(deny_unknown_fields)]
pub struct Recovery {
    pub release: RecoveryAction,
    pub lost_holder: RecoveryAction,
    pub attempt_failure: RecoveryAction,
    pub cancellation_acknowledged: RecoveryAction,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowRecoveryAction")
)]
#[serde(deny_unknown_fields)]
pub struct RecoveryAction {
    /// None means stay; manual must then be true. No implicit move to ready.
    pub target: Option<Pair>,
    pub manual: bool,
    pub slot: Slot,
    pub release_role: bool,
    pub release_reservation: bool,
}

/// A requirement for an external policy receipt. No expression, executable or cached pass.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowGuard")
)]
#[serde(deny_unknown_fields)]
pub struct Guard {
    pub id: String,
    pub adapter: String,
}

/// The host must bind these requirements to a trusted principal outside request JSON.
/// Task roles also require the relationship to this task; reviewers must be independent.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowAuthority")
)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Authority {
    TaskRole { role: Role },
    ProjectCapability { capability: String },
    SystemCapability { capability: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowEffect")
)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Effect {
    AcquireRole {
        role: Role,
    },
    ReleaseRole {
        role: Role,
    },
    Slot {
        policy: Slot,
    },
    Review {
        approved: bool,
    },
    Block {
        save_resume: bool,
    },
    ClearBlock,
    Outcome {
        outcome: Outcome,
        satisfies_dependencies: bool,
    },
    ParentIntegration,
    Prepare {
        adapter: String,
    },
    Finalize {
        adapter: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowRetryBound")
)]
#[serde(deny_unknown_fields)]
pub struct RetryBound {
    pub maximum: u32,
    pub exhausted: Pair,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS), ts(rename = "WorkflowEdge"))]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub id: String,
    pub label: String,
    pub from: Pair,
    pub to: Pair,
    pub operation: Operation,
    /// A legacy alias resolves only when unique at a source pair.
    pub alias: Option<String>,
    pub authority: Vec<Authority>,
    pub inputs: Vec<String>,
    pub dependencies: Dependency,
    pub guards: Vec<String>,
    pub effects: Vec<Effect>,
    pub automatic: bool,
    pub retry: Option<RetryBound>,
    pub human_wait: bool,
}

/// Metadata supplied by a reader of a committed project file, never inferred from a label.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowSource")
)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub commit: String,
    pub project_blob: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowRevision")
)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub format: u32,
    pub digest: String,
    pub source: Source,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowPositionSnapshot")
)]
#[serde(deny_unknown_fields)]
pub struct PositionSnapshot {
    pub workflow: Revision,
    pub activation_generation: u64,
    pub task_revision: u64,
    pub at: Pair,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS), ts(rename = "WorkflowRead"))]
#[serde(deny_unknown_fields)]
pub struct WorkflowRead {
    pub definition: Definition,
    pub revision: Revision,
    pub activation_generation: u64,
}

/// No caller-supplied principal. These fields fence intention, not authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowInputValue")
)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum InputValue {
    Text(String),
    Revision(String),
    Pair(Pair),
    Flag(bool),
}

/// Operation inputs remain separate from input-revision fencing and host-provided authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowOperationRequest")
)]
#[serde(deny_unknown_fields)]
pub struct OperationRequest {
    pub project: String,
    pub task: String,
    pub edge: String,
    pub expected: PositionSnapshot,
    pub input_revisions: std::collections::BTreeMap<String, String>,
    pub idempotency_key: String,
    pub inputs: std::collections::BTreeMap<String, InputValue>,
}

vocabulary!(RefusalCode as "WorkflowRefusalCode" {
    StaleTask,
    StaleWorkflow,
    StaleActivation,
    StaleInput,
    UnknownId,
    RemovedId,
    Unauthorized,
    Dependency,
    Capacity,
    HumanWait,
    GuardUnavailable,
    UnsupportedActivation,
    UnsupportedWriter,
    UnsupportedAdapter,
    ExplicitActionRequired
});

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowNextAction")
)]
#[serde(deny_unknown_fields)]
pub struct NextAction {
    pub edge: String,
    pub operation: Operation,
    pub destination: Pair,
    pub label: String,
    pub responsible: Vec<Authority>,
    pub required_inputs: Vec<String>,
    pub unavailable: Vec<RefusalCode>,
}

/// A principal-scoped preview, never an authorization receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowNextActions")
)]
#[serde(deny_unknown_fields)]
pub struct NextActions {
    pub snapshot: PositionSnapshot,
    pub actions: Vec<NextAction>,
}

/// The accepted output standing for a task another depends on, which a request taking an edge
/// that depends on `accepted_output` names under its slug (`input_revisions`): none when none
/// stands, which leaves that edge refused as `dependency`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowTakenOutput")
)]
#[serde(deny_unknown_fields)]
pub struct TakenOutput {
    pub task: String,
    pub output: Option<String>,
}

/// The local owner's next actions on one task, as a page reads them: what `GET`
/// [`WORKFLOW_ACTIONS_PATH`] answers, a [`NextActions`] preview and, where an edge leaving the
/// task's pair depends on `accepted_output`, the output standing for each task it depends on
/// (`takes`, empty otherwise), which the page sends with such an edge.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowActionsRead")
)]
#[serde(deny_unknown_fields)]
pub struct ActionsRead {
    pub snapshot: PositionSnapshot,
    pub actions: Vec<NextAction>,
    pub takes: Vec<TakenOutput>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowOperationResult")
)]
#[serde(deny_unknown_fields)]
pub struct OperationResult {
    pub transition: String,
    pub next: NextActions,
    pub effects: Vec<Effect>,
}

/// The active custom workflow, read by the Symphony's pages: what `GET` [`WORKFLOW_PATH`] answers.
pub const WORKFLOW_PATH: &str = "/api/workflow";

/// The local owner's next actions on one task a custom workflow holds (`?slug=SLUG`): what `GET`
/// [`WORKFLOW_ACTIONS_PATH`] answers, an [`ActionsRead`], a preview and never a permit.
pub const WORKFLOW_ACTIONS_PATH: &str = "/api/workflow-actions";

/// The owner's operation on a task a custom workflow holds, posted as `{"slug", "request"}` with an
/// [`OperationRequest`]: the principal is the local owner the server binds, never one it names.
/// The answer is `{"ok": true, "result": OperationResult}`, or a refusal whose text starts with
/// its [`RefusalCode`]'s word.
pub const WORKFLOW_APPLY_ROUTE: &str = "/api/workflow-apply";

/// What [`WORKFLOW_PATH`] answers: the board's active custom workflow, or none on a board whose
/// own writers keep its tasks (no `[workflow]` was ever activated); and the board's project, which
/// an [`OperationRequest`] names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct BoardWorkflow {
    pub project: String,
    pub workflow: Option<WorkflowRead>,
}

/// Where a task a custom workflow holds stands, as a page shows it: its phase and its
/// coordination state (IDs of the active definition, whose labels [`WORKFLOW_PATH`] reads), the
/// pair a blocked task resumes to, its workflow revision, which only its own transitions move and
/// which an [`OperationRequest`] names, and the person's recovery it waits on, if any.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowPlace")
)]
pub struct TaskPlace {
    pub phase: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<Pair>,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual: Option<ManualRecovery>,
}

/// A holder's exit that left its task waiting on a person's recovery by hand, until the task's
/// next transition: which exit (`release` or `lost_holder`), whose and of which role.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "typescript",
    derive(ts_rs::TS),
    ts(rename = "WorkflowManualRecovery")
)]
pub struct ManualRecovery {
    pub trigger: String,
    pub holder: String,
    pub role: String,
}

/// The attestation contract this release reads: an [`Attestation`] or [`Observation`] naming
/// another is refused, as is a committed authority that asks for another.
pub const ATTESTATION_CONTRACT: u32 = 1;

/// What a project's approval authority signs: the bytes of a JSON [`Attestation`] or
/// [`Observation`], exactly as signed, and its Ed25519 signature over them, in lowercase hex.
/// The board verifies the signature against the public key the project commits for that
/// authority, then reads the payload; a signature establishes who said it, never that it is
/// still current.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signed {
    pub payload: String,
    pub signature: String,
}

vocabulary!(Attested as "WorkflowAttested" {
    Pass,
    Refuse,
    Unknown
});

/// The principal an attestation covers, as the host authenticated it: its kind (`agent`,
/// `human`, `service`), its issuer and its subject.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attestee {
    pub kind: String,
    pub issuer: String,
    pub subject: String,
}

/// A project authority's answer for one guard of one operation: whose authority, at which of its
/// epochs and generations; the exact operation it covers (project, task, guard, edge, the
/// workflow revision and activation, the task's revision and pair, the input revisions and the
/// principal); its result with the ledger facts it rests on; and when it holds. It approves
/// nothing by itself: the board admits an operation only while every binding is still the
/// board's and the authority's generation is the one the board accepted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attestation {
    pub contract: u32,
    pub adapter: String,
    pub issuer: String,
    pub epoch: String,
    pub generation: u64,
    pub project: String,
    pub task: String,
    pub guard: String,
    pub edge: String,
    /// The digest of the workflow revision it covers.
    pub workflow: String,
    pub activation_generation: u64,
    pub task_revision: u64,
    pub at: Pair,
    /// Each input named once: a name given twice refuses the attestation, where a map read
    /// keeps the last, so what it binds never depends on which of the two a reader keeps.
    #[serde(deserialize_with = "named_once")]
    pub input_revisions: std::collections::BTreeMap<String, String>,
    pub principal: Attestee,
    pub result: Attested,
    /// The authority's references to the decisions it rests on: required for a pass.
    pub facts: Vec<String>,
    pub reasons: Vec<String>,
    /// Seconds since the Unix epoch.
    pub issued_at: f64,
    pub expires_at: f64,
}

/// An authority's state as the board accepted it: its epoch, its generation in that epoch and the
/// digest of its decisions at that generation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityState {
    pub epoch: String,
    pub generation: u64,
    pub state: String,
}

/// A project authority's report that its decisions moved (an approval, a rejection, a
/// withdrawal, a lost approver): its new epoch, generation and decision digest, and the state the
/// board accepted before it, none for an authority the board has accepted nothing from. The board
/// accepts it only from that state, so no generation is lowered and no gap is passed over.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub contract: u32,
    pub adapter: String,
    pub issuer: String,
    pub epoch: String,
    pub generation: u64,
    pub state: String,
    pub previous: Option<AuthorityState>,
    pub at: f64,
}

/// A map of text to text, refused when it names a key twice.
fn named_once<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<std::collections::BTreeMap<String, String>, D::Error> {
    struct Once;
    impl<'de> serde::de::Visitor<'de> for Once {
        type Value = std::collections::BTreeMap<String, String>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a map naming each input once")
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut named = std::collections::BTreeMap::new();
            while let Some((name, revision)) = map.next_entry::<String, String>()? {
                match named.entry(name) {
                    std::collections::btree_map::Entry::Vacant(at) => {
                        at.insert(revision);
                    }
                    std::collections::btree_map::Entry::Occupied(at) => {
                        let twice = format!("input {} is named twice", at.key());
                        return Err(serde::de::Error::custom(twice));
                    }
                }
            }
            Ok(named)
        }
    }
    deserializer.deserialize_map(Once)
}
