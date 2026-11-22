//! The board over SurrealDB: the reads, the rules and the copy (`tools/tasks_store.py` `Store`).
//!
//! Every read is one request whose statements run in one transaction, so it sees the board as of one
//! commit (the Python store's `_snapshot`). A rule reads the task it acts on, decides here, and writes
//! in a transaction that throws `stale` if what it decided on has moved since: the task's `version`,
//! the actor's session, and the `last_seen` of every session it took to be gone (each write that
//! could make a session healthier moves its `last_seen`). A write that meets another's (SurrealDB's
//! optimistic conflict) or finds itself stale reads and decides again.

use crate::health::{self, EXECUTING, Health};
use crate::maps::Collect as _;
pub(crate) use crate::quote::repr;
use crate::rows::{LinkRow, MessageRow, RoleRow, SessionRow, TaskRow};
use crate::rpc::{Client, DbError};
use crate::schema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::hash::{BuildHasher, RandomState};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) mod admission;
mod artifacts;
pub(crate) mod asks;
mod beside;
mod branches;
mod callback_peer;
mod callback_state;
mod callbacks;
mod cap;
mod copy;
mod database;
mod depends;
mod dispatcher;
pub(crate) mod execution;
mod fields;
pub(crate) mod guard;
mod history;
mod kept;
mod launches;
mod messages;
mod opening;
mod pool;
mod postgres;
mod pull_account;
mod queued_lines;
mod raw;
mod reading;
pub mod recovery;
mod report_lines;
mod resume;
pub(crate) mod search;
mod session_urls;
mod sessions;
mod settings;
mod snapshot;
mod spending;
mod stage_lines;
mod stamp;
mod statement;
pub(crate) mod stored_settings;
mod surreal;
mod thread_window;
mod transaction;
pub(crate) mod unanswered;
mod waits;
mod work;
pub(crate) mod workflow;
mod workflow_events;
mod writers;

use beside::Beside;
use postgres::{self as pg, Pg};
use statement::{Statement, both};
use surreal::reading::{native_bounds, reading};
use surreal::session_read;

pub(crate) use crate::stages::BLOCK_LINE;
pub use asks::{ASK_KINDS, Ask, LATE_SECONDS, NewAsk};
pub use callbacks::{Callback, ChatSeat, Presence};
/// Every statement the cap on an orchestrator's open tasks sends: the plan test's to plan.
pub use cap::STATEMENTS as SLOT_READS;
pub use copy::{CARRIED, leases_ended, leases_past};
pub use database::Database;
pub use launches::NewLaunch;
pub use messages::reviewer_verdict;
pub use opening::{Opening, Standing};
pub use raw::{Raw, Rows as RawRows};
use reading::native_health;
pub use report_lines::{ReportPlace, ReportRead, ReportTask};
pub use search::{SearchHit, SearchSource};
pub use sessions::{Beat, Binding, KINDS, NewSession, Pick};
pub use stage_lines::StageRecord;
pub use stamp::TaskStamp;
pub use thread_window::{THREAD_WINDOW_LIMIT, ThreadWindow};
pub use unanswered::{LOOKUP_WAIT as UNANSWERED_WAIT, LOOKUP_WAIT_VAR as UNANSWERED_WAIT_VAR};
pub use waits::{AssessmentReceipt, AssessmentState};
pub use work::{
    BEFORE_WORK, COVERED, CoveredLine, Filed, Given, Held, LANDS_FROM, Landed, NewTask, On,
    PIPELINE, PULLABLE, ROLES, RunPlace, STATUSES, Unfresh, approved, can_transition, covered_by,
    covered_line, landing,
};
pub use writers::{
    EVERY as WRITERS_EVERY, Program, RECENT as WRITERS_RECENT, SINCE as WRITERS_SINCE, Writer,
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
thread_local! {
    /// The tries this thread's calls made again ([`Store::tried_again_here`]).
    static TRIED_AGAIN: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
/// What a write throws when what its rule decided on has moved.
const STALE: &str = "stale";
/// What a write throws to refuse goes before its words: the refusal comes back as `Error::Refused`.
const REFUSED: &str = "refused: ";
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
    /// The latest nonempty recorded executor branch, independent of session health or closure.
    pub branch: Option<String>,
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
    /// The database the board is in, and its client.
    db: database::Board,
    /// The client the unanswered-post lookup reads through when one is set
    /// ([`Store::looking_up_within`]): this one's board, a read that ran out its shorter wait not
    /// sent again ([`Client::bounded`]). None reads through `db`, as every other read.
    lookup: Option<database::Board>,
    clock: Clock,
    pid_alive: Probe,
    host: String,
    /// Where new sessions' handles come from.
    pick: Mutex<Box<dyn Pick>>,
    callback_snapshot: Mutex<Option<Snapshot>>,
    callback_idle: Mutex<Option<callbacks::Idle>>,
    /// The hc terminals the server clock's current tick sampled, which every view of that tick reads.
    clock_panes: Mutex<Option<crate::cli::agents::Answer<Vec<crate::cli::agents::Pane>>>>,
    /// The process listing a long-lived reader last took ([`Store::kept_scan`]).
    process_scan: kept::Scan,
    /// Where this store's writes send their callback dispatch; none dispatches in the write.
    hand_off: Option<HandOff>,
    /// Whether its delivering passes give the open tasks no orchestrator holds first: a detached
    /// dispatcher's ([`Store::giving`]).
    giving: bool,
    /// What its delivering passes settle of the tasks a custom workflow holds, after the give:
    /// [`Store::settle_held_if_due`], installed by [`Store::giving`] alone, which says why it is a
    /// pointer.
    settling: Option<fn(&Store, &Board) -> bool>,
    /// The messages a dispatch pass reads every recipient's inbox from ([`pool::Pool`]).
    inbox_pool: Mutex<Option<pool::Pool>>,
    /// What it holds its starts and its PULL digests to ([`crate::limits`]): the open tasks an
    /// orchestrator may hold before a start is refused, the low-volume threshold, the agents a
    /// machine runs and when the pipeline alarm raises and clears; set, or the project file's as
    /// it is now.
    limits: crate::limits::Held,
    /// The schema version this store writes, which every write's [`schema::GUARD`] checks.
    schema: i64,
    /// How many additive steps on that contract it has ([`schema::ADDED`]): a whole-board copy's
    /// and a rebind's guard holds the board to exactly these.
    additions: i64,
    /// Each table its additive steps define, with the step: a write naming one is refused on a
    /// board without that step ([`schema::needs`]).
    added: std::sync::Arc<[(String, i64)]>,
    /// What a write on a board at another version is refused with, naming the board's project.
    refusal: schema::Refusal,
    /// Whether `TASKS_SERVER` named the board's server, so the command that reaches the board again
    /// names it too ([`Store::board_command`]).
    named_server: bool,
    /// The recovery authority it was handed ([`Store::recovering`]): none refuses every write
    /// bound to a recovery generation ([`recovery`]).
    authority: Option<std::sync::Arc<dyn recovery::Authority>>,
    /// Whether it was opened to read by the schema rule ([`schema::Access::Read`], the Symphony's
    /// opening), whose ordinary writes alone take a board one schema ahead that lets a Symphony
    /// of its schema write it ([`schema::COMPATIBLE`]); set only by an opening that read the board,
    /// never by a store taken unchecked.
    reader: bool,
}

/// The command that reaches the board on `database` again: `just tasks`, naming the database when
/// it is not the project's own, and `server` when `TASKS_SERVER` named it.
pub fn board_command(server: Option<&str>, database: &str) -> String {
    let mut command = String::new();
    if let Some(server) = server {
        command.push_str(&format!("TASKS_SERVER={server} "));
    }
    command.push_str("just tasks");
    if database != crate::DATABASE {
        command.push_str(&format!(" --database {database}"));
    }
    command
}

impl Store {
    /// This store, reached on a server `TASKS_SERVER` named: the commands it tells a session to run
    /// name that server ([`board_command`]).
    pub fn on_named_server(mut self) -> Store {
        self.named_server = true;
        self
    }

    /// This store, each read of its unanswered-post lookup waiting at most `wait` (its client's own
    /// wait when that is shorter) and sent once: a board command's, [`UNANSWERED_WAIT`] unless
    /// [`UNANSWERED_WAIT_VAR`] says, so no command waits minutes on what it can do without. A
    /// store none is set on waits on the lookup as on any read.
    pub fn looking_up_within(mut self, wait: Duration) -> Store {
        self.lookup = Some(self.db.bounded(wait));
        self
    }

    /// The command that reaches this store's board again ([`board_command`]).
    pub fn board_command(&self) -> String {
        board_command(
            self.named_server.then(|| self.db.addr()).as_deref(),
            self.db.database(),
        )
    }

    /// The SurrealDB client of a board on SurrealDB. A board on PostgreSQL has none: its client is
    /// [`Store::database`]'s.
    pub fn client(&self) -> &Client {
        match self.db.reach() {
            database::Reach::SurrealDb(client) => client,
            database::Reach::PostgreSql(_) => {
                panic!("this board is on PostgreSQL, which no SurrealDB client reaches")
            }
        }
    }

    /// The database the board is in.
    pub fn database(&self) -> &Database {
        self.db.local()
    }

    /// Transfer a preliminary version reader's connection to its command's full opening.
    pub(crate) fn into_database(self) -> Database {
        self.db.into_local()
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
        Ok(self.retrying(|| self.shown(&which))?.tasks())
    }

    /// Eligibility without transferring briefs, roles or session health for the entire board.
    pub(crate) fn task_slugs(&self, status: Option<&str>) -> Result<Vec<String>, Error> {
        let by_status = status.is_some();
        let read = || both(surreal::task_slugs(by_status), pg::task_slugs(by_status));
        self.retrying(|| self.transact(read(), json!({"status":status})))
    }

    pub(crate) fn tasks_by_slug(&self, slugs: &[String]) -> Result<Vec<Task>, Error> {
        if slugs.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self.retrying(|| self.shown(&Which::Slugs(slugs)))?.tasks())
    }

    pub fn get(&self, slug: &str) -> Result<Task, Error> {
        let view = self.retrying(|| self.shown(&Which::Slug(slug)))?;
        view.task(slug)?;
        Ok(view.tasks().remove(0))
    }

    pub fn session(&self, handle: &str) -> Result<Session, Error> {
        let found: Found = self.retrying(|| {
            self.transact(
                both(session_read(), Pg::read(pg::sessions::session_read())),
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
        let read = || both(surreal::claimable(), pg::claimable());
        let mut slugs: Vec<String> = self.retrying(|| {
            self.transact(read(), json!({"handle": handle, "executing": EXECUTING}))
        })?;
        slugs.sort_unstable();
        slugs.dedup();
        Ok(slugs)
    }

    pub fn thread(&self, slug: &str) -> Result<Vec<Message>, Error> {
        #[derive(Deserialize)]
        struct Thread {
            known: bool,
            messages: Vec<MessageRow>,
        }
        let thread: Thread = self.retrying(|| {
            self.transact(
                both(surreal::THREAD, pg::messages::thread()),
                json!({"slug": slug}),
            )
        })?;
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
        let read = || both(surreal::THREADS, pg::messages::threads());
        let read: Threads = self.retrying(|| self.transact(read(), json!({"slugs": slugs})))?;
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
            .collect_map())
    }

    /// Become the executor. Of any number of claims at once, one wins. A claim that starts ready
    /// work is refused when it would raise an orchestrator's open tasks past the store's cap
    /// ([`crate::slots`]).
    pub fn claim(&self, slug: &str, handle: &str) -> Result<Task, Error> {
        self.ruling(slug, handle, true, |view| {
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
            change.say = Say::system(crate::stages::claimed_line(handle, &handed));
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

    /// A reviewer's `approve` (the task is done, and marked approved and not landed until a landing
    /// closes it, [`crate::orchestration::unlanded`]) or `changes` (back to its executor). The reviewer
    /// is fresh (RULES.md `workflow/reviewers-not-the-owner`), by the rule the land goes by
    /// ([`Held::unfresh`]), so the board takes no verdict the land would refuse afterwards: a
    /// session that ever executed the task, or a work subtask under it, gives it none, whatever
    /// role it holds now, though the instruction subtask it claimed to review the task is not such
    /// work ([`Store::ever_held`]); nor one that orchestrates it or did, or filed it, though it holds
    /// the reviewer's role.
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
        // The board the command a refusal names reaches: this one.
        let board = self.board_command();
        let held = self.ever_held(slug)?;
        if held.executed.contains(handle) {
            return Err(refused(work::unfresh_refusal(
                &board,
                slug,
                handle,
                Unfresh::Executed,
                false,
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
            let orchestrates = view.holds(handle, "orchestrator");
            // One that executed it was refused above; one that requested it is told it gives no
            // verdict a landing goes by, whatever role it holds, and who does.
            if let Some(why) = held.unfresh(handle, &task.created_by, orchestrates) {
                return Err(refused(work::unfresh_refusal(
                    &board, slug, handle, why, false,
                )));
            }
            if !view.holds(handle, "reviewer") {
                return Err(refused(format!(
                    "{handle} is not a reviewer of {slug}; take the role first: {board} role \
                     {slug} reviewer"
                )));
            }
            let approve = verdict == "approve";
            Ok(Change {
                status: Some(if approve { "done" } else { "in_progress" }.into()),
                say: Some(Say {
                    kind: "verdict",
                    text: said.clone(),
                    recipient: (!approve).then(|| "@executor".into()),
                }),
                // Done, and its land still somebody's to make: marked until a landing closes it.
                unlanded: approve.then(|| json!(view.now)),
                ..Change::default()
            })
        })?;
        if verdict == "changes" {
            let what = format!("the changes verdict from {handle} to @executor");
            self.tell_if_unread(slug, None, &what, None);
        }
        Ok(task)
    }

    /// A slice's executor closes it with its integration result; only the parent lands. A task of
    /// any kind whose work landed with another's closes as covered by that landing instead
    /// ([`Store::close_covered`]), which the refusal of a top-level task names.
    pub fn done(&self, slug: &str, handle: &str, note: &str) -> Result<Task, Error> {
        if note.trim().is_empty() {
            return Err(refused(
                "closing a task needs a note saying what came of it".into(),
            ));
        }
        let board = self.board_command();
        self.rule(slug, handle, |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            if task.parent.is_none() {
                return Err(refused(format!(
                    "{slug} is not a subtask; it closes by a verdict or a landing, or, when its \
                     work landed with another task's, as covered by that landing: {}",
                    work::covered_command(&board, slug)
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
    /// decides on, in the same transaction as its task, as does a red's close ([`Store::ruled`]).
    /// One that gives a session the orchestrator role ([`Change::gains`]) reads that session's open
    /// tasks for the limit on them ([`Store::gaining`]). The cap and the limit are read once, so a
    /// project file changed meanwhile never splits one decision.
    ///
    /// A task with no workflow position that the change moves out of done or cancelled while a
    /// custom workflow is active is refused by the database (`workflow_position`), and the rule is
    /// then written again placing it at its mapped pair in the same write
    /// ([`crate::workflow::activate::Placing::reopened`]): a board with no workflow writes as it
    /// always did, in one write.
    fn ruling(
        &self,
        slug: &str,
        actor: &str,
        starts: bool,
        decide: impl Fn(&View) -> Result<Change, Error>,
    ) -> Result<Task, Error> {
        match self.ruling_placed(slug, actor, starts, &decide, false) {
            Err(Error::Refused(why)) if why.starts_with(&workflow::unplaced(slug)) => {
                self.ruling_placed(slug, actor, starts, &decide, true)
            }
            ruled => ruled,
        }
    }

    /// [`Store::ruling`]'s write, placing a task it reopens where `place` says to.
    fn ruling_placed(
        &self,
        slug: &str,
        actor: &str,
        starts: bool,
        decide: &impl Fn(&View) -> Result<Change, Error>,
        place: bool,
    ) -> Result<Task, Error> {
        let limits = self.limits();
        let cap = limits.orchestrator_slots;
        let which = if starts && cap.is_some() {
            Which::Starting(slug)
        } else {
            Which::Slug(slug)
        };
        // A task a custom workflow holds moves, and is staffed, by its workflow's edges alone.
        let decide = |view: &View| {
            let change = decide(view)?;
            match view.task(slug) {
                Ok(task) if workflow::held(task) && change.moves_or_staffs() => {
                    Err(refused(workflow::explicit_action(slug)))
                }
                _ => Ok(change),
            }
        };
        self.retrying(|| {
            let mut ruled = self.ruled(&which, slug, cap, &decide)?;
            if let Some((to, what)) = &ruled.change.gains {
                let limit = limits.orchestrator_tasks;
                self.gaining(to, what, limit, &mut ruled.slots)?;
            }
            let (view, change) = (&ruled.view, &ruled.change);
            let placing = if place {
                self.reopening(view, change, slug, actor)?
            } else {
                None
            };
            let version = view.snap.tasks.first().map_or(0, |t| t.version);
            let (reason, needs, gates, unlanded, blocked, moved, holds) = (
                change.reason.is_some(),
                change.needs.is_some(),
                change.gates.is_some(),
                change.unlanded.is_some(),
                change.blocked.is_some(),
                (change.status.as_deref()).map(|status| status == "cancelled"),
                !ruled.slots.is_empty(),
            );
            let placed = placing.is_some();
            let body = both(
                surreal::rule(
                    reason,
                    (needs, gates),
                    unlanded,
                    blocked,
                    moved,
                    holds,
                    placed,
                ),
                pg::rule(
                    reason,
                    (needs, gates),
                    unlanded,
                    blocked,
                    moved,
                    holds,
                    placed,
                ),
            );
            let mut vars = change.vars(slug, actor, version, view.now);
            vars["slots"] = Value::Array(ruled.slots);
            if let Some(placing) = placing {
                vars["placed"] = json!(placing.position());
                vars["placing"] = json!(placing);
            }
            self.written(body, vars, slug)
        })
    }

    /// Where `change` places `slug` as it moves it out of done or cancelled with no workflow
    /// position, on the workflow active now; none for any other change, or a board with none
    /// active. Refused, naming the pair, where the active workflow cannot place it.
    fn reopening(
        &self,
        view: &View,
        change: &Change,
        slug: &str,
        actor: &str,
    ) -> Result<Option<workflow::Transition>, Failure> {
        const CLOSED: [&str; 2] = ["done", "cancelled"];
        let task = view.task(slug).map_err(Failure::Stop)?;
        let Some(status) = change.status.as_deref() else {
            return Ok(None);
        };
        if workflow::held(task)
            || !CLOSED.contains(&task.status.as_str())
            || CLOSED.contains(&status)
        {
            return Ok(None);
        }
        let Some(placing) = self.placing_context().map_err(Failure::Stop)? else {
            return Ok(None);
        };
        let roles: Vec<(String, String, bool)> = (view.snap.roles.iter())
            .filter(|r| r.slug == slug)
            .map(|r| (r.handle.clone(), r.role.clone(), true))
            .collect();
        let principal = if actor == work::OWNER {
            crate::workflow::local_owner()
        } else {
            crate::workflow::agent_session(actor)
        };
        let reopening = crate::workflow::activate::Reopening {
            slug,
            left: &task.status,
            status,
            roles: &roles,
            retired: workflow::retired(&task.fields),
        };
        placing
            .reopened(&reopening, &principal, view.now)
            .map(Some)
            .map_err(|why| Failure::Stop(Error::Refused(why)))
    }

    /// `which` with every session: what a rule decides on, the cap's ledger and a whole board read.
    #[track_caller]
    fn view(&self, which: &Which) -> Result<View, Failure> {
        let (read, vars) = read_of(which, Sessions::Whole);
        let snap: Snapshot = self.transact(read, vars)?;
        Ok(View::new(self, snap))
    }

    /// `which` for an answer that is its tasks alone ([`Sessions::Shown`]): only [`View::tasks`],
    /// and the asker of `Which::Pullable`, may be asked of it.
    #[track_caller]
    fn shown(&self, which: &Which) -> Result<View, Failure> {
        let (read, vars) = read_of(which, Sessions::Shown);
        let snap: Snapshot = self.transact(read, vars)?;
        Ok(View::new(self, snap))
    }

    /// `body`'s writes, then `slug` read as `get` reads it, in one transaction: the task as the write
    /// left it, in the write's one round trip. `body` ends without a `RETURN`.
    #[track_caller]
    fn written<'a>(
        &self,
        body: impl Into<Statement<'a>>,
        vars: Value,
        slug: &str,
    ) -> Result<Task, Failure> {
        self.written_statement(body.into(), vars, slug, true)
    }

    #[track_caller]
    fn written_statement(
        &self,
        body: Statement,
        mut vars: Value,
        slug: &str,
        deliver: bool,
    ) -> Result<Task, Failure> {
        let which = Which::Slug(slug);
        let (read, more) = reading(&which, Sessions::Shown);
        if let (Some(vars), Value::Object(more)) = (vars.as_object_mut(), more) {
            vars.extend(more);
        }
        // On PostgreSQL the write's block, then the read, its answer.
        let postgres = match body.postgres {
            Some(Pg::Script(block, None)) => {
                Some(Pg::script_then(block, pg::reading::written(&which)))
            }
            _ => None,
        };
        let written = Statement {
            surreal: surreal::written(&body.surreal, &read).into(),
            postgres,
        };
        // Include recipient presence in the write's existing result instead of a separate read.
        let snap: Snapshot = self.transact_dispatch(written, vars, false)?;
        // The deferred owner filing leaves both delivery and block recovery to the server clock.
        let refresh = deliver
            && snap
                .tasks
                .iter()
                .any(|t| matches!(t.row.status.as_str(), "done" | "blocked"))
            && self.resume_after_write();
        if deliver && snap.has_callbacks {
            self.after_write();
        }
        let view = View::new(self, snap);
        view.task(slug).map_err(Failure::Stop)?;
        if refresh {
            return self.get(slug).map_err(Failure::Stop);
        }
        Ok(view.tasks().remove(0))
    }

    /// How many times a call on this thread has read and decided again since the thread began, the
    /// board having moved under it (a conflict, or a stale guard): the work a busy board cost this
    /// thread's calls, which a caller reads before and after a call to count that call's own.
    pub fn tried_again_here() -> u64 {
        TRIED_AGAIN.with(std::cell::Cell::get)
    }

    fn retrying<T>(&self, mut once: impl FnMut() -> Result<T, Failure>) -> Result<T, Error> {
        retried(PATIENCE, &mut once)
    }
}

/// `once` tried until it gives its result or refuses, again each time the board moved under it,
/// for `patience` of the wall clock at most, sleeping the backoff's wait between two tries
/// ([`retried_on`]).
#[inline(never)]
fn retried<T>(
    patience: Duration,
    once: &mut dyn FnMut() -> Result<T, Failure>,
) -> Result<T, Error> {
    let began = Instant::now();
    retried_on(patience, &|| began.elapsed(), &rest, once)
}

/// The store's rest between two tries of a call that met a board moving under it.
fn rest(wait: Duration) {
    #[expect(
        clippy::disallowed_methods,
        reason = "the named helper of RULES.md code/no-timed-waits for a retry with backoff: the board store's"
    )]
    std::thread::sleep(wait);
}

/// `once` tried until it gives its result or refuses, again each time the board moved under it,
/// for as long as `elapsed` is short of `patience`, however many tries that takes: each try again
/// counted on this thread ([`Store::tried_again_here`]), after `rest` for the backoff's next wait.
/// Its clock and its rest are its caller's, so a test moves them and never waits on either.
fn retried_on<T>(
    patience: Duration,
    elapsed: &dyn Fn() -> Duration,
    rest: &dyn Fn(Duration),
    once: &mut dyn FnMut() -> Result<T, Failure>,
) -> Result<T, Error> {
    let mut attempt = 0;
    loop {
        match once() {
            Ok(done) => return Ok(done),
            Err(Failure::Stop(error)) => return Err(error),
            Err(Failure::Again) if elapsed() < patience => {
                TRIED_AGAIN.with(|tried| tried.set(tried.get() + 1));
                rest(backoff(attempt));
                attempt += 1;
            }
            Err(Failure::Again) => {
                return Err(Error::Db(format!(
                    "gave up after {} s and {} tries: the board kept moving under the write",
                    patience.as_secs(),
                    attempt + 1
                )));
            }
        }
    }
}

#[derive(Deserialize)]
struct Found {
    session: Option<SessionRow>,
    held: Vec<String>,
    native_until: Option<f64>,
}

enum Which<'a> {
    All,
    /// One whole-board observation with the admission ledger's validated hold records.
    SlotHoldings,
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

/// The sessions a read returns. The board keeps every session it has seen, so the whole list
/// grows with its history, and a read that only shows tasks needs the few on them.
#[derive(Clone, Copy)]
enum Sessions {
    /// Every session the board has seen: a rule may ask after any handle (its actor, a hand
    /// over's target), the cap's ledger reads every session's parent, and a board is all of them.
    Whole,
    /// The sessions holding a role on the tasks read, and the session `Which::Pullable` asks
    /// for: all a task's roles' health is derived from, since health is a session's own row and
    /// `held`, which is read whole either way. A role whose holder has no session reads lost in
    /// both.
    Shown,
}

/// Why one try of a call ended without its result.
#[derive(Debug)]
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

/// What a read gives back: the tasks asked for, their links and roles, the sessions asked for
/// ([`Sessions`]), who holds claimable work, and the newest message's id.
#[derive(Clone, Deserialize)]
struct Snapshot {
    #[serde(default)]
    branches: Vec<branches::Recorded>,
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
    /// The admission ledger inputs (`Which::Starting`, `Which::SlotHoldings`).
    #[serde(default)]
    slots: Option<cap::Read>,
}

#[derive(Clone, Deserialize)]
struct NativePresence {
    handle: String,
    until: f64,
}

/// A task as the board holds it. The row refuses a field it does not know, but read beside the
/// record's `version` ([`Beside`]) it is handed only its own, so `version` is the record's; every
/// read of a task goes through here.
#[derive(Clone)]
struct TaskRecord {
    row: TaskRow,
    version: i64,
}

impl<'de> Deserialize<'de> for TaskRecord {
    fn deserialize<D: serde::Deserializer<'de>>(from: D) -> Result<Self, D::Error> {
        let mut record = Beside::read(from)?;
        Ok(TaskRecord {
            version: record.take("version")?,
            row: record.rest()?,
        })
    }
}

/// A snapshot, as of the clock when it was read, with each session's health worked out once.
struct View {
    snap: Snapshot,
    now: f64,
    sessions: HashMap<String, (usize, Health)>,
    /// The command that reaches the board it was read from ([`Store::board_command`]).
    board: String,
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
            board: store.board_command(),
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
            None => Err(refused(unknown_actor(&self.board, handle))),
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
                branch: branches::latest(&self.snap.branches, &row.slug).map(String::from),
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
    /// A post of the actor's after `say`, in the same write: a move's reason
    /// (`work::reason_line`).
    reason: Option<String>,
    /// Work it starts, which takes a slot of an orchestrator's under the store's cap.
    opens: Option<Opens>,
    /// The session it gives the orchestrator role on the task, held to the limit on the open tasks
    /// an orchestrator holds the role on (`[limits] orchestrator_tasks`, [`Store::gaining`]), and
    /// what a refusal says cannot happen.
    gains: Option<(String, String)>,
    /// The task's needs it writes, the board's own field ([`crate::stamps::Needs`],
    /// [`Store::set_needs`]).
    needs: Option<Value>,
    /// The task's gate decisions it writes, every one, the board's own field
    /// ([`crate::gates::Decision`], [`Store::decide_gate`]).
    gates: Option<Value>,
    /// What the block it makes waits on, the board's own field ([`crate::waits_on::Blocked`]).
    blocked: Option<Value>,
    /// The mark of a task approved and not landed, the board's own field
    /// ([`crate::orchestration::UNLANDED`]): when its approve was given, or null to lift it, as its
    /// landing's line and a move back to review do.
    unlanded: Option<Value>,
}

impl Change {
    /// Whether it moves the task, blocks it, or adds or drops an executor or a reviewer: what a
    /// custom workflow's edges alone do to a task it holds. The orchestrator's role and a
    /// reservation are the board's coordination, which its writers keep: a launch reserves the
    /// work it starts a session for, and admission honours the reservation, and consumes it, when
    /// that session takes the edge (docs/workflow/WORKFLOW.md, "Operations").
    fn moves_or_staffs(&self) -> bool {
        let staffing = |role: &str| role != "orchestrator" && role != "reserved";
        self.status.is_some()
            || self.blocked.is_some()
            || self.add.as_ref().is_some_and(|r| staffing(&r.role))
            || self.drop.iter().any(|(_, role)| staffing(role))
    }
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
    /// Whether the change lifts the task's red mark ([`Store::unmark_red`]).
    unmarks: bool,
    /// The sessions whose parents stand in for an orchestrator when none holds the task: its
    /// executor once it starts.
    by: Vec<String>,
    /// What the refusal says cannot happen.
    what: String,
    /// For a task a custom workflow holds, whether the position it moves to holds a slot by its
    /// slot policy ([`crate::slots::Entry::slot`]).
    slot: Option<bool>,
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
            "move_reason": self.reason,
            "needs": self.needs,
            "gates": self.gates,
            "blocked": self.blocked,
            "unlanded": self.unlanded,
        })
    }
}

fn refused(message: String) -> Error {
    Error::Refused(message)
}

/// The refusal of `handle`, a session the board does not know: register on `board`
/// ([`Store::board_command`]), the board that refused it.
fn unknown_actor(board: &str, handle: &str) -> String {
    format!(
        "no session {}; register first (`{board} register`)",
        repr(handle)
    )
}

fn left_actor(handle: &str) -> String {
    format!("session {handle} has left; attach again")
}

impl Store {
    /// `ACT`'s variables, and `more`'s.
    fn acting(&self, actor: &str, now: f64, more: Value) -> Value {
        let mut vars = json!({
            "actor": actor,
            "now": now,
            "unknown_actor": format!("{REFUSED}{}", unknown_actor(&self.board_command(), actor)),
            "left_actor": format!("{REFUSED}{}", left_actor(actor)),
        });
        if let (Some(vars), Value::Object(more)) = (vars.as_object_mut(), more) {
            vars.extend(more);
        }
        vars
    }

    /// Whether `error` is this board's refusal of `handle` as the session that acts: one it does
    /// not know, or one that left, as every write says it. Who acts is no input of a command's own,
    /// so its command line exits as every command's does for it, whatever its own refusals do.
    pub fn refuses_actor(&self, error: &Error, handle: &str) -> bool {
        matches!(error, Error::Refused(why)
            if *why == unknown_actor(&self.board_command(), handle) || *why == left_actor(handle))
    }
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

/// The read of `which` with the sessions `sessions` asks for, in each database's language, and
/// its variables.
fn read_of(which: &Which, sessions: Sessions) -> (Statement<'static>, Value) {
    let (read, vars) = reading(which, sessions);
    let statement = Statement {
        surreal: read.into(),
        postgres: Some(pg::reading::reading(which, sessions)),
    };
    (statement, vars)
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
mod tests;
