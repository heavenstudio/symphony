//! `tasks run`, the binary itself: the command it wraps keeps its arguments (any bytes), streams,
//! descriptors, ignored signals, signal mask, session and status, and is the process its caller
//! started; its record in a work folder of the test's own names the task its session holds on a
//! board of the test's own (or the one named) while it runs, then how it ended and how long it took.
//! The recorder finishes after its command does, so what it keeps is waited for.

use crate::{Case, git, repository, scratch};
use std::fs;
use std::io::Read as _;
use std::net::TcpListener;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use symphony_api::{End, RunLine};
use symphony_board::cli::Env;
use symphony_board::cli::machine_work::recorded::{self, Phase, Record, Step};
use symphony_board::sys::{Child, Command, Stdio, output, wait_output};

pub(crate) const MACHINE: &str = "0123456789abcdef0123456789abcdef";
const TASK_VAR: &str = "TASKS_TASK";
/// Names the day files' folder of a test's own.
pub(crate) const RUNS_VAR: &str = "SYMPHONY_RUNS";

/// Every line of the day files in `runs`, read once they hold `ends` end lines (and no more), in
/// the order each file holds them: each a whole line.
pub(crate) fn day_lines(runs: &Path, ends: usize) -> Vec<RunLine> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let mut days: Vec<PathBuf> = fs::read_dir(runs)
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
                    .collect()
            })
            .unwrap_or_default();
        days.sort();
        let lines: Vec<RunLine> = days
            .iter()
            .flat_map(|day| {
                let text = fs::read_to_string(day).unwrap();
                assert!(text.is_empty() || text.ends_with('\n'), "{text}");
                text.lines()
                    .map(|line| {
                        serde_json::from_str::<RunLine>(line)
                            .unwrap_or_else(|error| panic!("{error}: {line}"))
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let ended = lines.iter().filter(|line| line.end.is_some()).count();
        if ended >= ends {
            assert_eq!(ended, ends, "{lines:#?}");
            return lines;
        }
        assert!(
            Instant::now() < deadline,
            "{ends} runs not ended within 30 s: {lines:#?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// What `git ARGS` in `dir` prints, trimmed.
pub(crate) fn git_says(dir: &Path, args: &[&str]) -> String {
    let done = output(Command::new("git").arg("-C").arg(dir).args(args)).unwrap();
    assert!(done.status.success(), "git {args:?}: {done:?}");
    String::from_utf8(done.stdout).unwrap().trim().to_string()
}

/// The history of the work folder `work` once it keeps `count` operations.
pub(crate) fn kept(work: &Path, count: usize) -> Vec<Record> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let history = recorded::history(work).unwrap_or_default();
        if history.len() >= count {
            assert_eq!(history.len(), count, "{history:?}");
            return history;
        }
        assert!(
            Instant::now() < deadline,
            "{count} operations not kept within 30 s: {history:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A work folder and a machine folder of the test's own, under `name`.
struct Folders {
    work: PathBuf,
    machine: PathBuf,
    dir: PathBuf,
}

impl Folders {
    fn new(name: &str) -> Folders {
        let dir = scratch(name);
        let machine = dir.join("machine");
        fs::create_dir_all(&machine).unwrap();
        fs::write(machine.join("machine-id"), format!("1\t{MACHINE}\n")).unwrap();
        Folders {
            work: dir.join("work"),
            machine,
            dir,
        }
    }

    /// The operations a reader finds now.
    fn records(&self) -> Vec<Record> {
        recorded::read(&self.work)
            .records
            .into_iter()
            .map(|(record, _)| record)
            .collect()
    }

    /// The one operation recorded, once it is in `state`.
    fn once(&self, state: Step) -> Record {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let [one] = self.records().as_slice()
                && one.state == state
            {
                return one.clone();
            }
            assert!(
                Instant::now() < deadline,
                "no operation {state:?} within 30 s: {:?}",
                self.records()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// The `tasks` binary running `args` for `case`'s board, as `session` when one is given, recording
/// in `folders`; standard streams the test's to set.
fn tasks(case: &Case, folders: &Folders, session: Option<&str>, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tasks"));
    command
        .args(args)
        .envs(&case.env.vars)
        .env("TASKS_WORK", &folders.work)
        .env("SYMPHONY_MACHINE_DIR", &folders.machine)
        .env_remove("TASKS_SESSION")
        .env_remove(TASK_VAR)
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove(recorded::PARENT_VAR)
        .env_remove(recorded::PIPELINE_VAR)
        .env_remove(recorded::TRIGGER_VAR)
        .env_remove(RUNS_VAR)
        .current_dir(&folders.dir);
    if let Some(session) = session {
        command.env("TASKS_SESSION", session);
    }
    command
}

/// A command that writes its pid to `pid`, then runs until `release` exists.
fn until_released(release: &Path, pid: &Path) -> Vec<String> {
    [
        "sh",
        "-c",
        "echo $$ > \"$2\"; while [ ! -e \"$1\" ]; do sleep 0.02; done",
        "sh",
        release.to_str().unwrap(),
        pid.to_str().unwrap(),
    ]
    .map(str::to_string)
    .to_vec()
}

fn spawn(command: &mut Command) -> Child {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn run_args<'a>(phase: &'a str, label: &'a str, command: &'a [String]) -> Vec<&'a str> {
    let mut args = vec!["run", "--phase", phase, "--label", label, "--"];
    args.extend(command.iter().map(String::as_str));
    args
}

/// Whether process `pid` is there to signal.
fn alive(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 only asks whether the process exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// The pid the command wrote to `file`, once it did.
fn written_pid(file: &Path) -> libc::pid_t {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(pid) = fs::read_to_string(file).unwrap_or_default().trim().parse() {
            return pid;
        }
        assert!(Instant::now() < deadline, "no pid within 30 s");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_recorded_command_keeps_its_arguments_streams_session_and_status() {
    let case = Case::new();
    let folders = Folders::new("recorder-streams");
    let input = folders.dir.join("stdin");
    fs::write(&input, "caller input\n").unwrap();
    let script = "printf 'arg=<%s>\\n' \"$@\"; cat; printf 'diagnostic' >&2; \
                  printf 'session=%s\\nparent=%s\\n' \"$TASKS_SESSION\" \"$TASKS_RUN_PARENT\"; exit 7";
    let done = wait_output(
        tasks(
            &case,
            &folders,
            Some("nobody-here"),
            &[
                "run",
                "--phase",
                "test",
                "--",
                "sh",
                "-c",
                script,
                "sh",
                "one argument",
                "--",
                "--exact",
            ],
        )
        .stdin(Stdio::from(fs::File::open(&input).unwrap()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap(),
    )
    .unwrap();
    let finished = folders.once(Step::Finished);
    assert_eq!(done.status.code(), Some(7));
    assert_eq!(done.stderr, b"diagnostic");
    assert_eq!(
        String::from_utf8(done.stdout).unwrap(),
        format!(
            "arg=<one argument>\narg=<-->\narg=<--exact>\ncaller input\nsession=nobody-here\nparent={}\n",
            finished.operation_id
        )
    );
    assert_eq!((finished.exit_code, finished.signal), (Some(7), None));
    assert_eq!(
        finished.label,
        format!("sh -c {script} sh one argument -- --exact")
    );
    assert_eq!(finished.phase, Phase::Test);
    // The session holds nothing on this board: no task, but the session it ran for.
    assert_eq!(finished.handle.as_deref(), Some("nobody-here"));
    assert_eq!(finished.task, None);
    assert_eq!(finished.machine_id.as_deref(), Some(MACHINE));
    assert_eq!(
        Path::new(&finished.checkout),
        folders.dir.canonicalize().unwrap()
    );
    // Its recorder writes it finished, then lets its lease go: a step of its own after it.
    let lease = folders
        .work
        .join(recorded::OPERATIONS)
        .join(format!("{}.lease", finished.operation_id));
    let deadline = Instant::now() + Duration::from_secs(30);
    while lease.exists() {
        assert!(
            Instant::now() < deadline,
            "a finished operation lets its lease go"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Seconds since the epoch, the clock a record is stamped by.
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

/// A board server that takes one connection and never answers it: once it took it, it makes the
/// file `accepted` and tells when it took it, then when the asker gave up and closed it.
struct Silent {
    address: String,
    heard: mpsc::Receiver<(f64, f64)>,
}

impl Silent {
    fn new(accepted: &Path) -> Silent {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let (tell, heard) = mpsc::channel();
        let accepted = accepted.to_path_buf();
        std::thread::spawn(move || {
            let (mut asked, _) = listener.accept().unwrap();
            let taken = now();
            fs::write(&accepted, "").unwrap();
            let mut said = [0; 4096];
            while matches!(asked.read(&mut said), Ok(read) if read > 0) {}
            let _ = tell.send((taken, now()));
        });
        Silent { address, heard }
    }

    /// When it took the question, then when the asker gave up.
    fn heard(&self) -> (f64, f64) {
        self.heard.recv_timeout(Duration::from_secs(60)).unwrap()
    }
}

/// A command that runs until `accepted` exists: until the board is being asked its task.
fn until_asked(accepted: &Path) -> Vec<&str> {
    vec![
        "/bin/sh",
        "-c",
        "while [ ! -e \"$1\" ]; do :; done",
        "sh",
        accepted.to_str().unwrap(),
    ]
}

/// `finished`, run while the board took the question of its task at `taken` and its asker gave up
/// at `closed`, was stamped as it ran, not once the board answered: asked for after `called`,
/// started before the board was asked, ended after that (its command waited for it) and before
/// the asker gave up; for its session, and no task.
fn stamped_while_asked(finished: &Record, called: f64, (taken, closed): (f64, f64)) {
    let (queued, started, ended) = (
        finished.queued_at,
        finished.started_at.unwrap(),
        finished.ended_at.unwrap(),
    );
    assert!(
        called <= queued && queued <= started && started <= taken,
        "called at {called}, the board asked at {taken}: {finished:?}"
    );
    assert!(
        taken <= ended && ended < closed,
        "the board asked at {taken}, given up at {closed}: {finished:?}"
    );
    assert_eq!(
        (finished.handle.as_deref(), finished.task.as_deref()),
        (Some("asks-a-silent-board"), None)
    );
}

#[test]
fn its_times_are_stamped_as_they_come_while_a_board_that_never_answers_is_asked_its_task() {
    let mut case = Case::new();
    let folders = Folders::new("recorder-silent-board");
    let accepted = folders.dir.join("accepted");
    let board = Silent::new(&accepted);
    case.env
        .vars
        .insert("TASKS_SERVER".into(), board.address.clone());
    let mut args = vec!["run", "--phase", "test", "--"];
    args.extend(until_asked(&accepted));
    let called = now();
    let done =
        output(tasks(&case, &folders, Some("asks-a-silent-board"), &args).stdin(Stdio::null()))
            .unwrap();
    assert!(done.status.success(), "{done:?}");
    stamped_while_asked(&folders.once(Step::Finished), called, board.heard());
}

#[test]
fn in_its_callers_process_its_times_are_stamped_as_they_come_while_the_board_is_asked() {
    let case = Case::new();
    let folders = Folders::new("recorder-silent-board-in-process");
    let accepted = folders.dir.join("accepted");
    let board = Silent::new(&accepted);
    let mut env = case.env.clone();
    for (name, value) in [
        ("TASKS_SERVER", board.address.as_str()),
        ("TASKS_SESSION", "asks-a-silent-board"),
        ("TASKS_WORK", folders.work.to_str().unwrap()),
        ("SYMPHONY_MACHINE_DIR", folders.machine.to_str().unwrap()),
    ] {
        env.vars.insert(name.into(), value.into());
    }
    env.cwd.clone_from(&folders.dir);
    let mut args = vec!["run", "--phase", "test", "--"];
    args.extend(until_asked(&accepted));
    let called = now();
    let ran = crate::run(&env, &args, "");
    assert_eq!(ran.code, 0, "{}{}", ran.out, ran.err);
    stamped_while_asked(&folders.once(Step::Finished), called, board.heard());
}

/// Whether this process's child `pid` has ended, while it is not reaped; none once it is.
fn ended(pid: libc::pid_t) -> Option<bool> {
    // SAFETY: siginfo_t is plain data, for which all zeroes is a value.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: reads whether one child of this process ended into `info`, waiting for and reaping
    // none.
    let waited = unsafe {
        libc::waitid(
            libc::P_PID,
            libc::id_t::try_from(pid).unwrap(),
            &raw mut info,
            libc::WEXITED | libc::WNOWAIT | libc::WNOHANG,
        )
    };
    if waited != 0 {
        let error = std::io::Error::last_os_error();
        assert_eq!(error.raw_os_error(), Some(libc::ECHILD), "{error}");
        return None;
    }
    Some(info.si_signo == libc::SIGCHLD)
}

#[test]
fn a_command_that_ends_while_the_board_is_asked_is_left_unreaped_until_its_recorder_is_done() {
    let case = Case::new();
    let folders = Folders::new("recorder-unreaped-in-process");
    let accepted = folders.dir.join("accepted");
    let pid = folders.dir.join("pid");
    let board = Silent::new(&accepted);
    let mut env = case.env.clone();
    for (name, value) in [
        ("TASKS_SERVER", board.address.as_str()),
        ("TASKS_SESSION", "asks-a-silent-board"),
        ("TASKS_WORK", folders.work.to_str().unwrap()),
        ("SYMPHONY_MACHINE_DIR", folders.machine.to_str().unwrap()),
    ] {
        env.vars.insert(name.into(), value.into());
    }
    env.cwd.clone_from(&folders.dir);
    let command = "echo $$ > \"$2\"; while [ ! -e \"$1\" ]; do :; done; exit 7";
    let args = [
        "run",
        "--phase",
        "test",
        "--",
        "/bin/sh",
        "-c",
        command,
        "sh",
        accepted.to_str().unwrap(),
        pid.to_str().unwrap(),
    ]
    .map(String::from);
    let (tell, done) = mpsc::channel();
    std::thread::spawn(move || {
        let args = args.each_ref().map(String::as_str);
        let _ = tell.send(crate::run(&env, &args, ""));
    });
    let pid = written_pid(&pid);
    let deadline = Instant::now() + Duration::from_secs(30);
    while ended(pid) != Some(true) {
        assert_eq!(ended(pid), Some(false), "reaped before it was seen ended");
        assert!(Instant::now() < deadline, "not ended within 30 s");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        board.heard.try_recv().is_err(),
        "the board was no longer asked once it ended"
    );
    let ran = done.recv_timeout(Duration::from_secs(60)).unwrap();
    assert_eq!(ran.code, 7, "{}{}", ran.out, ran.err);
    assert_eq!(ended(pid), None, "left unreaped");
    let finished = folders.once(Step::Finished);
    assert_eq!((finished.exit_code, finished.signal), (Some(7), None));
}

#[test]
fn a_command_word_of_any_bytes_reaches_the_command_whole() {
    use std::os::unix::ffi::OsStrExt as _;
    let case = Case::new();
    let folders = Folders::new("recorder-bytes");
    let odd = std::ffi::OsStr::from_bytes(b"a\xffb");
    let done = output(
        tasks(&case, &folders, None, &["run", "--phase", "build", "--"])
            .args([
                std::ffi::OsStr::new("printf"),
                std::ffi::OsStr::new("%s"),
                odd,
            ])
            .stdin(Stdio::null()),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    assert_eq!(done.stdout, b"a\xffb");
    assert_eq!(folders.once(Step::Finished).label, "printf %s a\u{fffd}b");
}

#[test]
fn a_command_keeps_the_descriptors_ignored_signals_and_signal_mask_its_caller_handed_it() {
    use std::os::unix::process::CommandExt as _;
    /// A descriptor a jobserver might pass, open across exec: the number the command writes to.
    const HANDED: libc::c_int = 5;
    let case = Case::new();
    let folders = Folders::new("recorder-inherited");
    let passed = folders.dir.join("passed");
    let script =
        format!("echo through >&{HANDED}; kill -HUP $$; kill -INT $$; kill -USR1 $$; echo alive");
    let run = |wrapped: bool| {
        let file = fs::File::create(&passed).unwrap();
        let mut command = if wrapped {
            let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_tasks"));
            command
                .args(["run", "--phase", "test", "--"])
                .envs(&case.env.vars)
                .env("TASKS_WORK", &folders.work)
                .env("SYMPHONY_MACHINE_DIR", &folders.machine)
                .env_remove("TASKS_SESSION")
                .env_remove(TASK_VAR)
                .env_remove(recorded::PARENT_VAR);
            command
        } else {
            std::process::Command::new("sh")
        };
        if wrapped {
            command.arg("sh");
        }
        command
            .args(["-c", script.as_str()])
            .current_dir(&folders.dir);
        let fd = std::os::fd::AsRawFd::as_raw_fd(&file);
        // SAFETY: dup2, fcntl, signal and sigprocmask are async-signal-safe and change only the
        // child.
        unsafe {
            command.pre_exec(move || {
                // The file opens close-on-exec at whatever number is free, HANDED itself among
                // them, and dup2 onto its own number changes nothing, close-on-exec included: the
                // flag is cleared here, whatever number the file opened at.
                if libc::dup2(fd, HANDED) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                let flags = libc::fcntl(HANDED, libc::F_GETFD);
                if flags == -1
                    || libc::fcntl(HANDED, libc::F_SETFD, flags & !libc::FD_CLOEXEC) == -1
                {
                    return Err(std::io::Error::last_os_error());
                }
                libc::signal(libc::SIGHUP, libc::SIG_IGN);
                libc::signal(libc::SIGINT, libc::SIG_IGN);
                let mut blocked: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&raw mut blocked);
                libc::sigaddset(&raw mut blocked, libc::SIGUSR1);
                libc::sigprocmask(libc::SIG_BLOCK, &raw const blocked, std::ptr::null_mut());
                Ok(())
            });
        }
        let done = command.stdin(std::process::Stdio::null()).output().unwrap();
        drop(file);
        (done, fs::read_to_string(&passed).unwrap())
    };
    // The shell itself keeps them: what the command must find through the recorder too.
    let (direct, through) = run(false);
    assert_eq!(
        (
            direct.status.code(),
            direct.stdout.as_slice(),
            through.as_str()
        ),
        (Some(0), &b"alive\n"[..], "through\n"),
        "{direct:?}"
    );
    let (wrapped, through) = run(true);
    assert_eq!(
        (
            wrapped.status.code(),
            wrapped.stdout.as_slice(),
            through.as_str()
        ),
        (Some(0), &b"alive\n"[..], "through\n"),
        "{wrapped:?}"
    );
    assert_eq!(folders.once(Step::Finished).exit_code, Some(0));
}

#[test]
fn a_command_ended_by_a_signal_ends_its_caller_by_the_same_signal() {
    let case = Case::new();
    let folders = Folders::new("recorder-signalled");
    let done = output(
        tasks(
            &case,
            &folders,
            None,
            &["run", "--phase", "build", "--", "sh", "-c", "kill -TERM $$"],
        )
        .stdin(Stdio::null()),
    )
    .unwrap();
    assert_eq!(done.status.signal(), Some(15), "{done:?}");
    let finished = folders.once(Step::Finished);
    assert_eq!((finished.exit_code, finished.signal), (None, Some(15)));
}

#[test]
fn a_signal_sent_to_the_process_its_caller_started_reaches_its_command() {
    let case = Case::new();
    let folders = Folders::new("recorder-forwards");
    let trapped = folders.dir.join("trapped");
    let mut recorder = spawn(&mut tasks(
        &case,
        &folders,
        None,
        &[
            "run",
            "--phase",
            "test",
            "--",
            "sh",
            "-c",
            "trap 'exit 42' TERM; : > \"$1\"; while :; do sleep 0.02; done",
            "sh",
            trapped.to_str().unwrap(),
        ],
    ));
    folders.once(Step::Running);
    // Running says its command started, not that it is ready for the signal yet: it says that.
    let deadline = Instant::now() + Duration::from_secs(30);
    while !trapped.exists() {
        assert!(Instant::now() < deadline, "the command never set its trap");
        std::thread::sleep(Duration::from_millis(20));
    }
    let pid = libc::pid_t::try_from(recorder.id()).unwrap();
    // SAFETY: a signal to the process this test started and has not waited for.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    let status = recorder.wait().unwrap();
    assert_eq!(status.code(), Some(42), "{status:?}");
    assert_eq!(folders.once(Step::Finished).exit_code, Some(42));
}

/// The process the caller started is the command's: killing it outright leaves no command running
/// on, and its recorder, in a process of its own, still says how it ended.
#[cfg(target_os = "macos")]
#[test]
fn a_kill_of_the_process_its_caller_started_ends_its_command_and_is_recorded() {
    let case = Case::new();
    let folders = Folders::new("recorder-killed");
    let (release, pid) = (folders.dir.join("release"), folders.dir.join("pid"));
    let command = until_released(&release, &pid);
    let mut started = spawn(&mut tasks(
        &case,
        &folders,
        None,
        &run_args("build", "killed", &command),
    ));
    let running = folders.once(Step::Running);
    let caller = libc::pid_t::try_from(started.id()).unwrap();
    let command = written_pid(&pid);
    assert_eq!(
        command, caller,
        "the command is the process its caller started"
    );
    assert_eq!(running.child_pid, Some(started.id()));
    assert_ne!(
        running.pid,
        started.id(),
        "its recorder is a process of its own"
    );
    started.kill().unwrap();
    assert_eq!(started.wait().unwrap().signal(), Some(libc::SIGKILL));
    assert!(!alive(command), "the command ran on");
    let finished = folders.once(Step::Finished);
    assert_eq!(
        (finished.exit_code, finished.signal),
        (None, Some(libc::SIGKILL))
    );
    fs::write(&release, "").unwrap();
}

#[test]
fn a_command_that_cannot_start_is_said_and_recorded_as_a_shell_says_it_and_never_ran() {
    let case = Case::new();
    let unrunnable = scratch("recorder-unrunnable").join("not executable");
    fs::write(&unrunnable, "#!/bin/sh\n").unwrap();
    for (name, command, code) in [
        ("recorder-missing", "no-such-command-anywhere", 127),
        ("recorder-refused", unrunnable.to_str().unwrap(), 126),
    ] {
        let folders = Folders::new(name);
        let done = output(
            tasks(
                &case,
                &folders,
                None,
                &["run", "--phase", "build", "--", command],
            )
            .stdin(Stdio::null()),
        )
        .unwrap();
        assert_eq!(done.status.code(), Some(code), "{done:?}");
        assert!(done.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&done.stderr).starts_with(&format!("tasks run: {command}: ")),
            "{done:?}"
        );
        let finished = folders.once(Step::Finished);
        assert_eq!(finished.exit_code, Some(code));
        assert_eq!(finished.started_at, None, "it never ran");
        assert!(finished.reason.starts_with("its command did not start"));
    }
}

#[test]
fn the_task_its_session_holds_is_its_work_while_it_runs_and_its_record_keeps_how_long_it_took() {
    let case = Case::new();
    let worker = case.register("worker");
    case.ok(&worker, &["create", "tools/recorded", "Recorded"]);
    case.ok(&worker, &["claim", "tools/recorded"]);
    let folders = Folders::new("recorder-claimed");
    let (release, pid) = (folders.dir.join("release"), folders.dir.join("pid"));
    let command = until_released(&release, &pid);
    let mut recorder = spawn(&mut tasks(
        &case,
        &folders,
        Some(&worker),
        &run_args("test", "verify", &command),
    ));
    let running = folders.once(Step::Running);
    assert_eq!(running.task.as_deref(), Some("tools/recorded"));
    assert_eq!(running.handle.as_deref(), Some(worker.as_str()));
    assert_eq!(running.label, "verify");
    assert_eq!(running.phase, Phase::Test);
    assert!(running.child_pid.is_some());
    assert!(running.queued_at <= running.started_at.unwrap());
    // Its recorder says it still runs, a beat at a time.
    let deadline = Instant::now() + Duration::from_secs(30);
    let beaten = loop {
        let now = folders.once(Step::Running);
        if now.beat_at > running.beat_at {
            break now;
        }
        assert!(Instant::now() < deadline, "no beat within 30 s");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(beaten.started_at, running.started_at);
    fs::write(&release, "").unwrap();
    assert!(recorder.wait().unwrap().success());
    let finished = folders.once(Step::Finished);
    assert_eq!(finished.operation_id, running.operation_id);
    assert_eq!(finished.task.as_deref(), Some("tools/recorded"));
    assert_eq!((finished.exit_code, finished.signal), (Some(0), None));
    let duration = finished.duration().unwrap();
    assert!(
        duration >= beaten.beat_at - running.started_at.unwrap(),
        "it ran at least until its beat: {finished:?}"
    );
    assert_eq!(kept(&folders.work, 1), [finished]);
}

#[test]
fn a_run_records_the_task_named_else_the_environments_else_the_one_its_session_holds() {
    let case = Case::new();
    let worker = case.register("worker");
    for slug in ["tools/one", "tools/two"] {
        case.ok(&worker, &["create", slug, "Held"]);
        case.ok(&worker, &["claim", slug]);
    }
    for (name, session, named, from_env, expected) in [
        ("recorder-unattributed", None, None, None, (None, None)),
        (
            "recorder-ambiguous",
            Some(worker.as_str()),
            None,
            None,
            (Some(worker.as_str()), None),
        ),
        (
            "recorder-named",
            Some(worker.as_str()),
            Some("tools/two"),
            Some("tools/one"),
            (Some(worker.as_str()), Some("tools/two")),
        ),
        (
            "recorder-env-task",
            Some(worker.as_str()),
            None,
            Some("tools/one"),
            (Some(worker.as_str()), Some("tools/one")),
        ),
        // An orchestrator, or a subagent with no session of its own.
        (
            "recorder-sessionless",
            None,
            None,
            Some("tools/two"),
            (None, Some("tools/two")),
        ),
    ] {
        let folders = Folders::new(name);
        let mut args = vec!["run", "--phase", "build"];
        if let Some(task) = named {
            args.extend(["--task", task]);
        }
        args.extend(["--", "true"]);
        let mut command = tasks(&case, &folders, session, &args);
        if let Some(task) = from_env {
            command.env(TASK_VAR, task);
        }
        let done = output(command.stdin(Stdio::null())).unwrap();
        assert!(done.status.success(), "{name}: {done:?}");
        let finished = folders.once(Step::Finished);
        assert_eq!(
            (finished.handle.as_deref(), finished.task.as_deref()),
            expected,
            "{name}"
        );
    }
}

/// An executor staffed by `/work` holds its feature and the instruction under it: its run is the
/// feature's. A session holding two tasks side by side under one still records none.
#[test]
fn a_session_holding_a_task_and_work_under_it_records_that_task() {
    let case = Case::new();
    let worker = case.register("worker");
    let other = case.register("worker");
    case.ok(&worker, &["create", "tools/feature", "Feature"]);
    case.ok(&worker, &["create", "tools/apart", "Apart"]);
    for (slug, parent) in [
        ("tools/feature/work", "tools/feature"),
        ("tools/feature/slice", "tools/feature"),
        ("tools/apart/one", "tools/apart"),
        ("tools/apart/two", "tools/apart"),
    ] {
        case.ok(&worker, &["create", slug, "Under", "--parent", parent]);
    }
    for slug in ["tools/feature/work", "tools/feature", "tools/feature/slice"] {
        case.ok(&worker, &["claim", slug]);
    }
    for slug in ["tools/apart/one", "tools/apart/two"] {
        case.ok(&other, &["claim", slug]);
    }
    for (name, session, expected) in [
        ("recorder-tree", worker.as_str(), Some("tools/feature")),
        ("recorder-trees-apart", other.as_str(), None),
    ] {
        let folders = Folders::new(name);
        let mut command = tasks(
            &case,
            &folders,
            Some(session),
            &["run", "--phase", "test", "--", "true"],
        );
        let done = output(command.stdin(Stdio::null())).unwrap();
        assert!(done.status.success(), "{name}: {done:?}");
        let finished = folders.once(Step::Finished);
        assert_eq!(
            (finished.handle.as_deref(), finished.task.as_deref()),
            (Some(session), expected),
            "{name}"
        );
    }
}

/// A repository of its own under `name`: its main checkout, on `main`, and a linked worktree on
/// each of `branches` (a branch may be in several), canonical, as the recorder and the beat hook
/// read them.
fn worktrees<const N: usize>(name: &str, branches: [&str; N]) -> (PathBuf, [PathBuf; N]) {
    let main = crate::repository(&scratch(name).join("main"))
        .canonicalize()
        .unwrap();
    for branch in branches.iter().collect::<std::collections::BTreeSet<_>>() {
        crate::git(&main, &["branch", "-q", branch]);
    }
    let trees = std::array::from_fn(|at| {
        let tree = main.with_file_name(format!("tree-{at}"));
        let path = tree.to_str().unwrap();
        crate::git(&main, &["worktree", "add", "-q", "-f", path, branches[at]]);
        tree.canonicalize().unwrap()
    });
    (main, trees)
}

/// `case`'s board with its clock `seconds` before the clock its runs read.
fn ago(case: &Case, seconds: f64) -> Case {
    let mut env = case.env.clone();
    let then = crate::common::NOW - seconds;
    env.vars.insert("TASKS_NOW".into(), format!("{then:?}"));
    Case { env }
}

/// The beat hook's events for a board command naming `handle`, run from `cwd` and ended (so it
/// leaves no busy mark): by the subagent `agent`, else by the full Claude Code session `claude`.
fn bind(case: &Case, handle: &str, agent: Option<&str>, claude: &str, cwd: &Path) {
    for hook in ["PreToolUse", "PostToolUse"] {
        let mut event = serde_json::json!({"hook_event_name": hook, "tool_name": "Bash",
            "tool_input": {"command": format!("just tasks show tools/bound --as {handle}")},
            "tool_use_id": format!("call-{handle}"), "session_id": claude, "cwd": cwd});
        if let Some(agent) = agent {
            event["agent_id"] = agent.into();
        }
        let ran = crate::run(&case.env, &["beat"], &event.to_string());
        assert_eq!((ran.code, ran.err.as_str()), (0, ""), "{event}");
    }
}

/// The handle and the task a run of `true` with `vars`, in `cwd`, records on `case`'s board.
fn run_in(
    case: &Case,
    name: &str,
    cwd: &Path,
    vars: &[(&str, &str)],
) -> (Option<String>, Option<String>) {
    let folders = Folders::new(name);
    let mut command = tasks(
        case,
        &folders,
        None,
        &["run", "--phase", "test", "--", "true"],
    );
    command.current_dir(cwd).envs(vars.iter().copied());
    let done = output(command.stdin(Stdio::null())).unwrap();
    assert!(done.status.success(), "{vars:?}: {done:?}");
    let finished = folders.once(Step::Finished);
    (finished.handle, finished.task)
}

/// Runs on a board, each with a work folder of its own, named `PREFIX-N`: a test's prefix is its
/// own, since tests run at once under one scratch folder and a folder named again is emptied.
struct Runs<'c> {
    case: &'c Case,
    prefix: &'c str,
    made: std::cell::Cell<usize>,
}

impl<'c> Runs<'c> {
    fn on(case: &'c Case, prefix: &'c str) -> Runs<'c> {
        Runs {
            case,
            prefix,
            made: std::cell::Cell::new(0),
        }
    }

    /// A run in `cwd` with `vars` records `expected`, its handle and its task, for the reason `why`.
    fn record(
        &self,
        why: &str,
        cwd: &Path,
        vars: &[(&str, &str)],
        expected: (Option<&str>, Option<&str>),
    ) {
        let at = self.made.replace(self.made.get() + 1);
        let found = run_in(self.case, &format!("{}-{at}", self.prefix), cwd, vars);
        assert_eq!(
            (found.0.as_deref(), found.1.as_deref()),
            expected,
            "{why}: in {} with {vars:?}",
            cwd.display()
        );
    }
}

const CLAUDE: &str = "CLAUDE_CODE_SESSION_ID";

/// With no task named, a run's task is, in this order: the given session's; the one live session
/// the beat hook placed in its linked worktree; its branch's when that is an open task's slug; the
/// live full session its Claude Code session binds (a subagent's is its parent's, so this never
/// beats the branch); none. Each earlier one wins over a later one that says otherwise, and the
/// main checkout, where every agent's board commands run, places no one.
#[test]
fn an_unnamed_runs_task_is_its_sessions_the_placed_sessions_its_branchs_then_its_claude_sessions() {
    const PARENT: &str = "claude-parent";
    const ORCHESTRATOR: &str = "claude-orchestrator";
    let case = Case::new();
    let [explicit, idle, here, full, one, two, in_main] =
        ["worker"; 7].map(|kind| case.register(kind));
    let orchestrator = case.register("orchestrator");
    for (slug, by) in [
        ("tools/explicit", Some(explicit.as_str())),
        ("tools/bound", Some(here.as_str())),
        ("tools/full-session", Some(full.as_str())),
        ("tools/one", Some(&one)),
        ("tools/two", Some(&two)),
        ("tools/in-main", Some(&in_main)),
        ("tools/branch", None),
        ("tools/branch-gone", None),
    ] {
        case.ok(&explicit, &["create", slug, "Held"]);
        if let Some(by) = by {
            case.ok(by, &["claim", slug]);
        }
    }
    case.ok(&explicit, &["move", "tools/branch-gone", "cancelled"]);
    let (main, [bound, shared, plain, gone, unknown]) = worktrees(
        "recorder-attribution",
        [
            "tools/branch",
            "tools/branch",
            "tools/branch",
            "tools/branch-gone",
            "tools/no-such-task",
        ],
    );
    bind(&case, &here, Some("agent-here"), PARENT, &bound);
    bind(&case, &one, Some("agent-one"), PARENT, &shared);
    bind(&case, &two, Some("agent-two"), PARENT, &shared);
    bind(&case, &in_main, Some("agent-in-main"), PARENT, &main);
    bind(&case, &full, None, PARENT, &main);
    bind(&case, &orchestrator, None, ORCHESTRATOR, &main);
    let runs = Runs::on(&case, "recorder-attribution-run");
    runs.record(
        "1. the session given, over a placed one, the branch and the Claude Code session",
        &bound,
        &[("TASKS_SESSION", explicit.as_str()), (CLAUDE, PARENT)],
        (Some(explicit.as_str()), Some("tools/explicit")),
    );
    runs.record(
        "the task named, over everything",
        &bound,
        &[(TASK_VAR, "tools/named"), (CLAUDE, PARENT)],
        (None, Some("tools/named")),
    );
    runs.record(
        "a given session holding nothing keeps its handle; the branch names the task",
        &bound,
        &[("TASKS_SESSION", idle.as_str())],
        (Some(idle.as_str()), Some("tools/branch")),
    );
    runs.record(
        "2. the session placed here, over the branch and the Claude Code session",
        &bound,
        &[(CLAUDE, PARENT)],
        (Some(here.as_str()), Some("tools/bound")),
    );
    runs.record(
        "two placed here: no handle is guessed, and the branch names the task",
        &shared,
        &[(CLAUDE, PARENT)],
        (None, Some("tools/branch")),
    );
    runs.record(
        "3. a subagent not placed yet: its branch, over its parent's task",
        &plain,
        &[(CLAUDE, PARENT)],
        (None, Some("tools/branch")),
    );
    runs.record(
        "4. a branch naming no task: the full session its Claude Code session binds",
        &unknown,
        &[(CLAUDE, PARENT)],
        (Some(full.as_str()), Some("tools/full-session")),
    );
    runs.record(
        "the main checkout places no one: its Claude Code session's",
        &main,
        &[(CLAUDE, PARENT)],
        (Some(full.as_str()), Some("tools/full-session")),
    );
    runs.record(
        "an orchestrator's build in the main checkout is no task's",
        &main,
        &[(CLAUDE, ORCHESTRATOR)],
        (Some(orchestrator.as_str()), None),
    );
    runs.record(
        "5. a terminal's build in the main checkout is no one's",
        &main,
        &[],
        (None, None),
    );
    runs.record("a finished task's branch is none", &gone, &[], (None, None));
    runs.record("a branch no task has is none", &unknown, &[], (None, None));
}

/// A session counts only while the board reads it live: one whose agent finished and never left
/// (as no `/work` agent leaves), or went stale, is passed over wherever the beat hook last placed
/// it, and counts again once it beats.
#[test]
fn only_a_live_session_takes_a_run_so_a_finished_ones_last_place_never_does() {
    const ORCHESTRATOR: &str = "claude-orchestrator";
    const GONE: &str = "claude-gone";
    let case = Case::new();
    let (main, [stale_tree, other, unknown]) = worktrees(
        "recorder-live",
        ["tools/branch", "tools/other", "tools/no-such-task"],
    );
    let [filer, other_executor] = ["worker"; 2].map(|kind| case.register(kind));
    for slug in ["tools/branch", "tools/other"] {
        case.ok(&filer, &["create", slug, "Open"]);
    }
    // A subagent whose last tool call was in a worktree, two minutes ago: stale.
    let two_minutes_ago = ago(&case, 120.0);
    let stale = two_minutes_ago.register("worker");
    two_minutes_ago.ok(&stale, &["create", "tools/stale", "Held"]);
    two_minutes_ago.ok(&stale, &["claim", "tools/stale"]);
    bind(
        &two_minutes_ago,
        &stale,
        Some("agent-stale"),
        "claude-stale",
        &stale_tree,
    );
    // An hour ago: a reviewer of work in review, last placed in another task's worktree; a subagent
    // last placed in the main checkout; a full session whose Claude Code session is gone.
    let an_hour_ago = ago(&case, 3600.0);
    let [executor, main_agent, full] = ["worker"; 3].map(|kind| an_hour_ago.register(kind));
    let reviewer = an_hour_ago.register("reviewer");
    for (slug, by) in [
        ("tools/reviewed", &executor),
        ("tools/in-main", &main_agent),
        ("tools/gone-full", &full),
    ] {
        an_hour_ago.ok(by, &["create", slug, "Held"]);
        an_hour_ago.ok(by, &["claim", slug]);
    }
    an_hour_ago.ok(&executor, &["move", "tools/reviewed", "review"]);
    an_hour_ago.ok(&reviewer, &["role", "tools/reviewed", "reviewer"]);
    bind(
        &an_hour_ago,
        &reviewer,
        Some("agent-reviewer"),
        "claude-review",
        &other,
    );
    bind(
        &an_hour_ago,
        &main_agent,
        Some("agent-in-main"),
        ORCHESTRATOR,
        &main,
    );
    bind(&an_hour_ago, &full, None, GONE, &main);
    // Now: the other task's executor is placed in its worktree, and an orchestrator is live.
    let orchestrator = case.register("orchestrator");
    case.ok(&other_executor, &["claim", "tools/other"]);
    bind(
        &case,
        &other_executor,
        Some("agent-other"),
        "claude-other",
        &other,
    );
    bind(&case, &orchestrator, None, ORCHESTRATOR, &main);
    let runs = Runs::on(&case, "recorder-live-run");
    runs.record(
        "a stale subagent's worktree: its branch",
        &stale_tree,
        &[],
        (None, Some("tools/branch")),
    );
    runs.record(
        "a reviewer that last ran in another task's worktree: the live session placed there",
        &other,
        &[],
        (Some(other_executor.as_str()), Some("tools/other")),
    );
    runs.record(
        "an orchestrator's build in the main checkout, where a finished subagent last ran",
        &main,
        &[(CLAUDE, ORCHESTRATOR)],
        (Some(orchestrator.as_str()), None),
    );
    runs.record(
        "a terminal's build in the main checkout",
        &main,
        &[],
        (None, None),
    );
    runs.record(
        "a full session gone stale is not its Claude Code session's",
        &unknown,
        &[(CLAUDE, GONE)],
        (None, None),
    );
    // The stale subagent beats again: it is live, and the run is its task's.
    bind(
        &case,
        &stale,
        Some("agent-stale"),
        "claude-stale",
        &stale_tree,
    );
    runs.record(
        "the stale subagent, live again",
        &stale_tree,
        &[],
        (Some(stale.as_str()), Some("tools/stale")),
    );
}

#[test]
fn a_board_that_cannot_be_reached_or_a_test_board_with_no_work_folder_still_runs_the_command() {
    let mut case = Case::new();
    case.env
        .vars
        .insert("TASKS_SERVER".into(), "127.0.0.1:1".into());
    let folders = Folders::new("recorder-unreachable");
    let done = output(
        tasks(
            &case,
            &folders,
            Some("worker"),
            &["run", "--phase", "test", "--", "sh", "-c", "exit 3"],
        )
        .stdin(Stdio::null()),
    )
    .unwrap();
    assert_eq!(done.status.code(), Some(3));
    let finished = folders.once(Step::Finished);
    assert_eq!(
        (finished.handle.as_deref(), finished.task.as_deref()),
        (Some("worker"), None)
    );
    let unrecorded = output(
        tasks(
            &case,
            &folders,
            None,
            &[
                "run",
                "--phase",
                "test",
                "--",
                "sh",
                "-c",
                "printf '%s' \"$TASKS_RUN_PARENT\"; exit 4",
            ],
        )
        .env_remove("TASKS_WORK")
        .stdin(Stdio::null()),
    )
    .unwrap();
    assert_eq!(unrecorded.status.code(), Some(4));
    assert!(unrecorded.stderr.is_empty(), "{unrecorded:?}");
    assert!(
        unrecorded.stdout.is_empty(),
        "no operation to be the parent of: {unrecorded:?}"
    );
    assert_eq!(folders.records().len(), 1, "nothing more was recorded");
}

#[test]
fn the_next_run_finishes_work_whose_recorder_died_and_keeps_both_in_the_history() {
    let case = Case::new();
    let folders = Folders::new("recorder-lost");
    let (release, pid) = (folders.dir.join("release"), folders.dir.join("pid"));
    let command = until_released(&release, &pid);
    let mut started = spawn(&mut tasks(
        &case,
        &folders,
        None,
        &run_args("test", "lost", &command),
    ));
    let running = folders.once(Step::Running);
    let recorder = libc::pid_t::try_from(running.pid).unwrap();
    // SAFETY: the recorder of the operation this test started.
    assert_eq!(unsafe { libc::kill(recorder, libc::SIGKILL) }, 0);
    let deadline = Instant::now() + Duration::from_secs(30);
    while alive(recorder) {
        assert!(Instant::now() < deadline, "the recorder outlived its kill");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Its command outlives it; a reader cannot say the work ended, nor that it runs.
    let found = recorded::read(&folders.work);
    assert!(found.records.is_empty(), "{found:?}");
    assert!(found.uncertain);
    let done = output(
        tasks(
            &case,
            &folders,
            None,
            &["run", "--phase", "build", "--", "true"],
        )
        .stdin(Stdio::null()),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    let history = kept(&folders.work, 2);
    assert_eq!(history[0].operation_id, running.operation_id);
    assert_eq!((history[0].exit_code, history[0].signal), (None, None));
    assert_eq!(history[0].state, Step::Finished);
    assert_eq!(history[1].exit_code, Some(0));
    assert!(!recorded::read(&folders.work).uncertain);
    // The command itself ran on, unrecorded, and ends as it would have.
    fs::write(&release, "").unwrap();
    assert!(started.wait().unwrap().success());
}

#[test]
fn a_recorder_run_by_a_recorded_command_records_it_as_its_parent() {
    let case = Case::new();
    let folders = Folders::new("recorder-nested");
    let tasks_binary = env!("CARGO_BIN_EXE_tasks");
    let done = output(
        tasks(
            &case,
            &folders,
            None,
            &[
                "run",
                "--phase",
                "test",
                "--label",
                "outer",
                "--",
                tasks_binary,
                "run",
                "--phase",
                "build",
                "--label",
                "inner",
                "--",
                "true",
            ],
        )
        .stdin(Stdio::null()),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    let history = kept(&folders.work, 2);
    let labelled = |label: &str| {
        history
            .iter()
            .find(|record| record.label == label)
            .unwrap_or_else(|| panic!("no {label}: {history:?}"))
    };
    let (inner, outer) = (labelled("inner"), labelled("outer"));
    assert_eq!(
        inner.parent_id.as_deref(),
        Some(outer.operation_id.as_str())
    );
    assert_eq!(outer.parent_id, None);
    assert_eq!((inner.exit_code, outer.exit_code), (Some(0), Some(0)));
}

#[test]
fn a_probe_answers_its_version_and_a_command_line_it_cannot_read_is_refused() {
    let case = Case::new();
    let folders = Folders::new("recorder-probe");
    let probe =
        output(tasks(&case, &folders, None, &["run", "--probe"]).stdin(Stdio::null())).unwrap();
    assert!(probe.status.success());
    assert_eq!(
        (probe.stdout.as_slice(), probe.stderr.as_slice()),
        (&b"run 1\nrun 2\n"[..], &b""[..])
    );
    let refused = output(
        tasks(
            &case,
            &folders,
            None,
            &["run", "--phase", "lint", "--", "true"],
        )
        .stdin(Stdio::null()),
    )
    .unwrap();
    assert_eq!(refused.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("usage: tasks run"));
    assert!(!folders.work.exists(), "nothing was recorded");
}

#[test]
fn a_command_line_of_another_command_that_is_not_text_is_refused_without_a_panic() {
    use std::os::unix::ffi::OsStrExt as _;
    let case = Case::new();
    let folders = Folders::new("recorder-not-text");
    let done = output(
        tasks(&case, &folders, None, &["show"])
            .arg(std::ffi::OsStr::from_bytes(b"\xff"))
            .stdin(Stdio::null()),
    )
    .unwrap();
    assert_eq!(done.status.code(), Some(2), "{done:?}");
    assert_eq!(done.stderr, b"tasks: an argument is not UTF-8 text\n");
}

/// `tasks run --pipeline PIPELINE --label LABEL -- sh -c SCRIPT` in `repo`, its lines going to
/// `runs`: its status.
fn pipeline_run(
    case: &Case,
    folders: &Folders,
    (repo, runs): (&Path, &Path),
    (pipeline, label): (&str, &str),
    script: &str,
) -> std::process::ExitStatus {
    let args = [
        "run",
        "--phase",
        "test",
        "--pipeline",
        pipeline,
        "--label",
        label,
        "--",
        "sh",
        "-c",
        script,
    ];
    let done = output(
        tasks(case, folders, None, &args)
            .env(RUNS_VAR, runs)
            .current_dir(repo)
            .stdin(Stdio::null()),
    )
    .unwrap();
    done.status
}

/// `case`'s environment for `tasks run` in this process, at top level in `repo`, recording in
/// `folders` and writing its lines to `runs`.
fn in_process(case: &Case, folders: &Folders, (repo, runs): (&Path, &Path)) -> Env {
    let mut env = case.env.clone();
    for (name, value) in [
        ("TASKS_WORK", folders.work.as_path()),
        ("SYMPHONY_MACHINE_DIR", folders.machine.as_path()),
        (RUNS_VAR, runs),
    ] {
        env.vars
            .insert(name.into(), value.to_string_lossy().into_owned());
    }
    for name in [
        "TASKS_SESSION",
        TASK_VAR,
        recorded::PARENT_VAR,
        recorded::PIPELINE_VAR,
        recorded::TRIGGER_VAR,
    ] {
        env.vars.remove(name);
    }
    env.cwd = repo.to_path_buf();
    env
}

/// The runs `lines` say, folded: the one named `run`.
fn the_run<'a>(runs: &'a [RunLine], run: &str) -> &'a RunLine {
    runs.iter()
        .find(|line| line.run == run)
        .unwrap_or_else(|| panic!("no run {run}: {runs:#?}"))
}

#[test]
fn a_run_says_how_it_ended_and_a_rerun_of_its_command_on_its_tree_names_the_run_it_repeats() {
    let case = Case::new();
    let folders = Folders::new("recorder-run-lines");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    // A day file 90 days and more old goes at the next line.
    fs::create_dir_all(&runs).unwrap();
    fs::write(runs.join("2000-01-01.jsonl"), "").unwrap();
    let labelled = |pipeline: &str, label: &str, script: &str| {
        pipeline_run(&case, &folders, (&repo, &runs), (pipeline, label), script)
    };
    // One command, `just test-rs -p a`, whatever its shell does this time.
    let ran = |pipeline: &str, script: &str| labelled(pipeline, "just test-rs -p a", script);
    assert_eq!(ran("test", "exit 3").code(), Some(3));
    let red = symphony_api::fold(day_lines(&runs, 1)).remove(0);
    assert!(
        !runs.join("2000-01-01.jsonl").exists(),
        "an old day file stayed"
    );
    let commit = git_says(&repo, &["rev-parse", "HEAD"]);
    let tree = git_says(&repo, &["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        (red.end, red.exit, red.signal),
        (Some(End::Red), Some(3), None)
    );
    assert_eq!(red.schema, symphony_api::RUN_SCHEMA);
    assert_eq!(
        (
            red.pipeline.as_str(),
            red.step.as_deref(),
            red.parent.as_deref()
        ),
        ("test", None, None)
    );
    assert_eq!(red.commit.as_deref(), Some(commit.as_str()));
    assert_eq!(red.tree.as_deref(), Some(tree.as_str()));
    assert_eq!(
        (red.dirty, red.branch.as_deref()),
        (Some(false), Some("main"))
    );
    assert_eq!(red.machine.as_deref(), Some(MACHINE));
    assert_eq!(red.project.as_deref(), Some("repo"));
    assert_eq!(
        (red.trigger.as_str(), red.handle.as_deref()),
        ("owner", None)
    );
    let (started, ended) = (red.started.unwrap(), red.ended.unwrap());
    assert!(red.queued <= started && started <= ended, "{red:?}");
    assert!(red.cpu_s.is_some_and(|cpu| cpu >= 0.0), "{red:?}");
    assert_eq!(red.rerun_of, None);
    // Its record in the work folder says the same run.
    let [kept_red] = kept(&folders.work, 1).try_into().unwrap();
    assert_eq!(kept_red.operation_id, red.run);
    let run = kept_red.run.unwrap();
    assert_eq!(
        (run.pipeline.as_str(), run.tree.as_deref()),
        ("test", Some(tree.as_str()))
    );
    // The same command on the same tree: it repeats the red, and the CPU its command burned is said.
    let burn = "i=0; while [ $i -lt 100000 ]; do i=$((i+1)); done";
    assert!(ran("test", burn).success());
    let lines = symphony_api::fold(day_lines(&runs, 2));
    let green = &lines[1];
    assert_eq!((green.end, green.exit), (Some(End::Green), Some(0)));
    assert_eq!(green.rerun_of.as_deref(), Some(red.run.as_str()));
    assert!(green.cpu_s.is_some_and(|cpu| cpu >= 0.05), "{green:?}");
    // Another pipeline, or another command of the same one, repeats none.
    assert!(labelled("lint", "just test-rs -p a", "true").success());
    assert!(labelled("test", "just test-rs -p b", "true").success());
    let lines = symphony_api::fold(day_lines(&runs, 4));
    let others: Vec<(&str, &str, Option<&str>)> = lines[2..]
        .iter()
        .map(|line| {
            (
                line.pipeline.as_str(),
                line.label.as_str(),
                line.rerun_of.as_deref(),
            )
        })
        .collect();
    assert!(
        others.contains(&("lint", "just test-rs -p a", None)),
        "{others:?}"
    );
    assert!(
        others.contains(&("test", "just test-rs -p b", None)),
        "{others:?}"
    );
    // A run interrupted says by which signal.
    assert_eq!(ran("test", "kill -INT $$").signal(), Some(libc::SIGINT));
    let lines = symphony_api::fold(day_lines(&runs, 5));
    let interrupted = &lines[4];
    assert_eq!(
        (interrupted.end, interrupted.exit, interrupted.signal),
        (Some(End::Interrupted), None, Some(libc::SIGINT))
    );
    assert_eq!(interrupted.rerun_of.as_deref(), Some(green.run.as_str()));
    // A checkout that differs from its commit is dirty, and repeats nothing.
    fs::write(repo.join("untracked"), "x").unwrap();
    assert!(ran("test", "true").success());
    let lines = symphony_api::fold(day_lines(&runs, 6));
    assert_eq!(
        (lines[5].dirty, lines[5].rerun_of.as_deref()),
        (Some(true), None)
    );
    // Committed, it is a new tree, which no run checked before.
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "untracked"]);
    assert!(ran("test", "true").success());
    let lines = symphony_api::fold(day_lines(&runs, 7));
    assert_eq!(
        (lines[6].dirty, lines[6].rerun_of.as_deref()),
        (Some(false), None)
    );
    assert_ne!(lines[6].tree.as_deref(), Some(tree.as_str()));
    // Each run wrote its start line, then its end line.
    let all = day_lines(&runs, 7);
    assert_eq!(all.len(), 14, "{all:#?}");
    for line in &lines {
        let starts = all
            .iter()
            .filter(|each| each.run == line.run && each.is_start());
        assert_eq!(starts.count(), 1, "{}", line.run);
    }
}

#[test]
fn runs_started_back_to_back_each_name_the_one_their_caller_ran_before() {
    let case = Case::new();
    let folders = Folders::new("recorder-run-back-to-back");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    // Each starts as soon as the one before gave its caller its status, before its recorder
    // wrote its end line.
    let label = ("test", "just test-rs");
    let statuses: Vec<Option<i32>> = ["exit 3", "true", "true", "true"]
        .into_iter()
        .map(|script| pipeline_run(&case, &folders, (&repo, &runs), label, script).code())
        .collect();
    assert_eq!(statuses, [Some(3), Some(0), Some(0), Some(0)]);
    let mut lines = symphony_api::fold(day_lines(&runs, 4));
    lines.sort_by(|one, other| one.queued.total_cmp(&other.queued));
    assert_eq!(lines[0].rerun_of, None);
    for pair in lines.windows(2) {
        assert_eq!(
            pair[1].rerun_of.as_deref(),
            Some(pair[0].run.as_str()),
            "{lines:#?}"
        );
    }
}

#[test]
fn a_checkout_is_clean_or_dirty_as_its_command_starts_whatever_the_command_writes() {
    let case = Case::new();
    let folders = Folders::new("recorder-run-clean-start");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    let label = ("test", "makes a file");
    let script = "touch made";
    // As the binary, which on macOS becomes its command.
    assert!(pipeline_run(&case, &folders, (&repo, &runs), label, script).success());
    let [made] = symphony_api::fold(day_lines(&runs, 1)).try_into().unwrap();
    assert_eq!(made.dirty, Some(false), "{made:?}");
    assert!(repo.join("made").exists());
    fs::remove_file(repo.join("made")).unwrap();
    // In a caller's process, as its child.
    let ran = crate::run(
        &in_process(&case, &folders, (&repo, &runs)),
        &[
            "run",
            "--phase",
            "test",
            "--pipeline",
            "test",
            "--label",
            label.1,
            "--",
            "sh",
            "-c",
            script,
        ],
        "",
    );
    assert_eq!(ran.code, 0, "{}{}", ran.out, ran.err);
    let lines = symphony_api::fold(day_lines(&runs, 2));
    let child = lines.iter().find(|line| line.run != made.run).unwrap();
    assert_eq!(child.dirty, Some(false), "{child:?}");
    assert_eq!(child.rerun_of.as_deref(), Some(made.run.as_str()));
    // Its count is the reap's, which watches for no process that left: no word either way.
    assert!(child.cpu_s.is_some());
    assert_eq!(child.cpu_partial, None);
}

#[test]
fn in_a_callers_process_a_held_lock_never_holds_up_the_status() {
    let case = Case::new();
    let folders = Folders::new("recorder-run-held-lock");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    fs::create_dir_all(&runs).unwrap();
    let holder = fs::File::create(runs.join("lock")).unwrap();
    holder.lock().unwrap();
    let began = Instant::now();
    let ran = crate::run(
        &in_process(&case, &folders, (&repo, &runs)),
        &[
            "run",
            "--phase",
            "test",
            "--pipeline",
            "test",
            "--",
            "sh",
            "-c",
            "exit 7",
        ],
        "",
    );
    let took = began.elapsed();
    assert_eq!(ran.code, 7, "{}{}", ran.out, ran.err);
    assert!(took < Duration::from_secs(1), "{took:?}");
    // Both lines were appended still, each whole, without the lock.
    let [ended] = symphony_api::fold(day_lines(&runs, 1)).try_into().unwrap();
    assert_eq!((ended.end, ended.exit), (Some(End::Red), Some(7)));
    assert_eq!(day_lines(&runs, 1).len(), 2);
    holder.unlock().unwrap();
}

#[cfg(target_os = "macos")]
#[test]
fn on_macos_a_run_says_whether_its_cpu_misses_a_process_that_left_its_command() {
    let case = Case::new();
    let folders = Folders::new("recorder-run-cpu-partial");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    /// Ends the child left running however the test ends.
    struct Release(PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(&self.0, "");
        }
    }
    let release = Release(folders.dir.join("release"));
    // A child left running when the command ends goes to the system, out of its reach: it is
    // released once the recorder's last reading is written. It holds none of the streams the test
    // reads to their end.
    let leaves = format!(
        "(while [ ! -e '{}' ]; do sleep 0.05; done) </dev/null >/dev/null 2>&1 & sleep 1.5",
        release.0.display()
    );
    // Plain runs at once, each reaped by the test, as its own thread sees it end, before or after
    // its recorder reads it the last time: nothing is missed either way.
    const PLAIN: usize = 4;
    std::thread::scope(|scope| {
        let mut started =
            vec![scope.spawn(|| {
                pipeline_run(&case, &folders, (&repo, &runs), ("test", "leaves"), &leaves)
            })];
        for _ in 0..PLAIN {
            started.push(scope.spawn(|| {
                pipeline_run(
                    &case,
                    &folders,
                    (&repo, &runs),
                    ("test", "plain"),
                    "sleep 0.8",
                )
            }));
        }
        for each in started {
            assert!(each.join().unwrap().success());
        }
    });
    let lines = symphony_api::fold(day_lines(&runs, PLAIN + 1));
    drop(release);
    let said = |label: &str| -> Vec<(bool, Option<bool>)> {
        let lines = lines.iter().filter(|line| line.label == label);
        lines
            .map(|line| (line.cpu_s.is_some(), line.cpu_partial))
            .collect()
    };
    assert_eq!(said("plain"), [(true, Some(false)); PLAIN], "{lines:#?}");
    assert_eq!(said("leaves"), [(true, Some(true))], "{lines:#?}");
}

/// The keys of the JSON object `text`, in the order it says them.
fn keys(text: &str) -> Vec<String> {
    struct Keys;
    impl<'de> serde::de::Visitor<'de> for Keys {
        type Value = Vec<String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an object")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut keys = Vec::new();
            while let Some(key) = map.next_key::<String>()? {
                map.next_value::<serde::de::IgnoredAny>()?;
                keys.push(key);
            }
            Ok(keys)
        }
    }
    serde::Deserializer::deserialize_map(&mut serde_json::Deserializer::from_str(text), Keys)
        .unwrap()
}

/// Each word in backquotes in `text`, in order.
fn quoted(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_runs_doc_names_each_field_a_written_line_holds_in_its_order_and_each_end() {
    let case = Case::new();
    let folders = Folders::new("recorder-runs-doc");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    let label = ("test", "just test-rs");
    assert_eq!(
        pipeline_run(&case, &folders, (&repo, &runs), label, "exit 3").code(),
        Some(3)
    );
    day_lines(&runs, 1);
    let day = fs::read_dir(&runs)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .unwrap();
    let text = fs::read_to_string(day).unwrap();
    let end = text
        .lines()
        .find(|line| line.contains("\"end\":\""))
        .unwrap();
    let written = keys(end);
    let doc = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/RUNS.md"))
        .unwrap();
    // The example: a line as the recorder writes one, its keys in the order it writes them. A
    // recorder that watches for no lost process writes no `cpu_partial`.
    let example = doc
        .lines()
        .find(|line| line.starts_with("{\"schema\":"))
        .unwrap();
    serde_json::from_str::<RunLine>(example).unwrap();
    let mut said = keys(example);
    if !written.iter().any(|key| key == "cpu_partial") {
        said.retain(|key| key != "cpu_partial");
    }
    assert_eq!(said, written);
    // The table: each field a line holds, in its order, then those later tools fill.
    let table = doc.split("## A line").nth(1).unwrap();
    let rows: Vec<&str> = table
        .lines()
        .skip_while(|line| !line.starts_with("| `"))
        .take_while(|line| line.starts_with('|'))
        .collect();
    let fields: Vec<String> = rows
        .iter()
        .flat_map(|row| quoted(row.split('|').nth(1).unwrap()))
        .collect();
    let mut every = keys(example);
    every.extend(["log", "log_bytes", "suites", "failed"].map(String::from));
    assert_eq!(fields, every);
    // Each end a line can say, as it says it.
    let ends = |end: End| match end {
        End::Green | End::Red | End::TimedOut | End::Interrupted | End::Incomplete => {
            serde_json::to_value(end)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        }
    };
    let row = rows
        .iter()
        .find(|row| row.starts_with("| `end` |"))
        .unwrap();
    assert_eq!(
        quoted(row.split('|').nth(2).unwrap()),
        [
            End::Green,
            End::Red,
            End::TimedOut,
            End::Interrupted,
            End::Incomplete
        ]
        .map(ends)
    );
    assert!(end.contains("\"end\":\"red\""), "{end}");
}

#[test]
fn a_run_inside_a_run_and_a_command_naming_no_pipeline_write_no_line() {
    let case = Case::new();
    let folders = Folders::new("recorder-run-nested");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    let tasks_binary = env!("CARGO_BIN_EXE_tasks");
    let seen = folders.dir.join("seen");
    // The plain command sees no pipeline, so a `--step` inside it is no step.
    let script = format!(
        "'{tasks_binary}' run --phase build --pipeline lint --step lint -- \
           '{tasks_binary}' run --phase build --pipeline build -- true && \
         '{tasks_binary}' run --phase build --label plain -- sh -c \
           'echo \"[$SYMPHONY_RUN_PIPELINE]\" > \"{seen}\" && \
            \"{tasks_binary}\" run --phase build --step deep -- true'",
        seen = seen.display()
    );
    assert!(
        pipeline_run(
            &case,
            &folders,
            (&repo, &runs),
            ("verify", "just verify"),
            &script
        )
        .success()
    );
    assert_eq!(fs::read_to_string(&seen).unwrap(), "[]\n");
    let history = kept(&folders.work, 5);
    let lines = symphony_api::fold(day_lines(&runs, 2));
    let verify = lines.iter().find(|line| line.step.is_none()).unwrap();
    let lint = lines.iter().find(|line| line.step.is_some()).unwrap();
    assert_eq!(
        (verify.pipeline.as_str(), verify.parent.as_deref()),
        ("verify", None)
    );
    assert_eq!(
        (
            lint.pipeline.as_str(),
            lint.step.as_deref(),
            lint.parent.as_deref()
        ),
        ("verify", Some("lint"), Some(verify.run.as_str()))
    );
    // Five operations, two of them runs: the build inside the step, the plain command and the
    // step inside that are operations of the work folder alone.
    let unlined: Vec<&Record> = history
        .iter()
        .filter(|record| record.run.is_none())
        .collect();
    assert_eq!(unlined.len(), 3, "{history:#?}");
    assert_eq!(the_run(&lines, &lint.run).end, Some(End::Green));
}

#[test]
fn in_a_callers_process_a_step_hands_on_its_pipeline_and_a_plain_command_removes_it() {
    let case = Case::new();
    let folders = Folders::new("recorder-run-nested-child");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    // Inside a run of `verify`, as its recorder tells it.
    let mut env = in_process(&case, &folders, (&repo, &runs));
    env.vars.insert(
        recorded::PARENT_VAR.into(),
        "0123456789abcdef0123456789abcdee".into(),
    );
    env.vars
        .insert(recorded::PIPELINE_VAR.into(), "verify".into());
    let seen = |step: &[&str]| {
        let file = folders.dir.join("seen");
        let script = format!("echo \"[$SYMPHONY_RUN_PIPELINE]\" > '{}'", file.display());
        let mut args = vec!["run", "--phase", "build"];
        args.extend(step);
        args.extend(["--", "sh", "-c", script.as_str()]);
        let ran = crate::run(&env, &args, "");
        assert_eq!(ran.code, 0, "{}{}", ran.out, ran.err);
        fs::read_to_string(file).unwrap()
    };
    assert_eq!(seen(&["--step", "lint"]), "[verify]\n");
    assert_eq!(seen(&["--label", "plain"]), "[]\n");
    assert_eq!(seen(&["--pipeline", "build"]), "[]\n");
    let [step] = symphony_api::fold(day_lines(&runs, 1)).try_into().unwrap();
    assert_eq!(
        (step.pipeline.as_str(), step.step.as_deref()),
        ("verify", Some("lint"))
    );
}

#[test]
fn runs_at_once_each_write_their_lines_whole() {
    let case = Case::new();
    let folders = Folders::new("recorder-run-many");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    let label = "x".repeat(3000);
    let mut started: Vec<Child> = (0..8)
        .map(|_| {
            spawn(
                tasks(
                    &case,
                    &folders,
                    None,
                    &[
                        "run",
                        "--phase",
                        "test",
                        "--label",
                        label.as_str(),
                        "--pipeline",
                        "test",
                        "--",
                        "true",
                    ],
                )
                .env(RUNS_VAR, &runs)
                .current_dir(&repo),
            )
        })
        .collect();
    for child in &mut started {
        assert!(child.wait().unwrap().success());
    }
    let lines = day_lines(&runs, 8);
    assert_eq!(lines.len(), 16);
    assert!(lines.iter().all(|line| line.label == label));
    assert_eq!(symphony_api::fold(lines).len(), 8);
}

#[test]
fn a_run_whose_recorder_died_is_ended_incomplete_by_the_next_recorders_sweep() {
    let case = Case::new();
    let folders = Folders::new("recorder-run-lost");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    let (release, pid) = (folders.dir.join("release"), folders.dir.join("pid"));
    let command = until_released(&release, &pid);
    let mut args = vec!["run", "--phase", "test", "--pipeline", "test", "--"];
    args.extend(command.iter().map(String::as_str));
    let mut started = spawn(
        tasks(&case, &folders, None, &args)
            .env(RUNS_VAR, &runs)
            .current_dir(&repo),
    );
    let running = folders.once(Step::Running);
    // Its start line follows its record's.
    let deadline = Instant::now() + Duration::from_secs(30);
    let start = loop {
        if let [start] = day_lines(&runs, 0).as_slice() {
            break start.clone();
        }
        assert!(Instant::now() < deadline, "no start line within 30 s");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(start.is_start() && start.started.is_some(), "{start:?}");
    assert_eq!(start.run, running.operation_id);
    let recorder = libc::pid_t::try_from(running.pid).unwrap();
    // SAFETY: the recorder of the operation this test started.
    assert_eq!(unsafe { libc::kill(recorder, libc::SIGKILL) }, 0);
    let deadline = Instant::now() + Duration::from_secs(30);
    while alive(recorder) {
        assert!(Instant::now() < deadline, "the recorder outlived its kill");
        std::thread::sleep(Duration::from_millis(20));
    }
    // The sweeping recorder's own lines go elsewhere; the lost run's end goes where its start went.
    let elsewhere = folders.dir.join("elsewhere");
    let done = output(
        tasks(
            &case,
            &folders,
            None,
            &[
                "run",
                "--phase",
                "build",
                "--pipeline",
                "build",
                "--",
                "true",
            ],
        )
        .env(RUNS_VAR, &elsewhere)
        .current_dir(&repo)
        .stdin(Stdio::null()),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    let [own] = symphony_api::fold(day_lines(&elsewhere, 1))
        .try_into()
        .unwrap();
    assert_eq!(own.pipeline, "build");
    let [lost] = symphony_api::fold(day_lines(&runs, 1)).try_into().unwrap();
    assert_eq!(lost.run, running.operation_id);
    assert_eq!(
        (lost.end, lost.exit, lost.signal, lost.cpu_s),
        (Some(End::Incomplete), None, None, None)
    );
    assert!(lost.ended.is_some());
    fs::write(&release, "").unwrap();
    assert!(started.wait().unwrap().success());
}
