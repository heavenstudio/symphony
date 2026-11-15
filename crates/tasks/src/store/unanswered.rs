//! One definition of pending addressed work for PULL, conversation recovery and pane retention,
//! read in proportion to what it is asked about ([`Asked`]): the threads of the tasks or handles
//! asked, by record range or index, never every thread of the board nor a recipient's every record
//! after each of its posts.

use super::work::{Tree, above};
use super::{Board, Error, Failure, Health, Message, Store, Task};
use crate::rows::MessageRow;
use crate::rows::TaskSessionRow;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

#[path = "unanswered_answers.rs"]
mod answers;
#[path = "unanswered_recipients.rs"]
mod recipients;

const UNANSWERED_SECONDS: f64 = 30.0 * 60.0;

/// The longest one read of the lookup waits for the board in a board command, sent once
/// ([`Store::looking_up_within`]), unless [`LOOKUP_WAIT_VAR`] says. Each caller does without the
/// answer: PULL leaves its unanswered posts out and says why, recovery tries again at its next
/// sweep, and the reap keeps a pane it cannot judge.
pub const LOOKUP_WAIT: Duration = Duration::from_secs(10);

/// The setting of [`LOOKUP_WAIT`] in seconds, read only when set (`Env::seconds`): a test gives the
/// lookup a backstop past its client's own wait, or makes it run out at once.
pub const LOOKUP_WAIT_VAR: &str = "TASKS_UNANSWERED_WAIT";

/// The statuses an executor's post can be overdue in; a reviewer's only in review.
const OPEN: [&str; 3] = ["in_progress", "blocked", "review"];

/// What a lookup of pending posts asks about, which is all it reads.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Asked<'a> {
    /// The posts on these tasks overdue at this time ([`Pending::overdue`]), for PULL and
    /// recovery: only the threads of open ones are read, and the threads below a task only for a
    /// post overdue on it.
    Overdue(&'a [Task], f64),
    /// Every post these handles owe on any task, for the reap: the threads that address one of
    /// them, hold a system record of one (its role history) or name one in a role.
    Handles(&'a BTreeSet<String>),
}

// Deliberate state changes answer work too. Automatic departure, launch and delivery records do
// not. Role acquisition also puts older role-addressed posts before the new holder's work.
const ANSWER_SQL: &str = "(kind IN ['msg', 'verdict'] OR (kind = 'system' AND
    (string::starts_with(text, sender + ' moved it ')
    OR string::starts_with(text, sender + ' claimed it')
    OR (string::starts_with(text, sender + ' released ') AND string::contains(text, ' (released)'))
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
        self.due(now)
            && (matches!(self.health, Health::Lost | Health::Left)
                || self.launch.as_ref().is_some_and(|s| s.closed.is_some()))
    }

    /// Its role answers for its task in this status and its post is half an hour old: overdue
    /// once its agent is gone or its launch closed as well.
    fn due(&self, now: f64) -> bool {
        let applicable = match self.role.as_str() {
            "executor" => OPEN.contains(&self.status.as_str()),
            "reviewer" => self.status == "review",
            _ => false,
        };
        applicable && now - self.post.ts >= UNANSWERED_SECONDS
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

/// A reviewer's record on a review instruction, a child a launch starts no reviewer on (it sends
/// the reviewer to the task itself), reviews the parent. On the child itself the same agent
/// executes the instruction, holding its reservation there.
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

// Each read below puts `then` after every SELECT it makes inside its closures: nothing for the
// lookup, ` EXPLAIN` for the test of the plans those closures get (the legacy planner's, which
// leaves an index or a range for a table scan where a top-level statement would not).

/// Each handle's threads that address it or hold a system record it made, by the recipient and
/// sender indexes: one equality on each index's first columns.
pub(super) fn naming_sql(then: &str) -> String {
    format!(
        "RETURN $handles.map(|$handle| array::union(
(SELECT VALUE slug FROM message WITH INDEX message_recipient WHERE recipient = $handle{then}),
(SELECT VALUE slug FROM message WITH INDEX message_sender WHERE sender = $handle AND kind = 'system'{then})));"
    )
}

/// These handles' reviewer launches, as the board lists launches. Launches have no handle index:
/// this reads the launch table, for the rare reviewer post no launch on its task explains.
const REVIEWER_LAUNCHES: &str = "RETURN SELECT * OMIT id FROM task_session
WHERE handle IN $handles AND role = 'reviewer' ORDER BY started, slug, handle;";

/// The asked threads and launches ([`Threads`]), by record range: a thread's messages are
/// `message:[slug, sid]` and its launches `task_session:[slug, handle]`.
pub(super) fn threads_sql(then: &str) -> String {
    format!(
        "RETURN {{
threads: $slugs.map(|$slug| (SELECT sid, slug, sender, recipient, kind, ts,
IF kind = 'system' {{ text }} ELSE {{ '' }} AS text
FROM message:[$slug, NONE]..[$slug, ..] WHERE {ANSWER_SQL}
OR (kind = 'system' AND string::starts_with(text, sender + ' released ')){then})),
launches: array::flatten($launched.map(|$slug| (SELECT * OMIT id FROM task_session:[$slug, NONE]..[$slug, ..]{then})))
}};"
    )
}

/// For each ask (a recipient, the sequence it asks after and the threads below a task), the
/// sequence ids of the recipient's answers there after it, by record range from that sequence on.
pub(super) fn below_sql(then: &str) -> String {
    format!(
        "RETURN $asks.map(|$ask| {{ LET $handle = $ask.handle; LET $after = $ask.after;
RETURN array::flatten($ask.below.map(|$slug| (SELECT VALUE sid FROM message:[$slug, $after]..[$slug, ..]
WHERE sender = $handle AND sid > $after AND {ANSWER_SQL}{then}))); }});"
    )
}

/// The asked tasks' threads (only the metadata of their posts, answers and releases), and the
/// launches on those tasks and on the instructions under them, by when they started.
#[derive(Deserialize)]
struct Threads {
    threads: Vec<Vec<MessageRow>>,
    launches: Vec<TaskSessionRow>,
}

/// Each handle's health on the board and when it was last seen.
type Seen<'a> = BTreeMap<&'a str, (Health, f64)>;

/// The posts on `task`'s thread (`rows`, as [`Store::threads_and_launches`] reads it) that its
/// holders owe, each recipient's once: its holders are its executors and reviewers, the launches on
/// it or under it (`launches`), its role history and the sessions it addresses.
fn owed(
    task: &Task,
    rows: Vec<MessageRow>,
    launches: &[TaskSessionRow],
    by_slug: &BTreeMap<&str, &Task>,
    sessions: &Seen<'_>,
    pending: &mut Vec<Pending>,
) {
    let mut thread: Vec<_> = rows.into_iter().map(super::message).collect();
    let recipients = recipients::resolve(&thread, &task.roles);
    for post in &mut thread {
        if post.kind == "system"
            && (!post.text.starts_with(&format!("{} released ", post.sender))
                || post.text.contains(" (released)"))
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
    for launch in launches {
        if let Some(role) = recorded_role(launch, task, by_slug) {
            holders.insert((launch.handle.clone(), role.into()));
        }
    }
    // Role history remains evidence after departure even without a launch to restore.
    for receivers in recipients.values() {
        holders.extend(
            receivers
                .iter()
                .map(|(handle, role)| (handle.clone(), role.clone())),
        );
    }
    for to in thread.iter().filter_map(|m| m.recipient.as_ref()) {
        if sessions.contains_key(to.as_str()) && !holders.iter().any(|(h, _)| h == to) {
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
            let addressed_role = recipients.get(&post.id).and_then(|to| to.get(&handle));
            if addressed_role.is_some_and(|assigned| assigned != &role) {
                continue;
            }
            let alias = addressed_role.map_or("", String::as_str);
            if unanswered_post(post, last_reply.as_slice(), &handle, alias, since)
                && seen.insert((handle.clone(), post.id))
            {
                let session = sessions.get(handle.as_str());
                pending.push(Pending {
                    slug: task.slug.clone(),
                    handle: handle.clone(),
                    role: role.clone(),
                    post: post.clone(),
                    // A reviewer launched elsewhere is found later, for the posts still asked.
                    launch: launches
                        .iter()
                        .rev()
                        .find(|s| s.handle == handle && recorded_role(s, task, by_slug).is_some())
                        .cloned(),
                    health: session.map_or(Health::Left, |s| s.0),
                    seen: session.map_or(0.0, |s| s.1),
                    status: task.status.clone(),
                });
            }
        }
    }
}

impl Store {
    /// Pending posts of what `asked` names, including direct handle posts on finished tasks (which
    /// still keep a pane). `board` is the caller's look at the board: its tasks' tree and its
    /// sessions' health. Role aliases retain their recipients from the post's thread prefix,
    /// including departed agents. A later release or replacement cannot assign an old post to
    /// another conversation. Every read is bounded ([`LOOKUP_WAIT`]).
    pub(crate) fn pending_posts(
        &self,
        board: &Board,
        asked: Asked<'_>,
    ) -> Result<Vec<Pending>, Error> {
        let by_slug: BTreeMap<_, _> = board.tasks.iter().map(|t| (t.slug.as_str(), t)).collect();
        let tasks: Vec<&Task> = match asked {
            Asked::Overdue(tasks, _) => tasks
                .iter()
                .filter(|t| OPEN.contains(&t.status.as_str()))
                .collect(),
            Asked::Handles(handles) => {
                let named = self.threads_naming(handles)?;
                board
                    .tasks
                    .iter()
                    .filter(|t| {
                        named.contains(&t.slug)
                            || t.roles.iter().any(|r| handles.contains(&r.handle))
                    })
                    .collect()
            }
        };
        if tasks.is_empty() {
            return Ok(Vec::new());
        }
        let confirmed = self.confirmed_callbacks()?;
        // The raw session list has heartbeat health only. Board health also applies native
        // presence and its Unknown grace, so a quiet but confirmed agent is never recovered.
        let sessions: Seen<'_> = board
            .sessions
            .iter()
            .map(|s| {
                let health = if confirmed.contains(&s.row.handle) {
                    Health::Live
                } else {
                    s.health
                };
                (s.row.handle.as_str(), (health, s.row.last_seen))
            })
            .collect();
        let read = self.threads_and_launches(&tasks, board)?;
        let mut pending = Vec::new();
        for (task, rows) in tasks.iter().zip(read.threads) {
            owed(
                task,
                rows,
                &read.launches,
                &by_slug,
                &sessions,
                &mut pending,
            );
        }
        pending.retain(|p| match asked {
            Asked::Overdue(_, now) => p.due(now),
            Asked::Handles(handles) => handles.contains(&p.handle),
        });
        self.reviewer_launches(&mut pending)?;
        if let Asked::Overdue(_, now) = asked {
            pending.retain(|p| p.overdue(now));
        }
        self.unanswered_below(pending, board)
    }

    /// The threads that address one of `handles` or hold a system record one of them made (its
    /// claims, joins and releases: its role history), by the recipient and sender indexes.
    fn threads_naming(&self, handles: &BTreeSet<String>) -> Result<BTreeSet<String>, Error> {
        let named: Vec<Vec<String>> = self
            .bounded_read(&naming_sql(""), json!({"handles": handles}))
            .map_err(recovery_error)?;
        Ok(named.into_iter().flatten().collect())
    }

    /// `tasks`' threads and the launches on them and on the instructions under them, in one read
    /// of their record ranges: only the metadata of posts, deliberate actions and releases crosses
    /// the boundary.
    fn threads_and_launches(&self, tasks: &[&Task], board: &Board) -> Result<Threads, Error> {
        let slugs: BTreeSet<&str> = tasks.iter().map(|t| t.slug.as_str()).collect();
        let mut launched = slugs.clone();
        launched.extend(
            board
                .tasks
                .iter()
                .filter(|t| t.instruction && t.parent.as_deref().is_some_and(|p| slugs.contains(p)))
                .map(|t| t.slug.as_str()),
        );
        let in_order: Vec<&str> = tasks.iter().map(|t| t.slug.as_str()).collect();
        let mut read: Threads = self
            .bounded_read(
                &threads_sql(""),
                json!({"slugs": in_order, "launched": launched}),
            )
            .map_err(recovery_error)?;
        // As the board lists them, so the latest launch of a handle is its last.
        read.launches.sort_by(|a, b| {
            (a.started.total_cmp(&b.started))
                .then_with(|| a.slug.cmp(&b.slug))
                .then_with(|| a.handle.cmp(&b.handle))
        });
        Ok(read)
    }

    /// A review instruction can name a target other than its parent: a reviewer's post with no
    /// launch on its task or under it takes that handle's latest reviewer launch anywhere, read
    /// for those handles alone.
    fn reviewer_launches(&self, pending: &mut [Pending]) -> Result<(), Error> {
        let wants = |p: &Pending| p.role == "reviewer" && p.launch.is_none();
        let handles: BTreeSet<&str> = pending
            .iter()
            .filter(|p| wants(p))
            .map(|p| p.handle.as_str())
            .collect();
        if handles.is_empty() {
            return Ok(());
        }
        let launches: Vec<TaskSessionRow> = self
            .bounded_read(REVIEWER_LAUNCHES, json!({"handles": handles}))
            .map_err(recovery_error)?;
        for p in pending.iter_mut().filter(|p| wants(p)) {
            p.launch = launches
                .iter()
                .rev()
                .find(|s| s.handle == p.handle)
                .cloned();
        }
        Ok(())
    }

    /// The posts of `pending` their recipient has not answered on a task under the post's own,
    /// after it (an answer on its own thread is the thread check already made): the threads below
    /// each asked task read from its earliest post's sequence on, for its recipients alone.
    fn unanswered_below(
        &self,
        pending: Vec<Pending>,
        board: &Board,
    ) -> Result<Vec<Pending>, Error> {
        let tree: Tree = board
            .tasks
            .iter()
            .map(|t| (t.slug.clone(), t.parent.clone()))
            .collect();
        let asked: BTreeSet<&str> = pending.iter().map(|p| p.slug.as_str()).collect();
        let mut below: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for task in &board.tasks {
            for over in above(&tree, &task.slug) {
                if let Some(over) = asked.get(over).filter(|over| **over != task.slug) {
                    below.entry(over).or_default().push(&task.slug);
                }
            }
        }
        let mut after: BTreeMap<(&str, &str), i64> = BTreeMap::new();
        for p in pending
            .iter()
            .filter(|p| below.contains_key(p.slug.as_str()))
        {
            let first = after
                .entry((p.handle.as_str(), p.slug.as_str()))
                .or_insert(p.post.id);
            *first = (*first).min(p.post.id);
        }
        if after.is_empty() {
            return Ok(pending);
        }
        let asks: Vec<Value> = after
            .iter()
            .map(|((handle, slug), after)| {
                json!({"handle": handle, "after": after, "below": below[slug]})
            })
            .collect();
        let found: Vec<Vec<i64>> = self
            .bounded_read(&below_sql(""), json!({"asks": asks}))
            .map_err(recovery_error)?;
        let latest: BTreeMap<(String, String), i64> = after
            .keys()
            .zip(found)
            .filter_map(|((handle, slug), sids)| {
                Some((
                    (handle.to_string(), slug.to_string()),
                    sids.into_iter().max()?,
                ))
            })
            .collect();
        Ok(pending
            .into_iter()
            .filter(|p| {
                latest
                    .get(&(p.handle.clone(), p.slug.clone()))
                    .is_none_or(|last| *last <= p.post.id)
            })
            .collect())
    }

    /// Reserve the sole automatic resume attempt for this handle/post, across restarts and sweeps.
    /// Reserve before starting a process: an interruption may leave it flagged, never duplicated.
    /// Role recipients come from the immutable thread prefix; later holders cannot reassign them.
    pub(crate) fn claim_unanswered_restore(&self, pending: &Pending) -> Result<bool, Error> {
        let rows = answers::rows(self, &pending.handle, pending.post.id)?;
        let parents = answers::parents(self, &rows)?;
        let tree = parents.iter().cloned().collect();
        if answers::answered(&rows, &pending.slug, &tree) {
            return Ok(false);
        }
        self.reserve_unanswered_restore(pending, &rows, &parents)
    }

    /// Recheck the recipient suffix and every parent edge used in the answer decision in the
    /// reservation transaction. A changed observation waits for the next sweep, unreserved.
    fn reserve_unanswered_restore(
        &self,
        pending: &Pending,
        rows: &[answers::Answer],
        parents: &[(String, Option<String>)],
    ) -> Result<bool, Error> {
        self.transact(
            &format!("LET $key = type::record('unanswered_restore', [$slug, $handle, $post]);
IF record::exists($key) {{ RETURN false; }};
IF type::record('session', $handle).last_seen != $seen {{ RETURN false; }};
IF {} != $answers {{ RETURN false; }};
IF $parents.map(|$p| [$p[0], type::record('task', $p[0]).parent ?? NULL]) != $parents {{ RETURN false; }};
LET $status = type::record('task', $slug).status;
IF ($role = 'executor' AND $status NOT IN ['in_progress', 'blocked', 'review'])
    OR ($role = 'reviewer' AND $status != 'review') {{ RETURN false; }};
CREATE $key CONTENT {{ slug: $slug, handle: $handle, post: $post }} RETURN NONE;
RETURN true;", answers::read()),
            json!({"slug": pending.slug, "handle": pending.handle, "post": pending.post.id, "seen":pending.seen, "role":pending.role, "answers":rows, "parents":parents}),
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
#[path = "unanswered_scratch.rs"]
mod scratch;

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
