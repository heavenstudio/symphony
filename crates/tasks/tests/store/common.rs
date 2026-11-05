//! What the store's tests share: one SurrealDB server for the whole test binary, a database of its own
//! per test, a settable clock and process table (`tasks_store_test.py`'s `Clock` and `alive`), and a
//! board seeded through the one-way copy.
//!
//! The server is the pinned `surreal` (`SF_SURREAL`, else `tools/surreal.sh` fetches it), in memory on a
//! free local port. It runs under a shell that stops it when this process's pipe to the shell closes,
//! so it goes with the test binary however that ends, and that ends as soon as the server does, so a
//! server whose port was taken before it bound it is started again at once on another
//! (`symphony_board::port`), every start within one readiness bound; one that ends for any other reason
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
use symphony_board::health::{LIVE_SECONDS, STALE_SECONDS};
use symphony_board::port::{NotStarted, Start};
use symphony_board::rows::{Dump, LinkRow, RoleRow, SessionRow, TaskRow};
use symphony_board::rpc::{self, Client, DbError};
use symphony_board::sys::{Child, Command};
use symphony_board::{Health, NewSession, NewTask, Store, Task};

pub const NOW: f64 = 1_800_000_000.0;
/// Past the lost line.
pub const GONE: f64 = LIVE_SECONDS + STALE_SECONDS + 1.0;
pub const HOST: &str = "this-host";

/// Raw fixture writes can meet SurrealDB's deferred HNSW compaction even after a store call
/// commits. Retry conflicts with the store's 30 s patience and capped, jittered backoff.
/// Multi-statement callers must be idempotent or supply their own transaction.
pub trait RawWrite {
    fn write(&self, sql: &str, vars: serde_json::Value) -> Result<Vec<serde_json::Value>, DbError>;
}

impl RawWrite for Client {
    fn write(&self, sql: &str, vars: serde_json::Value) -> Result<Vec<serde_json::Value>, DbError> {
        retry_conflicts(Instant::now() + Duration::from_secs(30), || {
            self.query(sql, vars.clone())
        })
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
    shell: Mutex<Child>,
    log: PathBuf,
}

static SERVER: OnceLock<Server> = OnceLock::new();
static DATABASES: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" {
    fn atexit(at_exit: extern "C" fn()) -> std::ffi::c_int;
}

/// At the test binary's exit: the server ends, the shell it ran under is waited for, and its log goes
/// (a server that never came up is reported, log and all, by `start`).
extern "C" fn stop_server() {
    if let Some(server) = SERVER.get() {
        let mut shell = server.shell.lock().unwrap_or_else(|held| held.into_inner());
        drop(shell.stdin.take());
        let _ = shell.wait();
        let _ = std::fs::remove_file(&server.log);
    }
}

/// The pinned `surreal`: `SF_SURREAL`, else the one `tools/surreal.sh` fetches (once per machine).
pub fn surreal() -> PathBuf {
    if let Some(named) = std::env::var_os("SF_SURREAL") {
        return named.into();
    }
    let script = project_root().join("tools/surreal.sh");
    let found = symphony_board::sys::output(Command::new("bash").arg(&script))
        .expect("bash runs tools/surreal.sh");
    assert!(
        found.status.success(),
        "tools/surreal.sh found no SurrealDB server for the tests: {}",
        String::from_utf8_lossy(&found.stderr)
    );
    PathBuf::from(String::from_utf8(found.stdout).unwrap().trim())
}

fn project_root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|root| {
            root.join("tools/surreal.sh").is_file()
                && root.join("tools/fixture_process.sh").is_file()
        })
        .expect("the project supplies the owned server fixture scripts")
        .to_path_buf()
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
    assert_eq!(unsafe { atexit(stop_server) }, 0, "the server's end is set");
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
pub fn launch(binary: &Path, log: &Path) -> Result<(String, Child), NotStarted<Infallible>> {
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
    // Up once it answers and the server itself listens there (the shell says its pid), every start
    // within the one readiness bound: a process that took its port answers too.
    let until = Instant::now() + Duration::from_secs(20);
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
            let mut shell = spawn_with_input(
            Command::new("bash")
                .arg("-c")
                .arg(concat!(
                    r#". "$2"; fixture_close_fds; exec 3<&0; "$0" start --unauthenticated --bind "$1" --log error memory >&2 3<&- & s=$!; "#,
                    r#"echo "$s"; exec >/dev/null; "#,
                    r#"(fixture_wait_eof <&3; kill $s 2>/dev/null) & c=$!; exec 3<&-; wait $s; r=$?; kill $c 2>/dev/null; wait $c 2>/dev/null; exit $r"#,
                ))
                .arg(binary)
                .arg(&addr)
                .arg(project_root().join("tools/fixture_process.sh"))
                .stdout(announcer)
                .stderr(output),
        )
        .expect("sh starts the server");
            let mut line = String::new();
            let _ = BufReader::new(announced).read_line(&mut line);
            let server: Option<u32> = line.trim().parse().ok();
            while Instant::now() < until {
                if let Some(status) = shell.try_wait().unwrap() {
                    return Ok::<_, Infallible>(Start::Ended(format!(
                        "{status}; it said: {}",
                        said()
                    )));
                }
                if rpc::healthy(&addr)
                    && server.is_some_and(|pid| {
                        symphony_board::port::listens(pid, reserved, symphony_board::sys::output)
                            .unwrap()
                    })
                {
                    return Ok(Start::Up((addr, shell)));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            drop(shell.stdin.take()); // the shell stops the server, if it is still there, and ends
            let _ = shell.wait();
            panic!(
                "the SurrealDB server did not answer on {addr} within 20 s; its log ({}):\n{}",
                log.display(),
                said()
            );
        },
    );
    started.map(|(up, _)| up)
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

/// A board of its own, its clock at `NOW` and no process alive until a test says so.
pub struct Fixture {
    pub store: Store,
    db: Client,
    clock: Arc<Mutex<f64>>,
    alive: Arc<Mutex<HashSet<i64>>>,
}

impl Fixture {
    /// A board with nothing on it.
    pub fn empty() -> Fixture {
        Fixture::new(Seed::default())
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

    pub fn new(seed: Seed) -> Fixture {
        let db = client();
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
        Store::with(
            self.other_client(),
            Box::new(move || *clock.lock().unwrap()),
            Box::new(move |pid| alive.lock().unwrap().contains(&pid)),
            "another-host",
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
        Store::with(
            self.other_client(),
            Box::new(move || {
                once.call_once(&meanwhile);
                *clock.lock().unwrap()
            }),
            Box::new(move |pid| alive.lock().unwrap().contains(&pid)),
            HOST,
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
        Store::with(
            self.other_client(),
            Box::new(move || {
                let read = *clock.lock().unwrap();
                once.call_once(&meanwhile);
                read
            }),
            Box::new(move |pid| alive.lock().unwrap().contains(&pid)),
            HOST,
        )
        .expect("the store opens")
    }

    /// Another store on the same board whose clock lets `meanwhile` act before every read of it: a
    /// board that moves under each of its rules' reads for as long as `meanwhile` acts.
    pub fn busy(&self, meanwhile: impl Fn() + Send + Sync + 'static) -> Store {
        let (clock, alive) = (Arc::clone(&self.clock), Arc::clone(&self.alive));
        Store::with(
            self.other_client(),
            Box::new(move || {
                meanwhile();
                *clock.lock().unwrap()
            }),
            Box::new(move |pid| alive.lock().unwrap().contains(&pid)),
            HOST,
        )
        .expect("the store opens")
    }

    /// A client of its own on this board's database.
    pub fn other_client(&self) -> Client {
        Client::new(self.db.addr(), "sf", self.db.database())
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
}

fn open(db: &Client, clock: &Arc<Mutex<f64>>, alive: &Arc<Mutex<HashSet<i64>>>) -> Store {
    let (clock, alive) = (Arc::clone(clock), Arc::clone(alive));
    Store::with(
        Client::new(db.addr(), "sf", db.database()),
        Box::new(move || *clock.lock().unwrap()),
        Box::new(move |pid| alive.lock().unwrap().contains(&pid)),
        HOST,
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

/// A board to seed, in the dump's shape: sessions seen at `NOW`, tasks in the order given.
#[derive(Default, Clone)]
pub struct Seed {
    pub dump: Dump,
}

impl Seed {
    pub fn session(self, handle: &str) -> Seed {
        self.session_row(session(handle))
    }

    pub fn session_row(mut self, row: SessionRow) -> Seed {
        self.dump.sessions.push(row);
        self
    }

    pub fn task(self, slug: &str, status: &str) -> Seed {
        let row = TaskRow {
            id: self.dump.tasks.len() as i64 + 1,
            ..task(slug, status)
        };
        self.task_row(row)
    }

    pub fn task_row(mut self, row: TaskRow) -> Seed {
        self.dump.tasks.push(row);
        self
    }

    pub fn role(mut self, slug: &str, handle: &str, role: &str) -> Seed {
        let since = NOW + self.dump.roles.len() as f64;
        self.dump.roles.push(RoleRow {
            slug: slug.into(),
            handle: handle.into(),
            role: role.into(),
            since,
            expires: None,
        });
        self
    }

    pub fn reserved(mut self, slug: &str, handle: &str, expires: Option<f64>) -> Seed {
        self.dump.roles.push(RoleRow {
            slug: slug.into(),
            handle: handle.into(),
            role: "reserved".into(),
            since: NOW,
            expires,
        });
        self
    }

    pub fn link(mut self, slug: &str, kind: &str, target: &str) -> Seed {
        self.dump.task_links.push(LinkRow {
            slug: slug.into(),
            kind: kind.into(),
            target: target.into(),
        });
        self
    }
}

pub fn session(handle: &str) -> SessionRow {
    SessionRow {
        handle: handle.into(),
        kind: "worker".into(),
        connected: NOW,
        last_seen: NOW,
        ..Default::default()
    }
}

pub fn task(slug: &str, status: &str) -> TaskRow {
    TaskRow {
        id: 1,
        slug: slug.into(),
        title: format!("Task {slug}"),
        status: status.into(),
        priority: 2,
        created_by: "owner".into(),
        created: NOW,
        updated: NOW,
        ..Default::default()
    }
}

/// The refusal's words, or a panic naming what came back instead.
pub fn refused<T: std::fmt::Debug>(result: Result<T, symphony_board::Error>) -> String {
    match result {
        Err(symphony_board::Error::Refused(message)) => message,
        other => panic!("expected a refusal, got {other:?}"),
    }
}
