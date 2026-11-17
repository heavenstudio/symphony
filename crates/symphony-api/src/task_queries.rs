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

/// The order slugs are listed in: as text, but a run of digits by its number, so `ready-3` comes
/// before `ready-30`, and `ready-9` before `ready-10`. Slugs whose numbers are equal but spelled
/// apart (`a1`, `a01`) keep their order as text, so only equal slugs are equal and the order is
/// total: the board sorts a column by it and pages the column after its last slug by it.
pub fn slug_order(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    fn digits(s: &[u8]) -> usize {
        s.iter().take_while(|c| c.is_ascii_digit()).count()
    }
    /// A run of digits without its leading zeros, which compares as its number by length first.
    fn number(s: &[u8]) -> &[u8] {
        &s[s.iter().take_while(|c| **c == b'0').count()..]
    }
    let (mut left, mut right) = (a.as_bytes(), b.as_bytes());
    loop {
        match (left.first(), right.first()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(one), Some(other)) if one.is_ascii_digit() && other.is_ascii_digit() => {
                let (ends, other_ends) = (digits(left), digits(right));
                let (value, other_value) = (number(&left[..ends]), number(&right[..other_ends]));
                let order = value
                    .len()
                    .cmp(&other_value.len())
                    .then(value.cmp(other_value));
                if order != Ordering::Equal {
                    return order;
                }
                (left, right) = (&left[ends..], &right[other_ends..]);
            }
            (Some(one), Some(other)) if one != other => return one.cmp(other),
            _ => (left, right) = (&left[1..], &right[1..]),
        }
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
            fields: std::collections::BTreeMap::default(),
            waits_on: None,
            review_rounds: None,
            from: None,
        }
    }

    #[test]
    fn slugs_order_as_text_with_each_run_of_digits_by_its_number() {
        let mut slugs = vec![
            "ready-31",
            "ready-3",
            "ready-30",
            "ready-10",
            "ready-9",
            "ready-",
            "ready-03",
            "ready-3a",
            "ready-3-1",
            "ready-300",
            "ready",
            "ready-a",
            "ready-#",
            "ready-é",
            "ready-0",
            "ready-00",
            "a2b10",
            "a2b9",
            "a10b1",
            "",
        ];
        slugs.sort_by(|a, b| slug_order(a, b));
        assert_eq!(
            slugs,
            [
                "",
                "a2b9",
                "a2b10",
                "a10b1",
                "ready",
                "ready-",
                "ready-#",
                "ready-0",
                "ready-00",
                "ready-03",
                "ready-3",
                "ready-3-1",
                "ready-3a",
                "ready-9",
                "ready-10",
                "ready-30",
                "ready-31",
                "ready-300",
                "ready-a",
                "ready-é",
            ]
        );
    }

    /// The order is total and agrees with equality, as a sort and a page's cursor need: each pair
    /// compares one way and its reverse the other, only a slug equals itself, and it is transitive.
    #[test]
    fn the_slug_order_is_total() {
        use std::cmp::Ordering;
        let slugs = [
            "", "0", "00", "1", "01", "10", "9", "a", "a0", "a00", "a1", "a01", "a1b", "a01b",
            "a1-", "a-1", "a-01", "a/2", "a/10", "a/b", "A1", "a 1", "é1", "é01", "b", "b0b",
        ];
        for a in slugs {
            for b in slugs {
                assert_eq!(slug_order(a, b), slug_order(b, a).reverse(), "{a} {b}");
                assert_eq!(slug_order(a, b) == Ordering::Equal, a == b, "{a} {b}");
                for c in slugs {
                    if slug_order(a, b) == Ordering::Less && slug_order(b, c) == Ordering::Less {
                        assert_eq!(slug_order(a, c), Ordering::Less, "{a} {b} {c}");
                    }
                }
            }
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
