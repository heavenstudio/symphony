//! What work a task counted as over the alarm's window, as the board's tree stood then: a review
//! mirror counts on its own until the task it reviews is made, and an instruction subtask counts
//! with its parent only while it is under one the board holds ([`Ledger::work`]), so a task made
//! or moved in the tree changes the counts from then on, never the hours before ([`own_over`]).

use super::Line;
use super::rework::merged;
use crate::Task;
use crate::slots::{Entry, Ledger};
use std::collections::{HashMap, HashSet};

/// The line a move in the tree writes: `HANDLE put it under PARENT (was PARENT)`, `nothing` for
/// the top.
pub const UNDER: &str = " put it under ";
/// What a move's line says of the parent it left, after the one it went under.
pub const WAS: &str = " (was ";
/// The top of the tree, as a move's line names it.
pub const NOTHING: &str = "nothing";

/// A move of a task in the tree, as its line says it ([`moved`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moved<'a> {
    /// The parent it went under; none, the top.
    pub to: Option<&'a str>,
    /// The parent it left; none, the top.
    pub from: Option<&'a str>,
}

/// The move in the tree `line` says, if it is one: a system line, its writer's handle before
/// [`UNDER`]. A line written before the board said what a move left is taken to have left the
/// task where it went.
pub fn moved(line: &Line) -> Option<Moved<'_>> {
    let (by, rest) = line.text.split_once(UNDER)?;
    if line.kind != "system" || by.contains(' ') {
        return None;
    }
    let said = rest.strip_suffix(')').and_then(|rest| rest.split_once(WAS));
    let (to, from) = said.unwrap_or((rest, rest));
    Some(Moved {
        to: parent(to),
        from: parent(from),
    })
}

/// The parent a move's line names: none for [`NOTHING`], the top.
fn parent(slug: &str) -> Option<&str> {
    (slug != NOTHING).then_some(slug)
}

/// The parent a task was under at `at`, its moves in the window (`moves`, oldest first) leaving
/// `now`, its parent now: what the newest move by then left it under, else what the oldest left.
fn parent_at<'a>(now: Option<&'a str>, moves: &[(f64, Moved<'a>)], at: f64) -> Option<&'a str> {
    match (moves.iter().rev()).find(|(when, _)| *when <= at) {
        Some((_, last)) => last.to,
        None => moves.first().map_or(now, |(_, first)| first.from),
    }
}

/// Whether each of `counted` that counted with other work at some time over the window from
/// `since` to `now` counted as work of its own from each time on, oldest first ([`Ledger::work`]):
/// as the tree of `tasks`, the board's, stood at each change of it that can change what one counts
/// as, each task there from when it was made and each under the parent its moves in the window
/// (`moves`, oldest first) left it then. One left out counted as its own all through: tasks are
/// never removed, so one that is its own now with no move in the window was its own all through.
pub fn own_over<'a>(
    tasks: &'a [Task],
    counted: &[&'a Task],
    moves: &HashMap<&'a str, Vec<(f64, Moved<'a>)>>,
    since: f64,
    now: f64,
) -> HashMap<&'a str, Vec<(f64, bool)>> {
    let parent_at = |task: &'a Task, at: f64| {
        let now = task.parent.as_deref();
        (moves.get(task.slug.as_str())).map_or(now, |moves| parent_at(now, moves, at))
    };
    let ledger_at = |at: f64| {
        let made = tasks.iter().filter(|t| t.created <= at);
        Ledger::new(made.map(|t| entry(t, parent_at(t, at))), [])
    };
    let today = ledger_at(now);
    let others: Vec<&Task> = (counted.iter().copied())
        .filter(|t| today.work(&t.slug) != t.slug || moves.contains_key(t.slug.as_str()))
        .collect();
    if others.is_empty() {
        return HashMap::new();
    }
    // What can change what one of them counts as: a task its slug holds made (the task a mirror
    // reviews, the mirror itself), one it was under made, or a move of it.
    let mut under: HashSet<&str> = HashSet::new();
    for task in &others {
        under.extend(task.parent.as_deref());
        let left = moves.get(task.slug.as_str()).into_iter().flatten();
        under.extend(left.flat_map(|(_, m)| [m.to, m.from]).flatten());
    }
    let changes_one = |t: &Task| {
        under.contains(t.slug.as_str())
            || others.iter().any(|o| {
                o.slug.contains(&t.slug)
                    || t.branch
                        .as_deref()
                        .is_some_and(|branch| branch == crate::on_review::branch_of(&o.slug))
            })
    };
    let made = (tasks.iter())
        .filter(|t| t.created > since && t.created <= now && changes_one(t))
        .map(|t| t.created);
    let moved_at = (others.iter())
        .filter_map(|t| moves.get(t.slug.as_str()))
        .flat_map(|moves| moves.iter().map(|(at, _)| *at));
    let mut times: Vec<f64> = std::iter::once(since)
        .chain(made)
        .chain(moved_at.filter(|at| *at > since && *at <= now))
        .collect();
    #[expect(
        clippy::disallowed_methods,
        reason = "a busy board path keeps std's in-place sort (crate::order): PULL's alarms"
    )]
    times.sort_unstable_by(f64::total_cmp);
    times.dedup();
    let mut own: HashMap<&'a str, Vec<(f64, bool)>> = HashMap::new();
    for at in times {
        let tree = ledger_at(at);
        for task in &others {
            let alone = task.created <= at && tree.work(&task.slug) == task.slug;
            let flags = own.entry(task.slug.as_str()).or_default();
            if flags.last().is_none_or(|(_, last)| *last != alone) {
                flags.push((at, alone));
            }
        }
    }
    own
}

/// `task` as the ledger reads which work it is part of, under `parent`.
fn entry<'a>(task: &'a Task, parent: Option<&'a str>) -> Entry<'a> {
    Entry {
        slug: &task.slug,
        branch: task.branch.as_deref(),
        parent,
        status: &task.status,
        instruction: task.instruction,
        held: false,
        orchestrators: Vec::new(),
        executors: Vec::new(),
        red: false,
        slot: None,
    }
}

/// `status`'s timeline counted only while the task was work of its own (`own`, [`own_over`]).
pub fn own_only<'a>(
    status: &[(f64, Option<&'a str>)],
    own: &[(f64, bool)],
) -> Vec<(f64, Option<&'a str>)> {
    merged(status, own, |status, own| status.filter(|_| own))
}

#[cfg(test)]
mod tests;
