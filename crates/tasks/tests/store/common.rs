//! What the store's tests share: one PostgreSQL server for the whole test binary, a database of
//! its own per test ([`postgresql`], [`database`]), a settable clock and process table
//! (`tasks_store_test.py`'s `Clock` and `alive`), and a board seeded through the one-way copy
//! ([`Seed`], `seed.rs`).
//!
//! The server is a scratch board's PostgreSQL under the temporary folder, its programs the ones
//! the repository's project file names (`[board] postgresql`), otherwise found on PATH. It starts
//! on its socket alone at the first test that asks for it and ends with the test binary however
//! that ends, as every board's PostgreSQL ends with the process that started it
//! (`symphony_board::server::postgres`); at a normal exit it is stopped and its folder removed.

use std::collections::HashSet;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use symphony_board::health::{LIVE_SECONDS, STALE_SECONDS};
use symphony_board::pg;
use symphony_board::server::{Home, Postgres};
use symphony_board::sys::{Child, Command};
use symphony_board::{Database, Health, NewSession, NewTask, Store, Task};

#[path = "fixture_root.rs"]
mod fixture_root;
#[path = "seed.rs"]
mod seed;
#[allow(
    unused_imports,
    reason = "each test binary including this file uses some of the seed's builders"
)]
pub use seed::{Seed, session, task};

pub const NOW: f64 = 1_800_000_000.0;
/// Past the lost line.
pub const GONE: f64 = LIVE_SECONDS + STALE_SECONDS + 1.0;
pub const HOST: &str = "this-host";
/// What a command's environment in a test sets [`symphony_board::UNANSWERED_WAIT_VAR`] to: a day,
/// past any client's own wait, which is then the unanswered lookup's backstop, so no result turns
/// on how long a loaded machine takes to answer it. A store a test opens itself sets no deadline.
#[allow(
    dead_code,
    reason = "the binaries that run commands read it; the store's own tests open stores alone"
)]
pub const LOOKUP_BACKSTOP: &str = "86400";

static DATABASES: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" {
    fn atexit(at_exit: extern "C" fn()) -> std::ffi::c_int;
}

/// Register a fixture's synchronous cleanup for normal process exit. Local ownership handles
/// unwinding before publication; the lifetime pipe remains the fallback for abrupt process death.
pub fn on_exit(cleanup: extern "C" fn()) {
    // SAFETY: atexit records a no-argument C callback to call once during this process's exit.
    let registered = unsafe { atexit(cleanup) };
    assert_eq!(registered, 0, "fixture cleanup is registered");
}

pub fn project_root() -> PathBuf {
    fixture_root::workspace(Path::new(env!("CARGO_MANIFEST_DIR")))
}

/// `command` started with its stdin a [`symphony_board::sys::pipe`], kept as `child.stdin`, and its other
/// streams as `command` sets them: what the child reads ends once the test drops that input, never
/// later because a child another test spawned took a copy of it.
#[allow(
    dead_code,
    reason = "the test binaries that start children of their own call it; the store's start none"
)]
pub fn spawn_with_input(command: &mut Command) -> std::io::Result<Child> {
    let (input, stdin) = symphony_board::sys::pipe()?;
    let mut child = command.stdin(input).spawn()?;
    child.stdin = Some(OwnedFd::from(stdin).into());
    Ok(child)
}

/// The test binary's PostgreSQL: a scratch board's server under the temporary folder, started on
/// its socket alone at the first test that asks for it, stopped and its folder removed at the
/// binary's exit, and ended with the binary however that ends, as every board's PostgreSQL ends
/// with the process that started it (`symphony_board::server::postgres`). Its programs are the
/// ones the repository's project file names (`[board] postgresql`), otherwise discovered on PATH.
struct PgServer {
    dir: PathBuf,
    pg: Postgres,
    socket: PathBuf,
}

static POSTGRESQL: OnceLock<PgServer> = OnceLock::new();

/// A database of its own on the test binary's PostgreSQL ([`PgServer`]), made by the store that
/// opens it.
#[allow(
    dead_code,
    reason = "each test binary including this file runs on the databases its tests name"
)]
pub fn postgresql() -> pg::Client {
    let server = POSTGRESQL.get_or_init(start_postgresql);
    let db = format!(
        "t{}_{}",
        std::process::id(),
        DATABASES.fetch_add(1, Ordering::SeqCst)
    );
    pg::Client::new(&server.socket, &db)
}

/// A database of its own on the test binary's PostgreSQL, as the store takes it.
#[allow(
    dead_code,
    reason = "each test binary including this file reaches its boards as its tests name"
)]
pub fn database() -> Database {
    postgresql().into()
}

fn start_postgresql() -> PgServer {
    let dir = std::env::temp_dir().join(format!("board-pg-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    let programs = symphony_board::server::postgres::discover_programs(&project_root()).unwrap();
    let pg = Home::at(&dir.join("board"))
        .telling(|_| {})
        .postgresql(&programs);
    let serving = pg
        .ensure()
        .unwrap_or_else(|why| panic!("the tests' PostgreSQL did not start: {why}"));
    on_exit(stop_postgresql);
    PgServer {
        dir,
        pg,
        socket: serving.socket,
    }
}

/// At the test binary's exit: its PostgreSQL stopped and its folder removed.
extern "C" fn stop_postgresql() {
    if let Some(server) = POSTGRESQL.get() {
        let _ = server.pg.stop();
        let _ = std::fs::remove_dir_all(&server.dir);
    }
}

/// A board of its own, its clock at `NOW` and no process alive until a test says so.
pub struct Fixture {
    pub store: Store,
    db: Database,
    clock: Arc<Mutex<f64>>,
    alive: Arc<Mutex<HashSet<i64>>>,
}

impl Fixture {
    /// A board with nothing on it.
    pub fn empty() -> Fixture {
        Fixture::new(Seed::default())
    }

    /// The refusal of `handle`, a session this board does not know: register on this very board,
    /// a test's database of its own, which the command names.
    pub fn no_session(&self, handle: &str) -> String {
        let board = self.store.board_command();
        assert!(board.starts_with("just tasks --database "), "{board}");
        format!("no session '{handle}'; register first (`{board} register`)")
    }

    /// A session registered through the store (`tasks_store_test.py`'s `_session`): its handle.
    pub fn session(&self, kind: &str) -> String {
        self.session_with(NewSession::new(kind))
    }

    pub fn session_with(&self, new: NewSession) -> String {
        self.store
            .register(&new)
            .expect("the session registers")
            .row
            .handle
    }

    /// A task the owner made through the store (`tasks_store_test.py`'s `_task`): "Jump departure".
    pub fn task(&self, slug: &str) -> Task {
        self.task_with(NewTask::new(slug, "Jump departure", "owner"))
    }

    pub fn task_with(&self, new: NewTask) -> Task {
        self.store.create(&new).expect("the task is made")
    }

    /// Seed legacy child-review state for compatibility tests; new slices cannot enter it.
    pub fn historical_review(&self, slug: &str) {
        (self.store.raw().row("task", slug))
            .set(serde_json::json!({"status": "review"}))
            .unwrap();
    }

    /// Every row of each of the board's tables, as [`Store::raw`] reads them past every rule:
    /// what a write refused leaves as it was.
    #[allow(
        dead_code,
        reason = "each test binary including this file reads the board as its tests need"
    )]
    pub fn rows(&self) -> serde_json::Value {
        const TABLES: [&str; 20] = [
            "task",
            "link",
            "session",
            "session_revision",
            "role",
            "message",
            "said",
            "delivery",
            "delivery_key",
            "barred_key",
            "task_session",
            "counter",
            "meta",
            "writer",
            "callback_state",
            "callback_lease",
            "callback_event",
            "chat_event",
            "ask",
            "usage",
        ];
        let raw = self.store.raw();
        (TABLES.iter())
            .map(|table| {
                (
                    table.to_string(),
                    raw.table(table).get().expect("the table reads"),
                )
            })
            .collect::<serde_json::Map<_, _>>()
            .into()
    }

    /// The text of each line on `slug`'s thread.
    pub fn said(&self, slug: &str) -> Vec<String> {
        let thread = self.store.thread(slug).expect("the task is there");
        thread.into_iter().map(|m| m.text).collect()
    }

    /// The session's health as the store reads it now.
    pub fn health(&self, handle: &str) -> Health {
        self.store
            .session(handle)
            .expect("the session is there")
            .health
    }

    /// A board of its own seeded with `seed`.
    pub fn new(seed: Seed) -> Fixture {
        Fixture::in_database(database(), seed)
    }

    /// A board in `db` seeded with `seed`. A seed that does not copy says why and the load average
    /// then, so one that outlasted the client's wait on a loaded machine reads as what it is
    /// (RULES.md `tests/no-test-fails-on-load`).
    pub fn in_database(db: Database, seed: Seed) -> Fixture {
        let clock = Arc::new(Mutex::new(NOW));
        let alive = Arc::new(Mutex::new(HashSet::new()));
        let store = open(&db, &clock, &alive);
        if let Err(why) = store.copy(&seed.dump) {
            panic!("the seed copies: {why:?}; {}", test_time::Load::now());
        }
        Fixture {
            store,
            db,
            clock,
            alive,
        }
    }

    /// Another store on the same board, clock and process table: another session's process.
    pub fn other(&self) -> Store {
        open(&self.db, &self.clock, &self.alive)
    }

    /// Another store on the same board, clock and process table, on another host: its writes
    /// dispatch no callback registered on this one, which hears them from the next dispatch here.
    pub fn elsewhere(&self) -> Store {
        let (clock, alive) = (Arc::clone(&self.clock), Arc::clone(&self.alive));
        Store::opening_on(
            self.db.another(),
            Box::new(move || *clock.lock().unwrap()),
            Box::new(move |pid| alive.lock().unwrap().contains(&pid)),
            "another-host",
            &symphony_board::Opening::write(),
        )
        .expect("the store opens")
    }

    /// Another store on the same board whose clock, read between a rule's read and its write, first
    /// lets `meanwhile` act once: the board moves under a rule that has already decided.
    pub fn late(&self, meanwhile: impl Fn() + Send + Sync + 'static) -> Store {
        let (clock, alive, once) = (
            Arc::clone(&self.clock),
            Arc::clone(&self.alive),
            std::sync::Once::new(),
        );
        Store::opening_on(
            self.db.another(),
            Box::new(move || {
                once.call_once(&meanwhile);
                *clock.lock().unwrap()
            }),
            Box::new(move |pid| alive.lock().unwrap().contains(&pid)),
            HOST,
            &symphony_board::Opening::write(),
        )
        .expect("the store opens")
    }

    /// Another store on the same board whose clock, the first time it is read, gives the time as it
    /// was and only then lets `meanwhile` act once: a write that read the clock, then lost the race to
    /// one that read it later.
    pub fn slow(&self, meanwhile: impl Fn() + Send + Sync + 'static) -> Store {
        let (clock, alive, once) = (
            Arc::clone(&self.clock),
            Arc::clone(&self.alive),
            std::sync::Once::new(),
        );
        Store::opening_on(
            self.db.another(),
            Box::new(move || {
                let read = *clock.lock().unwrap();
                once.call_once(&meanwhile);
                read
            }),
            Box::new(move |pid| alive.lock().unwrap().contains(&pid)),
            HOST,
            &symphony_board::Opening::write(),
        )
        .expect("the store opens")
    }

    /// Another store on the same board whose clock lets `meanwhile` act before every read of it: a
    /// board that moves under each of its rules' reads for as long as `meanwhile` acts.
    pub fn busy(&self, meanwhile: impl Fn() + Send + Sync + 'static) -> Store {
        let (clock, alive) = (Arc::clone(&self.clock), Arc::clone(&self.alive));
        Store::opening_on(
            self.db.another(),
            Box::new(move || {
                meanwhile();
                *clock.lock().unwrap()
            }),
            Box::new(move |pid| alive.lock().unwrap().contains(&pid)),
            HOST,
            &symphony_board::Opening::write(),
        )
        .expect("the store opens")
    }

    /// A client of its own on this board's database.
    #[allow(
        dead_code,
        reason = "each test binary including this file runs on the databases its tests name"
    )]
    pub fn other_database(&self) -> Database {
        self.db.another()
    }

    /// A PostgreSQL client of its own on this board's database: for a test's statement of its
    /// own, past the store, where no command and no [`Store::raw`] row does what it needs.
    #[allow(
        dead_code,
        reason = "each test binary including this file runs on the databases its tests name"
    )]
    pub fn postgresql(&self) -> pg::Client {
        self.db.another().into_postgresql()
    }

    /// Counts each row of `table` written from now on (made, changed or removed) by a trigger of
    /// the test's own: how many so far.
    #[allow(
        dead_code,
        reason = "each test binary including this file reads the board as its tests need"
    )]
    pub fn writes_to(&self, table: &str) -> impl Fn() -> i64 + use<> {
        let client = self.postgresql();
        client
            .simple(&format!(
                "CREATE TABLE fixture_writes_{table} (n bigint NOT NULL); \
                 INSERT INTO fixture_writes_{table} VALUES (0); \
                 CREATE FUNCTION fixture_count_{table}() RETURNS trigger LANGUAGE plpgsql AS $$ \
                 BEGIN UPDATE fixture_writes_{table} SET n = n + 1; RETURN NULL; END $$; \
                 CREATE TRIGGER fixture_count AFTER INSERT OR UPDATE OR DELETE ON {table} \
                 FOR EACH ROW EXECUTE FUNCTION fixture_count_{table}();"
            ))
            .unwrap();
        let count = format!("SELECT n::text FROM fixture_writes_{table}");
        move || {
            let answer = client.request(&[pg::Step::bare(&count)]).unwrap();
            answer[0].value().unwrap().parse().unwrap()
        }
    }

    pub fn advance(&self, seconds: f64) {
        *self.clock.lock().unwrap() += seconds;
    }

    pub fn now(&self) -> f64 {
        *self.clock.lock().unwrap()
    }

    pub fn alive(&self, pids: &[i64]) {
        *self.alive.lock().unwrap() = pids.iter().copied().collect();
    }

    /// Sets the board's clock to `hours` after the UTC midnight before [`NOW`].
    pub fn at_hour(&self, hours: f64) {
        let midnight = symphony_board::reports::day_start(NOW);
        self.advance(midnight + hours * symphony_board::reports::HOUR - self.now());
    }
}

/// The scratch board the day files' tests read (`tests/store/reports.rs`, the server's roll-up),
/// written through the store's commands from the day before the UTC midnight before [`NOW`] to 09:30
/// after it, where its clock is left: tasks across statuses, `sim/b` bounced twice by two `changes`
/// verdicts, the red `tests/main-red-1` filed and closed, `sim/a` approved and landed, `sim/old`
/// open since the day before, `sim/gone` closed then, and `sim/b`'s review subtask; the sessions
/// that work it registered the day before but one, an opus worker at 01:00.
pub fn report_board() -> Fixture {
    let f = Fixture::new(Seed::default());
    f.at_hour(-24.0);
    let orch = f.session("orchestrator");
    let exe = f.session("worker");
    let rev = f.session("reviewer");
    let filer = f.session("filer");
    let made = |slug: &str| f.task_with(NewTask::new(slug, "Work", &orch));
    made("sim/old");
    made("sim/gone");
    f.store.claim("sim/gone", &exe).unwrap();
    f.store.move_to("sim/gone", "cancelled", "owner").unwrap();
    f.at_hour(1.0);
    let late = f.session_with(NewSession {
        model: "opus".into(),
        ..NewSession::new("worker")
    });
    for slug in ["sim/a", "sim/b"] {
        made(slug);
        f.store.take_role(slug, &orch, "orchestrator").unwrap();
    }
    f.task_with(NewTask {
        red: Some("main".into()),
        ..NewTask::new("tests/main-red-1", "Main is red", &filer)
    });
    f.task_with(NewTask {
        parent: Some("sim/b".into()),
        instruction: true,
        ..NewTask::new("sim/b/review", "Review it", &orch)
    });
    f.at_hour(2.0);
    f.store.claim("sim/a", &exe).unwrap();
    f.store.claim("sim/b", &exe).unwrap();
    f.store.claim("tests/main-red-1", &late).unwrap();
    f.at_hour(3.0);
    f.store.move_to("sim/a", "review", &exe).unwrap();
    f.store.move_to("sim/b", "review", &exe).unwrap();
    f.store
        .move_to("tests/main-red-1", "review", &late)
        .unwrap();
    f.at_hour(3.5);
    f.store.take_role("sim/a", &rev, "reviewer").unwrap();
    f.store
        .take_role("tests/main-red-1", &rev, "reviewer")
        .unwrap();
    f.store.claim("sim/b/review", &rev).unwrap();
    f.at_hour(4.0);
    f.store.verdict("sim/a", &rev, "approve", "fine").unwrap();
    f.store
        .verdict("tests/main-red-1", &rev, "approve", "green again")
        .unwrap();
    f.store.take_role("sim/b", &rev, "reviewer").unwrap();
    f.store
        .verdict("sim/b", &rev, "changes", "a test is missing")
        .unwrap();
    f.at_hour(4.5);
    f.store
        .landed(
            "sim/a",
            &orch,
            "landed sim/a on main at 3e0c661",
            false,
            &["done"],
        )
        .unwrap();
    f.at_hour(5.0);
    f.store.say("sim/b", &exe, "on it", None).unwrap();
    f.at_hour(6.0);
    f.store.move_to("sim/b", "review", &exe).unwrap();
    f.at_hour(7.0);
    f.store
        .verdict("sim/b", &rev, "changes", "one more")
        .unwrap();
    f.at_hour(8.0);
    f.store.move_to("sim/b", "review", &exe).unwrap();
    f.at_hour(9.5);
    f
}

fn open(db: &Database, clock: &Arc<Mutex<f64>>, alive: &Arc<Mutex<HashSet<i64>>>) -> Store {
    let (clock, alive) = (Arc::clone(clock), Arc::clone(alive));
    Store::opening_on(
        db.another(),
        Box::new(move || *clock.lock().unwrap()),
        Box::new(move |pid| alive.lock().unwrap().contains(&pid)),
        HOST,
        &symphony_board::Opening::write(),
    )
    .expect("the store opens")
}

/// Two sessions and two tasks, and a store whose rule sees amber-heron come back between its read and
/// its write, by acting on the other task (acting is being seen).
pub fn amber_back_meanwhile() -> (Fixture, Store) {
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .session("brisk-finch")
            .task("sim/jump", "ready")
            .task("sim/other", "ready"),
    );
    let back = f.other();
    let late = f.late(move || {
        back.claim("sim/other", "amber-heron").unwrap();
    });
    (f, late)
}

/// What `work` gives, run on a thread of its own and waited on as a wait on its condition
/// (`test_time::until_true`): work that never ends fails the test as a stall naming `what`, and
/// never hangs it. `allowance` is what the work takes on a quiet machine, with room to spare.
#[allow(
    dead_code,
    reason = "each test binary including this file bounds the work its tests name"
)]
pub fn bounded<T: Send + 'static>(
    what: &str,
    allowance: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> T {
    let running = std::thread::spawn(work);
    test_time::until_true(what, test_time::backstop(allowance), || {
        running.is_finished()
    })
    .unwrap_or_else(|stall| panic!("{stall}"));
    running
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// A new task `slug`, a subtask of `parent`.
pub fn under(slug: &str, parent: &str) -> NewTask {
    NewTask {
        parent: Some(parent.into()),
        ..NewTask::new(slug, "Jump departure", "owner")
    }
}

/// An orchestrator on `a`; `a/b` under it, `a/b/c` under that.
pub fn chain() -> (Fixture, String) {
    let f = Fixture::new(Seed::default());
    let orch = f.session("orchestrator");
    f.task("a");
    f.store.take_role("a", &orch, "orchestrator").unwrap();
    f.task_with(under("a/b", "a"));
    f.task_with(under("a/b/c", "a/b"));
    (f, orch)
}

/// `amber-heron` and `brisk-finch`, and `sim/jump` at `status`.
pub fn two_and_a_task(status: &str) -> Fixture {
    Fixture::new(
        Seed::default()
            .session("amber-heron")
            .session("brisk-finch")
            .task("sim/jump", status),
    )
}

/// `sim/jump` in review, everyone seen at `NOW`: `ochre-owl` orchestrates, `amber-heron` executed
/// it, `brisk-finch` reviews it.
pub fn in_review() -> Fixture {
    Fixture::new(
        Seed::default()
            .session("ochre-owl")
            .session("amber-heron")
            .session("brisk-finch")
            .task("sim/jump", "review")
            .role("sim/jump", "ochre-owl", "orchestrator")
            .role("sim/jump", "amber-heron", "executor")
            .role("sim/jump", "brisk-finch", "reviewer"),
    )
}

/// The refusal's words, or a panic naming what came back instead.
pub fn refused<T: std::fmt::Debug>(result: Result<T, symphony_board::Error>) -> String {
    match result {
        Err(symphony_board::Error::Refused(message)) => message,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// The workflow pair `phase/state`.
pub fn pair(phase: &str, state: &str) -> symphony_api::workflow::Pair {
    symphony_api::workflow::Pair {
        phase: phase.into(),
        state: state.into(),
    }
}

/// An hc with no terminal, used by presence sweeps and closes of historical panes.
pub struct NoTerminals;
impl symphony_board::launch::hc::Hc for NoTerminals {
    fn run(&self, _args: &[&str]) -> Result<String, symphony_board::launch::LaunchError> {
        Ok(r#"{"terminals":[]}"#.into())
    }
    fn feed(
        &self,
        _args: &[&str],
        _input: &str,
    ) -> Result<String, symphony_board::launch::LaunchError> {
        panic!("no terminal needs the core's documents")
    }
    fn app(&self, _args: &[&str]) -> Result<String, symphony_board::launch::LaunchError> {
        panic!("no terminal needs the window")
    }
    fn caller(&self) -> Option<String> {
        None
    }
    fn board(&self) -> String {
        "lamp-works/tasks".into()
    }
}

/// The public presence sweep `store` runs over an hc that lists no terminal, keeping its round in
/// `state`: it looks up no terminal or process, and refreshes and prunes the board's callbacks as
/// ever.
pub fn sweep_presence(store: &Store, state: PathBuf) {
    use symphony_board::launch::reap::{self, REAP_SECONDS, Reap};

    let sweep = Reap {
        state,
        apply: true,
        processes: &|_| panic!("no terminal needs a process inventory"),
        caller: None,
        min_idle: REAP_SECONDS,
    };
    let swept = reap::sweep(store, &NoTerminals, &sweep, &mut Vec::new());
    assert!(swept.unwrap().is_some());
    std::fs::remove_file(&sweep.state).unwrap();
    std::fs::remove_file(reap::lock_path(&sweep.state)).unwrap();
}

/// A custom workflow small enough for any test that needs a task one holds: claimed at
/// `build/available` into `build/working`, which its executor staffs and whose lost holder the
/// board recovers back to `build/available`; submitted to `verify/awaiting`; accepted by the owner
/// at `verify/accepted`, terminal.
pub const HELD_FLOW: &str = r#"
[workflow]
format = 1
initial = { phase = "build", state = "available" }
task_kinds = ["task"]
phases = [
  { id = "build", label = "Build", order = 0, phone_order = 0, column = true },
  { id = "verify", label = "Verify", order = 1, phone_order = 1, column = true },
]
states = [
  { id = "available", label = "Available" }, { id = "working", label = "Working" },
  { id = "awaiting", label = "Awaiting" }, { id = "accepted", label = "Accepted" },
]
capabilities = ["owner"]
adapters = []
guards = []
positions = [
  { at = { phase = "build", state = "available" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = true, terminal = false, legacy = "ready", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "build", state = "working" }, staffing = "executor", slot = "acquire", dependencies = "none", unsuccessful_dependency = "block", accepts_work = true, terminal = false, legacy = "in_progress", satisfies_dependencies = false, completion = "none", recovery = { release = { target = { phase = "build", state = "available" }, manual = false, slot = "release", release_role = true, release_reservation = false }, lost_holder = { target = { phase = "build", state = "available" }, manual = false, slot = "release", release_role = true, release_reservation = false }, attempt_failure = { manual = true, slot = "retain", release_role = false, release_reservation = false }, cancellation_acknowledged = { manual = true, slot = "retain", release_role = false, release_reservation = false } } },
  { at = { phase = "verify", state = "awaiting" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = false, legacy = "review", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "verify", state = "accepted" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = true, legacy = "done", outcome = "success", satisfies_dependencies = true, completion = "covered_close" },
]
edges = [
  { id = "claim", label = "Claim", from = { phase = "build", state = "available" }, to = { phase = "build", state = "working" }, operation = "claim", authority = [{ kind = "task_role", role = "executor" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "acquire_role", role = "executor" }, { kind = "slot", policy = "acquire" }], automatic = false, human_wait = false },
  { id = "submit", label = "Submit", from = { phase = "build", state = "working" }, to = { phase = "verify", state = "awaiting" }, operation = "request_review", authority = [{ kind = "task_role", role = "executor" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "slot", policy = "release" }], automatic = false, human_wait = false },
  { id = "accept", label = "Accept", from = { phase = "verify", state = "awaiting" }, to = { phase = "verify", state = "accepted" }, operation = "close_covered", authority = [{ kind = "project_capability", capability = "owner" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "outcome", outcome = "success", satisfies_dependencies = true }, { kind = "slot", policy = "release" }], automatic = false, human_wait = true },
]
"#;

/// [`HELD_FLOW`] as a commit's project file sets it, from fixed commits.
pub fn held_flow() -> symphony_board::workflow::Custom {
    let definition = (symphony_board::workflow::written(HELD_FLOW).unwrap().0).unwrap();
    let revision = definition
        .revision(symphony_api::workflow::Source {
            commit: "1".repeat(40),
            project_blob: "2".repeat(40),
        })
        .unwrap();
    symphony_board::workflow::Custom {
        definition,
        revision,
        mappings: Vec::new(),
        authorities: Vec::new(),
        project: None,
    }
}

/// `edge` taken on `slug` by the agent session `handle`, at the revision it reads now.
pub fn take_edge(store: &Store, slug: &str, handle: &str, edge: &str) {
    use symphony_board::workflow::{Host, agent_session};
    let (principal, host) = (agent_session(handle), Host::local());
    let next = store.workflow_actions(slug, &principal, &host).unwrap();
    let request = symphony_api::workflow::OperationRequest {
        project: store.database().database().to_string(),
        task: slug.into(),
        edge: edge.into(),
        input_revisions: Default::default(),
        idempotency_key: format!("{slug}@{}:{edge}", next.snapshot.task_revision),
        inputs: Default::default(),
        expected: next.snapshot,
    };
    store.apply_operation(&principal, &host, &request).unwrap();
}
