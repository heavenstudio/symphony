//! Select display roles and legacy run details independently of machine execution evidence.

use super::Run;
use crate::{Task, health::FINISHED};
use std::collections::HashMap;

fn rank(role: &str) -> u8 {
    match role {
        "executor" => 0,
        "reviewer" => 1,
        "reserved" => 2,
        _ => 9,
    }
}

pub(super) fn pane_state(status: &str) -> Option<&'static str> {
    match status {
        "working" => Some("working"),
        "blocked" => Some("waiting_on_input"),
        "idle" | "done" => Some("idle"),
        _ => None,
    }
}

/// Index roles once, rather than scanning every task for every historical session on each tick.
pub(super) fn tasks_by_handle(tasks: &[Task]) -> HashMap<&str, Vec<(&Task, &str)>> {
    let mut held = HashMap::<&str, Vec<(&Task, &str)>>::new();
    for task in tasks {
        let mut roles = HashMap::<&str, &str>::new();
        for role in task.roles.iter().filter(|r| r.role != "orchestrator") {
            let best = roles.entry(&role.handle).or_insert(&role.role);
            if rank(&role.role) < rank(best) {
                *best = &role.role;
            }
        }
        for (handle, role) in roles {
            held.entry(handle).or_default().push((task, role));
        }
    }
    held
}

/// The task it works now: an open one by role, priority and slug; else the finished one updated last.
pub(super) fn primary<'t>(held: &[(&'t Task, &'t str)]) -> Option<(&'t Task, &'t str)> {
    let open = held
        .iter()
        .filter(|(t, _)| !FINISHED.contains(&t.status.as_str()))
        .min_by(|(a, ra), (b, rb)| {
            (rank(ra), a.priority, &a.slug).cmp(&(rank(rb), b.priority, &b.slug))
        });
    if let Some(found) = open {
        return Some(*found);
    }
    let mut last: Option<(&Task, &str)> = None;
    for &(t, role) in held {
        if last.is_none_or(|(l, _)| t.updated > l.updated) {
            last = Some((t, role));
        }
    }
    last
}

/// Of `runs`, a holding one before a waiting one, the earliest first.
pub(super) fn first<'r>(runs: &[&'r Run]) -> Option<&'r Run> {
    let holding: Vec<&Run> = runs
        .iter()
        .copied()
        .filter(|r| r.phase == "holding")
        .collect();
    let pool = if holding.is_empty() {
        runs
    } else {
        holding.as_slice()
    };
    let since = |r: &Run| r.since.unwrap_or(0.0);
    let mut found: Option<&Run> = None;
    for &r in pool {
        if found.is_none_or(|f| since(r) < since(f)) {
            found = Some(r);
        }
    }
    found
}
