//! Exercise the real fixture boundary: shutdown must finish before a scope or test process ends,
//! and an error keeps its original cause. Every server, log and port belongs to this case.

use crate::children::Child as _;
use crate::{board, store};
use std::hash::{BuildHasher, RandomState};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::OnceLock;
use symphony_board::db::DbError;
use symphony_board::rpc::{self, Client};
use symphony_board::sys::{self, Command};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let token = RandomState::new().hash_one(name);
        let dir = std::env::temp_dir().join(format!(
            "symphony-perf-cleanup-{}-{name}-{token:016x}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }

    fn server(&self, name: &str) -> (String, store::ServerChild) {
        let (addr, child) = store::launch(&store::surreal(), &self.0.join(name)).unwrap();
        assert!(
            rpc::healthy(&addr),
            "the owned server must be up before cleanup"
        );
        (addr, child)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stopped(addr: &str) {
    let _released = TcpListener::bind(addr).expect("shutdown must release the owned listener");
}

#[test]
fn completion_setup_error_and_worker_unwind_reap_only_their_own_servers() {
    let scratch = Scratch::new("scopes");
    let (other_addr, other) = scratch.server("other.log");
    let (addr, server) = scratch.server("normal.log");
    drop(server);
    stopped(&addr);

    let (addr, server) = scratch.server("setup-error.log");
    let setup = || -> Result<(), DbError> {
        let _server = server;
        Client::new(&addr, "sf", "private")
            .query("THROW 'fixture setup refused';", serde_json::json!({}))?;
        Ok(())
    };
    assert_eq!(
        setup(),
        Err(DbError::Thrown("fixture setup refused".into()))
    );
    stopped(&addr);

    let (addr, server) = scratch.server("worker-panic.log");
    std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let _server = server;
            std::panic::panic_any(String::from("fixture worker refused"));
        });
        let why = worker
            .join()
            .expect_err("the worker's original panic survives cleanup");
        assert_eq!(
            why.downcast_ref::<String>().unwrap(),
            "fixture worker refused"
        );
        stopped(&addr);
    });
    assert!(rpc::healthy(&other_addr), "another owned run remains up");
    drop(other);
    stopped(&other_addr);
}

#[test]
fn static_board_exit_waits_for_its_server_even_after_a_poisoned_worker() {
    let scratch = Scratch::new("exit");
    let record = scratch.0.join("address");
    let done = sys::output(
        Command::of(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "cleanup::static_board_worker",
                "--nocapture",
            ])
            .env("SYMPHONY_PERF_CLEANUP_RECORD", &record),
    )
    .unwrap();
    eprintln!(
        "static board subprocess:\n{}\n{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    assert!(
        done.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(record.with_extension("stopped"))
            .expect("the exit witness must confirm shutdown before descriptors close"),
        "stopped"
    );
    let addr = std::fs::read_to_string(record).expect("the worker recorded its live board");
    stopped(&addr);
}

static EXIT_ADDRESS: OnceLock<(String, PathBuf)> = OnceLock::new();

extern "C" fn require_stopped_before_descriptors_close() {
    if let Some((addr, stopped)) = EXIT_ADDRESS.get() {
        if let Ok(_released) = TcpListener::bind(addr) {
            let _ = std::fs::write(stopped, "stopped");
        } else {
            eprintln!("the static board survived its exit cleanup: {addr}");
        }
    }
}

#[test]
#[ignore = "private subprocess exercising the static board's process-exit cleanup"]
fn static_board_worker() {
    let record = PathBuf::from(
        std::env::var_os("SYMPHONY_PERF_CLEANUP_RECORD").expect("private address record"),
    );
    // Exit callbacks run in reverse registration order. This witness runs after the board's
    // cleanup, but before process exit closes its descriptors: an EOF-only cleanup cannot pass.
    store::on_exit(require_stopped_before_descriptors_close);
    let held = board::board();
    assert!(rpc::healthy(&held.addr));
    assert_eq!(
        held.copied,
        serde_json::json!({
            "task": 3, "link": 0, "session": 0, "role": 0, "message": 0, "delivery": 0,
            "barred_key": 0, "task_session": 0, "delivery_key": 0, "ask": 0, "staging": [],
        }),
        "the cleanup worker needs only a few rows on the static board"
    );
    assert!(held.listeners.paths.is_empty());
    assert!(held.first_looks.is_empty());
    EXIT_ADDRESS
        .set((held.addr.clone(), record.with_extension("stopped")))
        .unwrap();
    std::fs::write(record, &held.addr).unwrap();
    drop(held);
    let worker = std::thread::spawn(|| {
        let _held = board::board();
        std::panic::panic_any(String::from("board worker refused"));
    });
    assert_eq!(
        worker.join().unwrap_err().downcast_ref::<String>().unwrap(),
        "board worker refused"
    );
    // The static is poisoned; atexit must still take and drop it synchronously.
}
