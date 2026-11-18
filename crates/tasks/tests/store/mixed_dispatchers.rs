//! Dispatchers built before one orchestrator per task and after it, alternating on one
//! orchestrator's callback state: the fields older ones read keep their whole-board meaning, and
//! neither repeats a line the other said.

use super::common::Fixture;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use symphony_board::health::{EXECUTING, FINISHED};
use symphony_board::{Health, Task};

/// What a dispatcher built before one orchestrator per task records of `handle` once it has looked
/// and told: its look of the whole board (every ready task, every task below what it orchestrates
/// and the whole board's staffing; its children by parent), every open task's status, and every
/// entry of the whole digest as delivered. Reckoned here by those rules, from the board as it is.
pub(super) fn older_record(fixture: &Fixture, handle: &str) -> Value {
    let board = fixture.store.board().unwrap();
    let open = |slug: &str| {
        (board.tasks.iter()).any(|t| t.slug == slug && !FINISHED.contains(&t.status.as_str()))
    };
    let holds = |t: &&Task, roles: &[&str]| {
        (t.roles.iter()).any(|r| r.handle == handle && roles.contains(&r.role.as_str()))
    };
    let mine: Vec<&Task> = (board.tasks.iter())
        .filter(|t| t.roles.iter().any(|r| r.handle == handle))
        .collect();
    let mut below = BTreeSet::new();
    let mut todo: Vec<String> = (mine.iter())
        .filter(|t| holds(t, &["orchestrator"]))
        .map(|t| t.slug.clone())
        .collect();
    while let Some(slug) = todo.pop() {
        let children = (board.tasks.iter()).filter(|t| t.parent.as_deref() == Some(slug.as_str()));
        for child in children {
            if below.insert(child.slug.clone()) {
                todo.push(child.slug.clone());
            }
        }
    }
    let status = |slug: &str| {
        let task = (board.tasks.iter()).find(|t| t.slug == slug);
        task.unwrap().status.clone()
    };
    let statuses: BTreeMap<String, String> = (mine.iter().map(|t| t.slug.clone()))
        .chain(below.iter().cloned())
        .filter(|slug| open(slug))
        .map(|slug| (slug.clone(), status(&slug)))
        .collect();
    let verdict_tasks: BTreeSet<String> = (mine.iter())
        .filter(|t| holds(t, &["executor", "orchestrator"]))
        .map(|t| t.slug.clone())
        .chain(below.iter().cloned())
        .filter(|slug| open(slug))
        .collect();
    let roles: BTreeSet<(String, String, String)> = (mine.iter())
        .filter(|t| open(&t.slug))
        .flat_map(|t| (t.roles.iter()).map(|r| (t.slug.clone(), r.handle.clone(), r.role.clone())))
        .collect();
    let ready: BTreeMap<String, String> = (board.tasks.iter())
        .filter(|t| t.status == "ready")
        .map(|t| (t.slug.clone(), t.title.clone()))
        .collect();
    let active = |t: &&Task| {
        matches!(t.status.as_str(), "ready" | "review") || EXECUTING.contains(&t.status.as_str())
    };
    let staffing: BTreeSet<(&str, &str, &str, bool)> = (board.tasks.iter())
        .filter(active)
        .flat_map(|t| {
            (t.roles.iter())
                .filter(|r| matches!(r.role.as_str(), "executor" | "reviewer" | "reserved"))
                .map(|r| {
                    (
                        t.slug.as_str(),
                        r.handle.as_str(),
                        r.role.as_str(),
                        r.health.holds(),
                    )
                })
        })
        .collect();
    let children: BTreeMap<String, Health> = (board.sessions.iter())
        .filter(|s| s.row.parent.as_deref() == Some(handle))
        .map(|s| (s.row.handle.clone(), s.health))
        .collect();
    let every: BTreeMap<String, String> = (board.tasks.iter())
        .filter(|t| open(&t.slug))
        .map(|t| (t.slug.clone(), t.status.clone()))
        .collect();
    // With no pane to read, the agents view calls a stale worker idle.
    let idle: Vec<String> = (board.sessions.iter())
        .filter(|s| {
            s.health == Health::Stale && !matches!(s.row.kind.as_str(), "orchestrator" | "chat")
        })
        .map(|s| s.row.handle.clone())
        .collect();
    let digest =
        symphony_board::cli::pull::digest(&fixture.store, &idle, None, fixture.now(), &[]).unwrap();
    json!({
        "seen": {
            "latest_message": board.latest_message, "verdict_tasks": verdict_tasks,
            "staffing": serde_json::to_string(&staffing).unwrap(), "roles": roles,
            "statuses": statuses, "ready": ready, "children": children,
        },
        "statuses": every,
        "pull_entries": digest.entries(),
    })
}

/// The fields of `handle`'s callback state older dispatchers read: its record, every open status
/// and the delivered digest entries (as a set: their order is the store's).
pub(super) fn older_fields(fixture: &Fixture, handle: &str) -> Value {
    let state = fixture
        .store
        .raw()
        .row("callback_state", handle)
        .fields(&["seen", "statuses", "pull_entries"])
        .unwrap();
    let entries: BTreeSet<(String, String)> =
        serde_json::from_value(state["pull_entries"].clone()).unwrap();
    json!({"seen": state["seen"], "statuses": state["statuses"], "pull_entries": entries})
}
