//! The opt-in waiting board: complete facets and column totals, then one ordered page.
//! Search and finished-window selection are applied by the parent before this projection.

use super::{Json, PAGE, Request, Response, StatusCode, error, no_store, query};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use symphony_api::{
    MachineWork, Task, TaskListing, Tasks, WAITS_KINDS, WaitingBoard, WaitingColumn,
};
use symphony_board::waits_on::{NO_REASON, NOBODY, PARKED};

/// The whole ordering key, also the opaque cursor. A wait-kind list has no stage partition.
#[derive(Serialize, Deserialize)]
struct Key(String, u8, usize, f64, String);

impl Key {
    fn compare(&self, other: &Self) -> Ordering {
        self.0
            .cmp(&other.0)
            .then(self.1.cmp(&other.1))
            .then(self.2.cmp(&other.2))
            .then(self.3.total_cmp(&other.3))
            .then_with(|| symphony_api::slug_order(&self.4, &other.4))
    }
}

fn kind(task: &Task) -> &str {
    task.waits_on
        .as_ref()
        .map(|waits| waits.kind.as_str())
        .filter(|kind| WAITS_KINDS.contains(kind))
        .unwrap_or(NOBODY)
}

fn since(task: &Task) -> f64 {
    task.waits_on
        .as_ref()
        .map(|waits| waits.since)
        .filter(|since| since.is_finite())
        .unwrap_or(task.updated)
}

/// Only the board's named block can be blocked. Its derived `nobody` line can name a dependency
/// that disappeared; that is still a real block. An unnamed legacy block is ordinary work.
fn normalize(task: &mut Task) {
    if task.status != "blocked" {
        return;
    }
    let named = task.waits_on.as_ref().is_some_and(|waits| {
        waits.kind == PARKED
            || (["decision", "check", "task", NOBODY].contains(&waits.kind.as_str())
                && !waits.who.trim().is_empty()
                && waits.why != NO_REASON)
    });
    if !named {
        task.status = original(task).to_string();
        task.waits_on = Some(symphony_api::WaitsOn {
            kind: NOBODY.into(),
            who: String::new(),
            why: NO_REASON.into(),
            since: since(task),
        });
    }
}

fn original(task: &Task) -> &str {
    task.from
        .as_deref()
        .filter(|from| {
            [
                "concept",
                "design",
                "backlog",
                "ready",
                "in_progress",
                "review",
            ]
            .contains(from)
        })
        .unwrap_or("ready")
}

fn stage<'a>(task: &'a Task, work: &MachineWork, activity: bool) -> &'a str {
    if task.status == "blocked" {
        original(task)
    } else {
        work.task_stage(task, activity).as_str()
    }
}

fn key(task: &Task, stage: &str, across: bool) -> Key {
    let blocked = task.status == "blocked";
    let parked = kind(task) == PARKED;
    Key(
        if across { String::new() } else { stage.into() },
        if parked { 2 } else { u8::from(!blocked) },
        if blocked && !parked {
            WAITS_KINDS
                .iter()
                .position(|one| *one == kind(task))
                .unwrap_or(1)
        } else {
            0
        },
        if task.is_finished() {
            -task.updated
        } else {
            since(task)
        },
        task.slug.clone(),
    )
}

fn refusal(request: &Request) -> Option<String> {
    if ["parent", "holder", "role", "roots"]
        .iter()
        .any(|name| query(request, name).is_some())
        || query(request, "status")
            .as_deref()
            .is_some_and(|status| ["signoff", "blocked"].contains(&status))
    {
        return Some("view=waiting selects board columns, not hierarchy, holders, signoff or a blocked column".into());
    }
    query(request, "wait")
        .filter(|kind| !WAITS_KINDS.contains(&kind.as_str()))
        .map(|kind| {
            format!(
                "unknown wait kind {kind:?}: expected {}",
                WAITS_KINDS.join(", ")
            )
        })
}

pub(super) fn project(
    request: &Request,
    mut tasks: Tasks,
    work: MachineWork,
    signoff: Vec<String>,
) -> Response {
    if let Some(why) = refusal(request) {
        return error(StatusCode::BAD_REQUEST, why);
    }
    let cursor = match query(request, "cursor")
        .map(|text| serde_json::from_str::<Key>(&text))
        .transpose()
    {
        Ok(cursor) if cursor.as_ref().is_none_or(|key| key.3.is_finite()) => cursor,
        _ => return error(StatusCode::BAD_REQUEST, "invalid waiting-board cursor"),
    };
    let wait = query(request, "wait");
    let status = query(request, "status");
    let activity = query(request, "activity").as_deref() == Some("1");
    let mut waiting = WaitingBoard {
        kinds: WAITS_KINDS.iter().map(|kind| ((*kind).into(), 0)).collect(),
        columns: Vec::new(),
    };
    for task in &mut tasks.tasks {
        normalize(task);
        if !task.is_finished() {
            let rank = WAITS_KINDS
                .iter()
                .position(|one| *one == kind(task))
                .unwrap_or(1);
            waiting.kinds[rank].1 += 1;
        }
    }
    tasks.tasks.retain(|task| {
        wait.as_ref()
            .is_none_or(|wanted| !task.is_finished() && kind(task) == wanted)
    });
    let mut columns = std::collections::BTreeMap::<String, WaitingColumn>::new();
    let mut lifecycle = std::collections::BTreeMap::<String, usize>::new();
    for task in &tasks.tasks {
        let stage = stage(task, &work, activity);
        let column = columns
            .entry(stage.into())
            .or_insert_with(|| WaitingColumn {
                status: stage.into(),
                ..WaitingColumn::default()
            });
        if task.status == "blocked" {
            column.blocked += 1;
        } else {
            column.ordinary += 1;
        }
        column.oldest = Some(
            column
                .oldest
                .map_or(since(task), |old| old.min(since(task))),
        );
        *lifecycle.entry(task.status.clone()).or_default() += 1;
    }
    waiting.columns = columns.into_values().collect();
    let counts = waiting
        .columns
        .iter()
        .map(|column| (column.status.clone(), column.ordinary + column.blocked))
        .collect();
    let mut rows: Vec<_> = tasks
        .tasks
        .into_iter()
        .filter_map(|task| {
            let stage = stage(&task, &work, activity);
            if status.as_ref().is_some_and(|wanted| wanted != stage) {
                return None;
            }
            let key = key(&task, stage, wait.is_some());
            Some((key, task))
        })
        .collect();
    rows.sort_by(|(a, _), (b, _)| a.compare(b));
    if let Some(cursor) = cursor {
        rows.retain(|(key, _)| key.compare(&cursor) == Ordering::Greater);
    }
    let offset = query(request, "offset")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let length = query(request, "rows")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(PAGE)
        .max(PAGE);
    let all = query(request, "all").as_deref() == Some("1");
    let counts_only = query(request, "counts").as_deref() == Some("1");
    let mut rows: Vec<_> = rows
        .into_iter()
        .skip(offset)
        .take(if counts_only {
            0
        } else if all {
            usize::MAX
        } else {
            length
        })
        .collect();
    let next = rows
        .last()
        .map(|(key, _)| serde_json::to_string(key).expect("a finite task key serializes"));
    for (_, task) in &mut rows {
        task.body.clear();
    }
    no_store(Json(TaskListing {
        machine_work: if query(request, "work").as_deref() == Some("0") {
            MachineWork::default()
        } else {
            work
        },
        tasks: rows.into_iter().map(|(_, task)| task).collect(),
        statuses: symphony_board::STATUSES
            .iter()
            .filter(|status| **status != "blocked")
            .copied()
            .chain(activity.then_some(["build", "test"]).into_iter().flatten())
            .map(str::to_string)
            .collect(),
        signoff,
        counts,
        lifecycle_counts: lifecycle.into_iter().collect(),
        next,
        orchestrators: tasks.orchestrators,
        unstarted: tasks.unstarted,
        waiting: Some(waiting),
    }))
}
