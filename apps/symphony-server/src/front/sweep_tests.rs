//! The shared installation's idle-session sweep, on fixture launchers and clocks.

use super::test_clock::TestClock;
use super::*;
use std::sync::atomic::AtomicUsize;
use symphony_board::builds::Beyond;

/// The wall-clock backstop of every wait here: each waits on its condition, and only a stall
/// reaches this (RULES.md `tests/no-test-fails-on-load`).
const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(15));

pub(super) struct SweepFixture {
    pub(super) root: PathBuf,
    pub(super) shared: Arc<Shared>,
    background: Option<Background>,
}

impl SweepFixture {
    /// A shared installation whose launcher runs `script`; its sweep starts with `start`. Its
    /// project's hub stands for one whose first board read is done and found a board (a board
    /// still starting), so its maintenance runs.
    pub(super) fn new(script: &str) -> Self {
        let fixture = Self::without_hub(script);
        let hub = crate::live::Hub::new(&fixture.root, &fixture.root.join("client"));
        assert!(fixture.shared.api.set(Arc::new(hub)).is_ok());
        fixture
    }

    /// [`SweepFixture::new`] with no hub yet, for a test to give it its own.
    pub(super) fn without_hub(script: &str) -> Self {
        Self::at(script, "")
    }

    /// [`SweepFixture::new`] for a project the one server serves at `base`, `/<id>`.
    fn serving(script: &str, base: &str) -> Self {
        let fixture = Self::at(script, base);
        let hub = crate::live::Hub::new(&fixture.root, &fixture.root.join("client"));
        assert!(fixture.shared.api.set(Arc::new(hub)).is_ok());
        fixture
    }

    fn at(script: &str, base: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "symphony-sweep-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let gate = crate::testing::open_gate();
        let shared = Shared::project(
            i64::from(std::process::id()),
            1,
            &root,
            Some(&root),
            gate,
            base,
        );
        let fixture = Self {
            root,
            shared,
            background: None,
        };
        std::fs::create_dir(fixture.root.join("tools")).unwrap();
        std::fs::create_dir_all(fixture.root.join(".target/bin")).unwrap();
        crate::fixture_script::write(
            &fixture.board_build(),
            &format!("#!/bin/sh\n{script}\n"),
            false,
        );
        fixture
    }

    /// The project's build of `tasks`, which its idle sweep runs: the stand-in running the script.
    pub(super) fn board_build(&self) -> PathBuf {
        self.root.join(".target/bin/tasks")
    }

    /// Own `worker` as the server does, beside any started before, until the fixture is dropped.
    pub(super) fn start(&mut self, worker: std::thread::JoinHandle<()>) {
        let shared = self.shared.clone();
        self.background
            .get_or_insert_with(|| Background {
                shared,
                workers: Vec::new(),
            })
            .workers
            .push(worker);
    }

    /// Wait until `file` holds `expected`; only a stall reaches the backstop, and its failure
    /// says what the file held last and the load average.
    pub(super) fn wait_for(&self, file: &str, expected: &str) {
        let mut read = None;
        let waited = test_time::until_true(
            format!("the sweep to write {expected:?} to {file}"),
            BACKSTOP,
            || {
                read = std::fs::read_to_string(self.root.join(file)).ok();
                read.as_deref() == Some(expected)
            },
        );
        if let Err(stall) = waited {
            panic!("{stall}; {file} holds {read:?}");
        }
    }
}

impl Drop for SweepFixture {
    fn drop(&mut self) {
        drop(self.background.take());
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The rounds of a worker on a [`TestClock`], which the test moves on by this between two.
const INTERVAL: Duration = Duration::from_secs(300);

/// A reap child's timeout on a [`TestClock`]: the clock moves only by an [`INTERVAL`] at a time,
/// a few times a test, so no round reaches it; a round that never ends is the backstop's.
const ROUND_TIMEOUT: Duration = Duration::from_secs(86_400);

/// Start `fixture`'s maintenance on `clock`: its first round at once, each next one as the test
/// moves the clock on ([`next_round`]).
fn maintain_on(fixture: &mut SweepFixture, clock: &TestClock) {
    fixture.start(maintenance_worker(
        fixture.shared.clone(),
        INTERVAL,
        ROUND_TIMEOUT,
        clock.reader(),
        None,
    ));
}

/// Once the worker on `clock` has waited `waits` times in all, the last after the round before,
/// move the clock on by an [`INTERVAL`]: its wait ends, and the next round is due.
fn next_round(clock: &TestClock, waits: usize) {
    clock.waited(waits);
    clock.advance(INTERVAL);
}

#[test]
fn the_shared_installation_ends_a_hung_reap_child_at_its_sweep_timeout() {
    // The deadline is the shared installation's own SWEEP_TIMEOUT, read on a clock this test
    // moves: the child below never ends by itself.
    let clock = TestClock::new();
    let mut fixture = SweepFixture::new("printf '%s\\n' \"$$\" > entered; exec sleep 86400");
    let announced = crate::testing::fifo(&fixture.root.join("entered"));
    fixture
        .start(session_maintenance(&fixture.shared, Some(&fixture.root), clock.reader()).unwrap());
    let first = crate::testing::entered(&announced);
    // The child may announce itself before its sweep reads the start of the deadline; the
    // next read is that start or comes after it.
    clock.read_at(None, 1);
    clock.set(SWEEP_TIMEOUT - Duration::from_secs(1));
    assert!(
        sys::pid_alive(first),
        "the reap child was ended before its timeout"
    );
    clock.set(SWEEP_TIMEOUT);
    assert!(
        !sys::pid_alive(first),
        "the reap child outlived its {SWEEP_TIMEOUT:?} timeout"
    );
    // The next round starts at the reap interval, with a child of its own.
    clock.set(Duration::from_secs_f64(REAP_SECONDS));
    let second = crate::testing::entered(&announced);
    assert_ne!(first, second);
    drop(fixture);
    assert!(!sys::pid_alive(second), "shutdown ends the round in flight");
}

#[test]
fn the_shared_installation_sweeps_at_startup_before_its_normal_interval() {
    // The child never ends by itself, so only a cancellation ends the drop before the sweep's
    // four-minute deadline, which is past the backstop below.
    let mut fixture = SweepFixture::new("printf '%s' \"$*\" > started; exec sleep 86400");
    use std::os::unix::fs::MetadataExt;
    let other = SweepFixture::new("exit 0");
    let first = std::fs::metadata(fixture.board_build()).unwrap();
    let second = std::fs::metadata(other.board_build()).unwrap();
    assert_eq!((first.dev(), first.ino()), (second.dev(), second.ino()));
    fixture.start(session_maintenance(&fixture.shared, Some(&fixture.root), MachineClock).unwrap());
    fixture.wait_for("started", "reap");
    const { assert!(BACKSTOP.as_secs() < SWEEP_TIMEOUT.as_secs()) };
    let stopping = std::thread::spawn(move || drop(fixture));
    test_time::until_true(
        "shutdown to cancel an active sweep instead of waiting for its four-minute deadline",
        BACKSTOP,
        || stopping.is_finished(),
    )
    .unwrap();
    stopping.join().unwrap();
}

#[test]
fn the_board_build_is_the_main_checkouts_link_else_the_search_paths_else_where_one_goes() {
    use std::os::unix::fs::PermissionsExt;
    let tree = crate::testing::Tree::new();
    let program = |rel: &str| {
        let path = tree.path(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    };
    // A main checkout and a linked worktree of it, as git marks them.
    std::fs::create_dir_all(tree.path("main/.git/worktrees/wt")).unwrap();
    std::fs::write(tree.path("main/.git/worktrees/wt/commondir"), "../..\n").unwrap();
    std::fs::create_dir_all(tree.path("wt")).unwrap();
    std::fs::write(
        tree.path("wt/.git"),
        format!(
            "gitdir: {}\n",
            tree.path("main/.git/worktrees/wt").display()
        ),
    )
    .unwrap();
    let main = tree.path("main").canonicalize().unwrap();
    let path = tree.path("path").into_os_string();
    let beyond = Beyond::path(Some(&path));
    let missing = format!(
        "no build of tasks for {}: link the project's build at {}/.target/bin/tasks, install a \
         release on the machine or put tasks on PATH",
        main.display(),
        main.display()
    );
    assert_eq!(board_build_on(&tree.path("wt"), &beyond), Err(missing));
    program("wt/.target/bin/tasks"); // a branch's build, never the board's
    let installed = program("path/tasks");
    assert_eq!(board_build_on(&tree.path("wt"), &beyond), Ok(installed));
    let linked = program("main/.target/bin/tasks");
    assert_eq!(
        board_build_on(&tree.path("wt"), &beyond),
        Ok(main.join(".target/bin/tasks"))
    );
    assert_eq!(
        board_build_on(&main, &Beyond::default()),
        Ok(linked.canonicalize().unwrap())
    );
}

#[test]
fn a_sweep_is_due_when_either_clock_passes_the_interval() {
    let interval = Duration::from_secs(300);
    let (mono, wall) = (
        Instant::now(),
        SystemTime::UNIX_EPOCH + Duration::from_secs(1 << 30),
    );
    let last = Stamp { mono, wall };
    let at = |awake: u64, elapsed: u64| Stamp {
        mono: mono + Duration::from_secs(awake),
        wall: wall + Duration::from_secs(elapsed),
    };
    assert!(sweep_due(None, last, interval), "the first round is due");
    assert!(!sweep_due(Some(last), at(299, 299), interval));
    assert!(sweep_due(Some(last), at(300, 300), interval));
    // A 32-minute sleep: the monotonic clock advanced one second, the wall clock 1920.
    assert!(
        sweep_due(Some(last), at(1, 1920), interval),
        "a wake runs a round at once"
    );
    // The wall clock set back must not hold a round off.
    let back = Stamp {
        mono: mono + Duration::from_secs(300),
        wall: wall - Duration::from_secs(3600),
    };
    assert!(sweep_due(Some(last), back, interval));
    assert!(!sweep_due(
        Some(last),
        Stamp {
            wall: wall - Duration::from_secs(5),
            ..at(10, 0)
        },
        interval
    ));
}

#[test]
fn a_round_is_reported_late_by_its_whole_delay_past_the_threshold() {
    let interval = Duration::from_secs(300);
    let (mono, wall) = (
        Instant::now(),
        SystemTime::UNIX_EPOCH + Duration::from_secs(1 << 30),
    );
    let last = Stamp { mono, wall };
    let after = |secs: u64| Stamp {
        mono,
        wall: wall + Duration::from_secs(secs),
    };
    assert_eq!(sweep_late(last, after(300), interval), None);
    assert_eq!(
        sweep_late(last, after(330), interval),
        None,
        "30 s is the threshold itself"
    );
    assert_eq!(
        sweep_late(last, after(1920), interval),
        Some(Duration::from_secs(1620)),
        "a 32-minute sleep is 1620 s past the interval"
    );
    assert_eq!(sweep_late(last, after(10), interval), None);
}

#[test]
fn each_sweep_is_recorded_in_the_boards_health_failed_with_its_error_or_ok() {
    let mut fixture = SweepFixture::without_hub(
        "if [ ! -f failed ]; then touch failed; echo 'the board did not answer' >&2; exit 3; fi",
    );
    let (hub, clock) = clocked_hub(&fixture);
    let at = clock.now();
    let sweep = TestClock::new();
    maintain_on(&mut fixture, &sweep);
    next_round(&sweep, 1);
    let report = waited("no sweep was recorded ok", || {
        Some(hub.health.report(at)).filter(|report| report.sweep.ok > 0)
    });
    assert_eq!(report.sweep.failed, 1, "{report:?}");
    assert_eq!(
        report.sweep.error.as_deref(),
        Some("exit status: 3: the board did not answer")
    );
    assert_eq!(
        report.warnings.len(),
        1,
        "a good sweep alone does not clear it: {:?}",
        report.warnings
    );
    // The failed sweep told the pages before the good one was recorded.
    assert_eq!(*hub.server_notice.borrow(), SWEEP_FAILING);

    // A minute on the hub's clock, the next good sweep clears it.
    clock.set(at + symphony_board::board_health::CLEARS_AFTER);
    next_round(&sweep, 2);
    waited("the pages were not told the sweep is well", || {
        hub.server_notice.borrow().is_empty().then_some(())
    });
    assert_eq!(
        hub.health.report(clock.now()).warnings,
        Vec::<String>::new()
    );
}

#[test]
fn a_sweep_that_finds_no_board_is_not_counted_failed() {
    let script = "if [ ! -f failed ]; then touch failed; echo 'tasks: the main board is not made \
                  yet, and a reader never makes a board: nothing was made or started; `just tasks \
                  list` in main makes it, on PostgreSQL' >&2; exit 1; fi";
    let mut fixture = SweepFixture::without_hub(script);
    let (hub, clock) = clocked_hub(&fixture);
    let at = clock.now();
    let sweep = TestClock::new();
    maintain_on(&mut fixture, &sweep);
    next_round(&sweep, 1);
    let report = waited("no sweep was recorded ok", || {
        Some(hub.health.report(at)).filter(|report| report.sweep.ok > 0)
    });
    assert!(
        fixture.root.join("failed").exists(),
        "the first sweep ran and found no board"
    );
    assert_eq!(
        (report.sweep.failed, report.warnings.len()),
        (0, 0),
        "{report:?}"
    );
    assert_eq!(*hub.server_notice.borrow(), "");
}

/// A board still on SurrealDB is refused, naming how it moves, and is a board all the same: a sweep
/// that meets it failed.
#[test]
fn a_sweep_whose_board_is_still_on_surrealdb_is_counted_failed() {
    let mut fixture = SweepFixture::without_hub(
        "if [ ! -f failed ]; then touch failed; printf 'tasks: ' >&2; cat unserved >&2; exit 1; fi",
    );
    let unserved = format!(
        "the board in {} is on SurrealDB, which this build serves no more: {}. Nothing was started",
        fixture.root.join(".git/symphony/tasks").display(),
        symphony_board::server::GONE
    );
    std::fs::write(fixture.root.join("unserved"), &unserved).unwrap();
    let (hub, clock) = clocked_hub(&fixture);
    let at = clock.now();
    let sweep = TestClock::new();
    maintain_on(&mut fixture, &sweep);
    next_round(&sweep, 1);
    let report = waited("no sweep was recorded ok", || {
        Some(hub.health.report(at)).filter(|report| report.sweep.ok > 0)
    });
    assert_eq!(report.sweep.failed, 1, "{report:?}");
    assert_eq!(
        report.sweep.error.as_deref(),
        Some(format!("exit status: 1: tasks: {unserved}").as_str())
    );
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert_eq!(*hub.server_notice.borrow(), SWEEP_FAILING);
}

const SWEEP_FAILING: &str =
    "The task board is unwell: its idle sweep is failing. Run “just tasks status” to see why.";

/// The hub's clock, as the test moves it.
struct HubClock(Arc<std::sync::atomic::AtomicU64>);

impl HubClock {
    fn now(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::SeqCst))
    }

    fn set(&self, at: f64) {
        self.0.store(at.to_bits(), Ordering::SeqCst);
    }
}

/// A hub for `fixture`'s server, its clock standing at the wall clock's time until the test
/// moves it.
fn clocked_hub(fixture: &SweepFixture) -> (Arc<crate::live::Hub>, HubClock) {
    let time = Arc::new(std::sync::atomic::AtomicU64::new(
        symphony_board::sweep::trigger::now().to_bits(),
    ));
    let mut hub = crate::live::Hub::new(&fixture.root, &fixture.root.join("client"));
    let read = HubClock(Arc::clone(&time));
    hub.clock = Box::new(move || read.now());
    let hub = Arc::new(hub);
    assert!(fixture.shared.api.set(hub.clone()).is_ok());
    (hub, HubClock(time))
}

/// What `seen` sees first; `waiting` says what did not come when only a stall reaches the
/// backstop.
fn waited<T>(waiting: &str, seen: impl FnMut() -> Option<T>) -> T {
    test_time::until(waiting, BACKSTOP, seen).unwrap()
}

#[test]
fn a_failing_sweep_is_said_on_every_page_of_the_project() {
    let mut fixture = SweepFixture::without_hub("echo 'the board did not answer' >&2; exit 3");
    let (hub, _) = clocked_hub(&fixture);
    maintain_on(&mut fixture, &TestClock::new());
    waited("no failed sweep was said", || {
        (!hub.server_notice.borrow().is_empty()).then_some(())
    });
    assert_eq!(*hub.server_notice.borrow(), SWEEP_FAILING);
}

#[test]
fn a_failed_sweeps_long_error_is_kept_on_one_short_line_in_the_record() {
    // `tasks reap`'s long stderr: many lines, then its error.
    let mut fixture = SweepFixture::without_hub(
        "i=0; while [ $i -lt 300 ]; do echo \"Compiling crate $i\" >&2; i=$((i+1)); done\n\
         echo 'error: the board did not answer' >&2; exit 3",
    );
    let (hub, clock) = clocked_hub(&fixture);
    maintain_on(&mut fixture, &TestClock::new());
    let report = waited("no failed sweep was recorded", || {
        Some(hub.health.report(clock.now())).filter(|report| report.sweep.failed > 0)
    });
    let stderr: String = (0..300)
        .map(|i| format!("Compiling crate {i}\n"))
        .collect::<String>()
        + "error: the board did not answer";
    let whole = format!("exit status: 3: {stderr}");
    let error = report.sweep.error.clone().unwrap();
    assert_eq!(
        error,
        symphony_board::board_health::shortened(&whole),
        "cut to its first and last characters"
    );
    assert!(
        error.starts_with("exit status: 3: Compiling crate 0 ⏎ Compiling crate 1 ⏎ ")
            && error.ends_with("Compiling crate 299 ⏎ error: the board did not answer")
            && error.chars().count() < whole.chars().count() / 4,
        "{error}"
    );
    let lines = report.lines(clock.now(), &report.warnings);
    assert!(
        lines.iter().all(|line| !line.contains('\n')),
        "the status prints it on one line: {lines:?}"
    );
}

#[test]
fn a_held_lifecycle_lock_does_not_stall_the_sweep_rounds() {
    // Hub startup holds the lifecycle lock across its board's calls; the rounds must go on.
    let mut fixture = SweepFixture::new(
        "if [ ! -f started ]; then printf first > started; exit 0; fi\nprintf again > retried",
    );
    let sweep = TestClock::new();
    let shared = fixture.shared.clone();
    let _hub_starting = shared.lifecycle.lock().unwrap();
    maintain_on(&mut fixture, &sweep);
    fixture.wait_for("started", "first");
    next_round(&sweep, 1);
    fixture.wait_for("retried", "again");
}

/// The lines of `file` in the fixture's checkout, none while it is not there.
fn lines(fixture: &SweepFixture, file: &str) -> Vec<String> {
    std::fs::read_to_string(fixture.root.join(file))
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

/// A project whose hub has not read its board, or read none there, is not maintained: its sweep
/// looks at the round due, waits for its hub to find a board, looking again after the backstop's
/// period on its clock (which this test never moves), and runs nothing, so it makes no board where
/// none is. Each read of the hub that finds a board or finds none wakes it, and once one finds a
/// scratch board here, the round runs.
#[test]
fn a_project_is_maintained_only_once_its_hub_has_read_a_board_there() {
    let mut fixture = SweepFixture::without_hub("printf '%s\\n' \"$1\" >> calls");
    let sweep = TestClock::new();
    maintain_on(&mut fixture, &sweep);
    // No hub yet: whether there is a board is not known, and the round due waits for it.
    assert_eq!(sweep.waited(1), [AFTER_A_SLEEP]);
    assert_eq!(lines(&fixture, "calls"), Vec::<String>::new());

    // Its hub's reach found no board: none is made yet, and a reader never makes one. That read
    // wakes the sweep, which looks and waits again.
    let hub = Arc::new(crate::live::Hub::new(
        &fixture.root,
        &fixture.root.join("client"),
    ));
    hub.wake_at_board(fixture.shared.board_wakers.clone());
    assert!(fixture.shared.api.set(hub.clone()).is_ok());
    let missing = "the main board is not made yet, and a reader never makes a board: nothing was \
                   made or started; `just tasks list` in main makes it, on PostgreSQL"
        .to_string();
    assert!(
        hub.reconnect(&mut |_: &Path| Err(missing.clone()))
            .is_none()
    );
    assert_eq!(hub.has_board(), Some(false));
    assert_eq!(sweep.waited(2), [AFTER_A_SLEEP; 2]);
    assert_eq!(lines(&fixture, "calls"), Vec::<String>::new());

    // The hub's next read finds a board: it wakes the sweep, whose round runs.
    let board = crate::testing::board::Fixture::empty();
    hub.board_read(&board.store).unwrap();
    assert_eq!(hub.has_board(), Some(true));
    sweep.waited(3);
    assert_eq!(lines(&fixture, "calls"), ["reap"]);
}

/// Between two rounds the sweep waits on its clock for the time left until the next is due, at
/// most the backstop's period (a machine that slept says nothing of it): a move of the clock wakes
/// it, a move short of the interval runs nothing, and the move to the interval runs the round. Its
/// stop ends a wait that no move would.
#[test]
fn a_sweep_waits_on_its_clock_for_the_next_round_and_its_stop_ends_the_wait() {
    let mut fixture = SweepFixture::new("printf '%s\\n' \"$1\" >> calls");
    let sweep = TestClock::new();
    maintain_on(&mut fixture, &sweep);
    assert_eq!(sweep.waited(1), [AFTER_A_SLEEP]);
    assert_eq!(lines(&fixture, "calls"), ["reap"]);
    // Five seconds short of the interval: the wait is for those five.
    sweep.move_to(INTERVAL - Duration::from_secs(5));
    assert_eq!(sweep.waited(2), [AFTER_A_SLEEP, Duration::from_secs(5)]);
    assert_eq!(
        lines(&fixture, "calls"),
        ["reap"],
        "no round before its time"
    );
    sweep.move_to(INTERVAL);
    assert_eq!(
        sweep.waited(3)[2],
        AFTER_A_SLEEP,
        "the next round's whole wait"
    );
    assert_eq!(lines(&fixture, "calls"), ["reap", "reap"]);
    let stopping = std::thread::spawn(move || drop(fixture));
    test_time::until_true(
        "the stop to end the sweep's wait on a clock that does not move",
        BACKSTOP,
        || stopping.is_finished(),
    )
    .unwrap();
    stopping.join().unwrap();
    assert_eq!(
        sweep.waits(),
        3,
        "the stop ends the worker, no look after it"
    );
}

#[test]
fn the_time_left_to_a_round_is_the_sooner_of_its_two_clocks() {
    let interval = Duration::from_secs(300);
    let (mono, wall) = (
        Instant::now(),
        SystemTime::UNIX_EPOCH + Duration::from_secs(1 << 30),
    );
    let last = Stamp { mono, wall };
    let at = |awake: u64, elapsed: u64| Stamp {
        mono: mono + Duration::from_secs(awake),
        wall: wall + Duration::from_secs(elapsed),
    };
    let secs = Duration::from_secs;
    assert_eq!(sweep_left(last, last, interval), interval);
    assert_eq!(sweep_left(last, at(120, 120), interval), secs(180));
    // A sleep: the wall clock went on while the monotonic one stood.
    assert_eq!(sweep_left(last, at(10, 200), interval), secs(100));
    // The wall clock set back counts for nothing, and the monotonic one still runs out.
    let back = Stamp {
        mono: mono + secs(30),
        wall: wall - secs(3600),
    };
    assert_eq!(sweep_left(last, back, interval), secs(270));
    assert_eq!(sweep_left(last, at(300, 300), interval), Duration::ZERO);
    assert_eq!(sweep_left(last, at(1, 1920), interval), Duration::ZERO);
}

#[test]
fn each_round_is_told_to_the_hub_whose_tick_waits_after_a_sleep_for_the_next() {
    let mut fixture = SweepFixture::without_hub("exit 0");
    let (hub, clock) = clocked_hub(&fixture);
    let at = clock.now();
    // Before any round, the tick judges, however far the wall clock went.
    clock.set(at + 3600.0);
    assert!(hub.judges());
    clock.set(at);
    let sweep = TestClock::new();
    maintain_on(&mut fixture, &sweep);
    waited("no first round was recorded", || {
        Some(hub.health.report(at)).filter(|report| report.sweep.ok > 0)
    });
    assert!(hub.judges());
    // An hour on the hub's wall clock and next to none awake: the machine slept.
    clock.set(at + 3600.0);
    assert!(!hub.judges());
    next_round(&sweep, 1);
    waited("the tick still waits after the round past the wake", || {
        hub.judges().then_some(())
    });
}

#[test]
fn each_round_of_the_one_servers_sweep_tries_its_projects_held_update_first_on_its_clock() {
    use symphony::machine::updates::{Ledger, Update};
    // alpha, served at /alpha, keeps a board whose database is down; the release this server
    // stands for is installed and named by `current`, and alpha's update to its schema is open.
    let mut fixture = SweepFixture::serving("echo round >> rounds", "/alpha");
    let done = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&fixture.root)
        .output()
        .unwrap();
    assert!(done.status.success());
    std::fs::create_dir_all(fixture.root.join(".git/symphony/tasks")).unwrap();
    let tree = crate::testing::Tree::new();
    let machine = tree.path("machine");
    let release = "release-2026-11-20";
    let folder = symphony::machine::release::folder(&machine, release);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join(symphony::machine::release::MANIFEST),
        serde_json::json!({
            "release": release, "commit": "0123456", "schema": symphony_board::SCHEMA,
            "protocol": {"major": 0, "minor": 3}, "tasks_api": {"major": 2, "minor": 2}, "pin": 1,
            "client": "client/x", "artifacts": [],
        })
        .to_string(),
    )
    .unwrap();
    std::os::unix::fs::symlink(Path::new("releases").join(release), machine.join("current"))
        .unwrap();
    let entry: symphony::machine::Entry = serde_json::from_value(serde_json::json!({
        "id": "alpha", "name": "alpha", "root": fixture.root.canonicalize().unwrap(), "pin": 1,
    }))
    .unwrap();
    symphony::machine::Registry {
        projects: vec![entry],
    }
    .write(&machine)
    .unwrap();
    let open = || {
        Ledger {
            version: symphony::machine::updates::VERSION,
            updates: vec![Update::new("alpha", symphony_board::SCHEMA, 1, None)],
        }
        .write(&machine)
        .unwrap();
    };
    let failed = || {
        let ledger = Ledger::read(&machine).unwrap();
        let update = &ledger.updates[0];
        assert!(update.open(), "{update:?}");
        (update.failures.iter())
            .map(|failure| (failure.at, failure.run.clone()))
            .collect::<Vec<_>>()
    };
    open();
    let clock = TestClock::new();
    fixture.start(maintenance_worker(
        fixture.shared.clone(),
        INTERVAL,
        ROUND_TIMEOUT,
        clock.reader(),
        Some(crate::held::Retrying {
            machine: machine.clone(),
            release: release.into(),
        }),
    ));
    // The round's reap comes after its retry: the ledger was written before it.
    fixture.wait_for("rounds", "round\n");
    let first = failed();
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(first[0].1, format!("the machine's server, {release}"));
    // The next round, an interval on: the update tried again, its time read on the same clock.
    open();
    next_round(&clock, 1);
    fixture.wait_for("rounds", "round\nround\n");
    let second = failed();
    assert_eq!(second.len(), 1, "{second:?}");
    assert_eq!(second[0].0 - first[0].0, INTERVAL.as_secs_f64());
    assert!(
        symphony::machine::updates::try_lock(&machine)
            .unwrap()
            .is_some(),
        "the round let the machine's updates lock go"
    );
}

#[test]
fn a_server_serving_its_one_project_at_the_root_tries_no_held_update() {
    let fixture = SweepFixture::new("exit 0");
    let tree = crate::testing::Tree::new();
    let retrying = crate::held::Retrying {
        machine: tree.path("machine"),
        release: "release-2026-11-20".into(),
    };
    let clock = TestClock::new();
    let said = crate::held::round(&fixture.shared, &retrying, ROUND_TIMEOUT, &clock.reader());
    assert_eq!(said, Vec::<String>::new());
    assert!(
        !tree.path("machine").exists(),
        "nothing was read or made there"
    );
}
