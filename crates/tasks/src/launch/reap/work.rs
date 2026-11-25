//! Pending work that keeps an idle pane, including descendant sessions and re-review: an addressed
//! post waiting for its answer keeps it at once, a role or live reservation on open work until it
//! has sat idle at its prompt for [`super::IDLE_HOUR`].

use crate::health::FINISHED;
use crate::store::unanswered::Asked;
use crate::{Board, Error, Message, Store};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// How the reason a role holder's terminal closed for starts ([`Hold::closing`]).
pub(super) const HOUR: &str = "idle an hour";

/// Landing creates `review/BRANCH`, then `review-N/BRANCH` for N >= 2.
fn reviewed_branch(slug: &str) -> Option<&str> {
    let (prefix, branch) = slug.split_once('/')?;
    if prefix == "review" {
        return Some(branch);
    }
    let number = prefix.strip_prefix("review-")?;
    let ordinal = number.parse::<usize>().ok()?;
    (ordinal >= 2 && number == ordinal.to_string()).then_some(branch)
}

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
    if branch != slug && reviewed_branch(slug) != Some(branch) {
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

/// Why a handle's terminal is held: what it, or a session under it (`through`, a subagent), waits
/// on. The variants go strongest first.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Hold {
    /// An addressed post waits for its answer: the terminal is kept at once.
    Post {
        slug: String,
        post: i64,
        through: Option<String>,
    },
    /// A role on open work: an executor of a task `in_progress`, `blocked` or in `review`, a
    /// reviewer of one in `review` or awaiting its re-review, or a live reservation on an unfinished
    /// task. The terminal is kept until it has sat idle at its prompt for [`super::IDLE_HOUR`].
    Role {
        slug: String,
        role: String,
        status: String,
        through: Option<String>,
    },
}

impl Hold {
    /// The same hold, for a session above `holder`, which holds it itself.
    fn through(&self, holder: &str) -> Hold {
        let mut above = self.clone();
        let (Hold::Post { through, .. } | Hold::Role { through, .. }) = &mut above;
        through.get_or_insert_with(|| holder.to_string());
        above
    }

    /// Why the sweep keeps the terminal for it.
    pub(super) fn why(&self) -> String {
        match self {
            Hold::Post {
                slug,
                post,
                through,
            } => {
                let to = through
                    .as_ref()
                    .map_or(String::new(), |holder| format!(" to its session {holder}"));
                format!("an addressed post #{post}{to} on {slug} waits for its answer")
            }
            Hold::Role {
                slug,
                role,
                status,
                through,
            } => {
                let who = through.as_ref().map_or("it is".to_string(), |holder| {
                    format!("its session {holder} is")
                });
                let standing = standing(role, slug, status);
                format!("{who} {standing}: it closes once idle an hour")
            }
        }
    }

    /// Why the sweep closes the terminal of a role it held once idle an hour, and what brings it
    /// back besides the restore line said with it: a reviewer launch for the task's next tip (which
    /// resumes the task's earlier reviewer), a changes verdict to the executor of work in review,
    /// or a post addressed to it (the unanswered post's restore). None for a post, which never
    /// closes.
    pub(super) fn closing(&self) -> Option<String> {
        let Hold::Role {
            slug,
            role,
            status,
            through,
        } = self
        else {
            return None;
        };
        let back = if role == "reviewer" {
            "a reviewer launch for its next tip resumes it, a post to it restores it"
        } else if status == "review" {
            "a changes verdict or a post to it restores it"
        } else {
            "a post to it restores it"
        };
        let standing = standing(role, slug, status);
        Some(match through {
            Some(holder) => format!("{HOUR} as the parent of {holder}, {standing}; {back}"),
            None => format!("{HOUR} as {standing}; {back}"),
        })
    }

    pub(super) fn is_post(&self) -> bool {
        matches!(self, Hold::Post { .. })
    }

    /// What names this hold in an idle mark: `ROLE SLUG STATUS`, then `via HOLDER` for one held
    /// through a session under it (`#POST` for a post, which marks nothing).
    pub(super) fn mark(&self) -> String {
        match self {
            Hold::Post { post, .. } => format!("#{post}"),
            Hold::Role {
                slug,
                role,
                status,
                through,
            } => {
                let via = through
                    .as_ref()
                    .map_or(String::new(), |holder| format!(" via {holder}"));
                format!("{role} {slug} {status}{via}")
            }
        }
    }
}

/// `ROLE of SLUG, STATUS`, the status as a reader says it.
fn standing(role: &str, slug: &str, status: &str) -> String {
    let status = match (role, status) {
        ("reviewer", "in_progress") => "awaiting its re-review",
        (_, "in_progress") => "in progress",
        (_, "review") => "in review",
        (_, other) => other,
    };
    if role == "reserved" {
        format!("holder of the reservation on {slug}, {status}")
    } else {
        format!("{role} of {slug}, {status}")
    }
}

/// What holds a handle's terminal, itself or through a session under it (a subagent): an addressed
/// post waiting for its answer, else a role on open work ([`Hold`]). A handle's own hold goes
/// before one through a session under it, a post before a role, and among equals the first by
/// task. A departed middle session still links its live descendants to their ancestors; leaving
/// releases its own roles. One look at the board, and for each handle a sweep asks of
/// ([`Holds::of`]) the posts and roles of it and the sessions under it alone, never every
/// handle's.
pub(super) struct Holds<'a> {
    store: &'a Store,
    board: Board,
    /// Each session's parent session.
    parent: HashMap<String, String>,
    /// What each handle asked of so far is held by.
    found: HashMap<String, Option<Hold>>,
}

impl<'a> Holds<'a> {
    pub(super) fn read(store: &'a Store) -> Result<Holds<'a>, Error> {
        let board = store.board()?;
        let parent = (board.sessions.iter())
            .filter_map(|s| Some((s.row.handle.clone(), s.row.parent.clone()?)))
            .collect();
        Ok(Holds {
            store,
            board,
            parent,
            found: HashMap::new(),
        })
    }

    /// What holds `handle`'s terminal, if anything does.
    pub(super) fn of(&mut self, handle: &str) -> Result<Option<Hold>, Error> {
        if let Some(found) = self.found.get(handle) {
            return Ok(found.clone());
        }
        let mut asked: BTreeSet<String> = BTreeSet::new();
        asked.extend(self.under(handle));
        asked.insert(handle.to_string());
        let posts: Vec<(Hold, String)> = (self.store)
            .pending_posts(&self.board, Asked::Handles(&asked))?
            .into_iter()
            .map(|p| {
                let hold = Hold::Post {
                    slug: p.slug,
                    post: p.post.id,
                    through: None,
                };
                (hold, p.handle)
            })
            .collect();
        let roles = self.roles(&asked)?;
        let first = |holds: &[(Hold, String)], own: bool| {
            (holds.iter())
                .filter(|(_, holder)| (holder == handle) == own)
                .min()
                .map(|(hold, holder)| match own {
                    true => hold.clone(),
                    false => hold.through(holder),
                })
        };
        let hold = first(&posts, true)
            .or_else(|| first(&posts, false))
            .or_else(|| first(&roles, true))
            .or_else(|| first(&roles, false));
        self.found.insert(handle.to_string(), hold.clone());
        Ok(hold)
    }

    /// The sessions under `handle`: those whose chain of parents reaches it, once round a loop.
    fn under(&self, handle: &str) -> Vec<String> {
        let reaches = |session: &str| {
            let mut seen = HashSet::from([session]);
            let mut at = self.parent.get(session);
            while let Some(here) = at.filter(|here| seen.insert(here.as_str())) {
                if here == handle {
                    return true;
                }
                at = self.parent.get(here);
            }
            false
        };
        (self.parent.keys())
            .filter(|session| *session != handle && reaches(session))
            .cloned()
            .collect()
    }

    /// The roles of `asked` on open work, each with its holder: an executor of a task in progress,
    /// blocked or in review, a reviewer of one in review or awaiting its re-review, a live
    /// reservation on an unfinished task.
    fn roles(&self, asked: &BTreeSet<String>) -> Result<Vec<(Hold, String)>, Error> {
        let rereviewed: Vec<String> = (self.board.tasks.iter())
            .filter(|t| t.status == "in_progress")
            .filter(|t| (t.roles.iter()).any(|r| r.role == "reviewer" && asked.contains(&r.handle)))
            .map(|t| t.slug.clone())
            .collect();
        let threads = match rereviewed.is_empty() {
            true => BTreeMap::new(),
            false => self.store.threads(&rereviewed)?,
        };
        let mut roles = Vec::new();
        for task in &self.board.tasks {
            for role in task.roles.iter().filter(|r| asked.contains(&r.handle)) {
                let live = match role.role.as_str() {
                    // Store::tasks omits lapsed reservations; do not derive their expiry again here.
                    "reserved" => !FINISHED.contains(&task.status.as_str()),
                    "executor" => {
                        matches!(task.status.as_str(), "in_progress" | "blocked" | "review")
                    }
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
                    let hold = Hold::Role {
                        slug: task.slug.clone(),
                        role: role.role.clone(),
                        status: task.status.clone(),
                        through: None,
                    };
                    roles.push((hold, role.handle.clone()));
                }
            }
        }
        Ok(roles)
    }
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
        for slug in [
            "sim/jump",
            "review/sim/jump",
            "review-2/sim/jump",
            "review-3/sim/jump",
            "review-24/sim/jump",
        ] {
            assert_eq!(review_decision(&original, "reviewer", slug), Some(true));
        }
        for slug in [
            "sim/other",
            "review-2/sim/other",
            "reviewer/sim/jump",
            "review-/sim/jump",
            "review-0/sim/jump",
            "review-1/sim/jump",
            "review-02/sim/jump",
            "review-+2/sim/jump",
            "review-other/sim/jump",
        ] {
            assert_eq!(review_decision(&original, "reviewer", slug), None);
        }
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
