//! `tasks run`, the binary itself: the command it wraps keeps its arguments, streams, session and
//! status, signals reach it, and its record in a work folder of the test's own names the task its
//! session holds on a board of the test's own while it runs, then how it ended and how long it took.

use crate::{Case, scratch};
use std::fs;
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use symphony_board::cli::machine_work::recorded::{self, Phase, Record, Step};
use symphony_board::sys::{Child, Command, Stdio, output, wait_output};

const MACHINE: &str = "0123456789abcdef0123456789abcdef";

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
        .env_remove(recorded::PARENT_VAR)
        .current_dir(&folders.dir);
    if let Some(session) = session {
        command.env("TASKS_SESSION", session);
    }
    command
}

/// A command that runs until `release` exists.
fn until_released(release: &Path) -> Vec<String> {
    [
        "sh",
        "-c",
        "while [ ! -e \"$1\" ]; do sleep 0.02; done",
        "sh",
        release.to_str().unwrap(),
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
    assert!(
        !folders
            .work
            .join(recorded::OPERATIONS)
            .join(format!("{}.lease", finished.operation_id))
            .exists(),
        "a finished operation lets its lease go"
    );
}

#[test]
fn a_command_ended_by_a_signal_ends_its_recorder_by_the_same_signal() {
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
fn a_signal_sent_to_the_recorder_reaches_its_command_whose_status_it_ends_with() {
    let case = Case::new();
    let folders = Folders::new("recorder-forwards");
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
            "trap 'exit 42' TERM; while :; do sleep 0.02; done",
        ],
    ));
    folders.once(Step::Running);
    let pid = libc::pid_t::try_from(recorder.id()).unwrap();
    // SAFETY: a signal to the recorder this test started and has not waited for.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
    let status = recorder.wait().unwrap();
    assert_eq!(status.code(), Some(42), "{status:?}");
    assert_eq!(folders.once(Step::Finished).exit_code, Some(42));
}

#[test]
fn a_command_that_cannot_start_is_said_and_recorded_as_a_shell_says_it_and_never_ran() {
    let case = Case::new();
    let folders = Folders::new("recorder-missing");
    let done = output(
        tasks(
            &case,
            &folders,
            None,
            &["run", "--phase", "build", "--", "no-such-command-anywhere"],
        )
        .stdin(Stdio::null()),
    )
    .unwrap();
    assert_eq!(done.status.code(), Some(127));
    assert!(done.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&done.stderr).starts_with("tasks run: no-such-command-anywhere: "),
        "{done:?}"
    );
    let finished = folders.once(Step::Finished);
    assert_eq!(finished.exit_code, Some(127));
    assert_eq!(finished.started_at, None, "it never ran");
    assert!(finished.reason.starts_with("its command did not start"));
}

#[test]
fn the_task_its_session_holds_is_its_work_while_it_runs_and_its_record_keeps_how_long_it_took() {
    let case = Case::new();
    let worker = case.register("worker");
    case.ok(&worker, &["create", "tools/recorded", "Recorded"]);
    case.ok(&worker, &["claim", "tools/recorded"]);
    let folders = Folders::new("recorder-claimed");
    let release = folders.dir.join("release");
    let command = until_released(&release);
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
    assert_eq!(recorded::history(&folders.work).unwrap(), [finished]);
}

#[test]
fn a_run_with_no_session_or_one_holding_several_tasks_records_no_task_and_one_named_records_it() {
    let case = Case::new();
    let worker = case.register("worker");
    for slug in ["tools/one", "tools/two"] {
        case.ok(&worker, &["create", slug, "Held"]);
        case.ok(&worker, &["claim", slug]);
    }
    for (name, session, task, expected) in [
        ("recorder-unattributed", None, None, (None, None)),
        (
            "recorder-ambiguous",
            Some(worker.as_str()),
            None,
            (Some(worker.as_str()), None),
        ),
        (
            "recorder-named",
            Some(worker.as_str()),
            Some("tools/two"),
            (Some(worker.as_str()), Some("tools/two")),
        ),
    ] {
        let folders = Folders::new(name);
        let mut args = vec!["run", "--phase", "build"];
        if let Some(task) = task {
            args.extend(["--task", task]);
        }
        args.extend(["--", "true"]);
        let done = output(tasks(&case, &folders, session, &args).stdin(Stdio::null())).unwrap();
        assert!(done.status.success(), "{name}: {done:?}");
        let finished = folders.once(Step::Finished);
        assert_eq!(
            (finished.handle.as_deref(), finished.task.as_deref()),
            expected,
            "{name}"
        );
    }
}

#[test]
fn a_board_that_cannot_be_reached_or_a_test_board_with_no_work_folder_still_runs_the_command() {
    let mut case = Case::new();
    case.env
        .vars
        .insert("TASKS_SERVER".into(), "127.0.0.1:1".into());
    let folders = Folders::new("recorder-unreachable");
    let began = Instant::now();
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
    assert!(
        began.elapsed() < Duration::from_secs(10),
        "{:?}",
        began.elapsed()
    );
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
            &["run", "--phase", "test", "--", "sh", "-c", "exit 4"],
        )
        .env_remove("TASKS_WORK")
        .stdin(Stdio::null()),
    )
    .unwrap();
    assert_eq!(unrecorded.status.code(), Some(4));
    assert!(unrecorded.stderr.is_empty(), "{unrecorded:?}");
    assert_eq!(folders.records().len(), 1, "nothing more was recorded");
}

#[test]
fn the_next_run_finishes_work_whose_recorder_died_and_keeps_both_in_the_history() {
    let case = Case::new();
    let folders = Folders::new("recorder-lost");
    let release = folders.dir.join("release");
    let command = until_released(&release);
    let mut lost = spawn(&mut tasks(
        &case,
        &folders,
        None,
        &run_args("test", "lost", &command),
    ));
    let running = folders.once(Step::Running);
    lost.kill().unwrap();
    lost.wait().unwrap();
    // Its command outlives it; a reader cannot say the work ended, nor that it runs.
    let found = recorded::read(&folders.work);
    assert!(found.records.is_empty());
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
    fs::write(&release, "").unwrap();
    let history = recorded::history(&folders.work).unwrap();
    assert_eq!(history.len(), 2, "{history:?}");
    assert_eq!(history[0].operation_id, running.operation_id);
    assert_eq!((history[0].exit_code, history[0].signal), (None, None));
    assert_eq!(history[0].state, Step::Finished);
    assert_eq!(history[1].exit_code, Some(0));
    assert!(!recorded::read(&folders.work).uncertain);
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
    let history = recorded::history(&folders.work).unwrap();
    let [inner, outer] = history.as_slice() else {
        panic!("{history:?}");
    };
    assert_eq!(
        (inner.label.as_str(), outer.label.as_str()),
        ("inner", "outer")
    );
    assert_eq!(
        inner.parent_id.as_deref(),
        Some(outer.operation_id.as_str())
    );
    assert_eq!(outer.parent_id, None);
}

#[test]
fn a_probe_answers_without_a_word_and_a_command_line_it_cannot_read_is_refused() {
    let case = Case::new();
    let folders = Folders::new("recorder-probe");
    let probe =
        output(tasks(&case, &folders, None, &["run", "--probe"]).stdin(Stdio::null())).unwrap();
    assert!(probe.status.success());
    assert!(probe.stdout.is_empty() && probe.stderr.is_empty());
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
