//! `tasks run`, the binary itself: the command it wraps keeps its arguments (any bytes), streams,
//! descriptors, ignored signals, signal mask, session and status, and is the process its caller
//! started; its record in a work folder of the test's own names the task its session holds on a
//! board of the test's own (or the one named) while it runs, then how it ended and how long it took.
//! The recorder finishes after its command does, so what it keeps is waited for.
//!
//! Every run here is on a scratch board, and takes [`SCALE`], or the largest scale where a board
//! that never answers must still be asked however late its end is stamped ([`ASKED`]) or a held
//! lock must not hold a run up ([`LOCK_SCALE`]): each wait of the recorder's a machine's speed
//! meets (its word that it watches its command, the board's answer, git's, and apart from its
//! command another run's end and the day files' lock) is that many times its own, so a loaded
//! machine never decides what a run records. What the scale would hide, a recorder grown slow, the
//! quickest of a test's several runs shows ([`QUICKEST_START`]). A wait of the tests' own is on its condition, with a backstop only a stall
//! reaches ([`patience`]), and one that gives up says what the work folder holds ([`holds`]): never
//! recorded, or recorded and never finished.

use crate::children::Child as _;
use crate::{Case, git, repository, scratch};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read as _;
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use symphony_api::{End, RunLine};
use symphony_board::cli::Env;
use symphony_board::cli::machine_work::recorded::{self, Phase, Record, Step};
use symphony_board::health::LIVE_SECONDS;
use symphony_board::stamps::read;
use symphony_board::sweep::trigger::now;
use symphony_board::sys::{Child, Command, Stdio, output, wait_output};

pub(crate) const MACHINE: &str = "0123456789abcdef0123456789abcdef";
const TASK_VAR: &str = "TASKS_TASK";
/// Names the day files' folder of a test's own.
pub(crate) const RUNS_VAR: &str = "SYMPHONY_RUNS";
/// Names a scale of the recorder's waits, which it takes on a scratch board alone.
pub(crate) const SCALE_VAR: &str = "TASKS_RUN_WAIT_SCALE";
/// Names a file the recorder makes once it has stamped its command's end.
const END_MARK_VAR: &str = "TASKS_RUN_END_MARK";
/// The tests' scale: 20 s for its recorder's word and the board's answer, 12 s for git's, 100 s for
/// an earlier run's end and 40 s for the day files' lock, far past what a loaded machine takes, and
/// inside the backstop each test waits for a record with ([`patience`]).
pub(crate) const SCALE: &str = "20";
/// A board that cannot be reached, which refuses at once, on a database of its own: a scratch
/// board, for a test that has none to give its runs [`SCALE`].
pub(crate) const UNREACHED: [(&str, &str); 2] = [
    ("TASKS_SERVER", crate::UNREACHED_SERVER),
    ("TASKS_DATABASE", "unreached"),
];

/// What the work folder `work` holds, for a wait that gave up to say: each operation's record as
/// its recorder last wrote it, read whether or not a live recorder still holds its lease (a reader
/// passes over an unfinished one nobody holds), each lease with no record, and how many operations
/// the history keeps. So a run never recorded (nothing at all, as when its command went on without
/// its recorder, which then let its lease go; or a lease with no record) is told from one recorded
/// and never finished.
pub(crate) fn holds(work: &Path) -> String {
    let operations = work.join(recorded::OPERATIONS);
    let names: Vec<String> = fs::read_dir(&operations)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    let ids: BTreeSet<&str> = names
        .iter()
        .filter_map(|name| {
            name.strip_suffix(".lease")
                .or_else(|| name.strip_suffix(".json"))
        })
        .collect();
    let mut said: Vec<String> = ids
        .iter()
        .map(|id| {
            let lease = match held(&operations.join(format!("{id}.lease"))) {
                Some(true) => "its lease held",
                Some(false) => "its lease let go",
                None => "no lease",
            };
            match fs::read(operations.join(format!("{id}.json"))) {
                Err(_) => format!("{id}: never recorded, {lease} and no record"),
                Ok(bytes) => match serde_json::from_slice::<Record>(&bytes) {
                    Ok(record) if record.state == Step::Finished => format!(
                        "{id}: finished (exit {:?}, signal {:?}, {:?}), {lease}",
                        record.exit_code, record.signal, record.reason
                    ),
                    Ok(record) => format!(
                        "{id}: recorded {:?} and never finished, {lease}",
                        record.state
                    ),
                    Err(error) => format!("{id}: a record that does not read ({error}), {lease}"),
                },
            }
        })
        .collect();
    if said.is_empty() {
        said.push(format!(
            "never recorded: no lease and no record in {}",
            operations.display()
        ));
    }
    let kept = recorded::history(work).map_or(0, |history| history.len());
    format!("{}; the history keeps {kept}", said.join("; "))
}

/// Whether the lease at `path` is locked by its recorder, which holds it while it records: none
/// when there is no lease to ask.
fn held(path: &Path) -> Option<bool> {
    let file = fs::File::open(path).ok()?;
    Some(matches!(
        file.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ))
}

/// Every line of the day files in `runs`, read once they hold `ends` end lines (and no more), in
/// the order each file holds them: each a whole line. A wait that gives up says what the work
/// folder `work` holds.
#[track_caller]
pub(crate) fn day_lines(work: &Path, runs: &Path, ends: usize) -> Vec<RunLine> {
    let mut last = Vec::new();
    let found = test_time::until(format!("{ends} runs ended"), patience(), || {
        let mut days: Vec<PathBuf> = fs::read_dir(runs)
            .map(|entries| {
                entries
                    .map(|entry| entry.unwrap().path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
                    .collect()
            })
            .unwrap_or_default();
        symphony_board::order::sort_by(&mut days, &Ord::cmp);
        let lines: Vec<RunLine> = days
            .iter()
            .flat_map(|day| {
                let text = day_text(day);
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
            return Some(lines);
        }
        last = lines;
        None
    });
    let lines = found
        .unwrap_or_else(|stall| panic!("{stall}: {last:#?}; the work folder: {}", holds(work)));
    let ended = lines.iter().filter(|line| line.end.is_some()).count();
    assert_eq!(ended, ends, "{lines:#?}");
    lines
}

/// What the day file at `day` holds: nothing once it is gone, as one listed may be by the time it
/// is read. A recorder apart from its command appends its lines, and prunes the old day files
/// after each, while its caller already reads what it appended (on macOS, once its command
/// ended), so a file of a day past [`symphony_board::runs::KEPT_DAYS`] goes between the listing
/// and the read, a day with no file to every reader of the runs ([`symphony_board::runs::rollup`]).
fn day_text(day: &Path) -> String {
    match fs::read_to_string(day) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        read => read.unwrap_or_else(|error| panic!("{}: {error}", day.display())),
    }
}

#[test]
fn a_day_file_gone_since_it_was_listed_reads_as_a_day_with_no_lines() {
    let dir = scratch("recorder-day-text");
    let day = dir.join("2000-01-01.jsonl");
    fs::write(&day, "{}\n").unwrap();
    assert_eq!(day_text(&day), "{}\n");
    fs::remove_file(&day).unwrap();
    assert_eq!(day_text(&day), "");
}

/// Only a file gone reads as no lines: any other error reading one is the test's to fail, naming
/// the path, here a folder where a day file is listed.
#[test]
#[should_panic(expected = "2000-01-01.jsonl: Is a directory")]
fn a_day_file_that_cannot_be_read_for_another_reason_fails_naming_it() {
    let day = scratch("recorder-day-unreadable").join("2000-01-01.jsonl");
    fs::create_dir(&day).unwrap();
    day_text(&day);
}

/// The backstop of the tests' own waits for what a run records: the 30 s they once gave it, as
/// `test_time::backstop` makes it (RULES.md `tests/no-test-fails-on-load`).
pub(crate) fn patience() -> Duration {
    test_time::backstop(Duration::from_secs(30))
}

/// What `git ARGS` in `dir` prints, trimmed.
pub(crate) fn git_says(dir: &Path, args: &[&str]) -> String {
    let done = output(Command::of("git").arg("-C").arg(dir).args(args)).unwrap();
    assert!(done.status.success(), "git {args:?}: {done:?}");
    String::from_utf8(done.stdout).unwrap().trim().to_string()
}

/// The history of the work folder `work` once it keeps `count` operations.
#[track_caller]
pub(crate) fn kept(work: &Path, count: usize) -> Vec<Record> {
    let found = test_time::until(format!("{count} operations kept"), patience(), || {
        let history = recorded::history(work).unwrap_or_default();
        (history.len() >= count).then_some(history)
    });
    let history = found.unwrap_or_else(|stall| {
        panic!(
            "{stall}: {:?}; the work folder: {}",
            recorded::history(work),
            holds(work)
        )
    });
    assert_eq!(history.len(), count, "{history:?}");
    history
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

    /// The one operation recorded, once it is in `state`; a wait that gives up says the records a
    /// reader found and what the work folder holds.
    #[track_caller]
    fn once(&self, state: Step) -> Record {
        let found = test_time::until(
            format!("one operation {state:?}"),
            patience(),
            || match self.records().as_slice() {
                [one] if one.state == state => Some(one.clone()),
                _ => None,
            },
        );
        found.unwrap_or_else(|stall| {
            panic!(
                "{stall}: {:?}; the work folder: {}",
                self.records(),
                holds(&self.work)
            )
        })
    }
}

/// The signals the recorder passes on, which a command it runs is ended by: each at its default
/// for a command these tests end by one, whatever the run of the tests inherited (a background
/// job inherits SIGINT and SIGQUIT ignored, and the recorder rightly leaves an ignored one so).
const ENDING: [libc::c_int; 4] = [libc::SIGHUP, libc::SIGINT, libc::SIGQUIT, libc::SIGTERM];

/// The `tasks` binary running `args` for `case`'s board, as `session` when one is given, recording
/// in `folders`, its waits at [`SCALE`], with each of [`ENDING`] at its default; standard streams
/// the test's to set.
fn tasks(case: &Case, folders: &Folders, session: Option<&str>, args: &[&str]) -> Command {
    let mut command = Command::of(env!("CARGO_BIN_EXE_tasks"));
    for signal in ENDING {
        command.default_signal(signal);
    }
    command
        .args(args)
        .envs(&case.env.vars)
        .env(SCALE_VAR, SCALE)
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
#[track_caller]
fn written_pid(file: &Path) -> libc::pid_t {
    test_time::until(format!("a pid in {}", file.display()), patience(), || {
        fs::read_to_string(file)
            .unwrap_or_default()
            .trim()
            .parse()
            .ok()
    })
    .unwrap()
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
    let gone = test_time::until_true("a finished operation lets its lease go", patience(), || {
        !lease.exists()
    });
    gone.unwrap_or_else(|stall| panic!("{stall}: {}", holds(&folders.work)));
}

/// A board server that takes one connection and never answers it: once it took it, it makes the
/// file `accepted`. It tells when it took it, then when its asker was let go: by
/// [`Silent::release`], or by the asker's own wait, which runs a minute in these tests
/// ([`ASKED`]), so that a loaded machine never lets it go first. Its socket is PostgreSQL's in a
/// folder of its own under `/tmp`, short enough for a socket's path, removed with it.
struct Silent {
    address: String,
    folder: PathBuf,
    heard: mpsc::Receiver<(f64, f64)>,
    held: Arc<Mutex<Option<UnixStream>>>,
    /// Set once its asker was let go, before it reads the time it tells for that.
    let_go: Arc<AtomicBool>,
}

impl Drop for Silent {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.folder);
    }
}

impl Silent {
    fn new(accepted: &Path) -> Silent {
        static MADE: AtomicUsize = AtomicUsize::new(0);
        let folder = PathBuf::from(format!(
            "/tmp/symphony-silent-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let socket = folder.join(symphony_board::server::postgres::SOCKET_FILE);
        let listener = UnixListener::bind(&socket).unwrap();
        let address = format!("postgresql:{}", folder.display());
        let (tell, heard) = crate::test_channel::channel();
        let held = Arc::new(Mutex::new(None));
        let holder = Arc::clone(&held);
        let let_go = Arc::new(AtomicBool::new(false));
        let gone = Arc::clone(&let_go);
        let accepted = accepted.to_path_buf();
        std::thread::spawn(move || {
            let (mut asked, _) = listener.accept().unwrap();
            let taken = now();
            *holder.lock().unwrap() = Some(asked.try_clone().unwrap());
            fs::write(&accepted, "").unwrap();
            let mut said = [0; 4096];
            while matches!(asked.read(&mut said), Ok(read) if read > 0) {}
            gone.store(true, Ordering::SeqCst);
            let _ = tell.send((taken, now()));
        });
        Silent {
            address,
            folder,
            heard,
            held,
            let_go,
        }
    }

    /// Whether it holds the question it took, its asker not let go: the time it will tell for
    /// that is read after this.
    fn holds(&self) -> bool {
        self.held.lock().unwrap().is_some() && !self.let_go.load(Ordering::SeqCst)
    }

    /// The question it took closed unanswered, which lets its asker go: called once `accepted`
    /// exists.
    fn release(&self) {
        if let Some(asked) = &*self.held.lock().unwrap() {
            let _ = asked.shutdown(Shutdown::Both);
        }
    }

    /// When it took the question, then when its asker was let go.
    fn heard(&self) -> (f64, f64) {
        self.heard.recv_timeout(asked_patience()).unwrap()
    }
}

/// The backstop of a wait for a run that asks a board that never answers: the minute its asker is
/// given ([`ASKED`]), as `test_time::backstop` makes it.
fn asked_patience() -> Duration {
    test_time::backstop(Duration::from_secs(60))
}

/// A command that runs until `accepted` exists: until the board is being asked its task.
fn until_asked(accepted: &Path) -> Vec<String> {
    [
        "/bin/sh",
        "-c",
        "while [ ! -e \"$1\" ]; do :; done",
        "sh",
        accepted.to_str().unwrap(),
    ]
    .map(str::to_string)
    .to_vec()
}

/// The scale of a run that asks a board that never answers: the most a scale takes, a minute for
/// the board's own second, which the test cuts short once the recorder stamped its command's end
/// ([`released_once_stamped`]). So the board is still asked however late a loaded machine lets the
/// recorder stamp that end.
const ASKED: &str = "60";

/// Once its recorder made `mark` ([`END_MARK_VAR`]), having stamped its command's end, `board`
/// still holds the question it took, and is then made to let it go: whatever order a loaded
/// machine runs the recorder's threads and the test's in, the end was stamped while the board was
/// asked, and before the time the board tells for letting its asker go. A recorder that stamped
/// the end only once the board let its asker go would make the mark only once the asker's own
/// minute ran out ([`ASKED`]), the question let go, or stamp the end after that time.
fn released_once_stamped(mark: &Path, board: &Silent) {
    test_time::until_true(
        "its recorder stamped its command's end",
        asked_patience(),
        || mark.exists(),
    )
    .unwrap();
    assert!(
        board.holds(),
        "the board let its asker go before the recorder stamped its command's end"
    );
    board.release();
}

/// Once this process's child `pid` has ended, which is left unreaped for its own caller to reap.
fn until_ended(pid: libc::pid_t) {
    test_time::until_true(format!("its command {pid} ended"), patience(), || {
        let seen = ended(pid);
        assert_ne!(seen, None, "reaped before it was seen ended");
        seen == Some(true)
    })
    .unwrap();
}

/// `finished`, run while the board took the question of its task at `taken` and let its asker go
/// at `closed`, once its recorder stamped its command's end ([`released_once_stamped`]), was
/// stamped as it ran, not once the board let go: asked for after `called`, started before the
/// board was asked, ended after that (its command waited for it) and before the board let go; for
/// its session, and no task. A recorder that stamped its end once the board let its asker go would
/// stamp it after `closed`.
#[track_caller]
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
        "the board asked at {taken}, let go at {closed}: {finished:?}"
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
    let command = until_asked(&accepted);
    let mark = folders.dir.join("stamped");
    let mut args = vec!["run", "--phase", "test", "--"];
    args.extend(command.iter().map(String::as_str));
    let called = now();
    let started = tasks(&case, &folders, Some("asks-a-silent-board"), &args)
        .env(SCALE_VAR, ASKED)
        .env(END_MARK_VAR, &mark)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // On macOS the binary becomes its command, watched by a recorder of its own; elsewhere it is
    // its command's parent. Either stamps the command's end while it still asks the board, which
    // then lets it go.
    released_once_stamped(&mark, &board);
    let done = wait_output(started).unwrap();
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
    let mark = folders.dir.join("stamped");
    for (name, value) in [
        ("TASKS_SERVER", board.address.as_str()),
        ("TASKS_SESSION", "asks-a-silent-board"),
        ("TASKS_WORK", folders.work.to_str().unwrap()),
        ("SYMPHONY_MACHINE_DIR", folders.machine.to_str().unwrap()),
        (SCALE_VAR, ASKED),
        (END_MARK_VAR, mark.to_str().unwrap()),
    ] {
        env.vars.insert(name.into(), value.into());
    }
    env.cwd.clone_from(&folders.dir);
    let mut args = ["run", "--phase", "test", "--"]
        .map(str::to_string)
        .to_vec();
    args.extend(until_asked(&accepted));
    let called = now();
    let (tell, done) = crate::test_channel::channel();
    std::thread::spawn(move || {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let _ = tell.send(crate::run(&env, &args, ""));
    });
    // Its command is this process's child, whose end its waiter stamps while the board is asked.
    released_once_stamped(&mark, &board);
    let ran = done.recv_timeout(asked_patience()).unwrap();
    assert_eq!(ran.code, 0, "{}{}", ran.out, ran.err);
    stamped_while_asked(&folders.once(Step::Finished), called, board.heard());
}

/// In its caller's process the command is a child built from the caller's `Env`, and still the
/// caller's own: it keeps the caller's session and the marks of how the caller was asked for
/// (`sys::MARKS`), which no child the board starts keeps, as the `tasks` binary does on macOS by
/// becoming the command.
#[test]
fn in_its_callers_process_the_command_keeps_its_callers_session_and_marks() {
    let case = Case::new();
    let folders = Folders::new("recorder-marks-in-process");
    let heard = folders.dir.join("heard");
    let mut env = case.env.clone();
    for (name, value) in [
        ("TASKS_SERVER", crate::UNREACHED_SERVER),
        ("TASKS_SESSION", "worker"),
        ("TASKS_WORK", folders.work.to_str().unwrap()),
        ("SYMPHONY_MACHINE_DIR", folders.machine.to_str().unwrap()),
        ("SYMPHONY_UNASKED", "1"),
        ("SYMPHONY_LAND", "1"),
    ] {
        env.vars.insert(name.into(), value.into());
    }
    env.cwd.clone_from(&folders.dir);
    let ran = crate::run(
        &env,
        &[
            "run",
            "--phase",
            "test",
            "--",
            "sh",
            "-c",
            "printf '%s|%s|%s' \"${TASKS_SESSION-unset}\" \"${SYMPHONY_UNASKED-unset}\" \
             \"${SYMPHONY_LAND-unset}\" > \"$1\"",
            "sh",
            heard.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(ran.code, 0, "{}{}", ran.out, ran.err);
    assert_eq!(fs::read_to_string(&heard).unwrap(), "worker|1|1");
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
        (SCALE_VAR, ASKED),
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
    let (tell, done) = crate::test_channel::channel();
    std::thread::spawn(move || {
        let args = args.each_ref().map(String::as_str);
        let _ = tell.send(crate::run(&env, &args, ""));
    });
    let pid = written_pid(&pid);
    until_ended(pid);
    // The board lets its asker go only now: until then it is asked, however long the command took.
    assert!(
        board.heard.try_recv().is_err(),
        "the board was no longer asked once it ended"
    );
    board.release();
    let ran = done.recv_timeout(asked_patience()).unwrap();
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
    // A run's own: its stdout and stderr go through its recorder, and nothing else does.
    let piped = Folders::new("recorder-inherited-run");
    let passed = folders.dir.join("passed");
    let script =
        format!("echo through >&{HANDED}; kill -HUP $$; kill -INT $$; kill -USR1 $$; echo alive");
    let run = |wrapped: Option<&Folders>| {
        let file = fs::File::create(&passed).unwrap();
        let mut command = if let Some(folders) = wrapped {
            let mut command = std::process::Command::of(env!("CARGO_BIN_EXE_tasks"));
            command.args(["run", "--phase", "test"]);
            if std::ptr::eq(folders, &piped) {
                command
                    .args(["--pipeline", "test"])
                    .env(RUNS_VAR, folders.dir.join("runs"));
            } else {
                command.env_remove(RUNS_VAR);
            }
            command
                .args(["--", "sh"])
                .envs(&case.env.vars)
                .env(SCALE_VAR, SCALE)
                .env("TASKS_WORK", &folders.work)
                .env("SYMPHONY_MACHINE_DIR", &folders.machine)
                .env_remove("TASKS_SESSION")
                .env_remove(TASK_VAR)
                .env_remove(recorded::PARENT_VAR)
                .env_remove(recorded::PIPELINE_VAR);
            command
        } else {
            std::process::Command::of("sh")
        };
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
    let (direct, through) = run(None);
    assert_eq!(
        (
            direct.status.code(),
            direct.stdout.as_slice(),
            through.as_str()
        ),
        (Some(0), &b"alive\n"[..], "through\n"),
        "{direct:?}"
    );
    for recorded in [&folders, &piped] {
        let (wrapped, through) = run(Some(recorded));
        assert_eq!(
            (
                wrapped.status.code(),
                wrapped.stdout.as_slice(),
                through.as_str()
            ),
            (Some(0), &b"alive\n"[..], "through\n"),
            "{wrapped:?}"
        );
        assert_eq!(recorded.once(Step::Finished).exit_code, Some(0));
    }
    // Only the run kept its output.
    let [line] = symphony_api::fold(day_lines(&piped.work, &piped.dir.join("runs"), 1))
        .try_into()
        .unwrap();
    let log = line.log.expect("a run's log");
    assert_eq!(
        fs::read(piped.dir.join("runs").join(log)).unwrap(),
        b"alive\n"
    );
    assert!(!folders.dir.join("runs").exists());
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
    test_time::until_true("the command set its trap", patience(), || trapped.exists()).unwrap();
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

/// Where the process `pid` works, as the kernel says it: what `lsof` lists as its `cwd`.
#[cfg(target_os = "macos")]
fn works_in(pid: libc::pid_t) -> PathBuf {
    use std::os::unix::ffi::OsStrExt as _;
    // SAFETY: a struct of plain numbers and bytes, which all zeroes is a value of.
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_vnodepathinfo>()).unwrap();
    // SAFETY: writes at most `size` bytes into `info`, which is that large.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    assert_eq!(read, size, "the kernel does not say where {pid} works");
    // SAFETY: the kernel wrote a NUL-terminated path into it.
    let path = unsafe { std::ffi::CStr::from_ptr(info.pvi_cdir.vip_path.as_ptr().cast()) };
    PathBuf::from(std::ffi::OsStr::from_bytes(path.to_bytes()))
}

/// A recorder works at the root while its command works in its caller's folder: whoever asks which
/// processes work in a checkout (a release run's look after a check, the worktree sweep, a land)
/// finds the command and never its recorder, however long that takes to write down how it ended.
/// What it records is still its command's folder, and a work folder named from that folder is
/// still found from it.
#[cfg(target_os = "macos")]
#[test]
fn its_recorder_works_out_of_its_callers_folder_and_records_from_it_all_the_same() {
    let case = Case::new();
    let mut folders = Folders::new("recorder-elsewhere");
    folders.work = folders.dir.join("named/from/here");
    let (release, pid) = (folders.dir.join("release"), folders.dir.join("pid"));
    let command = until_released(&release, &pid);
    let mut started = spawn(
        tasks(
            &case,
            &folders,
            None,
            &run_args("test", "elsewhere", &command),
        )
        .env("TASKS_WORK", "named/from/here"),
    );
    let running = folders.once(Step::Running);
    let here = folders.dir.canonicalize().unwrap();
    assert_eq!(works_in(written_pid(&pid)).canonicalize().unwrap(), here);
    let recorder = libc::pid_t::try_from(running.pid).unwrap();
    assert_eq!(
        works_in(recorder),
        Path::new("/"),
        "its recorder works in its command's folder"
    );
    assert_eq!(Path::new(&running.checkout).canonicalize().unwrap(), here);
    fs::write(&release, "").unwrap();
    assert!(started.wait().unwrap().success());
    assert_eq!(folders.once(Step::Finished).exit_code, Some(0));
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
    let beaten = test_time::until("a beat after it started", patience(), || {
        let now = folders.once(Step::Running);
        (now.beat_at > running.beat_at).then_some(now)
    })
    .unwrap_or_else(|stall| panic!("{stall}: {}", holds(&folders.work)));
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
/// The hook says nothing of a beat it gave up on, so the board is read for what they left: the
/// handle bound to that key, and a subagent placed in `cwd` (a full session's binding keeps no
/// checkout).
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
    let bound = case.store().agent_bindings().unwrap().remove(handle);
    let left = bound.map(|bound| (bound.agent, bound.worktree));
    let worktree = agent.map_or("", |_| cwd.to_str().unwrap());
    assert_eq!(
        left,
        Some((agent.unwrap_or(claude).to_owned(), worktree.to_owned())),
        "{handle}'s binding"
    );
}

/// What a run of `true` with `vars`, in `cwd`, records on `case`'s board, once it finished.
#[track_caller]
fn run_in(case: &Case, name: &str, cwd: &Path, vars: &[(&str, &str)]) -> Record {
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
    folders.once(Step::Finished)
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

    /// A run in `cwd` with `vars` records `expected`, its handle and its task, for the reason `why`:
    /// its record.
    #[track_caller]
    fn record(
        &self,
        why: &str,
        cwd: &Path,
        vars: &[(&str, &str)],
        expected: (Option<&str>, Option<&str>),
    ) -> Record {
        let at = self.made.replace(self.made.get() + 1);
        let found = run_in(self.case, &format!("{}-{at}", self.prefix), cwd, vars);
        assert_eq!(
            (found.handle.as_deref(), found.task.as_deref()),
            expected,
            "{why}: in {} with {vars:?}",
            cwd.display()
        );
        found
    }
}

const CLAUDE: &str = "CLAUDE_CODE_SESSION_ID";

/// With no task named, a run's task is, in this order: the given session's; the one live session
/// the beat hook placed in its linked worktree; its branch's when that is a task's slug, whatever
/// its status; the live full session its Claude Code session binds (a subagent's is its parent's,
/// so this never beats the branch); none. Each earlier one wins over a later one that says otherwise, and the
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
    runs.record(
        "a cancelled task's branch is still its task's",
        &gone,
        &[],
        (None, Some("tools/branch-gone")),
    );
    runs.record(
        "a closed task's branch beats the Claude Code session's",
        &gone,
        &[(CLAUDE, PARENT)],
        (None, Some("tools/branch-gone")),
    );
    runs.record("a branch no task has is none", &unknown, &[], (None, None));
}

/// An approve leaves a task done, and its executor holds no done task: a land that conflicts sends
/// it back to its branch, to merge main and run its tests at the merged tip, as the land's own
/// conflict line says. What it runs there is the task's by its branch, with nothing named, and
/// stands as the `tests` stamp at the merged tip by the reader the land and the task page read;
/// what the same session runs in another worktree is not the task's.
#[test]
fn runs_on_an_approved_tasks_branch_after_a_land_conflict_are_its_tests_at_the_merged_tip() {
    const SLUG: &str = "tools/approved";
    const CLAUDE_EXE: &str = "claude-executor-parent";
    let case = Case::new();
    let [orch, exe, reviewer] =
        ["orchestrator", "worker", "reviewer"].map(|kind| case.register(kind));
    case.ok(
        &orch,
        &["create", SLUG, "Approved, then conflicting at its land"],
    );
    case.ok(&exe, &["claim", SLUG]);
    case.ok(&exe, &["move", SLUG, "review"]);
    case.ok(&reviewer, &["role", SLUG, "reviewer"]);
    case.ok(&reviewer, &["verdict", SLUG, "approve", "fine"]);
    let shown = case.ok("", &["show", SLUG]);
    assert!(shown.lines().next().unwrap().contains("[done,"), "{shown}");
    let (main, [branch, elsewhere]) = worktrees("recorder-approved", [SLUG, "tools/no-such-task"]);
    // The task's work on its branch, approved there; then main moves, and the land conflicts: the
    // executor merges main into the branch.
    git(
        &branch,
        &["commit", "-q", "--allow-empty", "-m", "the work"],
    );
    let approved = git_says(&branch, &["rev-parse", "HEAD"]);
    git(
        &main,
        &["commit", "-q", "--allow-empty", "-m", "main moved"],
    );
    git(&branch, &["merge", "-q", "--no-edit", "main"]);
    let merged = git_says(&branch, &["rev-parse", "HEAD"]);
    assert_ne!(merged, approved);
    bind(&case, &exe, Some("agent-executor"), CLAUDE_EXE, &branch);
    let runs = Runs::on(&case, "recorder-approved-run");
    let ours = runs.record(
        "the executor placed on the approved task's branch, holding no task: the branch's",
        &branch,
        &[(CLAUDE, CLAUDE_EXE)],
        (Some(exe.as_str()), Some(SLUG)),
    );
    assert_eq!(ours.commit.as_deref(), Some(merged.as_str()));
    let tests = read::tests(std::slice::from_ref(&ours), &merged, |task| task == SLUG);
    assert!(tests.met, "{tests:?}");
    assert_eq!(tests.by.as_deref(), Some(exe.as_str()));
    assert_eq!(tests.commit.as_deref(), Some(merged.as_str()));
    // Its session, placed in another worktree, holds nothing there: that run is no task's, and
    // stands as none of the approved task's.
    bind(&case, &exe, Some("agent-executor"), CLAUDE_EXE, &elsewhere);
    let theirs = runs.record(
        "the same session in another worktree: no task's",
        &elsewhere,
        &[(CLAUDE, CLAUDE_EXE)],
        (Some(exe.as_str()), None),
    );
    let at_main = theirs.commit.clone().unwrap();
    let tests = read::tests(&[theirs], &at_main, |task| task == SLUG);
    assert!(!tests.met, "{tests:?}");
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
        .insert("TASKS_SERVER".into(), crate::UNREACHED_SERVER.into());
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
    test_time::until_true("the recorder gone after its kill", patience(), || {
        !alive(recorder)
    })
    .unwrap();
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

/// Past the lost line.
const GONE: f64 = crate::common::GONE + 100.0;

/// The health the board reads of `handle` by `case`'s clock.
fn health(case: &Case, handle: &str) -> &'static str {
    case.store().session(handle).unwrap().health.as_str()
}

/// `handle` as the board last saw it [`GONE`] ago, whatever beat for it since.
fn unseen(case: &Case, handle: &str) {
    let then = crate::common::NOW - GONE;
    let store = case.store();
    let row = store.raw().row("session", handle);
    row.set(serde_json::json!({"last_seen": then})).unwrap();
}

/// `handle` once the board reads it live by `case`'s clock: a wait on the recorder's word, which
/// comes within its own twenty seconds, with a backstop only a stall reaches.
#[track_caller]
fn until_live(case: &Case, handle: &str, work: &Path) {
    let backstop = test_time::backstop(Duration::from_secs(120));
    test_time::until_true(format!("{handle} read live"), backstop, || {
        health(case, handle) == "live"
    })
    .unwrap_or_else(|stall| {
        panic!(
            "{stall}: it reads {}; {}",
            health(case, handle),
            holds(work)
        )
    });
}

/// An orchestrator, and two workers the board last saw [`GONE`] ago, each executing a task of its
/// own: the orchestrator, the workers and their tasks.
fn two_gone_quiet(case: &Case) -> (String, [(String, &'static str); 2]) {
    let then = ago(case, GONE);
    let orchestrator = case.register("orchestrator");
    let workers = ["tools/quiet", "tools/beside"].map(|slug| {
        let worker = then.register("worker");
        then.ok(&worker, &["create", slug, "Quiet"]);
        then.ok(&worker, &["claim", slug]);
        (worker, slug)
    });
    (orchestrator, workers)
}

/// The tasks a PULL lists orphaned for `orchestrator`, by `case`'s clock.
fn orphaned(case: &Case, orchestrator: &str) -> Vec<String> {
    let said = case.ok(orchestrator, &["pull"]);
    said.lines()
        .filter_map(|line| line.trim_start().strip_prefix("in_progress_orphaned "))
        .filter_map(|rest| rest.split_whitespace().nth(1).map(str::to_owned))
        .collect()
}

/// The health `slug`'s executor reads on the task itself, by `case`'s clock.
fn executor(case: &Case, slug: &str) -> &'static str {
    let task = case.store().get(slug).unwrap();
    let role = task.roles.iter().find(|role| role.role == "executor");
    role.unwrap().health.as_str()
}

/// An agent waiting on its own build or test makes no tool call. While the run its record names
/// it for goes on, its recorder says it is there: it reads live, on its task too, and a PULL does
/// not list its task orphaned, however long ago its last call was. Another session the beat hook
/// placed in the same linked worktree, which the record does not name, is kept by nothing. From
/// the run's end it ages as from any last sign of life: live, then stale, then lost.
#[test]
fn a_session_reads_live_while_its_own_recorded_run_goes_on_and_ages_from_its_end() {
    let case = Case::new();
    let (orchestrator, [(worker, quiet), (other, beside)]) = two_gone_quiet(&case);
    // Both placed by their last tool calls, long ago, in the one checkout the run works in.
    let (_, [shared]) = worktrees("recorder-session-seen-tree", ["tools/shared"]);
    let then = ago(&case, GONE);
    bind(
        &then,
        &worker,
        Some("agent-worker"),
        "claude-parent",
        &shared,
    );
    bind(&then, &other, Some("agent-other"), "claude-parent", &shared);
    assert_eq!(
        (health(&case, &worker), health(&case, &other)),
        ("lost", "lost")
    );
    assert_eq!(orphaned(&case, &orchestrator), [beside, quiet]);
    let folders = Folders::new("recorder-session-seen");
    let (release, pid) = (folders.dir.join("release"), folders.dir.join("pid"));
    let command = until_released(&release, &pid);
    let mut recorder = spawn(
        tasks(
            &case,
            &folders,
            Some(&worker),
            &run_args("test", "long", &command),
        )
        .current_dir(&shared),
    );
    let running = folders.once(Step::Running);
    assert_eq!(running.handle.as_deref(), Some(worker.as_str()));
    until_live(&case, &worker, &folders.work);
    assert_eq!(executor(&case, quiet), "live");
    assert_eq!(orphaned(&case, &orchestrator), [beside]);
    assert_eq!(
        (health(&case, &other), executor(&case, beside)),
        ("lost", "lost")
    );
    // Its end is said too, whenever it comes: seen long ago again, it is seen as the run ends.
    unseen(&case, &worker);
    fs::write(&release, "").unwrap();
    assert!(recorder.wait().unwrap().success());
    folders.once(Step::Finished);
    until_live(&case, &worker, &folders.work);
    // Nothing says it after that: it ages from the run's end.
    let later = |seconds: f64| ago(&case, -seconds);
    assert_eq!(health(&later(LIVE_SECONDS), &worker), "live");
    assert_eq!(health(&later(LIVE_SECONDS + 1.0), &worker), "stale");
    let gone = later(crate::common::GONE);
    assert_eq!(
        (health(&gone, &worker), executor(&gone, quiet)),
        ("lost", "lost")
    );
    assert_eq!(orphaned(&gone, &orchestrator), [beside, quiet]);
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
    places: (&Path, &Path),
    names: (&str, &str),
    script: &str,
) -> std::process::ExitStatus {
    let done = output(pipeline_command(case, folders, places, names, script).stdin(Stdio::null()))
        .unwrap();
    done.status
}

/// The command [`pipeline_run`] runs.
fn pipeline_command(
    case: &Case,
    folders: &Folders,
    (repo, runs): (&Path, &Path),
    (pipeline, label): (&str, &str),
    script: &str,
) -> Command {
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
    let mut command = tasks(case, folders, None, &args);
    command.env(RUNS_VAR, runs).current_dir(repo);
    command
}

/// The most the quickest of a test's runs may take from being asked for to its command starting
/// (`started - queued`, which its record holds as `started_at - queued_at`). On macOS that is the
/// span a command waits, for READY (the product's 1 s, which a test cannot read), to hear its
/// recorder watches it before it goes on unrecorded; this is half of it. The tests' [`SCALE`] lets
/// a recorder grown slow still record, so this is what turns one red. Spans measured: 2 ms to 87 ms
/// at loads of 73 to 120 on 16 cores.
const QUICKEST_START: Duration = Duration::from_millis(500);

/// That the quickest of the runs `lines` fold to started within [`QUICKEST_START`] of being asked
/// for: one run stalled, or several, never fails it; a recorder slow on every run does. A failure
/// says each run's span.
#[track_caller]
fn one_started_promptly(lines: &[RunLine]) {
    let spans: Vec<f64> = lines
        .iter()
        .filter_map(|line| Some(line.started? - line.queued))
        .collect();
    let quickest = spans.iter().copied().reduce(f64::min);
    assert!(
        quickest.is_some_and(|quickest| quickest < QUICKEST_START.as_secs_f64()),
        "none of {} runs started within {QUICKEST_START:?} of being asked for (READY is 1 s): each \
         one's span, in seconds: {spans:?}",
        lines.len()
    );
}

/// `case`'s environment for `tasks run` in this process, at top level in `repo`, recording in
/// `folders` and writing its lines to `runs`, its waits at [`SCALE`].
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
    env.vars.insert(SCALE_VAR.into(), SCALE.into());
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
    let red = symphony_api::fold(day_lines(&folders.work, &runs, 1)).remove(0);
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
    let lines = symphony_api::fold(day_lines(&folders.work, &runs, 2));
    let green = &lines[1];
    assert_eq!((green.end, green.exit), (Some(End::Green), Some(0)));
    assert_eq!(green.rerun_of.as_deref(), Some(red.run.as_str()));
    assert!(green.cpu_s.is_some_and(|cpu| cpu >= 0.05), "{green:?}");
    // Another pipeline, or another command of the same one, repeats none.
    assert!(labelled("lint", "just test-rs -p a", "true").success());
    assert!(labelled("test", "just test-rs -p b", "true").success());
    let lines = symphony_api::fold(day_lines(&folders.work, &runs, 4));
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
    let lines = symphony_api::fold(day_lines(&folders.work, &runs, 5));
    let interrupted = &lines[4];
    assert_eq!(
        (interrupted.end, interrupted.exit, interrupted.signal),
        (Some(End::Interrupted), None, Some(libc::SIGINT))
    );
    assert_eq!(interrupted.rerun_of.as_deref(), Some(green.run.as_str()));
    // A checkout that differs from its commit is dirty, and repeats nothing.
    fs::write(repo.join("untracked"), "x").unwrap();
    assert!(ran("test", "true").success());
    let lines = symphony_api::fold(day_lines(&folders.work, &runs, 6));
    assert_eq!(
        (lines[5].dirty, lines[5].rerun_of.as_deref()),
        (Some(true), None)
    );
    // Committed, it is a new tree, which no run checked before.
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "untracked"]);
    assert!(ran("test", "true").success());
    let lines = symphony_api::fold(day_lines(&folders.work, &runs, 7));
    assert_eq!(
        (lines[6].dirty, lines[6].rerun_of.as_deref()),
        (Some(false), None)
    );
    assert_ne!(lines[6].tree.as_deref(), Some(tree.as_str()));
    // Each run wrote its start line, then its end line.
    let all = day_lines(&folders.work, &runs, 7);
    assert_eq!(all.len(), 14, "{all:#?}");
    for line in &lines {
        let starts = all
            .iter()
            .filter(|each| each.run == line.run && each.is_start());
        assert_eq!(starts.count(), 1, "{}", line.run);
    }
    // Seven runs one after another, with git, over the test's seconds: its recorder is prompt.
    one_started_promptly(&lines);
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
    let mut lines = symphony_api::fold(day_lines(&folders.work, &runs, 4));
    symphony_board::order::sort_by(&mut lines, &|one, other| {
        one.queued.total_cmp(&other.queued)
    });
    assert_eq!(lines[0].rerun_of, None);
    for pair in lines.windows(2) {
        assert_eq!(
            pair[1].rerun_of.as_deref(),
            Some(pair[0].run.as_str()),
            "{lines:#?}"
        );
    }
    one_started_promptly(&lines);
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
    let [made] = symphony_api::fold(day_lines(&folders.work, &runs, 1))
        .try_into()
        .unwrap();
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
    let lines = symphony_api::fold(day_lines(&folders.work, &runs, 2));
    let child = lines.iter().find(|line| line.run != made.run).unwrap();
    assert_eq!(child.dirty, Some(false), "{child:?}");
    assert_eq!(child.rerun_of.as_deref(), Some(made.run.as_str()));
    // Its count is the reap's, which watches for no process that left: no word either way.
    assert!(child.cpu_s.is_some());
    assert_eq!(child.cpu_partial, None);
}

/// The scale of the case below: the most a scale takes, so a recorder apart from its caller would
/// wait two minutes for the day files' lock before each line (`runs::LOCK_WAIT`, sixty times),
/// which no loaded machine reaches with a recorder that never waits so.
const LOCK_SCALE: u32 = 60;

#[test]
fn in_a_callers_process_a_held_lock_never_holds_up_the_status() {
    let case = Case::new();
    let folders = Folders::new("recorder-run-held-lock");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    fs::create_dir_all(&runs).unwrap();
    let holder = fs::File::create(runs.join("lock")).unwrap();
    holder.lock().unwrap();
    // A recorder apart from its caller waits for the lock before each line, two minutes here: one
    // that ends sooner, the lock held throughout, never waited so.
    let mut env = in_process(&case, &folders, (&repo, &runs));
    env.vars.insert(SCALE_VAR.into(), LOCK_SCALE.to_string());
    let began = Instant::now();
    let ran = crate::run(
        &env,
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
    assert!(
        took < symphony_board::runs::LOCK_WAIT * LOCK_SCALE,
        "waited for the held lock as a recorder apart from its caller does: {took:?}"
    );
    // Both lines were appended still, each whole, without the lock.
    let [ended] = symphony_api::fold(day_lines(&folders.work, &runs, 1))
        .try_into()
        .unwrap();
    assert_eq!((ended.end, ended.exit), (Some(End::Red), Some(7)));
    assert_eq!(day_lines(&folders.work, &runs, 1).len(), 2);
    holder.unlock().unwrap();
}

#[test]
fn in_a_callers_process_a_run_whose_command_cannot_start_keeps_no_log() {
    let case = Case::new();
    let folders = Folders::new("recorder-run-unstarted-log");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    let env = in_process(&case, &folders, (&repo, &runs));
    let missing = folders.dir.join("no-such-command");
    let missing = missing.to_str().unwrap();
    let asked = [
        "run",
        "--phase",
        "test",
        "--pipeline",
        "test",
        "--",
        missing,
    ];
    let ran = crate::run(&env, &asked, "");
    assert_eq!(ran.code, 127, "{}{}", ran.out, ran.err);
    // Its log was made before its command was tried, and went when it could not start.
    let [ended] = symphony_api::fold(day_lines(&folders.work, &runs, 1))
        .try_into()
        .unwrap();
    assert_eq!(
        (ended.started, ended.log, ended.log_bytes),
        (None, None, None)
    );
    let kept: Vec<PathBuf> = fs::read_dir(runs.join("logs"))
        .into_iter()
        .flatten()
        .flat_map(|day| fs::read_dir(day.unwrap().path()).unwrap())
        .map(|log| log.unwrap().path())
        .collect();
    assert!(kept.is_empty(), "{kept:?}");
}

/// The first reading a beat wrote into the running record of the operation labelled `label` that
/// read `processes` processes or more; a wait that gives up says the records a reader found and
/// what the work folder holds.
#[cfg(target_os = "macos")]
#[track_caller]
fn read_at_least(folders: &Folders, label: &str, processes: u32) -> recorded::CpuReading {
    let what = format!("a reading of {processes} processes of {label}");
    test_time::until(what, patience(), || {
        (folders.records().into_iter())
            .filter(|record| record.label == label && record.state == Step::Running)
            .find_map(|record| record.reading.filter(|read| read.processes >= processes))
    })
    .unwrap_or_else(|stall| {
        panic!(
            "{stall}: {:?}; the work folder: {}",
            folders.records(),
            holds(&folders.work)
        )
    })
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
    // released once the recorder's last reading is written. It ends too once the test's folder is
    // gone, which a release written just before the folder's removal would leave it never to see,
    // and by itself after six minutes of its looks, should the test be killed before either. It
    // holds none of the streams the test reads to their end. The command then reads its stdin,
    // starting no other process, and ends once the test closes it: once a reading its recorder
    // wrote with a beat read two processes, the command and that child under it. However late a
    // loaded machine lets the recorder look, the child is read before it leaves; a test that ends
    // before closes it all the same.
    let leaves = format!(
        "(i=0; while [ -d '{}' ] && [ ! -e '{}' ] && [ $i -lt 7200 ]; do sleep 0.05; \
         i=$((i + 1)); done) </dev/null >/dev/null 2>&1 & read -r line; :",
        folders.dir.display(),
        release.0.display()
    );
    let mut leaving =
        pipeline_command(&case, &folders, (&repo, &runs), ("test", "leaves"), &leaves)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
    // Plain runs at once, each reaped by the test only once every recorder wrote its end line: the
    // last reading of each is of its command ended and not yet reaped, however late a loaded
    // machine let its recorder look, so nothing is missed. What a caller that reaps first leaves a
    // count, the unit tests of `cpu::Tree::reaped_first` hold.
    const PLAIN: usize = 4;
    let run = |names: (&str, &str), script: &str| {
        spawn(&mut pipeline_command(
            &case,
            &folders,
            (&repo, &runs),
            names,
            script,
        ))
    };
    let mut started: Vec<Child> = (0..PLAIN)
        .map(|_| run(("test", "plain"), "sleep 0.8"))
        .collect();
    let under = read_at_least(&folders, "leaves", 2);
    assert!(
        !under.partial,
        "a reading while the child is under its command misses nothing: {under:?}"
    );
    drop(leaving.stdin.take());
    started.push(leaving);
    let lines = symphony_api::fold(day_lines(&folders.work, &runs, PLAIN + 1));
    for mut each in started {
        assert!(each.wait().unwrap().success());
    }
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

/// A script that prints a real `cargo test`'s output (src/runs/suites/tests.rs says what it holds)
/// on `stream` (`1` or `2`) and exits `code`.
fn cargo_test(stream: u8, code: u8) -> String {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/runs/fixtures/cargo-test.log");
    format!("cat '{}' >&{stream}; exit {code}", fixture.display())
}

#[test]
fn the_runs_doc_names_each_field_a_written_line_holds_in_its_order_and_each_end() {
    let case = Case::new();
    let folders = Folders::new("recorder-runs-doc");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    let label = ("test", "just test-rs");
    // A test run's, so its line says its tests too.
    let script = cargo_test(1, 3);
    assert_eq!(
        pipeline_run(&case, &folders, (&repo, &runs), label, &script).code(),
        Some(3)
    );
    day_lines(&folders.work, &runs, 1);
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
    let doc = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/workflow/RUNS.md"),
    )
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
    // The table: each field a line holds, in its order.
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
    assert_eq!(fields, keys(example));
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
    let lines = symphony_api::fold(day_lines(&folders.work, &runs, 2));
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
    let [step] = symphony_api::fold(day_lines(&folders.work, &runs, 1))
        .try_into()
        .unwrap();
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
    let lines = day_lines(&folders.work, &runs, 8);
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
    let start = test_time::until("its start line", patience(), || {
        match day_lines(&folders.work, &runs, 0).as_slice() {
            [start] => Some(start.clone()),
            _ => None,
        }
    })
    .unwrap_or_else(|stall| panic!("{stall}: {}", holds(&folders.work)));
    assert!(start.is_start() && start.started.is_some(), "{start:?}");
    assert_eq!(start.run, running.operation_id);
    let recorder = libc::pid_t::try_from(running.pid).unwrap();
    // SAFETY: the recorder of the operation this test started.
    assert_eq!(unsafe { libc::kill(recorder, libc::SIGKILL) }, 0);
    test_time::until_true("the recorder gone after its kill", patience(), || {
        !alive(recorder)
    })
    .unwrap();
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
    let [own] = symphony_api::fold(day_lines(&folders.work, &elsewhere, 1))
        .try_into()
        .unwrap();
    assert_eq!(own.pipeline, "build");
    let [lost] = symphony_api::fold(day_lines(&folders.work, &runs, 1))
        .try_into()
        .unwrap();
    assert_eq!(lost.run, running.operation_id);
    assert_eq!(
        (lost.end, lost.exit, lost.signal, lost.cpu_s),
        (Some(End::Incomplete), None, None, None)
    );
    assert!(lost.ended.is_some());
    fs::write(&release, "").unwrap();
    assert!(started.wait().unwrap().success());
}

/// The command lines the runners themselves refuse, run through the recorder: each is recorded
/// refused with the runner's `error:` line, its end still the red its exit says, and a command
/// that failed, timed out or was stopped saying the same words is not. Cargo's is what
/// `tools/board-pg-lifecycle` and `tests/idle-count-discards-restored` typed on 2026-11-18: test
/// names after the first with no `--`.
#[test]
fn a_command_line_its_runner_refuses_is_recorded_refused_and_a_command_that_failed_is_not() {
    let case = Case::new();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let nextest = output(Command::of(&cargo).args(["nextest", "--version"]))
        .is_ok_and(|said| said.status.success());
    let justfile = "one ARG:\n    @echo {{ARG}}\n";
    let usage = "error: unexpected argument '";
    // What each script's record says of a refusal: the start of its line, or none.
    let cases: Vec<(String, Option<&str>)> = vec![
        (format!("'{cargo}' test own_names owner_words"), Some(usage)),
        (format!("'{cargo}' test --no-such-option"), Some(usage)),
        (format!("'{cargo}' clippy --no-such-option"), Some(usage)),
        (
            format!("'{cargo}' nextest run --no-such-option"),
            // Without nextest Cargo names no such command, which is none of the refusals read.
            nextest.then_some(usage),
        ),
        ("just --no-such-option".into(), Some(usage)),
        (
            "just no-such-recipe".into(),
            Some("error: justfile does not contain recipe `no-such-recipe`"),
        ),
        (
            "just on".into(),
            Some("error: justfile does not contain recipe `on`"),
        ),
        ("just one".into(), Some("error: recipe `one` got 0 ")),
        // A recipe that ran and failed, and a runner with nothing to run.
        ("just one a; exit 1".into(), None),
        ("cd / && just --justfile /no/such/justfile one".into(), None),
        // The same words from a command that built something first, went on, or did not end red.
        (
            format!("echo '   Compiling c v0.1.0 (/wt/c)'; '{cargo}' test a b"),
            None,
        ),
        (
            format!("'{cargo}' test a b; echo 'test t ... FAILED' >&2; exit 101"),
            None,
        ),
        (format!("'{cargo}' test a b; exit 124"), None),
        (format!("'{cargo}' test a b; kill -TERM $$"), None),
        (format!("'{cargo}' test a b; exit 0"), None),
        (
            "echo 'assertion `left == right` failed' >&2; exit 1".into(),
            None,
        ),
        ("exit 1".into(), None),
    ];
    for (n, (script, expected)) in cases.iter().enumerate() {
        let folders = Folders::new(&format!("recorder-refused-{n}"));
        let repo = repository(&folders.dir.join("repo"));
        fs::write(repo.join("justfile"), justfile).unwrap();
        let runs = folders.dir.join("runs");
        let done = output(
            pipeline_command(
                &case,
                &folders,
                (&repo, &runs),
                ("test", "just test-rs"),
                script,
            )
            .stdin(Stdio::null()),
        )
        .unwrap();
        let record = folders.once(Step::Finished);
        let said = format!("{script}: {done:?}");
        assert_eq!(record.exit_code, done.status.code(), "{said}");
        let refused = record.run.as_ref().unwrap().refused.as_deref();
        match expected {
            Some(start) => {
                assert!(
                    refused.is_some_and(|line| line.starts_with(start)),
                    "{said}"
                );
                assert_eq!(read::refused(&record), refused);
                assert!(read::red(&record).is_some(), "{said}");
                // Its line in the day files ends as its exit says: the refusal is the record's.
                let lines = day_lines(&folders.work, &runs, 1);
                assert_eq!(lines[1].end, Some(End::Red), "{said}");
            }
            None => assert_eq!(refused, None, "{said}"),
        }
    }
}

/// A shell function a run's script calls as `upto TEXT`: once its own log (`$SYMPHONY_RUNS`, its
/// operation `$TASKS_RUN_PARENT`) holds TEXT, so what it writes next is read after it; a backstop
/// only a stall reaches lets it go on.
const UPTO: &str = "set -- \"$SYMPHONY_RUNS\"/logs/*/\"$TASKS_RUN_PARENT\".log; log=$1; \
                    upto() { i=0; until grep -q \"$1\" \"$log\" || [ $i -ge 3000 ]; do \
                    sleep 0.01; i=$((i+1)); done; }; ";

#[test]
fn a_runs_streams_reach_its_caller_as_written_and_its_log_keeps_them_plain_in_order() {
    let case = Case::new();
    let folders = Folders::new("recorder-run-log");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    // A day folder of logs older than 14 days goes at the next line.
    fs::create_dir_all(runs.join("logs/2000-01-01")).unwrap();
    let script = format!(
        "{UPTO}printf '\\033[1;32mgreen\\033[0m one\\n'; upto 'green one'; \
         printf 'two \\033]0;a title\\007on stderr\\n' >&2; upto 'two on stderr'; \
         printf 'three\\n'; exit 4"
    );
    let done = output(
        pipeline_command(
            &case,
            &folders,
            (&repo, &runs),
            ("test", "just test-rs"),
            &script,
        )
        .stdin(Stdio::null()),
    )
    .unwrap();
    // The caller gets each stream's bytes as the command wrote them, escapes and all.
    assert_eq!(done.status.code(), Some(4), "{done:?}");
    assert_eq!(done.stdout, b"\x1b[1;32mgreen\x1b[0m one\nthree\n");
    assert_eq!(done.stderr, b"two \x1b]0;a title\x07on stderr\n");
    let lines = day_lines(&folders.work, &runs, 1);
    let [start, end] = [&lines[0], &lines[1]];
    let log = format!("logs/{}/{}.log", symphony_api::run_day(end.queued), end.run);
    assert_eq!(
        (start.log.as_deref(), start.log_bytes),
        (Some(log.as_str()), None),
        "a start line names the log a run that runs writes"
    );
    let kept = fs::read(runs.join(&log)).unwrap();
    assert_eq!(kept, b"green one\ntwo on stderr\nthree\n");
    assert_eq!(
        (end.log.as_deref(), end.log_bytes),
        (Some(log.as_str()), Some(kept.len() as u64))
    );
    assert!(
        !runs.join("logs/2000-01-01").exists(),
        "an old day of logs stayed"
    );
}

#[test]
fn a_steps_log_is_its_own_and_its_runs_log_holds_it_too() {
    let case = Case::new();
    let folders = Folders::new("recorder-run-log-step");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    let tasks_binary = env!("CARGO_BIN_EXE_tasks");
    let script = format!(
        "echo before; '{tasks_binary}' run --phase build --step lint -- sh -c 'echo inside'; \
         echo after"
    );
    let label = ("verify", "just verify");
    assert!(pipeline_run(&case, &folders, (&repo, &runs), label, &script).success());
    let lines = symphony_api::fold(day_lines(&folders.work, &runs, 2));
    let read = |line: &RunLine| {
        let log = line
            .log
            .as_deref()
            .unwrap_or_else(|| panic!("no log: {line:?}"));
        let text = fs::read_to_string(runs.join(log)).unwrap();
        assert_eq!(line.log_bytes, Some(text.len() as u64), "{line:?}");
        text
    };
    let step = lines.iter().find(|line| line.step.is_some()).unwrap();
    assert_eq!(read(step), "inside\n");
    // The step's output reaches its run's log through the step's recorder, so it may come after
    // what the run writes once the step's command ended.
    let run = lines.iter().find(|line| line.step.is_none()).unwrap();
    let mut words: Vec<String> = read(run).lines().map(str::to_owned).collect();
    symphony_board::order::sort_by(&mut words, &Ord::cmp);
    assert_eq!(words, ["after", "before", "inside"]);
    assert_ne!(run.log, step.log);
}

#[cfg(target_os = "macos")]
#[test]
fn what_a_process_a_run_left_writes_after_its_command_ended_still_reaches_its_caller_and_log() {
    // Elsewhere the recorder is its command's parent, and its caller waits for it to drain.
    use std::io::Read as _;
    let case = Case::new();
    let folders = Folders::new("recorder-run-log-left");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    let release = folders.dir.join("release");
    let script = format!(
        "(while [ ! -e '{}' ]; do sleep 0.02; done; echo late) & echo now",
        release.display()
    );
    let mut started = pipeline_command(&case, &folders, (&repo, &runs), ("test", "t"), &script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdout = started.stdout.take().unwrap();
    // The process its caller waits for is the command's, which ended with the other still running.
    assert!(started.wait().unwrap().success());
    fs::write(&release, "").unwrap();
    let mut said = String::new();
    stdout.read_to_string(&mut said).unwrap();
    assert_eq!(said, "now\nlate\n");
    let [end] = symphony_api::fold(day_lines(&folders.work, &runs, 1))
        .try_into()
        .unwrap();
    let log = runs.join(end.log.unwrap());
    // The drain's wait is the product's DRAIN at the tests' scale: a log short of the late line
    // stalled past it, which a loaded machine alone reaches.
    assert_eq!(
        end.log_bytes,
        Some(9),
        "stalled: the stream the left process held did not end within the drain (DRAIN, 1 s, at \
         scale {SCALE}) after the release; {}",
        test_time::Load::now()
    );
    assert_eq!(fs::read_to_string(log).unwrap(), "now\nlate\n");
}

/// [`pipeline_command`] run with its stdout and stderr the files `out` and `err` (one file twice,
/// as `>f 2>&1` hands it, or two): its status, as soon as its caller has it.
fn filed_run(command: &mut Command, out: &fs::File, err: &fs::File) -> std::process::ExitStatus {
    command
        .stdin(Stdio::null())
        .stdout(out.try_clone().unwrap())
        .stderr(err.try_clone().unwrap())
        .spawn()
        .unwrap()
        .wait()
        .unwrap()
}

#[test]
fn a_caller_that_reads_its_file_the_moment_it_has_the_status_finds_its_last_bytes_and_the_log_them()
{
    // The release run's `"$@" >"$log" 2>&1; tail "$log"`, and an agent's tool call: no pipe stands
    // between the command and its caller's file, so its bytes are there before its status is.
    let case = Case::new();
    let folders = Folders::new("recorder-run-log-file");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    let script = "printf '\\033[1m%2000s\\033[0m\\n' x; printf 'on stderr\\n' >&2; echo END";
    let times = 10;
    let mut read = Vec::new();
    for time in 0..times {
        let path = folders.dir.join(format!("both-{time}"));
        let both = fs::File::create(&path).unwrap();
        let mut command = pipeline_command(&case, &folders, (&repo, &runs), ("test", "t"), script);
        let status = filed_run(&mut command, &both, &both);
        let text = fs::read(&path).unwrap();
        assert!(status.success(), "{status:?}");
        assert!(
            text.ends_with(b"on stderr\nEND\n"),
            "run {time} of {times}: its caller read its file with the status and missed its end: \
             {} bytes, ending {:?}",
            text.len(),
            String::from_utf8_lossy(&text[text.len().saturating_sub(40)..])
        );
        read.push(text);
    }
    // Each run's log holds what its command added to the one file both streams name, once, plain.
    let plain = format!("{:>2000}\non stderr\nEND\n", "x");
    let lines = symphony_api::fold(day_lines(&folders.work, &runs, times));
    for (line, text) in lines.iter().zip(&read) {
        assert_eq!(
            text.as_slice(),
            format!("\x1b[1m{:>2000}\x1b[0m\non stderr\nEND\n", "x").as_bytes()
        );
        let log = line
            .log
            .as_deref()
            .unwrap_or_else(|| panic!("no log: {line:?}"));
        assert_eq!(
            fs::read_to_string(runs.join(log)).unwrap(),
            plain,
            "{line:?}"
        );
        assert_eq!(line.log_bytes, Some(plain.len() as u64), "{line:?}");
    }
}

#[test]
fn a_file_callers_log_holds_what_the_run_added_to_each_of_its_files_in_turn_and_its_steps_own() {
    let case = Case::new();
    let folders = Folders::new("recorder-run-log-files");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    // `>>out 2>err`: a file that held a line already, and another.
    let (out_path, err_path) = (folders.dir.join("out"), folders.dir.join("err"));
    fs::write(&out_path, "before\n").unwrap();
    let out = fs::OpenOptions::new().append(true).open(&out_path).unwrap();
    let err = fs::File::create(&err_path).unwrap();
    // The step last: on macOS its recorder reads where the files end once it saw its command end,
    // so what the run wrote after it at once could be in its log too (docs/workflow/RUNS.md, *Logs*).
    let tasks_binary = env!("CARGO_BIN_EXE_tasks");
    let script = format!(
        "printf '\\033[32mout one\\033[0m\\n'; printf 'err one\\n' >&2; echo out two; \
         '{tasks_binary}' run --phase build --step lint -- sh -c 'echo inside; echo inside err >&2'"
    );
    let mut command = pipeline_command(&case, &folders, (&repo, &runs), ("verify", "v"), &script);
    assert!(filed_run(&mut command, &out, &err).success());
    // The caller's files hold what the command wrote, escapes and all, after what they held.
    assert_eq!(
        fs::read(&out_path).unwrap(),
        b"before\n\x1b[32mout one\x1b[0m\nout two\ninside\n"
    );
    assert_eq!(fs::read(&err_path).unwrap(), b"err one\ninside err\n");
    let lines = day_lines(&folders.work, &runs, 2);
    let start = lines
        .iter()
        .find(|line| line.step.is_none() && line.end.is_none())
        .unwrap();
    let read = |line: &RunLine| {
        let log = line
            .log
            .as_deref()
            .unwrap_or_else(|| panic!("no log: {line:?}"));
        let text = fs::read_to_string(runs.join(log)).unwrap();
        assert_eq!(line.log_bytes, Some(text.len() as u64), "{line:?}");
        text
    };
    let folded = symphony_api::fold(lines.clone());
    let run = folded.iter().find(|line| line.step.is_none()).unwrap();
    // Its start line names the log, which holds nothing until its command ended.
    assert_eq!(
        (start.log.as_deref(), start.log_bytes),
        (run.log.as_deref(), None)
    );
    assert_eq!(read(run), "out one\nout two\ninside\nerr one\ninside err\n");
    // The step's caller is the run's files: its log is what it added to them.
    let step = folded.iter().find(|line| line.step.is_some()).unwrap();
    assert_eq!(read(step), "inside\ninside err\n");
    assert_ne!(run.log, step.log);
}

#[test]
fn a_test_runs_end_line_says_what_its_log_says_of_each_binary_and_each_failed_test() {
    let case = Case::new();
    let folders = Folders::new("recorder-run-suites");
    let repo = repository(&folders.dir.join("repo"));
    let runs = folders.dir.join("runs");
    let script = cargo_test(2, 101);
    let names = ("test", "just test-rs");
    // From pipes, and from a file both streams name, as an agent's tool call hands it.
    let piped = pipeline_run(&case, &folders, (&repo, &runs), names, &script);
    let both = fs::File::create(folders.dir.join("both")).unwrap();
    let mut command = pipeline_command(&case, &folders, (&repo, &runs), names, &script);
    let filed = filed_run(&mut command, &both, &both);
    assert_eq!((piped.code(), filed.code()), (Some(101), Some(101)));
    let lines = day_lines(&folders.work, &runs, 2);
    for start in lines.iter().filter(|line| line.end.is_none()) {
        assert_eq!((&start.suites, &start.failed), (&None, &None), "{start:?}");
    }
    let ends: Vec<&RunLine> = lines.iter().filter(|line| line.end.is_some()).collect();
    assert_eq!(ends.len(), 2);
    for end in ends {
        let suites: Vec<(&str, u32, u32, bool)> = (end.suites.iter().flatten())
            .map(|suite| (suite.name.as_str(), suite.passed, suite.failed, suite.error))
            .collect();
        assert_eq!(
            suites,
            [
                ("unittests src/lib.rs (symphony_args)", 1, 2, false),
                ("tests/aborts.rs (aborts)", 0, 0, true),
                ("tests/arguments.rs (arguments)", 8, 0, false),
                ("Doc-tests symphony_args", 1, 0, false),
            ],
            "{end:?}"
        );
        let failed: Vec<&str> = (end.failed.iter().flatten())
            .map(|test| test.name.as_str())
            .collect();
        assert_eq!(failed, ["fixture::compares", "fixture::panics"], "{end:?}");
    }
}
