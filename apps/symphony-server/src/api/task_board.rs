//! Board display-stage counts and pagination share one filtered snapshot.

use super::{Hub, Json, Request, Response, StatusCode, error, no_store, query};
use crate::lock::Held as _;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// What a listing's `status` selects beside the board's statuses: the owner's sign-off column and
/// the display stages of work a machine builds or tests.
const SELECTABLE: [&str; 3] = ["signoff", "build", "test"];

/// The tasks a listing's page holds, unless its `rows` asks for more. The board's client reserves
/// a page's rows of a column while it loads (`PAGE` in `apps/symphony-web/src/board.tsx`), so the
/// two change together.
const PAGE: usize = 30;

/// The stages whose rows are listed newest first: a finished task's.
const FINISHED: [&str; 2] = ["done", "cancelled"];

/// What a listing's rows are ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Order {
    /// A search's: the best match first.
    Match,
    /// A task's subtasks, the task groups and a holder's tasks: by priority.
    Priority,
    /// The board's: by stage, a finished stage's rows newest first and any other's by priority.
    Stage,
}

/// A row's place in its listing, also the opaque cursor: its stage, its priority, when it was last
/// updated (a search's row: its score) and its slug. The sort, a cursor's test and the next page's
/// key are this one comparison ([`Key::compare`]), so a page starts where the one before it ended.
/// A cursor is read as the four values it was written as, so what a malformed one is told stays.
#[derive(Debug, Serialize, Deserialize)]
#[serde(from = "(String, i64, f64, String)")]
struct Key(String, i64, f64, String);

impl From<(String, i64, f64, String)> for Key {
    fn from((stage, priority, at, slug): (String, i64, f64, String)) -> Key {
        Key(stage, priority, at, slug)
    }
}

impl Key {
    /// Slugs order by `slug_order` (`ready-3` before `ready-30`), whatever came before them.
    fn compare(&self, other: &Key, order: Order) -> Ordering {
        let slug = || symphony_api::slug_order(&self.3, &other.3);
        let newest = || other.2.total_cmp(&self.2).then_with(slug);
        let priority = || self.1.cmp(&other.1).then_with(slug);
        match order {
            Order::Match => newest(),
            Order::Priority => priority(),
            Order::Stage => self.0.cmp(&other.0).then_with(|| {
                if FINISHED.contains(&self.0.as_str()) {
                    newest()
                } else {
                    priority()
                }
            }),
        }
    }
}

/// The column of a task a custom workflow holds: this, then its phase's ID. A phase's ID is the
/// project's own and may be any board status's word, so a phase's column never takes a status's
/// key, and a phase named for Build or Test never takes the process overlays' (docs/WORKFLOW.md,
/// "The app").
pub(super) const PHASE: &str = "phase:";

/// A listing's `status`, when it names one: a board status, one of [`SELECTABLE`], or in the
/// waiting view, the one whose columns are phases, a phase's column ([`PHASE`] and a workflow ID).
pub(super) fn status_argument(request: &Request) -> Result<(), String> {
    let phases = query(request, "view").as_deref() == Some("waiting");
    match query(request, "status") {
        Some(wanted)
            if !SELECTABLE.contains(&wanted.as_str())
                && !symphony_board::STATUSES.contains(&wanted.as_str())
                && !(phases && wanted.strip_prefix(PHASE).is_some_and(workflow_id)) =>
        {
            Err(format!(
                "no status {wanted:?}: a listing selects one of {}, {}, or in view=waiting \
                 {PHASE}PHASE",
                symphony_board::STATUSES.join(", "),
                SELECTABLE.join(", ")
            ))
        }
        _ => Ok(()),
    }
}

/// Whether `id` is a workflow ID's shape: a lowercase ASCII letter, then lowercase letters, digits,
/// `_` and `-` (docs/WORKFLOW.md, "Definition format 1").
pub(super) fn workflow_id(id: &str) -> bool {
    id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// The column the waiting view puts `task` in: the process overlay its machine work puts it in
/// (Build, Test), else its phase's when a custom workflow holds it ([`PHASE`]), else its status.
/// Every other listing keeps its lifecycle stage, a held task's status among them.
pub(super) fn column(
    work: &symphony_api::MachineWork,
    task: &symphony_api::Task,
    activity: bool,
) -> String {
    match (work.task_stage(task, activity), &task.workflow) {
        (symphony_api::TaskStage::Lifecycle(_), Some(place)) => format!("{PHASE}{}", place.phase),
        (stage, _) => stage.as_str().to_string(),
    }
}

pub(super) fn listing(
    hub: &Hub,
    request: &Request,
    tasks: symphony_api::Tasks,
    search: &str,
    scores: &std::collections::BTreeMap<String, f64>,
) -> Response {
    project(
        request,
        tasks,
        search,
        scores,
        if crate::hosted::admission(request).is_some() {
            symphony_api::MachineWork::default()
        } else {
            hub.machine_snapshot()
        },
        hub.task_signoff.lock().held().clone(),
        hub.workflow_generation(),
    )
}

fn project(
    request: &Request,
    mut tasks: symphony_api::Tasks,
    search: &str,
    scores: &std::collections::BTreeMap<String, f64>,
    work: symphony_api::MachineWork,
    mut signoff: Vec<String>,
    workflow: Option<u64>,
) -> Response {
    let offset = query(request, "offset")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    // `rows`: a page as long as the rows a column already shows, read again by the board's live
    // refresh so the pages it loaded stay; never shorter than a page.
    let page = query(request, "rows")
        .and_then(|v| v.parse::<usize>().ok())
        .map_or(PAGE, |rows| rows.max(PAGE));
    let all = query(request, "all").as_deref() == Some("1");
    // `status` is one a listing selects ([`status_argument`], checked before the board is read).
    let status = query(request, "status");
    // The board opts into display stages. Existing lifecycle queries (including holder and
    // hierarchy readers) retain their status/count/cursor semantics without that opt-in, a task a
    // custom workflow holds at its status: only the waiting view puts it in its phase's column.
    let activity = query(request, "activity").as_deref() == Some("1");
    let stage = |task: &symphony_api::Task| work.task_stage(task, activity).as_str().to_string();
    if !search.is_empty() {
        signoff.retain(|slug| scores.contains_key(slug));
    }
    let counts_only = query(request, "counts").as_deref() == Some("1");
    let older = query(request, "older").as_deref() == Some("1");
    let days = if query(request, "days").as_deref() == Some("7") {
        7.0
    } else {
        1.0
    };
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
        - days * 86_400.0;
    let parent = query(request, "parent");
    let holder = query(request, "holder");
    let role = query(request, "role");
    let open_only = query(request, "open").as_deref() == Some("1");
    let roots = query(request, "roots").as_deref() == Some("1");
    let top_level = parent.is_none() && holder.is_none();
    // A task's subtasks and the task groups list a subtask only while it is a row of its own: a
    // finished instruction that staffed its parent's work or review is the parent's row.
    let listed = |task: &symphony_api::Task| {
        symphony::taskview::listed_under_parent(&task.slug, task.instruction, &task.status)
    };
    let ids: std::collections::BTreeSet<_> = tasks.tasks.iter().map(|t| t.slug.clone()).collect();
    let parents: std::collections::BTreeSet<_> = tasks
        .tasks
        .iter()
        .filter(|t| listed(t))
        .filter_map(|t| t.parent.clone())
        .collect();
    tasks.tasks.retain(|task| {
        (!top_level || task.is_task())
            && (parent
                .as_ref()
                .is_none_or(|p| task.parent.as_ref() == Some(p) && listed(task)))
            && (!open_only || !task.is_finished())
            && holder.as_ref().is_none_or(|handle| {
                task.roles.iter().any(|r| {
                    r.handle == *handle && role.as_ref().is_none_or(|role| r.role == *role)
                })
            })
            && (!roots
                || (parents.contains(&task.slug)
                    && task.parent.as_ref().is_none_or(|p| !ids.contains(p))))
            && (all || older || !task.is_finished() || task.updated >= cutoff)
            && (search.is_empty() || scores.contains_key(&task.slug))
    });
    if query(request, "view").as_deref() == Some("waiting") {
        return waiting::project(request, tasks, work, signoff, workflow);
    }
    let cursor = match query(request, "cursor")
        .map(|text| serde_json::from_str::<Key>(&text))
        .transpose()
    {
        Ok(cursor) => cursor,
        Err(why) => {
            return error(
                StatusCode::BAD_REQUEST,
                format!("invalid task cursor: {why}"),
            );
        }
    };
    let order = if !search.is_empty() {
        Order::Match
    } else if parent.is_some() || roots || holder.is_some() {
        Order::Priority
    } else {
        Order::Stage
    };
    let mut rows: Vec<(Key, symphony_api::Task)> = std::mem::take(&mut tasks.tasks)
        .into_iter()
        .map(|task| {
            let at = if search.is_empty() {
                task.updated
            } else {
                scores[&task.slug]
            };
            (
                Key(stage(&task), task.priority, at, task.slug.clone()),
                task,
            )
        })
        .collect();
    rows.sort_by(|a, b| a.0.compare(&b.0, order));
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    let mut lifecycle_counts = std::collections::BTreeMap::<String, usize>::new();
    let mut parent_index = 0_usize;
    rows.retain(|(key, task)| {
        *lifecycle_counts.entry(task.status.clone()).or_default() += 1;
        let task_stage = &key.0;
        let count = counts.entry(task_stage.clone()).or_default();
        let index = *count;
        *count += 1;
        let selected = status
            .as_ref()
            .is_none_or(|s| task_stage == s || (s == "signoff" && signoff.contains(&task.slug)));
        let after = cursor
            .as_ref()
            .is_none_or(|cursor| key.compare(cursor, order) == Ordering::Greater);
        let index = if cursor.is_some() {
            let index = parent_index;
            if selected && after {
                parent_index += 1;
            }
            index
        } else if parent.is_some()
            || roots
            || holder.is_some()
            || (status.as_deref() == Some("signoff") && selected)
        {
            let index = parent_index;
            parent_index += 1;
            index
        } else {
            index
        };
        !counts_only
            && selected
            && after
            && (all
                || if cursor.is_some() {
                    index < page
                } else {
                    (offset..offset.saturating_add(page)).contains(&index)
                })
    });
    let next = rows
        .last()
        .map(|(key, _)| serde_json::to_string(key).expect("a task paging key serializes"));
    tasks.tasks = rows.into_iter().map(|(_, task)| task).collect();
    for task in &mut tasks.tasks {
        task.body.clear();
    }
    // `work=0`: the reader has the machine's work from `/api/machine-work` and the feed. Its
    // recorders' heartbeats move it every sample, so a listing without it keeps one validator
    // while its tasks and counts stand (docs/analysis/client-cache.md).
    let without_work = query(request, "work").as_deref() == Some("0");
    no_store(Json(symphony_api::TaskListing {
        machine_work: if without_work {
            symphony_api::MachineWork::default()
        } else {
            work
        },
        statuses: symphony_board::STATUSES
            .iter()
            .map(|s| (*s).to_string())
            .chain(
                activity
                    .then_some(["build", "test"])
                    .into_iter()
                    .flatten()
                    .map(str::to_string),
            )
            .collect(),
        next,
        tasks: tasks.tasks,
        signoff,
        counts: counts.into_iter().collect(),
        lifecycle_counts: lifecycle_counts.into_iter().collect(),
        orchestrators: tasks.orchestrators,
        unstarted: tasks.unstarted,
        waiting: None,
    }))
}

#[cfg(test)]
mod tests;

mod waiting;
