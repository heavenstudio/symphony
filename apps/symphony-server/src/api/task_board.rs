//! Board display-stage counts and pagination share one filtered snapshot.

use super::{Hub, Json, Request, Response, StatusCode, error, no_store, query};

/// What a listing's `status` selects beside the board's statuses: the owner's sign-off column and
/// the display stages of work a machine builds or tests.
const SELECTABLE: [&str; 3] = ["signoff", "build", "test"];

/// The tasks a listing's page holds, unless its `rows` asks for more. The board's client reserves
/// a page's rows of a column while it loads (`PAGE` in `apps/symphony-web/src/board.tsx`), so the
/// two change together.
const PAGE: usize = 30;

/// A listing's `status`, when it names one: a board status or one of [`SELECTABLE`].
pub(super) fn status_argument(request: &Request) -> Result<(), String> {
    match query(request, "status") {
        Some(wanted)
            if !SELECTABLE.contains(&wanted.as_str())
                && !symphony_board::STATUSES.contains(&wanted.as_str()) =>
        {
            Err(format!(
                "no status {wanted:?}: a listing selects one of {} or {}",
                symphony_board::STATUSES.join(", "),
                SELECTABLE.join(", ")
            ))
        }
        _ => Ok(()),
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
        hub.task_signoff
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone(),
    )
}

fn project(
    request: &Request,
    mut tasks: symphony_api::Tasks,
    search: &str,
    scores: &std::collections::BTreeMap<String, f64>,
    work: symphony_api::MachineWork,
    mut signoff: Vec<String>,
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
    // hierarchy readers) retain their status/count/cursor semantics without that opt-in.
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
        return waiting::project(request, tasks, work, signoff);
    }
    let cursor = match query(request, "cursor")
        .map(|text| serde_json::from_str::<(String, i64, f64, String)>(&text))
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
    // Slugs order by `slug_order` (`ready-3` before `ready-30`), here and in the cursor below,
    // which must agree with this sort for a page to start where the one before it ended.
    let by_priority = |a: &symphony_api::Task, b: &symphony_api::Task| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| symphony_api::slug_order(&a.slug, &b.slug))
    };
    tasks.tasks.sort_by(|a, b| {
        if !search.is_empty() {
            return scores[&b.slug]
                .total_cmp(&scores[&a.slug])
                .then_with(|| symphony_api::slug_order(&a.slug, &b.slug));
        }
        if parent.is_some() || roots || holder.is_some() {
            return by_priority(a, b);
        }
        stage(a).cmp(&stage(b)).then_with(|| {
            if a.is_finished() {
                b.updated
                    .total_cmp(&a.updated)
                    .then_with(|| symphony_api::slug_order(&a.slug, &b.slug))
            } else {
                by_priority(a, b)
            }
        })
    });
    // Whether `slug` comes after the cursor's slug, in the order above.
    let after_slug = |slug: &str, cursor: &str| {
        symphony_api::slug_order(slug, cursor) == std::cmp::Ordering::Greater
    };
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    let mut lifecycle_counts = std::collections::BTreeMap::<String, usize>::new();
    let mut parent_index = 0_usize;
    tasks.tasks.retain(|task| {
        *lifecycle_counts.entry(task.status.clone()).or_default() += 1;
        let task_stage = stage(task);
        let count = counts.entry(task_stage.clone()).or_default();
        let index = *count;
        *count += 1;
        let selected = status
            .as_ref()
            .is_none_or(|s| task_stage == *s || (s == "signoff" && signoff.contains(&task.slug)));
        let after = cursor
            .as_ref()
            .is_none_or(|(state, priority, updated, slug)| {
                if !search.is_empty() {
                    return scores[&task.slug] < *updated
                        || (scores[&task.slug] == *updated && after_slug(&task.slug, slug));
                }
                let after_priority = || {
                    task.priority > *priority
                        || (task.priority == *priority && after_slug(&task.slug, slug))
                };
                if parent.is_some() || roots || holder.is_some() {
                    return after_priority();
                }
                task_stage > *state
                    || (task_stage == *state
                        && if ["done", "cancelled"].contains(&state.as_str()) {
                            task.updated < *updated
                                || (task.updated == *updated && after_slug(&task.slug, slug))
                        } else {
                            after_priority()
                        })
            });
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
    for task in &mut tasks.tasks {
        task.body.clear();
    }
    // `work=0`: the reader has the machine's work from `/api/machine-work` and the feed. Its
    // recorders' heartbeats move it every sample, so a listing without it keeps one validator
    // while its tasks and counts stand (docs/analysis/client-cache.md).
    let without_work = query(request, "work").as_deref() == Some("0");
    let next = tasks.tasks.last().map(|task| {
        serde_json::to_string(&(
            stage(task),
            task.priority,
            if search.is_empty() {
                task.updated
            } else {
                scores[&task.slug]
            },
            &task.slug,
        ))
        .expect("a task paging key serializes")
    });
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
