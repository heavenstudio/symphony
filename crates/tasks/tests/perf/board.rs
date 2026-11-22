//! The suite's board: the generated rows ([`crate::generated`]) copied onto a server of this test
//! binary's own that keeps them in `surrealkv://`, the live board's engine, a socket for each
//! recipient with a callback, and the `tasks` binary run on it as an agent runs it.
//!
//! The board is made once for the test binary ([`board`]): its copy and the first look of every
//! recipient take most of the suite's time. Each test holds it while it runs, so a timing never
//! shares the server with another test's commands.

use crate::children::Child as _;
use crate::cpu;
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
use symphony_board::rows::{Dump, TaskRow};
use symphony_board::rpc::Client;
use symphony_board::server::Storage;
use symphony_board::sys::{self, Command};
use symphony_board::{NAMESPACE, Store};

/// The database the board is in on the suite's server.
pub const DATABASE: &str = "perf";

/// The rows of each table [`Store::copy`] fills, and its staging tables (`copy_…`) still on the
/// board, in one read.
const COPIED: &str = "RETURN {
    task: array::len((SELECT VALUE id FROM task)),
    link: array::len((SELECT VALUE id FROM link)),
    session: array::len((SELECT VALUE id FROM session)),
    role: array::len((SELECT VALUE id FROM role)),
    message: array::len((SELECT VALUE id FROM message)),
    delivery: array::len((SELECT VALUE id FROM delivery)),
    barred_key: array::len((SELECT VALUE id FROM barred_key)),
    task_session: array::len((SELECT VALUE id FROM task_session)),
    delivery_key: array::len((SELECT VALUE id FROM delivery_key)),
    ask: array::len((SELECT VALUE id FROM ask)),
    staging: array::filter(object::keys((INFO FOR DB).tables), |$table| string::starts_with($table, 'copy_')),
};";

/// The longest a wait for the board's server to go quiet ([`Board::bill`]) may take: past it the
/// test fails, saying what the server took, and every wait after it fails at once.
const SETTLE: Duration = Duration::from_secs(300);

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

/// The suite's board: its server, the recipients' sockets, and what the recipients' first looks
/// sent.
pub struct Board {
    pub addr: String,
    /// Where the board's server keeps it: `surrealkv://` in the run's folder.
    pub engine: String,
    /// The rows of each table the copy filled, and the copy's staging tables left, read right after
    /// the copy ([`COPIED`]), before any test's command writes.
    pub copied: Value,
    /// The waits for the board's server's own process to go quiet, across which a run's cost
    /// counts its CPU.
    quiet: cpu::Quiet,
    dir: PathBuf,
    pub listeners: Listeners,
    /// What the write whose dispatch gave every recipient its first look sent the server.
    pub first_looks: Vec<Request>,
    /// The ready tasks not claimed yet.
    unclaimed: Vec<String>,
    /// Closing the lifetime pipe and waiting reaps the server and watchdog before teardown ends.
    _server: store::ServerChild,
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

/// The folder of each earlier run of the suite in `temp` (named for its process) whose process
/// has ended. Its server ended with that process, through the pipe [`Board`] holds; its folder
/// stays until a later run removes it here: a run removes none of its own, as one stopped by a
/// signal could not.
fn remove_ended_runs(temp: &Path) {
    let Ok(entries) = std::fs::read_dir(temp) else {
        return;
    };
    let ended = entries.flatten().filter(|entry| {
        let name = entry.file_name();
        let pid = (name.to_str())
            .and_then(|name| name.strip_prefix("symphony-perf-"))
            .and_then(|pid| pid.parse::<i64>().ok());
        pid.is_some_and(|pid| !sys::pid_alive(pid))
    });
    for entry in ended {
        let _ = std::fs::remove_dir_all(entry.path());
    }
}

/// The `tasks` binary timed: `SYMPHONY_PERF_TASKS` when it names one (another build's, to time it on the
/// same board), else this crate's.
pub fn program() -> PathBuf {
    std::env::var_os("SYMPHONY_PERF_TASKS")
        .map_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_tasks")), PathBuf::from)
}

/// How the suite's server keeps its data: as the board's, with a memtable that holds the copy of
/// this board, one transaction (it does not fit the board's own 256 MiB), whatever the machine's
/// memory is. Left to SurrealKV, the memtable is a share of that memory, too small on a machine of
/// 7 GiB.
const STORAGE: Storage = Storage {
    memtable: MEMTABLE_MIB << 20,
    ..Storage::BOARD
};
const MEMTABLE_MIB: u64 = 1024;

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
        let engine = format!("surrealkv://{}", dir.join("data").display());
        let (addr, server) = store::launch_under(
            &store::surreal(),
            &dir.join("surreal.log"),
            &engine,
            Some(STORAGE),
        )
        .unwrap_or_else(|why| panic!("the suite's server did not come up: {why:?}"));
        let pid = server.server.expect("a server up has said its process");
        let quiet = cpu::Quiet::new("the board's server", pid, SETTLE);
        let store = Store::with(
            Client::new(&addr, NAMESPACE, DATABASE).waiting(Duration::from_secs(600)),
            Box::new(|| NOW),
            Box::new(|_| false),
            &sys::hostname(),
        )
        .unwrap();
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
            let (wall, copied) = (quiet.bill(|| {
                let began = Instant::now();
                store.copy(&dump()).unwrap();
                began.elapsed()
            }))
            .unwrap_or_else(|why| panic!("perf: {why}"));
            // Seconds of the server's work, which any meter reads: one reading none reads another
            // process, and every bound a test of the board states would hold at none.
            cpu::worked("the board's copy, on its server", copied)
                .unwrap_or_else(|why| panic!("perf: {why}"));
            eprintln!("perf: the board copied in {wall:.1?}, {copied:.1?} of its server's CPU");
        }
        let copied = store
            .client()
            .query(COPIED, json!({}))
            .unwrap_or_else(|why| panic!("perf: the copied board's rows: {why}"))
            .remove(0);
        let recipients = if cleanup_worker {
            Vec::new()
        } else {
            recipients()
        };
        let listeners = Listeners::new(&dir, &recipients);
        let path = |handle: &str| format!("uds:{}", listeners.paths[handle].display());
        // One registration through the store, for the listener's identity as the store records it;
        // the rest as rows like it, so no registration dispatches to those before it.
        if let Some((first, rest)) = recipients.split_first() {
            store
                .callback(first, Some(&path(first)), false, false)
                .unwrap();
            let owner = (store.raw().row("callback_state", json!(first)))
                .field("owner")
                .unwrap();
            let rows: Vec<Value> = (rest.iter())
                .map(|handle| {
                    json!({"handle": handle, "address": path(handle), "host": sys::hostname(), "owner": owner,
                        "confirmed_until": 0, "chat": false, "chat_fallback": false, "chat_resume": false,
                        "registration_revision": 1})
                })
                .collect();
            store
                .client()
                .query(
                    "FOR $row IN $rows { UPSERT type::record('callback_state', $row.handle) CONTENT $row RETURN NONE; }; RETURN NONE;",
                    json!({"rows": rows}),
                )
                .unwrap();
        }
        let mut board = Board {
            addr,
            engine,
            copied,
            quiet,
            dir,
            listeners,
            first_looks: Vec::new(),
            unclaimed: ready(),
            _server: server,
        };
        if cleanup_worker {
            return board;
        }
        // Every recipient's first look, through the relay: what a write, and the dispatch it makes
        // to recipients not seen before, sends.
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
        let ran = board.run(&relay.addr, &opening, "");
        assert_eq!(ran.code, 0, "the first looks: {}", ran.err);
        eprintln!(
            "perf: every recipient's first look took {:.1?}",
            began.elapsed()
        );
        board.first_looks = relay.take();
        board
    }

    /// A ready task nobody has claimed, for a claim.
    pub fn unclaimed(&mut self) -> String {
        self.unclaimed.pop().expect("a ready task is left to claim")
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

    /// A client of the board, straight to its server.
    pub fn client(&self) -> Client {
        Client::new(&self.addr, NAMESPACE, DATABASE).waiting(Duration::from_secs(600))
    }

    /// `work` done, and the user and system CPU the board's server took for it, from quiet before
    /// it to quiet again after it ([`cpu::Quiet::bill`]): not what the server still did for the
    /// work before, which a loaded machine leaves for later (the first say after the board's copy
    /// took 10.8 and 13.9 s of the server's CPU at load 100 and 160, where the two after it took 2,
    /// and the first one 2 at lower load), and all it does for `work`, after `work` had its answer
    /// too. The test doing it holds the board, so no other test's work reaches that server
    /// meanwhile. A wait for quiet past [`SETTLE`] fails the test, saying what the server took,
    /// and every wait after it fails at once.
    pub fn bill<T>(&self, work: impl FnOnce() -> T) -> (T, Duration) {
        (self.quiet.bill(work)).unwrap_or_else(|why| panic!("perf: {why}"))
    }
}
