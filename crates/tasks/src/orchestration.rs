//! One orchestrator per task: whose a task is, and which tasks still hold more than one.
//!
//! A task is its orchestrator's when that session holds the role on it, or on the nearest task
//! above it that has one, and is live, stale or of unknown liveness ([`crate::Health::holds`]). A
//! task with no such orchestrator on it or above it is unclaimed: every orchestrator hears it, so
//! a lost or left orchestrator's work reads unclaimed until another takes the role over. An
//! orchestrator hears its own tasks and the unclaimed ones only ([`Owners::reaches`]).

use crate::rows::SessionRow;
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
    work.iter()
        .any(|task| hears(owners, &task.slug, parent, handle))
}

/// Whether `handle` hears of a quiet or waiting session's work on `slug`: that task's orchestrator,
/// or, for a task nobody orchestrates, the session's `parent`.
pub fn hears(owners: &Owners, slug: &str, parent: Option<&str>, handle: &str) -> bool {
    let owners = owners.of(slug);
    if owners.is_empty() {
        parent == Some(handle)
    } else {
        owners.contains(&handle)
    }
}

/// Whether `handle` is told when `session` goes quiet or waits in its pane: its parent is; an
/// orchestrator ([`claimable_work`] given as `work`) only by [`reports_to`].
pub fn told_of(
    owners: &Owners,
    work: Option<&HashMap<&str, Vec<&Task>>>,
    session: &SessionRow,
    handle: &str,
) -> bool {
    let parent = session.parent.as_deref();
    work.map_or(parent == Some(handle), |work| {
        let held = work
            .get(session.handle.as_str())
            .map_or(&[][..], Vec::as_slice);
        reports_to(owners, held, parent, handle)
    })
}

/// Of `held`, quiet session `child`'s claimable work, what `handle` is told of: an orchestrator
/// hears its own tasks', and those nobody orchestrates when it is `child`'s parent; any other
/// session, all of it, as a parent always has.
pub fn told(board: &Board, child: &str, handle: &str, held: Vec<String>) -> Vec<String> {
    let row = |h: &str| board.sessions.iter().find(|s| s.row.handle == h);
    if row(handle).is_none_or(|s| s.row.kind != "orchestrator") {
        return held;
    }
    let parent = row(child).and_then(|s| s.row.parent.as_deref());
    let owners = Owners::new(&board.tasks);
    held.into_iter()
        .filter(|slug| hears(&owners, slug, parent, handle))
        .collect()
}

/// The status of each unfinished task on the board, by slug: the map an orchestrator's callback
/// state keeps, in the meaning dispatchers built before one orchestrator per task read it.
pub fn open_statuses(board: &Board) -> BTreeMap<String, String> {
    (board.tasks.iter())
        .filter(|t| !crate::health::FINISHED.contains(&t.status.as_str()))
        .map(|t| (t.slug.clone(), t.status.clone()))
        .collect()
}

/// Of `statuses`, those of the tasks orchestrator `handle` hears: what schedules its PULL digest,
/// so another orchestrator's tasks moving schedule nothing for it.
pub fn heard_statuses<'statuses>(
    owners: &Owners,
    statuses: &'statuses BTreeMap<String, String>,
    handle: &str,
) -> BTreeMap<&'statuses str, &'statuses str> {
    (statuses.iter())
        .filter(|(slug, _)| owners.reaches(slug, handle))
        .map(|(slug, status)| (slug.as_str(), status.as_str()))
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
        .map(|(handle, health)| format!("{handle} ({health})"))
        .collect();
    format!("{to} took orchestrator over from {}", from.join(", "))
}

/// Whether a move of the orchestrator role from `from` to `to` is said to `handle`: each session it
/// left, and the one it reached.
pub fn moved_for(from: &[&str], to: &str, handle: &str) -> bool {
    to == handle || from.contains(&handle)
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

    /// A role `handle` holds as `role`, its holder of `health`.
    fn held(handle: &str, role: &str, health: Health) -> Role {
        Role {
            handle: handle.to_string(),
            role: role.to_string(),
            since: 0.0,
            expires: None,
            health,
        }
    }

    fn task(slug: &str, parent: Option<&str>, roles: Vec<Role>) -> Task {
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
            roles,
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
            task("a", None, vec![held("amber", "orchestrator", Health::Live)]),
            task("a/b", Some("a"), Vec::new()),
            task(
                "a/c",
                Some("a"),
                vec![held("cobalt", "orchestrator", Health::Stale)],
            ),
            task(
                "a/c/d",
                Some("a/c"),
                vec![held("w", "executor", Health::Live)],
            ),
            task(
                "gone",
                None,
                vec![held("ghost", "orchestrator", Health::Left)],
            ),
            task(
                "gone/x",
                Some("gone"),
                vec![held("lost", "orchestrator", Health::Lost)],
            ),
            task("free", None, Vec::new()),
        ];
        let owners = Owners::new(&tasks);
        let nobody: [&str; 0] = [];
        assert_eq!(owners.of("a/b"), ["amber"]);
        assert_eq!(owners.of("a/c/d"), ["cobalt"]);
        assert_eq!(owners.of("gone/x"), nobody);
        assert_eq!(owners.of("free"), nobody);
        assert_eq!(owners.of("not-on-the-board"), nobody);
        let reach = |slug: &str| ["amber", "cobalt"].map(|handle| owners.reaches(slug, handle));
        assert_eq!(reach("a/b"), [true, false]);
        assert_eq!(reach("free"), [true, true]);
        let elsewhere = ["amber", "cobalt"].map(|handle| owners.elsewhere("a/c/d", handle));
        assert_eq!(elsewhere, [true, false]);
    }

    #[test]
    fn a_loop_in_the_tree_ends_the_walk() {
        let tasks = vec![
            task("x", Some("y"), Vec::new()),
            task("y", Some("x"), Vec::new()),
        ];
        assert_eq!(Owners::new(&tasks).of("x"), [""; 0]);
    }

    #[test]
    fn a_double_belongs_to_both_until_it_is_settled() {
        let roles = vec![
            held("amber", "orchestrator", Health::Live),
            held("cobalt", "orchestrator", Health::Live),
            held("teal", "orchestrator", Health::Left),
        ];
        let tasks = vec![task("a", None, roles)];
        let owners = Owners::new(&tasks);
        assert_eq!(owners.of("a"), ["amber", "cobalt"]);
        let found: Vec<&str> = doubled(&tasks).map(|t| t.slug.as_str()).collect();
        assert_eq!(found, ["a"]);
        let single = vec![task(
            "b",
            None,
            vec![held("amber", "orchestrator", Health::Left)],
        )];
        assert_eq!(doubled(&single).count(), 0);
    }

    #[test]
    fn a_quiet_session_reports_to_the_orchestrator_of_its_work_else_its_parent() {
        let mut ready = task("free", None, vec![held("w2", "executor", Health::Lost)]);
        ready.status = "ready".into();
        let mut reviewed = task("done-one", None, vec![held("w3", "executor", Health::Lost)]);
        reviewed.status = "review".into();
        let amber = vec![
            held("amber", "orchestrator", Health::Live),
            held("w", "executor", Health::Lost),
        ];
        let b = board(vec![task("a", None, amber), ready, reviewed]);
        let owners = Owners::new(&b.tasks);
        let work = claimable_work(&b);
        let of = |h: &str| work.get(h).cloned().unwrap_or_default();
        assert_eq!(of("w3").len(), 0, "work in review is nobody's to take");
        let told = |child: &str, parent: Option<&str>| {
            ["amber", "cobalt"].map(|handle| reports_to(&owners, &of(child), parent, handle))
        };
        // Launched by cobalt, working amber's task: amber hears it.
        assert_eq!(told("w", Some("cobalt")), [true, false]);
        // Work nobody orchestrates, or nothing claimable: the parent.
        assert_eq!(told("w2", Some("cobalt")), [false, true]);
        assert_eq!(told("w3", Some("cobalt")), [false, true]);
        assert_eq!(told("w3", None), [false, false]);
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
