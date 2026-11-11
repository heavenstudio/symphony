//! `tasks run --phase build|test [--label TEXT] [--task SLUG] [--as HANDLE] -- COMMAND [ARGS...]`:
//! COMMAND run as this process's child, with its caller's arguments, streams, environment, working
//! directory and limits, its exit status this process's own; and recorded in the work folder
//! ([`recorded`]) while it runs: queued, running once it started, said again every [`BEAT`] while
//! it runs, then finished with how it ended, and kept in the history. A build or test it records
//! puts the task it ran for in the board's Build or Test column (docs/MACHINE_WORK.md).
//!
//! The task is explicit or none: `--task`, else the one task the session (`--as`, else
//! `TASKS_SESSION`) holds on the board when it begins (it executes it, or reviews it in review),
//! read without starting the board's server and waiting at most [`WAIT`]; none when it holds none
//! or several, or the board cannot say. A command a recorder runs names its operation in
//! [`recorded::PARENT_VAR`], so a recorder it starts records it as its parent.
//!
//! Nothing it records reaches the command's streams: a record it cannot write leaves the command
//! running as it would unrecorded. It says something only of a command line it cannot read (2) and
//! of a command that cannot start (127, or 126, as a shell says them). As the `tasks` binary it
//! passes the hang-up, interrupt, quit and terminate signals it gets to its command, and ends by
//! the signal that ended its command. `tasks run --probe` answers 0, saying nothing: what a
//! wrapper asks before it hands a build to a `tasks` that may be older than this command.

use super::Env;
use super::machine_work::recorded::{self, Phase, Record, Step};
use crate::sys::{self, FileLock};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

/// The command's name.
pub const COMMAND: &str = "run";
/// How often a running operation's recorder says it still runs: well inside the Symphony's
/// ten seconds before it calls work stale.
pub const BEAT: Duration = Duration::from_secs(2);
/// How long the board is asked which task the session holds.
const WAIT: Duration = Duration::from_secs(1);
const USAGE: &str = "usage: tasks run --phase build|test [--label TEXT] [--task SLUG] [--as HANDLE] -- COMMAND [ARGS...]\n       tasks run --probe\n";

/// A command line read.
#[derive(Debug, PartialEq)]
struct Asked {
    phase: Phase,
    label: Option<String>,
    task: Option<String>,
    handle: Option<String>,
    command: Vec<String>,
}

/// `args` read: none for `--probe`.
fn parse(args: &[String]) -> Result<Option<Asked>, String> {
    if args.len() == 1 && args[0] == "--probe" {
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
        let value = args
            .get(at + 1)
            .filter(|value| value.as_str() != "--")
            .cloned()
            .ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
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

/// `tasks run ARGS` in `env`, saying what it must on `err`. `process` when this is the `tasks`
/// binary's own process, whose signals it passes on and whose end it may be.
pub fn run(args: &[String], env: &Env, err: &mut dyn Write, process: bool) -> i32 {
    let asked = match parse(args) {
        Ok(Some(asked)) => asked,
        Ok(None) => return 0,
        Err(why) => {
            let _ = write!(err, "tasks run: {why}\n{USAGE}");
            return 2;
        }
    };
    let identity = |name: &String| super::machine_work::identity(name);
    let handle = asked
        .handle
        .clone()
        .or_else(|| env.var("TASKS_SESSION").map(str::to_owned))
        .filter(identity);
    let task = match &asked.task {
        Some(task) => Some(task.clone()).filter(identity),
        None => handle.as_deref().and_then(|handle| held_task(env, handle)),
    };
    let recording = recorded::folder(env)
        .and_then(|folder| Recording::begin(folder, &asked, handle, task, env).ok());
    // As the binary, the command starts in this process's own environment and folder, whatever
    // bytes they hold; in a caller's process, in the `env` it was handed.
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
    if let Some(recording) = &recording {
        command.env(recorded::PARENT_VAR, recording.id());
    }
    let forwarding = process.then(signals::Forwarding::install);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            drop(forwarding);
            let code = if error.kind() == io::ErrorKind::NotFound {
                127
            } else {
                126
            };
            let _ = writeln!(err, "tasks run: {}: {error}", asked.command[0]);
            if let Some(recording) = recording {
                recording.finish(
                    Some(code),
                    None,
                    &format!("its command did not start: {error}"),
                );
            }
            return code;
        }
    };
    if let Some(forwarding) = &forwarding {
        forwarding.child(child.id());
    }
    let beating = recording.as_ref().map(|recording| {
        recording.started(child.id());
        recording.beating()
    });
    let ended = child.wait();
    drop(forwarding);
    if let Some(beating) = beating {
        beating.stop();
    }
    let status = match ended {
        Ok(status) => status,
        Err(error) => {
            let _ = writeln!(err, "tasks run: the end of {}: {error}", asked.command[0]);
            if let Some(recording) = recording {
                recording.finish(None, None, &format!("its end could not be read: {error}"));
            }
            return 1;
        }
    };
    if let Some(recording) = recording {
        recording.finish(status.code(), status.signal(), "");
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

/// One operation being recorded: its record, written whole at each change, and its lease, held
/// locked until it finishes.
struct Recording {
    folder: PathBuf,
    path: PathBuf,
    lease: FileLock,
    record: Arc<Mutex<Record>>,
}

impl Recording {
    /// Its lease taken and its record written queued, in `folder`, once the operations whose
    /// recorders are gone are finished.
    fn begin(
        folder: PathBuf,
        asked: &Asked,
        handle: Option<String>,
        task: Option<String>,
        env: &Env,
    ) -> io::Result<Recording> {
        let now = crate::store::now();
        let _ = recorded::sweep(&folder, now);
        let operations = folder.join(recorded::OPERATIONS);
        fs::create_dir_all(&operations)?;
        let id = recorded::new_id()?;
        let path = operations.join(format!("{id}.json"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path.with_extension("lease"))?;
        file.lock()?;
        let lease = FileLock(file);
        let label = asked
            .label
            .clone()
            .unwrap_or_else(|| asked.command.join(" "));
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
            label: recorded::bounded(&label),
            state: Step::Queued,
            handle,
            task,
            checkout: recorded::bounded(&env.cwd.to_string_lossy()),
            branch: branch.map(|branch| recorded::bounded(&branch)),
            commit: commit(&env.cwd),
            machine_id: machine_id(env),
            host: recorded::bounded(&sys::hostname()),
            pid: std::process::id(),
            child_pid: None,
            queued_at: now,
            started_at: None,
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
            record: Arc::new(Mutex::new(record)),
        })
    }

    fn id(&self) -> String {
        self.held().operation_id.clone()
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Record> {
        self.record.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Its record changed by `change`, said now, and written.
    fn update(&self, change: impl FnOnce(&mut Record, f64)) -> Record {
        let mut record = self.held();
        let now = crate::store::now();
        change(&mut record, now);
        record.beat_at = now;
        let _ = recorded::write(&self.path, &record);
        record.clone()
    }

    /// Its command started as `pid`.
    fn started(&self, pid: u32) {
        self.update(|record, now| {
            record.state = Step::Running;
            record.started_at = Some(now);
            record.child_pid = Some(pid);
        });
    }

    /// Say it still runs every [`BEAT`] until stopped.
    fn beating(&self) -> Beating {
        let (stop, stopped) = mpsc::channel::<()>();
        let (path, record) = (self.path.clone(), Arc::clone(&self.record));
        let thread = std::thread::spawn(move || {
            while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(BEAT) {
                let mut record = record.lock().unwrap_or_else(PoisonError::into_inner);
                record.beat_at = crate::store::now();
                let _ = recorded::write(&path, &record);
            }
        });
        Beating { stop, thread }
    }

    /// It ended so (`exit_code` or `signal`, neither when that is not known): its record written
    /// finished and kept in the history, then its lease let go.
    fn finish(self, exit_code: Option<i32>, signal: Option<i32>, reason: &str) {
        let finished = self.update(|record, now| {
            record.state = Step::Finished;
            record.ended_at = Some(now);
            record.exit_code = exit_code;
            record.signal = signal;
            reason.clone_into(&mut record.reason);
        });
        let _ = recorded::keep(&self.folder, &finished);
        drop(self.lease);
        let _ = fs::remove_file(self.path.with_extension("lease"));
    }
}

/// A recording's beat, until it is stopped.
struct Beating {
    stop: mpsc::Sender<()>,
    thread: JoinHandle<()>,
}

impl Beating {
    fn stop(self) {
        drop(self.stop);
        let _ = self.thread.join();
    }
}

/// The signals a recorder passes to its command, and its end by the one that ended it.
mod signals {
    use std::sync::atomic::{AtomicI32, Ordering};

    /// The command's pid once it started, else 0.
    static CHILD: AtomicI32 = AtomicI32::new(0);
    /// A signal that came before the command started, else 0.
    static PENDING: AtomicI32 = AtomicI32::new(0);
    /// What a terminal, a supervisor or a timeout sends to end what it started.
    const FORWARDED: [libc::c_int; 4] = [libc::SIGHUP, libc::SIGINT, libc::SIGQUIT, libc::SIGTERM];

    extern "C" fn forward(signal: libc::c_int) {
        let child = CHILD.load(Ordering::SeqCst);
        if child > 0 {
            // SAFETY: kill is async-signal-safe, and the pid is this process's unreaped child.
            unsafe { libc::kill(child, signal) };
        } else {
            PENDING.store(signal, Ordering::SeqCst);
        }
    }

    /// The forwarding handlers, installed until dropped, when the dispositions before come back.
    pub(super) struct Forwarding {
        before: Vec<(libc::c_int, libc::sighandler_t)>,
    }

    impl Forwarding {
        pub(super) fn install() -> Forwarding {
            CHILD.store(0, Ordering::SeqCst);
            PENDING.store(0, Ordering::SeqCst);
            let handler = forward as extern "C" fn(libc::c_int) as libc::sighandler_t;
            let before = FORWARDED
                .iter()
                // SAFETY: installs a handler that only reads and writes atomics and calls kill.
                .map(|&signal| (signal, unsafe { libc::signal(signal, handler) }))
                .collect();
            Forwarding { before }
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
            for &(signal, handler) in &self.before {
                // SAFETY: puts back the disposition this signal had before install.
                unsafe { libc::signal(signal, handler) };
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

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_string()).collect()
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
