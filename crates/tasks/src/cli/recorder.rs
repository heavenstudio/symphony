//! `tasks run --phase build|test [--label TEXT] [--task SLUG] [--as HANDLE] -- COMMAND [ARGS...]`:
//! COMMAND run with its caller's arguments (any bytes), streams, environment, working directory and
//! limits, its exit status its caller's; and recorded in the work folder ([`recorded`]) while it
//! runs: queued, running once it started, said again every [`BEAT`] while it runs, then kept in the
//! history and finished with how it ended. A build or test it records puts the task it ran for in
//! the board's Build or Test column (docs/workflow/MACHINE_WORK.md). A build wrapper runs it by the board's
//! binary (`tools/record.sh`, a review callback, the release run): the `tasks` recipe `symphony init`
//! installs refuses it, as an agent runs `just tasks` unasked and this runs any command.
//!
//! On macOS the `tasks` process becomes COMMAND (`execvp`): the process its caller started, waits
//! for and signals is the command's, which keeps every descriptor, the signal mask and the ignored
//! signals it was handed (only SIGPIPE starts at its default, as a shell's command does), and
//! whatever ends the one, SIGKILL included, ends the other. First it forks its recorder, detached
//! in a session of its own with none of the caller's streams open and out of its folder (at the
//! root, so it is never a process working in the command's checkout), which watches that process
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
//! branch when it is a task's slug, whatever its status (an approved task's executor holds none);
//! else the one held by the live full session the Claude Code session ([`CLAUDE_SESSION_VAR`])
//! binds, which a subagent shares with its parent, so it never beats the branch. A session counts
//! only while the board reads it live: one that finished without leaving, or went stale or lost, is
//! passed over. Of several tasks a session holds, the one the others are under; none when the board
//! cannot say. A command a recorder runs names its operation in [`recorded::PARENT_VAR`], so a
//! recorder it starts records it as its parent. That read holds up no time it records: the
//! operation is queued when `tasks run` was called, started when its command did (on macOS, when
//! its recorder told it to exec) and ended when its end came, each seen as it happens, by a thread
//! of its own while the board is asked; the record is written once the board answered or the wait
//! ran out.
//!
//! While its command runs, the recorder of an operation whose record names a session says on the
//! board that the session is there ([`Session`]): every [`SEEN`], and once more as the command
//! ends. An agent waiting on its own build or test makes no tool call, and reads live by that for
//! as long as the run goes on, then ages from its end. It is the one write a recorder makes on the
//! board, by one try, each answer waited for [`WAIT`] at most. Its command never waits on it, and
//! its caller only for the last where the recorder runs the command as its child: the status
//! comes once that is said.
//!
//! Nothing it records reaches the command's streams: a record it cannot write leaves the command
//! running as it would unrecorded. A run's or a step's stdout and stderr, which its lines name a
//! log of ([`crate::run_logs`]), are the one exception, by what its caller handed it ([`keeps`]).
//! Where either is a terminal, the command keeps the caller's streams and the run keeps no log.
//! Where either is a regular file, the command keeps them as well, and what it added to the
//! caller's files past where they ended as it started is copied, plain, into the log once it ended
//! ([`crate::run_logs::Files`]): its bytes are in the file before its status reaches anyone, and a
//! recorder that dies loses only the record and the log. Otherwise each stream is a pipe its
//! recorder reads, copying what comes on to the caller's stream and, plain, into the log; the end
//! line its size once both streams ended, or [`DRAIN`] after the command did. A command whose
//! recorder died with it still running finds its pipes' reader gone: its next write ends it by
//! SIGPIPE (141, as a shell says it), as a pipeline's command whose reader went. On macOS the pipes
//! are made, and the files marked, before the command execs, its recorder saying whether it copies
//! pipes, and the command takes them as its stdout and stderr only once it did; elsewhere the
//! command is its child on them.
//!
//! It says something only of a command line it cannot read (2) and of a command that cannot start
//! (127, or 126, as a shell says them). `tasks run --probe` answers [`PROBE`]: what a wrapper asks
//! before it hands a build to a `tasks` that may be older than this command.
//!
//! An operation is also a run, or a step of one, which the day files keep a line of
//! ([`crate::runs`], docs/workflow/RUNS.md): a step when `--step` names it and it runs inside a run or a step
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
//! read, with whether a reading knew it missed some (`supervised::cpu::Tree`). There each beat also
//! writes the latest reading into the running record ([`recorded::CpuReading`]): what a reader, or a
//! test, waits on to know the processes it holds were read.
//!
//! The bounds the machine's speed meets ([`READY`], [`WAIT`], [`GIT_WAIT`], [`RERUN_WAIT`] and
//! [`crate::runs::LOCK_WAIT`]) are a test's to scale, on a scratch board's database alone
//! ([`SCALE_VAR`], [`Waits`]), so a loaded machine never decides what a test's run records; a run
//! on the live board's database keeps them whatever it names.

use super::Env;
use super::machine_work::recorded::{self, Phase, Record, Run, Step};
use crate::lock::Recovered as _;
use crate::sys::{self, FileLock};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

/// The command's name.
pub const COMMAND: &str = "run";
/// How often a running operation's recorder says it still runs: well inside the Symphony's
/// ten seconds before it calls work stale.
pub const BEAT: Duration = Duration::from_secs(2);
/// How long the board is asked which task the session holds.
const WAIT: Duration = Duration::from_secs(1);
/// How often a running operation's recorder says the session its record names is there: a
/// monitor's heartbeat, well inside the 90 s a session reads live for after one
/// ([`crate::health::LIVE_SECONDS`]), and never the record's own [`BEAT`].
const SEEN: Duration = Duration::from_secs(20);
/// How long a command waits to hear its recorder watches it before it runs unrecorded.
#[cfg(target_os = "macos")]
const READY: Duration = Duration::from_secs(1);
/// Names the task a run is for in place of the one its session holds: what an orchestrator, or a
/// subagent with no session of its own, sets for the builds and tests it runs for a task.
pub const TASK_VAR: &str = "TASKS_TASK";
/// The Claude Code session a command runs in, as Claude Code sets it for its tool calls: the key
/// the beat hook binds a full session's handle by.
pub const CLAUDE_SESSION_VAR: &str = "CLAUDE_CODE_SESSION_ID";
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
/// The longest a run's recorder waits, once its command ended, for the streams it copies to end
/// before its end line says how much its log holds: a process that outlived the command and holds
/// one is not waited for longer.
const DRAIN: Duration = Duration::from_secs(1);
/// Names a test's scale of the waits a machine's speed decides ([`Waits`]): a whole number from 1
/// to [`MOST_SCALE`], taken only on a scratch board.
pub const SCALE_VAR: &str = "TASKS_RUN_WAIT_SCALE";
/// The largest scale [`SCALE_VAR`] takes: a minute for a bound of a second.
const MOST_SCALE: u32 = 60;
/// Names a file a run's recorder makes once it has stamped its command's end, whether or not the
/// board has answered which task the run is for: a test's mark of that moment, read only when set
/// ([`end_mark`]). It is made only where no file is, so it never replaces one.
pub const END_MARK_VAR: &str = "TASKS_RUN_END_MARK";
const USAGE: &str = "usage: tasks run --phase build|test [--label TEXT] [--task SLUG] [--as HANDLE]\n                 [--pipeline NAME] [--step NAME] [--commit SHA] [--branch NAME] -- COMMAND [ARGS...]\n       tasks run --probe\n";

/// `tasks run --help`, `just tasks help run`: its usage and what it does.
pub fn help() -> String {
    // The versions `--probe` prints, each as it is printed on a line of its own.
    let versions: Vec<String> = PROBE.lines().map(|line| format!("`{line}`")).collect();
    format!(
        "{USAGE}\nRun COMMAND as it would run alone, its exit status yours, and record it for the\n\
         board's Build or Test column while it runs (docs/workflow/MACHINE_WORK.md).\n\
         The task is --task, else $TASKS_TASK, else the one task the session holds. Run\n\
         it by the board's binary (.target/bin/tasks run), as a build wrapper does:\n\
         `just tasks`, which an agent runs unasked, refuses it, since it runs any\n\
         command.\n\n\
         options:\n  \
         -h, --help       show this help message and exit\n  \
         --phase PHASE    build or test: the column it is recorded in\n  \
         --label TEXT     how the column names it (default: the command's words)\n  \
         --task SLUG      the task it runs for\n  \
         --as HANDLE      the session it runs for (default: $TASKS_SESSION)\n  \
         --pipeline NAME  a run of this pipeline, when no recorded command runs it (docs/workflow/RUNS.md)\n  \
         --step NAME      a step of the run that runs it, by this name\n  \
         --commit SHA     the full commit id it checks, when not its checkout's\n  \
         --branch NAME    that commit's branch\n  \
         --probe          print each version of this command line it reads, a line each,\n                   \
         oldest first: {}\n",
        versions.join(", ")
    )
}

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

/// How long a run waits on what the machine's speed decides: its recorder's word that it watches
/// its command ([`READY`]), the board's answer ([`WAIT`]), git's ([`GIT_WAIT`]) and, apart from its
/// command, another recorder's end line ([`RERUN_WAIT`]) and its hold on the day files' lock
/// ([`crate::runs::LOCK_WAIT`]), each its own bound times a scale. The scale is 1 but where a test
/// names one ([`SCALE_VAR`]) and the board the run reaches is on a database other than the live
/// one's ([`crate::DATABASE`]: a test's `TASKS_DATABASE`, or a `--database`), which the board never
/// takes for a live board either ([`super::Reached::live`]), so a loaded machine never decides
/// what a test's run records. A run on the live one's database, by whatever server or folder it
/// reaches it and whether or not that server is up, or one that reaches no board, keeps its own
/// bounds whatever it names, and one that names none asks nothing more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Waits(u32);

impl Waits {
    /// The bounds themselves.
    const OWN: Waits = Waits(1);

    /// The waits of a run in `env`.
    fn of(env: &Env) -> Waits {
        let Some(scale) = env
            .var(SCALE_VAR)
            .and_then(|named| named.parse::<u32>().ok())
            .filter(|scale| (1..=MOST_SCALE).contains(scale))
        else {
            return Waits::OWN;
        };
        match super::reach(env, &env.cwd, false) {
            Ok(reached)
                if reached.live.is_none() && reached.client.database() != crate::DATABASE =>
            {
                Waits(scale)
            }
            _ => Waits::OWN,
        }
    }

    /// How long the board is asked which task the session holds.
    fn board(self) -> Duration {
        WAIT * self.0
    }

    /// How long git is asked a run's tree and whether its checkout is clean.
    fn git(self) -> Duration {
        GIT_WAIT * self.0
    }

    /// How long a command waits to hear its recorder watches it.
    #[cfg(target_os = "macos")]
    fn ready(self) -> Duration {
        READY * self.0
    }

    /// How long a recorder apart from its command waits for an earlier run it may repeat to end.
    fn rerun(self) -> Duration {
        RERUN_WAIT * self.0
    }

    /// How long a recorder apart from its command waits for the day files' lock before each line.
    fn lock(self) -> Duration {
        crate::runs::LOCK_WAIT * self.0
    }

    /// How long a run's recorder waits for the streams it copies to end once its command ended.
    fn drain(self) -> Duration {
        DRAIN * self.0
    }
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
    // Asked where one of its own options' names stands (each takes a value), before `--`: what
    // follows that is the command's.
    let options = args.split(|arg| arg == "--").next().unwrap_or_default();
    if options
        .iter()
        .step_by(2)
        .any(|arg| arg == "-h" || arg == "--help")
    {
        let _ = write!(out, "{}", help());
        return 0;
    }
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
/// worktree's, the branch's, the live session the Claude Code session binds), asked for at most
/// `wait`.
fn attributed(asked: &Asked, env: &Env, wait: Duration) -> (Option<String>, Option<String>) {
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
    match on_board(env, &place, wait) {
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

/// What a stream a caller handed a command is, as its log is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Caller {
    /// A terminal: a person's.
    Terminal,
    /// A regular file, which its caller may read the moment it has the command's status.
    File,
    /// Anything else: a pipe, a socket, `/dev/null`, one closed.
    Other,
}

/// How a run's log is written, if at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Keeps {
    /// No log: the command keeps its caller's streams.
    Nothing,
    /// The command's streams made pipes its recorder copies on and into the log as they come.
    Pipes,
    /// The command keeps its caller's streams, and what it added to its caller's regular files is
    /// copied into the log once it ended ([`crate::run_logs::Files`]).
    Files,
}

/// How the log of a command is written, by whether it is a run or a step (`run`) and what the
/// stdout and stderr its caller handed it are: none at a terminal, so a person sees the command
/// exactly as it would run unrecorded; from a regular file, which nothing then stands between the
/// command and, so its bytes are there before its status reaches anyone; else through pipes, whose
/// reader to their end gets every byte.
fn keeps(run: bool, streams: [Caller; 2]) -> Keeps {
    if !run || streams.contains(&Caller::Terminal) {
        Keeps::Nothing
    } else if streams.contains(&Caller::File) {
        Keeps::Files
    } else {
        Keeps::Pipes
    }
}

/// What this process's stdout and stderr are.
fn callers() -> [Caller; 2] {
    [1, 2].map(|fd| {
        // SAFETY: isatty only asks what a descriptor of this process's is.
        if unsafe { libc::isatty(fd) } == 1 {
            return Caller::Terminal;
        }
        // SAFETY: fstat writes one stat into it.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: as above, of a descriptor of this process's.
        let read = unsafe { libc::fstat(fd, &raw mut stat) } == 0;
        if read && stat.st_mode & libc::S_IFMT == libc::S_IFREG {
            Caller::File
        } else {
            Caller::Other
        }
    })
}

/// How `asked`'s log is written in `env`, as [`keeps`] decides for this process's own streams.
fn kept(asked: &Asked, env: &Env) -> Keeps {
    keeps(role(asked, env).is_some(), callers())
}

/// This process's stdout and stderr, each a descriptor of its own, close-on-exec: where the streams
/// of a run's command are copied on to, as they went before. None when either is closed.
fn originals() -> io::Result<[fs::File; 2]> {
    use std::os::fd::FromRawFd as _;
    let copy = |fd: libc::c_int| {
        // SAFETY: copies a descriptor of this process's, to one above the standard streams.
        let copied = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
        if copied == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: a new descriptor nothing else owns.
        Ok(unsafe { fs::File::from_raw_fd(copied) })
    };
    Ok([copy(1)?, copy(2)?])
}

/// `asked`'s command run as this process's child, recorded while it runs. In a caller's process
/// (`process` false), in the `env` it was handed; as the `tasks` binary, in its own environment,
/// passing on the signals it gets, then ending as its command did.
fn child(asked: &Asked, env: &Env, err: &mut dyn Write, process: bool) -> i32 {
    let queued_at = crate::store::now();
    let waits = Waits::of(env);
    // A run's log, kept as [`keeps`] says from this process's own streams, both of which it can
    // copy.
    let keeps = kept(asked, env);
    let originals = (keeps != Keeps::Nothing)
        .then(originals)
        .and_then(Result::ok);
    // Its lease, and a run's tree and whether its checkout is clean, and its log, before its command
    // starts.
    let mut prepared =
        recorded::folder(env)
            .zip(recorded::new_id().ok())
            .and_then(|(folder, id)| {
                let logged = originals.as_ref().map(|_| queued_at);
                Prepared::new(folder, id, (asked, None, logged), env, waits).ok()
            });
    let (mut tee, mut source) = (None, None);
    match prepared
        .as_mut()
        .and_then(Prepared::log_file)
        .zip(originals)
    {
        Some((log, originals)) if keeps == Keeps::Files => {
            // Where its caller's files end before its command can write to them.
            let files = crate::run_logs::Files::mark(originals);
            source = Some(crate::run_logs::Source::files(files, log));
        }
        logged => tee = logged,
    }
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
    // session (its board handle and hc terminal), which its own board commands and hc calls need,
    // and the marks of how the caller was asked for ([`sys::MARKS`]), as on macOS, where this
    // process becomes the command with its caller's whole environment.
    for name in sys::SESSION_VARS.into_iter().chain(sys::MARKS) {
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
    if tee.is_some() {
        command
            .stdout(sys::Stdio::piped())
            .stderr(sys::Stdio::piped());
    }
    // Its caller waits on it: the board's answer, and no other run's end, nor a held lock.
    let begin = |prepared: Option<Prepared>, started: Option<Start>| {
        let recording =
            Recording::begin(prepared?, asked, env, (queued_at, started), false).ok()?;
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
            // A command that never ran wrote nothing to keep.
            if let Some(prepared) = &mut prepared {
                prepared.unlogged();
            }
            if let Some(recording) = begin(prepared, None) {
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
    if let Some((log, [out, err])) = tee
        && let (Some(read_out), Some(read_err)) = (child.stdout.take(), child.stderr.take())
    {
        let streams = vec![
            crate::run_logs::Stream {
                from: fs::File::from(std::os::fd::OwnedFd::from(read_out)),
                to: out,
            },
            crate::run_logs::Stream {
                from: fs::File::from(std::os::fd::OwnedFd::from(read_err)),
                to: err,
            },
        ];
        source = crate::run_logs::Copying::start(streams, log)
            .ok()
            .map(crate::run_logs::Source::Copying);
    }
    let started = (child.id(), crate::store::now());
    if let Some(forwarding) = &forwarding {
        forwarding.child(started.0);
    }
    // Its end is seen by a thread of its own and stamped as it comes, while the board is asked,
    // and left unreaped: its pid stays its own while its signals are passed to it. It is reaped
    // here, once they no longer are.
    let (end, ends) = mpsc::channel();
    let mark = end_mark(env);
    let waiter = std::thread::spawn(move || {
        let seen = ended_unreaped(started.0);
        let _ = end.send((seen, crate::store::now()));
        stamped(mark.as_deref());
    });
    let recording = begin(prepared, Some(started));
    let (seen, at) = loop {
        match ends.recv_timeout(BEAT) {
            Ok(end) => break end,
            Err(RecvTimeoutError::Timeout) => {
                if let Some(recording) = &recording {
                    // It reads its command's CPU only once it reaped it: no reading to say.
                    recording.beat(None);
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
    // Its last output reaches its caller before its status does, and its log holds what its end
    // line says.
    let logged = source.as_ref().map(|source| source.ended(waits.drain()));
    if let (Some(recording), Some(bytes)) = (&recording, logged) {
        recording.logged(bytes);
    }
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

/// The file a run in `env` makes once its command's end is stamped ([`END_MARK_VAR`]), a relative
/// name read from the run's folder; none where it names none.
fn end_mark(env: &Env) -> Option<PathBuf> {
    (env.var(END_MARK_VAR))
        .filter(|named| !named.is_empty())
        .map(|named| env.cwd.join(named))
}

/// Makes `mark` ([`end_mark`]), the end of a run's command having been stamped, where no file is.
fn stamped(mark: Option<&Path>) {
    if let Some(mark) = mark {
        let _ = OpenOptions::new().write(true).create_new(true).open(mark);
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

/// The session and task a run at `place` is for, as the board `env` reaches says in one read,
/// without starting its server, waiting at most `wait` for each answer; none when it does not
/// answer. Whether a session is live is judged by the clock every board command reads (`TASKS_NOW`
/// pins it), else this machine's.
fn on_board(
    env: &Env,
    place: &crate::store::RunPlace,
    wait: Duration,
) -> Option<(Option<String>, Option<String>)> {
    let (reached, clock) = reached(env, wait)?;
    let store = super::open_unchecked(reached, &clock).ok()?;
    store.run_for(place).ok()
}

/// The board `env` reaches, its server never started, each of its answers waited for `wait` at
/// most, and the clock every board command reads; none when it is not there.
fn reached(env: &Env, wait: Duration) -> Option<(super::Reached, super::Clock)> {
    let reached = super::reach(env, &env.cwd, false).ok()?;
    let reached = super::Reached {
        client: reached.client.waiting(wait),
        ..reached
    };
    Some((
        reached,
        super::Clock::of(env).unwrap_or(super::Clock::Machine),
    ))
}

/// The session a run's record names, which its recorder says is there while the run goes on: an
/// agent waiting on its own build or test makes no tool call, so nothing else beats for it, and
/// it would read stale, then lost, with its work at the machine's mercy (docs/workflow/TASKS.md, "Health").
/// The board is told every [`SEEN`] and once more as the run ends, so the session ages from that
/// end as from any last sign of life. The session told of is the one the record names, however
/// the run was attributed as it began ([`attributed`]): never looked up by where the run works
/// again while it goes on, so no other session placed in its checkout is told of. A recorder that
/// died says nothing more: its session ages from the last time it did.
struct Session {
    env: Env,
    handle: String,
    /// How long the board is given to answer each time.
    wait: Duration,
    /// When the board was last told, or the run began: its start is its session's own tool call,
    /// which the beat hook said.
    last: Mutex<std::time::Instant>,
}

impl Session {
    /// Whether the board is to be told at `now`: [`SEEN`] after the last time, or the run `ended`.
    /// A time it is told is the last one, whether or not the board answers: a board that does not
    /// is asked again at the next, never at every beat.
    fn due(&self, now: std::time::Instant, ended: bool) -> bool {
        let mut last = self.last.lock().recover();
        if !ended && now.saturating_duration_since(*last) < SEEN {
            return false;
        }
        *last = now;
        true
    }

    /// The board told the session is there, when that is due ([`Session::due`]), by one try,
    /// opened as the beat hook opens it for its one write (its schema read only past the guard): a
    /// board that is down, slow, moving under it or that does not know the session (or knows it
    /// left) is told nothing, and the record goes on.
    fn seen(&self, ended: bool) {
        if !self.due(std::time::Instant::now(), ended) {
            return;
        }
        if let Some((reached, clock)) = reached(&self.env, self.wait)
            && let Ok(store) = super::open_beating(reached, &clock)
        {
            let _ = store.seen_once(&self.handle);
        }
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
/// a step, the commit, its tree and whether the checkout was clean then, and its log, made.
struct Prepared {
    folder: PathBuf,
    id: String,
    lease: Lease,
    commit: Option<String>,
    /// Its role, the tree and whether the checkout differed from its commit: none for an operation
    /// of the work folder alone.
    run: Option<(Role, Option<String>, Option<bool>)>,
    /// The day files' folder its lines go to, when it has one.
    runs: Option<PathBuf>,
    /// A run's or a step's log, by its name in `runs`, and its file until its copy takes it
    /// ([`crate::run_logs`]).
    log: Option<(String, Option<fs::File>)>,
    /// How long it waits on the board's answer, as on git's.
    waits: Waits,
}

impl Prepared {
    /// The lease of the operation `id` of `asked` in `env` taken in the work folder `folder`, never
    /// waited for longer than [`crate::runs::BRIEF_WAIT`] (a new file only a sweep's look holds, for
    /// an instant), naming the process its command is when that is known; then a run's or a step's
    /// tree and cleanliness asked of git for as long as its `waits` say ([`GIT_WAIT`]), and, when
    /// its command's streams can be copied (`queued_at`, the time it was asked for), its log made in
    /// its day files' folder.
    fn new(
        folder: PathBuf,
        id: String,
        (asked, command, queued_at): (&Asked, Option<u32>, Option<f64>),
        env: &Env,
        waits: Waits,
    ) -> io::Result<Prepared> {
        let operations = folder.join(recorded::OPERATIONS);
        fs::create_dir_all(&operations)?;
        let path = operations.join(format!("{id}.lease"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        let mut lease = Lease { lock: None, path };
        lease.lock = crate::runs::lock_within(file, crate::runs::BRIEF_WAIT)?;
        let Some(FileLock(file)) = &lease.lock else {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        };
        if let Some(pid) = command {
            (&*file).write_all(format!("{pid}\n").as_bytes())?;
        }
        let commit = asked.commit.clone().or_else(|| commit(&env.cwd));
        let run = role(asked, env).map(|role| {
            let (tree, dirty) = commit.as_deref().map_or((None, None), |commit| {
                crate::runs::tree_and_dirty(&env.cwd, commit, asked.commit.is_some(), waits.git())
            });
            (role, tree, dirty)
        });
        let runs = crate::runs::folder(env).map(|runs| env.cwd.join(runs));
        let log = match (&run, &runs, queued_at) {
            (Some(_), Some(runs), Some(at)) => {
                let name = crate::run_logs::name(&id, at);
                crate::run_logs::create(runs, &name)
                    .ok()
                    .map(|file| (name, Some(file)))
            }
            _ => None,
        };
        Ok(Prepared {
            folder,
            id,
            lease,
            commit,
            run,
            runs,
            log,
            waits,
        })
    }

    /// Its log's file, for its copy to write: none for an operation that keeps no log.
    fn log_file(&mut self) -> Option<fs::File> {
        self.log.as_mut().and_then(|(_, file)| file.take())
    }

    /// Its log no longer kept, its file removed: its command's streams are not copied after all.
    fn unlogged(&mut self) {
        if let (Some((name, _)), Some(runs)) = (self.log.take(), &self.runs) {
            let _ = fs::remove_file(runs.join(name));
        }
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
    /// The session its record names, which it says is there while it runs: none for an operation
    /// that names none, and for one inside a recorded command, whose recorder says it.
    session: Option<Session>,
}

impl Recording {
    /// Its record written as `prepared` was, as `asked` in `env`, asked for at `queued_at`: running
    /// since `started` when its command started, else queued. The task it is for is asked of the
    /// board first, for as long as its waits say ([`WAIT`]), then the run it repeats read (by a
    /// recorder `apart` from its command's caller, once each earlier run that may be that one
    /// ended, waiting [`RERUN_WAIT`] at most); the times were stamped before. A run or a step that
    /// started appends its start line.
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
            runs,
            log,
            waits,
        } = prepared;
        let (handle, task) = attributed(asked, env, waits.board());
        let branch = asked.branch.clone().or_else(|| {
            crate::git::checkout(&env.cwd)
                .map(|(_, branch)| branch)
                .filter(|branch| !branch.is_empty())
        });
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
                        settled(&folder, &id, same, queued_at, waits.rerun());
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
                log: log.map(|(name, _)| name),
                log_bytes: None,
                refused: None,
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
            reading: None,
        };
        recorded::write(&path, &record)?;
        let session = (record.handle.clone())
            .filter(|_| record.parent_id.is_none())
            .map(|handle| Session {
                env: env.clone(),
                handle,
                wait: waits.board(),
                last: Mutex::new(std::time::Instant::now()),
            });
        let recording = Recording {
            folder,
            runs,
            wait: if apart {
                waits.lock()
            } else {
                crate::runs::BRIEF_WAIT
            },
            path,
            lease,
            record: Mutex::new(record),
            session,
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

    /// The line `record` says appended to the day files, when it is a run's or a step's: an end
    /// line with what its log, whole by then ([`Recording::logged`]), says of its tests.
    fn line(&self, record: &Record) {
        if let (Some(runs), Some(line)) = (&self.runs, crate::runs::line(record)) {
            let line = crate::runs::with_tests(line, runs);
            let _ = crate::runs::append(runs, &line, crate::store::now(), self.wait);
        }
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Record> {
        self.record.lock().recover()
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

    /// Its log holds `bytes`, its command's streams having ended or been waited for: what its end
    /// line says.
    fn logged(&self, bytes: u64) {
        if let Some(run) = &mut self.held().run
            && run.log.is_some()
        {
            run.log_bytes = Some(bytes);
        }
    }

    /// It still runs, its processes as `reading` last read them when there is one; and so the
    /// session it names is there, which the board is told when that is due ([`Session::seen`]).
    fn beat(&self, reading: Option<recorded::CpuReading>) {
        self.update(|record, _| {
            if reading.is_some() {
                record.reading = reading;
            }
        });
        if let Some(session) = &self.session {
            session.seen(false);
        }
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
            // A command that ended red may never have run: its log, whole by now
            // ([`Recording::logged`]), says whether its runner refused its command line. One
            // that timed out or was stopped ran.
            let red = symphony_api::End::of(true, exit_code, signal) == symphony_api::End::Red;
            run.refused = match (&self.runs, &run.log) {
                (Some(runs), Some(log)) if red && signal.is_none() => {
                    crate::runs::refused::read(&runs.join(log))
                }
                _ => None,
            };
        }
        let _ = recorded::keep(&self.folder, &record);
        self.line(&record);
        let _ = recorded::write(&self.path, &record);
        drop(record);
        drop(self.lease);
        // Its session was there until now: it ages from the run's end.
        if let Some(session) = &self.session {
            session.seen(true);
        }
    }
}

/// Once no operation in the work folder `folder` but `ours`, asked for by `before`, may still end as
/// the run `same` repeats (`recorded::pending`), or `within` ([`RERUN_WAIT`], or a test's scale of
/// it) has passed: its end line is then in the day files, when it is to be.
fn settled(folder: &Path, ours: &str, same: crate::runs::Same, before: f64, within: Duration) {
    let deadline = std::time::Instant::now() + within;
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
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: looks again for the recorder's record of a run, which no event announces"
        )]
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The `tasks` process become its command, watched by a recorder it forks first.
#[cfg(target_os = "macos")]
mod supervised {
    use super::{
        Asked, BEAT, Cpu, Keeps, Prepared, Recording, SAMPLE, Waits, cpu_seconds, kept, role,
        unstarted,
    };
    use crate::cli::Env;
    use crate::cli::machine_work::recorded;
    use crate::lock::Recovered as _;
    use crate::run_logs::{Copying, Files, Source, Stream};
    use std::ffi::CString;
    use std::io::{self, Write};
    use std::os::fd::{AsRawFd, FromRawFd as _, OwnedFd};
    use std::os::unix::ffi::OsStrExt as _;
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// What the recorder's watch saw of its command, when it saw it, and the most CPU it read the
    /// command's processes had taken by then.
    type Heard = io::Result<(Seen, f64, Option<Cpu>)>;

    /// The watch's latest reading of the command's processes, which each beat writes into the
    /// running record.
    type Latest = Arc<Mutex<Option<recorded::CpuReading>>>;

    /// Become `asked`'s command, recorded by a recorder that watches this process, in `env`; what
    /// returns is the code of a command that could not start, said on `err`.
    pub(super) fn exec(asked: &Asked, env: &Env, err: &mut dyn Write) -> i32 {
        let queued_at = crate::store::now();
        let waits = Waits::of(env);
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
        // A run's log, kept as [`super::keeps`] says from this process's own streams: through pipes
        // its recorder copies on, once it says it does, or from its caller's files.
        let tee = Tee::new(kept(asked, env));
        if let Ok(id) = recorded::new_id()
            && let Some(teed) = watched(asked, env, (&id, queued_at), waits, tee.as_ref())
        {
            if teed && let Some(tee) = &tee {
                tee.redirect();
            }
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

    /// Copies of where a run's command's stdout and stderr went, made before its recorder is
    /// forked, and, where they are copied as they come, the pipes the command writes them to
    /// instead: the recorder keeps the copies and the pipes' reading ends, and copies each stream on
    /// and into the run's log, or reads its caller's files once the command ended.
    struct Tee {
        /// Each pipe's reading end and writing end, close-on-exec; none where the command keeps
        /// its caller's files.
        pipes: Option<[(OwnedFd, OwnedFd); 2]>,
        originals: [std::fs::File; 2],
    }

    impl Tee {
        /// The copies of this process's stdout and stderr, and the pipes where `keeps` says;
        /// none for no log, or when either stream is closed.
        fn new(keeps: Keeps) -> Option<Tee> {
            let pipes = match keeps {
                Keeps::Nothing => return None,
                Keeps::Pipes => Some([pipe().ok()?, pipe().ok()?]),
                Keeps::Files => None,
            };
            Some(Tee {
                pipes,
                originals: super::originals().ok()?,
            })
        }

        /// The descriptors its recorder keeps: the originals and the pipes' reading ends.
        fn kept(&self) -> Vec<libc::c_int> {
            let originals = self.originals.iter().map(AsRawFd::as_raw_fd);
            let pipes = self
                .pipes
                .iter()
                .flatten()
                .map(|(read, _)| read.as_raw_fd());
            originals.chain(pipes).collect()
        }

        /// This process's stdout and stderr made the pipes' writing ends, if it has pipes: what
        /// its command execs with, the ends themselves closing at that exec.
        fn redirect(&self) {
            for (pipe, stream) in self.pipes.iter().flatten().zip([1, 2]) {
                // SAFETY: points a standard stream at a pipe's end this process holds open.
                unsafe { libc::dup2(pipe.1.as_raw_fd(), stream) };
            }
        }

        /// In its recorder, where `log` is written from, on the descriptors it kept: owned by the
        /// files made of them, since the forked copy of the caller's memory that names them here
        /// is never dropped in a process that only ever `_exit`s. The caller's files are marked
        /// now, before the command hears it may exec.
        fn source(&self, log: std::fs::File) -> io::Result<Source> {
            // SAFETY: a descriptor the recorder kept open (`detach`), which nothing else here
            // closes.
            let adopt = |fd: &dyn AsRawFd| unsafe { std::fs::File::from_raw_fd(fd.as_raw_fd()) };
            let [out, err] = &self.originals;
            match &self.pipes {
                Some([read_out, read_err]) => {
                    let streams = vec![
                        Stream {
                            from: adopt(&read_out.0),
                            to: adopt(out),
                        },
                        Stream {
                            from: adopt(&read_err.0),
                            to: adopt(err),
                        },
                    ];
                    Copying::start(streams, log).map(Source::Copying)
                }
                None => Ok(Source::files(Files::mark([adopt(out), adopt(err)]), log)),
            }
        }
    }

    /// Whether a recorder forked now watches this process, as the operation `id` asked for at
    /// `queued_at`, and whether it copies `tee`'s streams: told on a pipe of their own, within as
    /// long as its `waits` say ([`super::READY`]); none when it does not watch it.
    fn watched(
        asked: &Asked,
        env: &Env,
        (id, queued_at): (&str, f64),
        waits: Waits,
        tee: Option<&Tee>,
    ) -> Option<bool> {
        let (told, tell) = pipe().ok()?;
        // SAFETY: getpid and getppid cannot fail.
        let (target, caller) = unsafe { (libc::getpid(), libc::getppid()) };
        // SAFETY: this process runs one thread, so its child may run what follows.
        match unsafe { libc::fork() } {
            -1 => None,
            0 => {
                drop(told);
                // SAFETY: as above: the grandchild records, and its parent ends at once, so the
                // command has no child of the recorder's to wait for.
                if unsafe { libc::fork() } == 0 {
                    let place = (target, caller);
                    record(place, &tell, (asked, env), (id, queued_at), waits, tee);
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
                match ready(&told, waits.ready()) {
                    Some(WATCHED) => Some(false),
                    Some(TEED) => Some(tee.is_some_and(|tee| tee.pipes.is_some())),
                    _ => None,
                }
            }
        }
    }

    /// What a recorder says once it watches its command, and copies none of its streams.
    const WATCHED: u8 = b'1';
    /// What it says once it watches its command and copies its streams.
    const TEED: u8 = b'2';

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

    /// What the recorder said on `told`, `within` ([`super::READY`], or a test's scale of it), once
    /// it watches this process ([`WATCHED`] or [`TEED`]); none when it said nothing by then.
    fn ready(told: &OwnedFd, within: Duration) -> Option<u8> {
        let deadline = Instant::now() + within;
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
                _ => return None,
            }
        }
        let mut byte = 0_u8;
        // SAFETY: reads at most one byte into it.
        (unsafe { libc::read(told.as_raw_fd(), (&raw mut byte).cast(), 1) } == 1).then_some(byte)
    }

    /// The recorder: out of the caller's session, streams and folder, it watches `target`, takes the
    /// operation's lease, reads a run's tree and cleanliness and, given `tee`, makes its log and
    /// starts copying its streams or marks its caller's files; and says so on `tell` when it has a
    /// work folder to record it in ([`TEED`] when it copies streams, else [`WATCHED`]). Then it
    /// records the operation `id` asked for at `queued_at` until `target` ends, waiting on git and
    /// the board as its `waits` say, and ends once the streams it copies ended. Told, the command
    /// execs: that is its start.
    fn record(
        (target, caller): (libc::pid_t, libc::pid_t),
        tell: &OwnedFd,
        (asked, env): (&Asked, &Env),
        (id, queued_at): (&str, f64),
        waits: Waits,
        tee: Option<&Tee>,
    ) -> ! {
        let watch = Watch::new(target);
        // SAFETY: a session of its own: no signal for the caller's group or terminal reaches it.
        unsafe { libc::setsid() };
        let mut keep = vec![
            watch.as_ref().map_or(-1, |watch| watch.0.as_raw_fd()),
            tell.as_raw_fd(),
        ];
        keep.extend(tee.map(Tee::kept).into_iter().flatten());
        detach(&keep);
        leave(&env.cwd);
        let mut prepared = watch
            .as_ref()
            .ok()
            .and_then(|_| recorded::folder(env))
            .and_then(|folder| {
                let command = Some(target.unsigned_abs());
                let logged = tee.map(|_| queued_at);
                Prepared::new(folder, id.to_owned(), (asked, command, logged), env, waits).ok()
            });
        // Its copy starts, or its caller's files are marked, before its command hears it is kept:
        // a copy that cannot start keeps none.
        let source = tee
            .zip(prepared.as_mut().and_then(Prepared::log_file))
            .and_then(|(tee, log)| tee.source(log).ok());
        if let (None, Some(prepared)) = (&source, &mut prepared) {
            prepared.unlogged();
        }
        let start = (target.unsigned_abs(), crate::store::now());
        let said = if matches!(source, Some(Source::Copying(_))) {
            TEED
        } else {
            WATCHED
        };
        let told = prepared.is_some()
            // SAFETY: writes one byte from it; a command gone on without it hears nothing.
            && unsafe { libc::write(tell.as_raw_fd(), (&raw const said).cast(), 1) } == 1;
        // The command waited for no more than that.
        match (told, watch, prepared) {
            (true, Ok(watch), Some(prepared)) => {
                // Read on a thread of its own, so what it sees is stamped as it comes while the
                // board is asked which task this is for.
                let (tell, heard) = mpsc::channel();
                let latest = Latest::default();
                let read = Arc::clone(&latest);
                let mark = super::end_mark(env);
                let ending = source.as_ref().and_then(Source::ending);
                std::thread::spawn(move || {
                    let place = (target, caller);
                    watching(&watch, &tell, place, (&read, ending), mark.as_deref());
                });
                let begun = Recording::begin(prepared, asked, env, (queued_at, None), true);
                if let Ok(recording) = begun {
                    recording.sweep();
                    let drain = source.as_ref().map(|source| (source, waits.drain()));
                    follow(&heard, recording, start, &latest, drain);
                }
            }
            // Gone on unrecorded, its command keeps its own streams: nothing to keep.
            (_, _, Some(mut prepared)) => prepared.unlogged(),
            _ => {}
        }
        // What its command and the processes it left write is copied on to their end.
        if let Some(source) = source {
            source.join();
        }
        // SAFETY: ends the recorder without the runtime's exit, which is the caller's.
        unsafe { libc::_exit(0) }
    }

    /// What `watch` sees of `target`, which `caller` started, sent on `tell` with the time it saw it
    /// and the most CPU it read `target`'s processes had taken, until its process ends, the watch
    /// fails, or nothing listens. That is read every [`SAMPLE`] and as soon as each event comes,
    /// its exit's first: a process that ended is read until its parent reaps it, and a last reading
    /// that comes after misses what ran since the one before, which the tree weighs
    /// ([`cpu::Tree::reaped_first`]). Each reading the root answered is also left in `latest`.
    /// Once the exit is told, what the caller's files gained is copied into the run's log, given
    /// their `ending`, before anything slower its recorder does: its caller may write to them
    /// again as soon as it has the status. Then `mark` is made ([`super::END_MARK_VAR`]).
    fn watching(
        watch: &Watch,
        tell: &mpsc::Sender<Heard>,
        (target, caller): (libc::pid_t, libc::pid_t),
        (latest, ending): (&Latest, Option<Arc<crate::run_logs::Ending>>),
        mark: Option<&std::path::Path>,
    ) {
        let mut tree = cpu::Tree::new(cpu::Machine, target).reaped_by(caller);
        let mut most = None;
        loop {
            let next = watch.next(SAMPLE);
            let at = Instant::now();
            let read = tree.read(at);
            most = most.max(read);
            if let Some(reading) = read.and(most).and_then(|ticks| reading(&tree, ticks)) {
                *latest.lock().recover() = Some(reading);
            }
            match next {
                Ok(Some(seen)) => {
                    let exited = seen.exit.is_some();
                    if let (true, None, Some(counted)) = (exited, read, most) {
                        tree.reaped_first(at, counted);
                    }
                    let cpu = most.map(|ticks| Cpu {
                        seconds: cpu::seconds(ticks, tree.tick),
                        partial: Some(tree.partial),
                    });
                    let told = tell.send(Ok((seen, crate::store::now(), cpu)));
                    if exited {
                        if let Some(ending) = &ending {
                            ending.copy();
                        }
                        super::stamped(mark);
                    }
                    if told.is_err() || exited {
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

    /// What `tree` read last, its readings having found `most` ticks at most: none for CPU no
    /// clock says.
    pub(super) fn reading<K: cpu::Kernel>(
        tree: &cpu::Tree<K>,
        most: u64,
    ) -> Option<recorded::CpuReading> {
        Some(recorded::CpuReading {
            at: crate::store::now(),
            cpu_s: cpu_seconds(cpu::seconds(most, tree.tick))?,
            partial: tree.partial,
            processes: u32::try_from(tree.processes()).unwrap_or(u32::MAX),
        })
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

    /// Out of its caller's folder, to the root: the recorder writes nothing there, and names what
    /// it reads there from `cwd`, the folder its command runs in, so whoever asks which processes
    /// work in a checkout (a release run's look after each check, the worktree sweep, a land)
    /// never counts a recorder still writing down how its command ended. It stays where it is
    /// when `cwd` is no whole path, as it then has nothing else to name that folder by.
    fn leave(cwd: &std::path::Path) {
        if cwd.is_absolute() {
            let _ = std::env::set_current_dir("/");
        }
    }

    /// Record its command as its watch was `heard`: started at `start` once it exec'd, said again
    /// every [`BEAT`] while it runs with the `latest` reading of its processes, finished with its
    /// status when its exit was seen, and with what its log holds once its `source` ended
    /// ([`Source::ended`], within the time given).
    fn follow(
        heard: &mpsc::Receiver<Heard>,
        recording: Recording,
        start: super::Start,
        latest: &Latest,
        source: Option<(&Source, Duration)>,
    ) {
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
                        if let Some((source, within)) = source {
                            recording.logged(source.ended(within));
                        }
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
                        recording.beat(*latest.lock().recover());
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
        use std::time::{Duration, Instant};

        /// The most processes one reading visits.
        const MOST: usize = 4096;
        /// The most children of one process read.
        const CHILDREN: usize = 512;
        /// The CPU a count may miss and still be whole: this much, or [`SHARE`] of the count.
        const LOSS: Duration = Duration::from_millis(10);
        /// The part of its count a count may miss and still be whole: one hundredth.
        const SHARE: u64 = 100;
        /// A tree that took less than this part of one CPU over its last reading's span (one
        /// hundredth) was idle then.
        const IDLE: u64 = 100;

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
            /// The CPU of the children `pid` reaped, in ticks: each one's own with that of the
            /// children it reaped in turn, added as `pid` reaps it.
            fn reaped(&self, pid: libc::pid_t) -> Option<u64>;
            /// The processes whose parent `pid` is now, at most `room` of them.
            fn children(&self, pid: libc::pid_t, room: usize) -> Vec<libc::pid_t>;
            /// Whether `pid` is a process still.
            fn alive(&self, pid: libc::pid_t) -> bool;
        }

        /// This machine's kernel.
        pub(in super::super) struct Machine;

        /// The kernel's account of `pid`, or why it gave none.
        fn rusage(pid: libc::pid_t) -> Result<libc::rusage_info_v2, Unread> {
            // SAFETY: plain data, for which all zeroes is a value.
            let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
            // SAFETY: writes one rusage_info_v2, the size of the flavor asked for, into `info`.
            let read =
                unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, (&raw mut info).cast()) };
            if read != 0 {
                let gone = std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
                return Err(if gone { Unread::Gone } else { Unread::Denied });
            }
            Ok(info)
        }

        impl Kernel for Machine {
            fn usage(&self, pid: libc::pid_t) -> Result<u64, Unread> {
                let info = rusage(pid)?;
                Ok([
                    info.ri_user_time,
                    info.ri_system_time,
                    info.ri_child_user_time,
                    info.ri_child_system_time,
                ]
                .into_iter()
                .fold(0, u64::saturating_add))
            }

            fn reaped(&self, pid: libc::pid_t) -> Option<u64> {
                let info = rusage(pid).ok()?;
                Some(
                    info.ri_child_user_time
                        .saturating_add(info.ri_child_system_time),
                )
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

        /// One reading the root answered: when it began, what it read, and what the root's caller
        /// had reaped just before.
        #[derive(Clone, Copy, Debug)]
        struct Reading {
            at: Instant,
            ticks: u64,
            caller: Option<u64>,
        }

        /// A command's processes, read again and again: what each reading found, and whether one
        /// knew it left out CPU they took (`partial`): a process under the command it could not
        /// read, more children of one process than [`CHILDREN`] or more processes than [`MOST`],
        /// or one read under it before that still runs but no longer under it (its parent ended
        /// first, and it went to the system's), whose time from then on no reading sees; or, when
        /// the root's caller reaped it before it was read again, more CPU than a count may miss
        /// that it may have taken since ([`Tree::reaped_first`]). One that starts and leaves
        /// between two readings is not seen at all.
        pub(in super::super) struct Tree<K> {
            kernel: K,
            root: libc::pid_t,
            /// The process that started the root and reaps it.
            caller: Option<libc::pid_t>,
            most: usize,
            room: usize,
            /// The processes under the root the last reading read.
            under: BTreeSet<libc::pid_t>,
            /// The last reading the root answered, and the one before it.
            last: Option<Reading>,
            before: Option<Reading>,
            pub(in super::super) partial: bool,
            /// Nanoseconds a tick is.
            pub(in super::super) tick: f64,
            /// The CPUs the machine runs at once.
            pub(in super::super) cpus: u64,
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
                    caller: None,
                    most,
                    room,
                    under: BTreeSet::new(),
                    last: None,
                    before: None,
                    partial: false,
                    tick: tick_ns(),
                    cpus: std::thread::available_parallelism()
                        .map_or(1, |cpus| u64::try_from(cpus.get()).unwrap_or(u64::MAX)),
                }
            }

            /// Its root started, and to be reaped, by `caller`.
            pub(in super::super) fn reaped_by(mut self, caller: libc::pid_t) -> Tree<K> {
                self.caller = Some(caller);
                self
            }

            /// `span` in ticks.
            fn ticks(&self, span: Duration) -> u64 {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    clippy::cast_precision_loss,
                    reason = "a span of seconds in ticks, far inside a u64; a tick is positive"
                )]
                let ticks = (span.as_nanos() as f64 / self.tick) as u64;
                ticks
            }

            /// The CPU the root and every process under it have taken so far, read at `at`, none
            /// once the root cannot be read. Each is read before its children are listed, so a
            /// child its parent reaps meanwhile is counted once, in the one or the other, or missed
            /// until the next reading: never twice. What the root's caller has reaped is read
            /// first, so it holds none of the root's own while the root answers after it.
            pub(in super::super) fn read(&mut self, at: Instant) -> Option<u64> {
                let caller = self.caller.and_then(|pid| self.kernel.reaped(pid));
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
                self.before = self.last.replace(Reading {
                    at,
                    ticks: total,
                    caller,
                });
                Some(total)
            }

            /// The root ended and its caller reaped it before it was read again, at `at`, its
            /// readings having `counted` ticks at most: partial when it may have taken more since
            /// the last reading than a count may miss ([`LOSS`], or [`SHARE`] of `counted`).
            /// - A process read under it then that still runs left the count with it: partial.
            /// - What its caller reaped since that reading holds all the root took, and of other
            ///   children only more: grown by no more than `counted` and what a count may miss, it
            ///   says the count is whole.
            /// - Else a tree idle over the span of its last two readings ([`IDLE`]), ended within
            ///   twice that span of the last, is taken to have stayed idle.
            /// - Else it may have kept every CPU busy since: partial when that is more than a count
            ///   may miss.
            pub(in super::super) fn reaped_first(&mut self, at: Instant, counted: u64) {
                if self.under.iter().any(|pid| self.kernel.alive(*pid)) {
                    self.partial = true;
                    return;
                }
                let Some(last) = self.last else {
                    self.partial = true;
                    return;
                };
                let allowed = self.ticks(LOSS).max(counted / SHARE);
                let reaped = last
                    .caller
                    .zip(self.caller.and_then(|pid| self.kernel.reaped(pid)))
                    .and_then(|(then, now)| now.checked_sub(then));
                if reaped.is_some_and(|reaped| {
                    reaped
                        .checked_sub(counted)
                        .is_some_and(|more| more <= allowed)
                }) {
                    return;
                }
                let since = at.saturating_duration_since(last.at);
                let idle = self.before.is_some_and(|before| {
                    let span = last.at.saturating_duration_since(before.at);
                    since <= span.saturating_mul(2)
                        && last
                            .ticks
                            .checked_sub(before.ticks)
                            .is_some_and(|grew| grew.saturating_mul(IDLE) <= self.ticks(span))
                });
                if idle {
                    return;
                }
                if self.ticks(since).saturating_mul(self.cpus) > allowed {
                    self.partial = true;
                }
            }

            /// How many processes the last reading the root answered read: the root and each one
            /// under it.
            pub(in super::super) fn processes(&self) -> usize {
                self.under.len() + 1
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
    fn a_runs_session_is_said_there_every_so_often_and_once_more_as_the_run_ends() {
        let began = std::time::Instant::now();
        let session = Session {
            env: Env {
                vars: Default::default(),
                cwd: PathBuf::new(),
            },
            handle: "amber-heron".into(),
            wait: WAIT,
            last: Mutex::new(began),
        };
        let short = SEEN - Duration::from_millis(1);
        // Its start is its session's own call: nothing is said until a while after it.
        assert!(!session.due(began, false));
        assert!(!session.due(began + short, false));
        assert!(session.due(began + SEEN, false));
        // Said once: the next time is that long after, whether or not the board answered.
        assert!(!session.due(began + SEEN, false));
        assert!(!session.due(began + SEEN + short, false));
        assert!(session.due(began + SEEN * 2 + BEAT, false));
        // Its end is said whenever it comes, and a beat read late after it says nothing more.
        assert!(session.due(began + SEEN * 2 + BEAT * 2, true));
        assert!(!session.due(began + SEEN * 2, false));
        assert!(session.due(began + SEEN * 2 + BEAT * 2, true));
        // Three of its times fit inside the time a session reads live after one.
        assert!(SEEN.as_secs_f64() * 3.0 <= crate::health::LIVE_SECONDS);
    }

    #[test]
    fn a_runs_log_is_kept_from_pipes_or_its_callers_files_and_never_at_a_terminal() {
        use Caller::{File, Other, Terminal};
        // Anything piped, `/dev/null` or closed: copied as it comes.
        assert_eq!(keeps(true, [Other, Other]), Keeps::Pipes);
        // An agent's tool call, the release run's steps, a detached board command: a regular file
        // on either stream, which the command keeps and its log is copied from at its end.
        for files in [[File, File], [File, Other], [Other, File]] {
            assert_eq!(keeps(true, files), Keeps::Files, "{files:?}");
        }
        // A person at a terminal, on either stream, whatever the other is: the command keeps the
        // caller's own and the run no log.
        for streams in [
            [Terminal, Other],
            [Other, Terminal],
            [Terminal, Terminal],
            [Terminal, File],
            [File, Terminal],
        ] {
            assert_eq!(keeps(true, streams), Keeps::Nothing, "{streams:?}");
        }
        // An operation that is no run keeps its caller's streams whatever they are.
        for streams in [[Other, Other], [File, File], [Terminal, Terminal]] {
            assert_eq!(keeps(false, streams), Keeps::Nothing, "{streams:?}");
        }
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
        assert!(super::super::usage::top().contains("\n  run "));
    }

    #[test]
    fn the_help_describes_every_option_the_usage_names_and_each_probe_version() {
        let help = help();
        let options = help.split_once("\noptions:\n").unwrap().1;
        let described: Vec<&str> = options
            .lines()
            .filter_map(|line| line.trim_start().split([' ', ',']).next())
            .filter(|word| word.starts_with("--"))
            .collect();
        let named: Vec<&str> = USAGE
            .split(|c: char| c.is_whitespace() || c == '[' || c == ']')
            .filter(|word| word.starts_with("--") && *word != "--")
            .collect();
        for flag in [
            "--phase",
            "--pipeline",
            "--step",
            "--commit",
            "--branch",
            "--probe",
        ] {
            assert!(
                named.contains(&flag),
                "{flag} is not in the usage: {named:?}"
            );
        }
        for flag in &named {
            assert!(described.contains(flag), "{flag} is not described: {help}");
        }
        // Each version the probe prints is named whole, not the two lines run together.
        for version in PROBE.lines() {
            assert!(options.contains(&format!("`{version}`")), "{help}");
        }
        assert!(!help.contains(&format!("`{PROBE}`")), "{help}");
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
    fn a_runs_end_mark_is_named_only_when_set_from_its_folder_and_never_replaces_a_file() {
        assert_eq!(end_mark(&env(&[])), None);
        assert_eq!(end_mark(&env(&[(END_MARK_VAR, "")])), None);
        let dir =
            std::env::temp_dir().join(format!("symphony-run-end-mark-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let in_dir = |named: &str| Env {
            cwd: dir.clone(),
            ..env(&[(END_MARK_VAR, named)])
        };
        let mark = end_mark(&in_dir("stamped")).unwrap();
        assert_eq!(
            mark,
            dir.join("stamped"),
            "a relative name is the run's folder's"
        );
        let kept = dir.join("kept");
        assert_eq!(
            end_mark(&in_dir(kept.to_str().unwrap())),
            Some(kept.clone())
        );
        stamped(None);
        assert!(!mark.exists(), "no mark named, none made");
        stamped(Some(&mark));
        assert_eq!(fs::read(&mark).unwrap(), b"");
        fs::write(&kept, "a file of its own").unwrap();
        stamped(Some(&kept));
        assert_eq!(fs::read_to_string(&kept).unwrap(), "a file of its own");
        fs::remove_dir_all(&dir).unwrap();
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
        // It runs until the kill below, however slowly a loaded machine reads it before.
        let mut shell = burning("exec sleep 600");
        let pid = libc::pid_t::try_from(shell.id()).unwrap();
        let tick = supervised::cpu::tick_ns();
        assert!(tick > 0.0);
        let mut tree = Tree::new(Machine, pid);
        // The shell's child burns, then is reaped: its time is the shell's, read the same.
        test_time::until_true(
            "the shell's tree read more than 0.05 s of CPU",
            test_time::backstop(Duration::from_secs(20)),
            || {
                let read = tree.read(std::time::Instant::now()).unwrap();
                supervised::cpu::seconds(read, tick) > 0.05
            },
        )
        .unwrap();
        assert!(!tree.partial, "a child reaped by its parent is no loss");
        shell.kill().unwrap();
        let _ = shell.wait();
        let gone = tree.read(std::time::Instant::now());
        assert_eq!(gone, None, "a process gone reads none");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_caller_that_reaps_a_command_holds_all_the_cpu_it_took_after() {
        use supervised::cpu::{Kernel as _, Machine, seconds, tick_ns};
        let me = libc::pid_t::try_from(std::process::id()).unwrap();
        let before = Machine.reaped(me).unwrap();
        let child = burning("exit 0");
        let (_, cpu) = reaped(child.id()).unwrap();
        let after = Machine.reaped(me).unwrap();
        // What other tests' children took, reaped meanwhile, only adds to it.
        let grew = seconds(after - before, tick_ns());
        assert!(cpu > 0.05 && grew >= cpu - 0.001, "{grew} < {cpu}");
    }

    /// A process table of the test's own: each pid's parent, its ticks, and whether it may be read;
    /// and the ticks of the children any process reaped, when it says.
    #[cfg(target_os = "macos")]
    #[derive(Default)]
    struct Table(
        std::cell::RefCell<std::collections::BTreeMap<libc::pid_t, (libc::pid_t, u64, bool)>>,
        std::cell::Cell<Option<u64>>,
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

        fn reaped(&self, _: libc::pid_t) -> Option<u64> {
            self.1.get()
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_reading_knows_when_it_misses_a_process_it_may_not_read_too_many_or_one_that_left() {
        use supervised::cpu::Tree;
        let now = std::time::Instant::now();
        // 1 runs 2 and 3; 3 runs 4.
        let whole = [(1, 0, 10), (2, 1, 20), (3, 1, 30), (4, 3, 40)];
        let table = Table::with(&whole);
        let mut tree = Tree::new(&table, 1);
        assert_eq!(tree.read(now), Some(100));
        assert!(!tree.partial);
        // 4 ends and 3 reaps it: its time is 3's now, and nothing is lost.
        table.0.borrow_mut().remove(&4);
        table.0.borrow_mut().insert(3, (1, 70, true));
        assert_eq!(tree.read(now), Some(100));
        assert!(!tree.partial, "a child reaped is no loss");
        // 3 ends unreaped by 1, leaving its child 5 to the system: 5 runs on, unread.
        table.0.borrow_mut().insert(5, (3, 5, true));
        assert_eq!(tree.read(now), Some(105));
        table.0.borrow_mut().remove(&3);
        table.0.borrow_mut().insert(5, (0, 9, true));
        assert_eq!(tree.read(now), Some(30));
        assert!(tree.partial, "a process that left the tree while it runs");
        // One it may not read.
        let table = Table::with(&whole);
        table.0.borrow_mut().insert(2, (1, 20, false));
        let mut tree = Tree::new(&table, 1);
        assert_eq!(tree.read(now), Some(80));
        assert!(tree.partial, "a process it may not read");
        // More children of one than it lists, or more processes than it reads.
        let table = Table::with(&whole);
        let mut tree = Tree::bounded(&table, 1, 4096, 2);
        tree.read(now);
        assert!(tree.partial, "as many children as there is room for");
        let mut tree = Tree::bounded(&table, 1, 3, 512);
        tree.read(now);
        assert!(tree.partial, "more processes than a reading visits");
        let mut tree = Tree::bounded(&table, 1, 4, 3);
        assert_eq!(tree.read(now), Some(100));
        assert!(!tree.partial, "room for each");
        // A root that cannot be read reads none.
        let table = Table::with(&[(1, 0, 10)]);
        table.0.borrow_mut().insert(1, (0, 10, false));
        assert_eq!(Tree::new(&table, 1).read(now), None);
        assert_eq!(Tree::new(&Table::default(), 1).read(now), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_beats_reading_says_the_most_cpu_read_whether_a_reading_missed_some_and_what_it_read() {
        use supervised::cpu::Tree;
        let now = std::time::Instant::now();
        // 1 runs 2, and 2 runs 3; a tick is a millisecond.
        let table = Table::with(&[(1, 0, 10), (2, 1, 20), (3, 2, 30)]);
        let mut tree = Tree::new(&table, 1);
        tree.tick = 1e6;
        let before = crate::store::now();
        let most = tree.read(now).unwrap();
        let reading = supervised::reading(&tree, most).unwrap();
        assert!(reading.at >= before, "{reading:?}");
        assert_eq!(
            (reading.cpu_s, reading.partial, reading.processes),
            (0.06, false, 3)
        );
        // 2 ends unreaped, leaving 3 to the system: the next reading reads the root alone and knows
        // it missed one, and the most read stays.
        table.0.borrow_mut().remove(&2);
        table.0.borrow_mut().insert(3, (0, 35, true));
        let read = tree.read(now).unwrap();
        let reading = supervised::reading(&tree, most.max(read)).unwrap();
        assert_eq!(
            (reading.cpu_s, reading.partial, reading.processes),
            (0.06, true, 1)
        );
        // CPU no clock says is no reading.
        tree.tick = f64::NAN;
        assert_eq!(supervised::reading(&tree, most), None);
    }

    /// Whether a root read at each `(ms, ticks)` of `readings`, then reaped by its caller before
    /// the reading at `end` ms, is partial: its caller having reaped `caller.0` ticks of children
    /// by the readings and `caller.1` after, when that is said; a child read under it still running
    /// once it is reaped when `left`. A tick is a millisecond, and the machine runs four CPUs.
    #[cfg(target_os = "macos")]
    fn reaped_first(
        readings: &[(u64, u64)],
        caller: Option<(u64, u64)>,
        end: u64,
        left: bool,
    ) -> bool {
        use supervised::cpu::Tree;
        let table = Table::default();
        if left {
            table.0.borrow_mut().insert(2, (1, 0, true));
        }
        table.1.set(caller.map(|(by, _)| by));
        let mut tree = Tree::new(&table, 1).reaped_by(9);
        tree.tick = 1e6;
        tree.cpus = 4;
        let began = std::time::Instant::now();
        let mut counted = 0;
        for &(at, ticks) in readings {
            table.0.borrow_mut().insert(1, (0, ticks, true));
            let read = tree.read(began + Duration::from_millis(at)).unwrap();
            counted = counted.max(read);
        }
        table.0.borrow_mut().remove(&1);
        if left {
            table.0.borrow_mut().insert(2, (0, 0, true));
        }
        table.1.set(caller.map(|(_, after)| after));
        let end = began + Duration::from_millis(end);
        assert_eq!(tree.read(end), None);
        tree.reaped_first(end, counted);
        tree.partial
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_command_its_caller_reaped_first_is_partial_only_when_its_readings_may_have_missed_cpu() {
        let (idle, busy) = ([(0, 100), (250, 100)], [(0, 100), (250, 200)]);
        // Idle over its last span, ended within twice that: nothing taken since.
        assert!(!reaped_first(&idle, None, 300, false));
        assert!(!reaped_first(&idle, None, 750, false));
        // Ended later than that, it may have kept four CPUs busy for 550 ms, more than 10 ms.
        assert!(reaped_first(&idle, None, 800, false));
        // Busy, it may have taken 4 x 50 ms since.
        assert!(reaped_first(&busy, None, 300, false));
        // Its caller reaped no more than its count and 10 ms (a hundredth of 200 ms is less).
        assert!(!reaped_first(&busy, Some((1000, 1210)), 300, false));
        // Its caller reaped others too, or not all of it yet: nothing said.
        assert!(reaped_first(&busy, Some((1000, 1211)), 300, false));
        assert!(reaped_first(&busy, Some((1000, 1150)), 300, false));
        assert!(reaped_first(&busy, Some((1000, 900)), 300, false));
        // Ended within 2 ms of its last reading: four CPUs take 8 ms at most.
        assert!(!reaped_first(&busy, None, 252, false));
        // A hundredth of a long count (300 ms) is more than four CPUs take in 50 ms.
        assert!(!reaped_first(
            &[(0, 10_000), (250, 30_000)],
            None,
            300,
            false
        ));
        assert!(reaped_first(
            &[(0, 10_000), (250, 30_000)],
            None,
            330,
            false
        ));
        // One reading only, of a short command: as for a busy one.
        assert!(!reaped_first(&[(0, 5)], None, 2, false));
        assert!(reaped_first(&[(0, 5)], None, 100, false));
        // Two readings a moment apart say nothing of the time since.
        assert!(reaped_first(&[(0, 5), (1, 5)], None, 100, false));
        // A count that fell between its last two readings says nothing of idleness.
        assert!(reaped_first(&[(0, 100), (250, 90)], None, 300, false));
        // A child that outlives it left with it, whatever else is said.
        assert!(reaped_first(&idle, Some((1000, 1100)), 300, true));
        // Nothing read before it ended: nothing counted to be whole.
        assert!(reaped_first(&[], None, 300, false));
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
        let attributed = |asked: &Asked, env: &Env| super::attributed(asked, env, WAIT);
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

    /// A folder of the test's own, removed when it ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!(
                "recorder-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir.canonicalize().unwrap())
        }

        /// A main checkout at `name`, a folder holding `.git`, whose board's server record names
        /// `port` when one is given, as the board's server writes it.
        fn repository(&self, name: &str, port: Option<u16>) -> PathBuf {
            let main = self.0.join(name);
            let board = main.join(".git").join(crate::server::FOLDER);
            fs::create_dir_all(&board).unwrap();
            if let Some(port) = port {
                let record = format!(r#"{{"addr":"127.0.0.1:{port}","pid":1,"version":"3.3.0"}}"#);
                fs::write(board.join("server.json"), record).unwrap();
                // A SurrealDB board's store, which its recorded server serves: a home holding
                // none is a board not made yet, on PostgreSQL.
                fs::create_dir_all(board.join("data")).unwrap();
            }
            main
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_scale_is_a_whole_number_from_one_to_sixty_and_lengthens_each_wait_alike() {
        let at = |scale: &str| {
            Waits::of(&env(&[
                ("TASKS_SERVER", "127.0.0.1:1"),
                ("TASKS_DATABASE", "scratch_waits"),
                (SCALE_VAR, scale),
            ]))
        };
        assert_eq!(Waits::of(&env(&[])), Waits::OWN);
        let own = Waits::OWN;
        #[cfg(target_os = "macos")]
        assert_eq!(own.ready(), READY);
        assert_eq!(
            (own.board(), own.git(), own.rerun(), own.lock()),
            (WAIT, GIT_WAIT, RERUN_WAIT, crate::runs::LOCK_WAIT)
        );
        let scaled = at("20");
        assert_eq!(scaled, Waits(20));
        #[cfg(target_os = "macos")]
        assert_eq!(scaled.ready(), Duration::from_secs(20));
        assert_eq!(
            (scaled.board(), scaled.git(), scaled.rerun(), scaled.lock()),
            (
                Duration::from_secs(20),
                Duration::from_secs(12),
                Duration::from_secs(100),
                Duration::from_secs(40)
            )
        );
        assert_eq!((at("1"), at("60")), (Waits(1), Waits(60)));
        for named in ["0", "61", "-2", "2.5", "twenty", "", " 20", "20 "] {
            assert_eq!(at(named), Waits::OWN, "{named:?}");
        }
    }

    /// A scale is taken only on a scratch board's database: a run that reaches its project's live
    /// board, by the server a `TASKS_SERVER` names or by its folder, keeps its own waits, and so
    /// does one on the live database of a server named from where the board cannot tell whose it
    /// is (the server down, the run in another repository or in none), or one that reaches no
    /// board; the same server's scratch database takes it.
    #[test]
    fn a_scale_is_never_taken_on_a_live_board_or_where_no_board_is_reached() {
        let scratch = Scratch::new("waits");
        let machine = scratch.0.join("machine");
        fs::create_dir_all(&machine).unwrap();
        // Port 1, which nothing listens on, is the one the repository's record names: where the
        // machine cannot say who listens, a server there is that repository's live board.
        let live = scratch.repository("live", Some(1));
        let unserved = scratch.repository("unserved", None);
        let machine = machine.to_str().unwrap();
        let at = |cwd: &Path, vars: &[(&str, &str)]| {
            let mut all = vec![("SYMPHONY_MACHINE_DIR", machine), (SCALE_VAR, "20")];
            all.extend_from_slice(vars);
            Waits::of(&Env {
                cwd: cwd.to_path_buf(),
                ..env(&all)
            })
        };
        let scratch_database = ("TASKS_DATABASE", "scratch_waits");
        // By the server it names: the live board's database, named or not, and a scratch one.
        let named = ("TASKS_SERVER", "127.0.0.1:1");
        assert_eq!(at(&live, &[named]), Waits::OWN);
        assert_eq!(at(&live, &[named, ("TASKS_DATABASE", "tasks")]), Waits::OWN);
        assert_eq!(at(&live, &[named, scratch_database]), Waits(20));
        // The live database of a server no one answers on, named from a repository that records
        // no server, or from no repository: the board takes it for no live board, and still the
        // scale is not taken.
        for cwd in [&unserved, &scratch.0] {
            assert_eq!(at(cwd, &[named]), Waits::OWN, "{}", cwd.display());
            let live_database = ("TASKS_DATABASE", crate::DATABASE);
            assert_eq!(at(cwd, &[named, live_database]), Waits::OWN);
            assert_eq!(at(cwd, &[named, scratch_database]), Waits(20));
        }
        // By its folder: the project's board, and a scratch database on its server.
        assert_eq!(at(&live, &[]), Waits::OWN);
        assert_eq!(at(&live, &[scratch_database]), Waits(20));
        // No board reached: no server recorded, or no repository at all.
        assert_eq!(at(&unserved, &[scratch_database]), Waits::OWN);
        assert_eq!(at(&scratch.0, &[scratch_database]), Waits::OWN);
    }

    /// Signals this test set to the dispositions it relies on, each put back as the test found it
    /// however the test ends: a run started as a background job inherits SIGQUIT ignored, a
    /// `nohup` one SIGHUP.
    struct Set(Vec<(libc::c_int, libc::sigaction)>);

    impl Set {
        fn to(dispositions: &[(libc::c_int, libc::sighandler_t)]) -> Set {
            let found = dispositions
                .iter()
                .map(|&(signal, disposition)| {
                    // SAFETY: sigaction is plain data, for which all zeroes is a value.
                    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
                    action.sa_sigaction = disposition;
                    // SAFETY: sigaction is plain data, for which all zeroes is a value.
                    let mut before: libc::sigaction = unsafe { std::mem::zeroed() };
                    // SAFETY: an action without a handler for a signal nothing in the test binary
                    // sends, the one it replaces read into `before`.
                    let set =
                        unsafe { libc::sigaction(signal, &raw const action, &raw mut before) };
                    assert_eq!(set, 0, "signal {signal}");
                    (signal, before)
                })
                .collect();
            Set(found)
        }
    }

    impl Drop for Set {
        fn drop(&mut self) {
            for (signal, before) in &self.0 {
                // SAFETY: the action this signal had when the test began.
                unsafe { libc::sigaction(*signal, before, std::ptr::null_mut()) };
            }
        }
    }

    #[test]
    fn forwarding_leaves_an_ignored_signal_ignored_and_passes_on_one_left_at_its_default() {
        let (ignored, passed) = (libc::SIGHUP, libc::SIGQUIT);
        let _set = Set::to(&[(ignored, libc::SIG_IGN), (passed, libc::SIG_DFL)]);
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
    }

    #[test]
    fn a_commit_is_read_from_a_loose_or_packed_ref_or_a_detached_head() {
        let dir = std::env::temp_dir().join(format!("symphony-run-commit-{}", std::process::id()));
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
