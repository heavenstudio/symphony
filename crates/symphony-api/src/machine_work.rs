//! Additive machine-work wire records. Missing fields on older snapshots mean unknown evidence.
//! The board owns normalization; these serde-only records also build for browser clients.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkFreshness {
    #[default]
    Unknown,
    Fresh,
    Stale,
    Disconnected,
    Incomplete,
    Conflict,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Build,
    Test,
    Command,
    Setup,
    Reservation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkState {
    Queued,
    Running,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkOperation {
    pub machine_id: Option<String>,
    /// None when legacy evidence has no operation nonce.
    pub operation_id: Option<String>,
    pub parent_id: Option<String>,
    pub handle: Option<String>,
    pub task: Option<String>,
    pub checkout: String,
    pub recipe: String,
    pub kind: WorkKind,
    pub state: WorkState,
    pub resource: String,
    pub reason: String,
    /// Producer time for display only. Freshness uses receiver-assigned evidence.
    pub phase_since: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkCounts {
    pub running_builds: usize,
    pub queued_builds: usize,
    pub running_tests: usize,
    pub queued_tests: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MachineObservation {
    pub machine_id: String,
    pub label: String,
    pub producer_id: String,
    pub sequence: u64,
    pub age_ms: Option<u64>,
    pub freshness: WorkFreshness,
    pub complete: bool,
    pub reason: String,
    /// None is unknown occupancy, never free capacity. Counts exclude reservations/commands.
    pub counts: Option<WorkCounts>,
    pub operations: Vec<WorkOperation>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MachineWork {
    pub freshness: WorkFreshness,
    pub machines: Vec<MachineObservation>,
    pub unassigned: Vec<WorkOperation>,
}
