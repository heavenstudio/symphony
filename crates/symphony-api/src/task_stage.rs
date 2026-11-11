//! Read-only board placement. Operational stages never change lifecycle or admission.

use crate::{MachineWork, Task, TaskStatus, WorkFreshness, WorkKind, WorkOperation, WorkState};
use serde::{Deserialize, Serialize};

/// Compact task board: briefs are fetched only when the owner opens a task.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
            if !matches!(
                machine.freshness,
                WorkFreshness::Fresh | WorkFreshness::Incomplete
            ) {
                continue;
            }
            for operation in &machine.operations {
                if operation.machine_id.as_deref() != Some(&machine.machine_id)
                    || !operation.is_for(&task.slug)
                    || operation.operation_id.as_deref().is_none_or(str::is_empty)
                    || !matches!(operation.kind, WorkKind::Build | WorkKind::Test)
                    || self.conflicts(operation)
                {
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
