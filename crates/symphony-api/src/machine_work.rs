//! Additive machine-work wire records. Missing fields on older snapshots mean unknown evidence.
//! The board owns normalization; these serde-only records also build for browser clients.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Build,
    Test,
    Command,
    Setup,
    Reservation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum WorkState {
    Queued,
    Running,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
    /// The tasks `task` is under on the board, nearest first, as the Symphony read the board
    /// ([`MachineWork::place_under`]): a run for a subtask is its parents' work too. `task` stays
    /// the recorder's binding.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub under: Vec<String>,
}

impl WorkOperation {
    /// The operation is `slug`'s work: its own task's, or a task's under `slug`.
    pub fn is_for(&self, slug: &str) -> bool {
        self.task.as_deref() == Some(slug) || self.under.iter().any(|above| above == slug)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct WorkCounts {
    pub running_builds: usize,
    pub queued_builds: usize,
    pub running_tests: usize,
    pub queued_tests: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
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
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct MachineWork {
    pub freshness: WorkFreshness,
    pub machines: Vec<MachineObservation>,
    pub unassigned: Vec<WorkOperation>,
    /// Project-configured advisory targets by stable machine id. Never admission limits.
    #[serde(default)]
    pub guidance: std::collections::BTreeMap<String, WorkGuidance>,
}

impl MachineWork {
    /// Says, on every operation that names a task, which tasks that task is under, by `parent`
    /// (a task's parent on the board, None at the top or for a task the board does not hold).
    /// A parent chain that loops stops where it would repeat.
    pub fn place_under<'a>(&mut self, parent: impl Fn(&str) -> Option<&'a str>) {
        let operations = self
            .machines
            .iter_mut()
            .flat_map(|machine| machine.operations.iter_mut())
            .chain(self.unassigned.iter_mut());
        for operation in operations {
            operation.under.clear();
            let Some(task) = operation.task.as_deref() else {
                continue;
            };
            let mut at = task;
            while let Some(above) = parent(at) {
                if above == task || operation.under.iter().any(|seen| seen == above) {
                    break;
                }
                operation.under.push(above.to_owned());
                at = above;
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct WorkGuidance {
    pub build: Option<u32>,
    pub test: Option<u32>,
}
