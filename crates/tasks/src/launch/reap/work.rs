//! Pending work that keeps an idle pane, including descendant sessions and re-review.

use crate::{Error, Message, Store};
use std::collections::{HashMap, HashSet};

/// Landing reviews use a message to the executor; ordinary board reviews use a verdict record.
fn review_decision(post: &Message, handle: &str, slug: &str) -> Option<bool> {
    if post.sender != handle {
        return None;
    }
    if post.kind == "verdict" {
        return Some(post.text == "changes" || post.text.starts_with("changes:"));
    }
    if post.kind != "msg" || post.recipient.as_deref() != Some("@executor") {
        return None;
    }
    let prefix = format!("{handle} asks for changes on ");
    let (branch, rest) = post
        .text
        .strip_prefix(prefix.as_str())?
        .split_once(" at tip ")?;
    if branch != slug && slug.strip_prefix("review/") != Some(branch) {
        return None;
    }
    let (tip, _) = rest.split_once(": ")?;
    (!tip.is_empty() && tip.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(true)
}

fn awaits_rereview(thread: &[Message], handle: &str, slug: &str) -> bool {
    thread
        .iter()
        .filter_map(|post| review_decision(post, handle, slug).map(|changes| (post.id, changes)))
        .max_by_key(|(id, _)| *id)
        .is_some_and(|(_, changes)| changes)
}

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
                                awaits_rereview(thread, &role.handle, &task.slug)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn post(id: i64, kind: &str, text: &str) -> Message {
        Message {
            id,
            slug: "sim/jump".into(),
            sender: "reviewer".into(),
            recipient: Some("@executor".into()),
            kind: kind.into(),
            text: text.into(),
            ts: 1.0,
        }
    }

    #[test]
    fn landing_changes_require_the_reviewers_actual_executor_message_and_branch() {
        let text = "reviewer asks for changes on sim/jump at tip 012345abcdef: fix the edge";
        let original = post(1, "msg", text);
        assert_eq!(
            review_decision(&original, "reviewer", "sim/jump"),
            Some(true)
        );
        assert_eq!(
            review_decision(&original, "reviewer", "review/sim/jump"),
            Some(true)
        );
        assert_eq!(review_decision(&original, "reviewer", "sim/other"), None);
        for (sender, to, kind, body) in [
            ("other", Some("@executor"), "msg", text),
            ("reviewer", None, "msg", text),
            ("reviewer", Some("@reviewer"), "msg", text),
            ("reviewer", Some("@executor"), "system", text),
            (
                "reviewer",
                Some("@executor"),
                "msg",
                "reviewer asks for changes on sim/jump at tip xyz: fix",
            ),
            (
                "reviewer",
                Some("@executor"),
                "msg",
                "reviewer asks for changes on sim/jump at tip : fix",
            ),
            (
                "reviewer",
                Some("@executor"),
                "msg",
                "reviewer asks for changes on sim/jump at tip 012345abcdef",
            ),
            ("reviewer", Some("@executor"), "msg", "please fix the edge"),
        ] {
            let mut invalid = original.clone();
            invalid.sender = sender.into();
            invalid.recipient = to.map(str::to_string);
            invalid.kind = kind.into();
            invalid.text = body.into();
            assert_eq!(
                review_decision(&invalid, "reviewer", "sim/jump"),
                None,
                "{invalid:?}"
            );
        }
    }

    #[test]
    fn only_a_later_review_decision_clears_the_need_for_rereview() {
        let changes = post(
            2,
            "msg",
            "reviewer asks for changes on sim/jump at tip 012345abcdef: (no note)",
        );
        let earlier = post(1, "verdict", "approve: previous patch");
        let chat = post(3, "msg", "waiting for the fix");
        assert!(awaits_rereview(
            &[chat, changes.clone(), earlier],
            "reviewer",
            "sim/jump"
        ));
        let approved = post(4, "verdict", "approve: fixed");
        assert!(!awaits_rereview(
            &[changes, approved],
            "reviewer",
            "sim/jump"
        ));
        for text in ["changes", "changes: fix"] {
            assert!(awaits_rereview(
                &[post(5, "verdict", text)],
                "reviewer",
                "sim/jump"
            ));
        }
        assert!(!awaits_rereview(&[], "reviewer", "sim/jump"));
    }
}
