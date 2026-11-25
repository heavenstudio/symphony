//! Resolve role aliases from the thread prefix at each post, never from today's role holders.

use super::super::{Message, Role};
use crate::maps::Collect as _;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Holders {
    active: BTreeSet<String>,
    departed: BTreeSet<String>,
}

impl Holders {
    fn acquire(&mut self, handle: &str, exclusive: bool) {
        if exclusive {
            self.active.clear();
        }
        self.active.insert(handle.into());
        self.departed.clone_from(&self.active);
    }

    fn release(&mut self, handle: &str) {
        self.active.remove(handle);
        if !self.active.is_empty() {
            self.departed.clone_from(&self.active);
        }
    }

    fn recipients(&self) -> BTreeSet<String> {
        if self.active.is_empty() {
            self.departed.clone()
        } else {
            self.active.clone()
        }
    }
}

enum Change {
    Acquire(&'static str),
    Release(Option<&'static str>),
}

/// Today's exclusive executor, or its last holder after departure. This supplies routing identity,
/// not proof of current ownership, and is separate from the per-post recipients `resolve` returns.
pub(super) fn executor(thread: &[Message], current: &[Role]) -> Option<String> {
    current
        .iter()
        .find(|r| r.role == "executor")
        .map(|r| r.handle.clone())
        .or_else(|| {
            thread
                .iter()
                .filter(|post| matches!(change(post), Some(Change::Acquire("executor"))))
                .max_by_key(|post| post.id)
                .map(|post| post.sender.clone())
        })
}

fn change(post: &Message) -> Option<Change> {
    if post.kind != "system" {
        return None;
    }
    let text = post
        .text
        .strip_prefix(post.sender.as_str())?
        .strip_prefix(' ')?;
    if text.starts_with("claimed it") || text == "joined as executor" {
        Some(Change::Acquire("executor"))
    } else if text == "joined as reviewer" {
        Some(Change::Acquire("reviewer"))
    } else if text.starts_with("released every role (") {
        Some(Change::Release(None))
    } else if text.starts_with("released executor (") {
        Some(Change::Release(Some("executor")))
    } else if text.starts_with("released reviewer (") {
        Some(Change::Release(Some("reviewer")))
    } else {
        None
    }
}

/// Empty roles still address their last holders, so a request can reach a departed agent. The
/// next acquisition replaces that fallback. Snapshots of earlier posts retain their recipients.
pub(super) fn resolve(
    thread: &[Message],
    current: &[Role],
) -> BTreeMap<i64, BTreeMap<String, String>> {
    let mut acquired = BTreeSet::new();
    acquired.extend(thread.iter().filter_map(|post| match change(post) {
        Some(Change::Acquire(role)) => Some((post.sender.as_str(), role)),
        _ => None,
    }));
    // Imported current roles may predate the retained journal. Their `since` remains evidence;
    // a launch record alone cannot assign all old conversations to a role-addressed post.
    let imported: Vec<_> = current
        .iter()
        .filter(|role| !acquired.contains(&(role.handle.as_str(), role.role.as_str())))
        .collect();
    let mut ordered: Vec<_> = thread.iter().collect();
    #[expect(
        clippy::disallowed_methods,
        reason = "a busy board path keeps std's in-place sort (crate::order): PULL's unanswered posts"
    )]
    ordered.sort_by_key(|post| post.id);
    let mut roles: BTreeMap<&str, Holders> = BTreeMap::new();
    let mut last_role: BTreeMap<&str, &str> = BTreeMap::new();
    let mut resolved = BTreeMap::new();
    for post in ordered {
        match change(post) {
            Some(Change::Acquire(role)) => {
                roles
                    .entry(role)
                    .or_default()
                    .acquire(&post.sender, role == "executor");
                last_role.insert(&post.sender, role);
            }
            Some(Change::Release(role)) => {
                for (held, holders) in &mut roles {
                    if role.is_none_or(|released| released == *held) {
                        holders.release(&post.sender);
                    }
                }
            }
            None => {}
        }
        let Some(to) = post.recipient.as_deref() else {
            continue;
        };
        if let Some(role @ ("executor" | "reviewer")) = to.strip_prefix('@') {
            let mut recipients = roles
                .get(role)
                .map_or_else(BTreeSet::new, Holders::recipients);
            recipients.extend(
                imported
                    .iter()
                    .filter(|held| held.role == role && held.since <= post.ts)
                    .map(|held| held.handle.clone()),
            );
            resolved.insert(
                post.id,
                recipients
                    .into_iter()
                    .map(|handle| (handle, role.into()))
                    .collect_map(),
            );
        } else if !to.starts_with('@') {
            let role = last_role.get(to).copied().or_else(|| {
                imported
                    .iter()
                    .filter(|held| {
                        held.handle == to
                            && held.since <= post.ts
                            && matches!(held.role.as_str(), "executor" | "reviewer")
                    })
                    .max_by(|left, right| left.since.total_cmp(&right.since))
                    .map(|held| held.role.as_str())
            });
            if let Some(role) = role {
                resolved.insert(post.id, BTreeMap::from([(to.into(), role.into())]));
            }
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assigned(handles: &[&str], role: &str) -> BTreeMap<String, String> {
        handles
            .iter()
            .map(|handle| ((*handle).into(), role.into()))
            .collect()
    }

    fn post(id: i64, sender: &str, text: &str, to: Option<&str>) -> Message {
        Message {
            id,
            slug: "tools/a".into(),
            sender: sender.into(),
            recipient: to.map(str::to_string),
            kind: if to.is_some() { "msg" } else { "system" }.into(),
            text: text.into(),
            ts: 1.0,
        }
    }

    #[test]
    fn replacement_departure_does_not_reassign_its_request_to_an_older_reviewer() {
        let mut thread = vec![
            post(1, "a", "a joined as reviewer", None),
            post(2, "a", "a released reviewer (released)", None),
            post(3, "b", "b joined as reviewer", None),
            post(4, "owner", "review", Some("@reviewer")),
            post(5, "b", "b released every role (left)", None),
            post(6, "owner", "return", Some("@reviewer")),
            post(7, "c", "c joined as reviewer", None),
            post(8, "owner", "next", Some("@reviewer")),
        ];
        thread.reverse(); // Equal clocks and database row ordering cannot replace sequence ids.
        let resolved = resolve(&thread, &[]);
        assert_eq!(resolved[&4], assigned(&["b"], "reviewer"));
        assert_eq!(resolved[&6], assigned(&["b"], "reviewer"));
        assert_eq!(resolved[&8], assigned(&["c"], "reviewer"));
    }

    #[test]
    fn concurrent_reviewers_keep_their_earlier_request_after_both_depart() {
        let thread = [
            post(1, "a", "a joined as reviewer", None),
            post(2, "b", "b joined as reviewer", None),
            post(3, "owner", "both", Some("@reviewer")),
            post(4, "a", "a released every role (left)", None),
            post(5, "owner", "remaining", Some("@reviewer")),
            post(6, "b", "b released every role (left)", None),
        ];
        let resolved = resolve(&thread, &[]);
        assert_eq!(resolved[&3], assigned(&["a", "b"], "reviewer"));
        assert_eq!(resolved[&5], assigned(&["b"], "reviewer"));
    }

    #[test]
    fn an_executor_handoff_is_exclusive_and_imported_roles_observe_since() {
        let thread = [
            post(1, "a", "a claimed it", None),
            post(2, "b", "b claimed it, handed over from a (lost)", None),
            post(3, "owner", "continue", Some("@executor")),
            post(4, "owner", "review", Some("@reviewer")),
        ];
        let roles = [Role {
            handle: "imported".into(),
            role: "reviewer".into(),
            since: 2.0,
            expires: None,
            health: super::super::super::Health::Live,
        }];
        let resolved = resolve(&thread, &roles);
        assert_eq!(resolved[&3], assigned(&["b"], "executor"));
        assert!(resolved[&4].is_empty());
        let mut later = thread[3].clone();
        later.ts = 2.0;
        assert_eq!(
            resolve(&[later], &roles)[&4],
            assigned(&["imported"], "reviewer")
        );
        let mut direct = post(5, "owner", "direct", Some("imported"));
        assert!(!resolve(&[direct.clone()], &roles).contains_key(&5));
        direct.ts = 2.0;
        assert_eq!(
            resolve(&[direct], &roles)[&5],
            assigned(&["imported"], "reviewer")
        );
    }

    #[test]
    fn direct_requests_retain_the_recipients_role_after_departure_and_replacement() {
        let thread = [
            post(1, "a", "a joined as reviewer", None),
            post(2, "owner", "finish", Some("a")),
            post(3, "a", "a released every role (left)", None),
            post(4, "b", "b joined as reviewer", None),
            post(5, "owner", "old context", Some("a")),
            post(6, "owner", "unknown", Some("unassigned")),
        ];
        let resolved = resolve(&thread, &[]);
        assert_eq!(resolved[&2], assigned(&["a"], "reviewer"));
        assert_eq!(resolved[&5], assigned(&["a"], "reviewer"));
        assert!(!resolved.contains_key(&6));
    }

    #[test]
    fn recovery_uses_the_latest_executor_even_after_departure_without_reassigning_posts() {
        let mut thread = vec![
            post(1, "a", "a claimed it", None),
            post(2, "owner", "old", Some("@executor")),
            post(3, "b", "b claimed it, handed over from a (lost)", None),
            post(4, "b", "b released every role (left)", None),
            post(5, "reviewer", "reviewer joined as reviewer", None),
        ];
        thread.reverse();
        assert_eq!(executor(&thread, &[]).as_deref(), Some("b"));
        assert_eq!(resolve(&thread, &[])[&2], assigned(&["a"], "executor"));
        let current = [Role {
            handle: "imported".into(),
            role: "executor".into(),
            since: 2.0,
            expires: None,
            health: super::super::super::Health::Live,
        }];
        assert_eq!(executor(&thread, &current).as_deref(), Some("imported"));
        assert!(executor(&[], &[]).is_none());
    }
}
