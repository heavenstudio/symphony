//! Threads: saying, inboxes and acknowledging them, what reaches the owner, each sender's last word,
//! and landings on the record.

use super::postgres::messages as pg;
use super::statement::Statement;
use super::work::{OWNER, ROLES, Tree, above, no_task};
use super::{
    Error, Failure, Message, NativePresence, REFUSED, Store, message, native_bounds, native_health,
    refused, repr,
};
use crate::health::{self, Health};
use crate::maps::Collect as _;
use crate::rows::{MessageRow, SessionRow};
use crate::stages::ARROW;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// The verdict a reviewer's line gives, `approve` or `changes`: a verdict line whose text is that
/// word alone or before its `: text`, as `just tasks verdict` writes it. A landing's line and a
/// subtask's `done:` give none. [`Store::newest_verdict`] reads by the same rule.
pub fn reviewer_verdict(message: &Message) -> Option<&'static str> {
    if message.kind != "verdict" {
        return None;
    }
    ["approve", "changes"]
        .into_iter()
        .find(|verdict| crate::stages::verdict_is(&message.text, verdict))
}

/// The kinds of message that reach an inbox; `system` is the record only.
pub const DELIVERED: [&str; 2] = ["msg", "verdict"];

/// The sender of the board's own addressed posts: no session's, so no recipient takes one for its
/// own and leaves it out of its inbox.
pub const BOARD: &str = "board";

/// Whom a post's write is authorized as: the owner, or a person a board named
/// ([`Store::is_person`]), whose write checks no session; the board, for
/// a notice a session's command makes, which checks its session and records no activity of it; or
/// the session posting, checked and its activity recorded.
#[derive(Clone, Copy, Debug)]
pub(super) enum Poster {
    Owner,
    Board,
    Session,
}

/// The mark on the owner's word about a task no live orchestrator holds, as the inbox of an
/// orchestrator it reached for that carries it ([`heard`]): every orchestrator hears it there, since
/// none of the task's relays it. This one is for a task nobody holds; one held by an orchestrator
/// that is not live names it ([`unowned_mark`]). Those on the task, and its holder, hear it
/// unmarked.
const UNOWNED: &str = "[unowned: no orchestrator holds it] ";

/// The mark ([`UNOWNED`]) for a task whose nearest holders are `held`, none of them live: each by
/// its handle and health, `[unowned: no live orchestrator holds it: amber (stale)] `.
fn unowned_mark(held: &[(&str, Health)]) -> String {
    if held.is_empty() {
        return UNOWNED.to_string();
    }
    let named: Vec<String> = (held.iter())
        .map(|(handle, health)| format!("{handle} ({})", health.as_str()))
        .collect();
    format!(
        "[unowned: no live orchestrator holds it: {}] ",
        named.join(", ")
    )
}

/// What the owner's move of a task to a finished status says to an inbox after the move's own line
/// ([`heard`]): those on the task stop, without an orchestrator relaying it.
const NO_FURTHER: &str = "; it moves no further: stop work on it";

/// `row`, which reached an inbox, as the inbox carries it: the owner's move of a task to a
/// finished status, or a person's ([`MessageRow::by_person`]), says it moves no further
/// ([`NO_FURTHER`]), and their word about a task no live orchestrator holds, as an orchestrator it
/// reached for that hears it, carries `mark` ([`unowned_mark`]). The thread keeps the line as
/// written.
fn heard(row: MessageRow, mark: Option<&str>) -> Message {
    let by_person = row.by_person();
    let mut message = message(row);
    if !by_person {
        return message;
    }
    let finished = |to: &str| crate::health::FINISHED.contains(&to.trim());
    if message.kind == "system"
        && (message.text.rsplit_once(ARROW)).is_some_and(|(_, to)| finished(to))
    {
        message.text.push_str(NO_FURTHER);
    }
    if let Some(mark) = mark {
        message.text.insert_str(0, mark);
    }
    message
}

/// The refusal of a post to `handle`, which no session has: a role's name written without its `@`
/// says how it is written.
fn nobody(handle: &str) -> String {
    let role = ROLES
        .iter()
        .find(|role| role.eq_ignore_ascii_case(handle))
        .map(|role| format!("; the task's {role} is --to @{role}"))
        .unwrap_or_default();
    format!("{REFUSED}no session {} to send to{role}", repr(handle))
}

impl Store {
    /// Post to a task's thread. Only addressed messages, verdicts and the owner's word enter inboxes
    /// ([`Store::inbox`]).
    pub fn say(
        &self,
        slug: &str,
        sender: &str,
        text: &str,
        to: Option<&str>,
    ) -> Result<Message, Error> {
        self.post(slug, sender, sender, text, to, None, false)
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
        if self.is_person(handle) || handle.starts_with('@') {
            return Err(refused("a notice needs a literal session handle".into()));
        }
        self.post(slug, actor, BOARD, text, Some(handle), None, false)
    }

    /// A line a tool says for `actor` on a task's thread, to `to` (a role, a session or none),
    /// authorized as `actor` and authored by the board, as [`Store::notify`]'s is: what a detached
    /// dispatch or a delivery watcher's round acting as the task's orchestrator says to
    /// `@orchestrator`, which a line of its own would reach nobody with, as an inbox leaves out
    /// what its own session said.
    pub fn say_for(
        &self,
        slug: &str,
        actor: &str,
        text: &str,
        to: Option<&str>,
    ) -> Result<Message, Error> {
        self.post(slug, actor, BOARD, text, to, None, false)
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
        self.post(slug, sender, sender, text, to, Some((key, false)), false)
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
        self.post(slug, sender, sender, text, to, Some((key, true)), false)
    }

    /// A line of the board's own to `slug`'s orchestrator, made once per `key` on its thread
    /// however often it is asked, in the post's own write (`delivery_key:[slug, key]`), as
    /// [`Store::say_keyed`] makes one; an identical line the board said there before under no key
    /// is taken for it. What the owner said that leaves a task's block standing is told so
    /// (`symphony::decided`).
    pub fn tell_orchestrator_once(&self, slug: &str, text: &str, key: &str) -> Result<(), Error> {
        let text = text.trim();
        if text.is_empty() {
            return Err(refused("an empty message says nothing".into()));
        }
        let body = || Statement::from(pg::unread(true)).posting();
        let vars = json!({
            "slug": slug, "sender": BOARD, "text": text, "to": "@orchestrator", "key": key,
            "said": text, "now": (self.clock)(),
        });
        self.retrying(|| self.transact::<Value>(body(), vars.clone()))
            .map(|_| ())
    }

    /// The owner's message on a task's thread, which waits on an agent's answer: posted as
    /// [`Store::say`] posts, and in the same write it opens the task's ask, or joins the open one
    /// ([`crate::Ask`]). The next line by a session holding a role on the task, or by the session
    /// it is sent to, answers it. The owner's other lines there (a board action's record) open none.
    pub fn say_asking(&self, slug: &str, text: &str, to: Option<&str>) -> Result<Message, Error> {
        self.say_asking_as(slug, OWNER, text, to)
    }

    /// [`Store::say_asking`] by the person `sender`, the actor of the write
    /// (`symphony_plugin::Actor`): a person's line, which opens or joins the task's ask, when it
    /// is `owner` or the person this store writes for ([`Store::for_person`]), marked theirs. Any
    /// other is posted as a session's line is, so one the board does not know is refused as every
    /// unknown actor is (docs/product/analysis/users-and-identity.md, "The way from one owner").
    pub fn say_asking_as(
        &self,
        slug: &str,
        sender: &str,
        text: &str,
        to: Option<&str>,
    ) -> Result<Message, Error> {
        self.post(slug, sender, sender, text, to, None, true)
    }

    /// A post authorized as `actor` and authored by `sender` (the board's own, [`BOARD`], for a
    /// notice), made once per key when `keyed` gives one, with whether that key adopts a line made
    /// under no key ([`Store::adopt_or_say`]); with `asking`, the owner's line opens or joins the
    /// task's ask ([`pg::post`]).
    #[expect(
        clippy::too_many_arguments,
        reason = "each caller names its post's every part; a struct would only rename them"
    )]
    pub(super) fn post(
        &self,
        slug: &str,
        actor: &str,
        sender: &str,
        text: &str,
        to: Option<&str>,
        keyed: Option<(&str, bool)>,
        asking: bool,
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
        let poster = if self.is_person(actor) {
            Poster::Owner
        } else if sender == BOARD {
            Poster::Board
        } else {
            Poster::Session
        };
        let (task_about, task_words) = super::asks::on_thread_vars(slug, text);
        let shape = keyed.map(|(_, adopt)| adopt);
        let body = || Statement::from(pg::post(poster, shape)).posting();
        let now = (self.clock)();
        let key = keyed.map(|(key, _)| key);
        let vars = self.acting(
            actor,
            now,
            json!({
                "slug": slug, "sender": sender, "text": text, "to": to, "handle": handle, "no_role": no_role, "key": key,
                "task_about": task_about, "task_words": task_words, "asking": asking,
                "no_task": format!("{REFUSED}{}", no_task(slug)),
                "nobody": handle.map(nobody),
            }),
        );
        let row: MessageRow = self.retrying(|| self.transact(body(), vars.clone()))?;
        // Told of the line's recipient, not this call's: a post repeated under a key, or one that
        // took a line made under no key, answers that line.
        self.tell_line_if_unread(slug, row.id, &row.sender, row.recipient.as_deref());
        Ok(message(row))
    }

    /// The line `id` on `slug`'s thread, `sender`'s to `recipient`, has no executor to read it
    /// ([`Store::tell_if_unread`]): told where it is written to `@executor`, or to a handle that
    /// executes the task; a line to nobody, to `owner`, to another role or to another handle tells
    /// no one. Said once per line, whichever write says it: under the line's own key,
    /// `unread:<id>`, which takes a notice of the line said under no key. A post and a held
    /// message passed on ([`Store::pass`]) tell by it alike.
    pub(super) fn tell_line_if_unread(
        &self,
        slug: &str,
        id: i64,
        sender: &str,
        recipient: Option<&str>,
    ) {
        let (to, only) = match recipient {
            Some(to @ "@executor") => (to, None),
            Some(to) if to != OWNER && !to.starts_with('@') => (to, Some(to)),
            _ => return,
        };
        let what = format!("#{id} from {sender} to {to}");
        self.tell_if_unread(slug, only, &what, Some(&format!("unread:{id}")));
    }

    /// `said`, a record line `sender` puts on `slug`'s thread, to no one, once per `key`: a later
    /// one under the key writes nothing ([`Store::say_keyed`]'s `delivery_key:[slug, key]`, read
    /// and written in the line's transaction). Within a caller's retry.
    pub(super) fn record_once(
        &self,
        slug: &str,
        sender: &str,
        said: &str,
        key: &str,
    ) -> Result<(), Failure> {
        let vars = json!({
            "slug": slug, "sender": sender, "said": said, "key": key, "now": (self.clock)(),
        });
        let body = Statement::from(pg::record_once()).posting();
        self.transact::<Value>(body, vars).map(|_| ())
    }

    /// Whether a line was made under `key` on `slug`'s thread ([`Store::record_once`],
    /// [`Store::say_keyed`]). Within a caller's retry.
    pub(super) fn keyed(&self, slug: &str, key: &str) -> Result<bool, Failure> {
        let vars = json!({"slug": slug, "key": key});
        let keyed: Value = self.read(Statement::from(pg::keyed()), vars)?;
        Ok(keyed == Value::Bool(true))
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
        let body = || Statement::from(pg::unread(key.is_some())).posting();
        let vars = json!({
            "slug": slug, "sender": BOARD, "text": text, "to": "@orchestrator", "key": key,
            "said": format!("{what} reaches no live executor"), "now": (self.clock)(),
        });
        self.retrying(|| self.transact::<Value>(body(), vars.clone()))
            .map(|_| ())
    }

    /// Unread addressed messages, verdicts for the executor and orchestrators, and the owner's word
    /// (its posts, verdicts and moves) on its tasks and the ones it orchestrates, and, for an
    /// orchestrator, on every task no live orchestrator holds (nobody, or a holder stale or of
    /// unknown liveness), marked ([`unowned_mark`], [`heard`]); such a holder hears it too,
    /// unmarked. Unaddressed agent posts remain on the thread without notifying participants.
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
            orchestrating: bool,
            roles: Vec<Held>,
            tree: Vec<(String, Option<String>)>,
            messages: Vec<MessageRow>,
        }
        let mut vars =
            json!({"handle": handle, "delivered": DELIVERED, "since":since, "pool": pool});
        native_bounds(&mut vars);
        let read: Read =
            self.retrying(|| self.read_together(Statement::from(pg::inbox()), vars.clone()))?;
        if !read.known {
            return Err(refused(format!("no session {}", repr(handle))));
        }
        let tree: Tree = read.tree.into_iter().collect();
        let mut holds: HashMap<&str, Vec<&str>> = HashMap::new();
        for held in &read.roles {
            holds.entry(&held.slug).or_default().push(&held.role);
        }
        // Each orchestrator's health: those that still hold, as `Owners::of` counts them
        // (`crate::orchestration`), hold their tasks; only a live one keeps the owner's word to itself.
        let now = (self.clock)();
        let native: HashMap<&str, f64> = (read.native_presence.iter())
            .map(|n| (n.handle.as_str(), n.until))
            .collect();
        let health_of: HashMap<&str, Health> = (read.orchestrator_sessions.iter())
            .map(|row| {
                let health = health::health(row, now, &self.host, &*self.pid_alive, false);
                let until = native.get(row.handle.as_str()).copied();
                (row.handle.as_str(), native_health(row, health, now, until))
            })
            .collect();
        let holding: HashSet<&str> = (health_of.iter())
            .filter(|(_, health)| health.holds())
            .map(|(handle, _)| *handle)
            .collect();
        let mut orchestrators: HashMap<&str, Vec<&str>> = HashMap::new();
        let held = (read.orchestrators.iter()).filter(|(_, who)| holding.contains(who.as_str()));
        for (slug, who) in held {
            orchestrators.entry(slug).or_default().push(who);
        }
        // The holders of the nearest task, at or above the message's, that has one still there.
        let holders = |slug: &str| {
            std::iter::once(slug)
                .chain(above(&tree, slug))
                .find_map(|at| orchestrators.get(at))
        };
        // @orchestrator and verdicts go to the orchestrator of that task: one orchestrator a task,
        // never every one up the tree.
        let nearest = |slug: &str| holders(slug).is_some_and(|held| held.contains(&handle));
        // A task no live orchestrator holds: the holders that are not live, none when nobody holds
        // it (as `Owners::of` reads it); `None` when a live one holds it.
        let unowned = |slug: &str| -> Option<Vec<(&str, Health)>> {
            let held = holders(slug).map_or(&[][..], Vec::as_slice);
            let health = |who: &&str| health_of.get(who).copied().unwrap_or(Health::Left);
            if held.iter().any(|who| health(who) == Health::Live) {
                return None;
            }
            Some(held.iter().map(|who| (*who, health(who))).collect())
        };
        let orchestrating = read.orchestrating;
        let reaches = |m: &MessageRow| {
            let to = m.recipient.as_deref();
            let roles = holds.get(m.slug.as_str());
            // The owner's word, a post, a verdict or a move, reaches everyone on the task and its
            // orchestrator; on a task no live orchestrator holds, every orchestrator, since none
            // relays it.
            let owners_word = m.by_person()
                && (roles.is_some()
                    || nearest(&m.slug)
                    || (orchestrating && unowned(&m.slug).is_some()));
            to == Some(handle)
                || owners_word
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
            .map(|row| {
                // Only an orchestrator it reached for that hears the mark, never the holder.
                let marked = orchestrating && row.by_person() && !nearest(&row.slug);
                let mark = marked.then(|| unowned(&row.slug)).flatten();
                let mark = mark.map(|held| unowned_mark(&held));
                heard(row, mark.as_deref())
            })
            .collect())
    }

    /// `handle` has read messages `ids`: they leave its inbox.
    pub fn ack(&self, handle: &str, ids: &[i64]) -> Result<(), Error> {
        let body = || Statement::from(pg::ack());
        let vars = self.acting(handle, (self.clock)(), json!({"ids": ids}));
        self.retrying(|| self.transact::<Value>(body(), vars.clone()))
            .map(|_| ())
    }

    /// The newest messages to the owner, newest first.
    pub fn to_owner(&self, limit: usize) -> Result<Vec<Message>, Error> {
        let rows: Vec<MessageRow> = self.retrying(|| {
            self.read_together(Statement::from(pg::to_owner()), json!({"limit": limit}))
        })?;
        Ok(rows.into_iter().map(message).collect())
    }

    /// The questions to `person` (the owner, today) that still wait on them on the tasks `open`
    /// names, oldest first: lines of kind `msg` to `person` from anyone else that no post of
    /// `person`'s on the same thread follows, the rule by which the owner's activity list says
    /// "answered". Read by the recipient's index, then each line's own thread from it on by key, as
    /// [`Store::landings_posted`] reads its landings.
    pub fn questions_waiting(&self, person: &str, open: &[String]) -> Result<Vec<Message>, Error> {
        if open.is_empty() {
            return Ok(Vec::new());
        }
        let rows: Vec<MessageRow> = self.retrying(|| {
            self.read_together(
                Statement::from(pg::questions_waiting()),
                json!({"person": person, "open": open}),
            )
        })?;
        Ok(rows.into_iter().map(message).collect())
    }

    /// The newest reviewer's verdict on the board ([`reviewer_verdict`]), none on a board with none.
    /// It reads the lines from the newest back to that verdict, every line of a board with none, so
    /// a reader reads it once and follows the lines after it ([`Store::messages_after`]).
    pub fn newest_verdict(&self) -> Result<Option<Message>, Error> {
        let rows: Vec<MessageRow> =
            self.retrying(|| self.read_together(Statement::from(pg::newest_verdict()), json!({})))?;
        Ok(rows.into_iter().next().map(message))
    }

    /// New messages in creation order, for the Symphony server's shared delta feed. The cursor is
    /// the snapshot's latest_message, so two writes between reads are both delivered.
    pub fn messages_after(&self, since: i64, through: i64) -> Result<Vec<Message>, Error> {
        let rows = self.messages_after_rows(since, through)?;
        Ok(rows.into_iter().map(message).collect())
    }

    /// [`Store::messages_after`] as the board holds the lines, each with whether a person wrote
    /// it ([`MessageRow::by_person`]).
    pub fn messages_after_rows(&self, since: i64, through: i64) -> Result<Vec<MessageRow>, Error> {
        self.retrying(|| {
            self.read_together(
                Statement::from(pg::messages_after()),
                json!({"since": since, "through": through}),
            )
        })
    }

    /// Each sender's latest message or verdict, by handle: what an agent last said on the board (the
    /// board's own record lines are not its words). `said` names each one ([`crate::schema`]), so
    /// this reads one message a sender, not every message.
    pub fn last_messages(&self) -> Result<BTreeMap<String, Message>, Error> {
        let rows: Vec<MessageRow> =
            self.retrying(|| self.read_together(Statement::from(pg::last_messages()), json!({})))?;
        Ok(rows
            .into_iter()
            .map(|row| (row.sender.clone(), message(row)))
            .collect_map())
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
        let slugs: Vec<String> = self.retrying(|| {
            self.read_together(
                Statement::from(pg::landings_posted()),
                json!({"delivered": DELIVERED}),
            )
        })?;
        let mut landed = BTreeSet::new();
        landed.extend(slugs);
        Ok(landed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(sender: &str, kind: &str, text: &str) -> Message {
        Message {
            id: 1,
            slug: "tools/x".into(),
            sender: sender.into(),
            recipient: None,
            kind: kind.into(),
            text: text.into(),
            ts: 0.0,
        }
    }

    /// [`said`] as the board holds it, a person's line when `person`.
    fn row(sender: &str, kind: &str, text: &str, person: bool) -> MessageRow {
        let Message {
            id,
            slug,
            sender,
            recipient,
            kind,
            text,
            ts,
        } = said(sender, kind, text);
        MessageRow {
            id,
            slug,
            sender,
            recipient,
            kind,
            text,
            ts,
            person,
        }
    }

    fn heard_text(sender: &str, kind: &str, text: &str, unowned: bool) -> String {
        heard(row(sender, kind, text, false), unowned.then_some(UNOWNED)).text
    }

    #[test]
    fn a_reviewers_verdict_is_its_word_alone_or_before_its_text_on_a_verdict_line() {
        for (kind, text, verdict) in [
            ("verdict", "approve", Some("approve")),
            ("verdict", "approve: good at 4b5e7b0", Some("approve")),
            ("verdict", "changes", Some("changes")),
            ("verdict", "changes: the hash moved", Some("changes")),
            ("verdict", "approved: fine", None),
            ("verdict", "changesets differ", None),
            ("verdict", "done: integrated in 3e0c661", None),
            ("verdict", "landed tools/x on main at 3e0c661", None),
            ("msg", "approve: said as a message", None),
            ("system", "approve", None),
        ] {
            assert_eq!(
                reviewer_verdict(&said("slate-wren", kind, text)),
                verdict,
                "{kind}: {text}"
            );
        }
    }

    #[test]
    fn the_owners_move_to_a_finished_status_says_it_moves_no_further() {
        for to in ["cancelled", "done"] {
            let moved = format!("owner moved it in_progress → {to}");
            assert_eq!(
                heard_text(OWNER, "system", &moved, false),
                format!("{moved}{NO_FURTHER}")
            );
        }
        let reopened = "owner moved it done → ready";
        assert_eq!(heard_text(OWNER, "system", reopened, false), reopened);
        let posted = "I moved it review → cancelled";
        assert_eq!(
            heard_text(OWNER, "msg", posted, false),
            posted,
            "a post, not a move"
        );
    }

    #[test]
    fn only_the_owners_word_on_a_task_nobody_orchestrates_is_marked() {
        assert_eq!(
            heard_text(OWNER, "msg", "stop", true),
            format!("{UNOWNED}stop")
        );
        assert_eq!(
            heard_text(OWNER, "system", "owner moved it ready → cancelled", true),
            format!("{UNOWNED}owner moved it ready → cancelled{NO_FURTHER}")
        );
        assert_eq!(
            heard_text(OWNER, "verdict", "approve: ok", false),
            "approve: ok"
        );
        let agent = "amber moved it in_progress → cancelled";
        assert_eq!(heard_text("amber", "system", agent, true), agent);
        assert_eq!(heard_text("amber", "msg", "hi", true), "hi");
    }

    #[test]
    fn a_persons_word_is_heard_as_the_owners_is_and_a_sessions_of_that_name_is_not() {
        let moved = "ana moved it in_progress → done";
        let ana = |kind, text, person| heard(row("ana", kind, text, person), Some(UNOWNED)).text;
        assert_eq!(
            ana("system", moved, true),
            format!("{UNOWNED}{moved}{NO_FURTHER}")
        );
        assert_eq!(ana("msg", "stop", true), format!("{UNOWNED}stop"));
        assert_eq!(
            ana("system", moved, false),
            moved,
            "a session named ana is no person"
        );
        assert_eq!(ana("msg", "stop", false), "stop");
    }

    #[test]
    fn the_mark_names_the_holders_that_are_not_live_or_says_nobody_holds_it() {
        assert_eq!(unowned_mark(&[]), UNOWNED);
        assert_eq!(
            unowned_mark(&[("amber", Health::Stale)]),
            "[unowned: no live orchestrator holds it: amber (stale)] "
        );
        assert_eq!(
            unowned_mark(&[("amber", Health::Stale), ("teal", Health::Unknown)]),
            "[unowned: no live orchestrator holds it: amber (stale), teal (unknown)] "
        );
        let mark = unowned_mark(&[("amber", Health::Unknown)]);
        assert_eq!(
            heard(row(OWNER, "msg", "stop", false), Some(&mark)).text,
            "[unowned: no live orchestrator holds it: amber (unknown)] stop"
        );
    }
    #[test]
    fn pooled_inbox_and_cursor_read_the_same_unacknowledged_suffix() {
        use crate::store_fixture::{Fixture, Seed};
        let f = Fixture::new(Seed::default());
        let worker = f.session("worker");
        f.task("work");
        f.store.claim("work", &worker).unwrap();
        let first = f
            .store
            .say("work", "owner", "first", Some(&worker))
            .unwrap();
        let acked = f
            .store
            .say("work", "owner", "acked", Some(&worker))
            .unwrap();
        let last = f.store.say("work", "owner", "last", Some(&worker)).unwrap();
        f.store.ack(&worker, &[acked.id]).unwrap();
        let rows = f.store.messages_after(first.id, last.id).unwrap();
        assert_eq!(
            rows.iter().map(|m| m.id).collect::<Vec<_>>(),
            [acked.id, last.id]
        );
        let pool: Vec<Value> = rows
            .iter()
            .map(|m| {
                json!({"sid":m.id,"slug":m.slug,
            "sender":m.sender,"recipient":m.recipient,"kind":m.kind,"ts":m.ts})
            })
            .collect();
        let direct = f.store.inbox_read(&worker, Some(first.id), None).unwrap();
        let pooled = f
            .store
            .inbox_read(&worker, Some(first.id), Some(json!(pool)))
            .unwrap();
        assert_eq!(direct, [last]);
        assert_eq!(pooled, direct);
        assert!(
            f.store
                .inbox_read(&worker, Some(0), Some(json!([])))
                .unwrap()
                .is_empty()
        );
        assert!(
            f.store
                .messages_after(first.id, first.id)
                .unwrap()
                .is_empty()
        );
    }
}
