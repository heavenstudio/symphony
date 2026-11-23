//! The suite's board: the generated rows ([`crate::generated`]) copied onto a PostgreSQL of this
//! test binary's own, in the run's folder, a socket for each recipient with a callback, and the
//! `tasks` binary run on it as an agent runs it.
//!
//! The board is made once for the test binary ([`board`]): its copy and the first look of every
//! recipient take most of the suite's time. Each test holds it while it runs, so a timing never
//! shares the server with another test's commands. Its server is a scratch board's, so it ends
//! with this process however that ends (`symphony_board::server::postgres`); at the binary's exit
//! the board is dropped and its server stopped first ([`stop_board`]), before the test runner
//! looks at this process's children.

use crate::children::Child as _;
use crate::cpu;
use crate::findings::{Logged, plans_of};
use crate::generated::{NOW, dump, ready, recipients, thread, writer};
use crate::listeners::Listeners;
use crate::proxy::{Proxy, Request};
use crate::store;
use serde_json::{Value, json};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
use symphony_board::lock::Recovered as _;
use symphony_board::pg;
use symphony_board::rows::{Dump, TaskRow};
use symphony_board::server::Home;
use symphony_board::server::postgres::{Postgres, discover_programs};
use symphony_board::sys::{self, Command};
use symphony_board::{Database, Opening, Raw, Store};

/// The database the board is in on the suite's server.
pub const DATABASE: &str = "perf";

/// The tables [`Store::copy`] fills, whose rows [`Board::copied`] counts.
const COPIED: [&str; 10] = [
    "task",
    "link",
    "session",
    "role",
    "message",
    "delivery",
    "barred_key",
    "task_session",
    "delivery_key",
    "ask",
];

/// The longest a wait for the board's server to go quiet ([`Board::bill`]) may take: past it the
/// test fails, saying what the server took, and every wait after it fails at once.
const SETTLE: Duration = Duration::from_secs(300);

/// How long a client of the suite's waits for each answer: the copy of the whole board, on a
/// loaded machine, is the longest.
const PATIENCE: Duration = Duration::from_secs(600);

/// What the plan runs set on the board's database for each session that starts meanwhile
/// ([`Board::explained`]), and reset after them: `auto_explain` loaded, giving the plan of every
/// statement as JSON, those a block or a trigger runs too, without the values bound to it, at the
/// level of a notice, which goes to the session's client and not to the server's log; and no
/// sequential scan where an index serves, so a scan left in a plan is one no index serves,
/// whatever its table's size ([`crate::findings`]).
const EXPLAINING: [(&str, &str); 7] = [
    ("session_preload_libraries", "'auto_explain'"),
    ("auto_explain.log_min_duration", "0"),
    ("auto_explain.log_nested_statements", "on"),
    ("auto_explain.log_format", "'json'"),
    ("auto_explain.log_parameter_max_length", "0"),
    ("auto_explain.log_level", "'notice'"),
    ("enable_seqscan", "off"),
];

/// What one run of the `tasks` binary said, how long it took, and the CPU it cost.
pub struct Ran {
    pub code: i32,
    pub out: String,
    pub err: String,
    pub wall: Duration,
    /// The user and system CPU the `tasks` process took, with every child it reaped.
    pub tasks_cpu: Duration,
    /// The user and system CPU the board's server took for the run, from quiet before its start
    /// to quiet again after its reap ([`Board::bill`]).
    pub server_cpu: Duration,
}

/// What the write whose dispatch gave every recipient its first look sent the server, and the
/// plans the server gave for it ([`Board::explained`]).
#[derive(Default)]
pub struct Looked {
    pub requests: Vec<Request>,
    pub logged: Logged,
}

/// The suite's board: its server, the recipients' sockets, and what the recipients' first looks
/// sent.
pub struct Board {
    /// Where a command reaches the board: `postgresql:` and its server's socket's folder.
    pub addr: String,
    /// The folder of its server's socket, which a client or a relay names.
    pub socket: PathBuf,
    /// Its server's postmaster, the first process of the tree the board's CPU is read in.
    pub pid: i64,
    /// The rows of each table the copy filled, read right after the copy, before any test's
    /// command writes.
    pub copied: Value,
    /// The waits for the board's server to go quiet, across which a run's cost counts its CPU.
    quiet: cpu::Quiet,
    /// The run's folder, the server's data and log in it.
    pub dir: PathBuf,
    pub listeners: Listeners,
    /// What the recipients' first looks sent, and how the server planned it.
    pub first_looks: Looked,
    /// The ready tasks not claimed yet.
    unclaimed: Vec<String>,
    /// The suite's PostgreSQL, stopped as the board drops.
    pub server: Postgres,
}

impl Drop for Board {
    /// Its server stopped and waited for, so it has ended before this process does.
    fn drop(&mut self) {
        if let Err(why) = self.server.stop() {
            eprintln!("perf: the board's PostgreSQL did not stop: {why}");
        }
    }
}

static BOARD: OnceLock<Mutex<Option<Board>>> = OnceLock::new();

pub struct Held(MutexGuard<'static, Option<Board>>);

impl Deref for Held {
    type Target = Board;

    fn deref(&self) -> &Board {
        self.0.as_ref().expect("a held board is initialized")
    }
}

impl DerefMut for Held {
    fn deref_mut(&mut self) -> &mut Board {
        self.0.as_mut().expect("a held board is initialized")
    }
}

extern "C" fn stop_board() {
    if let Some(board) = BOARD.get() {
        let mut board = board.lock().recover();
        // Statics do not drop at exit. Drop here, before the native runner inspects our children.
        drop(board.take());
    }
}

/// The suite's board, made on first use and held by the caller until it lets go.
pub fn board() -> Held {
    // A test that failed holding the board (an assertion) left it as its commands left it: the
    // server, its rows and the sockets stand, and each test reads only what it makes itself or
    // what every test leaves as it was, so the next test may take it.
    let mut held = BOARD
        .get_or_init(|| {
            store::on_exit(stop_board);
            Mutex::new(None)
        })
        .lock()
        .recover();
    held.get_or_insert_with(Board::make);
    Held(held)
}

/// Whether the folder named `name` in a temporary folder is an earlier run's whose process has
/// ended: `PREFIX` and the process's id, then anything after a `-`.
fn ended_run(name: &str, prefix: &str) -> bool {
    let pid = (name.strip_prefix(prefix))
        .and_then(|rest| rest.split('-').next())
        .and_then(|pid| pid.parse::<i64>().ok());
    pid.is_some_and(|pid| !sys::pid_alive(pid))
}

/// The folder of each earlier run of the suite in `temp` (named for its process) whose process
/// has ended, its server's socket's folder, and each relay's folder of such a run in `/tmp`. Its
/// server ended with that process, as a scratch board's does; its folders stay until a later run
/// removes them here: a run removes none of its own, as one stopped by a signal could not.
fn remove_ended_runs(temp: &Path) {
    for (folder, prefix) in [
        (temp, "symphony-perf-"),
        (Path::new("/tmp"), "symphony-perf-relay-"),
    ] {
        let Ok(entries) = std::fs::read_dir(folder) else {
            continue;
        };
        let ended = entries.flatten().filter(|entry| {
            let name = entry.file_name();
            name.to_str().is_some_and(|name| ended_run(name, prefix))
        });
        for entry in ended {
            let run = entry.path();
            let socket = Home::new(&run).postgresql(Path::new("")).socket();
            let _ = std::fs::remove_dir_all(socket);
            let _ = std::fs::remove_dir_all(run);
        }
    }
}

/// The `tasks` binary timed: `SYMPHONY_PERF_TASKS` when it names one (another build's, to time it on the
/// same board), else this crate's.
pub fn program() -> PathBuf {
    std::env::var_os("SYMPHONY_PERF_TASKS")
        .map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_tasks")), PathBuf::from)
}

/// A client of the board's database on the server whose socket is in `socket`.
fn client_of(socket: &Path) -> pg::Client {
    pg::Client::new(socket, DATABASE).waiting(PATIENCE)
}

/// The rows of each table the copy fills ([`COPIED`]) on the board in `db`.
fn copied(db: &Database) -> Value {
    let raw = Raw::of(db);
    let counted = (COPIED.iter()).map(|table| {
        let rows = (raw.table(table).count())
            .unwrap_or_else(|why| panic!("perf: the copied board's {table} rows: {why}"));
        (table.to_string(), json!(rows))
    });
    Value::Object(counted.collect())
}

impl Board {
    fn make() -> Board {
        // Only the private cleanup worker receives this record path. It exercises the same
        // static, server and exit callback with a few rows and no recipients' first looks.
        let cleanup_worker = std::env::var_os("SYMPHONY_PERF_CLEANUP_RECORD").is_some();
        // Short: the recipients' sockets live here, and a socket's path has room for 104 bytes.
        let temp = std::env::temp_dir();
        remove_ended_runs(&temp);
        let dir = temp.join(format!("symphony-perf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir); // an ended run's, whose process had this one's id
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ps"), "").unwrap();
        let programs = discover_programs(&store::project_root())
            .unwrap_or_else(|why| panic!("the suite's PostgreSQL: {why}"));
        let server = Home::new(&dir).telling(|_| {}).postgresql(&programs);
        let serving = (server.ensure())
            .unwrap_or_else(|why| panic!("the suite's PostgreSQL did not come up: {why}"));
        let root = u32::try_from(serving.pid).expect("a postmaster's process id");
        let quiet = cpu::Quiet::tree("the board's server", root, SETTLE);
        let db: Database = client_of(&serving.socket).into();
        let store = Store::opening_on(
            db.another(),
            Box::new(|| NOW),
            Box::new(|_| false),
            &sys::hostname(),
            &Opening::write(),
        )
        .unwrap_or_else(|why| panic!("perf: the board opens: {why}"));
        if cleanup_worker {
            // Cleanup checks no cost, and this copy can be too small for a coarse CPU meter.
            let rows = Dump {
                tasks: (0..3)
                    .map(|id| TaskRow {
                        id: id + 1,
                        slug: format!("cleanup/task-{id}"),
                        title: "A cleanup worker's task".into(),
                        status: "ready".into(),
                        created_by: "owner".into(),
                        ..TaskRow::default()
                    })
                    .collect(),
                ..Dump::default()
            };
            store.copy(&rows).unwrap();
        } else {
            // The copy, then its tables' statistics and visibility made as autovacuum would make
            // them in its first minutes, so the plans and the timings meet a board as the live one
            // stands, and no vacuum of the copy's is billed to a command timed after it.
            let (wall, copied) = (quiet.bill(|| {
                let began = Instant::now();
                store.copy(&dump()).unwrap();
                (client_of(&serving.socket).simple("VACUUM (ANALYZE)"))
                    .unwrap_or_else(|why| panic!("perf: the copy's vacuum: {why:?}"));
                began.elapsed()
            }))
            .unwrap_or_else(|why| panic!("perf: {why}"));
            // Seconds of the server's work, which any meter reads: one reading none reads another
            // process, and every bound a test of the board states would hold at none.
            cpu::worked("the board's copy, on its server", copied)
                .unwrap_or_else(|why| panic!("perf: {why}"));
            eprintln!(
                "perf: the board copied and analyzed in {wall:.1?}, {copied:.1?} of its server's CPU"
            );
        }
        let copied = copied(&db);
        let recipients = if cleanup_worker {
            Vec::new()
        } else {
            recipients()
        };
        let listeners = Listeners::new(&dir, &recipients);
        let path = |handle: &str| format!("uds:{}", listeners.paths[handle].display());
        // One registration through the store, for the listener's identity as the store records
        // it; the rest as rows like it, so no registration dispatches to those before it.
        if let Some((first, rest)) = recipients.split_first() {
            store
                .callback(first, Some(&path(first)), false, false)
                .unwrap();
            let raw = Raw::of(&db);
            let registered = (raw.row("callback_state", json!(first)).get())
                .unwrap_or_else(|why| panic!("perf: {first}'s registration: {why}"));
            assert!(
                registered.is_object(),
                "{first}'s registration: {registered}"
            );
            for handle in rest {
                let mut row = registered.clone();
                row["handle"] = json!(handle);
                row["address"] = json!(path(handle));
                (raw.row("callback_state", json!(handle)).put(row))
                    .unwrap_or_else(|why| panic!("perf: {handle}'s registration: {why}"));
            }
        }
        let mut board = Board {
            addr: format!("postgresql:{}", serving.socket.display()),
            socket: serving.socket,
            pid: serving.pid,
            copied,
            quiet,
            dir,
            listeners,
            first_looks: Looked::default(),
            unclaimed: ready(),
            server,
        };
        if cleanup_worker {
            return board;
        }
        // Every recipient's first look, through the relay and planned: what a write, and the
        // dispatch it makes to recipients not seen before, sends.
        let relay = Proxy::to(&board.addr);
        let began = Instant::now();
        let (thread, writer) = (thread(), writer());
        let opening = [
            "say",
            &thread,
            "the board opens",
            "--to",
            "@orchestrator",
            "--as",
            &writer,
        ];
        let (ran, logged) =
            board.explained(&relay, || board.run_unbilled(&relay.addr, &opening, ""));
        assert!(
            ran.status.success(),
            "the first looks: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
        eprintln!(
            "perf: every recipient's first look took {:.1?}, {} plans given",
            began.elapsed(),
            logged.plans.len()
        );
        board.first_looks = Looked {
            requests: relay.take(),
            logged,
        };
        board
    }

    /// A ready task nobody has claimed, for a claim.
    pub fn unclaimed(&mut self) -> String {
        self.unclaimed.pop().expect("a ready task is left to claim")
    }

    /// A client of the board, straight to its server.
    pub fn client(&self) -> pg::Client {
        client_of(&self.socket)
    }

    /// The board, straight on its server, as a store reaches it.
    pub fn database(&self) -> Database {
        self.client().into()
    }

    /// A store of the board, straight on its server, on `host`, its clock at [`NOW`].
    pub fn store(&self, host: &str) -> Store {
        Store::opening_on(
            self.database(),
            Box::new(|| NOW),
            Box::new(|_| false),
            host,
            &Opening::write(),
        )
        .unwrap_or_else(|why| panic!("perf: a store of the board: {why}"))
    }

    /// `tasks ARGS` as the [`program`], on the board as the server at `server` reaches it, `input`
    /// on its stdin, its clock at [`NOW`] and its dispatch in the write, as `TASKS_DISPATCH=inline`
    /// keeps it: what a write costs is what its command takes. Its cost in CPU is the process's
    /// own and what the board's server took for it, from quiet to quiet again ([`Board::bill`]).
    pub fn run(&self, server: &str, args: &[&str], input: &str) -> Ran {
        let ((said, tasks_cpu, wall), server_cpu) = self.bill(|| self.execute(server, args, input));
        Ran {
            code: said.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&said.stdout).into_owned(),
            err: String::from_utf8_lossy(&said.stderr).into_owned(),
            wall,
            tasks_cpu,
            server_cpu,
        }
    }

    /// The same command as [`Self::run`], without billing the server or waiting for quiet: a
    /// plan check needs its requests and output, not its cost.
    pub fn run_unbilled(&self, server: &str, args: &[&str], input: &str) -> Output {
        self.execute(server, args, input).0
    }

    /// Start, feed and reap one command. Both callers keep the same environment and process
    /// handling; only [`Self::run`] wraps this work in the server's bill.
    fn execute(&self, server: &str, args: &[&str], input: &str) -> (Output, Duration, Duration) {
        use std::io::Write as _;
        let mut command = Command::of(program());
        command
            .args(args)
            .cleared()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &self.dir)
            .env("TASKS_SERVER", server)
            .env("TASKS_DATABASE", DATABASE)
            .env("TASKS_NOW", NOW.to_string())
            .env("TASKS_DISPATCH", "inline")
            .env(
                symphony_board::UNANSWERED_WAIT_VAR,
                crate::store::LOOKUP_BACKSTOP,
            )
            .env("TASKS_PS_TABLE", self.dir.join("ps"))
            .env("HERDR_BIN", "/usr/bin/false")
            .current_dir(&self.dir);
        let began = Instant::now();
        let mut child = sys::spawn_piped(&mut command).expect("the tasks binary starts");
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input.as_bytes()).unwrap();
        }
        let (said, tasks_cpu) = cpu::reaped(child);
        (said, tasks_cpu, began.elapsed())
    }

    /// `work` done, and the user and system CPU the board's server took for it, from quiet before
    /// it to quiet again after it ([`cpu::Quiet::bill`]): not what the server still did for the
    /// work before, which a loaded machine leaves for later, and all it does for `work`, after
    /// `work` had its answer too. The test doing it holds the board, so no other test's work
    /// reaches that server meanwhile. A wait for quiet past [`SETTLE`] fails the test, saying
    /// what the server took, and every wait after it fails at once.
    pub fn bill<T>(&self, work: impl FnOnce() -> T) -> (T, Duration) {
        (self.quiet.bill(work)).unwrap_or_else(|why| panic!("perf: {why}"))
    }

    /// `work` done, and the plan of every statement a session of the board's database started
    /// meanwhile ran ([`EXPLAINING`], [`plans_of`]): the settings are set before `work` and reset
    /// after it, also when it panics, and apply to the sessions that start between, each
    /// command's. Each plan is a notice to the session that ran the statement, which `relay` keeps
    /// before the answer it belongs to passes, so every plan of `work` done through `relay` is kept
    /// once `work` is done. A notice kept before the run is no plan of it, and is forgotten.
    pub fn explained<T>(&self, relay: &Proxy, work: impl FnOnce() -> T) -> (T, Logged) {
        relay.notices();
        let settings = Explaining::set(&self.socket);
        let done = work();
        drop(settings);
        (done, plans_of(&relay.notices()))
    }
}

/// [`EXPLAINING`] set on the board's database, until it is dropped.
struct Explaining {
    admin: pg::Client,
}

impl Explaining {
    fn set(socket: &Path) -> Explaining {
        let admin = pg::Client::new(socket, pg::MAINTENANCE).waiting(PATIENCE);
        let set: String = (EXPLAINING.iter())
            .map(|(name, value)| format!("ALTER DATABASE {DATABASE} SET {name} = {value};\n"))
            .collect();
        (admin.simple(&set)).unwrap_or_else(|why| panic!("perf: the plans' settings: {why:?}"));
        Explaining { admin }
    }
}

impl Drop for Explaining {
    fn drop(&mut self) {
        let reset: String = (EXPLAINING.iter())
            .map(|(name, _)| format!("ALTER DATABASE {DATABASE} RESET {name};\n"))
            .collect();
        if let Err(why) = self.admin.simple(&reset) {
            eprintln!("perf: the plans' settings were not reset: {why:?}");
        }
    }
}

#[cfg(test)]
mod runs {
    use super::*;

    /// An earlier run's folder is one whose process ended, named for it; this process's, or a
    /// name of another shape, is not.
    #[test]
    fn an_ended_run_is_one_whose_process_ended() {
        let mine = std::process::id();
        assert!(!ended_run(
            &format!("symphony-perf-{mine}"),
            "symphony-perf-"
        ));
        assert!(!ended_run(
            &format!("symphony-perf-relay-{mine}-3"),
            "symphony-perf-relay-"
        ));
        assert!(ended_run("symphony-perf-999999999", "symphony-perf-"));
        assert!(!ended_run("symphony-perf-cleanup", "symphony-perf-"));
        assert!(!ended_run("other-123", "symphony-perf-"));
    }
}
