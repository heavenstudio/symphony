//! The shared installation's idle-session sweep and embedding maintenance, on fixture launchers
//! and clocks.

use super::*;
use std::sync::atomic::AtomicUsize;

pub(super) struct SweepFixture {
    pub(super) root: PathBuf,
    pub(super) shared: Arc<Shared>,
    background: Option<Background>,
}

impl SweepFixture {
    pub(super) fn with_embedding_launcher(script: &str) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let fixture = Self::new(script);
        let launcher = fixture.root.join("tools/tasks.sh");
        std::fs::write(&launcher, format!("#!/bin/sh\n{script}\n")).unwrap();
        assert!(std::fs::symlink_metadata(&launcher).unwrap().is_file());
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o644)).unwrap();
        fixture
    }

    /// A shared installation whose launcher runs `script`; its sweep starts with `start`.
    pub(super) fn new(script: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "sf-sweep-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let gate = Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now));
        let shared = Shared::new(i64::from(std::process::id()), 1, &root, Some(&root), gate);
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

    /// Own `worker` as the server does, until the fixture is dropped.
    pub(super) fn start(&mut self, worker: std::thread::JoinHandle<()>) {
        self.background = Some(Background {
            shared: self.shared.clone(),
            workers: vec![worker],
        });
    }

    pub(super) fn wait_for(&self, file: &str, expected: &str) {
        let began = Instant::now();
        loop {
            let read = std::fs::read_to_string(self.root.join(file)).ok();
            if read.as_deref() == Some(expected) {
                return;
            }
            assert!(
                began.elapsed() < Duration::from_secs(15),
                "sweep did not write {file}: it holds {read:?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for SweepFixture {
    fn drop(&mut self) {
        drop(self.background.take());
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A clock only the test moves. Every read is counted, so the test knows when the worker has
/// looked at a time it set and acted on it.
#[derive(Clone)]
struct TestClock {
    start: Stamp,
    /// How far past `start` the clock reads, and how often it was read.
    state: Arc<(Mutex<(Duration, u64)>, Condvar)>,
}

impl TestClock {
    fn new() -> Self {
        Self {
            start: Stamp::now(),
            state: Arc::default(),
        }
    }

    fn reader(&self) -> impl Fn() -> Stamp + Send + 'static {
        let clock = self.clone();
        move || {
            let (state, read) = &*clock.state;
            let mut state = state.lock().unwrap();
            state.1 += 1;
            read.notify_all();
            Stamp {
                mono: clock.start.mono + state.0,
                wall: clock.start.wall + state.0,
            }
        }
    }

    /// Set the clock to `elapsed` past its start, when given, then wait for `more` reads.
    fn read_at(&self, elapsed: Option<Duration>, more: u64) {
        let (state, read) = &*self.state;
        let mut state = state.lock().unwrap();
        if let Some(elapsed) = elapsed {
            state.0 = elapsed;
        }
        let seen = state.1;
        let (_state, waited) = read
            .wait_timeout_while(state, Duration::from_secs(30), |state| {
                state.1 < seen + more
            })
            .unwrap();
        assert!(!waited.timed_out(), "the sweep stopped reading its clock");
    }

    /// Move the clock to `elapsed` past its start. The first read after decides on that time;
    /// the second comes only once the worker has acted on the decision.
    fn set(&self, elapsed: Duration) {
        self.read_at(Some(elapsed), 2);
    }
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
    let mut fixture = SweepFixture::new("printf '%s' \"$*\" > started; exec sleep 30");
    use std::os::unix::fs::MetadataExt;
    let other = SweepFixture::new("exit 0");
    let first = std::fs::metadata(fixture.board_build()).unwrap();
    let second = std::fs::metadata(other.board_build()).unwrap();
    assert_eq!((first.dev(), first.ino()), (second.dev(), second.ino()));
    fixture.start(session_maintenance(&fixture.shared, Some(&fixture.root), Stamp::now).unwrap());
    fixture.wait_for("started", "reap");
    let stopping = Instant::now();
    drop(fixture);
    assert!(
        stopping.elapsed() < Duration::from_secs(5),
        "shutdown must cancel an active sweep instead of waiting for its four-minute deadline"
    );
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
    let missing = format!(
        "no build of tasks for {}: link the project's build at {}/.target/bin/tasks or put tasks \
         on PATH",
        main.display(),
        main.display()
    );
    assert_eq!(board_build_on(&tree.path("wt"), Some(&path)), Err(missing));
    program("wt/.target/bin/tasks"); // a branch's build, never the board's
    let installed = program("path/tasks");
    assert_eq!(board_build_on(&tree.path("wt"), Some(&path)), Ok(installed));
    let linked = program("main/.target/bin/tasks");
    assert_eq!(
        board_build_on(&tree.path("wt"), Some(&path)),
        Ok(main.join(".target/bin/tasks"))
    );
    assert_eq!(
        board_build_on(&main, None),
        Ok(linked.canonicalize().unwrap())
    );
}

#[test]
fn a_private_preview_never_starts_embedding_maintenance() {
    let fixture = SweepFixture::with_embedding_launcher("echo wrong > started");
    assert!(embeddings::start(&fixture.shared, None, Stamp::now).is_none());
    assert!(!fixture.root.join("started").exists());
}

/// Subprocess entry: measure the launched process rather than reconstructing its command.
#[test]
fn embedding_child_resource_probe() {
    let Ok(args) = std::env::var("SF_EMBEDDING_RESOURCE_PROBE") else {
        return;
    };
    // SAFETY: getpriority reads this process's scheduling priority without mutating it.
    let priority = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
    std::fs::write("resources", format!("{priority}\n{args}\n")).unwrap();
    std::fs::write("resources-ready", "yes").unwrap();
}

#[test]
fn embedding_maintenance_runs_a_non_executable_launcher_niced_with_one_inference_thread() {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: getpriority reads this process's inherited scheduling priority.
    let inherited = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
    let exe = std::env::current_exe().unwrap();
    let exe = exe.to_str().unwrap().replace('\'', "'\\''");
    let script = format!(
        "SF_EMBEDDING_RESOURCE_PROBE=\"$*\" exec '{exe}' --exact front::sweep_tests::embedding_child_resource_probe --nocapture"
    );
    let mut fixture = SweepFixture::with_embedding_launcher(&script);
    assert_eq!(
        std::fs::metadata(fixture.root.join("tools/tasks.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o644
    );
    fixture.start(embeddings::start(&fixture.shared, Some(&fixture.root), Stamp::now).unwrap());
    fixture.wait_for("resources-ready", "yes");
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("resources")).unwrap(),
        format!(
            "{}\nembed --maintenance --threads 1\n",
            (inherited + 10).min(19)
        ),
        "the actual child must inherit nice +10 and select exactly one inference thread"
    );
}

#[test]
fn embedding_maintenance_starts_at_startup_and_retries_a_failed_pass_each_minute() {
    let clock = TestClock::new();
    let mut fixture = SweepFixture::with_embedding_launcher(
        "printf '%s\\n' \"$*\" >> calls; if [ ! -f attempted ]; then touch attempted; exit 1; fi",
    );
    fixture.start(embeddings::start(&fixture.shared, Some(&fixture.root), clock.reader()).unwrap());
    fixture.wait_for("calls", "embed --maintenance --threads 1\n");
    clock.set(embeddings::INTERVAL - Duration::from_secs(1));
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("calls")).unwrap(),
        "embed --maintenance --threads 1\n"
    );
    clock.set(embeddings::INTERVAL);
    fixture.wait_for(
        "calls",
        "embed --maintenance --threads 1\nembed --maintenance --threads 1\n",
    );
}

#[test]
fn embedding_maintenance_bounds_a_hung_pass_and_cancels_its_owned_child_on_shutdown() {
    let clock = TestClock::new();
    let mut fixture =
        SweepFixture::with_embedding_launcher("printf '%s\\n' \"$$\" > entered; exec sleep 86400");
    let announced = crate::testing::fifo(&fixture.root.join("entered"));
    fixture.start(embeddings::start(&fixture.shared, Some(&fixture.root), clock.reader()).unwrap());
    let first = crate::testing::entered(&announced);
    clock.read_at(None, 1);
    clock.set(embeddings::TIMEOUT - Duration::from_secs(1));
    assert!(
        sys::pid_alive(first),
        "a first backfill gets its own runtime budget"
    );
    clock.set(embeddings::TIMEOUT);
    assert!(!sys::pid_alive(first), "a hung pass is bounded");
    let second = crate::testing::entered(&announced);
    assert_ne!(first, second);
    drop(fixture);
    assert!(!sys::pid_alive(second), "shutdown ends the pass in flight");
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
    let mut fixture = SweepFixture::new(
        "if [ ! -f failed ]; then touch failed; echo 'the board did not answer' >&2; exit 3; fi",
    );
    let (hub, clock) = clocked_hub(&fixture);
    let at = clock.now();
    fixture.start(maintenance_worker(
        fixture.shared.clone(),
        Duration::ZERO,
        Duration::from_secs(3),
        Stamp::now,
    ));
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
    let script = format!(
        "if [ ! -f failed ]; then touch failed; echo 'tasks: SurrealDB {} is not at /nowhere/surreal, \
         where `SF_SURREAL` names it' >&2; exit 1; fi",
        symphony_board::server::SURREAL_VERSION
    );
    let mut fixture = SweepFixture::new(&script);
    let (hub, clock) = clocked_hub(&fixture);
    let at = clock.now();
    fixture.start(maintenance_worker(
        fixture.shared.clone(),
        Duration::ZERO,
        Duration::from_secs(3),
        Stamp::now,
    ));
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

#[test]
fn a_sweep_whose_board_store_cannot_be_served_is_counted_failed() {
    let mut fixture = SweepFixture::new(
        "if [ ! -f failed ]; then touch failed; printf 'tasks: ' >&2; cat unserved >&2; exit 1; fi",
    );
    let unserved = symphony_board::server::ServerError::Unserved {
        surreal: "/nowhere/surreal".into(),
        data: fixture.root.join(".git/sf-tasks/data"),
    };
    std::fs::write(fixture.root.join("unserved"), unserved.to_string()).unwrap();
    let (hub, clock) = clocked_hub(&fixture);
    let at = clock.now();
    fixture.start(maintenance_worker(
        fixture.shared.clone(),
        Duration::ZERO,
        Duration::from_secs(3),
        Stamp::now,
    ));
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
struct Clock(Arc<std::sync::atomic::AtomicU64>);

impl Clock {
    fn now(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::SeqCst))
    }

    fn set(&self, at: f64) {
        self.0.store(at.to_bits(), Ordering::SeqCst);
    }
}

/// A hub for `fixture`'s server, its clock standing at the wall clock's time until the test
/// moves it.
fn clocked_hub(fixture: &SweepFixture) -> (Arc<crate::live::Hub>, Clock) {
    let time = Arc::new(std::sync::atomic::AtomicU64::new(
        symphony_board::sweep::trigger::now().to_bits(),
    ));
    let mut hub = crate::live::Hub::new(&fixture.root, &fixture.root.join("client"));
    let read = Clock(Arc::clone(&time));
    hub.clock = Box::new(move || read.now());
    let hub = Arc::new(hub);
    assert!(fixture.shared.api.set(hub.clone()).is_ok());
    (hub, Clock(time))
}

/// What `seen` sees first, within fifteen seconds; `waiting` says what did not come.
fn waited<T>(waiting: &str, mut seen: impl FnMut() -> Option<T>) -> T {
    let began = Instant::now();
    loop {
        if let Some(seen) = seen() {
            return seen;
        }
        assert!(began.elapsed() < Duration::from_secs(15), "{waiting}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_failing_sweep_is_said_on_every_page_of_the_project() {
    let mut fixture = SweepFixture::new("echo 'the board did not answer' >&2; exit 3");
    let (hub, _) = clocked_hub(&fixture);
    fixture.start(maintenance_worker(
        fixture.shared.clone(),
        Duration::ZERO,
        Duration::from_secs(3),
        Stamp::now,
    ));
    waited("no failed sweep was said", || {
        (!hub.server_notice.borrow().is_empty()).then_some(())
    });
    assert_eq!(*hub.server_notice.borrow(), SWEEP_FAILING);
}

#[test]
fn a_failed_sweeps_long_error_is_kept_on_one_short_line_in_the_record() {
    // `tasks reap`'s long stderr: many lines, then its error.
    let mut fixture = SweepFixture::new(
        "i=0; while [ $i -lt 300 ]; do echo \"Compiling crate $i\" >&2; i=$((i+1)); done\n\
         echo 'error: the board did not answer' >&2; exit 3",
    );
    let (hub, clock) = clocked_hub(&fixture);
    fixture.start(maintenance_worker(
        fixture.shared.clone(),
        Duration::ZERO,
        Duration::from_secs(3),
        Stamp::now,
    ));
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
    // Hub startup holds the lifecycle lock across SurrealDB calls; the rounds must go on.
    let mut fixture = SweepFixture::new(
        "if [ ! -f started ]; then printf first > started; exit 0; fi\nprintf again > retried",
    );
    fixture.start(maintenance_worker(
        fixture.shared.clone(),
        Duration::ZERO,
        Duration::from_secs(3),
        Stamp::now,
    ));
    let _hub_starting = fixture.shared.lifecycle.lock().unwrap();
    fixture.wait_for("started", "first");
    fixture.wait_for("retried", "again");
}
