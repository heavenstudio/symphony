//! Read-only board placement. Operational stages never change lifecycle or admission.

use crate::{
    MachineObservation, MachineWork, Task, TaskStatus, WorkFreshness, WorkKind, WorkOperation,
    WorkState,
};
use serde::{Deserialize, Serialize};

/// Compact task board: briefs are fetched only when the owner opens a task.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TaskListing {
    #[serde(default)]
    pub machine_work: MachineWork,
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub statuses: Vec<String>,
    #[serde(default)]
    pub signoff: Vec<String>,
    #[serde(default)]
    pub counts: Vec<(String, usize)>,
    /// Complete filtered/windowed lifecycle totals, used when the client's feed is lost.
    #[serde(default)]
    pub lifecycle_counts: Vec<(String, usize)>,
    pub next: Option<String>,
    /// Each session holding the orchestrator role on an open task, by handle, whatever the
    /// listing selects: the board says once when none is live.
    #[serde(default)]
    pub orchestrators: Vec<crate::Orchestrator>,
    /// The board's ready tasks no live executor holds with nothing open before them, whatever the
    /// listing selects ([`crate::Tasks::unstarted`]).
    #[serde(default)]
    pub unstarted: usize,
    /// Complete totals for the opt-in waiting-columns projection; absent on ordinary listings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<WaitingBoard>,
}

/// The waiting board's facets and columns, counted before pagination.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct WaitingBoard {
    /// Open tasks by [`crate::WAITS_KINDS`], in that order, before the selected wait kind.
    pub kinds: Vec<(String, usize)>,
    /// Every column after the selected wait kind, including columns outside the requested page.
    pub columns: Vec<WaitingColumn>,
    /// Open tasks a custom workflow holds by their coordination state's ID, ordered by ID, before
    /// the selected wait kind and state: empty, and left out, on a board with none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<(String, usize)>,
    /// The active custom workflow's activation generation: absent on a board whose own writers keep
    /// its tasks, so a page draws a workflow's phases and states only where one is active, and
    /// reads its definition again when it changes (docs/workflow/WORKFLOW.md, "The app").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<u64>,
}

/// One waiting column's ordinary and blocked tasks and earliest status entry.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct WaitingColumn {
    pub status: String,
    pub ordinary: usize,
    pub blocked: usize,
    /// Seconds since the epoch, not a changing age; none for an empty column.
    pub oldest: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskStage<'a> {
    Lifecycle(&'a str),
    Build,
    Test,
}

impl<'a> TaskStage<'a> {
    pub const PIPELINE: [TaskStage<'static>; 8] = [
        TaskStage::Lifecycle("concept"),
        TaskStage::Lifecycle("design"),
        TaskStage::Lifecycle("ready"),
        TaskStage::Lifecycle("in_progress"),
        TaskStage::Build,
        TaskStage::Test,
        TaskStage::Lifecycle("review"),
        TaskStage::Lifecycle("done"),
    ];

    pub const fn as_str(self) -> &'a str {
        match self {
            Self::Lifecycle(status) => status,
            Self::Build => "build",
            Self::Test => "test",
        }
    }
}

impl MachineWork {
    /// Running work wins over queued work, then Test over Build. Producer order, clocks and
    /// pagination cannot change placement. Current positive evidence in a partial inventory
    /// can place a task, without claiming complete machine occupancy. Expired, conflicting or
    /// disconnected observations remain phase chips in the task's underlying lifecycle stage.
    /// A task's work is what names it or a task under it ([`WorkOperation::is_for`]).
    pub fn task_stage<'a>(&self, task: &'a Task, connected: bool) -> TaskStage<'a> {
        if !connected || !task.has_status(TaskStatus::InProgress) {
            return TaskStage::Lifecycle(&task.status);
        }
        let mut stage = TaskStage::Lifecycle(&task.status);
        let mut priority = 0;
        for machine in &self.machines {
            for operation in &machine.operations {
                if !operation.is_for(&task.slug) || !self.can_place(machine, operation) {
                    continue;
                }
                // Queued Build < queued Test < running Build < running Test.
                let candidate = 1
                    + 2 * u8::from(operation.state == WorkState::Running)
                    + u8::from(operation.kind == WorkKind::Test);
                if candidate > priority {
                    priority = candidate;
                    stage = if operation.kind == WorkKind::Test {
                        TaskStage::Test
                    } else {
                        TaskStage::Build
                    };
                }
            }
        }
        stage
    }

    /// Whether `other` puts every task in the stage this work puts it in ([`Self::task_stage`]):
    /// the same operations place tasks, in the same order, each for the same task and the tasks
    /// it is under, of the same kind, running or queued alike. A machine's heartbeat, its age,
    /// its sequence and its other operations move nothing on a board, whose listings are read
    /// without the work (`work=0`), so the board reads its counts and columns again only when
    /// this is false. Two works placing alike in another order count as different.
    pub fn places_alike(&self, other: &MachineWork) -> bool {
        fn placing(work: &MachineWork) -> Vec<(&Option<String>, &Vec<String>, bool, bool)> {
            let mut placing = Vec::new();
            for machine in &work.machines {
                for operation in &machine.operations {
                    if work.can_place(machine, operation) {
                        placing.push((
                            &operation.task,
                            &operation.under,
                            operation.kind == WorkKind::Test,
                            operation.state == WorkState::Running,
                        ));
                    }
                }
            }
            placing
        }
        placing(self) == placing(other)
    }

    /// Whether `operation`, as `machine` reports it, can place a task in Build or Test: current
    /// evidence on a fresh or partial machine, recorded by that machine, with an operation id, of
    /// a build or a test, which no other report of the same operation contradicts (looked for
    /// last, among every machine's).
    fn can_place(&self, machine: &MachineObservation, operation: &WorkOperation) -> bool {
        matches!(
            machine.freshness,
            WorkFreshness::Fresh | WorkFreshness::Incomplete
        ) && operation.machine_id.as_deref() == Some(&machine.machine_id)
            && !operation.operation_id.as_deref().is_none_or(str::is_empty)
            && matches!(operation.kind, WorkKind::Build | WorkKind::Test)
            && !self.conflicts(operation)
    }

    fn conflicts(&self, operation: &WorkOperation) -> bool {
        for machine in &self.machines {
            for other in &machine.operations {
                if other.machine_id == operation.machine_id
                    && other.operation_id == operation.operation_id
                    && other != operation
                {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests;
