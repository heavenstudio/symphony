//! Threads: saying, inboxes and acknowledging them, what reaches the owner, each sender's last word,
//! and landings on the record.

use super::work::{OWNER, ROLES, Tree, above, no_task};
use super::{ACT, Error, Message, REFUSED, Store, acting, logged, message, refused, repr};
use crate::health::Health;
use crate::rows::MessageRow;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The kinds of message that reach an inbox; `system` is the record only.
pub const DELIVERED: [&str; 2] = ["msg", "verdict"];

/// The sender of the board's own addressed posts: no session's, so no recipient takes one for its
/// own and leaves it out of its inbox.
pub const BOARD: &str = "board";

/// What an inbox is worked out from: the session's roles, the tree, and the messages since it
/// connected that are not its own, that it has not acknowledged, and that could reach it (not
/// addressed to another handle or the owner). The acknowledged ids are read once rather than looked
/// up per message, which on the live board halves the read for a session days old.
const INBOX: &str = "
LET $me = (SELECT connected FROM ONLY type::record('session', $handle));
LET $acked = (SELECT VALUE message FROM delivery WHERE handle = $handle AND message > $since);
RETURN {
    orchestrators: (SELECT VALUE [slug, handle] FROM role WHERE role = 'orchestrator'),
    known: $me != NONE,
    roles: (SELECT slug, role FROM role WHERE handle = $handle),
    tree: (SELECT VALUE [slug, parent] FROM task),
    messages: IF $me = NONE { [] } ELSE {
        (SELECT * OMIT id FROM message WHERE sid > $since AND ts >= $me.connected AND kind IN $delivered AND sender != $handle
            AND (sender = 'owner' OR recipient = NONE OR recipient = $handle OR string::starts_with(recipient, '@') OR kind = 'verdict')
            AND sid NOTINSIDE $acked ORDER BY sid)
    },
};";

impl Store {
    /// Post to a task's thread. Only addressed messages and owner messages enter inboxes.
    pub fn say(
        &self,
        slug: &str,
        sender: &str,
        text: &str,
        to: Option<&str>,
    ) -> Result<Message, Error> {
        let text = text.trim();
        if text.is_empty() {
            return Err(refused("an empty message says nothing".into()));
        }
        let role = to.and_then(|to| to.strip_prefix('@'));
        let no_role = role.filter(|role| !ROLES.contains(role)).map(|role| {
            let roles: Vec<String> = ROLES.iter().map(|r| format!("@{r}")).collect();
            format!("{REFUSED}no role @{role}; use one of {}", roles.join(", "))
        });
        let handle = to.filter(|to| *to != OWNER && role.is_none());
        let act = if sender == OWNER { "" } else { ACT };
        let body = format!(
            "{act}
IF !record::exists(type::record('task', $slug)) {{ THROW $no_task }};
IF $no_role != NONE {{ THROW $no_role }};
IF $handle != NONE AND !record::exists(type::record('session', $handle)) {{ THROW $nobody }};
{}
RETURN [$sid, $ts];",
            logged("$slug", "$actor", "'msg'", "$text", "$to")
        );
        let now = (self.clock)();
        let vars = acting(
            sender,
            now,
            json!({
                "slug": slug, "text": text, "to": to, "handle": handle, "no_role": no_role,
                "no_task": format!("{REFUSED}{}", no_task(slug)),
                "nobody": handle.map(|h| format!("{REFUSED}no session {} to send to", repr(h))),
            }),
        );
        let (id, ts): (i64, f64) = self.retrying(|| self.transact(&body, vars.clone()))?;
        if role == Some("executor") {
            self.tell_if_unread(slug, None, &format!("#{id} from {sender} to @executor"));
        } else if let Some(to) = handle {
            self.tell_if_unread(slug, Some(to), &format!("#{id} from {sender} to {to}"));
        }
        Ok(Message {
            id,
            slug: slug.into(),
            sender: sender.into(),
            recipient: to.map(String::from),
            kind: "msg".into(),
            text: text.into(),
            ts,
        })
    }

    /// `what` (a post or a changes verdict to `slug`'s executor, or to `only` when that handle
    /// executes it) has no executor to read it now: none has a native callback, and each has its
    /// pane recorded closed or no live beat. The task's orchestrators hear it at once on the
    /// thread, with the restore line of each executor whose pane was closed. A post to a handle
    /// that does not execute `slug` is left alone. Never fails the write it follows: what went
    /// wrong goes to stderr.
    pub(super) fn tell_if_unread(&self, slug: &str, only: Option<&str>, what: &str) {
        if let Err(problem) = self.unread(slug, only, what) {
            eprintln!("could not tell @orchestrator that no executor reads {what}: {problem}");
        }
    }

    fn unread(&self, slug: &str, only: Option<&str>, what: &str) -> Result<(), Error> {
        let task = self.get(slug)?;
        let executors: Vec<_> = task
            .roles
            .iter()
            .filter(|r| r.role == "executor" && only.is_none_or(|h| r.handle == h))
            .collect();
        if only.is_some() && executors.is_empty() {
            return Ok(());
        }
        let callbacks = self.callbacks()?;
        let launched = self.task_sessions(slug)?;
        let mut gone = Vec::new();
        for executor in executors {
            let handle = executor.handle.as_str();
            let mine: Vec<_> = launched.iter().filter(|s| s.handle == handle).collect();
            let closed = mine
                .last()
                .filter(|_| mine.iter().all(|s| s.closed.is_some()));
            if callbacks.contains_key(handle)
                || (executor.health == Health::Live && closed.is_none())
            {
                return Ok(());
            }
            gone.push(match closed {
                Some(s) => format!(
                    "{handle}'s pane was closed ({}); just tasks restore {slug} {handle}",
                    s.closed_why
                ),
                None => format!("{handle} is {} with no callback", executor.health.as_str()),
            });
        }
        if gone.is_empty() {
            gone.push("no session holds the role".into());
        }
        let text = format!("{what} reaches no live executor: {}", gone.join("; "));
        let body = format!(
            "{}\nRETURN NONE;",
            logged("$slug", "$sender", "'msg'", "$text", "$to")
        );
        let vars = json!({"slug": slug, "sender": BOARD, "text": text, "to": "@orchestrator", "now": (self.clock)()});
        self.retrying(|| self.transact::<Value>(&body, vars.clone()))
            .map(|_| ())
    }

    /// Unread addressed messages, owner messages on its tasks, and verdicts for the executor and
    /// orchestrators. Unaddressed agent posts remain on the thread without notifying participants.
    pub fn inbox(&self, handle: &str) -> Result<Vec<Message>, Error> {
        self.inbox_after(handle, i64::MIN)
    }

    /// Native dispatch has already copied older messages to its durable outbox. Newly acquired
    /// roles must not backfill that history; manual inbox reads still use the complete history.
    pub(super) fn inbox_after(&self, handle: &str, since: i64) -> Result<Vec<Message>, Error> {
        #[derive(Deserialize)]
        struct Held {
            slug: String,
            role: String,
        }
        #[derive(Deserialize)]
        struct Read {
            orchestrators: Vec<(String, String)>,
            known: bool,
            roles: Vec<Held>,
            tree: Vec<(String, Option<String>)>,
            messages: Vec<MessageRow>,
        }
        let vars = json!({"handle": handle, "delivered": DELIVERED, "since":since});
        let read: Read = self.retrying(|| self.transact(INBOX, vars.clone()))?;
        if !read.known {
            return Err(refused(format!("no session {}", repr(handle))));
        }
        let tree: Tree = read.tree.into_iter().collect();
        let mut holds: HashMap<&str, Vec<&str>> = HashMap::new();
        for held in &read.roles {
            holds.entry(&held.slug).or_default().push(&held.role);
        }
        let mut orchestrators: HashMap<&str, Vec<&str>> = HashMap::new();
        for (slug, who) in &read.orchestrators {
            orchestrators.entry(slug).or_default().push(who);
        }
        // @orchestrator and verdicts go to the orchestrator of the nearest task, at or above the
        // message's, that has one: one orchestrator a task, never every one up the tree.
        let nearest = |slug: &str| {
            std::iter::once(slug)
                .chain(above(&tree, slug))
                .find_map(|at| orchestrators.get(at))
                .is_some_and(|held| held.contains(&handle))
        };
        let reaches = |m: &MessageRow| {
            let to = m.recipient.as_deref();
            let roles = holds.get(m.slug.as_str());
            to == Some(handle)
                || (m.sender == OWNER && roles.is_some())
                || (m.kind == "verdict"
                    && roles.is_some_and(|held| {
                        held.contains(&"executor") || held.contains(&"orchestrator")
                    }))
                || ((to == Some("@orchestrator") || m.kind == "verdict") && nearest(&m.slug))
                || to
                    .and_then(|to| to.strip_prefix('@'))
                    .filter(|role| *role != "orchestrator")
                    .is_some_and(|role| roles.is_some_and(|held| held.contains(&role)))
        };
        Ok(read
            .messages
            .into_iter()
            .filter(reaches)
            .map(message)
            .collect())
    }

    /// `handle` has read messages `ids`: they leave its inbox.
    pub fn ack(&self, handle: &str, ids: &[i64]) -> Result<(), Error> {
        let body = format!(
            "{ACT}
FOR $id IN $ids {{
    IF !record::exists(type::record('delivery', [$id, $actor])) {{
        CREATE type::record('delivery', [$id, $actor]) CONTENT {{ message: $id, handle: $actor, acked: $now }} RETURN NONE;
    }};
}};
RETURN NONE;"
        );
        let vars = acting(handle, (self.clock)(), json!({"ids": ids}));
        self.retrying(|| self.transact::<Value>(&body, vars.clone()))
            .map(|_| ())
    }

    /// The newest messages to the owner, newest first.
    pub fn to_owner(&self, limit: usize) -> Result<Vec<Message>, Error> {
        let read = "RETURN SELECT * OMIT id FROM message WHERE recipient = 'owner' ORDER BY sid DESC LIMIT $limit;";
        let rows: Vec<MessageRow> =
            self.retrying(|| self.transact(read, json!({"limit": limit})))?;
        Ok(rows.into_iter().map(message).collect())
    }

    /// New messages in creation order, for the Symphony server's shared delta feed. The cursor is
    /// the snapshot's latest_message, so two writes between reads are both delivered.
    pub fn messages_after(&self, since: i64, through: i64) -> Result<Vec<Message>, Error> {
        let read = "RETURN SELECT * OMIT id FROM message WHERE sid > $since AND sid <= $through ORDER BY sid ASC;";
        let rows: Vec<MessageRow> =
            self.retrying(|| self.transact(read, json!({"since": since, "through": through})))?;
        Ok(rows.into_iter().map(message).collect())
    }

    /// Each sender's latest message or verdict, by handle: what an agent last said on the board (the
    /// board's own record lines are not its words).
    pub fn last_messages(&self) -> Result<BTreeMap<String, Message>, Error> {
        let read = "
LET $last = (SELECT sender, array::max(array::group([[sid, slug]])) AS last FROM message WHERE kind IN $delivered GROUP BY sender)
    .map(|$sender| $sender.last[0]);
RETURN SELECT * OMIT id FROM $last.map(|$m| type::record('message', [$m[1], $m[0]]));";
        let rows: Vec<MessageRow> =
            self.retrying(|| self.transact(read, json!({"delivered": DELIVERED})))?;
        Ok(rows
            .into_iter()
            .map(|row| (row.sender.clone(), message(row)))
            .collect())
    }

    /// The slugs whose own thread carries a `landed <slug> on main` line (what `just land` says when
    /// it moves main) that no status change on the task follows: `land` deletes the branch, so for a
    /// task still open afterwards this line is the only trace, and a task moved on again since is new
    /// work, not a landing left open. A status change writes one of: a `moved it` line, a verdict (a
    /// `changes` sends the task back), or a `claimed it` line (a re-claim after a release). A release
    /// alone does not: roles come and go after a landing.
    pub fn landings_posted(&self) -> Result<BTreeSet<String>, Error> {
        #[derive(Deserialize)]
        struct Line {
            slug: String,
            sid: i64,
        }
        #[derive(Deserialize)]
        struct Read {
            landed: Vec<Line>,
            moved: Vec<Line>,
        }
        let read = "RETURN {
    landed: (SELECT slug, sid FROM message
        WHERE kind IN $delivered AND string::contains(text, 'landed ' + slug + ' on main')),
    moved: (SELECT slug, math::max(sid) AS sid FROM message WHERE kind = 'verdict'
        OR (kind = 'system' AND (string::contains(text, ' moved it ') OR string::contains(text, ' claimed it')))
        GROUP BY slug),
};";
        let read: Read = self.retrying(|| self.transact(read, json!({"delivered": DELIVERED})))?;
        let moved: HashMap<&str, i64> = read
            .moved
            .iter()
            .map(|line| (line.slug.as_str(), line.sid))
            .collect();
        Ok(read
            .landed
            .into_iter()
            // `<=`: a landing the board records (`Store::landed`) is itself a verdict, the newest such line;
            // only one after it voids it.
            .filter(|line| {
                moved
                    .get(line.slug.as_str())
                    .is_none_or(|&last| last <= line.sid)
            })
            .map(|line| line.slug)
            .collect())
    }
}
