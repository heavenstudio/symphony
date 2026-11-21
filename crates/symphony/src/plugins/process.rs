//! A plugin that answers from a process of its own (`[plugins.process]`, docs/PLUGIN_PROTOCOL.md):
//! the host's side of the protocol. Its project names it in `[plugins] crates` as a linked plugin
//! is named, and its command in `[plugins.process]`; [`made`] gives the one [`Remote`] for that
//! name, command and checkout in this process, kept for the process's life, which stands in the
//! project's plugins where a linked plugin would. A linked worktree (a branch's checkout) starts
//! none of its own: its main checkout's process answers for it ([`answering`]).
//!
//! **Its process.** Started when what it declared or a call of it is first needed, in the
//! project's checkout, without the session ([`crate::children::command`]), its stdin and stdout
//! the protocol's and each line of its
//! stderr appended to its log (`plugins/NAME.log` in the artifacts folder, rolled to `NAME.1.log`
//! past [`LOG_MAX`]), with a line there for each start and end. The first request declares: the
//! plugin's [`Declaration`], the minor the two agree ([`Version::agreed`]). A plugin of another
//! major, one that declares itself by another name than the project gives it, or one that declares
//! a seam a process does not answer is refused and killed, and not started again while its program
//! is the file it was. One thread writes the host's lines to its stdin from a queue of [`QUEUED`],
//! and one reads its answers, each line at most [`LINE_MAX`] bytes, and hands each to the call its
//! id names. A call waits [`WAIT`] for its answer, its line's writing included: one whose request
//! was not even written in that wait ends the process, as it reads no more; else the call is given
//! up and sent a `cancel`, and a process that still has not answered it a wait later is ended. A
//! process is ended too when its stdout fails or holds a line too long, or its queue is full: ended
//! is killed, so nothing it runs outlives the reason. A process that ends fails its calls waiting
//! with how it ended, and is started again by the first call [`PAUSE`] after: at most [`STARTS`]
//! starts in [`WINDOW`], then it stays down until the oldest leaves the window. Its stdin ends with
//! this process, which ends it (the SDK's `serve` at most its `ENDING` after).
//!
//! **Its seams.** What the plugin declared stands for it here: its components, card kinds,
//! renderers, routes, home regions, board actions, watches and doc checks ([`seams`]), each
//! forwarding its calls; its navigation and theme, which are declaration alone; and its commands,
//! each run as its program ([`seams::RemoteCommand`]). A plugin declaring any other seam is refused,
//! naming it ([`unanswered`]). A call newer than the agreed minor is not sent: the seam's own
//! default answers it. A call that gets no answer (the process down, no answer in time, a refusal,
//! an answer that does not read) is shown where its answer would go: a component's in its place on
//! the page ([`super::error`]), a card kind's as the kind's default, as if the plugin said nothing
//! of the card, and each other seam's as [`seams`] says. A page's components share their `once`
//! values in the plugin as they do linked: every call of one [`Page`] names its render
//! ([`PageData::render`]). A register read asks every card kind of every card in one request
//! ([`Remote::read_cards`], [`ReadCards`]), its answers kept for that read.
//!
//! **Measured.** Under `[plugins] timing` or `loopback` each call writes its line to the project's
//! record ([`super::measure`]) as a process call: the plugin's own time, as it says it, the host's
//! whole wait for the answer, and the bytes of the two lines.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::io::{self, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use serde_json::value::RawValue;
use symphony_plugin::protocol::{
    Answered, ApprovalNeedsAgent, Ask, AutoApprovable, CacheVersion, CardData, CardGates, Chosen,
    Declaration, Declare, Envelope, KindAnswers, LINE_MAX, Lined, Member, PROTOCOL, PageData,
    Pictures, Raw, ReadCards, Recognises, Render, Request, Resolve, Slot, TriageCategory, Version,
    read_line,
};
use symphony_plugin::vocabulary::{Block, NavItem, Picture};
use symphony_plugin::{
    Authority, BoardAction, Card, CardKind, Command, Component, DocCheck, HomeRegion, Page, Plugin,
    Renderer, Route, Theme, Watch,
};

use super::measure::{self, Line, Record};
use crate::project::{PluginProcess, Project, Stamp};
use seams::{
    RemoteAction, RemoteAuthority, RemoteCheck, RemoteCommand, RemoteRegion, RemoteRenderer,
    RemoteRoute, RemoteWatch,
};

/// How long a call waits for its answer, as the gates seam waits for its answerers.
pub const WAIT: Duration = Duration::from_secs(10);
/// How long after its process ended a plugin is started again: the calls meanwhile show it down.
pub const PAUSE: Duration = Duration::from_secs(1);
/// How many starts a plugin's process gets in [`WINDOW`].
pub const STARTS: usize = 5;
/// The window [`STARTS`] counts in.
pub const WINDOW: Duration = Duration::from_secs(180);
/// How long a process whose stdout ended is given to end before it is killed, and one killed to be
/// seen ended.
pub const GRACE: Duration = Duration::from_secs(1);
/// The longest an end takes, with a margin: its process's two graces to be reaped and its stderr's
/// one to be read to its end. A call that finds its process ending waits this at most for how it
/// ended.
const ENDS_IN: Duration = Duration::from_secs(4);
/// The size past which a plugin's log is rolled to `NAME.1.log`, replacing the one before, so the
/// two hold about twice this at most.
pub const LOG_MAX: u64 = 4 * 1024 * 1024;
/// The host's lines queued for a process's stdin at most: one more, with none of them taken, ends
/// it.
pub const QUEUED: usize = 256;
/// What the log shows of a line of the plugin's that answers no call: its first bytes.
const SHOWN: usize = 256;
/// The longest line of a process's stderr its log takes whole: a longer one goes in pieces.
const ERROR_LINE: usize = 4096;

/// The bounds a remote keeps to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// How long after its process ended it is started again.
    pub pause: Duration,
    /// How long a call waits for its answer.
    pub wait: Duration,
    /// How long a started process's declaration is waited for.
    pub declare: Duration,
    /// How long a call given up and sent a `cancel` may still be answered: a process that has not
    /// answered it by then is ended.
    pub cancel: Duration,
    /// The longest line either way, its newline aside.
    pub line: usize,
    /// The size past which its log is rolled.
    pub log: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            pause: PAUSE,
            wait: WAIT,
            declare: WAIT,
            cancel: WAIT,
            line: LINE_MAX,
            log: LOG_MAX,
        }
    }
}

/// The plugin `name` of a project, answering from the process `process` starts: the one this
/// process holds for that name, command and checkout, made at the first ask.
pub fn made(name: &str, process: &PluginProcess) -> &'static Remote {
    static MADE: LazyLock<Mutex<Vec<&'static Remote>>> = LazyLock::new(|| Mutex::new(Vec::new()));
    let mut made = lock(&MADE);
    if let Some(found) = made.iter().find(|remote| {
        remote.name == name
            && remote.process.command == process.command
            && remote.process.root == process.root
    }) {
        return found;
    }
    let started = process.clone();
    let remote = Remote::leaked(
        name,
        process.clone(),
        Box::new(move || spawn(&started)),
        Limits::default(),
    );
    made.push(remote);
    remote
}

/// The process answering for the plugin `name` of the checkout `own` is written for (`own.root`):
/// `own` itself in a main checkout. A linked worktree, a branch's checkout, never starts one of its
/// own: the one its main checkout's `[plugins.process]` starts for `name` answers for it, run in the
/// main checkout, each call naming the branch's checkout, so the plugin reads the branch's files;
/// none when that file names no command for it. What a branch writes in its own `[plugins.process]`
/// takes effect once it lands.
pub fn answering(name: &str, own: &PluginProcess) -> Option<PluginProcess> {
    match crate::builds::main_checkout(&own.root) {
        None => Some(own.clone()),
        Some(main) => Project::of(&main).plugin_processes.get(name).cloned(),
    }
}

/// Whether a plugin of the checkout at `root` answering from a process (its main checkout's, from a
/// linked worktree: [`answering`]) has not declared: what a load it refused waits for.
pub fn undeclared(root: &Path) -> bool {
    let answers = crate::builds::main_checkout(root).unwrap_or_else(|| root.to_path_buf());
    lock(&REMOTES)
        .iter()
        .any(|remote| remote.process.root == answers && remote.declared.get().is_none())
}

/// The remote `plugin` is, when it is one: a plugin of a project answering from its own process.
pub fn remote(plugin: &dyn Plugin) -> Option<&'static Remote> {
    lock(&REMOTES)
        .iter()
        .copied()
        .find(|remote| std::ptr::addr_eq(*remote, plugin))
}

/// Every remote made in this process, made by [`made`] or a test's peer.
static REMOTES: LazyLock<Mutex<Vec<&'static Remote>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// `mutex` held, whatever a thread that panicked holding it left.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A process started, as the host speaks with it and ends it.
pub struct Spawned {
    /// Its stdin: where the host's lines go.
    pub input: Box<dyn Write + Send>,
    /// Its stdout: where its answers come from.
    pub output: Box<dyn io::Read + Send>,
    /// Its stderr, each line of which goes to its log, when the host reads it.
    pub errors: Option<Box<dyn io::Read + Send>>,
    pub pid: Option<u32>,
    pub process: Box<dyn Handle>,
}

/// A process started, as the host ends it.
pub trait Handle: Send {
    /// Kills it, when it has not ended.
    fn kill(&mut self);
    /// How it ended (`exited with code 0`, `was killed by signal 9`) once it has; none while it
    /// runs.
    fn ended(&mut self) -> Option<String>;
}

/// What starts a plugin's process.
pub type Start = Box<dyn Fn() -> io::Result<Spawned> + Send + Sync>;

/// A child of this process; dropped while it runs, it is killed and reaped.
struct Child {
    child: symphony_board::sys::Child,
    how: Option<String>,
}

impl Handle for Child {
    fn kill(&mut self) {
        if self.how.is_none() {
            let _ = self.child.kill();
        }
    }
    fn ended(&mut self) -> Option<String> {
        if self.how.is_none() {
            self.how = match self.child.try_wait() {
                Ok(Some(status)) => Some(ended(status)),
                Ok(None) => None,
                Err(why) => Some(format!("could not be waited for: {why}")),
            };
        }
        self.how.clone()
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if self.how.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// `process` started: its program in its checkout, without the session, its three streams piped.
fn spawn(process: &PluginProcess) -> io::Result<Spawned> {
    let (program, args) = process
        .command
        .split_first()
        .ok_or_else(|| io::Error::other("it names no program"))?;
    let piped = symphony_board::sys::Stdio::piped;
    let mut command = crate::children::command(program);
    command
        .args(args)
        .current_dir(&process.root)
        .stdin(piped())
        .stdout(piped())
        .stderr(piped());
    let mut started = Child {
        child: command.spawn()?,
        how: None,
    };
    let input = started
        .child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("its stdin was not opened"))?;
    let output = started
        .child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("its stdout was not opened"))?;
    let errors = started.child.stderr.take();
    Ok(Spawned {
        input: Box::new(input),
        output: Box::new(output),
        errors: errors.map(|errors| -> Box<dyn io::Read + Send> { Box::new(errors) }),
        pid: Some(started.child.id()),
        process: Box::new(started),
    })
}

/// How a process that ended with `status` ended.
fn ended(status: std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt as _;
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exited with code {code}"),
        (None, Some(signal)) => format!("was killed by signal {signal}"),
        (None, None) => format!("ended ({status})"),
    }
}

/// How `process` ended, once it has, within [`GRACE`].
fn waited(process: &mut dyn Handle) -> Option<String> {
    let deadline = Instant::now() + GRACE;
    loop {
        if let Some(how) = process.ended() {
            return Some(how);
        }
        if Instant::now() >= deadline {
            return None;
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: looks again for a process's answer until a deadline, where its pipe is the event"
        )]
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// How `process`, whose stdout ended, ended: given [`GRACE`] to end, then killed.
fn reaped(process: &mut dyn Handle) -> String {
    if let Some(how) = waited(process) {
        return how;
    }
    process.kill();
    match waited(process) {
        Some(_) => format!("did not end {GRACE:?} after its stdout did, and was killed"),
        None => "did not end when it was killed".into(),
    }
}

/// `bytes`, a line of the plugin's, as the log shows it: its first [`SHOWN`] bytes as text, with
/// no control character.
fn shown(bytes: &[u8]) -> String {
    let cut = bytes.len().min(SHOWN);
    let text: String = String::from_utf8_lossy(&bytes[..cut])
        .chars()
        .map(|c| if c.is_control() { '\u{fffd}' } else { c })
        .collect();
    if cut < bytes.len() {
        format!("{text}… ({} bytes)", bytes.len())
    } else {
        text
    }
}

/// The seams `declaration` holds that a plugin's process does not answer: each is answered once a
/// plugin first declares it from a process, and refused by name until then, rather than lost.
fn unanswered(declaration: &Declaration) -> Vec<&'static str> {
    let d = declaration;
    [
        ("board fields", d.board_fields.is_empty()),
        ("gates", d.gates.is_empty()),
        ("prose rules", d.prose_rules.is_empty()),
        ("generators", d.generators.is_empty()),
        ("transports", d.transports.is_empty()),
    ]
    .into_iter()
    .filter(|(_, none)| !none)
    .map(|(seam, _)| seam)
    .collect()
}

/// A project's plugin answering from its own process.
pub struct Remote {
    /// This remote, for its threads and its seams' objects: set once it is made.
    me: OnceLock<&'static Remote>,
    name: String,
    process: PluginProcess,
    start: Start,
    limits: Limits,
    state: Mutex<State>,
    /// When its process was started, the newest last, within [`WINDOW`].
    starts: Mutex<VecDeque<Instant>>,
    /// What it declared at its first start, which its seams' objects stand for.
    declared: OnceLock<Declared>,
    /// The next call's id.
    ids: AtomicU64,
    /// Where its calls are recorded, when the project's settings say so.
    record: Mutex<Option<&'static Record>>,
}

enum State {
    /// Never started.
    Idle,
    /// Started: down once its connection has ended.
    Running(Arc<Connection>),
    /// Its process could not start or declare at `at`, as `why` says.
    Down { at: Instant, why: String },
    /// Refused at its declaration, as `why` says: not started again while its program is the file
    /// it was, `program`.
    Refused { why: String, program: Option<Stamp> },
}

/// Why a start gave no connection.
enum Unstarted {
    /// Down for now: started again after the pause, under the starts' limit.
    Failed(String),
    /// Refused at its declaration.
    Refused(String),
}

/// One start of a plugin's process: what the host writes to, the calls waiting on it, and the
/// process itself.
struct Connection {
    pid: Option<u32>,
    agreed: OnceLock<Version>,
    /// Where the host's lines go, the thread writing its stdin taking them; none once it is ending,
    /// which closes its stdin.
    queue: Mutex<Option<mpsc::SyncSender<Vec<u8>>>>,
    /// The lines queued, numbered from 1, and the lines written whole: a call whose request is not
    /// written in its wait finds the process reads no more.
    queued: AtomicU64,
    written: AtomicU64,
    process: Mutex<Box<dyn Handle>>,
    /// Told when its stderr has ended, so its end is logged after all it said.
    errors: Mutex<Option<mpsc::Receiver<()>>>,
    waiting: Mutex<Waiting>,
    /// Told when it has ended, so a call that found it ending says how.
    ended: Condvar,
}

/// The calls a connection's answers go to, by id; the ones given up whose answers may still come;
/// whether it is ending, and once it has ended, when and how.
#[derive(Default)]
struct Waiting {
    calls: BTreeMap<u64, mpsc::Sender<Result<Reply, String>>>,
    given_up: BTreeSet<u64>,
    ending: bool,
    ended: Option<(Instant, String)>,
}

impl Connection {
    /// Its pid as the log names it, after a space; nothing with none.
    fn pid(&self) -> String {
        self.pid.map_or_else(String::new, |pid| format!(" {pid}"))
    }

    /// When and how it ended, once it has.
    fn ended(&self) -> Option<(Instant, String)> {
        lock(&self.waiting).ended.clone()
    }

    /// Why it takes no call: how it ended, waited for while another call ends it, or that it is
    /// ending still past [`ENDS_IN`].
    fn closed(&self) -> String {
        let waiting = lock(&self.waiting);
        let (waiting, _) = self
            .ended
            .wait_timeout_while(waiting, ENDS_IN, |waiting| waiting.ended.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        waiting
            .ended
            .clone()
            .map_or_else(|| "its process is ending".into(), |(_, why)| why)
    }
}

/// An answer as it came: the `answered` member raw, the plugin's time, the line's length.
struct Reply {
    answered: Box<RawValue>,
    us: Option<f64>,
    bytes: usize,
}

/// What [`Remote::asked`] needs of a call's [`Ask`], held as values so that one body serves every
/// call: its tag, the minor it came in, whether it goes on a pipe, and its seam and call.
#[derive(Clone, Copy)]
struct Call {
    tag: &'static str,
    since: u32,
    piped: bool,
    names: (&'static str, &'static str),
}

impl Call {
    fn of<A: Ask>() -> Call {
        Call {
            tag: A::TAG,
            since: A::SINCE,
            piped: A::PIPED,
            names: A::names(),
        }
    }
}

/// What reads a call's answer as it came, keeping it: why it does not read, if it does not.
type ReadAnswer<'a> = &'a mut dyn FnMut(&RawValue) -> Result<(), String>;

/// `raw`, the plugin's answer to the call `tag`: the answer, or why it does not read or that the
/// plugin refused the call. The answer's wrapper is read by one body for every call
/// ([`answer_in`]), so each call's own reading is only its answer's type.
fn answer_of<T: serde::de::DeserializeOwned>(tag: &str, raw: &RawValue) -> Result<T, String> {
    let answer = answer_in(tag, raw)?;
    serde_json::from_str(answer.get())
        .or_else(|why| {
            content::compatible(tag, answer)
                .map_or(Err(why), |answer| serde_json::from_str(&answer))
        })
        .map_err(|why| cannot_read(tag, &why))
}

#[path = "process_content.rs"]
mod content;

/// The answer `raw` holds as it came, or why it does not read or that the plugin refused the call.
#[inline(never)]
fn answer_in<'a>(tag: &str, raw: &'a RawValue) -> Result<&'a RawValue, String> {
    serde_json::from_str::<Answered<&RawValue>>(raw.get())
        .map_err(|why| cannot_read(tag, &why))?
        .shown()
        .map_err(|why| format!("it refused {tag}: {why}"))
}

/// Why an answer to the call `tag` does not read.
#[inline(never)]
fn cannot_read(tag: &str, why: &serde_json::Error) -> String {
    format!("its answer to {tag} cannot be read: {why}")
}

/// Why a call has no answer of the plugin's.
#[derive(Debug, PartialEq)]
pub enum Failure {
    /// The call is newer than the minor agreed: the seam's default answers it.
    Newer(Version),
    /// The plugin did not answer: its process down, no answer in time, a refusal.
    Down(String),
}

impl Remote {
    /// A remote for the plugin `name`, its process started by `start`, kept to `limits`, kept for
    /// this process's life.
    pub fn leaked(
        name: &str,
        process: PluginProcess,
        start: Start,
        limits: Limits,
    ) -> &'static Remote {
        let remote: &'static Remote = Box::leak(Box::new(Remote {
            me: OnceLock::new(),
            name: name.to_string(),
            process,
            start,
            limits,
            state: Mutex::new(State::Idle),
            starts: Mutex::new(VecDeque::new()),
            declared: OnceLock::new(),
            ids: AtomicU64::new(1),
            record: Mutex::new(None),
        }));
        let _ = remote.me.set(remote);
        lock(&REMOTES).push(remote);
        remote
    }

    fn this(&self) -> &'static Remote {
        self.me
            .get()
            .copied()
            .expect("a remote is made by Remote::leaked, which sets it")
    }

    /// How its process is started.
    pub fn process(&self) -> &PluginProcess {
        &self.process
    }

    /// Its calls recorded into `record`, or none.
    pub(super) fn record_into(&self, record: Option<&'static Record>) {
        *lock(&self.record) = record;
    }

    /// Its process's id while it runs.
    pub fn pid(&self) -> Option<u32> {
        match &*lock(&self.state) {
            State::Running(connection) if connection.ended().is_none() => connection.pid,
            State::Idle | State::Running(_) | State::Down { .. } | State::Refused { .. } => None,
        }
    }

    /// Why it is down, while it is.
    pub fn down(&self) -> Option<String> {
        match &*lock(&self.state) {
            State::Running(connection) => connection.ended().map(|(_, why)| why),
            State::Down { why, .. } | State::Refused { why, .. } => Some(why.clone()),
            State::Idle => None,
        }
    }

    /// What it declared, starting it first when it never has.
    pub fn declaration(&self) -> Result<&Declaration, String> {
        self.declared().map(|declared| &declared.declaration)
    }

    /// What it declared, starting it first when it never has, or why it has not.
    fn declared(&self) -> Result<&Declared, String> {
        if let Some(declared) = self.declared.get() {
            return Ok(declared);
        }
        self.connection()?;
        self.declared
            .get()
            .ok_or_else(|| "it started and declared nothing".to_string())
    }

    /// `why` as a seam shows it: the plugin named.
    fn failed(&self, why: &str) -> String {
        format!("plugin {}: {why}", self.name)
    }

    /// A line of the host's in its log, stamped.
    fn log(&self, line: &str) {
        self.append(&format!("{} symphony: {line}\n", crate::clock::now_stamp()));
    }

    /// `text` appended to its log in one write, so a line two threads log at once is never cut by
    /// the other's; a log that would grow past its bound is rolled to `NAME.1.log` first.
    fn append(&self, text: &str) {
        static LOGS: Mutex<()> = Mutex::new(());
        let _one = lock(&LOGS);
        let log = &self.process.log;
        if let Some(folder) = log.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let held = std::fs::metadata(log).map_or(0, |meta| meta.len());
        let adding = u64::try_from(text.len()).unwrap_or(u64::MAX);
        if held > 0 && held.saturating_add(adding) > self.limits.log {
            let _ = std::fs::rename(log, measure::rolled(log));
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
        {
            let _ = file.write_all(text.as_bytes());
        }
    }

    /// What tells its program's file from another put in its place: the path its command names, or
    /// the file of that name on `PATH`. None when there is no such file.
    fn program(&self) -> Option<Stamp> {
        let program = self.process.command.first()?;
        let path = if program.contains('/') {
            PathBuf::from(program)
        } else {
            crate::builds::on_path(program, std::env::var_os("PATH").as_deref())?
        };
        Some(Stamp::of(&path))
    }

    /// The connection to its running process, started when it is not and may be.
    fn connection(&self) -> Result<Arc<Connection>, String> {
        let mut state = lock(&self.state);
        let ended = match &*state {
            State::Running(connection) => match connection.ended() {
                None => return Ok(connection.clone()),
                Some(ended) => Some(ended),
            },
            State::Idle | State::Down { .. } | State::Refused { .. } => None,
        };
        if let Some((at, why)) = ended {
            *state = State::Down { at, why };
        }
        match &*state {
            State::Down { at, why } if at.elapsed() < self.limits.pause => {
                return Err(format!("{why}; it starts again in a moment"));
            }
            State::Refused { why, program } if *program == self.program() => {
                return Err(why.clone());
            }
            State::Idle | State::Running(_) | State::Down { .. } | State::Refused { .. } => {}
        }
        let mut starts = lock(&self.starts);
        while starts.front().is_some_and(|at| at.elapsed() >= WINDOW) {
            starts.pop_front();
        }
        if starts.len() >= STARTS {
            let why = match &*state {
                State::Down { why, .. } | State::Refused { why, .. } => why.clone(),
                State::Idle | State::Running(_) => String::new(),
            };
            return Err(format!(
                "{why}; started {STARTS} times in {} s, it is not started again for now",
                WINDOW.as_secs()
            ));
        }
        starts.push_back(Instant::now());
        drop(starts);
        let program = self.program();
        match self.started() {
            Ok(connection) => {
                *state = State::Running(connection.clone());
                Ok(connection)
            }
            Err(Unstarted::Failed(why)) => {
                self.log(&why);
                *state = State::Down {
                    at: Instant::now(),
                    why: why.clone(),
                };
                Err(why)
            }
            Err(Unstarted::Refused(why)) => {
                self.log(&why);
                *state = State::Refused {
                    why: why.clone(),
                    program,
                };
                Err(why)
            }
        }
    }

    /// Its process started and declared: the connection, or why there is none. Every lock this
    /// takes is the connection's, as its caller holds the state's.
    fn started(&self) -> Result<Arc<Connection>, Unstarted> {
        let program = self
            .process
            .command
            .first()
            .map_or("its command", String::as_str);
        let spawned = (self.start)()
            .map_err(|why| Unstarted::Failed(format!("{program} cannot start: {why}")))?;
        let (queue, lines) = mpsc::sync_channel(QUEUED);
        let (errors_ended, errors_end) = mpsc::channel();
        let connection = Arc::new(Connection {
            pid: spawned.pid,
            agreed: OnceLock::new(),
            queue: Mutex::new(Some(queue)),
            queued: AtomicU64::new(0),
            written: AtomicU64::new(0),
            process: Mutex::new(spawned.process),
            errors: Mutex::new(spawned.errors.is_some().then_some(errors_end)),
            waiting: Mutex::new(Waiting::default()),
            ended: Condvar::new(),
        });
        let pid = connection.pid();
        self.log(&format!("started{pid}: {}", self.process.command.join(" ")));
        let threads = self.threads(
            &connection,
            (spawned.input, lines),
            spawned.output,
            spawned.errors.map(|errors| (errors, errors_ended)),
        );
        if let Err(why) = threads {
            self.end(
                &connection,
                Some(format!("its threads cannot start: {why}")),
            );
            return Err(Unstarted::Failed(connection.closed()));
        }
        let declaration = match self.declaration_on(&connection) {
            Ok(declaration) => declaration,
            Err(why) => {
                self.end(&connection, Some(format!("it did not declare: {why}")));
                return Err(Unstarted::Failed(format!(
                    "its process{pid} did not declare: {why}"
                )));
            }
        };
        let agreed = PROTOCOL
            .agreed(declaration.protocol)
            .and_then(|agreed| self.answerable(&declaration).map(|()| agreed));
        let agreed = match agreed {
            Ok(agreed) => agreed,
            Err(why) => {
                self.end(&connection, Some(format!("it is refused: {why}")));
                return Err(Unstarted::Refused(format!(
                    "its process{pid} is refused: {why}; it is not started again until its \
                     program or the project's settings change"
                )));
            }
        };
        let _ = connection.agreed.set(agreed);
        if let Some(kept) = self.declared.get() {
            if kept.declaration != declaration {
                self.log(
                    "its declaration changed since its first start: the objects it declared then \
                     stand for it until this process ends",
                );
            }
        } else {
            let _ = self.declared.set(Declared::of(self.this(), declaration));
        }
        Ok(connection)
    }

    /// Why `declaration` is refused here, if it is: a plugin declaring itself by another name than
    /// the project gives it, or declaring a seam a process does not answer, which would be lost.
    fn answerable(&self, declaration: &Declaration) -> Result<(), String> {
        if declaration.name != self.name {
            return Err(format!(
                "it declares the plugin {:?}, not {:?}, which the project names",
                declaration.name, self.name
            ));
        }
        let others = unanswered(declaration);
        if others.is_empty() {
            return Ok(());
        }
        Err(format!(
            "it declares {}, which a plugin's process does not answer: link the plugin, or serve \
             one without them",
            others.join(", ")
        ))
    }

    /// The threads of `connection`: one writing the lines queued to its stdin, one reading its
    /// stdout, and one its stderr, telling when that ended.
    fn threads(
        &self,
        connection: &Arc<Connection>,
        (input, lines): (Box<dyn Write + Send>, mpsc::Receiver<Vec<u8>>),
        output: Box<dyn io::Read + Send>,
        errors: Option<(Box<dyn io::Read + Send>, mpsc::Sender<()>)>,
    ) -> io::Result<()> {
        let this = self.this();
        let named =
            |what: &str| std::thread::Builder::new().name(format!("plugin {} {what}", self.name));
        let reading = connection.clone();
        named("reads").spawn(move || this.read(&reading, output))?;
        let writing = connection.clone();
        named("writes").spawn(move || this.write(&writing, input, &lines))?;
        if let Some((errors, ended)) = errors {
            named("errors").spawn(move || {
                this.errors(errors);
                let _ = ended.send(());
            })?;
        }
        Ok(())
    }

    /// The lines queued for `connection` written to its stdin, `input`, until its queue closes. A
    /// write that fails ends it, unless its process ended first, which its reader says.
    fn write(
        &self,
        connection: &Connection,
        mut input: Box<dyn Write + Send>,
        lines: &mpsc::Receiver<Vec<u8>>,
    ) {
        for line in lines {
            if let Err(why) = input.write_all(&line).and_then(|()| input.flush()) {
                drop(input);
                let deadline = Instant::now() + GRACE;
                while !lock(&connection.waiting).ending && Instant::now() < deadline {
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "no-timed-waits baseline: looks again for a flag another thread sets, where a condvar is the event"
                    )]
                    std::thread::sleep(Duration::from_millis(2));
                }
                self.end(
                    connection,
                    Some(format!("its stdin could not be written: {why}")),
                );
                return;
            }
            connection.written.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// The answers of `connection`'s process read from its stdout, `output`, until it ends, each
    /// handed to its call; then the connection ended. A read that fails or a line too long ends it.
    fn read(&self, connection: &Connection, output: Box<dyn io::Read + Send>) {
        let mut output = BufReader::new(output);
        let failed = loop {
            match read_line(&mut output, self.limits.line) {
                Ok(Lined::Line(line)) => self.answer(connection, &line),
                Ok(Lined::End) => break None,
                Ok(Lined::TooLong(head)) => {
                    break Some(format!(
                        "it wrote a line longer than the protocol's {} bytes: {}…",
                        self.limits.line,
                        shown(&head[..head.len().min(SHOWN)])
                    ));
                }
                Err(why) => break Some(format!("its stdout could not be read: {why}")),
            }
        };
        self.end(connection, failed);
    }

    /// The line `line` of `connection`'s process handed to the call it answers; logged when it
    /// answers none, or a call given up.
    fn answer(&self, connection: &Connection, line: &[u8]) {
        if line.iter().all(u8::is_ascii_whitespace) {
            return;
        }
        let read = serde_json::from_slice::<Raw>(line).map_err(|e| e.to_string());
        let answer = read.and_then(|read| {
            let (id, us) = (read.id, read.us);
            match read.member()? {
                Member::Answered(answered) => Ok((id, us, answered)),
                Member::Request(_) | Member::Cancel => {
                    Err(format!("call {id}: a plugin sends answers only"))
                }
            }
        });
        let (id, us, answered) = match answer {
            Ok(answer) => answer,
            Err(why) => {
                self.log(&format!(
                    "a line that answers no call ({why}): {}",
                    shown(line)
                ));
                return;
            }
        };
        let mut waiting = lock(&connection.waiting);
        if let Some(call) = waiting.calls.remove(&id) {
            drop(waiting);
            let bytes = line.len() + 1;
            let _ = call.send(Ok(Reply {
                answered,
                us,
                bytes,
            }));
        } else if waiting.given_up.remove(&id) {
            drop(waiting);
            self.log(&format!(
                "call {id} was answered after it was given up: dropped"
            ));
        } else {
            drop(waiting);
            self.log(&format!(
                "a line that answers no call (no call {id} waits): {}",
                shown(line)
            ));
        }
    }

    /// Each line of `errors`, a process's stderr, appended to its log as it said it, until it ends.
    fn errors(&self, errors: Box<dyn io::Read + Send>) {
        let mut errors = BufReader::new(errors);
        loop {
            let mut said = match read_line(&mut errors, ERROR_LINE) {
                Ok(Lined::Line(line)) => String::from_utf8_lossy(&line).into_owned(),
                Ok(Lined::TooLong(head)) => String::from_utf8_lossy(&head).into_owned() + "…",
                Ok(Lined::End) | Err(_) => return,
            };
            said.push('\n');
            self.append(&said);
        }
    }

    /// `connection` ended, once: its stdin closed, its process killed when the host ends it for
    /// `reason` or waited for when it ended of itself (`reason` none), its end logged, and every
    /// call waiting on it failed with why. The remote is down from then on.
    fn end(&self, connection: &Connection, reason: Option<String>) {
        {
            let mut waiting = lock(&connection.waiting);
            if waiting.ending {
                return;
            }
            waiting.ending = true;
        }
        lock(&connection.queue).take();
        let how = {
            let mut process = lock(&connection.process);
            if reason.is_some() {
                process.kill();
            }
            reaped(&mut **process)
        };
        if let Some(errors) = lock(&connection.errors).take() {
            let _ = errors.recv_timeout(GRACE);
        }
        let pid = connection.pid();
        let why = match reason {
            Some(reason) => format!("its process{pid} was ended: {reason}"),
            None => format!("its process{pid} {how}"),
        };
        self.log(&why);
        let calls = {
            let mut waiting = lock(&connection.waiting);
            waiting.ended = Some((Instant::now(), why.clone()));
            waiting.given_up.clear();
            std::mem::take(&mut waiting.calls)
        };
        connection.ended.notify_all();
        for call in calls.into_values() {
            let _ = call.send(Err(why.clone()));
        }
    }

    /// The call `request` asked of the plugin, recorded under the project's settings: its answer,
    /// or why there is none. Only the reading of its answer is the call's own; the rest is one
    /// body for every call ([`Self::asked`]), so each seam a process answers adds little to the
    /// binary.
    pub fn ask<A: Ask>(&self, name: &str, request: A) -> Result<A::Answer, Failure> {
        let mut answer = None;
        self.asked(name, Call::of::<A>(), request.into(), &mut |raw| {
            answer = Some(answer_of(A::TAG, raw)?);
            Ok(())
        })?;
        answer.ok_or_else(|| Failure::Down(format!("its answer to {} was not read", A::TAG)))
    }

    /// [`Self::ask`] of every call: `request` sent as `call`, recorded, and its answer handed to
    /// `read`; nothing once `read` took it, or why there is no answer.
    #[inline(never)]
    fn asked(
        &self,
        name: &str,
        call: Call,
        request: Request,
        read: ReadAnswer<'_>,
    ) -> Result<(), Failure> {
        if !call.piped {
            return Err(Failure::Down(format!(
                "{} is not asked of a plugin's process",
                call.tag
            )));
        }
        let started = Instant::now();
        let asked = match self.connection() {
            Ok(connection) => {
                let agreed = connection.agreed.get().copied().unwrap_or(PROTOCOL);
                if call.since > agreed.minor {
                    return Err(Failure::Newer(agreed));
                }
                // How the process ended is in its log already, once.
                self.sent_within(&connection, request, self.limits.wait, read)
                    .inspect_err(|why| {
                        if !why.starts_with("its process") {
                            self.log(&format!("{} of {name}: {why}", call.tag));
                        }
                    })
            }
            Err(why) => Err(why),
        };
        let round_trip = started.elapsed();
        let record = *lock(&self.record);
        if let Some(record) = record {
            let (seam, call) = call.names;
            let (us, request_bytes, answer_bytes, failed) = match &asked {
                Ok((sent, reply)) => (reply.us, Some(*sent), Some(reply.bytes), None),
                Err(why) => (None, None, None, Some(why.clone())),
            };
            record.write(&Line {
                at_ms: measure::now_ms(),
                pid: std::process::id(),
                plugin: &self.name,
                path: measure::PROCESS,
                seam,
                name,
                call,
                us,
                round_trip_us: Some(measure::micros(round_trip)),
                request_bytes,
                answer_bytes,
                lost: None,
                failed,
            });
        }
        asked.map(|_| ()).map_err(Failure::Down)
    }

    /// The plugin's declaration on `connection`, waited for [`Limits::declare`].
    fn declaration_on(&self, connection: &Arc<Connection>) -> Result<Declaration, String> {
        let mut declared = None;
        let asked = Declare::new(PROTOCOL).into();
        self.sent_within(connection, asked, self.limits.declare, &mut |raw| {
            declared = Some(answer_of(Declare::TAG, raw)?);
            Ok(())
        })?;
        declared.ok_or_else(|| format!("its answer to {} was not read", Declare::TAG))
    }

    /// `request` sent on `connection` and its answer handed to `read` within `wait`: the request
    /// line's bytes and the reply; or why there is none.
    fn sent_within(
        &self,
        connection: &Arc<Connection>,
        request: Request,
        wait: Duration,
        read: ReadAnswer<'_>,
    ) -> Result<(usize, Reply), String> {
        let id = self.ids.fetch_add(1, Ordering::Relaxed);
        let mut line = serde_json::to_vec(&Envelope::<&Request, ()>::asking(id, &request))
            .map_err(|why| format!("the request cannot be written: {why}"))?;
        if line.len() > self.limits.line {
            return Err(format!(
                "its request is a line of {} bytes, longer than the protocol's {}",
                line.len(),
                self.limits.line
            ));
        }
        line.push(b'\n');
        let bytes = line.len();
        let (answer, answered) = mpsc::channel();
        {
            let mut waiting = lock(&connection.waiting);
            if waiting.ending {
                drop(waiting);
                return Err(connection.closed());
            }
            waiting.calls.insert(id, answer);
        }
        let sent = self.queued(connection, line).inspect_err(|_| {
            lock(&connection.waiting).calls.remove(&id);
        })?;
        let reply = match answered.recv_timeout(wait) {
            Ok(reply) => reply?,
            Err(mpsc::RecvTimeoutError::Timeout) => match self.given_up(connection, id, sent, wait)
            {
                // Answered, or failed, as it was given up.
                None => answered
                    .recv()
                    .map_err(|_| "its answers stopped".to_string())??,
                Some(why) => return Err(why),
            },
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("its answers stopped".into());
            }
        };
        read(&reply.answered)?;
        Ok((bytes, reply))
    }

    /// `line` queued for `connection`'s stdin: its number among the lines queued, or why it is not.
    /// A queue full, its lines not taken, ends the connection.
    fn queued(&self, connection: &Connection, line: Vec<u8>) -> Result<u64, String> {
        {
            let held = lock(&connection.queue);
            let Some(queue) = held.as_ref() else {
                drop(held);
                return Err(connection.closed());
            };
            match queue.try_send(line) {
                Ok(()) => return Ok(connection.queued.fetch_add(1, Ordering::SeqCst) + 1),
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Err("its stdin is closed".into());
                }
                Err(mpsc::TrySendError::Full(_)) => {}
            }
        }
        self.end(
            connection,
            Some(format!(
                "its stdin took none of the {QUEUED} lines queued for it"
            )),
        );
        Err(connection.closed())
    }

    /// The call `id`, its request the `sent`th line queued, given up after its `wait`: why it has
    /// no answer, or none when it was answered or failed meanwhile. A process that did not take
    /// even its request in that wait reads no more and is ended. Else the call is cancelled, and a
    /// process that has not answered it [`Limits::cancel`] later is ended ([`Self::after_cancel`]).
    fn given_up(
        &self,
        connection: &Arc<Connection>,
        id: u64,
        sent: u64,
        wait: Duration,
    ) -> Option<String> {
        {
            let mut waiting = lock(&connection.waiting);
            waiting.calls.remove(&id)?;
            waiting.given_up.insert(id);
        }
        if connection.written.load(Ordering::SeqCst) < sent {
            self.end(
                connection,
                Some(format!("its stdin took no line in {wait:?}")),
            );
            return Some(connection.closed());
        }
        if let Ok(mut cancel) = serde_json::to_vec(&Envelope::<(), ()>::cancelling(id)) {
            cancel.push(b'\n');
            let _ = self.queued(connection, cancel);
        }
        let (this, cancelled, cancel) = (self.this(), connection.clone(), self.limits.cancel);
        // With no thread to watch it, the call stays given up until the process ends or answers.
        let _ = std::thread::Builder::new()
            .name(format!("plugin {} call {id}", self.name))
            .spawn(move || {
                #[expect(clippy::disallowed_methods, reason = "no-timed-waits baseline: a thread sleeps out a call's deadline, where a deadline on the wait itself is the bound")]
                std::thread::sleep(cancel);
                this.after_cancel(&cancelled, id);
            });
        Some(format!("no answer in {wait:?}"))
    }

    /// The call `id`, given up and sent a `cancel`, once [`Limits::cancel`] has passed since: a
    /// process that has not answered it is ended, and one that has is kept.
    fn after_cancel(&self, connection: &Connection, id: u64) {
        if lock(&connection.waiting).given_up.contains(&id) {
            let cancel = self.limits.cancel;
            self.end(
                connection,
                Some(format!(
                    "call {id} had no answer {cancel:?} after it was cancelled"
                )),
            );
        }
    }
}

impl Remote {
    /// The objects `pick` takes of what it declared, as its seams answer: none when it never
    /// declared.
    fn each<'a, T: ?Sized>(&'a self, pick: impl FnOnce(&'a Declared) -> Vec<&'a T>) -> Vec<&'a T> {
        self.declared().map_or_else(|_| Vec::new(), pick)
    }
}

impl Plugin for Remote {
    fn name(&self) -> &str {
        &self.name
    }
    fn components(&self) -> Vec<&dyn Component> {
        self.each(|d| {
            d.components
                .iter()
                .map(|c| -> &dyn Component { c })
                .collect()
        })
    }
    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        self.each(|d| {
            d.card_kinds
                .iter()
                .map(|k| -> &dyn CardKind { k })
                .collect()
        })
    }
    fn renderers(&self) -> Vec<&dyn Renderer> {
        self.each(|d| d.renderers.iter().map(|r| -> &dyn Renderer { r }).collect())
    }
    fn routes(&self) -> Vec<&dyn Route> {
        self.each(|d| d.routes.iter().map(|r| -> &dyn Route { r }).collect())
    }
    fn nav(&self) -> Vec<NavItem> {
        self.declaration()
            .map_or_else(|_| Vec::new(), |declared| declared.nav.clone())
    }
    fn home_regions(&self) -> Vec<&dyn HomeRegion> {
        self.each(|d| {
            d.home_regions
                .iter()
                .map(|r| -> &dyn HomeRegion { r })
                .collect()
        })
    }
    fn board_actions(&self) -> Vec<&dyn BoardAction> {
        self.each(|d| {
            d.board_actions
                .iter()
                .map(|a| -> &dyn BoardAction { a })
                .collect()
        })
    }
    fn authorities(&self) -> Vec<&dyn Authority> {
        self.each(|d| {
            d.authorities
                .iter()
                .map(|a| -> &dyn Authority { a })
                .collect()
        })
    }
    fn watches(&self) -> Vec<&dyn Watch> {
        self.each(|d| d.watches.iter().map(|w| -> &dyn Watch { w }).collect())
    }
    fn doc_checks(&self) -> Vec<&dyn DocCheck> {
        self.each(|d| {
            d.doc_checks
                .iter()
                .map(|c| -> &dyn DocCheck { c })
                .collect()
        })
    }
    fn commands(&self) -> Vec<&dyn Command> {
        self.each(|d| d.commands.iter().map(|c| -> &dyn Command { c }).collect())
    }
    fn theme(&self) -> Option<Theme> {
        self.declaration().ok()?.theme.clone()
    }
}

/// What a remote declared, and the objects standing for its seams'.
struct Declared {
    declaration: Declaration,
    components: Vec<RemoteComponent>,
    card_kinds: Vec<RemoteKind>,
    renderers: Vec<RemoteRenderer>,
    routes: Vec<RemoteRoute>,
    home_regions: Vec<RemoteRegion>,
    board_actions: Vec<RemoteAction>,
    authorities: Vec<RemoteAuthority>,
    watches: Vec<RemoteWatch>,
    doc_checks: Vec<RemoteCheck>,
    commands: Vec<RemoteCommand>,
}

/// `text` kept for the process's life, as the declaration is: what a seam's object lends as a
/// `&str` of its own (a card kind's markers, a renderer's extensions) is borrowed from it.
fn kept(text: &str) -> &'static str {
    Box::leak(text.to_string().into_boxed_str())
}

impl Declared {
    fn of(remote: &'static Remote, declaration: Declaration) -> Declared {
        Declared {
            components: declaration
                .components
                .iter()
                .map(|name| RemoteComponent {
                    remote,
                    name: name.clone(),
                })
                .collect(),
            card_kinds: declaration
                .card_kinds
                .iter()
                .map(|kind| RemoteKind {
                    remote,
                    name: kind.name.clone(),
                    markers: kind.markers.iter().map(|marker| kept(marker)).collect(),
                })
                .collect(),
            renderers: declaration
                .renderers
                .iter()
                .map(|renderer| RemoteRenderer {
                    remote,
                    name: renderer.name.clone(),
                    extensions: renderer.extensions.iter().map(|e| kept(e)).collect(),
                    content_type: renderer.content_type.clone(),
                })
                .collect(),
            routes: declaration
                .routes
                .iter()
                .map(|path| RemoteRoute {
                    remote,
                    path: path.clone(),
                })
                .collect(),
            home_regions: declaration
                .home_regions
                .iter()
                .map(|id| RemoteRegion {
                    remote,
                    id: id.clone(),
                })
                .collect(),
            board_actions: declaration
                .board_actions
                .iter()
                .map(|name| RemoteAction {
                    remote,
                    name: name.clone(),
                })
                .collect(),
            authorities: declaration
                .authorities
                .iter()
                .map(|adapter| RemoteAuthority {
                    remote,
                    adapter: adapter.clone(),
                })
                .collect(),
            watches: declaration
                .watches
                .iter()
                .enumerate()
                .map(|(index, paths)| RemoteWatch {
                    remote,
                    index,
                    paths: paths.clone(),
                })
                .collect(),
            doc_checks: declaration
                .doc_checks
                .iter()
                .map(|check| RemoteCheck {
                    remote,
                    name: check.name.clone(),
                    inputs: check.inputs.clone(),
                })
                .collect(),
            commands: declaration
                .commands
                .iter()
                .map(|command| RemoteCommand {
                    remote,
                    name: command.name.clone(),
                    summary: command.summary.clone(),
                })
                .collect(),
            declaration,
        }
    }
}

/// A component the remote declared.
struct RemoteComponent {
    remote: &'static Remote,
    name: String,
}

/// A component's seam with only what it requires, so each of its other methods is the trait's
/// own default: what answers a call newer than the minor agreed.
struct ComponentDefaults<'a>(&'a RemoteComponent);

impl Component for ComponentDefaults<'_> {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn render(
        &self,
        page: &Page<'_>,
        argument: &str,
        option: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        self.0.render(page, argument, option)
    }
}

impl RemoteComponent {
    /// `request` asked of the remote: its answer, a failure shown as the component's error, or for
    /// a call newer than the minor agreed, `newer`'s.
    fn ask<A: Ask, T>(
        &self,
        request: A,
        answer: impl FnOnce(A::Answer) -> Result<T, String>,
        newer: impl FnOnce(Version) -> Result<T, String>,
    ) -> Result<T, String> {
        match self.remote.ask(&self.name, request) {
            Ok(answered) => answer(answered),
            Err(Failure::Newer(agreed)) => newer(agreed),
            Err(Failure::Down(why)) => Err(self.remote.failed(&why)),
        }
    }

    /// What a call the plugin's protocol does not have answers, when the seam requires it.
    fn missing<T>(&self, tag: &str, agreed: Version) -> Result<T, String> {
        Err(self
            .remote
            .failed(&format!("it speaks protocol {agreed}, which has no {tag}")))
    }
}

impl Component for RemoteComponent {
    fn name(&self) -> &str {
        &self.name
    }
    fn render(
        &self,
        page: &Page<'_>,
        argument: &str,
        option: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        self.ask(
            Render::new(
                self.name.clone(),
                PageData::of(page),
                argument.into(),
                option.map(str::to_string),
            ),
            |answer| answer,
            |agreed| self.missing(Render::TAG, agreed),
        )
    }
    fn resolve(&self, page: &Page<'_>, argument: &str, option: Option<&str>) -> Result<(), String> {
        self.ask(
            Resolve::new(
                self.name.clone(),
                PageData::of(page),
                argument.into(),
                option.map(str::to_string),
            ),
            |answer| answer,
            |_| ComponentDefaults(self).resolve(page, argument, option),
        )
    }
    fn cache_version(&self, page: &Page<'_>) -> Option<String> {
        // A component with no version is rendered afresh on every read: what a failure asks too.
        self.ask(
            CacheVersion::new(self.name.clone(), PageData::of(page)),
            Ok,
            |_| Ok(ComponentDefaults(self).cache_version(page)),
        )
        .ok()
        .flatten()
    }
}

/// A card kind the remote declared.
struct RemoteKind {
    remote: &'static Remote,
    name: String,
    markers: Vec<&'static str>,
}

/// A card kind's seam with only what it requires, so each of its other methods is the trait's own
/// default: what answers a call newer than the minor agreed, and one the plugin did not answer.
struct KindDefaults<'a>(&'a RemoteKind);

impl CardKind for KindDefaults<'_> {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn recognises(&self, card: &Card<'_>) -> bool {
        self.0.recognises(card)
    }
}

impl RemoteKind {
    /// `request` asked of the remote: its answer, or `default`'s when it has none.
    fn ask<A: Ask>(&self, request: A, default: impl FnOnce() -> A::Answer) -> A::Answer {
        self.remote
            .ask(&self.name, request)
            .unwrap_or_else(|_| default())
    }

    fn card(&self, card: &Card<'_>) -> CardData {
        CardData::of(card)
    }
}

impl CardKind for RemoteKind {
    fn name(&self) -> &str {
        &self.name
    }
    fn recognises(&self, card: &Card<'_>) -> bool {
        // Required of every kind, so in every minor: a kind that did not answer recognises nothing.
        self.ask(Recognises::new(self.name.clone(), self.card(card)), || {
            false
        })
    }
    fn pictures(&self, root: &Path, card: &Card<'_>) -> Vec<Picture> {
        self.ask(
            Pictures::new(self.name.clone(), root.to_path_buf(), self.card(card)),
            || KindDefaults(self).pictures(root, card),
        )
    }
    fn slot(&self, card: &Card<'_>) -> Option<String> {
        self.ask(Slot::new(self.name.clone(), self.card(card)), || {
            KindDefaults(self).slot(card)
        })
    }
    fn markers(&self) -> &[&str] {
        &self.markers
    }
    fn triage_category(&self, card: &Card<'_>) -> Option<String> {
        self.ask(
            TriageCategory::new(self.name.clone(), self.card(card)),
            || KindDefaults(self).triage_category(card),
        )
    }
    fn auto_approvable(&self, card: &Card<'_>) -> bool {
        self.ask(
            AutoApprovable::new(self.name.clone(), self.card(card)),
            || KindDefaults(self).auto_approvable(card),
        )
    }
    fn gates(&self, card: &Card<'_>) -> Option<(usize, String)> {
        self.ask(CardGates::new(self.name.clone(), self.card(card)), || {
            KindDefaults(self).gates(card)
        })
    }
    fn approval_needs_agent(&self, card: &Card<'_>) -> bool {
        self.ask(
            ApprovalNeedsAgent::new(self.name.clone(), self.card(card)),
            || KindDefaults(self).approval_needs_agent(card),
        )
    }
    fn chosen(&self, card: &Card<'_>) -> Result<Option<String>, String> {
        self.ask(Chosen::new(self.name.clone(), self.card(card)), || {
            KindDefaults(self).chosen(card)
        })
    }
}

impl Remote {
    /// Every card kind's answers of each of `cards`, one register read's, its pictures read in
    /// the checkout at `root`: asked in as few requests as keep each line within the bound
    /// ([`ReadCards`]), and kept for that read alone. None from a plugin that speaks a minor
    /// before the batch, declares no card kind or answered none of them: its kinds are asked card
    /// by card. The cards of a request that had no answer, and a card too long for a line alone,
    /// are asked card by card too.
    pub fn read_cards(&self, root: &Path, cards: &[CardData]) -> Option<Read> {
        let this = self.this();
        let declared = this.declared().ok()?;
        if declared.card_kinds.is_empty() || cards.is_empty() {
            return None;
        }
        let name = declared
            .card_kinds
            .iter()
            .map(|kind| kind.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let mut answered = HashMap::new();
        for chunk in self.chunks(root, cards) {
            let asked = ReadCards::new(root.to_path_buf(), chunk.to_vec());
            match self.ask(&name, asked) {
                Ok(read) if read.len() == chunk.len() => {
                    answered.extend(chunk.iter().cloned().zip(read));
                }
                Ok(read) => self.log(&format!(
                    "{} of {name}: {} answers to {} cards; they are asked card by card",
                    ReadCards::TAG,
                    read.len(),
                    chunk.len()
                )),
                Err(Failure::Newer(_)) => return None,
                // Why is in its log: these cards are asked one by one.
                Err(Failure::Down(_)) => {}
            }
        }
        if answered.is_empty() {
            return None;
        }
        let answers = Arc::new(Answers {
            root: root.to_path_buf(),
            cards: answered,
        });
        Some(Read {
            remote: this,
            kinds: declared
                .card_kinds
                .iter()
                .map(|kind| ReadKind {
                    kind: kind.name.as_str(),
                    remote: kind,
                    answers: answers.clone(),
                })
                .collect(),
        })
    }

    /// `cards` in runs, in their order, each as many as one [`ReadCards`] line holds within the
    /// bound, its id the longest an id is; a card too long for a line alone is in none.
    fn chunks<'c>(&self, root: &Path, cards: &'c [CardData]) -> Vec<&'c [CardData]> {
        let empty = ReadCards::new(root.to_path_buf(), Vec::new());
        let Ok(base) = serde_json::to_vec(&Envelope::<Request, ()>::asking(u64::MAX, empty.into()))
        else {
            return Vec::new();
        };
        let (base, line) = (base.len(), self.limits.line);
        let mut chunks = Vec::new();
        // The run being made: where it starts and the length of its line so far.
        let mut run: Option<(usize, usize)> = None;
        for (at, card) in cards.iter().enumerate() {
            let size = serde_json::to_vec(card).map_or(usize::MAX, |card| card.len());
            if base.saturating_add(size) > line {
                if let Some((start, _)) = run.take() {
                    chunks.push(&cards[start..at]);
                }
                continue;
            }
            run = match run {
                // A comma before each card but the first.
                Some((start, length)) if length + 1 + size <= line => {
                    Some((start, length + 1 + size))
                }
                Some((start, _)) => {
                    chunks.push(&cards[start..at]);
                    Some((at, base + size))
                }
                None => Some((at, base + size)),
            };
        }
        if let Some((start, _)) = run {
            chunks.push(&cards[start..]);
        }
        chunks
    }
}

/// What one register read had of a plugin's process for every card it holds, in one go
/// ([`Remote::read`]): kinds standing for the remote's, each answering a card the read holds from
/// what the process said of it, and any other card, or the pictures of another checkout, as the
/// remote does, card by card. Kept for the read alone, so nothing it says outlives the files and
/// threads it was made of.
pub struct Read {
    remote: &'static Remote,
    kinds: Vec<ReadKind>,
}

impl Read {
    /// Whether it holds the answers of `plugin`.
    pub fn of(&self, plugin: &dyn Plugin) -> bool {
        std::ptr::addr_eq(self.remote, plugin)
    }

    /// The remote's card kinds, answering the read's cards from it.
    pub fn card_kinds(&self) -> Vec<&dyn CardKind> {
        self.kinds
            .iter()
            .map(|kind| -> &dyn CardKind { kind })
            .collect()
    }
}

/// What a process said of a read's cards: each card's kinds' answers, its pictures read at `root`.
struct Answers {
    root: PathBuf,
    cards: HashMap<CardData, Vec<KindAnswers>>,
}

/// A card kind of a remote, answering a read's cards from what its process said of them.
struct ReadKind {
    kind: &'static str,
    remote: &'static RemoteKind,
    answers: Arc<Answers>,
}

impl ReadKind {
    /// What the process said of `card` for this kind, when the read holds it.
    fn answered(&self, card: &Card<'_>) -> Option<&KindAnswers> {
        self.answers
            .cards
            .get(&CardData::of(card))?
            .iter()
            .find(|answers| answers.kind == self.kind)
    }
}

impl CardKind for ReadKind {
    fn name(&self) -> &str {
        self.kind
    }
    fn recognises(&self, card: &Card<'_>) -> bool {
        self.answered(card)
            .map_or_else(|| self.remote.recognises(card), |said| said.recognises)
    }
    fn pictures(&self, root: &Path, card: &Card<'_>) -> Vec<Picture> {
        match self.answered(card) {
            Some(said) if root == self.answers.root => said.pictures.clone(),
            Some(_) | None => self.remote.pictures(root, card),
        }
    }
    fn slot(&self, card: &Card<'_>) -> Option<String> {
        self.answered(card)
            .map_or_else(|| self.remote.slot(card), |said| said.slot.clone())
    }
    fn markers(&self) -> &[&str] {
        self.remote.markers()
    }
    fn triage_category(&self, card: &Card<'_>) -> Option<String> {
        self.answered(card).map_or_else(
            || self.remote.triage_category(card),
            |said| said.triage_category.clone(),
        )
    }
    fn auto_approvable(&self, card: &Card<'_>) -> bool {
        self.answered(card).map_or_else(
            || self.remote.auto_approvable(card),
            |said| said.auto_approvable,
        )
    }
    fn gates(&self, card: &Card<'_>) -> Option<(usize, String)> {
        self.answered(card)
            .map_or_else(|| self.remote.gates(card), |said| said.gates.clone())
    }
    fn approval_needs_agent(&self, card: &Card<'_>) -> bool {
        self.answered(card).map_or_else(
            || self.remote.approval_needs_agent(card),
            |said| said.approval_needs_agent,
        )
    }
    fn chosen(&self, card: &Card<'_>) -> Result<Option<String>, String> {
        self.answered(card)
            .map_or_else(|| self.remote.chosen(card), |said| said.chosen.clone())
    }
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;

#[path = "process_seams.rs"]
mod seams;
