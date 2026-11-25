//! What the agents page shows of the board beside its agents.

use std::collections::BTreeSet;

use symphony_api::Task;

/// How many of an agent's finished tasks its card lists.
const FINISHED: usize = 5;

/// The tasks of `board` the page shows beside the agents named `handles`: each one's five tasks
/// finished last (`done` or `cancelled`, in any role) and every open task it holds in a role other
/// than orchestrator, in the board's order and without their briefs. Only those are copied.
pub(super) fn tasks<'a>(board: &[Task], handles: impl IntoIterator<Item = &'a str>) -> Vec<Task> {
    let finished = |task: &Task| ["done", "cancelled"].contains(&task.status.as_str());
    let mut shown = BTreeSet::new();
    for handle in handles {
        let holds = |task: &Task| task.roles.iter().any(|r| r.handle == handle);
        let mut done: Vec<&Task> = board
            .iter()
            .filter(|task| finished(task) && holds(task))
            .collect();
        symphony_board::order::sort_by(&mut done, &|a, b| b.updated.total_cmp(&a.updated));
        shown.extend(
            done.into_iter()
                .take(FINISHED)
                .map(|task| task.slug.as_str()),
        );
        shown.extend(
            board
                .iter()
                .filter(|task| {
                    !finished(task)
                        && task
                            .roles
                            .iter()
                            .any(|r| r.handle == handle && r.role != "orchestrator")
                })
                .map(|task| task.slug.as_str()),
        );
    }
    board
        .iter()
        .filter(|task| shown.contains(task.slug.as_str()))
        .map(|task| Task {
            body: String::new(),
            ..task.clone()
        })
        .collect()
}

#[cfg(test)]
mod tests;
