//! The PULL digest: work on the board that no event announces.
//!
//! The orchestrator's monitor (`just tasks-attach --kind orchestrator`) hears messages, verdicts and
//! status changes. It does not hear that a ready task has nobody on it, that an in-progress task's
//! executor is gone, that a review has no reviewer, or that a branch landed under a task still open.
//! This looks at the board for exactly those, and prints one header line and the first six of each:
//!
//! ```text
//! PULL 14:05 executing=3 ready_unstaffed=2 in_progress_orphaned=1 review_unreviewed=0 landed_open=1 idle=2
//!   ready_unstaffed p1 sim/jump-departure — Jump departure
//!   in_progress_orphaned p2 view/hud — HUD scale
//!   landed_open p2 sim/x — X   (landed on main, task still review)
//!   idle calm-vole
//! ```
//!
//! A task is **staffed** when it, or any unfinished task under it, has an executor whose session is live
//! or stale (so an agent working a subtask `X/do` staffs `X`). A ready task also needs its dependencies
//! done to be offered at all; cancelled prerequisites are listed as `dependency_blocked` with a
//! reason instead. The monitor delivers it only for newly actionable entries.

use crate::health::EXECUTING;
use crate::orchestration::{Owners, claimable_work};
use crate::sys::local_minute;
use crate::{Board, Error, Session, Store, Task};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// How many of a category are listed.
pub const SHOWN: usize = 6;
/// The categories, in the order they are listed; `idle` holds handles, the others tasks.
pub const CATEGORIES: [&str; 6] = [
    "ready_unstaffed",
    "dependency_blocked",
    "in_progress_orphaned",
    "review_unreviewed",
    "landed_open",
    "idle",
];

/// One look at the board.
#[derive(Debug, Clone)]
pub struct Digest {
    pub now: f64,
    /// In-progress tasks with an executor present.
    pub executing: usize,
    /// The slugs `executing` counts, for [`Digest::only_for`].
    running: Vec<String>,
    tasks: BTreeMap<&'static str, Vec<Task>>,
    /// Handles the agents API reports idle.
    pub idle: Vec<String>,
    /// Slug -> a short remark: who reserved it, that it landed.
    pub notes: BTreeMap<String, String>,
    /// Work retained because its shared native presence signal expired.
    pub liveness_unknown: Vec<String>,
}

impl Digest {
    /// Actionable identities, independent of clocks, counts or titles.
    pub fn entries(&self) -> BTreeSet<(String, String)> {
        CATEGORIES
            .iter()
            .flat_map(|name| {
                let entries: Vec<String> = if *name == "idle" {
                    self.idle.clone()
                } else {
                    self.items(name).iter().map(|t| t.slug.clone()).collect()
                };
                entries
                    .into_iter()
                    .map(move |entry| (name.to_string(), entry))
            })
            .collect()
    }
    /// Stable event content, independent of when the board was observed.
    pub fn event_lines(&self) -> Vec<String> {
        let mut lines = self.lines();
        lines[0] = format!("PULL {}", lines[0].splitn(3, ' ').nth(2).unwrap_or(""));
        lines
    }
    /// How many there are in `name`, `executing` or a category.
    pub fn count(&self, name: &str) -> usize {
        match name {
            "executing" => self.executing,
            "idle" => self.idle.len(),
            _ => self.items(name).len(),
        }
    }

    /// The tasks in category `name` (none for `idle`, which holds handles).
    pub fn items(&self, name: &str) -> &[Task] {
        self.tasks.get(name).map_or(&[], Vec::as_slice)
    }

    /// The digest as orchestrator `handle` hears it: the rows of its own tasks and of unclaimed
    /// ones ([`Owners::reaches`]), and the idle sessions but the children of another orchestrator
    /// still here. Staffing was judged on the whole board, so work on a subtask still staffs its
    /// parent.
    pub fn only_for(mut self, board: &Board, handle: &str) -> Digest {
        let owners = Owners::new(&board.tasks);
        for listed in self.tasks.values_mut() {
            listed.retain(|task| owners.reaches(&task.slug, handle));
        }
        self.tasks.retain(|_, listed| !listed.is_empty());
        self.running.retain(|slug| owners.reaches(slug, handle));
        self.executing = self.running.len();
        self.notes.retain(|slug, _| owners.reaches(slug, handle));
        let sessions: HashMap<&str, &Session> = (board.sessions.iter())
            .map(|s| (s.row.handle.as_str(), s))
            .collect();
        let elsewhere = |parent: &str| {
            let orchestrator = |p: &&Session| p.row.kind == "orchestrator" && p.health.holds();
            parent != handle && sessions.get(parent).is_some_and(orchestrator)
        };
        self.idle.retain(|idle| {
            let parent = sessions
                .get(idle.as_str())
                .and_then(|s| s.row.parent.as_deref());
            !parent.is_some_and(elsewhere)
        });
        let work = claimable_work(board);
        let heard = |held: &Vec<&Task>| held.iter().any(|t| owners.reaches(&t.slug, handle));
        self.liveness_unknown
            .retain(|quiet| work.get(quiet.as_str()).is_some_and(heard));
        self
    }

    pub fn lines(&self) -> Vec<String> {
        let counts: Vec<String> = ["executing"]
            .iter()
            .chain(CATEGORIES.iter())
            // Keep the usual digest compact; cancellation blockers get a count when present.
            .filter(|name| **name != "dependency_blocked" || self.count(name) > 0)
            .map(|name| format!("{name}={}", self.count(name)))
            .collect();
        let mut out = vec![format!(
            "PULL {} {}",
            &local_minute(self.now)[11..],
            counts.join(" ")
        )];
        if !self.liveness_unknown.is_empty() {
            out.push(format!("  liveness unknown: native presence sweep overdue or failed; work remains held: {}", self.liveness_unknown.join(", ")));
        }
        for name in CATEGORIES {
            let listed: Vec<String> = if name == "idle" {
                self.idle.iter().map(|h| format!("  idle {h}")).collect()
            } else {
                self.items(name)
                    .iter()
                    .map(|t| {
                        let note = self
                            .notes
                            .get(&t.slug)
                            .map(|n| format!("   ({n})"))
                            .unwrap_or_default();
                        format!("  {name} p{} {} — {}{note}", t.priority, t.slug, t.title)
                    })
                    .collect()
            };
            out.extend(listed.iter().take(SHOWN).cloned());
            if listed.len() > SHOWN {
                out.push(format!("  {name} … and {} more", listed.len() - SHOWN));
            }
        }
        out
    }
}

/// Look at the board. `idle`: handles the agents API reports idle; `landed(task)`: whether the task's
/// branch is already on main (a git question the caller answers; None skips it, and a `landed <slug>
/// on main` line on the task's own thread says it too); `verifying`: handles the agents API reports
/// running a test, who are present however quiet the board says they are.
pub fn digest(
    store: &Store,
    idle: &[String],
    landed: Option<&dyn Fn(&Task) -> bool>,
    now: f64,
    verifying: &[String],
) -> Result<Digest, Error> {
    let busy: HashSet<&str> = verifying.iter().map(String::as_str).collect();
    let posted = store.landings_posted()?;
    let tasks = store.tasks(None)?;
    let liveness_unknown = tasks
        .iter()
        .flat_map(|t| {
            t.roles
                .iter()
                .filter(move |role| crate::health::claimable(&role.role, &t.status))
        })
        .filter(|r| r.health == crate::Health::Unknown)
        .map(|r| r.handle.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let by_slug: HashMap<&str, &Task> = tasks.iter().map(|t| (t.slug.as_str(), t)).collect();
    let mut kids: HashMap<&str, Vec<&Task>> = HashMap::new();
    for t in &tasks {
        if let Some(parent) = t.parent.as_deref().filter(|p| by_slug.contains_key(p)) {
            kids.entry(parent).or_default().push(t);
        }
    }
    let children = |slug: &str| kids.get(slug).cloned().unwrap_or_default();
    // The tasks under it still to be executed: a subtask in review, done or cancelled has an executor
    // that is waiting or finished, not on this work.
    let below = |task: &Task| -> Vec<&Task> {
        let (mut found, mut todo) = (Vec::new(), children(&task.slug));
        while let Some(child) = todo.pop() {
            if EXECUTING.contains(&child.status.as_str()) {
                found.push(child);
            }
            todo.extend(children(&child.slug));
        }
        found
    };
    let present = |task: &Task, role: &str| {
        task.roles
            .iter()
            .any(|r| r.role == role && (r.health.holds() || busy.contains(r.handle.as_str())))
    };
    let staffed = |task: &Task| {
        present(task, "executor") || below(task).iter().any(|c| present(c, "executor"))
    };
    let mut running = Vec::new();
    let mut found: BTreeMap<&'static str, Vec<Task>> = BTreeMap::new();
    let mut notes = BTreeMap::new();
    let mut executing = 0;
    for t in &tasks {
        let cancelled: Vec<&str> = t
            .depends
            .iter()
            .filter(|slug| {
                by_slug
                    .get(slug.as_str())
                    .is_some_and(|d| d.status == "cancelled")
            })
            .map(String::as_str)
            .collect();
        if !matches!(t.status.as_str(), "done" | "cancelled") && !cancelled.is_empty() {
            found
                .entry("dependency_blocked")
                .or_default()
                .push(t.clone());
            notes.insert(
                t.slug.clone(),
                format!("cancelled dependencies: {}", cancelled.join(", ")),
            );
            continue;
        }
        match t.status.as_str() {
            "ready" => {
                let free = t
                    .depends
                    .iter()
                    .filter_map(|d| by_slug.get(d.as_str()))
                    .all(|d| d.status == "done");
                if free && !staffed(t) {
                    found.entry("ready_unstaffed").or_default().push(t.clone());
                    let reserved = t
                        .roles
                        .iter()
                        .find(|r| r.role == "reserved" && r.health.holds());
                    if let Some(r) = reserved {
                        notes.insert(t.slug.clone(), format!("reserved {}", r.handle));
                    }
                }
            }
            "in_progress" => {
                if staffed(t) {
                    executing += 1;
                    running.push(t.slug.clone());
                } else {
                    found
                        .entry("in_progress_orphaned")
                        .or_default()
                        .push(t.clone());
                }
            }
            "review"
                if !present(t, "reviewer") && !below(t).iter().any(|c| present(c, "executor")) =>
            {
                found
                    .entry("review_unreviewed")
                    .or_default()
                    .push(t.clone());
            }
            _ => {}
        }
        let open = t.status == "in_progress" || t.status == "review";
        if open && (posted.contains(&t.slug) || landed.is_some_and(|landed| landed(t))) {
            found.entry("landed_open").or_default().push(t.clone());
            notes
                .entry(t.slug.clone())
                .or_insert_with(|| format!("landed on main, task still {}", t.status));
        }
    }
    for listed in found.values_mut() {
        listed.sort_by(|a, b| (a.priority, &a.slug).cmp(&(b.priority, &b.slug)));
    }
    Ok(Digest {
        now,
        executing,
        running,
        tasks: found,
        idle: idle.to_vec(),
        notes,
        liveness_unknown,
    })
}
