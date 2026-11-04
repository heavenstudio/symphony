//! One definition of pending addressed work for PULL, conversation recovery and pane retention.

use super::{Error, Failure, Health, Message, Store, Task};
use crate::rows::MessageRow;
use crate::rows::TaskSessionRow;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[path = "unanswered_recipients.rs"]
mod recipients;

const UNANSWERED_SECONDS: f64 = 30.0 * 60.0;

// Actor-authored state changes answer work too. Other system records (launch, delivery, release)
// do not. Role acquisition also puts older role-addressed posts before the new holder's work.
const ANSWER_SQL: &str = "(kind IN ['msg', 'verdict'] OR (kind = 'system' AND
    (string::starts_with(text, sender + ' moved it ')
    OR string::starts_with(text, sender + ' claimed it')
    OR text = sender + ' joined as reviewer'
    OR text = sender + ' joined as executor')))";

#[derive(Clone, Debug)]
pub(crate) struct Pending {
    pub slug: String,
    pub handle: String,
    pub role: String,
    pub post: Message,
    pub launch: Option<TaskSessionRow>,
    pub health: Health,
    seen: f64,
    pub status: String,
}

impl Pending {
    pub fn overdue(&self, now: f64) -> bool {
        let applicable = match self.role.as_str() {
            "executor" => matches!(self.status.as_str(), "in_progress" | "blocked" | "review"),
            "reviewer" => self.status == "review",
            _ => false,
        };
        applicable
            && now - self.post.ts >= UNANSWERED_SECONDS
            && (matches!(self.health, Health::Lost | Health::Left)
                || self.launch.as_ref().is_some_and(|s| s.closed.is_some()))
    }

    pub fn command(&self) -> String {
        if let Some(launch) = self.launch.as_ref().filter(|s| restorable(s)) {
            format!("just tasks restore {} {}", launch.slug, self.handle)
        } else {
            format!("just tasks launch {} --role {}", self.slug, self.role)
        }
    }
}

pub(crate) fn restorable(launch: &TaskSessionRow) -> bool {
    !launch.claude_session.is_empty() && launch.claude_session != "codex:"
}

/// Review instructions launch on the child but review the parent. On the child itself the same
/// agent executes the instruction, as `launch::take_up` records its reservation there.
fn recorded_role<'a>(
    launch: &'a TaskSessionRow,
    task: &Task,
    tasks: &BTreeMap<&str, &Task>,
) -> Option<&'a str> {
    if launch.slug == task.slug {
        return Some(
            if launch.role == "reviewer" && task.instruction && task.parent.is_some() {
                "executor"
            } else {
                &launch.role
            },
        );
    }
    (launch.role == "reviewer"
        && tasks.get(launch.slug.as_str()).is_some_and(|child| {
            child.instruction && child.parent.as_deref() == Some(task.slug.as_str())
        }))
    .then_some("reviewer")
}

/// A post since this role began, with no later message, verdict or actor-authored action on its
/// thread. Only the actions selected by ANSWER_SQL become `activity`; other system records and
/// inbox acknowledgements are not answers. Sequence ids also order equal clocks.
pub(crate) fn unanswered_post(
    post: &Message,
    thread: &[Message],
    handle: &str,
    role: &str,
    since: f64,
) -> bool {
    matches!(post.kind.as_str(), "msg" | "verdict")
        && post.ts >= since
        && post.sender != handle
        && post.recipient.as_deref().is_some_and(|to| {
            to == handle || (!role.is_empty() && to.strip_prefix('@') == Some(role))
        })
        && !thread.iter().any(|reply| {
            reply.sender == handle
                && matches!(reply.kind.as_str(), "msg" | "verdict" | "activity")
                && reply.id > post.id
        })
}

impl Store {
    /// Pending posts, including direct handle posts on finished tasks (which still keep a pane).
    /// Role aliases retain their recipients from the post's thread prefix, including departed
    /// agents. A later release or replacement cannot assign an old post to another conversation.
    pub(crate) fn pending_posts(&self, tasks: &[Task]) -> Result<Vec<Pending>, Error> {
        let launches = self.all_task_sessions()?;
        let recorded_reviewers: BTreeSet<_> = launches
            .iter()
            .filter(|launch| launch.role == "reviewer")
            .map(|launch| launch.handle.as_str())
            .collect();
        let confirmed = self.confirmed_callbacks()?;
        let board = self.board()?;
        let by_slug: BTreeMap<_, _> = board.tasks.iter().map(|t| (t.slug.as_str(), t)).collect();
        // The raw session list has heartbeat health only. Board health also applies native
        // presence and its Unknown grace, so a quiet but confirmed agent is never recovered.
        let sessions: BTreeMap<_, _> = board
            .sessions
            .into_iter()
            .map(|s| {
                let health = if confirmed.contains(&s.row.handle) {
                    Health::Live
                } else {
                    s.health
                };
                (s.row.handle, (health, s.row.last_seen))
            })
            .collect();
        // Only metadata crosses the boundary, including the actor-authored state actions above.
        let rows: Vec<Vec<MessageRow>> = self
            .transact(
                &format!(
                    "RETURN $slugs.map(|$slug| (SELECT sid, slug, sender, recipient, kind, ts,
IF kind = 'system' {{ text }} ELSE {{ '' }} AS text
FROM message:[$slug, NONE]..[$slug, ..] WHERE {ANSWER_SQL}
OR (kind = 'system' AND string::starts_with(text, sender + ' released '))));"
                ),
                json!({"slugs": tasks.iter().map(|t| &t.slug).collect::<Vec<_>>()}),
            )
            .map_err(recovery_error)?;
        let mut pending = Vec::new();
        for (task, rows) in tasks.iter().zip(rows) {
            let mut thread: Vec<_> = rows.into_iter().map(super::message).collect();
            let recipients = recipients::resolve(&thread, &task.roles);
            for post in &mut thread {
                if post.kind == "system"
                    && !post.text.starts_with(&format!("{} released ", post.sender))
                {
                    post.kind = "activity".into();
                }
            }
            let mut holders: BTreeSet<(String, String)> = task
                .roles
                .iter()
                .filter(|r| matches!(r.role.as_str(), "executor" | "reviewer"))
                .map(|r| (r.handle.clone(), r.role.clone()))
                .collect();
            for launch in &launches {
                if let Some(role) = recorded_role(launch, task, &by_slug) {
                    holders.insert((launch.handle.clone(), role.into()));
                }
            }
            // A named review target may differ from the instruction's parent. Its role history
            // still binds the post after departure, when today's role rows have disappeared.
            for post in thread
                .iter()
                .filter(|post| post.recipient.as_deref() == Some("@reviewer"))
            {
                if let Some(handles) = recipients.get(&post.id) {
                    holders.extend(
                        handles
                            .iter()
                            .filter(|h| recorded_reviewers.contains(h.as_str()))
                            .map(|handle| (handle.clone(), "reviewer".into())),
                    );
                }
            }
            for to in thread.iter().filter_map(|m| m.recipient.as_ref()) {
                if sessions.contains_key(to) && !holders.iter().any(|(h, _)| h == to) {
                    holders.insert((to.clone(), String::new()));
                }
            }
            let mut seen = BTreeSet::new();
            for (handle, role) in holders {
                let since = task
                    .roles
                    .iter()
                    .find(|r| r.handle == handle && r.role == role)
                    .map_or(f64::NEG_INFINITY, |r| r.since);
                let last_reply = thread
                    .iter()
                    .filter(|post| {
                        post.sender == handle
                            && matches!(post.kind.as_str(), "msg" | "verdict" | "activity")
                    })
                    .max_by_key(|post| post.id)
                    .cloned();
                for post in &thread {
                    let alias = if recipients
                        .get(&post.id)
                        .is_some_and(|to| to.contains(&handle))
                    {
                        role.as_str()
                    } else {
                        ""
                    };
                    if unanswered_post(post, last_reply.as_slice(), &handle, alias, since)
                        && seen.insert((handle.clone(), post.id))
                    {
                        pending.push(Pending {
                            slug: task.slug.clone(),
                            handle: handle.clone(),
                            role: role.clone(),
                            post: post.clone(),
                            launch: launches
                                .iter()
                                .rev()
                                .find(|s| {
                                    s.handle == handle && recorded_role(s, task, &by_slug).is_some()
                                })
                                .or_else(|| {
                                    // A review instruction can name a target other than its parent.
                                    if role == "reviewer" {
                                        launches
                                            .iter()
                                            .rev()
                                            .find(|s| s.handle == handle && s.role == "reviewer")
                                    } else {
                                        None
                                    }
                                })
                                .cloned(),
                            health: sessions.get(&handle).map_or(Health::Left, |s| s.0),
                            seen: sessions.get(&handle).map_or(0.0, |s| s.1),
                            status: task.status.clone(),
                        });
                    }
                }
            }
        }
        Ok(pending)
    }

    /// Reserve the sole automatic resume attempt for this handle/post, across restarts and sweeps.
    /// Reserve before starting a process: an interruption may leave it flagged, never duplicated.
    /// Role recipients come from the immutable thread prefix; later holders cannot reassign them.
    pub(crate) fn claim_unanswered_restore(&self, pending: &Pending) -> Result<bool, Error> {
        self.transact(
            &format!("LET $key = type::record('unanswered_restore', [$slug, $handle, $post]);
IF record::exists($key) {{ RETURN false; }};
IF type::record('session', $handle).last_seen != $seen {{ RETURN false; }};
IF array::len(SELECT VALUE sid FROM message:[$slug, NONE]..[$slug, ..]
    WHERE sender = $handle AND {ANSWER_SQL} AND sid > $post) > 0 {{ RETURN false; }};
LET $status = type::record('task', $slug).status;
IF ($role = 'executor' AND $status NOT IN ['in_progress', 'blocked', 'review'])
    OR ($role = 'reviewer' AND $status != 'review') {{ RETURN false; }};
CREATE $key CONTENT {{ slug: $slug, handle: $handle, post: $post }} RETURN NONE;
RETURN true;"),
            json!({"slug": pending.slug, "handle": pending.handle, "post": pending.post.id, "seen":pending.seen, "role":pending.role}),
        )
        .map_err(recovery_error)
    }

    pub(crate) fn record_unanswered_restore(&self, slug: &str, text: &str) -> Result<(), Error> {
        let body = format!(
            "{}\nRETURN NONE;",
            super::logged("$slug", "'board'", "'msg'", "$text", "'@orchestrator'")
        );
        self.transact::<serde_json::Value>(
            &body,
            json!({"slug":slug, "text":text, "now":self.now()}),
        )
        .map(|_| ())
        .map_err(recovery_error)
    }
}

fn recovery_error(error: Failure) -> Error {
    match error {
        Failure::Stop(error) => error,
        Failure::Again => Error::Db(
            "board changed during unanswered recovery; automatic attempt not repeated".into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(id: i64, sender: &str, recipient: Option<&str>, kind: &str) -> Message {
        Message {
            id,
            slug: "tools/a".into(),
            sender: sender.into(),
            recipient: recipient.map(str::to_string),
            kind: kind.into(),
            text: String::new(),
            ts: 1.0,
        }
    }

    #[test]
    fn only_a_later_post_by_the_recipient_answers_an_addressed_post() {
        for to in ["worker", "@executor"] {
            let addressed = post(2, "owner", Some(to), "msg");
            let mut thread = vec![post(1, "worker", None, "msg"), addressed.clone()];
            for irrelevant in [
                post(3, "other", None, "msg"),
                post(4, "worker", None, "system"),
            ] {
                thread.push(irrelevant);
            }
            assert!(unanswered_post(
                &addressed, &thread, "worker", "executor", 0.0
            ));
            assert!(!unanswered_post(
                &addressed, &thread, "worker", "executor", 2.0
            ));
            for kind in ["msg", "verdict", "activity"] {
                let mut answered = thread.clone();
                answered.push(post(5, "worker", None, kind));
                assert!(!unanswered_post(
                    &addressed, &answered, "worker", "executor", 0.0
                ));
            }
        }
        for (sender, to, kind) in [
            ("owner", None, "msg"),
            ("owner", Some("other"), "msg"),
            ("owner", Some("@reviewer"), "msg"),
            ("worker", Some("worker"), "msg"),
            ("owner", Some("worker"), "system"),
        ] {
            let ignored = post(2, sender, to, kind);
            assert!(!unanswered_post(
                &ignored,
                std::slice::from_ref(&ignored),
                "worker",
                "executor",
                0.0
            ));
        }
        let verdict = post(2, "reviewer", Some("@executor"), "verdict");
        assert!(unanswered_post(
            &verdict,
            std::slice::from_ref(&verdict),
            "worker",
            "executor",
            0.0
        ));
    }
}
