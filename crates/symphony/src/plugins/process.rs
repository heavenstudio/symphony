//! A plugin that answers from a process of its own (`[plugins.process]`, docs/PLUGIN_PROTOCOL.md):
//! the host's side of the protocol. Its project names it in `[plugins] crates` as a linked plugin
//! is named, and its command in `[plugins.process]`; [`made`] gives the one [`Remote`] for that
//! name, command and checkout in this process, kept for the process's life, which stands in the
//! project's plugins where a linked plugin would.
//!
//! **Its process.** Started at the first call made of it, in the project's checkout, without the
//! session ([`crate::children::command`]), its stdin and stdout the protocol's and its stderr
//! appended to its log (`plugins/NAME.log` in the artifacts folder), with a line there for each
//! start and end. The first request declares: the plugin's [`Declaration`], the minor the two agree
//! ([`Version::agreed`]); a plugin of another major is refused by name and its process left to
//! end. One thread reads its answers and hands each to the call its id names; a call waits
//! [`WAIT`] for its answer, and one past it is given up (a `cancel` is sent). A process that ends
//! fails its calls waiting with how it ended, and is started again by the first call [`PAUSE`]
//! after: at most [`STARTS`] starts in [`WINDOW`], then it stays down until the oldest leaves the
//! window. Its stdin ends with this process, which ends it.
//!
//! **Its seams.** What the plugin declared stands for it here: its components and its card kinds,
//! each forwarding its calls. A call newer than the agreed minor is not sent: the seam's own default
//! answers it. A call that gets no answer (the process down, no answer in time, a refusal, an answer
//! that does not read) is shown where its answer would go: a component's in its place on the page
//! ([`super::error`]), a card kind's as the kind's default, as if the plugin said nothing of the
//! card. A page's components share their `once` values in the plugin as they do linked: every call
//! of one [`Page`] names its render ([`PageData::render`]).
//!
//! **Measured.** Under `[plugins] timing` or `loopback` each call writes its line to the project's
//! record ([`super::measure`]) as a process call: the plugin's own time, as it says it, the host's
//! whole wait for the answer, and the bytes of the two lines.

use std::collections::{BTreeMap, VecDeque};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use serde_json::value::RawValue;
use symphony_plugin::protocol::{
    Answered, ApprovalNeedsAgent, Ask, AutoApprovable, CacheVersion, CardData, CardGates, Chosen,
    Declaration, Declare, Envelope, Member, PROTOCOL, PageData, Pictures, Raw, Recognises, Render,
    Request, Resolve, Slot, TriageCategory, Version,
};
use symphony_plugin::vocabulary::{Block, Picture};
use symphony_plugin::{Card, CardKind, Component, Page, Plugin};

use super::measure::{self, Line, Record};
use crate::project::PluginProcess;

/// How long a call waits for its answer, as the gates seam waits for its answerers.
pub const WAIT: Duration = Duration::from_secs(10);
/// How long after its process ended a plugin is started again: the calls meanwhile show it down.
pub const PAUSE: Duration = Duration::from_secs(1);
/// How many starts a plugin's process gets in [`WINDOW`].
pub const STARTS: usize = 5;
/// The window [`STARTS`] counts in.
pub const WINDOW: Duration = Duration::from_secs(180);

/// The plugin `name` of a project, answering from the process `process` starts: the one this
/// process holds for that name, command and checkout, made at the first ask.
pub fn made(name: &str, process: &PluginProcess) -> &'static Remote {
    static MADE: LazyLock<Mutex<Vec<&'static Remote>>> = LazyLock::new(|| Mutex::new(Vec::new()));
    let mut made = MADE.lock().unwrap_or_else(PoisonError::into_inner);
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
        PAUSE,
        WAIT,
    );
    made.push(remote);
    remote
}

/// The remote `plugin` is, when it is one: a plugin of a project answering from its own process.
pub fn remote(plugin: &dyn Plugin) -> Option<&'static Remote> {
    REMOTES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .copied()
        .find(|remote| std::ptr::addr_eq(*remote, plugin))
}

/// Every remote made in this process, made by [`made`] or a test's peer.
static REMOTES: LazyLock<Mutex<Vec<&'static Remote>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// A process started, as the host speaks with it.
pub struct Spawned {
    /// Its stdin: where the host's lines go.
    pub input: Box<dyn Write + Send>,
    /// Its stdout: where its answers come from.
    pub output: Box<dyn Read + Send>,
    pub pid: Option<u32>,
    /// Waits for it to end, and says how it ended (`exited with code 0`, `was killed by signal
    /// 9`).
    pub ended: Box<dyn FnOnce() -> String + Send>,
}

/// What starts a plugin's process.
pub type Start = Box<dyn Fn() -> io::Result<Spawned> + Send + Sync>;

/// `process` started: its program in its checkout, without the session, its stderr appended to
/// its log.
fn spawn(process: &PluginProcess) -> io::Result<Spawned> {
    let (program, args) = process
        .command
        .split_first()
        .ok_or_else(|| io::Error::other("it names no program"))?;
    if let Some(folder) = process.log.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&process.log)?;
    let mut command = crate::children::command(program);
    command
        .args(args)
        .current_dir(&process.root)
        .stdin(symphony_board::sys::Stdio::piped())
        .stdout(symphony_board::sys::Stdio::piped())
        .stderr(log);
    let mut child = command.spawn()?;
    let input = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("its stdin was not opened"))?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("its stdout was not opened"))?;
    let pid = child.id();
    Ok(Spawned {
        input: Box::new(input),
        output: Box::new(output),
        pid: Some(pid),
        ended: Box::new(move || match child.wait() {
            Ok(status) => ended(status),
            Err(why) => format!("could not be waited for: {why}"),
        }),
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

/// A project's plugin answering from its own process.
pub struct Remote {
    /// This remote, for its threads and its seams' objects: set once it is made.
    me: OnceLock<&'static Remote>,
    name: String,
    process: PluginProcess,
    start: Start,
    pause: Duration,
    /// How long a call waits for its answer.
    wait: Duration,
    state: Mutex<State>,
    /// When its process was started, the newest last, within [`WINDOW`].
    starts: Mutex<VecDeque<Instant>>,
    /// What it declared at its first start, which its seams' objects stand for.
    declared: OnceLock<Declared>,
    /// The next call's id.
    ids: AtomicU64,
    /// The next start's number.
    generations: AtomicU64,
    /// Where its calls are recorded, when the project's settings say so.
    record: Mutex<Option<&'static Record>>,
}

enum State {
    /// Never started.
    Idle,
    Running(Arc<Connection>),
    /// Its process ended, or could not start, at `at`, as `why` says.
    Down {
        at: Instant,
        why: String,
    },
}

/// One start of a plugin's process: what the host writes to, and the calls waiting on it.
struct Connection {
    generation: u64,
    pid: Option<u32>,
    agreed: OnceLock<Version>,
    input: Mutex<Box<dyn Write + Send>>,
    waiting: Mutex<Waiting>,
}

/// The calls a connection's answers go to, by id; once it has ended, how.
#[derive(Default)]
struct Waiting {
    calls: BTreeMap<u64, mpsc::Sender<Result<Reply, String>>>,
    ended: Option<String>,
}

/// An answer as it came: the `answered` member raw, the plugin's time, the line's length.
struct Reply {
    answered: Box<RawValue>,
    us: Option<f64>,
    bytes: usize,
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
    /// A remote for the plugin `name`, its process started by `start` and started again `pause`
    /// after it ends, each call waiting `wait` for its answer, kept for this process's life.
    pub fn leaked(
        name: &str,
        process: PluginProcess,
        start: Start,
        pause: Duration,
        wait: Duration,
    ) -> &'static Remote {
        let remote: &'static Remote = Box::leak(Box::new(Remote {
            me: OnceLock::new(),
            name: name.to_string(),
            process,
            start,
            pause,
            wait,
            state: Mutex::new(State::Idle),
            starts: Mutex::new(VecDeque::new()),
            declared: OnceLock::new(),
            ids: AtomicU64::new(1),
            generations: AtomicU64::new(1),
            record: Mutex::new(None),
        }));
        let _ = remote.me.set(remote);
        REMOTES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(remote);
        remote
    }

    fn this(&self) -> &'static Remote {
        self.me
            .get()
            .copied()
            .expect("a remote is made by Remote::leaked, which sets it")
    }

    /// Its calls recorded into `record`, or none.
    pub(super) fn record_into(&self, record: Option<&'static Record>) {
        *self.record.lock().unwrap_or_else(PoisonError::into_inner) = record;
    }

    /// Its process's id while it runs.
    pub fn pid(&self) -> Option<u32> {
        match &*self.state.lock().unwrap_or_else(PoisonError::into_inner) {
            State::Running(connection) => connection.pid,
            State::Idle | State::Down { .. } => None,
        }
    }

    /// Why it is down, while it is.
    pub fn down(&self) -> Option<String> {
        match &*self.state.lock().unwrap_or_else(PoisonError::into_inner) {
            State::Down { why, .. } => Some(why.clone()),
            State::Idle | State::Running(_) => None,
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

    /// A line in its log, stamped.
    fn log(&self, line: &str) {
        if let Some(folder) = self.process.log.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let Ok(mut log) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.process.log)
        else {
            return;
        };
        // In one write, so a line two threads log at once is never cut by the other's.
        let stamped = format!("{} symphony: {line}\n", crate::clock::now_stamp());
        let _ = log.write_all(stamped.as_bytes());
    }

    /// The connection to its running process, started when it is not and may be.
    fn connection(&self) -> Result<Arc<Connection>, String> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match &*state {
            State::Running(connection) => return Ok(connection.clone()),
            State::Down { at, why } if at.elapsed() < self.pause => {
                return Err(format!("{why}; it starts again in a moment"));
            }
            State::Idle | State::Down { .. } => {}
        }
        let mut starts = self.starts.lock().unwrap_or_else(PoisonError::into_inner);
        while starts.front().is_some_and(|at| at.elapsed() >= WINDOW) {
            starts.pop_front();
        }
        if starts.len() >= STARTS {
            let why = match &*state {
                State::Down { why, .. } => why.clone(),
                State::Idle | State::Running(_) => String::new(),
            };
            return Err(format!(
                "{why}; started {STARTS} times in {} s, it is not started again for now",
                WINDOW.as_secs()
            ));
        }
        starts.push_back(Instant::now());
        drop(starts);
        match self.started() {
            Ok(connection) => {
                *state = State::Running(connection.clone());
                Ok(connection)
            }
            Err(why) => {
                self.log(&why);
                *state = State::Down {
                    at: Instant::now(),
                    why: why.clone(),
                };
                Err(why)
            }
        }
    }

    /// Its process started and declared: the connection, or why there is none.
    fn started(&self) -> Result<Arc<Connection>, String> {
        let program = self
            .process
            .command
            .first()
            .map_or("its command", String::as_str);
        let spawned = (self.start)().map_err(|why| format!("{program} cannot start: {why}"))?;
        let connection = Arc::new(Connection {
            generation: self.generations.fetch_add(1, Ordering::Relaxed),
            pid: spawned.pid,
            agreed: OnceLock::new(),
            input: Mutex::new(spawned.input),
            waiting: Mutex::new(Waiting::default()),
        });
        let (this, reading, output, ended) = (
            self.this(),
            connection.clone(),
            spawned.output,
            spawned.ended,
        );
        std::thread::Builder::new()
            .name(format!("plugin {}", self.name))
            .spawn(move || this.read(&reading, output, ended))
            .map_err(|why| format!("its answers cannot be read: {why}"))?;
        let pid = connection
            .pid
            .map_or_else(String::new, |pid| format!(" {pid}"));
        self.log(&format!("started{pid}: {}", self.process.command.join(" ")));
        let declared = self
            .sent::<Declare>(&connection, Declare { protocol: PROTOCOL })
            .and_then(|(declaration, _, _)| {
                PROTOCOL
                    .agreed(declaration.protocol)
                    .map(|agreed| (declaration, agreed))
            });
        let (declaration, agreed) = match declared {
            Ok(declared) => declared,
            Err(why) => {
                // Its input ended, the process ends.
                *connection
                    .input
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Box::new(io::sink());
                return Err(format!("its process{pid} did not declare: {why}"));
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

    /// Its process's answers, read until its stdout ends, each handed to its call; then each call
    /// still waiting failed with how the process ended, and the remote marked down.
    fn read(
        &self,
        connection: &Arc<Connection>,
        output: Box<dyn Read + Send>,
        ended: Box<dyn FnOnce() -> String + Send>,
    ) {
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else {
                break;
            };
            let read = serde_json::from_str::<Raw>(&line).map_err(|e| e.to_string());
            let answer = read.and_then(|read| {
                let (id, us) = (read.id, read.us);
                match read.member()? {
                    Member::Answered(answered) => Ok((id, us, answered)),
                    Member::Request(_) | Member::Cancel => {
                        Err(format!("call {id}: a plugin sends answers only"))
                    }
                }
            });
            match answer {
                Ok((id, us, answered)) => {
                    let waiting = connection
                        .waiting
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .calls
                        .remove(&id);
                    if let Some(waiting) = waiting {
                        let bytes = line.len() + 1;
                        let _ = waiting.send(Ok(Reply {
                            answered,
                            us,
                            bytes,
                        }));
                    }
                }
                Err(why) => self.log(&format!("a line that answers no call ({why}): {line}")),
            }
        }
        let pid = connection
            .pid
            .map_or_else(String::new, |pid| format!(" {pid}"));
        let why = format!("its process{pid} {}", ended());
        self.log(&why);
        let calls = {
            let mut waiting = connection
                .waiting
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            waiting.ended = Some(why.clone());
            std::mem::take(&mut waiting.calls)
        };
        for waiting in calls.into_values() {
            let _ = waiting.send(Err(why.clone()));
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let State::Running(running) = &*state
            && running.generation == connection.generation
        {
            *state = State::Down {
                at: Instant::now(),
                why,
            };
        }
    }

    /// The call `request` asked of the plugin, recorded under the project's settings: its answer,
    /// or why there is none.
    pub fn ask<A: Ask>(&self, name: &str, request: A) -> Result<A::Answer, Failure> {
        if !A::PIPED {
            return Err(Failure::Down(format!(
                "{} is not asked of a plugin's process",
                A::TAG
            )));
        }
        let started = Instant::now();
        let asked = match self.connection() {
            Ok(connection) => {
                let agreed = connection.agreed.get().copied().unwrap_or(PROTOCOL);
                if A::SINCE > agreed.minor {
                    return Err(Failure::Newer(agreed));
                }
                // How the process ended is in its log already, once.
                self.sent::<A>(&connection, request).inspect_err(|why| {
                    if !why.starts_with("its process") {
                        self.log(&format!("{} of {name}: {why}", A::TAG));
                    }
                })
            }
            Err(why) => Err(why),
        };
        let round_trip = started.elapsed();
        let record = *self.record.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(record) = record {
            let (seam, call) = A::names();
            let (us, request_bytes, answer_bytes, failed) = match &asked {
                Ok((_, sent, reply)) => (reply.us, Some(*sent), Some(reply.bytes), None),
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
        asked.map(|(answer, _, _)| answer).map_err(Failure::Down)
    }

    /// `request` sent on `connection` and its answer read: the answer, the request line's bytes and
    /// the reply; or why there is none.
    fn sent<A: Ask>(
        &self,
        connection: &Connection,
        request: A,
    ) -> Result<(A::Answer, usize, Reply), String> {
        let id = self.ids.fetch_add(1, Ordering::Relaxed);
        let request: Request = request.into();
        let mut line = serde_json::to_string(&Envelope::<&Request, ()>::asking(id, &request))
            .map_err(|why| format!("the request cannot be written: {why}"))?;
        line.push('\n');
        let (answer, answered) = mpsc::channel();
        {
            let mut waiting = connection
                .waiting
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(ended) = &waiting.ended {
                return Err(ended.clone());
            }
            waiting.calls.insert(id, answer);
        }
        let written = {
            let mut input = connection
                .input
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            input
                .write_all(line.as_bytes())
                .and_then(|()| input.flush())
        };
        let forget = || {
            connection
                .waiting
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .calls
                .remove(&id);
        };
        if let Err(why) = written {
            forget();
            let waiting = connection
                .waiting
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            return Err(waiting
                .ended
                .clone()
                .unwrap_or_else(|| format!("its process cannot be written to: {why}")));
        }
        let reply = match answered.recv_timeout(self.wait) {
            Ok(reply) => reply?,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                forget();
                if let Ok(mut cancel) = serde_json::to_string(&Envelope::<(), ()>::cancelling(id)) {
                    cancel.push('\n');
                    let mut input = connection
                        .input
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    let _ = input
                        .write_all(cancel.as_bytes())
                        .and_then(|()| input.flush());
                }
                return Err(format!("no answer in {:?}", self.wait));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("its answers stopped".into());
            }
        };
        let answer = serde_json::from_str::<Answered<A::Answer>>(reply.answered.get())
            .map_err(|why| format!("its answer to {} cannot be read: {why}", A::TAG))?
            .shown()
            .map_err(|why| format!("it refused {}: {why}", A::TAG))?;
        Ok((answer, line.len(), reply))
    }
}

impl Plugin for Remote {
    fn name(&self) -> &str {
        &self.name
    }
    fn components(&self) -> Vec<&dyn Component> {
        self.declared().map_or_else(
            |_| Vec::new(),
            |declared| {
                declared
                    .components
                    .iter()
                    .map(|component| -> &dyn Component { component })
                    .collect()
            },
        )
    }
    fn card_kinds(&self) -> Vec<&dyn CardKind> {
        self.declared().map_or_else(
            |_| Vec::new(),
            |declared| {
                declared
                    .card_kinds
                    .iter()
                    .map(|kind| -> &dyn CardKind { kind })
                    .collect()
            },
        )
    }
}

/// What a remote declared, and the objects standing for its seams'.
struct Declared {
    declaration: Declaration,
    components: Vec<RemoteComponent>,
    card_kinds: Vec<RemoteKind>,
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
                    // Kept for the process's life, as the declaration is: a kind's markers are
                    // borrowed from it.
                    markers: kind
                        .markers
                        .iter()
                        .map(|marker| &*Box::leak(marker.clone().into_boxed_str()))
                        .collect(),
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
            Render {
                component: self.name.clone(),
                page: PageData::of(page),
                argument: argument.into(),
                option: option.map(str::to_string),
            },
            |answer| answer,
            |agreed| self.missing(Render::TAG, agreed),
        )
    }
    fn resolve(&self, page: &Page<'_>, argument: &str, option: Option<&str>) -> Result<(), String> {
        self.ask(
            Resolve {
                component: self.name.clone(),
                page: PageData::of(page),
                argument: argument.into(),
                option: option.map(str::to_string),
            },
            |answer| answer,
            |_| ComponentDefaults(self).resolve(page, argument, option),
        )
    }
    fn cache_version(&self, page: &Page<'_>) -> Option<String> {
        // A component with no version is rendered afresh on every read: what a failure asks too.
        self.ask(
            CacheVersion {
                component: self.name.clone(),
                page: PageData::of(page),
            },
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
        self.ask(
            Recognises {
                kind: self.name.clone(),
                card: self.card(card),
            },
            || false,
        )
    }
    fn pictures(&self, root: &std::path::Path, card: &Card<'_>) -> Vec<Picture> {
        self.ask(
            Pictures {
                kind: self.name.clone(),
                root: root.to_path_buf(),
                card: self.card(card),
            },
            || KindDefaults(self).pictures(root, card),
        )
    }
    fn slot(&self, card: &Card<'_>) -> Option<String> {
        self.ask(
            Slot {
                kind: self.name.clone(),
                card: self.card(card),
            },
            || KindDefaults(self).slot(card),
        )
    }
    fn markers(&self) -> &[&str] {
        &self.markers
    }
    fn triage_category(&self, card: &Card<'_>) -> Option<String> {
        self.ask(
            TriageCategory {
                kind: self.name.clone(),
                card: self.card(card),
            },
            || KindDefaults(self).triage_category(card),
        )
    }
    fn auto_approvable(&self, card: &Card<'_>) -> bool {
        self.ask(
            AutoApprovable {
                kind: self.name.clone(),
                card: self.card(card),
            },
            || KindDefaults(self).auto_approvable(card),
        )
    }
    fn gates(&self, card: &Card<'_>) -> Option<(usize, String)> {
        self.ask(
            CardGates {
                kind: self.name.clone(),
                card: self.card(card),
            },
            || KindDefaults(self).gates(card),
        )
    }
    fn approval_needs_agent(&self, card: &Card<'_>) -> bool {
        self.ask(
            ApprovalNeedsAgent {
                kind: self.name.clone(),
                card: self.card(card),
            },
            || KindDefaults(self).approval_needs_agent(card),
        )
    }
    fn chosen(&self, card: &Card<'_>) -> Result<Option<String>, String> {
        self.ask(
            Chosen {
                kind: self.name.clone(),
                card: self.card(card),
            },
            || KindDefaults(self).chosen(card),
        )
    }
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
