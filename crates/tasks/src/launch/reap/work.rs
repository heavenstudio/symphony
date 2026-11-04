//! Pending work that keeps an idle pane, including descendant sessions and re-review.

use crate::{Error, Store};
use std::collections::{HashMap, HashSet};

/// Handles that hold live work, themselves or through a session under them (a subagent): an
/// executor on a task `in_progress`, `blocked` or in `review`, a reviewer on one in `review`. A
/// departed middle session still links its live descendants to their ancestors; leaving releases
/// its own roles.
pub(super) fn live_work(store: &Store) -> Result<HashSet<String>, Error> {
    let mut own: Vec<String> = Vec::new();
    let tasks = store.tasks(None)?;
    own.extend(store.pending_posts(&tasks)?.into_iter().map(|p| p.handle));
    let threads = store.threads(
        &tasks
            .iter()
            .filter(|t| t.status == "in_progress")
            .map(|t| t.slug.clone())
            .collect::<Vec<_>>(),
    )?;
    for task in tasks {
        for role in &task.roles {
            let live = match role.role.as_str() {
                "executor" => matches!(task.status.as_str(), "in_progress" | "blocked" | "review"),
                "reviewer" => {
                    task.status == "review"
                        || (task.status == "in_progress"
                            && threads.get(&task.slug).is_some_and(|thread| {
                                thread
                                    .iter()
                                    .rev()
                                    .find(|post| {
                                        post.kind == "verdict" && post.sender == role.handle
                                    })
                                    .is_some_and(|post| {
                                        post.text == "changes" || post.text.starts_with("changes:")
                                    })
                            }))
                }
                _ => false,
            };
            if live {
                own.push(role.handle.clone());
            }
        }
    }
    let parent: HashMap<String, String> = store
        .sessions()?
        .into_iter()
        .filter_map(|s| Some((s.row.handle, s.row.parent?)))
        .collect();
    let mut held = HashSet::new();
    for handle in own {
        let mut seen = HashSet::new();
        let mut at = Some(handle);
        while let Some(here) = at.filter(|h| seen.insert(h.clone())) {
            // the holder and every session above it
            held.insert(here.clone());
            at = parent.get(&here).cloned();
        }
    }
    Ok(held)
}
