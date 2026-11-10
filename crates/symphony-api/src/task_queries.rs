//! Domain queries shared by project summaries, board endpoints and client views.
//! Wire/storage representation is interpreted here, rather than in each caller.

use crate::Task;

/// A deliverable feature or one of its implementation slices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    Task,
    Subtask,
}

/// Named lifecycle states; their wire spelling is a compatibility detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    Concept,
    Design,
    Backlog,
    Ready,
    InProgress,
    Review,
    Done,
    Blocked,
    Cancelled,
}

impl TaskStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Concept => "concept",
            Self::Design => "design",
            Self::Backlog => "backlog",
            Self::Ready => "ready",
            Self::InProgress => "in_progress",
            Self::Review => "review",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
        }
    }
}

impl Task {
    /// Interpret the existing wire relationship as the domain kind.
    pub fn kind(&self) -> TaskKind {
        match self.parent {
            None => TaskKind::Task,
            Some(_) => TaskKind::Subtask,
        }
    }

    pub fn is_task(&self) -> bool {
        self.kind() == TaskKind::Task
    }

    pub fn is_subtask(&self) -> bool {
        self.kind() == TaskKind::Subtask
    }

    pub fn has_status(&self, status: TaskStatus) -> bool {
        self.status == status.as_str()
    }

    pub fn is_finished(&self) -> bool {
        self.has_status(TaskStatus::Done) || self.has_status(TaskStatus::Cancelled)
    }
}

/// Deliverable tasks, excluding all subtask depths.
pub fn tasks(records: &[Task]) -> impl Iterator<Item = &Task> {
    records.iter().filter(|record| record.is_task())
}

/// Implementation slices, including nested slices.
pub fn subtasks(records: &[Task]) -> impl Iterator<Item = &Task> {
    records.iter().filter(|record| record.is_subtask())
}

pub fn tasks_in_status(records: &[Task], status: TaskStatus) -> impl Iterator<Item = &Task> {
    tasks(records).filter(move |record| record.has_status(status))
}

pub fn tasks_in_progress(records: &[Task]) -> impl Iterator<Item = &Task> {
    tasks_in_status(records, TaskStatus::InProgress)
}

pub fn tasks_in_review(records: &[Task]) -> impl Iterator<Item = &Task> {
    tasks_in_status(records, TaskStatus::Review)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(slug: &str, status: &str, parent: Option<&str>) -> Task {
        Task {
            slug: slug.into(),
            title: slug.into(),
            body: String::new(),
            status: status.into(),
            priority: 2,
            parent: parent.map(String::from),
            created_by: "owner".into(),
            created: 0.0,
            updated: 0.0,
            links: Vec::new(),
            depends: Vec::new(),
            roles: Vec::new(),
            instruction: false,
            fields: Default::default(),
        }
    }

    #[test]
    fn domain_queries_distinguish_tasks_slices_depth_and_unknown_states() {
        let records = vec![
            record("feature", "in_progress", None),
            record("reviewed", "review", None),
            record("future", "future_state", None),
            record("feature/part", "in_progress", Some("feature")),
            record("feature/part/check", "review", Some("feature/part")),
        ];
        assert_eq!(
            tasks(&records).map(|t| t.slug.as_str()).collect::<Vec<_>>(),
            ["feature", "reviewed", "future"]
        );
        assert_eq!(subtasks(&records).count(), 2);
        assert_eq!(
            tasks_in_progress(&records)
                .map(|t| t.slug.as_str())
                .collect::<Vec<_>>(),
            ["feature"]
        );
        assert_eq!(
            tasks_in_review(&records)
                .map(|t| t.slug.as_str())
                .collect::<Vec<_>>(),
            ["reviewed"]
        );
        assert_eq!(tasks_in_status(&records, TaskStatus::Done).count(), 0);
        assert!(!records[2].is_finished());
        assert_eq!(tasks(&[]).count(), 0);
        assert_eq!(tasks_in_progress(&[]).count(), 0);
        for state in [TaskStatus::Done, TaskStatus::Cancelled] {
            let task = record("finished", state.as_str(), None);
            assert!(task.has_status(state));
            assert!(task.is_finished());
        }
    }
}
