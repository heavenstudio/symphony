//! One orchestrator per task: whose a task is, and which tasks still hold more than one.
//!
//! A task is its orchestrator's when that session holds the role on it, or on the nearest task
//! above it that has one, and is live, stale or of unknown liveness ([`crate::Health::holds`]). A
//! task with no such orchestrator on it or above it is unclaimed: every orchestrator hears it, so
//! a lost or left orchestrator's work reads unclaimed until another takes the role over. An
//! orchestrator hears its own tasks and the unclaimed ones only ([`Owners::reaches`]).

use crate::{Board, Task};
use std::collections::{BTreeMap, HashMap, HashSet};

/// The board's tasks by slug, read for whose each one is.
pub struct Owners<'t> {
    tasks: HashMap<&'t str, &'t Task>,
}

impl<'t> Owners<'t> {
    pub fn new(tasks: &'t [Task]) -> Owners<'t> {
        Owners {
            tasks: tasks.iter().map(|t| (t.slug.as_str(), t)).collect(),
        }
    }

    /// The holding orchestrators of the nearest task at or above `slug` that has any, nearest
    /// first; empty when the task is unclaimed or the board does not hold it. More than one only
    /// where a task still holds two ([`doubled`]).
    pub fn of(&self, slug: &str) -> Vec<&'t str> {
        let mut seen = HashSet::new();
        let mut at = self.tasks.get(slug).copied();
        while let Some(task) = at.filter(|t| seen.insert(t.slug.as_str())) {
            let held: Vec<&'t str> = task
                .roles
                .iter()
                .filter(|r| r.role == "orchestrator" && r.health.holds())
                .map(|r| r.handle.as_str())
                .collect();
            if !held.is_empty() {
                return held;
            }
            at = task
                .parent
                .as_deref()
                .and_then(|p| self.tasks.get(p).copied());
        }
        Vec::new()
    }

    /// Whether orchestrator `handle` hears `slug`: the task is its own, or nobody's.
    pub fn reaches(&self, slug: &str, handle: &str) -> bool {
        let owners = self.of(slug);
        owners.is_empty() || owners.contains(&handle)
    }

    /// Whether `slug` is some other orchestrator's, and not `handle`'s too.
    pub fn elsewhere(&self, slug: &str, handle: &str) -> bool {
        !self.reaches(slug, handle)
    }
}

/// Whether session `child` reports to `handle` when it goes stale or lost: the orchestrator of a
/// task it holds claimable work on, or, for work nobody orchestrates and a session holding none,
/// its parent. `work`: the tasks it holds claimable work on ([`claimable_work`]).
pub fn reports_to(owners: &Owners, work: &[&Task], parent: Option<&str>, handle: &str) -> bool {
    if work.is_empty() {
        return parent == Some(handle);
    }
    work.iter().any(|t| {
        let owners = owners.of(&t.slug);
        if owners.is_empty() {
            parent == Some(handle)
        } else {
            owners.contains(&handle)
        }
    })
}

/// The status of each unfinished task orchestrator `handle` hears, by slug: what schedules its
/// PULL digest, so another orchestrator's tasks moving schedule nothing for it.
pub fn open_statuses(board: &Board, handle: &str) -> BTreeMap<String, String> {
    let owners = Owners::new(&board.tasks);
    (board.tasks.iter())
        .filter(|t| !crate::health::FINISHED.contains(&t.status.as_str()))
        .filter(|t| owners.reaches(&t.slug, handle))
        .map(|t| (t.slug.clone(), t.status.clone()))
        .collect()
}

/// Each session's claimable work ([`crate::health::claimable`]), by handle.
pub fn claimable_work(board: &Board) -> HashMap<&str, Vec<&Task>> {
    let mut work: HashMap<&str, Vec<&Task>> = HashMap::new();
    for t in &board.tasks {
        for r in &t.roles {
            if crate::health::claimable(&r.role, &t.status) {
                work.entry(r.handle.as_str()).or_default().push(t);
            }
        }
    }
    work
}

/// The tasks holding two or more orchestrator roles, whatever their holders' health: what the
/// orchestrators settle by hand (`just tasks handover`, or `just tasks role` over a lost or left one).
pub fn doubled(tasks: &[Task]) -> impl Iterator<Item = &Task> {
    tasks
        .iter()
        .filter(|t| t.roles.iter().filter(|r| r.role == "orchestrator").count() > 1)
}

/// The line `just tasks handover` writes on the thread.
pub fn handed_line(from: &str, to: &str) -> String {
    format!("{from} handed orchestrator over to {to}; address {to} or @orchestrator")
}

/// The line taking the role over a lost or left holder writes on the thread; `from` are the
/// holders it dropped, each with its health.
pub fn took_line(to: &str, from: &[(String, &str)]) -> String {
    let from: Vec<String> = from
        .iter()
        .map(|(h, health)| format!("{h} ({health})"))
        .collect();
    format!("{to} took orchestrator over from {}", from.join(", "))
}

/// A move of the orchestrator role, read back from its thread line: the handles it left, and the
/// one it reached.
pub fn moved(text: &str) -> Option<(Vec<&str>, &str)> {
    if let Some((from, rest)) = text.split_once(" handed orchestrator over to ") {
        let to = rest.split_once(';').map_or(rest, |(to, _)| to);
        return Some((vec![from], to));
    }
    let (to, from) = text.split_once(" took orchestrator over from ")?;
    let from = from
        .split(", ")
        .map(|one| one.split_once(" (").map_or(one, |(h, _)| h))
        .collect();
    Some((from, to))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Health, Role, Session};

    fn task(slug: &str, parent: Option<&str>, roles: &[(&str, &str, Health)]) -> Task {
        Task {
            id: 0,
            slug: slug.into(),
            title: String::new(),
            body: String::new(),
            status: "in_progress".into(),
            priority: 2,
            parent: parent.map(str::to_string),
            created_by: String::new(),
            created: 0.0,
            updated: 0.0,
            links: Vec::new(),
            depends: Vec::new(),
            roles: roles
                .iter()
                .map(|(handle, role, health)| Role {
                    handle: handle.to_string(),
                    role: role.to_string(),
                    since: 0.0,
                    expires: None,
                    health: *health,
                })
                .collect(),
            instruction: false,
            fields: Default::default(),
        }
    }

    fn board(tasks: Vec<Task>) -> Board {
        Board {
            tasks,
            sessions: Vec::<Session>::new(),
            latest_message: 0,
        }
    }

    #[test]
    fn a_task_is_its_nearest_holding_orchestrators_and_unclaimed_without_one() {
        let tasks = vec![
            task("a", None, &[("amber", "orchestrator", Health::Live)]),
            task("a/b", Some("a"), &[]),
            task(
                "a/c",
                Some("a"),
                &[("cobalt", "orchestrator", Health::Stale)],
            ),
            task("a/c/d", Some("a/c"), &[("w", "executor", Health::Live)]),
            task("gone", None, &[("ghost", "orchestrator", Health::Left)]),
            task(
                "gone/x",
                Some("gone"),
                &[("lost", "orchestrator", Health::Lost)],
            ),
            task("free", None, &[]),
        ];
        let owners = Owners::new(&tasks);
        assert_eq!(owners.of("a/b"), ["amber"]);
        assert_eq!(owners.of("a/c/d"), ["cobalt"]);
        assert!(owners.of("gone/x").is_empty());
        assert!(owners.of("free").is_empty());
        assert!(owners.of("not-on-the-board").is_empty());
        assert!(owners.reaches("a/b", "amber") && !owners.reaches("a/b", "cobalt"));
        assert!(owners.elsewhere("a/c/d", "amber"));
        assert!(owners.reaches("free", "amber") && owners.reaches("free", "cobalt"));
    }

    #[test]
    fn a_loop_in_the_tree_ends_the_walk() {
        let tasks = vec![task("x", Some("y"), &[]), task("y", Some("x"), &[])];
        assert!(Owners::new(&tasks).of("x").is_empty());
    }

    #[test]
    fn a_double_belongs_to_both_until_it_is_settled() {
        let tasks = vec![task(
            "a",
            None,
            &[
                ("amber", "orchestrator", Health::Live),
                ("cobalt", "orchestrator", Health::Live),
                ("teal", "orchestrator", Health::Left),
            ],
        )];
        let owners = Owners::new(&tasks);
        assert_eq!(owners.of("a"), ["amber", "cobalt"]);
        let found: Vec<&str> = doubled(&tasks).map(|t| t.slug.as_str()).collect();
        assert_eq!(found, ["a"]);
        let single = vec![task("b", None, &[("amber", "orchestrator", Health::Left)])];
        assert_eq!(doubled(&single).count(), 0);
    }

    #[test]
    fn a_quiet_session_reports_to_the_orchestrator_of_its_work_else_its_parent() {
        let mut ready = task("free", None, &[("w2", "executor", Health::Lost)]);
        ready.status = "ready".into();
        let mut reviewed = task("done-one", None, &[("w3", "executor", Health::Lost)]);
        reviewed.status = "review".into();
        let b = board(vec![
            task(
                "a",
                None,
                &[
                    ("amber", "orchestrator", Health::Live),
                    ("w", "executor", Health::Lost),
                ],
            ),
            ready,
            reviewed,
        ]);
        let owners = Owners::new(&b.tasks);
        let work = claimable_work(&b);
        let of = |h: &str| work.get(h).cloned().unwrap_or_default();
        assert!(of("w3").is_empty());
        // Launched by cobalt, working amber's task: amber hears it.
        assert!(reports_to(&owners, &of("w"), Some("cobalt"), "amber"));
        assert!(!reports_to(&owners, &of("w"), Some("cobalt"), "cobalt"));
        // Work nobody orchestrates: the parent.
        assert!(reports_to(&owners, &of("w2"), Some("cobalt"), "cobalt"));
        assert!(!reports_to(&owners, &of("w2"), Some("cobalt"), "amber"));
        // Nothing claimable: the parent.
        assert!(reports_to(&owners, &of("w3"), Some("cobalt"), "cobalt"));
        assert!(!reports_to(&owners, &of("w3"), None, "cobalt"));
    }

    #[test]
    fn each_move_of_the_role_reads_back_from_its_line() {
        let handed = handed_line("amber", "cobalt");
        assert_eq!(
            handed,
            "amber handed orchestrator over to cobalt; address cobalt or @orchestrator"
        );
        assert_eq!(moved(&handed), Some((vec!["amber"], "cobalt")));
        let took = took_line(
            "cobalt",
            &[("amber".into(), "lost"), ("teal".into(), "left")],
        );
        assert_eq!(
            took,
            "cobalt took orchestrator over from amber (lost), teal (left)"
        );
        assert_eq!(moved(&took), Some((vec!["amber", "teal"], "cobalt")));
        assert_eq!(moved("amber joined as orchestrator"), None);
    }
}
