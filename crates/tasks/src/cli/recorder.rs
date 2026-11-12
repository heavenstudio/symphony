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
//! The task is explicit or none: `--task`, else `TASKS_TASK` ([`TASK_VAR`]), else the one task the
//! session (`--as`, else `TASKS_SESSION`) holds on the board when it begins (it executes it, or
//! reviews it in review), read without starting the board's server and waiting at most [`WAIT`];
//! none when it holds none or several, or the board cannot say. A command a recorder runs names its
//! operation in [`recorded::PARENT_VAR`], so a recorder it starts records it as its parent. That
//! read holds up no time it records: the operation is queued when `tasks run` was called, started
//! when its command did (on macOS, when its recorder told it to exec) and ended when its end came,
//! each seen as it happens, by a thread of its own while the board is asked; the record is written
//! once the board answered or the wait ran out.
//!
//! Nothing it records reaches the command's streams: a record it cannot write leaves the command
//! running as it would unrecorded. It says something only of a command line it cannot read (2) and
//! of a command that cannot start (127, or 126, as a shell says them). `tasks run --probe` answers
//! [`PROBE`]: what a wrapper asks before it hands a build to a `tasks` that may be older than this
//! command.
//!
//! An operation is also a run, or a step of one, which the day files keep a line of
//! ([`crate::runs`], docs/RUNS.md): a step when `--step` names it and it runs inside a run or a step
//! ([`recorded::PIPELINE_VAR`] names that one's pipeline); a run when `--pipeline` names one and it
//! runs inside no recorded command; neither otherwise.
//! Its line says what started it ([`recorded::TRIGGER_VAR`], else its session, else the owner), the
//! commit and tree it checked and whether its checkout was clean as its command started (read before
//! it starts, for at most [`GIT_WAIT`]), the earlier run of the same pipeline, step and label it
//! repeats on that tree, and once it ended, how and the CPU it took: the command's and every
//! child's it reaped. Its lease is taken before its command starts, so a run its caller starts
//! after it ended finds it: a recorder apart from its command waits up to [`RERUN_WAIT`] for such a
//! run to write its end line before it names the run it repeats.
//! As its child, the CPU is what the reap reads, and whether that misses a process that left the
//! command's tree is not watched; on macOS, where it becomes its command, the kernel's count of that
//! process and of every process under it, read every [`SAMPLE`] and once more as it ends, the most
//! read, with whether a reading knew it missed some (`supervised::cpu::Tree`).

use super::Env;
use super::machine_work::recorded::{self, Phase, Record, Run, Step};
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
/// What `tasks run --probe` answers: each version of this command line it takes, a line each,
/// oldest first. A wrapper that reads only the first line takes it at its first; the second adds
/// `--pipeline`, `--step`, `--commit` and `--branch`.
pub const PROBE: &str = "run 1\nrun 2";
/// How often, on macOS, the CPU of the command it watches is read while it runs.
#[cfg(target_os = "macos")]
const SAMPLE: Duration = Duration::from_millis(250);
/// The longest git is asked a run's tree and whether its checkout is clean, before its command
/// starts: well inside the second its command waits for its recorder on macOS.
const GIT_WAIT: Duration = Duration::from_millis(600);
/// The longest a recorder apart from its command waits for an earlier run it may repeat to end.
const RERUN_WAIT: Duration = Duration::from_secs(5);
const USAGE: &str = "usage: tasks run --phase build|test [--label TEXT] [--task SLUG] [--as HANDLE]\n                 [--pipeline NAME] [--step NAME] [--commit SHA] [--branch NAME] -- COMMAND [ARGS...]\n       tasks run --probe\n";

/// A command line read.
#[derive(Debug, PartialEq)]
struct Asked {
    phase: Phase,
    label: Option<String>,
    task: Option<String>,
    handle: Option<String>,
    /// The pipeline it is a run of, at top level.
    pipeline: Option<String>,
    /// Its name as a step of the run it runs inside.
    step: Option<String>,
    /// The commit it checks, when not its checkout's: a review callback's tip.
    commit: Option<String>,
    /// That commit's branch.
    branch: Option<String>,
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
    let (mut pipeline, mut step, mut commit, mut branch) = (None, None, None, None);
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
            "--pipeline" | "--step" => {
                let value = value?;
                if !symphony_api::run_name(&value) {
                    return Err(format!(
                        "{name} is 1 to 64 letters, digits, '-', '_' or '.', not {value:?}"
                    ));
                }
                *if name == "--pipeline" {
                    &mut pipeline
                } else {
                    &mut step
                } = Some(value);
            }
            "--commit" => {
                let value = value?;
                if !sha(&value) {
                    return Err(format!("--commit is a full commit id, not {value:?}"));
                }
                commit = Some(value);
            }
            "--branch" => branch = Some(value?),
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
        pipeline,
        step,
        commit,
        branch,
        command,
    }))
}

/// A full commit id: 40 hex digits (SHA-1) or 64 (SHA-256).
fn sha(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// What line an operation writes in the day files: a step of the run it runs inside, else a run
/// of the pipeline it names, else none.
#[derive(Clone, Debug, PartialEq)]
struct Role {
    pipeline: String,
    step: Option<String>,
}

/// `asked`'s role in `env`: inside a recorded command ([`recorded::PARENT_VAR`]), a step when it
/// names one and that command is a run or a step (whose recorder named its pipeline in
/// [`recorded::PIPELINE_VAR`]), else none; at top level, a run when it names a pipeline.
fn role(asked: &Asked, env: &Env) -> Option<Role> {
    if !env.var(recorded::PARENT_VAR).is_some_and(recorded::nonce) {
        return asked.pipeline.clone().map(|pipeline| Role {
            pipeline,
            step: None,
        });
    }
    let pipeline = env
        .var(recorded::PIPELINE_VAR)
        .filter(|pipeline| symphony_api::run_name(pipeline))?;
    Some(Role {
        pipeline: pipeline.to_owned(),
        step: Some(asked.step.clone()?),
    })
}

/// What started a run in `env` for the session `handle`: [`recorded::TRIGGER_VAR`] when it names
/// something, else the session, else the owner.
fn trigger(env: &Env, handle: Option<&str>) -> String {
    env.var(recorded::TRIGGER_VAR)
        .filter(|named| super::machine_work::identity(named))
        .or(handle)
        .unwrap_or("owner")
        .to_owned()
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

/// The session and the task `asked` runs for in `env`.
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
    let task = match named {
        Some(task) => Some(task).filter(identity),
        None => handle.as_deref().and_then(|handle| held_task(env, handle)),
    };
    (handle, task)
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
    // Its lease, and a run's tree and whether its checkout is clean, before its command starts.
    let prepared = recorded::folder(env)
        .zip(recorded::new_id().ok())
        .and_then(|(folder, id)| Prepared::new(folder, id, asked, env).ok());
    let id = prepared.as_ref().map(|prepared| prepared.id.clone());
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
        match role(asked, env) {
            Some(role) => command.env(recorded::PIPELINE_VAR, role.pipeline),
            None => command.env_remove(recorded::PIPELINE_VAR),
        };
    }
    // Its caller waits on it: the board's answer, and no other run's end, nor a held lock.
    let begin = |started: Option<Start>| {
        let recording =
            Recording::begin(prepared?, asked, env, (queued_at, started), false).ok()?;
        recording.sweep();
        Some(recording)
    };
    let forwarding = process.then(signals::Forwarding::install);
    let child = match command.spawn() {
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
                    (crate::store::now(), None),
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
    let ended = reaped(started.0);
    drop(child);
    // An end its waiter could not see is the one the reap read, as it came.
    let at = if seen.is_ok() {
        at
    } else {
        crate::store::now()
    };
    let (status, cpu_s) = match ended {
        Ok(ended) => ended,
        Err(error) => {
            let _ = writeln!(
                err,
                "tasks run: the end of {}: {error}",
                asked.command[0].to_string_lossy()
            );
            if let Some(recording) = recording {
                let reason = format!("its end could not be read: {error}");
                recording.finish(None, None, &reason, (at, None));
            }
            return 1;
        }
    };
    if let Some(recording) = recording {
        // The reap's count: whether it misses a process that left the command's tree is unknown.
        let cpu = Cpu {
            seconds: cpu_s,
            partial: None,
        };
        recording.finish(status.code(), status.signal(), "", (at, Some(cpu)));
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

/// This process's child `pid` reaped: how it ended, and the CPU seconds it took with every child it
/// reaped, which the reap reads.
fn reaped(pid: u32) -> io::Result<(std::process::ExitStatus, f64)> {
    let pid = libc::pid_t::try_from(pid).map_err(io::Error::other)?;
    loop {
        let mut status = 0;
        // SAFETY: rusage is plain data, for which all zeroes is a value.
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: reaps this process's child `pid`, writing its status and its usage.
        let waited = unsafe { libc::wait4(pid, &raw mut status, 0, &raw mut usage) };
        if waited == pid {
            let cpu = seconds(usage.ru_utime) + seconds(usage.ru_stime);
            return Ok((std::process::ExitStatus::from_raw(status), cpu));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// A `timeval` in seconds.
#[expect(
    clippy::cast_precision_loss,
    reason = "CPU seconds, far inside what an f64 holds exactly"
)]
fn seconds(time: libc::timeval) -> f64 {
    time.tv_sec as f64 + time.tv_usec as f64 / 1e6
}

/// The one task `handle` holds on the board `env` reaches, read without starting its server.
fn held_task(env: &Env, handle: &str) -> Option<String> {
    let reached = super::reach(env, &env.cwd, false).ok()?;
    let reached = super::Reached {
        client: reached.client.waiting(WAIT),
        ..reached
    };
    let store = super::open_unchecked(reached, &super::Clock::Machine);
    match store.claimable(handle).ok()?.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
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

/// The project `env`'s working directory is in, by its id.
fn project(env: &Env) -> Option<String> {
    let machine = crate::project::machine_folder(
        |name| env.var(name).map(OsString::from),
        cfg!(target_os = "macos"),
    )
    .ok();
    let found = crate::project::find(&env.cwd, machine.as_deref()).ok()?;
    Some(recorded::bounded(&found.id))
}

/// CPU seconds, kept to the millisecond; none for what no clock says.
fn cpu_seconds(seconds: f64) -> Option<f64> {
    (seconds.is_finite() && seconds >= 0.0).then(|| (seconds * 1000.0).round() / 1000.0)
}

/// The CPU a command took, as its recorder read it: its seconds, and whether they are known short
/// ([`symphony_api::RunLine::cpu_partial`]; none where the recorder does not watch for that).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Cpu {
    seconds: f64,
    partial: Option<bool>,
}

/// An operation's lease, `operations/ID.lease`, held locked while its recorder records it: let go,
/// its lock first, then its file.
struct Lease {
    lock: Option<FileLock>,
    path: PathBuf,
}

impl Drop for Lease {
    fn drop(&mut self) {
        drop(self.lock.take());
        let _ = fs::remove_file(&self.path);
    }
}

/// What a recorder knows before its command starts: its operation's lease, taken, and for a run or
/// a step, the commit, its tree and whether the checkout was clean then.
struct Prepared {
    folder: PathBuf,
    id: String,
    lease: Lease,
    commit: Option<String>,
    /// Its role, the tree and whether the checkout differed from its commit: none for an operation
    /// of the work folder alone.
    run: Option<(Role, Option<String>, Option<bool>)>,
}

impl Prepared {
    /// The lease of the operation `id` of `asked` in `env` taken in the work folder `folder`, then
    /// a run's or a step's tree and cleanliness asked of git for at most [`GIT_WAIT`].
    fn new(folder: PathBuf, id: String, asked: &Asked, env: &Env) -> io::Result<Prepared> {
        let operations = folder.join(recorded::OPERATIONS);
        fs::create_dir_all(&operations)?;
        let path = operations.join(format!("{id}.lease"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        let mut lease = Lease { lock: None, path };
        file.lock()?;
        lease.lock = Some(FileLock(file));
        let commit = asked.commit.clone().or_else(|| commit(&env.cwd));
        let run = role(asked, env).map(|role| {
            let (tree, dirty) = commit.as_deref().map_or((None, None), |commit| {
                crate::runs::tree_and_dirty(&env.cwd, commit, asked.commit.is_some(), GIT_WAIT)
            });
            (role, tree, dirty)
        });
        Ok(Prepared {
            folder,
            id,
            lease,
            commit,
            run,
        })
    }
}

/// One operation being recorded: its record, written whole at each change, and its lease, held
/// locked until it finishes.
struct Recording {
    folder: PathBuf,
    /// The day files of its project's runs, where its line goes when it is a run or a step.
    runs: Option<PathBuf>,
    /// The longest an append of its lines waits for their folder's lock.
    wait: Duration,
    path: PathBuf,
    lease: Lease,
    record: Mutex<Record>,
}

impl Recording {
    /// Its record written as `prepared` was, as `asked` in `env`, asked for at `queued_at`: running
    /// since `started` when its command started, else queued. The task it is for is asked of the
    /// board first, then the run it repeats read (by a recorder `apart` from its command's caller,
    /// once each earlier run that may be that one ended, waiting [`RERUN_WAIT`] at most); the times
    /// were stamped before. A run or a step that started appends its start line.
    fn begin(
        prepared: Prepared,
        asked: &Asked,
        env: &Env,
        (queued_at, started): (f64, Option<Start>),
        apart: bool,
    ) -> io::Result<Recording> {
        let Prepared {
            folder,
            id,
            lease,
            commit,
            run,
        } = prepared;
        let (handle, task) = attributed(asked, env);
        let branch = asked.branch.clone().or_else(|| {
            crate::git::checkout(&env.cwd)
                .map(|(_, branch)| branch)
                .filter(|branch| !branch.is_empty())
        });
        let runs = crate::runs::folder(env).map(|runs| env.cwd.join(runs));
        let label = recorded::bounded(&asked.label());
        let run = run.map(|(Role { pipeline, step }, tree, dirty)| {
            let rerun_of = match (&runs, &tree, dirty) {
                (Some(runs), Some(tree), Some(false)) => {
                    let same = crate::runs::Same {
                        pipeline: &pipeline,
                        step: step.as_deref(),
                        label: &label,
                        tree,
                    };
                    if apart {
                        settled(&folder, &id, same, queued_at);
                    }
                    crate::runs::rerun_of(runs, same, queued_at)
                }
                _ => None,
            };
            Run {
                pipeline,
                step,
                trigger: trigger(env, handle.as_deref()),
                project: project(env),
                tree,
                dirty,
                rerun_of,
                cpu_s: None,
                cpu_partial: None,
                runs: runs
                    .as_ref()
                    .and_then(|runs| runs.to_str())
                    .filter(|runs| runs.len() <= recorded::MAX_TEXT)
                    .map(str::to_owned),
            }
        });
        let path = folder.join(recorded::OPERATIONS).join(format!("{id}.json"));
        let record = Record {
            version: 1,
            operation_id: id,
            parent_id: env
                .var(recorded::PARENT_VAR)
                .filter(|parent| recorded::nonce(parent))
                .map(str::to_owned),
            phase: asked.phase,
            label,
            state: if started.is_some() {
                Step::Running
            } else {
                Step::Queued
            },
            handle,
            task,
            checkout: recorded::bounded(&env.cwd.to_string_lossy()),
            branch: branch.map(|branch| recorded::bounded(&branch)),
            commit,
            machine_id: machine_id(env),
            host: recorded::bounded(&sys::hostname()),
            pid: std::process::id(),
            child_pid: started.map(|(pid, _)| pid),
            queued_at,
            started_at: started.map(|(_, at)| at),
            ended_at: None,
            beat_at: crate::store::now(),
            exit_code: None,
            signal: None,
            reason: String::new(),
            run,
        };
        recorded::write(&path, &record)?;
        let recording = Recording {
            folder,
            runs,
            wait: if apart {
                crate::runs::LOCK_WAIT
            } else {
                crate::runs::BRIEF_WAIT
            },
            path,
            lease,
            record: Mutex::new(record),
        };
        if started.is_some() {
            recording.line(&recording.held());
        }
        Ok(recording)
    }

    /// The operations in its folder whose recorders are gone, finished, and the end line of each
    /// that was a run or a step appended.
    fn sweep(&self) {
        let _ = recorded::sweep(&self.folder, crate::store::now());
    }

    /// The line `record` says appended to the day files, when it is a run's or a step's.
    fn line(&self, record: &Record) {
        if let (Some(runs), Some(line)) = (&self.runs, crate::runs::line(record)) {
            let _ = crate::runs::append(runs, &line, crate::store::now(), self.wait);
        }
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

    /// Its command started: `(pid, at)`. A run or a step appends its start line.
    #[cfg_attr(
        not(target_os = "macos"),
        expect(
            dead_code,
            reason = "a recorder that watches its command says when it started"
        )
    )]
    fn started(&self, (pid, at): Start) {
        let mut record = self.held();
        record.state = Step::Running;
        record.started_at = Some(at);
        record.child_pid = Some(pid);
        record.beat_at = crate::store::now();
        let _ = recorded::write(&self.path, &record);
        self.line(&record);
    }

    /// It still runs.
    fn beat(&self) {
        self.update(|_, _| {});
    }

    /// It ended at `at` so (`exit_code` or `signal`, neither when that is not known), its command
    /// and the children it reaped having taken `cpu` of CPU: kept in the history, its end line
    /// appended when it is a run or a step, then its record written finished, then its lease let
    /// go. A recorder ended between the first and the third leaves it unfinished for a sweep to keep
    /// again, and to end its line again: the history reads each operation once, and a reader of the
    /// day files the first end line of each run.
    fn finish(
        self,
        exit_code: Option<i32>,
        signal: Option<i32>,
        reason: &str,
        (at, cpu): (f64, Option<Cpu>),
    ) {
        let mut record = self.held();
        record.state = Step::Finished;
        record.ended_at = Some(at);
        record.beat_at = crate::store::now();
        record.exit_code = exit_code;
        record.signal = signal;
        reason.clone_into(&mut record.reason);
        if let Some(run) = &mut record.run {
            run.cpu_s = cpu.and_then(|cpu| cpu_seconds(cpu.seconds));
            run.cpu_partial = cpu
                .and_then(|cpu| cpu.partial)
                .filter(|_| run.cpu_s.is_some());
        }
        let _ = recorded::keep(&self.folder, &record);
        self.line(&record);
        let _ = recorded::write(&self.path, &record);
        drop(record);
        drop(self.lease);
    }
}

/// Once no operation in the work folder `folder` but `ours`, asked for by `before`, may still end as
/// the run `same` repeats (`recorded::pending`), or [`RERUN_WAIT`] has passed: its end line is then
/// in the day files, when it is to be.
fn settled(folder: &Path, ours: &str, same: crate::runs::Same, before: f64) {
    let deadline = std::time::Instant::now() + RERUN_WAIT;
    let earlier = |record: &Record| {
        record.run.as_ref().is_some_and(|run| {
            run.dirty == Some(false)
                && same.of(
                    &run.pipeline,
                    run.step.as_deref(),
                    &record.label,
                    run.tree.as_deref(),
                )
        })
    };
    while std::time::Instant::now() < deadline && recorded::pending(folder, ours, before, earlier) {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The `tasks` process become its command, watched by a recorder it forks first.
#[cfg(target_os = "macos")]
mod supervised {
    use super::{Asked, BEAT, Cpu, Prepared, READY, Recording, SAMPLE, role, unstarted};
    use crate::cli::Env;
    use crate::cli::machine_work::recorded;
    use std::ffi::CString;
    use std::io::{self, Write};
    use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
    use std::os::unix::ffi::OsStrExt as _;
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::time::{Duration, Instant};

    /// What the recorder's watch saw of its command, when it saw it, and the most CPU it read the
    /// command's processes had taken by then.
    type Heard = io::Result<(Seen, f64, Option<Cpu>)>;

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
            match role(asked, env) {
                // SAFETY: as above.
                Some(role) => unsafe { std::env::set_var(recorded::PIPELINE_VAR, role.pipeline) },
                // SAFETY: as above.
                None => unsafe { std::env::remove_var(recorded::PIPELINE_VAR) },
            }
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

    /// The recorder: out of the caller's session and streams, it watches `target`, takes the
    /// operation's lease and reads a run's tree and cleanliness, and says so on `tell` when it has
    /// a work folder to record it in; then it records the operation `id` asked for at `queued_at`
    /// until `target` ends, and ends. Told, the command execs: that is its start.
    fn record(
        target: libc::pid_t,
        tell: &OwnedFd,
        asked: &Asked,
        env: &Env,
        (id, queued_at): (&str, f64),
    ) -> ! {
        let watch = Watch::new(target);
        // SAFETY: a session of its own: no signal for the caller's group or terminal reaches it.
        unsafe { libc::setsid() };
        detach(&[
            watch.as_ref().map_or(-1, |watch| watch.0.as_raw_fd()),
            tell.as_raw_fd(),
        ]);
        let prepared = watch
            .as_ref()
            .ok()
            .and_then(|_| recorded::folder(env))
            .and_then(|folder| Prepared::new(folder, id.to_owned(), asked, env).ok());
        let start = (target.unsigned_abs(), crate::store::now());
        let told = prepared.is_some()
            // SAFETY: writes one byte from it; a command gone on without it hears nothing.
            && unsafe { libc::write(tell.as_raw_fd(), b"1".as_ptr().cast(), 1) } == 1;
        // The command waited for no more than that.
        if let (true, Ok(watch), Some(prepared)) = (told, watch, prepared) {
            // Read on a thread of its own, so what it sees is stamped as it comes while the board
            // is asked which task this is for.
            let (tell, heard) = mpsc::channel();
            std::thread::spawn(move || watching(&watch, &tell, target));
            if let Ok(recording) = Recording::begin(prepared, asked, env, (queued_at, None), true) {
                recording.sweep();
                follow(&heard, recording, start);
            }
        }
        // SAFETY: ends the recorder without the runtime's exit, which is the caller's.
        unsafe { libc::_exit(0) }
    }

    /// What `watch` sees of `target`, sent on `tell` with the time it saw it and the most CPU it
    /// read `target`'s processes had taken, until its process ends, the watch fails, or nothing
    /// listens. That is read every [`SAMPLE`] and as soon as each event comes, its exit's first: a
    /// process that ended is read until its parent reaps it, and a last reading that comes after
    /// misses what ran since the one before.
    fn watching(watch: &Watch, tell: &mpsc::Sender<Heard>, target: libc::pid_t) {
        let tick = cpu::tick_ns();
        let mut tree = cpu::Tree::new(cpu::Machine, target);
        let mut most = None;
        loop {
            let next = watch.next(SAMPLE);
            let read = tree.read();
            most = most.max(read);
            match next {
                Ok(Some(seen)) => {
                    let exited = seen.exit.is_some();
                    if exited && read.is_none() {
                        tree.partial = true;
                    }
                    let cpu = most.map(|ticks| Cpu {
                        seconds: cpu::seconds(ticks, tick),
                        partial: Some(tree.partial),
                    });
                    if tell.send(Ok((seen, crate::store::now(), cpu))).is_err() || exited {
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
    /// descriptor closed but those it `keep`s: its watch, and the pipe it tells the command on.
    fn detach(keep: &[libc::c_int]) {
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
            if fd > 2 && !keep.contains(&fd) {
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
                Ok(Ok((seen, at, cpu))) => {
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
                        recording.finish(code, signal, reason, (at, cpu.filter(|_| started)));
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
                    recording.finish(None, None, &reason, (crate::store::now(), None));
                    return;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let reason = "its recorder lost sight of its command";
                    recording.finish(None, None, reason, (crate::store::now(), None));
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

    /// The CPU a process and the processes under it took, as one that is not their parent reads it
    /// (`proc_pid_rusage`): each one's own and that of the children it reaped, in the kernel's
    /// ticks. A process reads until its parent reaps it; then its time is its parent's.
    pub(super) mod cpu {
        use std::collections::BTreeSet;

        /// The most processes one reading visits.
        const MOST: usize = 4096;
        /// The most children of one process read.
        const CHILDREN: usize = 512;

        /// Why a process was not read.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(in super::super) enum Unread {
            /// It is gone: reaped, its time its parent's.
            Gone,
            /// It is there, but not this user's to read (a set-user-ID program's, say).
            Denied,
        }

        /// What a reading asks of the kernel.
        pub(in super::super) trait Kernel {
            /// The CPU `pid` took, with that of the children it reaped, in ticks.
            fn usage(&self, pid: libc::pid_t) -> Result<u64, Unread>;
            /// The processes whose parent `pid` is now, at most `room` of them.
            fn children(&self, pid: libc::pid_t, room: usize) -> Vec<libc::pid_t>;
            /// Whether `pid` is a process still.
            fn alive(&self, pid: libc::pid_t) -> bool;
        }

        /// This machine's kernel.
        pub(in super::super) struct Machine;

        impl Kernel for Machine {
            fn usage(&self, pid: libc::pid_t) -> Result<u64, Unread> {
                // SAFETY: plain data, for which all zeroes is a value.
                let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
                // SAFETY: writes one rusage_info_v2, the size of the flavor asked for, into `info`.
                let read = unsafe {
                    libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, (&raw mut info).cast())
                };
                if read != 0 {
                    let gone = std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
                    return Err(if gone { Unread::Gone } else { Unread::Denied });
                }
                Ok([
                    info.ri_user_time,
                    info.ri_system_time,
                    info.ri_child_user_time,
                    info.ri_child_system_time,
                ]
                .into_iter()
                .fold(0, u64::saturating_add))
            }

            fn children(&self, pid: libc::pid_t, room: usize) -> Vec<libc::pid_t> {
                let mut pids: Vec<libc::pid_t> = vec![0; room];
                let bytes = libc::c_int::try_from(std::mem::size_of_val(pids.as_slice()))
                    .unwrap_or(libc::c_int::MAX);
                // SAFETY: writes at most `bytes` bytes of pids into `pids`, which holds that many.
                let found =
                    unsafe { libc::proc_listchildpids(pid, pids.as_mut_ptr().cast(), bytes) };
                pids.truncate(usize::try_from(found).unwrap_or(0));
                pids.retain(|child| *child > 0);
                pids
            }

            fn alive(&self, pid: libc::pid_t) -> bool {
                // SAFETY: signal 0 only asks whether the process is there.
                let there = unsafe { libc::kill(pid, 0) } == 0;
                there || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
            }
        }

        /// A command's processes, read again and again: what each reading found, and whether one
        /// knew it left out CPU they took (`partial`): a process under the command it could not
        /// read, more children of one process than [`CHILDREN`] or more processes than [`MOST`],
        /// or one read under it before that still runs but no longer under it (its parent ended
        /// first, and it went to the system's), whose time from then on no reading sees. One that
        /// starts and leaves between two readings is not seen at all.
        pub(in super::super) struct Tree<K> {
            kernel: K,
            root: libc::pid_t,
            most: usize,
            room: usize,
            /// The processes under the root the last reading read.
            under: BTreeSet<libc::pid_t>,
            pub(in super::super) partial: bool,
        }

        impl<K: Kernel> Tree<K> {
            pub(in super::super) fn new(kernel: K, root: libc::pid_t) -> Tree<K> {
                Tree::bounded(kernel, root, MOST, CHILDREN)
            }

            /// Reading at most `most` processes, `room` children of each.
            pub(in super::super) fn bounded(
                kernel: K,
                root: libc::pid_t,
                most: usize,
                room: usize,
            ) -> Tree<K> {
                Tree {
                    kernel,
                    root,
                    most,
                    room,
                    under: BTreeSet::new(),
                    partial: false,
                }
            }

            /// The CPU the root and every process under it have taken so far, none once the root
            /// cannot be read. Each is read before its children are listed, so a child its parent
            /// reaps meanwhile is counted once, in the one or the other, or missed until the next
            /// reading: never twice.
            pub(in super::super) fn read(&mut self) -> Option<u64> {
                let mut total = self.kernel.usage(self.root).ok()?;
                let mut next = self.listed(self.root);
                let mut read = BTreeSet::new();
                let mut visited = 1;
                while let Some(pid) = next.pop() {
                    if visited >= self.most {
                        self.partial = true;
                        break;
                    }
                    visited += 1;
                    match self.kernel.usage(pid) {
                        Ok(taken) => {
                            total = total.saturating_add(taken);
                            read.insert(pid);
                            let children = self.listed(pid);
                            next.extend(children);
                        }
                        Err(Unread::Denied) => self.partial = true,
                        Err(Unread::Gone) => {}
                    }
                }
                if self
                    .under
                    .iter()
                    .any(|pid| !read.contains(pid) && self.kernel.alive(*pid))
                {
                    self.partial = true;
                }
                self.under = read;
                Some(total)
            }

            /// The children of `pid`; as many as there is room for is too many to know all.
            fn listed(&mut self, pid: libc::pid_t) -> Vec<libc::pid_t> {
                let children = self.kernel.children(pid, self.room);
                if children.len() >= self.room {
                    self.partial = true;
                }
                children
            }
        }

        /// Nanoseconds a tick is: what the machine's timebase says.
        #[expect(
            deprecated,
            reason = "libc's mach_timebase_info, which points at the mach2 crate this workspace does not take"
        )]
        pub(in super::super) fn tick_ns() -> f64 {
            let mut base = libc::mach_timebase_info { numer: 0, denom: 0 };
            // SAFETY: writes the timebase into `base`.
            let read = unsafe { libc::mach_timebase_info(&raw mut base) };
            if read == 0 && base.numer != 0 && base.denom != 0 {
                f64::from(base.numer) / f64::from(base.denom)
            } else {
                1.0
            }
        }

        /// `ticks` in seconds, `tick` nanoseconds each.
        #[expect(
            clippy::cast_precision_loss,
            reason = "CPU ticks, far inside what an f64 holds exactly"
        )]
        pub(in super::super) fn seconds(ticks: u64, tick: f64) -> f64 {
            ticks as f64 * tick / 1e9
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
                pipeline: None,
                step: None,
                commit: None,
                branch: None,
                command: args(&["cargo", "--", "--exact"]),
            }
        );
        assert_eq!(parse(&args(&["--probe"])), Ok(None));
        assert!(super::super::usage::HELP.contains("\n    run "));
    }

    #[test]
    fn a_command_line_names_its_pipeline_step_commit_and_branch() {
        let tip = "0123456789abcdef0123456789abcdef01234567";
        let read = parse(&args(&[
            "--pipeline",
            "review-callback",
            "--step",
            "client-check",
            "--commit",
            tip,
            "--branch",
            "tools/run-record",
            "--phase",
            "test",
            "--",
            "true",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(read.pipeline.as_deref(), Some("review-callback"));
        assert_eq!(read.step.as_deref(), Some("client-check"));
        assert_eq!(read.commit.as_deref(), Some(tip));
        assert_eq!(read.branch.as_deref(), Some("tools/run-record"));
        let sha256 = "a".repeat(64);
        let read = parse(&args(&[
            "--phase", "test", "--commit", &sha256, "--", "true",
        ]));
        assert_eq!(read.unwrap().unwrap().commit, Some(sha256));
        for (words, said) in [
            (vec!["--pipeline", "a b"], "--pipeline is 1 to 64"),
            (vec!["--step", ""], "--step is 1 to 64"),
            (vec!["--step", &"s".repeat(65)], "--step is 1 to 64"),
            (vec!["--pipeline", "a/b"], "--pipeline is 1 to 64"),
            (vec!["--commit", "HEAD"], "full commit id"),
            (vec!["--commit", &tip[..39]], "full commit id"),
            (vec!["--commit", &"g".repeat(40)], "full commit id"),
            (vec!["--branch"], "needs a value"),
        ] {
            let mut words = [vec!["--phase", "test"], words.clone()].concat();
            words.extend(["--", "true"]);
            let refused = parse(&args(&words)).unwrap_err();
            assert!(refused.contains(said), "{words:?}: {refused}");
        }
    }

    fn env(vars: &[(&str, &str)]) -> Env {
        Env {
            vars: vars
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect(),
            cwd: std::env::temp_dir(),
        }
    }

    #[test]
    fn a_step_inside_a_run_is_its_pipelines_and_a_pipeline_named_at_top_level_a_run() {
        let parent = "0123456789abcdef0123456789abcdef";
        let inside = [
            (recorded::PARENT_VAR, parent),
            (recorded::PIPELINE_VAR, "verify"),
        ];
        let asked = |words: &[&str]| {
            let mut words = words.to_vec();
            words.extend(["--phase", "build", "--", "true"]);
            parse(&args(&words)).unwrap().unwrap()
        };
        let lint = asked(&["--pipeline", "lint", "--step", "lint"]);
        let run = |pipeline: &str| {
            Some(Role {
                pipeline: pipeline.into(),
                step: None,
            })
        };
        assert_eq!(
            role(&lint, &env(&inside)),
            Some(Role {
                pipeline: "verify".into(),
                step: Some("lint".into()),
            })
        );
        assert_eq!(role(&lint, &env(&[])), run("lint"), "at top level, a run");
        // At top level: no recorded command it runs inside, whatever a pipeline says.
        for outside in [
            vec![(recorded::PIPELINE_VAR, "verify")],
            vec![
                (recorded::PARENT_VAR, "not-an-id"),
                (recorded::PIPELINE_VAR, "verify"),
            ],
        ] {
            assert_eq!(role(&lint, &env(&outside)), run("lint"), "{outside:?}");
        }
        // Inside a recorded command that is no run, or whose pipeline does not read: neither.
        for inside_another in [
            vec![(recorded::PARENT_VAR, parent)],
            vec![
                (recorded::PARENT_VAR, parent),
                (recorded::PIPELINE_VAR, "two words"),
            ],
        ] {
            assert_eq!(
                role(&lint, &env(&inside_another)),
                None,
                "{inside_another:?}"
            );
        }
        let step_only = asked(&["--step", "tests"]);
        assert_eq!(role(&step_only, &env(&[])), None);
        assert_eq!(
            role(&step_only, &env(&inside)).and_then(|role| role.step),
            Some("tests".into())
        );
        // A run named inside a run is no step of it, nor a run of its own.
        assert_eq!(role(&asked(&["--pipeline", "lint"]), &env(&inside)), None);
        assert_eq!(role(&asked(&[]), &env(&inside)), None);
    }

    #[test]
    fn a_trigger_is_the_one_named_else_the_session_else_the_owner() {
        assert_eq!(trigger(&env(&[]), None), "owner");
        assert_eq!(trigger(&env(&[]), Some("ember-osprey")), "ember-osprey");
        let named = env(&[(recorded::TRIGGER_VAR, "on_review")]);
        assert_eq!(trigger(&named, Some("ember-osprey")), "on_review");
        let unreadable = env(&[(recorded::TRIGGER_VAR, "two\nlines")]);
        assert_eq!(trigger(&unreadable, None), "owner");
    }

    #[test]
    fn cpu_seconds_keep_milliseconds_and_no_count_a_clock_cannot_say() {
        assert_eq!(cpu_seconds(1.234_56), Some(1.235));
        assert_eq!(cpu_seconds(0.0), Some(0.0));
        assert_eq!(cpu_seconds(-0.5), None);
        assert_eq!(cpu_seconds(f64::NAN), None);
        assert_eq!(cpu_seconds(f64::INFINITY), None);
    }

    /// A loop of the shell's that takes some CPU.
    const BURN: &str = "i=0; while [ $i -lt 100000 ]; do i=$((i+1)); done";

    /// A shell that burns CPU in a child it reaps, then becomes `then`.
    fn burning(then: &str) -> sys::Child {
        sys::command("/bin/sh")
            .args(["-c", &format!("({BURN}) & wait; {then}")])
            .spawn()
            .unwrap()
    }

    #[test]
    fn the_reap_reads_the_cpu_of_the_command_and_the_children_it_reaped() {
        let child = burning("exit 3");
        let (status, cpu) = reaped(child.id()).unwrap();
        assert_eq!(status.code(), Some(3));
        assert!(cpu > 0.05, "{cpu}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_watcher_reads_the_cpu_of_a_process_and_of_those_under_it() {
        use supervised::cpu::{Machine, Tree};
        let mut shell = burning("exec sleep 30");
        let pid = libc::pid_t::try_from(shell.id()).unwrap();
        let tick = supervised::cpu::tick_ns();
        assert!(tick > 0.0);
        let mut tree = Tree::new(Machine, pid);
        let began = std::time::Instant::now();
        let mut most = 0.0;
        // The shell's child burns, then is reaped: its time is the shell's, read the same.
        while began.elapsed() < Duration::from_secs(20) {
            let read = tree.read().unwrap();
            most = supervised::cpu::seconds(read, tick);
            if most > 0.05 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(most > 0.05, "{most}");
        assert!(!tree.partial, "a child reaped by its parent is no loss");
        shell.kill().unwrap();
        let _ = shell.wait();
        assert_eq!(tree.read(), None, "a process gone reads none");
    }

    /// A process table of the test's own: each pid's parent, its ticks, and whether it may be read.
    #[cfg(target_os = "macos")]
    #[derive(Default)]
    struct Table(
        std::cell::RefCell<std::collections::BTreeMap<libc::pid_t, (libc::pid_t, u64, bool)>>,
    );

    #[cfg(target_os = "macos")]
    impl Table {
        fn with(processes: &[(libc::pid_t, libc::pid_t, u64)]) -> Table {
            let table = Table::default();
            for &(pid, parent, ticks) in processes {
                table.0.borrow_mut().insert(pid, (parent, ticks, true));
            }
            table
        }
    }

    #[cfg(target_os = "macos")]
    impl supervised::cpu::Kernel for &Table {
        fn usage(&self, pid: libc::pid_t) -> Result<u64, supervised::cpu::Unread> {
            match self.0.borrow().get(&pid) {
                None => Err(supervised::cpu::Unread::Gone),
                Some((_, _, false)) => Err(supervised::cpu::Unread::Denied),
                Some((_, ticks, true)) => Ok(*ticks),
            }
        }

        fn children(&self, pid: libc::pid_t, room: usize) -> Vec<libc::pid_t> {
            let table = self.0.borrow();
            let children = table.iter().filter(|(_, (parent, ..))| *parent == pid);
            children.map(|(child, _)| *child).take(room).collect()
        }

        fn alive(&self, pid: libc::pid_t) -> bool {
            self.0.borrow().contains_key(&pid)
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_reading_knows_when_it_misses_a_process_it_may_not_read_too_many_or_one_that_left() {
        use supervised::cpu::Tree;
        // 1 runs 2 and 3; 3 runs 4.
        let whole = [(1, 0, 10), (2, 1, 20), (3, 1, 30), (4, 3, 40)];
        let table = Table::with(&whole);
        let mut tree = Tree::new(&table, 1);
        assert_eq!(tree.read(), Some(100));
        assert!(!tree.partial);
        // 4 ends and 3 reaps it: its time is 3's now, and nothing is lost.
        table.0.borrow_mut().remove(&4);
        table.0.borrow_mut().insert(3, (1, 70, true));
        assert_eq!(tree.read(), Some(100));
        assert!(!tree.partial, "a child reaped is no loss");
        // 3 ends unreaped by 1, leaving its child 5 to the system: 5 runs on, unread.
        table.0.borrow_mut().insert(5, (3, 5, true));
        assert_eq!(tree.read(), Some(105));
        table.0.borrow_mut().remove(&3);
        table.0.borrow_mut().insert(5, (0, 9, true));
        assert_eq!(tree.read(), Some(30));
        assert!(tree.partial, "a process that left the tree while it runs");
        // One it may not read.
        let table = Table::with(&whole);
        table.0.borrow_mut().insert(2, (1, 20, false));
        let mut tree = Tree::new(&table, 1);
        assert_eq!(tree.read(), Some(80));
        assert!(tree.partial, "a process it may not read");
        // More children of one than it lists, or more processes than it reads.
        let table = Table::with(&whole);
        let mut tree = Tree::bounded(&table, 1, 4096, 2);
        tree.read();
        assert!(tree.partial, "as many children as there is room for");
        let mut tree = Tree::bounded(&table, 1, 3, 512);
        tree.read();
        assert!(tree.partial, "more processes than a reading visits");
        let mut tree = Tree::bounded(&table, 1, 4, 3);
        assert_eq!(tree.read(), Some(100));
        assert!(!tree.partial, "room for each");
        // A root that cannot be read reads none.
        let table = Table::with(&[(1, 0, 10)]);
        table.0.borrow_mut().insert(1, (0, 10, false));
        assert_eq!(Tree::new(&table, 1).read(), None);
        assert_eq!(Tree::new(&Table::default(), 1).read(), None);
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
