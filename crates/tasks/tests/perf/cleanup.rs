//! The suite's board at the test binary's exit: its PostgreSQL is stopped and has ended before the
//! process does, also after a worker that held the board panicked, which poisons the static that
//! holds it. A scratch board's server would end with the process anyway, once its lifeline reads
//! the end (`symphony_board::server::postgres`); the exit's own stop is what this proves, by a
//! witness that runs after it and before the process's descriptors close.

use crate::board;
use crate::children::Child as _;
use crate::store;
use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use symphony_board::Raw;
use symphony_board::server::postgres::SOCKET_FILE;
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
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Whether the PostgreSQL whose data is `data` and whose socket is in `socket` has stopped: its
/// record of itself (`postmaster.pid`), which it removes as the last step of its shutdown, is
/// gone, and so is its socket.
fn stopped(data: &Path, socket: &Path) -> bool {
    !data.join("postmaster.pid").exists() && !socket.join(SOCKET_FILE).exists()
}

#[test]
fn static_board_exit_waits_for_its_server_even_after_a_poisoned_worker() {
    let scratch = Scratch::new("exit");
    let record = scratch.0.join("server");
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
    let said = format!(
        "{}\n{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    eprintln!("static board subprocess:\n{said}");
    assert!(done.status.success(), "{said}");
    assert_eq!(
        std::fs::read_to_string(record.with_extension("stopped"))
            .expect("the exit witness must confirm shutdown before descriptors close"),
        "stopped"
    );
    let recorded = std::fs::read_to_string(&record).expect("the worker recorded its live board");
    let mut lines = recorded.lines();
    let (pid, data, socket) = (
        lines.next().unwrap().parse::<i64>().unwrap(),
        PathBuf::from(lines.next().unwrap()),
        PathBuf::from(lines.next().unwrap()),
    );
    let ended = sys::gone(pid, test_time::backstop(Duration::from_secs(10)))
        .unwrap_or_else(|why| panic!("the end of the board's postmaster {pid}: {why}"));
    assert!(ended, "the board's postmaster {pid} outlived its process");
    assert!(stopped(&data, &socket), "{recorded}");
}

/// What the exit witness reads: the board's data and socket, and where it says they stopped.
static EXIT_RECORD: OnceLock<(PathBuf, PathBuf, PathBuf)> = OnceLock::new();

extern "C" fn require_stopped_before_descriptors_close() {
    if let Some((data, socket, witness)) = EXIT_RECORD.get() {
        if stopped(data, socket) {
            let _ = std::fs::write(witness, "stopped");
        } else {
            eprintln!(
                "the static board's server survived its exit cleanup: {}",
                data.display()
            );
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
    // cleanup, but before process exit closes its descriptors: a cleanup that left the server to
    // its lifeline's end cannot pass.
    store::on_exit(require_stopped_before_descriptors_close);
    let held = board::board();
    let serving = (held.server.current())
        .unwrap()
        .expect("the board's server serves");
    assert_eq!(serving.pid, held.pid);
    assert_eq!(
        Raw::of(&held.database()).table("task").count().unwrap(),
        3,
        "the board answers"
    );
    assert_eq!(
        held.copied,
        serde_json::json!({
            "task": 3, "link": 0, "session": 0, "role": 0, "message": 0, "delivery": 0,
            "barred_key": 0, "task_session": 0, "delivery_key": 0, "ask": 0,
        }),
        "the cleanup worker needs only a few rows on the static board"
    );
    assert!(held.listeners.paths.is_empty());
    assert!(held.first_looks.requests.is_empty());
    let data = held.server.data();
    assert!(!stopped(&data, &held.socket), "the board's server is up");
    EXIT_RECORD
        .set((
            data.clone(),
            held.socket.clone(),
            record.with_extension("stopped"),
        ))
        .unwrap();
    std::fs::write(
        &record,
        format!(
            "{}\n{}\n{}\n",
            held.pid,
            data.display(),
            held.socket.display()
        ),
    )
    .unwrap();
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
