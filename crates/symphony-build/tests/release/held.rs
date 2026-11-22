//! A held update tried again by the machine's server (`symphony::machine::held`), round by round,
//! on a scratch machine folder and scratch boards: a project's board a refresh left behind a
//! writer, its update open in the ledger with its task on the Symphony's board. The release
//! installed is this build's, its `tasks` the board's own binary, so each move is the release's
//! real migrate of a real board, and the server's word that it writes the board a real read of it.
//! What the server lends a round (its clock, its bounded run of a command, its boards) is
//! [`Machine`] here; the server's own clock driving a round is the server's test.

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::rc::Rc;
use std::time::Duration;

use serde_json::json;
use symphony::machine::held::{self, Host, Retry};
use symphony::machine::updates::{self, Failure, Ledger, Task, TaskBoard, Update};
use symphony::machine::writers;
use symphony::taskcli::CliBoard;
use symphony_board::Raw;
use symphony_board::rpc::Client;
use symphony_board::server::Home;
use symphony_board::sweep::trigger::now;
use symphony_board::sys::Command;

use super::Fixture;
use super::refresh::{Board, a_project, board_at, executable, machine, registered, wrote};

/// The release installed here: this build, serving its schema.
const RELEASE: &str = "release-2026-11-20";
/// The schema it serves, and the one the board is left behind at.
const SCHEMA: i64 = symphony_board::SCHEMA;
const BEHIND: i64 = symphony_board::SCHEMA - 1;
/// Each command's bound, as the server's sweep gives it.
const BOUND: Duration = Duration::from_secs(240);

/// The Symphony's checkout and a project `alpha`, both registered; the release installed with
/// `current` naming it, its `tasks` running the board's own binary; `alpha`'s board started at
/// [`BEHIND`], its one writer a build at `held` on this host; and the update `alpha@SCHEMA` open as
/// the refresh that left it filed it, its task on the Symphony's board.
struct Held {
    /// Stopped before the fixture's folder goes: fields drop in this order.
    _board: Board,
    fixture: Fixture,
    alpha: PathBuf,
    client: Client,
    /// The program the writer that holds the board ran: there while it holds.
    held: PathBuf,
    slug: String,
}

impl Held {
    fn new() -> Held {
        let fixture = Fixture::new();
        let alpha = a_project(&fixture, "alpha", &[]);
        // The Symphony's checkout keeps the release run's record: its board takes the update's
        // task.
        fs::create_dir_all(
            fixture
                .main
                .join(".git")
                .join(symphony_board::folders::RELEASE),
        )
        .unwrap();
        registered(
            &fixture,
            &[("alpha", &alpha, &[]), ("symphony", &fixture.main, &[])],
        );
        installed(&machine(&fixture), RELEASE, SCHEMA);
        Held::release_tasks(&fixture, None);
        std::os::unix::fs::symlink(
            Path::new("releases").join(RELEASE),
            machine(&fixture).join("current"),
        )
        .unwrap();
        // The project's own pinned SurrealDB, where its `just tasks` fetches it: what its snapshot
        // runs.
        let pinned = alpha
            .join(".git")
            .join(symphony_board::folders::SURREAL)
            .join(symphony_board::server::SURREAL_VERSION);
        fs::create_dir_all(&pinned).unwrap();
        std::os::unix::fs::symlink(super::board::surreal(), pinned.join("surreal")).unwrap();
        let (board, _, client) = board_at(&alpha, BEHIND);
        Raw::on(&client).table("writer").delete().unwrap();
        let program = fixture.root.join("alpha-build/tasks");
        executable(&program, "#!/bin/sh\n");
        wrote(&client, "tasks", &program, BEHIND, 60.0);
        let slug = updates::slug("alpha", SCHEMA, 1);
        let held = Held {
            _board: board,
            fixture,
            alpha,
            client,
            held: program,
            slug,
        };
        held.filed();
        held
    }

    /// The release's `tasks`: the board's own binary, or `script` in its place.
    fn release_tasks(fixture: &Fixture, script: Option<&str>) {
        let path = machine(fixture)
            .join("releases")
            .join(RELEASE)
            .join("bin/tasks");
        let _ = fs::remove_file(&path);
        let text = script.map_or_else(
            || {
                format!(
                    "#!/bin/sh\nexec '{}' \"$@\"\n",
                    super::common::tasks_binary()
                )
            },
            str::to_string,
        );
        executable(&path, &text);
    }

    /// The Symphony's board, as the fixture's runs reach it.
    fn ours(&self) -> CliBoard {
        CliBoard::with_command(
            vec![super::common::tasks_binary().to_string()],
            &self.fixture.env,
            &self.fixture.main,
        )
    }

    /// The update's task filed on the Symphony's board, and the ledger as the refresh left it:
    /// open, failed once by what holds the board now.
    fn filed(&self) {
        let board = self.ours();
        let filer = TaskBoard::register(&board).unwrap();
        board
            .call(
                "create",
                &json!({
                    "slug": self.slug, "title": "Schema update failed", "body": "update: alpha",
                    "by": filer, "priority": 0, "red": "release", "links": ["docs/RELEASES.md"],
                }),
            )
            .unwrap();
        TaskBoard::leave(&board, &filer).unwrap();
        let mut update = Update::new("alpha", SCHEMA, 1, None);
        update.failures.push(Failure {
            at: now() - 3600.0,
            run: "2026-11-20T0004-1".into(),
            why: self.holding(),
        });
        update.task = Some(Task {
            project: "symphony".into(),
            ours: true,
            slug: self.slug.clone(),
            closed: false,
        });
        Ledger {
            version: updates::VERSION,
            updates: vec![update],
        }
        .write(&machine(&self.fixture))
        .unwrap();
    }

    fn home(&self) -> Home {
        Home::in_common(&self.alpha.join(".git"), Some(&super::board::surreal()))
    }

    /// What the refresh said holds the board now: its writers behind, judged by the rule.
    fn holding(&self) -> String {
        let lines = self.home().writers(symphony_board::DATABASE).unwrap();
        let runs = writers::Runs::of(&self.alpha);
        let found = writers::lagging(lines, SCHEMA, now(), &machine(&self.fixture), &runs);
        found.holding(BEHIND, RELEASE, SCHEMA, now())
    }

    fn round(&self, host: &Machine) -> Vec<String> {
        held::retry(
            &Retry {
                machine: &machine(&self.fixture),
                release: RELEASE,
                project: "alpha",
                bound: BOUND,
            },
            host,
        )
    }

    fn host(&self) -> Machine {
        Machine {
            root: self.alpha.clone(),
            machine: machine(&self.fixture),
            env: self.fixture.env.clone(),
            late: Cell::new(false),
            refuse: Cell::new(false),
            unloaded: Cell::new(false),
            told: Rc::default(),
            through: RefCell::default(),
        }
    }

    fn ledger(&self) -> Update {
        Ledger::read(&machine(&self.fixture)).unwrap().updates[0].clone()
    }

    fn schema(&self) -> Option<i64> {
        self.home().schema(symphony_board::DATABASE).unwrap()
    }

    /// The posts on the update's task, in order.
    fn thread(&self) -> Vec<String> {
        let said = self
            .ours()
            .call("thread", &json!({"slug": self.slug}))
            .unwrap();
        (said.as_array().unwrap().iter())
            .map(|message| message["text"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    /// The posts on the update's task that say a failure.
    fn failures_told(&self) -> Vec<String> {
        (self.thread().into_iter())
            .filter(|text| text.starts_with("Failure "))
            .collect()
    }

    fn status(&self) -> String {
        TaskBoard::status(&self.ours(), &self.slug)
            .unwrap()
            .unwrap()
    }

    fn snapshot(&self) -> PathBuf {
        self.alpha.join(format!(
            ".git/symphony/tasks/snapshots/before-schema-{SCHEMA}-{RELEASE}.surql"
        ))
    }
}

/// The release `name`, serving `schema`, installed in the machine folder `machine` as its install
/// leaves it, its `tasks` a stand-in: its folder.
fn installed(machine: &Path, name: &str, schema: i64) -> PathBuf {
    let folder = machine.join("releases").join(name);
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("manifest.json"),
        json!({
            "release": name, "commit": "0123456", "schema": schema,
            "protocol": {"major": 0, "minor": 3}, "tasks_api": {"major": 2, "minor": 2},
            "pin": 1, "client": "client/x", "artifacts": [],
        })
        .to_string(),
    )
    .unwrap();
    executable(&folder.join("bin/symphony-server"), "#!/bin/sh\n");
    executable(&folder.join("bin/tasks"), "#!/bin/sh\nexit 9\n");
    folder
}

/// A project's own board as a round tells it: each call, said, in `told`.
struct Theirs(Rc<RefCell<Vec<String>>>);

impl TaskBoard for Theirs {
    fn register(&self) -> Result<String, String> {
        Ok("filer-1".into())
    }
    fn leave(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn status(&self, _: &str) -> Result<Option<String>, String> {
        Ok(Some("ready".into()))
    }
    fn say(&self, slug: &str, _: &str, text: &str) -> Result<(), String> {
        self.0.borrow_mut().push(format!("say {slug}: {text}"));
        Ok(())
    }
    fn close(&self, slug: &str, _: &str, why: &str) -> Result<(), String> {
        self.0.borrow_mut().push(format!("close {slug}: {why}"));
        Ok(())
    }
}

/// The machine's server as a round needs it: each command run to its end in the machine's own
/// environment (its folder, its SurrealDB), or, `late`, out of time as its sweep's bound ends one;
/// its word that it writes the board a read of it made now, or, `unloaded`, none yet; the
/// Symphony's board, or, `refuse`, one that cannot be reached; and a project's own, each call
/// through it in `told` and the release binary it was reached by in `through`.
struct Machine {
    root: PathBuf,
    machine: PathBuf,
    env: Vec<(String, String)>,
    late: Cell<bool>,
    refuse: Cell<bool>,
    unloaded: Cell<bool>,
    told: Rc<RefCell<Vec<String>>>,
    through: RefCell<Vec<PathBuf>>,
}

impl Host for Machine {
    fn now(&self) -> f64 {
        now()
    }

    fn run(&self, mut command: Command, bound: Duration) -> Result<Output, String> {
        if self.late.get() {
            return Err(format!("no answer within {} s", bound.as_secs()));
        }
        command
            .env("SYMPHONY_MACHINE_DIR", &self.machine)
            .env("SYMPHONY_SURREAL", super::board::surreal());
        symphony_board::sys::output(&mut command).map_err(|e| e.to_string())
    }

    fn writes(&self, schema: i64) -> Result<(), String> {
        if self.unloaded.get() {
            return Err("this server has not loaded the project".into());
        }
        held::writes(&self.root, schema)
    }

    fn ours(&self, main: &Path) -> Box<dyn TaskBoard> {
        let cli = if self.refuse.get() {
            "false".to_string()
        } else {
            super::common::tasks_binary().to_string()
        };
        Box::new(CliBoard::with_command(vec![cli], &self.env, main))
    }

    fn theirs(&self, root: &Path, tasks: &Path) -> Box<dyn TaskBoard> {
        assert_eq!(
            root, self.root,
            "a project's board is asked in its checkout"
        );
        self.through.borrow_mut().push(tasks.to_path_buf());
        Box::new(Theirs(Rc::clone(&self.told)))
    }
}

#[test]
fn a_board_held_by_a_writer_whose_program_then_goes_moves_at_the_next_round_and_its_task_closes() {
    let held = Held::new();
    let host = held.host();
    // Held: it stays, and the task hears nothing, as the refresh said the same.
    assert_eq!(
        held.round(&host),
        [format!(
            "alpha@{SCHEMA}: still held, as its last failure says"
        )]
    );
    assert_eq!(held.schema(), Some(BEHIND));
    assert_eq!(held.failures_told(), Vec::<String>::new());
    // The build that held it is gone from its path: the next round moves the board.
    fs::remove_file(&held.held).unwrap();
    let said = held.round(&host);
    assert_eq!(said.len(), 3, "{said:#?}");
    assert!(
        said[0].starts_with(&format!(
            "alpha@{SCHEMA}: its board moved from schema {BEHIND} to {SCHEMA} by {RELEASE}'s own \
             `tasks migrate --leave-behind`, its store snapshotted first to {}; not counted, as \
             its program is gone: tasks at schema {BEHIND} (",
            held.snapshot().display()
        )) && said[0].ends_with("nothing is at its path now"),
        "{said:#?}"
    );
    assert!(
        said[1].starts_with(&format!(
            "alpha@{SCHEMA}: served: the machine's server writes the board at schema {SCHEMA}. It \
             took "
        )),
        "{said:#?}"
    );
    assert_eq!(
        said[2],
        format!("alpha@{SCHEMA}: closed {} on symphony's board", held.slug)
    );
    assert_eq!(held.schema(), Some(SCHEMA));
    assert!(
        fs::read_to_string(held.snapshot())
            .unwrap()
            .contains("sim/kept")
    );
    // The ledger has the move with its times, each from its record.
    let update = held.ledger();
    let reached = update.reached.clone().unwrap();
    assert!(reached.source.starts_with("schema-seen.log"), "{reached:?}");
    let served = update.served.clone().unwrap();
    assert!(served.at >= reached.at && served.source.starts_with("the machine server's own read"));
    assert_eq!(update.served_by.as_deref(), Some(RELEASE));
    assert_eq!(update.release.as_deref(), Some(RELEASE));
    assert!(
        update
            .available
            .as_ref()
            .unwrap()
            .source
            .starts_with(RELEASE)
    );
    assert_eq!(update.failures.len(), 1, "moving is no failure");
    assert!(update.task.as_ref().unwrap().closed);
    // Its task, closed, says how long it took.
    assert_eq!(held.status(), "cancelled");
    let thread = held.thread();
    let closed: Vec<&String> = (thread.iter())
        .filter(|text| text.starts_with(&format!("alpha@{SCHEMA}: it is served")))
        .collect();
    assert_eq!(closed.len(), 1, "{thread:#?}");
    assert!(
        closed[0].starts_with(&format!(
            "alpha@{SCHEMA}: it is served (the machine's server, {RELEASE}, moved its board once \
             nothing held it). It took "
        )) && closed[0].ends_with("Closed by the machine's server, without a refresh."),
        "{thread:#?}"
    );
    // Nothing is left to do: the next round reads two files and does nothing.
    assert_eq!(held.round(&host), Vec::<String>::new());
}

#[test]
fn a_board_still_held_stays_and_its_task_hears_once_for_each_change_and_never_for_a_round() {
    let held = Held::new();
    let host = held.host();
    for _ in 0..3 {
        assert_eq!(
            held.round(&host),
            [format!(
                "alpha@{SCHEMA}: still held, as its last failure says"
            )]
        );
    }
    assert_eq!(held.ledger().failures.len(), 1);
    // Another writer behind: a change, heard once.
    let other = held.fixture.root.join("beta-build/agent-send");
    executable(&other, "#!/bin/sh\n");
    wrote(&held.client, "agent-send", &other, BEHIND, 30.0);
    let said = held.round(&host);
    assert_eq!(said.len(), 1);
    assert!(
        said[0].starts_with(&format!(
            "alpha@{SCHEMA}: not moved: its board stays at schema {BEHIND}: 2 writers"
        )) && said[0].ends_with(&format!("; heard on {}", held.slug)),
        "{said:#?}"
    );
    for _ in 0..3 {
        assert_eq!(
            held.round(&host),
            [format!(
                "alpha@{SCHEMA}: still held, as its last failure says"
            )]
        );
    }
    let failures = held.failures_told();
    assert_eq!(failures.len(), 1, "{failures:#?}");
    assert!(failures[0].starts_with(&format!(
        "Failure 2 of alpha@{SCHEMA} (the machine's server, {RELEASE}, at its round): its board \
         stays at schema {BEHIND}: 2 writers"
    )));
    let update = held.ledger();
    assert_eq!(update.failures.len(), 2);
    assert!(update.open() && update.reached.is_none());
    assert_eq!(held.schema(), Some(BEHIND));
}

#[test]
fn nothing_moves_while_the_release_lock_is_held_by_a_run_of_this_build_or_one_from_before() {
    let held = Held::new();
    let host = held.host();
    fs::remove_file(&held.held).unwrap();
    let machine = machine(&held.fixture);
    // A release run of this build holds the machine's updates lock from just after its own.
    let run = updates::lock(&machine).unwrap();
    for _ in 0..2 {
        let said = held.round(&host);
        assert!(
            said[0].contains("the machine's updates lock is held"),
            "{said:#?}"
        );
    }
    drop(run);
    // A run of a build from before that lock holds the run's lock alone, and says it runs.
    let record = held
        .fixture
        .main
        .join(".git")
        .join(symphony_board::folders::RELEASE);
    fs::write(
        record.join("running.json"),
        json!({"version": 2, "id": "2026-11-20T0100-1", "kind": "refresh", "pid": std::process::id(), "started_at": now(), "until": now() + 3600.0}).to_string(),
    )
    .unwrap();
    let said = held.round(&host);
    assert!(
        said[0].contains("a release run holds its lock (run 2026-11-20T0100-1"),
        "{said:#?}"
    );
    assert_eq!(held.schema(), Some(BEHIND));
    assert!(!held.snapshot().exists());
    assert_eq!(held.ledger().failures.len(), 1);
    assert_eq!(held.failures_told(), Vec::<String>::new());
    // Once it has ended, the next round moves the board.
    fs::remove_file(record.join("running.json")).unwrap();
    let said = held.round(&host);
    assert!(said[0].contains("its board moved from schema"), "{said:#?}");
    assert_eq!(held.schema(), Some(SCHEMA));
}

#[test]
fn a_snapshot_or_a_migrate_that_fails_or_runs_out_of_time_leaves_the_board_for_the_next_round() {
    let held = Held::new();
    let host = held.host();
    fs::remove_file(&held.held).unwrap();
    // The snapshot cannot be written: a failure, heard once, and nothing moved.
    let snapshots = held.alpha.join(".git/symphony/tasks/snapshots");
    fs::create_dir_all(snapshots.parent().unwrap()).unwrap();
    fs::write(&snapshots, "in the way").unwrap();
    for round in 0..2 {
        let said = held.round(&host);
        let expected = if round == 0 {
            "not moved: the snapshot of its store failed"
        } else {
            "still held"
        };
        assert!(said[0].contains(expected), "{said:#?}");
    }
    assert_eq!(held.schema(), Some(BEHIND));
    fs::remove_file(&snapshots).unwrap();
    // The migrate runs out of its bound, as the server's sweep ends one.
    host.late.set(true);
    let said = held.round(&host);
    assert!(
        said[0].contains(&format!(
            "not moved: {RELEASE}'s `tasks migrate --leave-behind` in {} did not end (no answer \
             within 240 s); its store as it was before is in {}, and the machine's server judges \
             it again at its next round",
            held.alpha.display(),
            held.snapshot().display()
        )),
        "{said:#?}"
    );
    host.late.set(false);
    // The release's migrate fails.
    Held::release_tasks(
        &held.fixture,
        Some("#!/bin/sh\necho 'the migrate is red' >&2\nexit 3\n"),
    );
    let said = held.round(&host);
    assert!(
        said[0].contains("failed (exit status: 3: the migrate is red)"),
        "{said:#?}"
    );
    let update = held.ledger();
    assert_eq!(
        update.failures.len(),
        4,
        "the refresh's, the snapshot's, the bound's, the migrate's"
    );
    assert!(update.open() && update.reached.is_none());
    assert_eq!(held.schema(), Some(BEHIND));
    // The next round with its migrate working moves it and finishes.
    Held::release_tasks(&held.fixture, None);
    let said = held.round(&host);
    assert!(said[0].contains("its board moved from schema"), "{said:#?}");
    assert!(held.ledger().task.unwrap().closed);
    assert_eq!(held.failures_told().len(), 3, "one post for each change");
}

#[test]
fn a_move_whose_ledger_was_not_written_and_a_close_that_failed_are_finished_by_the_next_round() {
    let held = Held::new();
    let host = held.host();
    // The board was moved, and the round that moved it ended before it wrote the ledger.
    let mut command = Command::new(super::common::tasks_binary());
    command
        .args(["migrate", "--leave-behind"])
        .current_dir(&held.alpha)
        .env_remove("TASKS_SERVER")
        .env_remove("TASKS_DATABASE")
        .env("SYMPHONY_MACHINE_DIR", machine(&held.fixture))
        .env("SYMPHONY_SURREAL", super::board::surreal());
    let migrate = symphony_board::sys::output(&mut command).unwrap();
    assert!(
        migrate.status.success(),
        "{}",
        String::from_utf8_lossy(&migrate.stderr)
    );
    assert_eq!(held.schema(), Some(SCHEMA));
    // Its task's board cannot be reached: served, not closed.
    host.refuse.set(true);
    let said = held.round(&host);
    assert_eq!(said.len(), 2, "{said:#?}");
    assert!(
        said[0].contains("served: the machine's server writes the board"),
        "{said:#?}"
    );
    assert!(
        said[1].contains("could not be closed on symphony's board"),
        "{said:#?}"
    );
    let update = held.ledger();
    let reached = update.reached.clone().unwrap();
    assert!(reached.source.starts_with("schema-seen.log"), "{reached:?}");
    assert!(update.served.is_some() && !update.task.as_ref().unwrap().closed);
    assert_ne!(held.status(), "cancelled");
    // The next round closes it, and moves nothing.
    host.refuse.set(false);
    assert_eq!(
        held.round(&host),
        [format!(
            "alpha@{SCHEMA}: closed {} on symphony's board",
            held.slug
        )]
    );
    assert_eq!(held.status(), "cancelled");
    assert_eq!(held.ledger().served, update.served, "served once");
    assert!(!held.snapshot().exists(), "no move was made here");
    assert_eq!(held.round(&host), Vec::<String>::new());
}

#[test]
fn the_server_writes_a_board_only_at_its_own_schema_by_a_read_made_now() {
    let fixture = Fixture::new();
    let alpha = a_project(&fixture, "alpha", &[]);
    let (_board, _, _) = board_at(&alpha, BEHIND);
    let behind = held::writes(&alpha, SCHEMA).unwrap_err();
    assert!(
        behind.contains(&format!("behind this build's {SCHEMA}")),
        "{behind}"
    );
    assert!(held::writes(&alpha, BEHIND).unwrap_err().contains(&format!(
        "this server's build writes schema {SCHEMA}, not {BEHIND}"
    )));
    let beta = a_project(&fixture, "beta", &[]);
    let (_beta, _, _) = board_at(&beta, SCHEMA);
    assert_eq!(held::writes(&beta, SCHEMA), Ok(()));
}

#[test]
fn a_release_run_waits_for_a_round_under_way_and_holds_the_machines_lock_to_its_end() {
    use std::io::{BufRead, BufReader, Write as _};
    use symphony_board::sys::Stdio;
    let held = Held::new();
    let host = held.host();
    fs::remove_file(&held.held).unwrap();
    let machine = machine(&held.fixture);
    // A round under way holds the machine's lock: the run says it waits, and says itself meanwhile.
    let round = updates::lock(&machine).unwrap();
    let fifo = held.fixture.root.join("go");
    let made = symphony_board::sys::output(Command::new("mkfifo").arg(&fifo)).unwrap();
    assert!(made.status.success());
    let pid = held.fixture.root.join("check.pid");
    let check = format!(
        "verify=echo $$ > '{}'; read line < '{}'",
        pid.display(),
        fifo.display()
    );
    let mut run = held
        .fixture
        .command(super::NOW, &["--check", &check, "--timeout", "600"], &[])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _running = super::stops::Process(libc::pid_t::try_from(run.id()).unwrap());
    let (said, lines) = std::sync::mpsc::channel();
    let stdout = run.stdout.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = said.send(line);
        }
    });
    let mut out = Vec::new();
    test_time::until_true(
        "the run to say it waits for the machine's lock",
        test_time::backstop(Duration::from_secs(60)),
        || {
            out.extend(lines.try_iter());
            out.iter()
                .any(|line| line.contains("this run waits for it"))
        },
    )
    .unwrap_or_else(|stall| panic!("{stall}: {out:#?}"));
    assert!(
        out.contains(&format!(
            "release: the machine's server is trying a held update again under {}: this run \
             waits for it",
            machine.join(updates::LOCK).display()
        )),
        "{out:#?}"
    );
    assert!(
        !pid.exists(),
        "no check runs while the round holds the lock"
    );
    assert!(
        held.fixture.records().join("running.json").exists(),
        "a run waiting says itself"
    );
    // The round ends: the run goes on to its check, holding the lock, and a round moves nothing.
    drop(round);
    let check_pid = super::stops::written_pid(&pid);
    assert!(
        updates::try_lock(&machine).unwrap().is_none(),
        "the run holds it"
    );
    let refused = held.round(&host);
    assert!(
        refused[0].contains("the machine's updates lock is held"),
        "{refused:#?}"
    );
    assert_eq!(held.schema(), Some(BEHIND));
    writeln!(fs::OpenOptions::new().write(true).open(&fifo).unwrap()).unwrap();
    let status = test_time::until(
        "the run to end",
        test_time::backstop(Duration::from_secs(120)),
        || run.try_wait().unwrap(),
    )
    .unwrap_or_else(|stall| panic!("{stall}"));
    assert!(super::stops::ends_within(check_pid, 5));
    assert!(status.success(), "{status}: {out:#?}");
    assert!(
        updates::try_lock(&machine).unwrap().is_some(),
        "let go at its end"
    );
    // The next round moves the board.
    assert!(held.round(&host)[0].contains("its board moved from schema"));
}

#[test]
fn a_task_on_the_projects_own_board_is_told_by_the_release_writing_its_schema_and_an_unwritten_board_waits()
 {
    let held = Held::new();
    let host = held.host();
    let machine = machine(&held.fixture);
    // The update's task is on alpha's own board, which a release installed before writes at BEHIND.
    let older = installed(&machine, "release-2026-11-12", BEHIND);
    let mut ledger = Ledger::read(&machine).unwrap();
    ledger.updates[0].task = Some(Task {
        project: "alpha".into(),
        ours: false,
        slug: held.slug.clone(),
        closed: false,
    });
    ledger.write(&machine).unwrap();
    // What holds it changes: heard on alpha's board, through that release's own `tasks`.
    let other = held.fixture.root.join("beta-build/agent-send");
    executable(&other, "#!/bin/sh\n");
    wrote(&held.client, "agent-send", &other, BEHIND, 30.0);
    let said = held.round(&host);
    assert!(
        said[0].ends_with(&format!("; heard on {}", held.slug)),
        "{said:#?}"
    );
    assert_eq!(*host.through.borrow(), [older.join("bin/tasks")]);
    assert!(
        host.told.borrow()[0].starts_with(&format!(
            "say {}: Failure 2 of alpha@{SCHEMA} (the machine's server, {RELEASE}, at its round): ",
            held.slug
        )),
        "{:#?}",
        host.told.borrow()
    );
    // Both builds gone: moved, and the server has not loaded the project yet, so it waits.
    fs::remove_file(&held.held).unwrap();
    fs::remove_file(&other).unwrap();
    host.unloaded.set(true);
    let said = held.round(&host);
    assert_eq!(said.len(), 2, "{said:#?}");
    assert!(said[0].contains("its board moved from schema"), "{said:#?}");
    assert_eq!(
        said[1],
        format!(
            "alpha@{SCHEMA}: its board is at schema {SCHEMA}, and the machine's server does not \
             write it yet (this server has not loaded the project): its next round looks again"
        )
    );
    let waiting = held.ledger();
    assert!(waiting.reached.is_some() && waiting.served.is_none() && waiting.open());
    assert_eq!(
        host.told.borrow().len(),
        1,
        "nothing closed, nothing more said"
    );
    // Loaded: served, its task closed on alpha's board through the release that writes it now.
    host.unloaded.set(false);
    let said = held.round(&host);
    assert!(
        said[0].contains("served: the machine's server writes the board"),
        "{said:#?}"
    );
    assert_eq!(
        said[1],
        format!("alpha@{SCHEMA}: closed {} on alpha's board", held.slug)
    );
    assert_eq!(
        host.through.borrow().last(),
        Some(&machine.join("releases").join(RELEASE).join("bin/tasks"))
    );
    let told = host.told.borrow();
    assert!(
        told[1].starts_with(&format!(
            "close {}: alpha@{SCHEMA}: it is served",
            held.slug
        )),
        "{told:#?}"
    );
    let served = held.ledger();
    assert_eq!(served.reached, waiting.reached, "reached once");
    assert!(served.task.unwrap().closed);
}
