//! What the store's tests share: one SurrealDB server for the whole test binary, a database of its own
//! per test, a settable clock and process table (`tasks_store_test.py`'s `Clock` and `alive`), and a
//! board seeded through the one-way copy ([`Seed`], `seed.rs`).
//!
//! The server is the pinned `surreal` (`SYMPHONY_SURREAL`, else `tools/surreal.sh` fetches it), in
//! memory on a free local port, its outbound connections allowed to this machine alone
//! (`--allow-net=127.0.0.1`, after the address, which a stand-in reads as its fourth word), for the
//! gates a test's statement waits at (`gate.rs`). The target is joined to the flag: `--allow-net`
//! takes every word after it up to the next flag, so an engine after a target set apart would be a
//! second target, and the server would run on its default `memory` whatever engine it was handed
//! ([`store_held_in`] refuses such a start). It runs under a shell that stops it when this
//! process's pipe to the shell closes,
//! so it goes with the test binary however that ends, and that ends as soon as the server does, so a
//! server whose port was taken before it bound it is started again at once on another
//! (`symphony_board::port`), every start within one backstop only a stall reaches
//! (`test_time::backstop`); one that ends for any other reason
//! fails the tests with its exit status and its log, which also holds what the shell said. The
//! fixture makes the log's folder itself ([`launch`]). The shell says the server's pid, so an
//! answer on the port counts only once that process itself listens there. At a normal exit the binary also waits for the
//! shell, so the server's CPU is counted in the rusage of whoever ran the tests, as a child's,
//! rather than lost with an orphan's, and removes the server's log.

use std::collections::HashSet;
use std::convert::Infallible;
use std::io::{BufRead, BufReader, Read};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use symphony_board::db::DbError;
use symphony_board::health::{LIVE_SECONDS, STALE_SECONDS};
use symphony_board::pg;
use symphony_board::port::{NotStarted, Start};
use symphony_board::rpc::{self, Client};
use symphony_board::server::{Home, Postgres, Storage};
use symphony_board::sys::{Child, Command};
use symphony_board::{Database, Health, NewSession, NewTask, Store, Task};

#[path = "fixture_root.rs"]
mod fixture_root;
#[path = "seed.rs"]
mod seed;
#[path = "server_child.rs"]
mod server_child;
#[allow(
    unused_imports,
    reason = "each test binary including this file uses some of the seed's builders"
)]
pub use seed::{Seed, session, task};
pub use server_child::ServerChild;

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

/// Raw fixture writes can meet SurrealDB's deferred HNSW compaction even after a store call
/// commits. Retry conflicts, with capped, jittered backoff, until the backstop of the store's 30 s
/// patience: a loaded machine slows the compaction, never ends it.
/// Multi-statement callers must be idempotent or supply their own transaction.
pub trait RawWrite {
    fn write(&self, sql: &str, vars: serde_json::Value) -> Result<Vec<serde_json::Value>, DbError>;
}

impl RawWrite for Client {
    fn write(&self, sql: &str, vars: serde_json::Value) -> Result<Vec<serde_json::Value>, DbError> {
        let until = Instant::now() + test_time::backstop(Duration::from_secs(30));
        retry_conflicts(until, || self.query(sql, vars.clone()))
    }
}

fn retry_conflicts<T>(
    until: Instant,
    mut once: impl FnMut() -> Result<T, DbError>,
) -> Result<T, DbError> {
    use std::hash::{BuildHasher, RandomState};

    let mut attempt = 0;
    loop {
        match once() {
            Err(DbError::Conflict) if Instant::now() < until => {
                let ceiling = 1000u64 << attempt.min(6);
                #[expect(
                    clippy::disallowed_methods,
                    reason = "no-timed-waits baseline: a test's backoff of its own, where the board store's is the helper"
                )]
                std::thread::sleep(Duration::from_micros(
                    RandomState::new().hash_one(attempt) % ceiling,
                ));
                attempt += 1;
            }
            result => return result,
        }
    }
}

#[test]
fn raw_writes_retry_conflicts_and_return_other_results_without_repeating_them() {
    let mut attempts = 0;
    let result = retry_conflicts(Instant::now() + Duration::from_secs(30), || {
        attempts += 1;
        if attempts < 3 {
            Err(DbError::Conflict)
        } else {
            Ok(17)
        }
    });
    assert_eq!(result, Ok(17));
    assert_eq!(attempts, 3);

    let future = Instant::now() + Duration::from_secs(30);
    for (error, until) in [
        (DbError::Failed("invalid vector".into()), future),
        (DbError::Thrown("refused".into()), future),
        (DbError::Conflict, Instant::now()),
    ] {
        let mut attempts = 0;
        let result = retry_conflicts::<()>(until, || {
            attempts += 1;
            assert_eq!(attempts, 1, "terminal errors must not be retried");
            Err(error.clone())
        });
        assert_eq!(result, Err(error));
        assert_eq!(attempts, 1);
    }
}

struct Server {
    addr: String,
    shell: Mutex<ServerChild>,
    log: PathBuf,
}

static SERVER: OnceLock<Server> = OnceLock::new();
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

/// At the test binary's exit: the server ends, the shell it ran under is waited for, and its log goes
/// (a server that never came up is reported, log and all, by `start`).
extern "C" fn stop_server() {
    if let Some(server) = SERVER.get() {
        let mut shell = server.shell.lock().unwrap_or_else(|held| held.into_inner());
        let _ = shell.shutdown();
        let _ = std::fs::remove_file(&server.log);
    }
}

/// The pinned `surreal`: `SYMPHONY_SURREAL`, else the one `tools/surreal.sh` fetches (once per
/// machine).
pub fn surreal() -> PathBuf {
    if let Some(named) = symphony_env::SURREAL.os() {
        return named.into();
    }
    let script = project_root().join("tools/surreal.sh");
    let found = symphony_board::sys::output(symphony_board::sys::command("bash").arg(&script))
        .expect("bash runs tools/surreal.sh");
    assert!(
        found.status.success(),
        "tools/surreal.sh found no SurrealDB server for the tests: {}",
        String::from_utf8_lossy(&found.stderr)
    );
    PathBuf::from(String::from_utf8(found.stdout).unwrap().trim())
}

/// The workspace of the native caller in this layout, with its owned fixture scripts.
pub fn project_root() -> PathBuf {
    fixture_root::workspace(Path::new(env!("CARGO_MANIFEST_DIR")))
}

/// `command` started with its stdin a [`symphony_board::sys::pipe`], kept as `child.stdin`, and its other
/// streams as `command` sets them: what the child reads ends once the test drops that input, never
/// later because a child another test spawned took a copy of it.
pub fn spawn_with_input(command: &mut Command) -> std::io::Result<Child> {
    let (input, stdin) = symphony_board::sys::pipe()?;
    let mut child = command.stdin(input).spawn()?;
    child.stdin = Some(OwnedFd::from(stdin).into());
    Ok(child)
}

fn start() -> Server {
    let log = option_env!("CARGO_TARGET_TMPDIR")
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join(format!("surreal-{}.log", std::process::id()));
    let (addr, shell) = launch(&surreal(), &log).unwrap_or_else(|why| {
        panic!(
            "the SurrealDB server did not come up: {why}; its log is {}",
            log.display()
        )
    });
    on_exit(stop_server);
    Server {
        addr,
        shell: Mutex::new(shell),
        log,
    }
}

/// `binary` started as the tests' server, in memory on a free local port: its address and the shell
/// it runs under, which stops it once the shell's input closes. The shell's own output and the
/// server's go to `log`, made here with its folder: Cargo makes `CARGO_TARGET_TMPDIR` only while it
/// compiles a test, so a test binary built earlier can find it gone. A server that ends before it
/// answers is reported with its exit status and its log.
pub fn launch(binary: &Path, log: &Path) -> Result<(String, ServerChild), NotStarted<Infallible>> {
    launch_on(binary, log, "memory")
}

/// [`launch`], the server keeping its data in `engine` (`memory`, or `surrealkv://FOLDER`, the
/// engine the board runs on): a server up that holds no store where `engine` names fails the test,
/// with its log ([`store_held_in`]).
pub fn launch_on(
    binary: &Path,
    log: &Path,
    engine: &str,
) -> Result<(String, ServerChild), NotStarted<Infallible>> {
    launch_under(binary, log, engine, None)
}

/// [`launch_on`], the server keeping its data as `storage` says where one is given: without one,
/// SurrealKV's memtable is its own, a share of the machine's memory, so what one transaction may
/// hold differs from machine to machine.
pub fn launch_under(
    binary: &Path,
    log: &Path,
    engine: &str,
    storage: Option<Storage>,
) -> Result<(String, ServerChild), NotStarted<Infallible>> {
    if let Some(folder) = log.parent() {
        std::fs::create_dir_all(folder).unwrap_or_else(|error| {
            panic!(
                "the SurrealDB server's log folder {} cannot be made: {error}",
                folder.display()
            )
        });
    }
    let said = || {
        let mut said = String::new();
        let _ = std::fs::File::open(log).and_then(|mut f| f.read_to_string(&mut said));
        said
    };
    // Up once it answers and the server itself listens there (the shell says its pid): a process
    // that took its port answers too. A quiet machine has it up within `READY`. Every start, a port
    // taken tried again on another included, waits within the one backstop, which only a stall
    // reaches: each attempt has what is left of it (RULES.md `tests/no-test-fails-on-load`).
    const READY: Duration = Duration::from_secs(20);
    let until = Instant::now() + test_time::backstop(READY);
    let started = symphony_board::port::start_on_free_port(
        symphony_board::port::ATTEMPTS,
        until,
        |reserved| {
            let addr = reserved.to_string();
            let output = std::fs::File::create(log).unwrap_or_else(|error| {
                panic!(
                    "the SurrealDB server's log {} cannot be written: {error}",
                    log.display()
                )
            });
            let (announced, announcer) =
                symphony_board::sys::pipe().expect("a pipe for the server's pid");
            let shell = spawn_with_input(
            symphony_board::sys::command("bash")
                .arg("-c")
                .arg(concat!(
                    r#". "$2"; fixture_close_fds; exec 3<&0; "$0" start --unauthenticated --bind "$1" --log error --allow-net=127.0.0.1 "$3" >&2 3<&- & s=$!; "#,
                    r#"echo "$s"; exec >/dev/null; "#,
                    r#"(fixture_wait_eof <&3; kill $s 2>/dev/null) & c=$!; exec 3<&-; wait $s; r=$?; kill $c 2>/dev/null; wait $c 2>/dev/null; exit $r"#,
                ))
                .arg(binary)
                .arg(&addr)
                .arg(project_root().join("tools/fixture_process.sh"))
                .arg(engine)
                .envs(storage.into_iter().flat_map(Storage::environment))
                .stdout(announcer)
                .stderr(output),
        )
        .expect("sh starts the server");
            // Own it before reading the announcement or checking readiness: either can unwind.
            let mut shell = ServerChild {
                shell,
                server: None,
            };
            let mut line = String::new();
            let _ = BufReader::new(announced).read_line(&mut line);
            shell.server = line.trim().parse().ok();
            let server = shell.server;
            let up = test_time::until(
                format!("the SurrealDB server answering on {addr}"),
                until.saturating_duration_since(Instant::now()),
                || match shell.shell.try_wait().unwrap() {
                    Some(status) => Some(Err(status)),
                    None => (rpc::healthy(&addr)
                        && server.is_some_and(|pid| {
                            symphony_board::port::listens(
                                pid,
                                reserved,
                                symphony_board::sys::output,
                            )
                            .unwrap()
                        }))
                    .then_some(Ok(())),
                },
            );
            match up {
                Ok(Ok(())) => Ok::<_, Infallible>(Start::Up((addr, shell))),
                Ok(Err(status)) => Ok(Start::Ended(format!("{status}; it said: {}", said()))),
                Err(stall) => {
                    let _ = shell.shutdown();
                    panic!("{stall}; its log ({}):\n{}", log.display(), said());
                }
            }
        },
    );
    let (addr, shell) = started.map(|(up, _)| up)?;
    if let Err(why) = store_held_in(engine) {
        // Unwinding drops `shell`, which ends the server.
        panic!(
            "the SurrealDB server on {addr}: {why}; its log ({}):\n{}",
            log.display(),
            said()
        );
    }
    Ok((addr, shell))
}

/// Whether a server holds the store `engine` names: for `surrealkv://FOLDER`, the lock SurrealKV
/// takes in FOLDER when it opens the store there, before the server answers. Any other engine
/// names no folder, and holds. One that does not is a server on another engine, its default
/// `memory` say: where it keeps nothing a restart finds, and copies a board many times slower.
pub fn store_held_in(engine: &str) -> Result<(), String> {
    let Some(folder) = engine.strip_prefix("surrealkv://") else {
        return Ok(());
    };
    let lock = Path::new(folder).join("LOCK");
    let elsewhere = format!("so it runs on an engine other than {engine}");
    let file = std::fs::File::open(&lock).map_err(|error| {
        format!(
            "it holds no store in {folder}: its lock {} cannot be opened ({error}), {elsewhere}",
            lock.display()
        )
    })?;
    match file.try_lock() {
        Err(std::fs::TryLockError::WouldBlock) => Ok(()),
        Ok(()) => Err(format!(
            "it holds no store in {folder}: nothing holds its lock {}, {elsewhere}",
            lock.display()
        )),
        Err(std::fs::TryLockError::Error(error)) => Err(format!(
            "its store's lock {} cannot be tried: {error}",
            lock.display()
        )),
    }
}

/// A database of its own on the shared server.
pub fn client() -> Client {
    let server = SERVER.get_or_init(start);
    let db = format!(
        "t{}_{}",
        std::process::id(),
        DATABASES.fetch_add(1, Ordering::SeqCst)
    );
    Client::new(&server.addr, "sf", &db)
}

pub fn server_addr() -> String {
    SERVER.get_or_init(start).addr.clone()
}

/// The database a store test's board is in. A test that runs on both is written once, by
/// [`crate::on_both`], and named by each: `NAME::surrealdb` and `NAME::postgresql`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "each test binary including this file runs on the databases its tests name"
)]
pub enum Backend {
    SurrealDb,
    PostgreSql,
}

#[allow(
    dead_code,
    reason = "each test binary including this file runs on the databases its tests name"
)]
impl Backend {
    /// A database of its own on this binary's server of the backend.
    pub fn database(self) -> Database {
        match self {
            Backend::SurrealDb => client().into(),
            Backend::PostgreSql => postgresql().into(),
        }
    }
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

fn start_postgresql() -> PgServer {
    let dir = std::env::temp_dir().join(format!("board-pg-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    let programs = symphony_board::server::postgres::discover_programs(&project_root()).unwrap();
    let pg = Home::new(&dir.join("board"), Path::new("/no/surreal"))
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

    /// Every row of each of the board's tables, as [`Store::raw`] reads them past every rule, on
    /// either database: what a write refused leaves as it was.
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

    /// A board on SurrealDB seeded with `seed`.
    pub fn new(seed: Seed) -> Fixture {
        Fixture::on(Backend::SurrealDb, seed)
    }

    /// A board on `backend` seeded with `seed`.
    pub fn on(backend: Backend, seed: Seed) -> Fixture {
        let db = backend.database();
        let clock = Arc::new(Mutex::new(NOW));
        let alive = Arc::new(Mutex::new(HashSet::new()));
        let store = open(&db, &clock, &alive);
        store.copy(&seed.dump).expect("the seed copies");
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

    /// A SurrealDB client of its own on this board's database.
    pub fn other_client(&self) -> Client {
        match self.db.another() {
            Database::SurrealDb(client) => client,
            Database::PostgreSql(_) => panic!("this board is on PostgreSQL"),
        }
    }

    /// A client of its own on this board's database, on either database.
    #[allow(
        dead_code,
        reason = "each test binary including this file runs on the databases its tests name"
    )]
    pub fn other_database(&self) -> Database {
        self.db.another()
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

/// The default report fixture, shared with the server's roll-up tests.
pub fn report_board() -> Fixture {
    report_board_on(Backend::SurrealDb)
}

/// The scratch board the day files' tests read (`tests/store/reports.rs`, the server's roll-up),
/// written through the store's commands from the day before the UTC midnight before [`NOW`] to 09:30
/// after it, where its clock is left: tasks across statuses, `sim/b` bounced twice by two `changes`
/// verdicts, the red `tests/main-red-1` filed and closed, `sim/a` approved and landed, `sim/old`
/// open since the day before, `sim/gone` closed then, and `sim/b`'s review subtask; the sessions
/// that work it registered the day before but one, an opus worker at 01:00.
pub fn report_board_on(backend: Backend) -> Fixture {
    let f = Fixture::on(backend, Seed::default());
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
pub fn amber_back_meanwhile(backend: Backend) -> (Fixture, Store) {
    let f = Fixture::on(
        backend,
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

/// The refusal's words, or a panic naming what came back instead.
pub fn refused<T: std::fmt::Debug>(result: Result<T, symphony_board::Error>) -> String {
    match result {
        Err(symphony_board::Error::Refused(message)) => message,
        other => panic!("expected a refusal, got {other:?}"),
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
