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
/// [`WORKFLOW_ACTIONS_PATH`] answers, a [`NextActions`] preview and never a permit.
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
