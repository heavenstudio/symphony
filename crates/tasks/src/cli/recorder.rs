//! `tasks run --phase build|test [--label TEXT] [--task SLUG] [--as HANDLE] -- COMMAND [ARGS...]`:
//! COMMAND run with its caller's arguments (any bytes), streams, environment, working directory and
//! limits, its exit status its caller's; and recorded in the work folder ([`recorded`]) while it
//! runs: queued, running once it started, said again every [`BEAT`] while it runs, then kept in the
//! history and finished with how it ended. A build or test it records puts the task it ran for in
//! the board's Build or Test column (docs/MACHINE_WORK.md).
//!
//! On macOS the `tasks` process becomes COMMAND (`execvp`): the process its caller started, waits
//! for and signals is the command's, which keeps every descriptor, the signal mask and the ignored
//! signals it was handed (only SIGPIPE starts at its default, as a shell's command does), and
//! whatever ends the one, SIGKILL included, ends the other. First it forks its recorder, detached
//! in a session of its own with none of the caller's streams open, which watches that process
//! through a kqueue (its exec, its exit and its status) and records it; the command waits at most
//! [`READY`] to hear the recorder watches it, and runs unrecorded otherwise. Elsewhere, and in a
//! caller's process ([`super::main`]), it runs COMMAND as its child and waits for it: as the
//! `tasks` binary it passes the hang-up, interrupt, quit and terminate signals its caller left at
//! their default to the command, and ends by the signal that ended it. The command's end is seen
//! without reaping it; it is reaped once no signal is passed on, so none reaches another process
//! that took its pid.
//!
//! The task is the one named, `--task`, else `TASKS_TASK` ([`TASK_VAR`]); else as the board says
//! when it begins ([`crate::store::Store::run_for`]), read in one read that writes nothing, without
//! starting the board's server and waiting at most [`WAIT`], in this order: the task the session
//! named (`--as`, else `TASKS_SESSION`) holds (it executes it, or reviews it in review); else the
//! one held by the one live session whose last tool call the beat hook placed in this linked
//! worktree (never the main checkout, where every agent's board commands run); else the checkout's
//! branch when it is the slug of an open task; else the one held by the live full session the
//! Claude Code session ([`CLAUDE_SESSION_VAR`]) binds, which a subagent shares with its parent, so
//! it never beats the branch. A session counts only while the board reads it live: one that
//! finished without leaving, or went stale or lost, is passed over. Of several tasks a session
//! holds, the one the others are under; none when the board cannot say. A command a recorder runs
//! names its operation in [`recorded::PARENT_VAR`], so a recorder it starts records it as its
//! parent. That read holds up no time it records: the operation is queued when `tasks run` was
//! called, started when its command did (on macOS, when its recorder told it to exec) and ended
//! when its end came, each seen as it happens, by a thread of its own while the board is asked; the
//! record is written once the board answered or the wait ran out.
//!
//! Nothing it records reaches the command's streams: a record it cannot write leaves the command
//! running as it would unrecorded. It says something only of a command line it cannot read (2) and
//! of a command that cannot start (127, or 126, as a shell says them). `tasks run --probe` answers
//! [`PROBE`]: what a wrapper asks before it hands a build to a `tasks` that may be older than this
//! command.

use super::Env;
use super::machine_work::recorded::{self, Phase, Record, Step};
use crate::sys::{self, FileLock};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

/// The command's name.
pub const COMMAND: &str = "run";
/// How often a running operation's recorder says it still runs: well inside the Symphony's
/// ten seconds before it calls work stale.
pub const BEAT: Duration = Duration::from_secs(2);
/// How long the board is asked which task the session holds.
const WAIT: Duration = Duration::from_secs(1);
/// How long a command waits to hear its recorder watches it before it runs unrecorded.
#[cfg(target_os = "macos")]
const READY: Duration = Duration::from_secs(1);
/// Names the task a run is for in place of the one its session holds: what an orchestrator, or a
/// subagent with no session of its own, sets for the builds and tests it runs for a task.
pub const TASK_VAR: &str = "TASKS_TASK";
/// The Claude Code session a command runs in, as Claude Code sets it for its tool calls: the key
/// the beat hook binds a full session's handle by.
pub const CLAUDE_SESSION_VAR: &str = "CLAUDE_CODE_SESSION_ID";
/// What `tasks run --probe` answers, a line: this command line, at its first version.
pub const PROBE: &str = "run 1";
const USAGE: &str = "usage: tasks run --phase build|test [--label TEXT] [--task SLUG] [--as HANDLE] -- COMMAND [ARGS...]\n       tasks run --probe\n";

/// A command line read.
#[derive(Debug, PartialEq)]
struct Asked {
    phase: Phase,
    label: Option<String>,
    task: Option<String>,
    handle: Option<String>,
    command: Vec<OsString>,
}

impl Asked {
    /// Its label, else its command's words, each as text.
    fn label(&self) -> String {
        self.label.clone().unwrap_or_else(|| {
            let words: Vec<_> = self
                .command
                .iter()
                .map(|word| word.to_string_lossy())
                .collect();
            words.join(" ")
        })
    }
}

/// `args` read: none for `--probe`. An option's value is text; the command's words are any bytes.
fn parse(args: &[OsString]) -> Result<Option<Asked>, String> {
    if let [only] = args
        && only == "--probe"
    {
        return Ok(None);
    }
    let (mut phase, mut label, mut task, mut handle) = (None, None, None, None);
    let mut command = Vec::new();
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if arg == "--" {
            command = args[at + 1..].to_vec();
            break;
        }
        let name = arg.to_string_lossy();
        let value = args
            .get(at + 1)
            .filter(|value| *value != "--")
            .map(|value| value.to_string_lossy().into_owned())
            .ok_or_else(|| format!("{name} needs a value"));
        match name.as_ref() {
            "--phase" => {
                phase = Some(match value?.as_str() {
                    "build" => Phase::Build,
                    "test" => Phase::Test,
                    other => return Err(format!("--phase is build or test, not {other:?}")),
                });
            }
            "--label" => label = Some(value?),
            "--task" => task = Some(value?),
            "--as" => handle = Some(value?),
            other => return Err(format!("unknown option {other:?}")),
        }
        at += 2;
    }
    let phase = phase.ok_or("--phase build or --phase test is required")?;
    if command.is_empty() {
        return Err("the command to run follows --".into());
    }
    Ok(Some(Asked {
        phase,
        label,
        task,
        handle,
        command,
    }))
}

/// `tasks run ARGS` in `env`, the probe's answer on `out` and what it must say on `err`. `process`
/// when this is the `tasks` binary's own process, which may become the command, pass its signals
/// on, or end by the one that ended it; it runs no other thread yet.
pub fn run(
    args: &[OsString],
    env: &Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
    process: bool,
) -> i32 {
    let asked = match parse(args) {
        Ok(Some(asked)) => asked,
        Ok(None) => {
            let _ = writeln!(out, "{PROBE}");
            return 0;
        }
        Err(why) => {
            let _ = write!(err, "tasks run: {why}\n{USAGE}");
            return 2;
        }
    };
    #[cfg(target_os = "macos")]
    if process {
        return supervised::exec(&asked, env, err);
    }
    child(&asked, env, err, process)
}

/// The session and the task `asked` runs for in `env`: the task named, else as the board says
/// ([`crate::store::Store::run_for`]: the given session's, the live session placed in this linked
/// worktree's, the branch's, the live session the Claude Code session binds).
fn attributed(asked: &Asked, env: &Env) -> (Option<String>, Option<String>) {
    let identity = |name: &String| super::machine_work::identity(name);
    let handle = asked
        .handle
        .clone()
        .or_else(|| env.var("TASKS_SESSION").map(str::to_owned))
        .filter(identity);
    let named = asked
        .task
        .clone()
        .or_else(|| env.var(TASK_VAR).map(str::to_owned));
    if let Some(task) = named {
        return (handle, Some(task).filter(identity));
    }
    let checkout = crate::git::checkout(&env.cwd);
    let place = crate::store::RunPlace {
        handle: handle.as_deref(),
        checkout: (checkout.as_ref())
            .filter(|(top, _)| crate::git::linked(top))
            .and_then(|(top, _)| top.to_str()),
        claude: env.var(CLAUDE_SESSION_VAR).filter(|id| !id.is_empty()),
        branch: checkout
            .as_ref()
            .map(|(_, branch)| branch.as_str())
            .filter(|branch| !branch.is_empty()),
    };
    match on_board(env, &place) {
        Some((found, task)) => (handle.or(found).filter(identity), task.filter(identity)),
        None => (handle, None),
    }
}

/// The command a code of a shell's says could not start: 127 not found, 126 found but not run.
fn unstarted(error: &io::Error) -> i32 {
    if error.kind() == io::ErrorKind::NotFound {
        127
    } else {
        126
    }
}

/// `asked`'s command run as this process's child, recorded while it runs. In a caller's process
/// (`process` false), in the `env` it was handed; as the `tasks` binary, in its own environment,
/// passing on the signals it gets, then ending as its command did.
fn child(asked: &Asked, env: &Env, err: &mut dyn Write, process: bool) -> i32 {
    let queued_at = crate::store::now();
    let folder = recorded::folder(env);
    let id = folder.as_ref().and_then(|_| recorded::new_id().ok());
    let mut command = if process {
        sys::command(&asked.command[0])
    } else {
        let mut command = sys::command_with(&asked.command[0], &env.vars);
        if !env.cwd.as_os_str().is_empty() {
            command.current_dir(&env.cwd);
        }
        command
    };
    command.args(&asked.command[1..]);
    // The command is the caller's own, not a child the board starts: it keeps the caller's
    // session (its board handle and hc terminal), which its own board commands and hc calls need.
    for name in sys::SESSION_VARS {
        if let Some(value) = env.vars.get(name) {
            command.env(name, value);
        }
    }
    if let Some(id) = &id {
        command.env(recorded::PARENT_VAR, id);
    }
    let begin = |started: Option<Start>| {
        let recording =
            Recording::begin(folder.clone()?, id.clone()?, asked, env, queued_at, started).ok()?;
        recording.sweep();
        Some(recording)
    };
    let forwarding = process.then(signals::Forwarding::install);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            drop(forwarding);
            let code = unstarted(&error);
            let _ = writeln!(
                err,
                "tasks run: {}: {error}",
                asked.command[0].to_string_lossy()
            );
            if let Some(recording) = begin(None) {
                recording.finish(
                    Some(code),
                    None,
                    &format!("its command did not start: {error}"),
                    crate::store::now(),
                );
            }
            return code;
        }
    };
    let started = (child.id(), crate::store::now());
    if let Some(forwarding) = &forwarding {
        forwarding.child(started.0);
    }
    // Its end is seen by a thread of its own and stamped as it comes, while the board is asked,
    // and left unreaped: its pid stays its own while its signals are passed to it. It is reaped
    // here, once they no longer are.
    let (end, ends) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let seen = ended_unreaped(started.0);
        let _ = end.send((seen, crate::store::now()));
    });
    let recording = begin(Some(started));
    let (seen, at) = loop {
        match ends.recv_timeout(BEAT) {
            Ok(end) => break end,
            Err(RecvTimeoutError::Timeout) => {
                if let Some(recording) = &recording {
                    recording.beat();
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                break (Err(io::Error::other("its waiter ended")), 0.0);
            }
        }
    };
    let _ = waiter.join();
    drop(forwarding);
    let ended = child.wait();
    // An end its waiter could not see is the one the reap read, as it came.
    let at = if seen.is_ok() {
        at
    } else {
        crate::store::now()
    };
    let status = match ended {
        Ok(status) => status,
        Err(error) => {
            let _ = writeln!(
                err,
                "tasks run: the end of {}: {error}",
                asked.command[0].to_string_lossy()
            );
            if let Some(recording) = recording {
                let reason = format!("its end could not be read: {error}");
                recording.finish(None, None, &reason, at);
            }
            return 1;
        }
    };
    if let Some(recording) = recording {
        recording.finish(status.code(), status.signal(), "", at);
    }
    match (status.code(), status.signal()) {
        (Some(code), _) => code,
        (None, Some(signal)) => {
            if process {
                signals::end_by(signal);
            }
            128 + signal
        }
        (None, None) => 1,
    }
}

/// Once this process's child `pid` has ended, left unreaped (`WNOWAIT`): until it is waited for,
/// its pid names it and no other process.
fn ended_unreaped(pid: u32) -> io::Result<()> {
    loop {
        // SAFETY: siginfo_t is plain data, for which all zeroes is a value.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: waits for one child of this process to end, writing into `info`, reaping none.
        let waited = unsafe {
            libc::waitid(
                libc::P_PID,
                libc::id_t::from(pid),
                &raw mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if waited == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// The session and task a run at `place` is for, as the board `env` reaches says in one read,
/// without starting its server; none when it does not answer. Whether a session is live is judged
/// by the clock every board command reads (`TASKS_NOW` pins it), else this machine's.
fn on_board(env: &Env, place: &crate::store::RunPlace) -> Option<(Option<String>, Option<String>)> {
    let reached = super::reach(env, &env.cwd, false).ok()?;
    let reached = super::Reached {
        client: reached.client.waiting(WAIT),
        ..reached
    };
    let clock = super::Clock::of(env).unwrap_or(super::Clock::Machine);
    let store = super::open_unchecked(reached, &clock);
    store.run_for(place).ok()
}

/// The commit checked out at the checkout `cwd` is in, read from its `.git` files with no `git`
/// process: its `HEAD`, through a loose or packed ref in the common dir.
fn commit(cwd: &Path) -> Option<String> {
    let (top, _) = crate::git::checkout(cwd)?;
    let common = crate::git::checkout_common_dir(&top)?;
    let dot = top.join(".git");
    let gitdir = if dot.is_dir() {
        dot
    } else {
        let text = fs::read_to_string(&dot).ok()?;
        top.join(text.trim().strip_prefix("gitdir:")?.trim())
    };
    let head = fs::read_to_string(gitdir.join("HEAD")).ok()?;
    let head = head.trim();
    let sha = match head.strip_prefix("ref: ") {
        None => head.to_string(),
        Some(name) => match fs::read_to_string(common.join(name)) {
            Ok(text) => text.trim().to_string(),
            Err(_) => fs::read_to_string(common.join("packed-refs"))
                .ok()?
                .lines()
                .find_map(|line| {
                    let (sha, named) = line.split_once(' ')?;
                    (named == name).then(|| sha.to_string())
                })?,
        },
    };
    (matches!(sha.len(), 40 | 64) && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then_some(sha)
}

/// The installation identity of the machine folder `env` names, never made here.
fn machine_id(env: &Env) -> Option<String> {
    let folder = crate::project::machine_folder(
        |name| env.var(name).map(OsString::from),
        cfg!(target_os = "macos"),
    )
    .ok()?;
    crate::project::machine_id(&folder).ok()
}

/// A command's start: its pid, and when.
type Start = (u32, f64);

/// One operation being recorded: its record, written whole at each change, and its lease, held
/// locked until it finishes.
struct Recording {
    folder: PathBuf,
    path: PathBuf,
    lease: FileLock,
    record: Mutex<Record>,
}

impl Recording {
    /// Its lease taken and its record written in `folder`, as the operation `id` of `asked` in
    /// `env`, asked for at `queued_at`: running since `started` when its command started, else
    /// queued. The task it is for is asked of the board first; the times were stamped before.
    fn begin(
        folder: PathBuf,
        id: String,
        asked: &Asked,
        env: &Env,
        queued_at: f64,
        started: Option<Start>,
    ) -> io::Result<Recording> {
        let (handle, task) = attributed(asked, env);
        let now = crate::store::now();
        let operations = folder.join(recorded::OPERATIONS);
        fs::create_dir_all(&operations)?;
        let path = operations.join(format!("{id}.json"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.with_extension("lease"))?;
        file.lock()?;
        let lease = FileLock(file);
        let branch = crate::git::checkout(&env.cwd)
            .map(|(_, branch)| branch)
            .filter(|branch| !branch.is_empty());
        let record = Record {
            version: 1,
            operation_id: id,
            parent_id: env
                .var(recorded::PARENT_VAR)
                .filter(|parent| recorded::nonce(parent))
                .map(str::to_owned),
            phase: asked.phase,
            label: recorded::bounded(&asked.label()),
            state: if started.is_some() {
                Step::Running
            } else {
                Step::Queued
            },
            handle,
            task,
            checkout: recorded::bounded(&env.cwd.to_string_lossy()),
            branch: branch.map(|branch| recorded::bounded(&branch)),
            commit: commit(&env.cwd),
            machine_id: machine_id(env),
            host: recorded::bounded(&sys::hostname()),
            pid: std::process::id(),
            child_pid: started.map(|(pid, _)| pid),
            queued_at,
            started_at: started.map(|(_, at)| at),
            ended_at: None,
            beat_at: now,
            exit_code: None,
            signal: None,
            reason: String::new(),
        };
        if let Err(error) = recorded::write(&path, &record) {
            drop(lease);
            let _ = fs::remove_file(path.with_extension("lease"));
            return Err(error);
        }
        Ok(Recording {
            folder,
            path,
            lease,
            record: Mutex::new(record),
        })
    }

    /// The operations in its folder whose recorders are gone, finished.
    fn sweep(&self) {
        let _ = recorded::sweep(&self.folder, crate::store::now());
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Record> {
        self.record.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Its record changed by `change`, said now, and written.
    fn update(&self, change: impl FnOnce(&mut Record, f64)) {
        let mut record = self.held();
        let now = crate::store::now();
        change(&mut record, now);
        record.beat_at = now;
        let _ = recorded::write(&self.path, &record);
    }

    /// Its command started: `(pid, at)`.
    #[cfg_attr(
        not(target_os = "macos"),
        expect(
            dead_code,
            reason = "a recorder that watches its command says when it started"
        )
    )]
    fn started(&self, (pid, at): Start) {
        self.update(|record, _| {
            record.state = Step::Running;
            record.started_at = Some(at);
            record.child_pid = Some(pid);
        });
    }

    /// It still runs.
    fn beat(&self) {
        self.update(|_, _| {});
    }

    /// It ended at `at` so (`exit_code` or `signal`, neither when that is not known): kept in the
    /// history, then its record written finished, then its lease let go. A recorder ended between
    /// the two leaves it unfinished for a sweep to keep again: the history reads each operation
    /// once.
    fn finish(self, exit_code: Option<i32>, signal: Option<i32>, reason: &str, at: f64) {
        let mut record = self.held();
        record.state = Step::Finished;
        record.ended_at = Some(at);
        record.beat_at = crate::store::now();
        record.exit_code = exit_code;
        record.signal = signal;
        reason.clone_into(&mut record.reason);
        let _ = recorded::keep(&self.folder, &record);
        let _ = recorded::write(&self.path, &record);
        drop(record);
        drop(self.lease);
        let _ = fs::remove_file(self.path.with_extension("lease"));
    }
}

/// The `tasks` process become its command, watched by a recorder it forks first.
#[cfg(target_os = "macos")]
mod supervised {
    use super::{Asked, BEAT, READY, Recording, unstarted};
    use crate::cli::Env;
    use crate::cli::machine_work::recorded;
    use std::ffi::CString;
    use std::io::{self, Write};
    use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
    use std::os::unix::ffi::OsStrExt as _;
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::time::{Duration, Instant};

    /// What the recorder's watch saw of its command, and when it saw it.
    type Heard = io::Result<(Seen, f64)>;

    /// Become `asked`'s command, recorded by a recorder that watches this process, in `env`; what
    /// returns is the code of a command that could not start, said on `err`.
    pub(super) fn exec(asked: &Asked, env: &Env, err: &mut dyn Write) -> i32 {
        let queued_at = crate::store::now();
        let Ok(words) = asked
            .command
            .iter()
            .map(|word| CString::new(word.as_bytes()))
            .collect::<Result<Vec<_>, _>>()
        else {
            let _ = writeln!(err, "tasks run: a word of the command holds a NUL byte");
            return 2;
        };
        let mut argv: Vec<*const libc::c_char> = words.iter().map(|word| word.as_ptr()).collect();
        argv.push(std::ptr::null());
        if let Ok(id) = recorded::new_id()
            && watched(asked, env, &id, queued_at)
        {
            // SAFETY: this process runs one thread: nothing reads its environment beside this.
            unsafe { std::env::set_var(recorded::PARENT_VAR, &id) };
        }
        // SAFETY: the default a shell starts its command with, which Rust's runtime replaced.
        unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
        // SAFETY: a null-terminated array of C strings, all alive through the call.
        unsafe { libc::execvp(argv[0], argv.as_ptr()) };
        let error = io::Error::last_os_error();
        // SAFETY: the runtime's own disposition again, for what this process says now.
        unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) };
        let _ = writeln!(
            err,
            "tasks run: {}: {error}",
            asked.command[0].to_string_lossy()
        );
        unstarted(&error)
    }

    /// Whether a recorder forked now watches this process, as the operation `id` asked for at
    /// `queued_at`: told within [`READY`] on a pipe of their own.
    fn watched(asked: &Asked, env: &Env, id: &str, queued_at: f64) -> bool {
        let Ok((told, tell)) = pipe() else {
            return false;
        };
        // SAFETY: getpid cannot fail.
        let target = unsafe { libc::getpid() };
        // SAFETY: this process runs one thread, so its child may run what follows.
        match unsafe { libc::fork() } {
            -1 => false,
            0 => {
                drop(told);
                // SAFETY: as above: the grandchild records, and its parent ends at once, so the
                // command has no child of the recorder's to wait for.
                if unsafe { libc::fork() } == 0 {
                    record(target, &tell, asked, env, (id, queued_at));
                }
                // SAFETY: ends the child between without the runtime's exit, which is the caller's.
                unsafe { libc::_exit(0) }
            }
            forked => {
                drop(tell);
                let mut status = 0;
                // SAFETY: waits for the child this call forked, which ends at once.
                while unsafe { libc::waitpid(forked, &raw mut status, 0) } == -1
                    && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted
                {
                }
                ready(&told)
            }
        }
    }

    /// A pipe, both its ends close-on-exec: the command holds neither.
    fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
        let mut fds = [0; 2];
        // SAFETY: writes two new descriptors into the array.
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a new descriptor nothing else owns.
        let read = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        // SAFETY: as above.
        let write = unsafe { OwnedFd::from_raw_fd(fds[1]) };
        for fd in fds {
            // SAFETY: sets the descriptor flag of one owned above.
            if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok((read, write))
    }

    /// Whether the recorder said on `told`, within [`READY`], that it watches this process.
    fn ready(told: &OwnedFd) -> bool {
        let deadline = Instant::now() + READY;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let mut poll = libc::pollfd {
                fd: told.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let wait = libc::c_int::try_from(left.as_millis()).unwrap_or(libc::c_int::MAX);
            // SAFETY: one pollfd, alive through the call.
            match unsafe { libc::poll(&raw mut poll, 1, wait) } {
                1 => break,
                -1 if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted => {}
                _ => return false,
            }
        }
        let mut byte = 0_u8;
        // SAFETY: reads at most one byte into it.
        unsafe { libc::read(told.as_raw_fd(), (&raw mut byte).cast(), 1) == 1 }
    }

    /// The recorder: it watches `target` and says so on `tell` when it has a work folder to record
    /// it in; then, out of the caller's session and streams, it records the operation `id` asked
    /// for at `queued_at` until `target` ends, and ends. Told, the command execs: that is its start.
    fn record(
        target: libc::pid_t,
        tell: &OwnedFd,
        asked: &Asked,
        env: &Env,
        (id, queued_at): (&str, f64),
    ) -> ! {
        let watch = Watch::new(target);
        let folder = watch.as_ref().ok().and_then(|_| recorded::folder(env));
        let start = (target.unsigned_abs(), crate::store::now());
        let told = folder.is_some()
            // SAFETY: writes one byte from it; a command gone on without it hears nothing.
            && unsafe { libc::write(tell.as_raw_fd(), b"1".as_ptr().cast(), 1) } == 1;
        // The command waited for no more than that.
        // SAFETY: a session of its own: no signal for the caller's group or terminal reaches it.
        unsafe { libc::setsid() };
        detach(watch.as_ref().map_or(-1, |watch| watch.0.as_raw_fd()));
        if let (true, Ok(watch), Some(folder)) = (told, watch, folder) {
            // Read on a thread of its own, so what it sees is stamped as it comes while the board
            // is asked which task this is for.
            let (tell, heard) = mpsc::channel();
            std::thread::spawn(move || watching(&watch, &tell));
            if let Ok(recording) =
                Recording::begin(folder, id.to_owned(), asked, env, queued_at, None)
            {
                recording.sweep();
                follow(&heard, recording, start);
            }
        }
        // SAFETY: ends the recorder without the runtime's exit, which is the caller's.
        unsafe { libc::_exit(0) }
    }

    /// What `watch` sees, sent on `tell` with the time it saw it, until its process ends, the watch
    /// fails, or nothing listens.
    fn watching(watch: &Watch, tell: &mpsc::Sender<Heard>) {
        loop {
            match watch.next(BEAT) {
                Ok(Some(seen)) => {
                    let exited = seen.exit.is_some();
                    if tell.send(Ok((seen, crate::store::now()))).is_err() || exited {
                        return;
                    }
                }
                Ok(None) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    let _ = tell.send(Err(error));
                    return;
                }
            }
        }
    }

    /// No stream of the caller's held open: `/dev/null` in, out and err, and every other
    /// descriptor closed but `keep`, the pipe the command was told on among them.
    fn detach(keep: libc::c_int) {
        if let Ok(null) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/null")
        {
            for stream in 0..3 {
                // SAFETY: points a standard stream at /dev/null, which stays open through it.
                unsafe { libc::dup2(null.as_raw_fd(), stream) };
            }
        }
        let open: Vec<libc::c_int> = std::fs::read_dir("/dev/fd")
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        for fd in open {
            if fd > 2 && fd != keep {
                // SAFETY: a descriptor inherited from the caller, which nothing here uses.
                unsafe { libc::close(fd) };
            }
        }
    }

    /// Record its command as its watch was `heard`: started at `start` once it exec'd, said again
    /// every [`BEAT`] while it runs, finished with its status when its exit was seen.
    fn follow(heard: &mpsc::Receiver<Heard>, recording: Recording, start: super::Start) {
        let mut started = false;
        loop {
            match heard.recv_timeout(BEAT) {
                Ok(Ok((seen, at))) => {
                    if seen.exec && !started {
                        started = true;
                        recording.started(start);
                    }
                    if let Some(status) = seen.exit {
                        let (code, signal) = ended(status);
                        let reason = if started {
                            ""
                        } else {
                            "its command did not start"
                        };
                        recording.finish(code, signal, reason, at);
                        return;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if started {
                        recording.beat();
                    }
                }
                Ok(Err(error)) => {
                    let reason = format!("its recorder lost sight of its command: {error}");
                    recording.finish(None, None, &reason, crate::store::now());
                    return;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let reason = "its recorder lost sight of its command";
                    recording.finish(None, None, reason, crate::store::now());
                    return;
                }
            }
        }
    }

    /// The exit code or the signal a wait status says.
    fn ended(status: libc::c_int) -> (Option<i32>, Option<i32>) {
        if libc::WIFEXITED(status) {
            (Some(libc::WEXITSTATUS(status)), None)
        } else if libc::WIFSIGNALED(status) {
            (None, Some(libc::WTERMSIG(status)))
        } else {
            (None, None)
        }
    }

    /// A kqueue that watches one process: its exec, and its exit with its status.
    struct Watch(OwnedFd);

    /// What one wait saw of the process watched.
    struct Seen {
        exec: bool,
        /// Its wait status, once it ended.
        exit: Option<libc::c_int>,
    }

    impl Watch {
        /// `target` watched, or why it cannot be: gone already, or not this user's to watch.
        fn new(target: libc::pid_t) -> io::Result<Watch> {
            // SAFETY: kqueue takes no argument.
            let queue = unsafe { libc::kqueue() };
            if queue == -1 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: a new descriptor nothing else owns.
            let watch = Watch(unsafe { OwnedFd::from_raw_fd(queue) });
            let change = libc::kevent {
                ident: target.unsigned_abs() as usize,
                filter: libc::EVFILT_PROC,
                flags: libc::EV_ADD,
                fflags: libc::NOTE_EXIT | libc::NOTE_EXITSTATUS | libc::NOTE_EXEC,
                data: 0,
                udata: std::ptr::null_mut(),
            };
            // SAFETY: one change in, no room for an event out: an error is the call's own.
            let added = unsafe {
                libc::kevent(
                    watch.0.as_raw_fd(),
                    &raw const change,
                    1,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null(),
                )
            };
            if added == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(watch)
        }

        /// What happened to the process within `wait`; none when nothing did.
        fn next(&self, wait: Duration) -> io::Result<Option<Seen>> {
            let timeout = libc::timespec {
                tv_sec: libc::time_t::try_from(wait.as_secs()).unwrap_or(libc::time_t::MAX),
                tv_nsec: libc::c_long::from(wait.subsec_nanos()),
            };
            // SAFETY: kevent is plain data, for which all zeroes is a value.
            let mut event: libc::kevent = unsafe { std::mem::zeroed() };
            // SAFETY: no change in, room for one event out, waiting at most `timeout`.
            let seen = unsafe {
                libc::kevent(
                    self.0.as_raw_fd(),
                    std::ptr::null(),
                    0,
                    &raw mut event,
                    1,
                    &raw const timeout,
                )
            };
            match seen {
                -1 => Err(io::Error::last_os_error()),
                0 => Ok(None),
                _ if event.flags & libc::EV_ERROR != 0 => Err(io::Error::from_raw_os_error(
                    i32::try_from(event.data).unwrap_or(libc::EINVAL),
                )),
                _ => Ok(Some(Seen {
                    exec: event.fflags & libc::NOTE_EXEC != 0,
                    exit: (event.fflags & libc::NOTE_EXIT != 0)
                        .then(|| libc::c_int::try_from(event.data).unwrap_or_default()),
                })),
            }
        }
    }
}

/// The signals a child-running recorder passes to its command, and its end by the one that ended
/// it.
mod signals {
    use std::sync::atomic::{AtomicI32, Ordering};

    /// The command's pid once it started, else 0.
    static CHILD: AtomicI32 = AtomicI32::new(0);
    /// A signal that came before the command started, else 0.
    static PENDING: AtomicI32 = AtomicI32::new(0);
    /// What a terminal, a supervisor or a timeout sends to end what it started.
    pub(super) const FORWARDED: [libc::c_int; 4] =
        [libc::SIGHUP, libc::SIGINT, libc::SIGQUIT, libc::SIGTERM];

    extern "C" fn forward(signal: libc::c_int) {
        let child = CHILD.load(Ordering::SeqCst);
        if child > 0 {
            // SAFETY: kill is async-signal-safe, and the pid is this process's unreaped child.
            unsafe { libc::kill(child, signal) };
        } else {
            PENDING.store(signal, Ordering::SeqCst);
        }
    }

    /// `signal`'s disposition now, unchanged; none when it cannot be read.
    pub(super) fn disposition(signal: libc::c_int) -> Option<libc::sighandler_t> {
        // SAFETY: sigaction is plain data, for which all zeroes is a value.
        let mut now: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: reads the disposition into `now`, changing none.
        let read = unsafe { libc::sigaction(signal, std::ptr::null(), &raw mut now) };
        (read == 0).then_some(now.sa_sigaction)
    }

    /// The forwarding handler over each of [`FORWARDED`] left at its default, until dropped, when
    /// the default comes back. One the caller ignores stays ignored, here and in the command.
    pub(super) struct Forwarding {
        installed: Vec<libc::c_int>,
    }

    impl Forwarding {
        pub(super) fn install() -> Forwarding {
            CHILD.store(0, Ordering::SeqCst);
            PENDING.store(0, Ordering::SeqCst);
            let handler = forward as extern "C" fn(libc::c_int) as libc::sighandler_t;
            let mut installed = Vec::new();
            for signal in FORWARDED {
                if disposition(signal) == Some(libc::SIG_DFL) {
                    // SAFETY: a handler that only reads and writes atomics and calls kill.
                    unsafe { libc::signal(signal, handler) };
                    installed.push(signal);
                }
            }
            Forwarding { installed }
        }

        /// The command started as `pid`: what came before goes to it now.
        pub(super) fn child(&self, pid: u32) {
            let pid = i32::try_from(pid).unwrap_or(0);
            CHILD.store(pid, Ordering::SeqCst);
            let pending = PENDING.swap(0, Ordering::SeqCst);
            if pending != 0 && pid > 0 {
                // SAFETY: the pid is this process's child, not yet waited for.
                unsafe { libc::kill(pid, pending) };
            }
        }
    }

    impl Drop for Forwarding {
        fn drop(&mut self) {
            CHILD.store(0, Ordering::SeqCst);
            for &signal in &self.installed {
                // SAFETY: the default this signal had before install.
                unsafe { libc::signal(signal, libc::SIG_DFL) };
            }
        }
    }

    /// End this process by `signal`, by its default action, as its command ended; returns when that
    /// action does not end it.
    pub(super) fn end_by(signal: i32) {
        // SAFETY: the default action of a signal number the command's end reported.
        unsafe { libc::signal(signal, libc::SIG_DFL) };
        // SAFETY: getpid cannot fail.
        let me = unsafe { libc::getpid() };
        // SAFETY: a signal to this process itself.
        unsafe { libc::kill(me, signal) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_command_line_reads_its_options_then_the_command_after_the_first_separator() {
        let read = parse(&args(&[
            "--phase", "test", "--label", "verify", "--as", "h", "--task", "a/b", "--", "cargo",
            "--", "--exact",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(
            read,
            Asked {
                phase: Phase::Test,
                label: Some("verify".into()),
                task: Some("a/b".into()),
                handle: Some("h".into()),
                command: args(&["cargo", "--", "--exact"]),
            }
        );
        assert_eq!(parse(&args(&["--probe"])), Ok(None));
        assert!(super::super::usage::HELP.contains("\n    run "));
    }

    #[test]
    fn a_command_of_any_bytes_is_read_whole_and_labelled_as_text() {
        use std::os::unix::ffi::OsStringExt as _;
        let odd = OsString::from_vec(vec![b'a', 0xff, b'b']);
        let mut words = args(&["--phase", "build", "--"]);
        words.extend([OsString::from("printf"), odd.clone()]);
        let read = parse(&words).unwrap().unwrap();
        assert_eq!(read.command, [OsString::from("printf"), odd]);
        assert_eq!(read.label(), "printf a\u{fffd}b");
        let mut named = args(&["--phase", "build", "--label"]);
        named.extend([
            OsString::from_vec(vec![0xfe]),
            OsString::from("--"),
            "true".into(),
        ]);
        assert_eq!(parse(&named).unwrap().unwrap().label(), "\u{fffd}");
    }

    #[test]
    fn a_command_line_missing_its_phase_command_or_a_value_or_naming_another_option_is_refused() {
        for (words, said) in [
            (vec!["--", "true"], "--phase"),
            (vec!["--phase", "build"], "follows --"),
            (vec!["--phase", "build", "--"], "follows --"),
            (vec!["--phase", "lint", "--", "true"], "build or test"),
            (vec!["--phase"], "needs a value"),
            (vec!["--phase", "--", "true"], "needs a value"),
            (
                vec!["--phase", "build", "--quiet", "--", "true"],
                "unknown option",
            ),
            (
                vec!["--probe", "--phase", "build", "--", "true"],
                "unknown option",
            ),
        ] {
            let refused = parse(&args(&words)).unwrap_err();
            assert!(refused.contains(said), "{words:?}: {refused}");
        }
    }

    #[test]
    fn a_task_is_named_else_the_environments_else_none_without_a_session() {
        let env = |vars: &[(&str, &str)]| Env {
            vars: vars
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect(),
            cwd: std::env::temp_dir(),
        };
        let mut asked = parse(&args(&["--phase", "test", "--", "true"]))
            .unwrap()
            .unwrap();
        assert_eq!(attributed(&asked, &env(&[])), (None, None));
        assert_eq!(
            attributed(&asked, &env(&[(TASK_VAR, "tools/from-env")])),
            (None, Some("tools/from-env".into()))
        );
        let both = env(&[(TASK_VAR, "tools/from-env"), ("TASKS_SESSION", "worker")]);
        assert_eq!(
            attributed(&asked, &both),
            (Some("worker".into()), Some("tools/from-env".into()))
        );
        asked.task = Some("tools/named".into());
        assert_eq!(attributed(&asked, &both).1.as_deref(), Some("tools/named"));
        // A name no session or task could have is none.
        assert_eq!(
            attributed(&asked, &env(&[(TASK_VAR, "line\nbreak")]))
                .1
                .as_deref(),
            Some("tools/named")
        );
        asked.task = None;
        assert_eq!(
            attributed(&asked, &env(&[(TASK_VAR, "line\nbreak")])),
            (None, None)
        );
    }

    #[test]
    fn forwarding_leaves_an_ignored_signal_ignored_and_passes_on_one_left_at_its_default() {
        let (ignored, passed) = (libc::SIGHUP, libc::SIGQUIT);
        // SAFETY: this test's own disposition for a signal nothing in the test binary sends.
        unsafe { libc::signal(ignored, libc::SIG_IGN) };
        assert_eq!(signals::disposition(passed), Some(libc::SIG_DFL));
        let forwarding = signals::Forwarding::install();
        assert_eq!(signals::disposition(ignored), Some(libc::SIG_IGN));
        let handler = signals::disposition(passed).unwrap();
        assert!(
            handler != libc::SIG_DFL && handler != libc::SIG_IGN,
            "{handler}"
        );
        drop(forwarding);
        assert_eq!(signals::disposition(ignored), Some(libc::SIG_IGN));
        assert_eq!(signals::disposition(passed), Some(libc::SIG_DFL));
        // SAFETY: the default back, as the test found it.
        unsafe { libc::signal(ignored, libc::SIG_DFL) };
    }

    #[test]
    fn a_commit_is_read_from_a_loose_or_packed_ref_or_a_detached_head() {
        let dir = std::env::temp_dir().join(format!("sf-run-commit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let git = dir.join(".git");
        fs::create_dir_all(git.join("refs/heads")).unwrap();
        let (loose, packed) = ("a".repeat(40), "b".repeat(40));
        fs::write(git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::write(git.join("refs/heads/main"), format!("{loose}\n")).unwrap();
        assert_eq!(commit(&dir), Some(loose.clone()));
        fs::write(git.join("HEAD"), "ref: refs/heads/packed\n").unwrap();
        fs::write(
            git.join("packed-refs"),
            format!("# pack-refs with: peeled\n{packed} refs/heads/packed\n"),
        )
        .unwrap();
        assert_eq!(commit(&dir), Some(packed));
        fs::write(git.join("HEAD"), format!("{loose}\n")).unwrap();
        assert_eq!(commit(&dir), Some(loose));
        fs::write(git.join("HEAD"), "ref: refs/heads/unborn\n").unwrap();
        assert_eq!(commit(&dir), None);
        fs::remove_dir_all(&dir).unwrap();
    }
}
