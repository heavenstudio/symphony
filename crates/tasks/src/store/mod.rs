//! The board over SurrealDB: the reads, the rules and the copy (`tools/tasks_store.py` `Store`).
//!
//! Every read is one request whose statements run in one transaction, so it sees the board as of one
//! commit (the Python store's `_snapshot`). A rule reads the task it acts on, decides here, and writes
//! in a transaction that throws `stale` if what it decided on has moved since: the task's `version`,
//! the actor's session, and the `last_seen` of every session it took to be gone (each write that
//! could make a session healthier moves its `last_seen`). A write that meets another's (SurrealDB's
//! optimistic conflict) or finds itself stale reads and decides again.

use crate::health::{self, EXECUTING, Health};
pub(crate) use crate::pyjson::repr;
use crate::rows::{LinkRow, MessageRow, RoleRow, SessionRow, TaskRow};
use crate::rpc::{Client, DbError};
use crate::schema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fmt::Write as _;
use std::hash::{BuildHasher, RandomState};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod callback_peer;
mod callback_state;
mod callbacks;
mod cap;
mod copy;
mod depends;
mod dispatcher;
mod embeddings;
mod fields;
mod launches;
mod messages;
mod pool;
mod pull_account;
mod queued_lines;
pub(crate) mod search;
mod sessions;
mod transaction;
mod work;

pub use callbacks::{Callback, Presence};
/// Every statement the cap on an orchestrator's open tasks sends: the plan test's to plan.
pub use cap::STATEMENTS as SLOT_READS;
pub use copy::Moved;
pub(crate) use copy::WRITTEN_SINCE;
pub use embeddings::{EmbeddingStats, VectorHit, VectorSearchProfile};
pub use launches::NewLaunch;
pub use search::SearchHit;
pub use sessions::{Beat, KINDS, NewSession, Pick};
pub use work::{
    BEFORE_WORK, LANDS_FROM, Landed, NewTask, PIPELINE, PULLABLE, ROLES, STATUSES, can_transition,
};

type Clock = Box<dyn Fn() -> f64 + Send + Sync>;
type Probe = Box<dyn Fn(i64) -> bool + Send + Sync>;

/// What a write that asks for a callback dispatch does instead of dispatching
/// ([`Store::dispatching_by`]): starts the dispatch elsewhere and says whether it did. When it did
/// not, the write dispatches itself.
pub type HandOff = Box<dyn Fn() -> bool + Send + Sync>;

/// How long a call reads and decides again before it gives up on a board that keeps moving: as long as
/// the SQLite store waited on its write lock, not a count of tries a busy board spends in two seconds.
const PATIENCE: Duration = Duration::from_secs(30);
/// What a write throws when what its rule decided on has moved.
const STALE: &str = "stale";
/// What a write throws to refuse goes before its words: the refusal comes back as `Error::Refused`.
const REFUSED: &str = "refused: ";
/// The board's schema version, or NONE before its tables are made: a read ([`crate::rpc::Kind`]).
const SCHEMA_VERSION: &str = "RETURN IF (INFO FOR DB).tables.meta = NONE { NONE } ELSE { (SELECT VALUE version FROM ONLY meta:schema) };";

/// Whether the server said `said` of a request in a namespace, or in the database `database`, that
/// it does not hold yet: a board no one has opened. These are the pinned server's words.
fn undefined(said: &str, database: &str) -> bool {
    said == format!("The database '{database}' does not exist")
        || said
            .strip_prefix("The namespace '")
            .is_some_and(|name| name.ends_with("' does not exist"))
}

/// Why a call gave no result.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The request was refused and nothing was written, in its words.
    Refused(String),
    /// SurrealDB could not be reached or did not take the statements.
    Db(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Refused(message) => f.write_str(message),
            Error::Db(message) => write!(f, "SurrealDB: {message}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq)]
pub struct Role {
    pub handle: String,
    pub role: String,
    pub since: f64,
    pub expires: Option<f64>,
    pub health: Health,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    /// The order it was created in.
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub body: String,
    pub status: String,
    pub priority: i64,
    pub parent: Option<String>,
    pub created_by: String,
    pub created: f64,
    pub updated: f64,
    /// The documents it implements.
    pub links: Vec<String>,
    /// The tasks it waits on.
    pub depends: Vec<String>,
    /// Who holds it, a lapsed reservation left out.
    pub roles: Vec<Role>,
    /// An instruction subtask: its executor closes it with `done`.
    pub instruction: bool,
    /// The plugins' board fields: plugin, then field, then its value.
    pub fields: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Value>>,
}

impl Task {
    pub fn holders(&self, role: &str) -> Vec<&str> {
        self.roles
            .iter()
            .filter(|r| r.role == role)
            .map(|r| r.handle.as_str())
            .collect()
    }

    /// Whether it was filed for a red ([`crate::red`]).
    pub fn red(&self) -> bool {
        crate::red::marked(&self.fields)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub row: SessionRow,
    pub health: Health,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub id: i64,
    pub slug: String,
    pub sender: String,
    pub recipient: Option<String>,
    pub kind: String,
    pub text: String,
    pub ts: f64,
}

/// The whole board as of one moment.
#[derive(Debug, Clone, PartialEq)]
pub struct Board {
    /// By priority, then the order they were created in.
    pub tasks: Vec<Task>,
    /// By when they connected, then handle.
    pub sessions: Vec<Session>,
    /// The newest message's id: a reader polling for change compares it.
    pub latest_message: i64,
}

pub struct Store {
    client: Client,
    clock: Clock,
    pid_alive: Probe,
    host: String,
    /// Where new sessions' handles come from.
    pick: Mutex<Box<dyn Pick>>,
    callback_snapshot: Mutex<Option<Snapshot>>,
    callback_idle: Mutex<Option<callbacks::Idle>>,
    /// The hc terminals the server clock's current tick sampled, which every view of that tick reads.
    clock_panes: Mutex<Option<crate::cli::agents::Answer<Vec<crate::cli::agents::Pane>>>>,
    /// Where this store's writes send their callback dispatch; none dispatches in the write.
    hand_off: Option<HandOff>,
    /// The messages a dispatch pass reads every recipient's inbox from ([`pool::Pool`]).
    inbox_pool: Mutex<Option<pool::Pool>>,
    /// The open tasks an orchestrator may hold before a start is refused ([`crate::slots`]); none
    /// caps nothing.
    slots: Option<usize>,
}

impl Store {
    /// The board in `client`'s database, its tables made if they are not there yet, read with this
    /// machine's clock, host name and process table.
    pub fn open(client: Client) -> Result<Store, Error> {
        let host = crate::sys::hostname();
        Store::with(
            client,
            Box::new(now),
            Box::new(crate::sys::pid_alive),
            &host,
        )
    }

    /// As `open`, with the clock, the process probe and the host name given (the tests' own). A
    /// board at this build's version opens on one read of it, which defines and writes nothing, so
    /// a slow board's late answer to it goes once more ([`crate::rpc::Kind`]) as a page's read
    /// does. Any other board is defined: its namespace and database first when the server lacks
    /// either, then its tables and version.
    pub fn with(
        client: Client,
        clock: Clock,
        pid_alive: Probe,
        host: &str,
    ) -> Result<Store, Error> {
        let store = Store::unchecked(client, clock, pid_alive, host);
        store.retrying(|| {
            let found = store.schema_version()?;
            if found.as_ref() == Some(&json!(schema::VERSION)) {
                return Ok(());
            }
            if found.is_none() {
                store.client.define_then("RETURN NONE;", json!({}))?;
            }
            store.client.query(schema::ANALYZERS, json!({}))?;
            // One transaction: a definition that fails, or loses to another open's at once, takes the
            // version with it, so no open takes a board missing it for made.
            let define = format!(
                "{}\nUPSERT meta:schema SET version = $version RETURN NONE;\nRETURN NONE;",
                schema::DEFINE
            );
            store.transact::<Value>(&define, json!({"version": schema::VERSION}))?;
            Ok(())
        })?;
        Ok(store)
    }

    /// The board's schema version (null before its tables are made), or none when the server lacks
    /// its namespace or its database: one read in them, which defines nothing.
    fn schema_version(&self) -> Result<Option<Value>, Failure> {
        match self.read(SCHEMA_VERSION, json!({})) {
            Err(Failure::Stop(Error::Db(said))) if undefined(&said, self.client.database()) => {
                Ok(None)
            }
            found => found.map(Some),
        }
    }

    /// As `with`, taking the board's tables as they are, without the request that checks them: the
    /// heartbeat's, one request on every tool call, on a board any other command has opened.
    pub fn unchecked(client: Client, clock: Clock, pid_alive: Probe, host: &str) -> Store {
        Store {
            client,
            clock,
            pid_alive,
            host: host.into(),
            pick: Mutex::new(Box::new(sessions::Random::seeded())),
            callback_snapshot: Mutex::new(None),
            callback_idle: Mutex::new(None),
            clock_panes: Mutex::new(None),
            hand_off: None,
            inbox_pool: Mutex::new(None),
            slots: None,
        }
    }

    /// This store, its starts held to `slots` open tasks an orchestrator: a claim, a move into
    /// in_progress and taking the orchestrator role on open work are refused past it
    /// ([`crate::slots`]). None caps nothing, as a store opens.
    pub fn capping(mut self, slots: Option<usize>) -> Store {
        self.slots = slots;
        self
    }

    /// The cap this store holds starts to, if any ([`Store::capping`]).
    pub fn slots(&self) -> Option<usize> {
        self.slots
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    /// The time by this store's clock.
    pub fn now(&self) -> f64 {
        (self.clock)()
    }

    /// Every task and session, with health, in one round trip.
    pub fn board(&self) -> Result<Board, Error> {
        let view = self.retrying(|| self.view(&Which::All))?;
        Ok(Self::board_view(view))
    }

    fn board_view(view: View) -> Board {
        let sessions = view
            .snap
            .sessions
            .iter()
            .map(|row| Session {
                row: row.clone(),
                health: view.health(&row.handle),
            })
            .collect();
        Board {
            tasks: view.tasks(),
            sessions,
            latest_message: view.snap.latest,
        }
    }

    /// The tasks, or those in `status`, by priority then creation.
    pub fn tasks(&self, status: Option<&str>) -> Result<Vec<Task>, Error> {
        let which = status.map_or(Which::All, Which::Status);
        Ok(self.retrying(|| self.view(&which))?.tasks())
    }

    /// Eligibility without transferring briefs, roles or session health for the entire board.
    pub(crate) fn task_slugs(&self, status: Option<&str>) -> Result<Vec<String>, Error> {
        let sql = if status.is_some() {
            "RETURN SELECT VALUE slug FROM task WHERE status = $status;"
        } else {
            "RETURN SELECT VALUE slug FROM task;"
        };
        self.retrying(|| self.transact(sql, json!({"status":status})))
    }

    pub(crate) fn tasks_by_slug(&self, slugs: &[String]) -> Result<Vec<Task>, Error> {
        if slugs.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self.retrying(|| self.view(&Which::Slugs(slugs)))?.tasks())
    }

    pub fn get(&self, slug: &str) -> Result<Task, Error> {
        let view = self.retrying(|| self.view(&Which::Slug(slug)))?;
        view.task(slug)?;
        Ok(view.tasks().remove(0))
    }

    pub fn session(&self, handle: &str) -> Result<Session, Error> {
        let found: Found = self.retrying(|| {
            self.transact(
                &session_read(),
                json!({"handle": handle, "executing": EXECUTING}),
            )
        })?;
        self.found(handle, found)
    }

    /// The session `session_read` found as `handle`, with its health.
    fn found(&self, handle: &str, found: Found) -> Result<Session, Error> {
        let row = found
            .session
            .ok_or_else(|| refused(format!("no session {}", repr(handle))))?;
        let health = health::health(
            &row,
            (self.clock)(),
            &self.host,
            &*self.pid_alive,
            !found.held.is_empty(),
        );
        let health = native_health(&row, health, self.now(), found.native_until);
        Ok(Session { row, health })
    }

    /// The tasks another session could take over if `handle` were gone.
    pub fn claimable(&self, handle: &str) -> Result<Vec<String>, Error> {
        let sql = format!("RETURN {CLAIMABLE};");
        let mut slugs: Vec<String> = self
            .retrying(|| self.transact(&sql, json!({"handle": handle, "executing": EXECUTING})))?;
        slugs.sort();
        slugs.dedup();
        Ok(slugs)
    }

    pub fn thread(&self, slug: &str) -> Result<Vec<Message>, Error> {
        #[derive(Deserialize)]
        struct Thread {
            known: bool,
            messages: Vec<MessageRow>,
        }
        let sql = "RETURN { known: record::exists(type::record('task', $slug)),
                            messages: (SELECT * OMIT id FROM message:[$slug, NONE]..[$slug, ..]) };";
        let thread: Thread = self.retrying(|| self.transact(sql, json!({"slug": slug})))?;
        if !thread.known {
            return Err(refused(format!("no task {}", repr(slug))));
        }
        Ok(thread.messages.into_iter().map(message).collect())
    }

    /// The threads of `slugs`, each in order, in one round trip; a slug no task has is refused, as
    /// [`Store::thread`] refuses it.
    pub fn threads(&self, slugs: &[String]) -> Result<BTreeMap<String, Vec<Message>>, Error> {
        #[derive(Deserialize)]
        struct Threads {
            unknown: Vec<String>,
            threads: Vec<Vec<MessageRow>>,
        }
        let sql = "RETURN {
    unknown: $slugs.filter(|$slug| !record::exists(type::record('task', $slug))),
    threads: $slugs.map(|$slug| (SELECT * OMIT id FROM message:[$slug, NONE]..[$slug, ..])),
};";
        let read: Threads = self.retrying(|| self.transact(sql, json!({"slugs": slugs})))?;
        if let Some(slug) = read.unknown.first() {
            return Err(refused(format!("no task {}", repr(slug))));
        }
        Ok(slugs
            .iter()
            .cloned()
            .zip(
                read.threads
                    .into_iter()
                    .map(|rows| rows.into_iter().map(message).collect()),
            )
            .collect())
    }

    /// Become the executor. Of any number of claims at once, one wins. A claim that starts ready
    /// work is refused when it would raise an orchestrator's open tasks past the store's cap
    /// ([`crate::slots`]).
    pub fn claim(&self, slug: &str, handle: &str) -> Result<Task, Error> {
        self.ruling(slug, handle, self.slots.is_some(), |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            if !EXECUTING.contains(&task.status.as_str()) {
                return Err(refused(format!(
                    "{slug} is {}; only concept, design or ready work can be claimed",
                    task.status
                )));
            }
            if !view.snap.open_deps.is_empty() {
                let open: Vec<&str> = view
                    .snap
                    .open_deps
                    .iter()
                    .map(|l| l.target.as_str())
                    .collect();
                return Err(refused(format!("{slug} waits on {}", open.join(", "))));
            }
            if view.holds(handle, "reviewer") {
                return Err(refused(format!(
                    "{handle} reviews {slug} and cannot also execute it"
                )));
            }
            let mut change = Change::default();
            let mut handed = Vec::new();
            for other in view.holders("executor").filter(|r| r.handle != handle) {
                let health = view.health(&other.handle);
                if health.holds() {
                    return Err(refused(format!(
                        "{slug} is held by {} ({})",
                        other.handle,
                        health.as_str()
                    )));
                }
                change.saw(view, &other.handle);
                change.drop(&other.handle, "executor");
                handed.push(format!("{} ({})", other.handle, health.as_str()));
            }
            if view.holds(handle, "executor") {
                return Ok(change);
            }
            let kept = view.lapse_reservations(&mut change);
            if let Some(other) = kept.iter().find(|r| r.handle != handle) {
                return Err(refused(format!("{slug} is reserved by {}", other.handle)));
            }
            for mine in kept {
                change.drop(&mine.handle, "reserved");
            }
            change.add = Some(RoleRow {
                slug: slug.into(),
                handle: handle.into(),
                role: "executor".into(),
                since: view.now,
                expires: None,
            });
            if task.status == "ready" {
                change.status = Some("in_progress".into());
                change.opens = Some(Opens {
                    status: change.status.clone(),
                    by: vec![handle.into()],
                    what: format!("{slug} cannot start"),
                    ..Opens::default()
                });
            }
            let mut text = format!("{handle} claimed it");
            if !handed.is_empty() {
                let _ = write!(text, ", handed over from {}", handed.join(", "));
            }
            change.say = Say::system(text);
            Ok(change)
        })
    }

    /// Book a task ahead of time so nobody else claims it; it lapses after `ttl` seconds or if the
    /// session is lost.
    pub fn reserve(&self, slug: &str, handle: &str, ttl: f64) -> Result<Task, Error> {
        self.rule(slug, handle, |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            if task.status == "done" || task.status == "cancelled" {
                return Err(refused(format!("{slug} is {}", task.status)));
            }
            let mut change = Change::default();
            for other in view.holders("executor").filter(|r| r.handle != handle) {
                if view.health(&other.handle).holds() {
                    return Err(refused(format!(
                        "{slug} is already claimed by {}",
                        other.handle
                    )));
                }
                change.saw(view, &other.handle);
            }
            let kept = view.lapse_reservations(&mut change);
            if let Some(other) = kept.iter().find(|r| r.handle != handle) {
                return Err(refused(format!("{slug} is reserved by {}", other.handle)));
            }
            let (since, expires) = (view.now, Some(view.now + ttl));
            change.add = Some(RoleRow {
                slug: slug.into(),
                handle: handle.into(),
                role: "reserved".into(),
                since,
                expires,
            });
            change.say = Say::system(format!("{handle} reserved it"));
            Ok(change)
        })
    }

    /// A reviewer's `approve` (the task is done) or `changes` (back to its executor).
    pub fn verdict(
        &self,
        slug: &str,
        handle: &str,
        verdict: &str,
        text: &str,
    ) -> Result<Task, Error> {
        if verdict != "approve" && verdict != "changes" {
            return Err(refused(format!(
                "a verdict is approve or changes, not {}",
                repr(verdict)
            )));
        }
        let said = format!("{verdict}: {}", text.trim())
            .trim_end_matches([':', ' '])
            .to_string();
        let task = self.rule(slug, handle, |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            if task.status != "review" {
                return Err(refused(format!("{slug} is {}, not in review", task.status)));
            }
            if !view.holds(handle, "reviewer") {
                return Err(refused(format!("{handle} is not a reviewer of {slug}")));
            }
            let approve = verdict == "approve";
            Ok(Change {
                status: Some(if approve { "done" } else { "in_progress" }.into()),
                say: Some(Say {
                    kind: "verdict",
                    text: said.clone(),
                    recipient: (!approve).then(|| "@executor".into()),
                }),
                ..Change::default()
            })
        })?;
        if verdict == "changes" {
            let what = format!("the changes verdict from {handle} to @executor");
            self.tell_if_unread(slug, None, &what, None);
        }
        Ok(task)
    }

    /// The executor of an instruction subtask closes it, the note being its result.
    pub fn done(&self, slug: &str, handle: &str, note: &str) -> Result<Task, Error> {
        if note.trim().is_empty() {
            return Err(refused(
                "closing a task needs a note saying what came of it".into(),
            ));
        }
        self.rule(slug, handle, |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            if !task.instruction || task.parent.is_none() {
                return Err(refused(format!(
                    "{slug} is not an instruction subtask; it closes by a verdict or a landing"
                )));
            }
            if task.status != "in_progress" {
                return Err(refused(format!(
                    "{slug} is {}, not in progress",
                    task.status
                )));
            }
            if !view.holds(handle, "executor") {
                return Err(refused(format!("{handle} is not the executor of {slug}")));
            }
            Ok(Change {
                status: Some("done".into()),
                say: Some(Say {
                    kind: "verdict",
                    text: format!("done: {}", note.trim()),
                    recipient: None,
                }),
                ..Change::default()
            })
        })
    }

    /// Read `slug`, `decide`, write what was decided; again from the read whenever the write met
    /// another or found the board moved. Then the task as the write left it.
    fn rule(
        &self,
        slug: &str,
        actor: &str,
        decide: impl Fn(&View) -> Result<Change, Error>,
    ) -> Result<Task, Error> {
        self.ruling(slug, actor, false, decide)
    }

    /// As [`Store::rule`]; one that `starts` work ([`Opens`]) under a cap also reads what the cap
    /// decides on, in the same transaction as its task ([`cap`]).
    fn ruling(
        &self,
        slug: &str,
        actor: &str,
        starts: bool,
        decide: impl Fn(&View) -> Result<Change, Error>,
    ) -> Result<Task, Error> {
        let which = if starts {
            Which::Starting(slug)
        } else {
            Which::Slug(slug)
        };
        self.retrying(|| {
            let view = self.view(&which)?;
            let change = decide(&view).map_err(Failure::Stop)?;
            let slots = match (&change.opens, self.slots) {
                (Some(opens), Some(cap)) => self.slots_taken(&view, slug, opens, cap)?,
                _ => Vec::new(),
            };
            let version = view.snap.tasks.first().map_or(0, |t| t.version);
            let mut body = if change.status.is_some() {
                format!(
                    "{WRITE}{}\n{}",
                    cap::HOLDING,
                    work::block_cancelled_dependencies(
                        change.status.as_deref() == Some("cancelled")
                    )
                )
            } else {
                WRITE.into()
            };
            if !slots.is_empty() {
                body.push_str(cap::GUARD);
            }
            let mut vars = change.vars(slug, actor, version, view.now);
            vars["slots"] = Value::Array(slots);
            self.written(&body, vars, slug)
        })
    }

    #[track_caller]
    fn view(&self, which: &Which) -> Result<View, Failure> {
        let (read, vars) = Self::reading(which);
        let snap: Snapshot = self.transact(&read, vars)?;
        Ok(View::new(self, snap))
    }

    /// `body`'s writes, then `slug` read as `get` reads it, in one transaction: the task as the write
    /// left it, in the write's one round trip. `body` ends without a `RETURN`.
    #[track_caller]
    fn written(&self, body: &str, mut vars: Value, slug: &str) -> Result<Task, Failure> {
        let (read, more) = Self::reading(&Which::Slug(slug));
        if let (Some(vars), Value::Object(more)) = (vars.as_object_mut(), more) {
            vars.extend(more);
        }
        // Include recipient presence in the write's existing result instead of a separate read.
        let snap: Snapshot = self.transact_dispatch(
            &format!("{body}\nLET $has_callbacks = array::len(SELECT handle FROM callback_state WHERE address != NONE AND type::record('session', handle).left_at = NONE LIMIT 1) > 0;\n{read}"),
            vars,
            false,
        )?;
        if snap.has_callbacks {
            self.after_write();
        }
        let view = View::new(self, snap);
        view.task(slug).map_err(Failure::Stop)?;
        Ok(view.tasks().remove(0))
    }

    /// The statements that read `which`, ending in the `RETURN` of a `Snapshot`, and their variables.
    fn reading(which: &Which) -> (String, Value) {
        const SCOPED: &str = " WHERE slug IN $slugs";
        const KNOWN: &str = "record::exists(type::record('task', $slug))";
        let (pick, scope, known, mut vars) = match which {
            Which::All => (
                "SELECT * OMIT id FROM task ORDER BY priority, sid",
                "",
                "true",
                json!({}),
            ),
            Which::Status(status) => (
                "SELECT * OMIT id FROM task WHERE status = $status ORDER BY priority, sid",
                SCOPED,
                "true",
                json!({"status": status}),
            ),
            Which::Slug(slug) | Which::Starting(slug) => (
                "SELECT * OMIT id FROM type::record('task', $slug)",
                SCOPED,
                KNOWN,
                json!({"slug": slug}),
            ),
            Which::Slugs(slugs) => (
                "SELECT * OMIT id FROM $wanted.map(|$slug| type::record('task', $slug)) ORDER BY priority, sid",
                SCOPED,
                "true",
                json!({"wanted": slugs}),
            ),
            Which::Children(slug) => (
                "SELECT * OMIT id FROM task WHERE parent = $slug ORDER BY sid",
                SCOPED,
                KNOWN,
                json!({"slug": slug}),
            ),
            Which::Pullable(handle) => (
                "SELECT * OMIT id FROM task WHERE status IN $pullable ORDER BY sid",
                SCOPED,
                "record::exists(type::record('session', $handle))",
                json!({"handle": handle, "pullable": PULLABLE}),
            ),
            Which::Linked(doc) => (
                "SELECT * OMIT id FROM array::distinct(SELECT VALUE slug FROM link
                     WHERE kind = 'doc' AND (target = $doc OR string::starts_with(target, $doc + '#')))
                     .map(|$s| type::record('task', $s)) ORDER BY priority, slug",
                SCOPED,
                "true",
                json!({"doc": doc}),
            ),
        };
        let (deps, mine) = match which {
            Which::Slug(_) | Which::Starting(_) | Which::Pullable(_) => (
                "(SELECT slug, kind, target FROM link WHERE slug IN $slugs AND kind = 'dep'
                  AND type::record('task', target).status NOT IN [NONE, 'done'] ORDER BY target)",
                if matches!(which, Which::Pullable(_)) {
                    "(SELECT VALUE slug FROM role WHERE handle = $handle)"
                } else {
                    "[]"
                },
            ),
            _ => ("[]", "[]"),
        };
        let (slot_lets, slots) = if matches!(which, Which::Starting(_)) {
            if let (Some(vars), Value::Object(more)) = (vars.as_object_mut(), cap::vars()) {
                vars.extend(more);
            }
            (cap::lets(), cap::field())
        } else {
            (String::new(), String::new())
        };
        vars["executing"] = json!(EXECUTING);
        native_bounds(&mut vars);
        let read = format!(
            "LET $tasks = ({pick});
             LET $slugs = $tasks.slug;
             {HELD}
             {slot_lets}
             RETURN {{
                 tasks: $tasks,
                 links: (SELECT slug, kind, target FROM link{scope} ORDER BY target),
                 roles: (SELECT slug, handle, role, since, expires FROM role{scope} ORDER BY since, handle),
                 sessions: (SELECT * OMIT id FROM session ORDER BY connected, handle),
                 held: $held,
                 latest: (SELECT VALUE last FROM ONLY counter:message) ?? 0,
                 callback_sequence: $callback_sequence ?? 0,
                 has_callbacks: $has_callbacks ?? false,
                 native_presence: {NATIVE_PRESENCE},
                 open_deps: {deps},
                 known: {known},
                 mine: {mine},
                 {slots}
             }};"
        );
        (read, vars)
    }

    fn retrying<T>(&self, mut once: impl FnMut() -> Result<T, Failure>) -> Result<T, Error> {
        let began = Instant::now();
        let mut attempt = 0;
        loop {
            match once() {
                Ok(done) => return Ok(done),
                Err(Failure::Stop(error)) => return Err(error),
                Err(Failure::Again) if began.elapsed() < PATIENCE => {
                    std::thread::sleep(backoff(attempt));
                    attempt += 1;
                }
                Err(Failure::Again) => {
                    return Err(Error::Db(format!(
                        "gave up after {} s and {} tries: the board kept moving under the write",
                        PATIENCE.as_secs(),
                        attempt + 1
                    )));
                }
            }
        }
    }
}

/// One session, `$handle`, and whether it holds work another could take over (a `Found`). Needs
/// `$executing`.
fn session_read() -> String {
    format!(
        "LET $c = (SELECT address, confirmed_until FROM ONLY type::record('callback_state', $handle));
         RETURN {{ session: (SELECT * OMIT id FROM ONLY type::record('session', $handle)), held: {CLAIMABLE},
             native_until: IF string::starts_with($c.address ?? '', 'codex:') {{ $c.confirmed_until ?? 0 }} ELSE {{ NONE }} }};"
    )
}

#[derive(Deserialize)]
struct Found {
    session: Option<SessionRow>,
    held: Vec<String>,
    native_until: Option<f64>,
}

/// A completed sweep either confirms a Codex pane or retires its callback. An expired registration
/// therefore means the sweep did not establish absence, even if ordinary heartbeats have expired.
/// Keep that uncertainty for at most another lost interval; a stalled sweep cannot hold work forever.
fn native_health(row: &SessionRow, health: Health, now: f64, until: Option<f64>) -> Health {
    if row.left_at.is_none()
        && health != Health::Live
        && until.is_some_and(|until| {
            until < now && now - until <= health::LIVE_SECONDS + health::STALE_SECONDS
        })
    {
        Health::Unknown
    } else {
        health
    }
}

/// `$held`: every session holding work another could take over (`CLAIMABLE`, for everyone), for
/// the `lost` line of health. Needs `$executing`. The roles are read by their tasks' slugs, through
/// `role_slug`, and kept by role after: one condition over both would read every role.
const HELD: &str = "LET $open = SELECT VALUE slug FROM task WHERE status IN $executing;
LET $waiting = SELECT VALUE slug FROM task WHERE status = 'review';
LET $held = array::distinct((SELECT handle, role, slug FROM role WHERE slug IN array::concat($open, $waiting))
    .filter(|$r| ($r.role = 'executor' AND $r.slug IN $open) OR ($r.role = 'reviewer' AND $r.slug IN $waiting))
    .map(|$r| $r.handle));";

/// The Codex registrations' presence, through `callback_state_address`: an address starting
/// `codex:` is one from `$codex` up to `$codex_after`, the bounds [`native_bounds`] gives. They
/// are variables, not text: the client takes a request for a read only when no `;` but those
/// between its statements is in it (`rpc::Kind::of` splits on every one, quoted or not), and a
/// read it took for a write would never be sent again when late.
pub(super) const NATIVE_PRESENCE: &str = "(SELECT handle, confirmed_until ?? 0 AS until FROM callback_state WHERE address >= $codex AND address < $codex_after)";

/// `vars` with [`NATIVE_PRESENCE`]'s bounds: `codex:`, and `codex;`, the character after the colon.
pub(super) fn native_bounds(vars: &mut Value) {
    vars["codex"] = json!("codex:");
    vars["codex_after"] = json!("codex;");
}

/// The tasks `$handle` holds that another could take over (`health::claimable`, in the database).
const CLAIMABLE: &str = "(SELECT VALUE slug FROM role WHERE handle = $handle
    AND ((role = 'executor' AND type::record('task', slug).status IN $executing)
      OR (role = 'reviewer' AND type::record('task', slug).status = 'review')))";

/// A rule's write: stale unless the actor may still act, the task is the version read and every
/// session taken to be gone is as it was read; then the actor is seen, the roles and status change,
/// the task's version moves on, and what happened goes on the record.
const WRITE: &str = "
LET $me = (SELECT left_at FROM ONLY type::record('session', $actor));
IF $me = NONE OR $me.left_at != NONE { THROW 'stale' };
IF (SELECT VALUE version FROM ONLY type::record('task', $slug)) != $version { THROW 'stale' };
FOR $s IN $seen {
    IF (SELECT VALUE last_seen FROM ONLY type::record('session', $s.handle)) != $s.last_seen { THROW 'stale' };
};
UPDATE type::record('session', $actor) SET last_seen = math::max([last_seen, $now]) RETURN NONE;
FOR $r IN $drop { DELETE type::record('role', [$slug, $r.handle, $r.role]); };
FOR $r IN $add { IF type::record('session', $r.handle).left_at != NONE { THROW 'stale' }; UPSERT type::record('role', [$slug, $r.handle, $r.role]) CONTENT $r RETURN NONE; };
UPDATE type::record('task', $slug) SET version += 1, status = $status ?? status, updated = $updated ?? updated RETURN NONE;
IF $say {
    LET $counted = (UPSERT ONLY counter:message SET last += 1, ts = IF ts != NONE AND ts > $now AND ts <= $now + 60 { ts } ELSE { $now });
    LET $sid = $counted.last;
    LET $ts = $counted.ts;
    CREATE type::record('message', [$slug, $sid]) SET sid = $sid, slug = $slug, sender = $actor, recipient = $say.recipient,
        kind = $say.kind, text = $say.text, ts = $ts RETURN NONE;
};";

enum Which<'a> {
    All,
    Status(&'a str),
    Slug(&'a str),
    /// One task, as `Slug`, with what the cap on an orchestrator's open tasks decides a start on.
    Starting(&'a str),
    Slugs(&'a [String]),
    /// The tasks right under one, in the order they were made.
    Children(&'a str),
    /// The tasks linked to a document or one of its headings, by priority then slug.
    Linked(&'a str),
    /// The concept, design and ready tasks, in the order made, and what the session named holds.
    Pullable(&'a str),
}

/// Why one try of a call ended without its result.
enum Failure {
    /// The board moved under it: read and decide again.
    Again,
    Stop(Error),
}

impl From<DbError> for Failure {
    fn from(error: DbError) -> Failure {
        match error {
            DbError::Conflict => Failure::Again,
            DbError::Thrown(why) if why == STALE => Failure::Again,
            DbError::Thrown(why) if why.starts_with(REFUSED) => {
                Failure::Stop(Error::Refused(why[REFUSED.len()..].to_string()))
            }
            other => Failure::Stop(Error::Db(other.to_string())),
        }
    }
}

/// What a read gives back: the tasks asked for, their links and roles, every session, who holds
/// claimable work, and the newest message's id.
#[derive(Clone, Deserialize)]
struct Snapshot {
    native_presence: Vec<NativePresence>,
    callback_sequence: u64,
    has_callbacks: bool,
    tasks: Vec<TaskRecord>,
    links: Vec<LinkRow>,
    roles: Vec<RoleRow>,
    sessions: Vec<SessionRow>,
    held: Vec<String>,
    latest: i64,
    /// What the tasks read wait on that is not done (`Which::Slug`, `Which::Pullable`).
    open_deps: Vec<LinkRow>,
    /// The tasks the session asking holds a role on (`Which::Pullable`).
    mine: Vec<String>,
    /// The task (`Which::Slug`, `Which::Children`) or session (`Which::Pullable`) the read is about
    /// is there.
    known: bool,
    /// What the cap decides a start on (`Which::Starting`).
    #[serde(default)]
    slots: Option<cap::Read>,
}

#[derive(Clone, Deserialize)]
struct NativePresence {
    handle: String,
    until: f64,
}

/// A task as the board holds it. The row refuses a field it does not know, but flattened it is handed
/// only its own, so `version` is the record's; every read of a task goes through here.
#[derive(Clone, Deserialize)]
struct TaskRecord {
    #[serde(flatten)]
    row: TaskRow,
    version: i64,
}

/// A snapshot, as of the clock when it was read, with each session's health worked out once.
struct View {
    snap: Snapshot,
    now: f64,
    sessions: HashMap<String, (usize, Health)>,
}

impl View {
    fn new(store: &Store, snap: Snapshot) -> View {
        let now = (store.clock)();
        let held: HashSet<&str> = snap.held.iter().map(String::as_str).collect();
        let native: HashMap<&str, f64> = snap
            .native_presence
            .iter()
            .map(|c| (c.handle.as_str(), c.until))
            .collect();
        let sessions = snap
            .sessions
            .iter()
            .enumerate()
            .map(|(at, row)| {
                let health = health::health(
                    row,
                    now,
                    &store.host,
                    &*store.pid_alive,
                    held.contains(row.handle.as_str()),
                );
                let health =
                    native_health(row, health, now, native.get(row.handle.as_str()).copied());
                (row.handle.clone(), (at, health))
            })
            .collect();
        View {
            snap,
            now,
            sessions,
        }
    }

    /// A role held by a session the board does not know is lost.
    fn health(&self, handle: &str) -> Health {
        self.sessions
            .get(handle)
            .map_or(Health::Lost, |&(_, health)| health)
    }

    fn session(&self, handle: &str) -> Option<&SessionRow> {
        self.sessions
            .get(handle)
            .map(|&(at, _)| &self.snap.sessions[at])
    }

    /// A session that may act: it exists and has not left.
    fn actor(&self, handle: &str) -> Result<(), Error> {
        match self.session(handle) {
            None => Err(refused(unknown_actor(handle))),
            Some(row) if row.left_at.is_some() => Err(refused(left_actor(handle))),
            Some(_) => Ok(()),
        }
    }

    fn task(&self, slug: &str) -> Result<&TaskRow, Error> {
        let found = self.snap.tasks.iter().find(|t| t.row.slug == slug);
        found
            .map(|t| &t.row)
            .ok_or_else(|| refused(format!("no task {}", repr(slug))))
    }

    /// The one task read's holders of `role`.
    fn holders<'v>(&'v self, role: &'v str) -> impl Iterator<Item = &'v RoleRow> {
        self.snap.roles.iter().filter(move |r| r.role == role)
    }

    fn holds(&self, handle: &str, role: &str) -> bool {
        self.holders(role).any(|r| r.handle == handle)
    }

    /// The reservations that still count; those past their time or held by a session gone are dropped
    /// in `change`.
    fn lapse_reservations(&self, change: &mut Change) -> Vec<&RoleRow> {
        let mut kept = Vec::new();
        for held in self.holders("reserved") {
            let lapsed = held.expires.is_some_and(|expires| expires < self.now);
            if lapsed || matches!(self.health(&held.handle), Health::Lost | Health::Left) {
                if !lapsed {
                    change.saw(self, &held.handle);
                }
                change.drop(&held.handle, "reserved");
            } else {
                kept.push(held);
            }
        }
        kept
    }

    /// The tasks read, each with its links and its holders' health, a lapsed reservation left out.
    fn tasks(&self) -> Vec<Task> {
        let mut links: HashMap<&str, Vec<&LinkRow>> = HashMap::new();
        for link in &self.snap.links {
            links.entry(link.slug.as_str()).or_default().push(link);
        }
        let mut roles: HashMap<&str, Vec<&RoleRow>> = HashMap::new();
        for role in &self.snap.roles {
            roles.entry(role.slug.as_str()).or_default().push(role);
        }
        let targets = |slug: &str, kind: &str| -> Vec<String> {
            links
                .get(slug)
                .into_iter()
                .flatten()
                .filter(|l| l.kind == kind)
                .map(|l| l.target.clone())
                .collect()
        };
        self.snap
            .tasks
            .iter()
            .map(|TaskRecord { row, .. }| Task {
                id: row.id,
                slug: row.slug.clone(),
                title: row.title.clone(),
                body: row.body.clone(),
                status: row.status.clone(),
                priority: row.priority,
                parent: row.parent.clone(),
                created_by: row.created_by.clone(),
                created: row.created,
                updated: row.updated,
                links: targets(&row.slug, "doc"),
                depends: targets(&row.slug, "dep"),
                roles: roles
                    .get(row.slug.as_str())
                    .into_iter()
                    .flatten()
                    .filter(|r| {
                        !(r.role == "reserved"
                            && r.expires.is_some_and(|expires| expires < self.now))
                    })
                    .map(|r| Role {
                        handle: r.handle.clone(),
                        role: r.role.clone(),
                        since: r.since,
                        expires: r.expires,
                        health: self.health(&r.handle),
                    })
                    .collect(),
                instruction: row.instruction,
                fields: row.fields.clone(),
            })
            .collect()
    }
}

/// What a rule writes, decided on a read of its task.
#[derive(Default)]
struct Change {
    /// Sessions whose being gone let the rule go ahead, with their `last_seen` as read.
    seen: Vec<(String, f64)>,
    /// Roles to drop from the task: a handle and a role.
    drop: Vec<(String, String)>,
    add: Option<RoleRow>,
    status: Option<String>,
    /// The move is a hold, which keeps the task's slot while it is blocked ([`cap`]).
    hold: bool,
    say: Option<Say>,
    /// Work it starts, which takes a slot of an orchestrator's under the store's cap.
    opens: Option<Opens>,
}

/// A start a rule makes: a claim or a move into an open status, the orchestrator role taken, or a
/// change of parent of open work. Under a cap it is refused when it would raise an orchestrator's
/// open tasks past it ([`crate::slots::started`]).
#[derive(Default)]
struct Opens {
    /// The status it moves the task to, when it moves it.
    status: Option<String>,
    /// The session taking the orchestrator role on the task, when that is the start.
    orchestrator: Option<String>,
    /// The task's new parent, or none, when that is the change.
    parent: Option<Option<String>>,
    /// The sessions whose parents stand in for an orchestrator when none holds the task: its
    /// executor once it starts.
    by: Vec<String>,
    /// What the refusal says cannot happen.
    what: String,
}

struct Say {
    kind: &'static str,
    text: String,
    recipient: Option<String>,
}

impl Say {
    /// A line of the board's own record, to everyone on the task.
    fn system(text: String) -> Option<Say> {
        Some(Say {
            kind: "system",
            text,
            recipient: None,
        })
    }
}

impl Change {
    fn saw(&mut self, view: &View, handle: &str) {
        if let Some(row) = view.session(handle) {
            self.seen.push((handle.into(), row.last_seen));
        }
    }

    fn drop(&mut self, handle: &str, role: &str) {
        self.drop.push((handle.into(), role.into()));
    }

    fn vars(&self, slug: &str, actor: &str, version: i64, now: f64) -> Value {
        let say = self.say.as_ref().map(|say| {
            let mut said = json!({"kind": say.kind, "text": say.text});
            if let Some(recipient) = &say.recipient {
                said["recipient"] = json!(recipient);
            }
            said
        });
        json!({
            "slug": slug,
            "actor": actor,
            "version": version,
            "now": now,
            "seen": self.seen.iter().map(|(handle, at)| json!({"handle": handle, "last_seen": at})).collect::<Vec<_>>(),
            "drop": self.drop.iter().map(|(handle, role)| json!({"handle": handle, "role": role})).collect::<Vec<_>>(),
            "add": self.add.iter().collect::<Vec<_>>(),
            "status": self.status,
            "hold": self.hold,
            "updated": self.status.as_ref().map(|_| now),
            "say": say,
        })
    }
}

fn refused(message: String) -> Error {
    Error::Refused(message)
}

fn unknown_actor(handle: &str) -> String {
    format!(
        "no session {}; attach first (just tasks-attach)",
        repr(handle)
    )
}

fn left_actor(handle: &str) -> String {
    format!("session {handle} has left; attach again")
}

/// The actor may act, as the write runs: it exists and has not left, or the write is refused; and
/// acting is being seen. Its variables are `acting`'s.
const ACT: &str = "
LET $me = (SELECT left_at FROM ONLY type::record('session', $actor));
IF $me = NONE { THROW $unknown_actor };
IF $me.left_at != NONE { THROW $left_actor };
UPDATE type::record('session', $actor) SET last_seen = math::max([last_seen, $now]) RETURN NONE;";

/// `ACT`'s variables, and `more`'s.
fn acting(actor: &str, now: f64, more: Value) -> Value {
    let mut vars = json!({
        "actor": actor,
        "now": now,
        "unknown_actor": format!("{REFUSED}{}", unknown_actor(actor)),
        "left_actor": format!("{REFUSED}{}", left_actor(actor)),
    });
    if let (Some(vars), Value::Object(more)) = (vars.as_object_mut(), more) {
        vars.extend(more);
    }
    vars
}

/// Statements that put a line on a task's thread at `$now`, each argument a SurrealQL expression. A line is
/// never dated before the one numbered ahead of it when that one is less than a minute ahead (a write reads
/// its clock at most `PATIENCE` before it commits); one further ahead came from a wrong clock, and the
/// lines after it keep their own dates rather than all take its.
fn logged(slug: &str, sender: &str, kind: &str, text: &str, recipient: &str) -> String {
    format!(
        "LET $counted = (UPSERT ONLY counter:message SET last += 1, ts = IF ts != NONE AND ts > $now AND ts <= $now + 60 {{ ts }} ELSE {{ $now }});
LET $sid = $counted.last;
LET $ts = $counted.ts;
CREATE type::record('message', [{slug}, $sid]) SET sid = $sid, slug = {slug}, sender = {sender}, recipient = {recipient},
    kind = {kind}, text = {text}, ts = $ts RETURN NONE;"
    )
}

fn message(row: MessageRow) -> Message {
    Message {
        id: row.id,
        slug: row.slug,
        sender: row.sender,
        recipient: row.recipient,
        kind: row.kind,
        text: row.text,
        ts: row.ts,
    }
}

fn values<T: serde::Serialize>(rows: &[T]) -> Vec<Value> {
    rows.iter()
        .map(|row| serde_json::to_value(row).expect("a row is plain data"))
        .collect()
}

pub(crate) fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs_f64())
}

/// A random wait of up to 2^attempt ms (at most 64), so writers that met do not meet again.
fn backoff(attempt: u32) -> Duration {
    let ceiling = 1000u64 << attempt.min(6);
    Duration::from_micros(RandomState::new().hash_one(attempt) % ceiling)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_expired_native_signal_never_overrules_a_heartbeat_or_an_explicit_departure() {
        let row = SessionRow::default();
        for (health, until, want) in [
            (Health::Live, Some(0.0), Health::Live),
            (Health::Lost, Some(100.0), Health::Lost),
            (Health::Stale, Some(99.0), Health::Unknown),
            (Health::Lost, Some(99.0), Health::Unknown),
            (Health::Left, Some(99.0), Health::Unknown),
            (Health::Lost, None, Health::Lost),
        ] {
            assert_eq!(native_health(&row, health, 100.0, until), want);
        }
        let left = SessionRow {
            left_at: Some(1.0),
            ..row
        };
        assert_eq!(
            native_health(&left, Health::Left, 100.0, Some(0.0)),
            Health::Left
        );
        assert!(Health::Unknown.holds());
        assert_eq!(Health::Unknown.as_str(), "unknown");
    }

    #[test]
    fn native_uncertainty_expires_without_overruling_the_heartbeat_health() {
        let row = SessionRow::default();
        for health in [Health::Live, Health::Stale, Health::Lost, Health::Left] {
            let protected = if health == Health::Live {
                health
            } else {
                Health::Unknown
            };
            for (now, expected) in [(699.0, protected), (700.0, protected), (701.0, health)] {
                assert_eq!(native_health(&row, health, now, Some(100.0)), expected);
            }
        }
    }

    #[test]
    fn a_name_is_quoted_as_python_quotes_it() {
        assert_eq!(repr("nobody"), "'nobody'");
        assert_eq!(repr(""), "''");
        assert_eq!(repr("it's"), "\"it's\"");
        assert_eq!(repr("it's \"x\""), "'it\\'s \"x\"'");
        assert_eq!(repr("a\\b\nc\td\r\u{1}"), "'a\\\\b\\nc\\td\\r\\x01'");
        assert_eq!(repr("sim/jump — ✓"), "'sim/jump — ✓'");
    }

    #[test]
    fn only_a_namespace_or_this_database_not_on_the_server_reads_as_a_board_not_made_yet() {
        assert!(undefined("The namespace 'sf' does not exist", "tasks"));
        assert!(undefined("The database 'tasks' does not exist", "tasks"));
        for other in [
            "The database 'other' does not exist",
            "The table 'meta' does not exist",
            "The namespace 'sf' already exists",
            "The namespace 'sf",
            "the server at 127.0.0.1:1 is slow: The database 'tasks' does not exist",
        ] {
            assert!(!undefined(other, "tasks"), "{other}");
        }
    }

    /// The open's check of the version is a read, so a late answer to it goes once more.
    #[test]
    fn the_schema_version_check_is_a_read() {
        assert_eq!(crate::rpc::Kind::of(SCHEMA_VERSION), crate::rpc::Kind::Read);
    }

    /// Each snapshot a page or a list reads is a read, so a late answer to it goes once more: its
    /// bounds are variables, so no `;` stands in it but between its statements.
    #[test]
    fn every_snapshot_read_is_a_read_and_binds_its_bounds() {
        let slugs = ["p/a".to_string()];
        for which in [
            Which::All,
            Which::Status("ready"),
            Which::Slug("p/a"),
            Which::Slugs(&slugs),
            Which::Children("p/a"),
            Which::Linked("docs/x.md"),
            Which::Pullable("someone"),
        ] {
            let (read, vars) = Store::reading(&which);
            assert_eq!(
                crate::rpc::Kind::of(&read),
                crate::rpc::Kind::Read,
                "{read}"
            );
            assert_eq!(
                (vars["codex"].as_str(), vars["codex_after"].as_str()),
                (Some("codex:"), Some("codex;")),
                "{vars}"
            );
        }
    }

    #[test]
    fn a_refusal_is_its_words_and_a_database_failure_says_so() {
        assert_eq!(
            Error::Refused("no task 'x'".into()).to_string(),
            "no task 'x'"
        );
        assert_eq!(
            Error::Db("cannot reach it".into()).to_string(),
            "SurrealDB: cannot reach it"
        );
    }
}
