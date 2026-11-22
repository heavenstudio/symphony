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
/// that disappeared; that is still a real block. An unnamed legacy block is ordinary work. A task
/// a custom workflow holds is blocked by its state, which the board's own block never names.
fn normalize(task: &mut Task) {
    if task.status != "blocked" || task.workflow.is_some() {
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

/// The column `task` goes in on this view: a blocked task the board's own writers keep in the
/// status it was blocked from, any other where the waiting view puts it ([`super::column`]), a
/// task a custom workflow holds in its phase's, blocked or not.
fn stage(task: &Task, work: &MachineWork, activity: bool) -> String {
    if task.status == "blocked" && task.workflow.is_none() {
        original(task).to_string()
    } else {
        super::column(work, task, activity)
    }
}

/// A wait facet that selects open held tasks by their coordination state, then the state's ID: the
/// tasks it selects are listed across their phases, as a wait kind's are, under the kind's own
/// `wait` (docs/workflow/WORKFLOW.md, "The app").
const STATE: &str = "state:";

/// The coordination state of a task a custom workflow holds; none for one the board's writers keep.
fn state(task: &Task) -> Option<&str> {
    task.workflow.as_ref().map(|place| place.state.as_str())
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
        .filter(|kind| {
            !WAITS_KINDS.contains(&kind.as_str())
                && !kind.strip_prefix(STATE).is_some_and(super::workflow_id)
        })
        .map(|kind| {
            format!(
                "unknown wait kind {kind:?}: expected {} or {STATE}STATE",
                WAITS_KINDS.join(", ")
            )
        })
}

pub(super) fn project(
    request: &Request,
    mut tasks: Tasks,
    work: MachineWork,
    signoff: Vec<String>,
    workflow: Option<u64>,
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
    let facet = query(request, "wait");
    let selected_state = (facet.as_deref())
        .and_then(|wanted| wanted.strip_prefix(STATE))
        .map(str::to_string);
    let wait = facet.filter(|_| selected_state.is_none());
    let across = wait.is_some() || selected_state.is_some();
    let status = query(request, "status");
    let activity = query(request, "activity").as_deref() == Some("1");
    let mut waiting = WaitingBoard {
        kinds: WAITS_KINDS.iter().map(|kind| ((*kind).into(), 0)).collect(),
        columns: Vec::new(),
        states: Vec::new(),
        workflow,
    };
    let mut states = std::collections::BTreeMap::<String, usize>::new();
    for task in &mut tasks.tasks {
        normalize(task);
        if !task.is_finished() {
            let rank = WAITS_KINDS
                .iter()
                .position(|one| *one == kind(task))
                .unwrap_or(1);
            waiting.kinds[rank].1 += 1;
            if let Some(state) = state(task) {
                *states.entry(state.into()).or_default() += 1;
            }
        }
    }
    waiting.states = states.into_iter().collect();
    tasks.tasks.retain(|task| {
        wait.as_ref()
            .is_none_or(|wanted| !task.is_finished() && kind(task) == wanted)
            && (selected_state.as_ref())
                .is_none_or(|wanted| !task.is_finished() && state(task) == Some(wanted.as_str()))
    });
    let mut columns = std::collections::BTreeMap::<String, WaitingColumn>::new();
    let mut lifecycle = std::collections::BTreeMap::<String, usize>::new();
    for task in &tasks.tasks {
        let stage = stage(task, &work, activity);
        let column = columns
            .entry(stage.clone())
            .or_insert_with(|| WaitingColumn {
                status: stage,
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
            if status.as_ref().is_some_and(|wanted| *wanted != stage) {
                return None;
            }
            let key = key(&task, &stage, across);
            Some((key, task))
        })
        .collect();
    symphony_board::order::sort_by(&mut rows, &|(a, _), (b, _)| a.compare(b));
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

#[cfg(test)]
mod ordering_study {
    use super::*;
    use serde_json::json;

    /// Original-sort oracle and measured shared ordering for the production Key+Task sort.
    #[tokio::test]
    async fn waiting_sort_study_preserves_full_key_payload_pages_and_reports_cost() {
        async fn read(records: &[Task], query: &str) -> Vec<u8> {
            let request = Request::builder()
                .uri(format!("/api/task-board?view=waiting&{query}"))
                .body(())
                .unwrap();
            let response = project(
                &request,
                Tasks {
                    version: String::new(),
                    tasks: records.to_vec(),
                    sessions: vec![],
                    orchestrators: vec![],
                    unstarted: 0,
                },
                MachineWork::default(),
                vec![],
                None,
            );
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec()
        }
        for count in [32, 512, 4096] {
            let records:Vec<Task>=(0..count).map(|i|serde_json::from_value(json!({
                "slug":format!("tools/task-{i}"),"title":format!("Exact title {i}"),"body":"payload ".repeat(32),
                "status":(["ready","blocked","review","done","cancelled"][i%5]),"from":"review","priority":i%4,"parent":null,"created_by":"owner","created":0.0,
                "updated":i%13,"links":[],"depends":[],"roles":[],"instruction":false,
                "waits_on":{"kind":(["decision","check","task","parked","working"][i%5]),"who":"named-thing","why":"exact reason","since":i%17}
            })).unwrap()).collect();
            for across in [false, true] {
                let rows = || {
                    records
                        .iter()
                        .map(|task| {
                            let mut task = task.clone();
                            normalize(&mut task);
                            (
                                key(&task, &stage(&task, &MachineWork::default(), false), across),
                                task,
                            )
                        })
                        .collect::<Vec<_>>()
                };
                let mut old_times = Vec::new();
                let mut new_times = Vec::new();
                let mut old = Vec::new();
                let mut shared = Vec::new();
                for sample in 0..10 {
                    old = rows();
                    shared = rows();
                    let (old_cpu, new_cpu) = if sample % 2 == 0 {
                        let (_, old_cpu) =
                            test_time::cpu::spent(|| old.sort_by(|(a, _), (b, _)| a.compare(b)));
                        let (_, new_cpu) = test_time::cpu::spent(|| {
                            symphony_board::order::sort_by(&mut shared, &|(a, _), (b, _)| {
                                a.compare(b)
                            })
                        });
                        (old_cpu, new_cpu)
                    } else {
                        let (_, new_cpu) = test_time::cpu::spent(|| {
                            symphony_board::order::sort_by(&mut shared, &|(a, _), (b, _)| {
                                a.compare(b)
                            })
                        });
                        let (_, old_cpu) =
                            test_time::cpu::spent(|| old.sort_by(|(a, _), (b, _)| a.compare(b)));
                        (old_cpu, new_cpu)
                    };
                    assert_eq!(
                        serde_json::to_vec(&shared).unwrap(),
                        serde_json::to_vec(&old).unwrap()
                    );
                    if sample >= 2 {
                        old_times.push(old_cpu);
                        new_times.push(new_cpu);
                    }
                }
                old_times.sort_unstable();
                new_times.sort_unstable();
                let (old_cpu, new_cpu) = (old_times[4], new_times[4]);
                assert_eq!(
                    serde_json::to_vec(&shared).unwrap(),
                    serde_json::to_vec(&old).unwrap()
                );
                let shared_tasks: Vec<_> = shared.into_iter().map(|(_, task)| task).collect();
                let normalized: Vec<_> = records
                    .iter()
                    .map(|task| {
                        let mut task = task.clone();
                        normalize(&mut task);
                        task
                    })
                    .collect();
                for query in if count <= 512 {
                    &[
                        "all=1",
                        "status=review",
                        "status=ready&rows=65&offset=7",
                        "wait=decision",
                        "wait=parked",
                        "counts=1",
                        "activity=1",
                    ][..]
                } else {
                    &[][..]
                } {
                    let first = read(&normalized, query).await;
                    assert_eq!(read(&shared_tasks, query).await, first, "{query}");
                    let value: serde_json::Value = serde_json::from_slice(&first).unwrap();
                    if let Some(cursor) = value["next"].as_str() {
                        let escaped: String = cursor.bytes().map(|b| format!("%{b:02X}")).collect();
                        let next = format!("{query}&cursor={escaped}");
                        assert_eq!(
                            read(&shared_tasks, &next).await,
                            read(&normalized, &next).await,
                            "{next}"
                        );
                    }
                }
                eprintln!(
                    "Waiting study n={count},across={across}: old_cpu_median={old_cpu:?},shared_cpu_median={new_cpu:?},old_range={:?},shared_range={:?}; Key+Task={}bytes,positions={}bytes plus usize scratch; existing signoff-guard scope unchanged",
                    (old_times[0], old_times[7]),
                    (new_times[0], new_times[7]),
                    std::mem::size_of::<(Key, Task)>(),
                    count * std::mem::size_of::<usize>()
                );
            }
        }
    }
}
