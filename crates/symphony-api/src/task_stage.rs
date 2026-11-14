//! Read-only board placement. Operational stages never change lifecycle or admission.

use crate::{
    MachineObservation, MachineWork, Task, TaskStatus, WorkFreshness, WorkKind, WorkOperation,
    WorkState,
};
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
