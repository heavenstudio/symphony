//! Resolve role aliases from the thread prefix at each post, never from today's role holders.

use super::super::{Message, Role};
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
pub(super) fn resolve(thread: &[Message], current: &[Role]) -> BTreeMap<i64, BTreeSet<String>> {
    let acquired: BTreeSet<_> = thread
        .iter()
        .filter_map(|post| match change(post) {
            Some(Change::Acquire(role)) => Some((post.sender.as_str(), role)),
            _ => None,
        })
        .collect();
    // Imported current roles may predate the retained journal. Their `since` remains evidence;
    // a launch record alone cannot assign all old conversations to a role-addressed post.
    let imported: Vec<_> = current
        .iter()
        .filter(|role| !acquired.contains(&(role.handle.as_str(), role.role.as_str())))
        .collect();
    let mut ordered: Vec<_> = thread.iter().collect();
    ordered.sort_by_key(|post| post.id);
    let mut roles: BTreeMap<&str, Holders> = BTreeMap::new();
    let mut resolved = BTreeMap::new();
    for post in ordered {
        match change(post) {
            Some(Change::Acquire(role)) => roles
                .entry(role)
                .or_default()
                .acquire(&post.sender, role == "executor"),
            Some(Change::Release(role)) => {
                for (held, holders) in &mut roles {
                    if role.is_none_or(|released| released == *held) {
                        holders.release(&post.sender);
                    }
                }
            }
            None => {}
        }
        let Some(role @ ("executor" | "reviewer")) = post
            .recipient
            .as_deref()
            .and_then(|to| to.strip_prefix('@'))
        else {
            continue;
        };
        let mut recipients = roles
            .get(role)
            .map_or_else(BTreeSet::new, Holders::recipients);
        recipients.extend(
            imported
                .iter()
                .filter(|held| held.role == role && held.since <= post.ts)
                .map(|held| held.handle.clone()),
        );
        resolved.insert(post.id, recipients);
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(resolved[&4], BTreeSet::from(["b".into()]));
        assert_eq!(resolved[&6], BTreeSet::from(["b".into()]));
        assert_eq!(resolved[&8], BTreeSet::from(["c".into()]));
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
        assert_eq!(resolved[&3], BTreeSet::from(["a".into(), "b".into()]));
        assert_eq!(resolved[&5], BTreeSet::from(["b".into()]));
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
        assert_eq!(resolved[&3], BTreeSet::from(["b".into()]));
        assert!(resolved[&4].is_empty());
        let mut later = thread[3].clone();
        later.ts = 2.0;
        assert_eq!(
            resolve(&[later], &roles)[&4],
            BTreeSet::from(["imported".into()])
        );
    }
}
