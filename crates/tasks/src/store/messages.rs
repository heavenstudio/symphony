//! Threads: saying, inboxes and acknowledging them, what reaches the owner, each sender's last word,
//! and landings on the record.

use super::work::{OWNER, ROLES, Tree, above, no_task};
use super::{
    ACT, Error, Message, NATIVE_PRESENCE, NativePresence, REFUSED, Store, acting, logged, message,
    native_bounds, native_health, refused, repr,
};
use crate::health::{self, Health};
use crate::rows::{MessageRow, SessionRow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

/// The kinds of message that reach an inbox; `system` is the record only.
pub const DELIVERED: [&str; 2] = ["msg", "verdict"];

/// The sender of the board's own addressed posts: no session's, so no recipient takes one for its
/// own and leaves it out of its inbox.
pub const BOARD: &str = "board";

/// Whether a message could reach `$handle`'s inbox: delivered, sent since the session connected, not
/// its own, and from the owner, to it, to a role or a verdict (not addressed to another handle or the
/// owner). Its fields are read under `m`: `""` for a row of the table, `"$m."` for a row of a
/// [`super::pool::Pool`]. The roles and the tree decide the rest ([`Store::inbox_read`]).
fn could_reach(m: &str) -> String {
    format!(
        "{m}ts >= $connected AND {m}kind IN $delivered AND {m}sender != $handle
        AND ({m}sender = 'owner' OR {m}recipient = $handle OR ({m}recipient != NONE AND string::starts_with({m}recipient, '@')) OR {m}kind = 'verdict')"
    )
}

/// What an inbox is worked out from: the session's roles, the tree, and the messages since it
/// connected (after `$since`, when it is given) that are not its own, that it has not acknowledged,
/// and that could reach it. Read from the message table, through `message_ts` from the session's
/// connection or `message_sid` from `$since`, or from `$pool`, a dispatch pass's rows of it
/// ([`super::pool::Pool`]), when one is given: then acknowledgements are looked up for those rows
/// alone, not read through `delivery_handle`. The connection time is a parameter of its own: the
/// server takes an index for a parameter's value, never for a field of one (`$me.connected` read
/// every message). Acknowledgement markers
/// overwrite decimal integer keys during tree construction, then one marker filter selects the
/// unread record references. Their bodies are fetched within the same transaction snapshot, and the
/// roles and the tree only when one is wanted. The orchestrators' sessions are fetched by id from
/// their roles: a `session WHERE handle IN (SELECT ... FROM role)` runs the role scan again for each
/// session, seconds on a board of thousands of each, for every inbox a message could reach.
static INBOX: LazyLock<String> = LazyLock::new(|| {
    format!(
        "
LET $me = (SELECT connected FROM ONLY type::record('session', $handle));
LET $connected = $me.connected;
LET $candidates = IF $me = NONE {{ [] }} ELSE IF $pool != NONE {{
    $pool.filter(|$m| $m.sid > $since AND {pooled}).map(|$m| [<string>$m.sid, type::record('message', [$m.slug, $m.sid])])
}} ELSE IF $since = NONE {{
    (SELECT VALUE [<string>sid, id] FROM message WHERE {scan})
}} ELSE {{
    (SELECT VALUE [<string>sid, id] FROM message WHERE sid > $since AND {scan})
}};
LET $acked = IF $pool != NONE {{
    $candidates.filter(|$c| record::exists(type::record('delivery', [<int>$c[0], $handle]))).map(|$c| [$c[0], NULL])
}} ELSE IF $since = NONE {{
    (SELECT VALUE [<string>message, NULL] FROM delivery WHERE handle = $handle)
}} ELSE {{
    (SELECT VALUE [<string>message, NULL] FROM delivery WHERE handle = $handle AND message > $since)
}};
{unread}
LET $placing = array::len($wanted) > 0;
LET $orchestrators = IF $placing {{ (SELECT VALUE [slug, handle] FROM role WHERE role = 'orchestrator') }} ELSE {{ [] }};
RETURN {{
    orchestrators: $orchestrators,
    orchestrator_sessions: (SELECT * OMIT id FROM array::distinct($orchestrators.map(|$held| type::record('session', $held[1])))),
    native_presence: IF $placing {{ {NATIVE_PRESENCE} }} ELSE {{ [] }},
    known: $me != NONE,
    roles: IF $placing {{ (SELECT slug, role FROM role WHERE handle = $handle) }} ELSE {{ [] }},
    tree: IF $placing {{ (SELECT VALUE [slug, parent] FROM task) }} ELSE {{ [] }},
    messages: (SELECT * OMIT id FROM $wanted ORDER BY sid),
}};",
        scan = could_reach(""),
        pooled = could_reach("$m."),
        unread = include_str!("inbox_unread.surql"),
    )
});

/// `line`, a message's write, made once per `$key` on `$slug`'s thread: the first write under the
/// key makes the line and `delivery_key:[slug, key]`, which names that line to every later one,
/// and a later write makes nothing. Given `same`, a condition on the thread's lines, the first
/// write under the key instead takes the earliest line meeting it when the thread holds one (a
/// line made under no key), binding the key to that line and writing nothing. `$had` is the key's
/// record before this write.
fn once(line: &str, same: Option<&str>) -> String {
    let make = format!(
        "{line}
    CREATE type::record('delivery_key', [$slug, $key]) SET slug = $slug, key = $key, message = $sid, ts = $ts RETURN NONE;"
    );
    let write = match same {
        None => make,
        Some(same) => format!(
            "LET $same = array::first((SELECT VALUE sid FROM message:[$slug, NONE]..[$slug, ..] WHERE {same} LIMIT 1));
    IF $same != NONE {{
        CREATE type::record('delivery_key', [$slug, $key]) SET slug = $slug, key = $key, message = $same, ts = $now RETURN NONE;
    }} ELSE {{
        {make}
    }};"
        ),
    };
    format!(
        "LET $had = (SELECT message FROM ONLY type::record('delivery_key', [$slug, $key]));
IF $had = NONE {{
    {write}
}};"
    )
}

impl Store {
    /// Post to a task's thread. Only addressed messages and owner messages enter inboxes.
    pub fn say(
        &self,
        slug: &str,
        sender: &str,
        text: &str,
        to: Option<&str>,
    ) -> Result<Message, Error> {
        self.post(slug, sender, sender, text, to, None)
    }

    /// A tool's addressed notification, authorized as `actor` and authored by the board so even
    /// a waiting actor receives it in its inbox. Ordinary agent posts keep their own author.
    pub fn notify(
        &self,
        slug: &str,
        actor: &str,
        handle: &str,
        text: &str,
    ) -> Result<Message, Error> {
        if handle == OWNER || handle.starts_with('@') {
            return Err(refused("a notice needs a literal session handle".into()));
        }
        self.post(slug, actor, BOARD, text, Some(handle), None)
    }

    /// Post to a task's thread once per `key`: the first post under a key on a thread is made, and
    /// every later one answers that message and writes nothing, whatever it says
    /// (`delivery_key:[slug, key]`, read and written in the post's transaction, so two at once make
    /// one line). A caller whose post may have committed without its answer (its run stopped)
    /// repeats the call rather than reads the thread, and a request of its that outlived it cannot
    /// post twice.
    pub fn say_keyed(
        &self,
        slug: &str,
        sender: &str,
        text: &str,
        to: Option<&str>,
        key: &str,
    ) -> Result<Message, Error> {
        self.post(slug, sender, sender, text, to, Some((key, false)))
    }

    /// `say_keyed`, but a key that names no line yet takes the thread's earliest line reading
    /// exactly `text` to `to` when there is one (a post made under no key, before keys were) and
    /// writes nothing: that line is answered, and told of as a repeat's is.
    pub fn adopt_or_say(
        &self,
        slug: &str,
        sender: &str,
        text: &str,
        to: Option<&str>,
        key: &str,
    ) -> Result<Message, Error> {
        self.post(slug, sender, sender, text, to, Some((key, true)))
    }

    /// A post authorized as `actor` and authored by `sender` (the board's own, [`BOARD`], for a
    /// notice), made once per key when `keyed` gives one, with whether that key adopts a line made
    /// under no key ([`Store::adopt_or_say`]).
    fn post(
        &self,
        slug: &str,
        actor: &str,
        sender: &str,
        text: &str,
        to: Option<&str>,
        keyed: Option<(&str, bool)>,
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
        let act = if actor == OWNER {
            ""
        } else if sender == BOARD {
            // A machine event is no tool call or heartbeat from the waiting session.
            super::write::AUTHORIZE
        } else {
            ACT
        };
        let line = logged("$slug", "$sender", "'msg'", "$text", "$to");
        // The answer is the line on the record: this post's, or the one its key names.
        let said = match keyed {
            None => format!(
                "{line}
RETURN (SELECT * OMIT id FROM ONLY type::record('message', [$slug, $sid]));"
            ),
            Some((_, adopt)) => format!(
                "{}
LET $made = (SELECT message FROM ONLY type::record('delivery_key', [$slug, $key]));
RETURN (SELECT * OMIT id FROM ONLY type::record('message', [$slug, $made.message]));",
                once(
                    &line,
                    adopt.then_some("kind = 'msg' AND text = $text AND recipient = $to")
                )
            ),
        };
        let body = format!(
            "{act}
IF !record::exists(type::record('task', $slug)) {{ THROW $no_task }};
IF $no_role != NONE {{ THROW $no_role }};
IF $handle != NONE AND !record::exists(type::record('session', $handle)) {{ THROW $nobody }};
{said}"
        );
        let now = (self.clock)();
        let key = keyed.map(|(key, _)| key);
        let vars = acting(
            actor,
            now,
            json!({
                "slug": slug, "sender": sender, "text": text, "to": to, "handle": handle, "no_role": no_role, "key": key,
                "no_task": format!("{REFUSED}{}", no_task(slug)),
                "nobody": handle.map(|h| format!("{REFUSED}no session {} to send to", repr(h))),
            }),
        );
        let row: MessageRow = self.retrying(|| self.transact(&body, vars.clone()))?;
        // Told of the line's recipient, not this call's: a post repeated under a key, or one that
        // took a line made under no key, answers that line, and says once that no executor reads
        // it when nothing said so yet (the board's notice goes under the line's own key,
        // `unread:<id>`, which takes a notice of the line said under no key, so it is said once
        // per line, whichever post says it).
        let unread = format!("unread:{}", row.id);
        match row.recipient.as_deref() {
            Some("@executor") => self.tell_if_unread(
                slug,
                None,
                &format!("#{} from {} to @executor", row.id, row.sender),
                Some(&unread),
            ),
            Some(to) if to != OWNER && !to.starts_with('@') => self.tell_if_unread(
                slug,
                Some(to),
                &format!("#{} from {} to {to}", row.id, row.sender),
                Some(&unread),
            ),
            _ => {}
        }
        Ok(message(row))
    }

    /// `what` (a post or a changes verdict to `slug`'s executor, or to `only` when that handle
    /// executes it) has no executor to read it now: none has a native callback, and each has its
    /// pane recorded closed or no live beat. The task's orchestrators hear it at once on the
    /// thread, with the restore line of each executor whose pane was closed. A post to a handle
    /// that does not execute `slug` is left alone. Under `key`, it is said once per key on the
    /// thread however often it is asked. Never fails the write it follows: what went wrong goes to
    /// stderr.
    pub(super) fn tell_if_unread(
        &self,
        slug: &str,
        only: Option<&str>,
        what: &str,
        key: Option<&str>,
    ) {
        if let Err(problem) = self.unread(slug, only, what, key) {
            eprintln!("could not tell @orchestrator that no executor reads {what}: {problem}");
        }
    }

    fn unread(
        &self,
        slug: &str,
        only: Option<&str>,
        what: &str,
        key: Option<&str>,
    ) -> Result<(), Error> {
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
        let line = logged("$slug", "$sender", "'msg'", "$text", "$to");
        let body = format!(
            "{}\nRETURN NONE;",
            match key {
                Some(_) => once(
                    &line,
                    Some("sender = $sender AND string::starts_with(text, $said)")
                ),
                None => line,
            }
        );
        let vars = json!({
            "slug": slug, "sender": BOARD, "text": text, "to": "@orchestrator", "key": key,
            "said": format!("{what} reaches no live executor"), "now": (self.clock)(),
        });
        self.retrying(|| self.transact::<Value>(&body, vars.clone()))
            .map(|_| ())
    }

    /// Unread addressed messages, owner messages on its tasks, and verdicts for the executor and
    /// orchestrators. Unaddressed agent posts remain on the thread without notifying participants.
    pub fn inbox(&self, handle: &str) -> Result<Vec<Message>, Error> {
        self.inbox_read(handle, None, None)
    }

    /// The inbox (after `since`, when given) from the message table, or from `pool`'s rows of it
    /// ([`super::pool::Pool`]), which needs a `since`.
    pub(super) fn inbox_read(
        &self,
        handle: &str,
        since: Option<i64>,
        pool: Option<Value>,
    ) -> Result<Vec<Message>, Error> {
        #[derive(Deserialize)]
        struct Held {
            slug: String,
            role: String,
        }
        #[derive(Deserialize)]
        struct Read {
            orchestrators: Vec<(String, String)>,
            orchestrator_sessions: Vec<SessionRow>,
            native_presence: Vec<NativePresence>,
            known: bool,
            roles: Vec<Held>,
            tree: Vec<(String, Option<String>)>,
            messages: Vec<MessageRow>,
        }
        let mut vars =
            json!({"handle": handle, "delivered": DELIVERED, "since":since, "pool": pool});
        native_bounds(&mut vars);
        let read: Read = self.retrying(|| self.transact(&INBOX, vars.clone()))?;
        if !read.known {
            return Err(refused(format!("no session {}", repr(handle))));
        }
        let tree: Tree = read.tree.into_iter().collect();
        let mut holds: HashMap<&str, Vec<&str>> = HashMap::new();
        for held in &read.roles {
            holds.entry(&held.slug).or_default().push(&held.role);
        }
        // Holders whose session still holds, as `Owners::of` counts them (`crate::orchestration`).
        let now = (self.clock)();
        let native: HashMap<&str, f64> = (read.native_presence.iter())
            .map(|n| (n.handle.as_str(), n.until))
            .collect();
        let holding: HashSet<&str> = (read.orchestrator_sessions.iter())
            .filter(|row| {
                let health = health::health(row, now, &self.host, &*self.pid_alive, false);
                let until = native.get(row.handle.as_str()).copied();
                native_health(row, health, now, until).holds()
            })
            .map(|row| row.handle.as_str())
            .collect();
        let mut orchestrators: HashMap<&str, Vec<&str>> = HashMap::new();
        let held = (read.orchestrators.iter()).filter(|(_, who)| holding.contains(who.as_str()));
        for (slug, who) in held {
            orchestrators.entry(slug).or_default().push(who);
        }
        // @orchestrator and verdicts go to the orchestrator of the nearest task, at or above the
        // message's, that has one still there: one orchestrator a task, never every one up the tree.
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
    /// board's own record lines are not its words). `said` names each one ([`crate::schema`]), so
    /// this reads one message a sender, not every message.
    pub fn last_messages(&self) -> Result<BTreeMap<String, Message>, Error> {
        let read = "LET $said = (SELECT VALUE type::record('message', [slug, sid]) FROM said);
RETURN SELECT * OMIT id FROM $said;";
        let rows: Vec<MessageRow> = self.retrying(|| self.transact(read, json!({})))?;
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
    ///
    /// The lines holding the word `landed` are found through the full-text index, and each slug's
    /// newest landing is kept unless its own thread, read from that line on by key, holds a status
    /// change after it (a landing the board records, `Store::landed`, is itself a verdict: only one
    /// after it voids it). An older landing on the thread has every change after the newest after it
    /// too, so the newest alone decides.
    pub fn landings_posted(&self) -> Result<BTreeSet<String>, Error> {
        let read = "
LET $landed = (SELECT slug, math::max(sid) AS sid FROM message
    WHERE text @@ 'landed' AND kind IN $delivered AND string::contains(text, 'landed ' + slug + ' on main')
    GROUP BY slug);
RETURN $landed.filter(|$line| (SELECT VALUE sid FROM message:[$line.slug, $line.sid]>..[$line.slug, ..]
    WHERE kind = 'verdict' OR (kind = 'system' AND (string::contains(text, ' moved it ') OR string::contains(text, ' claimed it')))
    LIMIT 1) = []).map(|$line| $line.slug);";
        let slugs: Vec<String> =
            self.retrying(|| self.transact(read, json!({"delivered": DELIVERED})))?;
        Ok(slugs.into_iter().collect())
    }
}
