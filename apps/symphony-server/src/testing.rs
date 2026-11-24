//! Existing review/store fixtures, with private repositories and databases for hub observations.

#[expect(
    dead_code,
    reason = "The hub uses the real store fixture's clock and database subset"
)]
#[path = "../../../crates/tasks/tests/store/common.rs"]
pub mod board;
/// The git layouts a checkout can have, shared with the symphony crate's tests.
#[path = "../../../crates/symphony/tests/common/layouts.rs"]
pub mod layouts;
#[path = "../../../crates/symphony/tests/common/tree.rs"]
mod tree;
pub use crate::shelf;
pub use tree::Tree;
/// A watcher whose events the test tells.
pub mod handing;

pub fn git(root: &std::path::Path, args: &[&str]) {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(args)
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR"),
    )
    .unwrap();
    assert!(
        done.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
}

/// The gate nearly every fixture serves behind: no peer is on the tailnet, at the machine's clock.
/// A test whose gate answers a peer, panics when asked, names this machine or sets the clock
/// builds its own.
pub fn open_gate() -> crate::gate::Gate {
    crate::gate::Gate::new(
        std::sync::Arc::new(|_| Ok(None)),
        std::sync::Arc::new(std::time::Instant::now),
    )
}

/// What the API answers a GET of `uri` asked of `shared` from this machine, as the server on port
/// 1234 a fixture stands for: its status and its body read as JSON.
pub async fn get_json(
    shared: &std::sync::Arc<crate::front::Shared>,
    uri: &str,
) -> (axum::http::StatusCode, serde_json::Value) {
    let request = axum::http::Request::builder()
        .uri(uri)
        .header("Host", "localhost:1234")
        .body(())
        .unwrap();
    let response = crate::api::answer(shared, request).await.unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

/// A GET of `path` with no body, asked of the router as a page on this machine asks it.
pub fn request(path: &str) -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .uri(path)
        .header("Host", "127.0.0.1:1")
        .body(axum::body::Body::empty())
        .unwrap()
}

/// A store of its own on the board in `db` at the fixture's time and host.
pub fn store_on(db: &symphony_board::Database) -> symphony_board::Store {
    symphony_board::Store::opening_on(
        db.another(),
        Box::new(|| board::NOW),
        Box::new(|_| false),
        board::HOST,
        &symphony_board::Opening::write(),
    )
    .unwrap()
}

/// The board `TASKS_SERVER` and `TASKS_DATABASE` name, as this process's commands reach it
/// ([`symphony_board::cli::writable_board`]): a client of its own, for a test's child that a
/// [`child_on`] handed the board.
pub fn named_board() -> symphony_board::Database {
    let env = symphony_board::cli::Env::of_process();
    let reached = symphony_board::cli::writable_board(&env).unwrap();
    reached.database().unwrap()
}

/// The test `test` run alone in a child of this test binary on `board`, `marker` set so it takes
/// its child side, with nothing of this process's session, client build or git folder: what it
/// printed and what it logged, once it passed.
pub fn child_on(board: &symphony_board::Database, marker: &str, test: &str) -> (String, String) {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture"])
            .env(marker, "1")
            .env("TASKS_SERVER", board.addr())
            .env("TASKS_DATABASE", board.database())
            .env_remove("ASKS_DB")
            .env_remove("TASKS_SESSION")
            .env_remove("SYMPHONY_UI_DIST")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_INDEX_FILE"),
    )
    .unwrap();
    let out = String::from_utf8_lossy(&done.stdout).into_owned();
    let err = String::from_utf8_lossy(&done.stderr).into_owned();
    assert!(done.status.success(), "{out}\n{err}");
    assert!(out.contains("1 passed"), "child ran no test: {out}");
    (out, err)
}

/// A FIFO at `path`, held open for reading and writing, so neither a fixture child's open for
/// writing nor this one blocks.
pub fn fifo(path: &std::path::Path) -> std::fs::File {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: name is a live, terminated path; the FIFO is owned by this fixture.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap()
}

/// Wait for a fixture record until the caller's deadline, retrying interrupted polls.
fn wait_entered(fd: std::os::fd::RawFd, deadline: std::time::Instant) {
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "fixture child readiness deadline passed: a stall; {}",
            test_time::Load::now()
        );
        // Round up so a sub-millisecond remainder still waits for readiness.
        let timeout = i32::try_from(remaining.as_nanos().div_ceil(1_000_000)).unwrap();
        // SAFETY: poll points to one initialized pollfd; poll reports invalid descriptors.
        let ready = unsafe { libc::poll(&mut poll, 1, timeout) };
        let error = if ready < 0 {
            Some(std::io::Error::last_os_error())
        } else if poll.revents & libc::POLLNVAL != 0 {
            Some(std::io::Error::from_raw_os_error(libc::EBADF))
        } else {
            None
        };
        if let Some(error) = error {
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            panic!("fixture child readiness poll failed: {error}");
        }
        if ready > 0 {
            break;
        }
    }
}

/// The pid a fixture child writes to the FIFO `file` as it enters, waited for until a backstop only a
/// stall reaches (six times the 30 s it had as a bound).
pub fn entered(file: &std::fs::File) -> i64 {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    wait_entered(
        file.as_raw_fd(),
        std::time::Instant::now() + test_time::backstop(std::time::Duration::from_secs(30)),
    );
    // The shell writes this short record atomically; one read also bounds malformed records
    // that omit their newline instead of hanging a line reader after the readiness deadline.
    let mut record = [0; 32];
    let mut reader = file;
    let count = reader.read(&mut record).unwrap();
    std::str::from_utf8(&record[..count])
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

/// Asserts that the stand-in `crate::fixture_script::write` installed at `program` is the
/// machine's one copy of its fixture script, which that write ran once, to its end, before handing
/// it out. macOS holds a new file's first exec while it assesses the file (25 to 31 s at a load of
/// 120, against milliseconds for the next exec), so that wait is setup, never part of the budget
/// of the case that starts the stand-in.
pub fn assert_primed(program: &std::path::Path) {
    use std::os::unix::fs::MetadataExt;
    let mut body = program.as_os_str().to_owned();
    body.push(".body");
    let body = std::fs::read_to_string(&body).unwrap();
    let copy = crate::fixture_script::machine_copy::machine_copy(
        &crate::fixture_script::tracked_wrapper(&body),
    );
    let (installed, shared) = (
        std::fs::metadata(program).unwrap(),
        std::fs::metadata(&copy).unwrap(),
    );
    assert_eq!(
        (installed.dev(), installed.ino()),
        (shared.dev(), shared.ino()),
        "{} is not the machine's already-run copy {} of its fixture script",
        program.display(),
        copy.display()
    );
}

/// What `thread` returned, once it has; a stall past `within` fails naming `what`, so a loop that
/// misses the wake meant to end it fails the test rather than hold it.
pub fn joined<T>(thread: std::thread::JoinHandle<T>, what: &str, within: std::time::Duration) -> T {
    test_time::until_true(what, within, || thread.is_finished())
        .unwrap_or_else(|stall| panic!("{stall}"));
    thread.join().unwrap()
}

pub fn repository(tree: &Tree) -> std::path::PathBuf {
    repository_at(tree, "main")
}

/// A repository in the folder `name` of `tree`, one document committed: its canonical root.
pub fn repository_at(tree: &Tree, name: &str) -> std::path::PathBuf {
    repository_holding(
        tree,
        name,
        "docs/TOPIC.md",
        "# Topic\n\n- [ ] **initial-card** — Initial question.\n",
    )
}

/// A repository in the folder `name` of `tree` with the one file `file` committed, `text` in it:
/// its canonical root.
pub fn repository_holding(tree: &Tree, name: &str, file: &str, text: &str) -> std::path::PathBuf {
    let root = tree.path(name);
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-b", "main"]);
    git(&root, &["config", "user.name", "Review fixture"]);
    git(&root, &["config", "user.email", "review@example.invalid"]);
    git(&root, &["config", "core.hooksPath", "/dev/null"]);
    tree.write(&format!("{name}/{file}"), text);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "Initial commit"]);
    root.canonicalize().unwrap()
}

/// The machine folder `machine` with a registry of the projects `ids`, each named `ID works` at a
/// root of its id beside the registry: the projects the machine's one server serves.
pub fn registered(machine: &std::path::Path, ids: &[&str]) {
    let projects = (ids.iter())
        .map(|id| symphony::machine::Entry {
            id: (*id).into(),
            name: format!("{id} works"),
            root: machine.join(id),
            manifest: None,
            plugins: Vec::new(),
            pin: 0,
            schema: 0,
        })
        .collect();
    symphony::machine::Registry { projects }
        .write(machine)
        .unwrap();
}

/// A colleague's laptop under a login other than this machine's (`ana@example.com`), as a peer
/// [`laptop_with`]'s whois names it.
pub const LAPTOP: &str = "100.101.102.104:5000";

/// A gate whose whois names a colleague's laptop at [`LAPTOP`], under a login other than this
/// machine's, and whose roster is `roster`.
pub fn laptop_with(roster: crate::gate::Roster) -> crate::gate::Gate {
    crate::gate::Gate::new(
        std::sync::Arc::new(|address: &str| {
            Ok((address == "100.101.102.104").then(|| {
                r#"{"Node": {"Name": "laptop"}, "UserProfile": {"ID": 12, "LoginName": "ana@example.com"}}"#.to_string()
            }))
        }),
        std::sync::Arc::new(std::time::Instant::now),
    )
    .with_status(std::sync::Arc::new(|| {
        Ok(r#"{"Self": {"DNSName": "studio.tail.ts.net.", "UserID": 11}}"#.to_string())
    }))
    .with_roster(roster)
}

/// The board of the project at a root, made on PostgreSQL as its command line makes it, in the
/// checkout's git folder: its server stopped when it drops.
pub struct Board(symphony_board::server::Home);

impl Board {
    /// The board at `root`, each of `people` on it with their powers, added by the person at the
    /// machine.
    pub fn holding(
        root: &std::path::Path,
        people: &[(&str, &[symphony_board::people::Power])],
    ) -> Board {
        let board = Board(symphony_board::server::Home::in_common(&root.join(".git")));
        let store = Board::store(root);
        for (name, powers) in people {
            store.add_person(name, &[], powers, "owner").unwrap();
        }
        board
    }

    /// A store on the board at `root`, as its command line reaches it.
    pub fn store(root: &std::path::Path) -> symphony_board::Store {
        symphony_board::cli::connect(&symphony_board::cli::Env {
            vars: Default::default(),
            cwd: root.to_path_buf(),
        })
        .unwrap()
    }
}

impl Drop for Board {
    fn drop(&mut self) {
        let _ = self.0.stop_board();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SIGNALS: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn interrupted(_: libc::c_int) {
        SIGNALS.fetch_add(1, Ordering::Relaxed);
    }

    /// The open gate answers this machine and nobody else: loopback, in each form, is admitted,
    /// and a tailnet address and one outside the tailnet are refused. That an address outside the
    /// tailnet is refused without asking whois is gate.rs's
    /// (`an_address_outside_the_tailnet_is_refused_without_asking`).
    #[test]
    fn the_open_gate_admits_this_machine_and_no_peer() {
        let gate = open_gate();
        for (ip, admitted) in [
            ("127.0.0.1", true),
            ("::1", true),
            ("::ffff:127.0.0.1", true),
            ("100.83.24.99", false),
            ("192.168.1.5", false),
        ] {
            assert_eq!(gate.allows(ip.parse().unwrap()), admitted, "{ip}");
        }
    }

    #[test]
    fn a_request_is_a_get_of_its_path_and_query_from_this_machine_with_no_body() {
        let asked = request("/api/tasks?slug=tools/run");
        assert_eq!(asked.method(), axum::http::Method::GET);
        assert_eq!(asked.uri(), "/api/tasks?slug=tools/run");
        let headers: Vec<_> = asked.headers().iter().collect();
        assert_eq!(headers.len(), 1, "{headers:?}");
        assert_eq!(asked.headers()["Host"], "127.0.0.1:1");
        use axum::body::HttpBody;
        assert!(asked.body().is_end_stream());
    }

    /// The JSON ask gives a refusal's status and body as it gives an answer's, and asks as the
    /// fixture's own host: a route no hub is needed for answers it.
    #[tokio::test]
    async fn the_json_ask_reads_a_refusals_status_and_body() {
        let tree = Tree::new();
        let root = repository(&tree);
        let shared = crate::front::Shared::new(1, 1234, &root, None, open_gate());
        let (status, said) = get_json(&shared, "/api/reports?grain=month").await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(
            said,
            serde_json::json!({ "error": "grain is hour, day or week, not month" })
        );
        assert!(shared.api.get().is_none(), "no hub was started");
    }

    /// A second store on a board reads what the first wrote, at the fixture's clock.
    #[test]
    fn a_store_on_a_board_reads_that_board_at_the_fixtures_clock() {
        let fixture = board::Fixture::new(board::Seed::default().task("tools/run", "ready"));
        let store = store_on(fixture.store.database());
        assert_eq!(store.now(), board::NOW);
        assert_eq!(store.get("tools/run").unwrap().slug, "tools/run");
        assert!(store.get("tools/other").is_err());
        let again = store_on(&fixture.other_database());
        let slugs: Vec<_> = (again.tasks(None).unwrap().into_iter())
            .map(|task| task.slug)
            .collect();
        assert_eq!(slugs, ["tools/run"]);
    }

    /// A child that ran no test passed nothing: the name it was given selects none.
    #[test]
    fn a_child_that_runs_no_test_is_refused() {
        let board = board::database();
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            child_on(
                &board,
                "SYMPHONY_TESTING_NO_CHILD",
                "testing::tests::no_such_test",
            )
        }))
        .unwrap_err();
        let message = *refused.downcast::<String>().unwrap();
        assert!(
            message.starts_with("child ran no test: ") && message.contains("0 passed"),
            "{message}"
        );
    }

    #[test]
    fn readiness_without_a_record_reports_its_deadline() {
        use std::os::fd::AsRawFd;
        let tree = Tree::new();
        let file = fifo(&tree.path("empty"));
        // Nothing writes the FIFO, which the test holds open: only the deadline ends the wait.
        let fd = file.as_raw_fd();
        let waiting = std::thread::spawn(move || {
            std::panic::catch_unwind(|| {
                wait_entered(
                    fd,
                    std::time::Instant::now() + std::time::Duration::from_millis(1),
                );
            })
        });
        test_time::until_true(
            "the fixture wait to end at the caller's deadline",
            test_time::backstop(std::time::Duration::from_secs(10)),
            || waiting.is_finished(),
        )
        .unwrap_or_else(|stall| panic!("fixture wait ignored the caller's deadline: {stall}"));
        let panic = waiting.join().unwrap().unwrap_err();
        drop(file);
        let message = match panic.downcast::<String>() {
            Ok(message) => *message,
            Err(panic) => (*panic.downcast::<&str>().unwrap()).to_owned(),
        };
        assert!(
            message.starts_with("fixture child readiness deadline passed: a stall; ")
                && message.contains("load average"),
            "{message}"
        );
    }

    /// A stand-in is primed only as the machine's copy `fixture_script::write` links; a file
    /// written in place with the same bytes and body is new to macOS, and is refused.
    #[test]
    fn only_the_machine_copy_of_a_fixture_script_passes_as_primed() {
        let tree = Tree::new();
        let body = "#!/bin/sh\nexit 0\n";
        let linked = tree.path("linked");
        crate::fixture_script::write(&linked, body, false);
        assert_primed(&linked);
        let fresh = tree.write("fresh", &std::fs::read_to_string(&linked).unwrap());
        tree.write("fresh.body", body);
        let refused = std::panic::catch_unwind(|| assert_primed(&fresh)).unwrap_err();
        let message = *refused.downcast::<String>().unwrap();
        assert!(
            message.contains("is not the machine's already-run copy"),
            "{message}"
        );
    }

    #[test]
    fn readiness_on_an_invalid_descriptor_reports_the_os_error() {
        let panic = std::panic::catch_unwind(|| {
            wait_entered(
                std::os::fd::RawFd::MAX,
                std::time::Instant::now() + std::time::Duration::from_secs(30),
            );
        })
        .unwrap_err();
        assert_eq!(
            *panic.downcast::<String>().unwrap(),
            format!(
                "fixture child readiness poll failed: {}",
                std::io::Error::from_raw_os_error(libc::EBADF)
            )
        );
    }

    #[test]
    fn entered_reads_pid_after_signals_interrupt_its_wait() {
        let tree = Tree::new();
        let mut file = fifo(&tree.path("entered"));
        // SIGURG has no other fixture owner. Restore the process-wide disposition even if
        // the readiness assertion unwinds; only this test's waiting thread receives it.
        struct Restore(libc::sigaction);
        impl Drop for Restore {
            fn drop(&mut self) {
                // SAFETY: the saved disposition came from sigaction below.
                assert_eq!(
                    unsafe { libc::sigaction(libc::SIGURG, &self.0, std::ptr::null_mut()) },
                    0
                );
            }
        }
        // SAFETY: zero initializes the action and its mask; sigaction fills the old action.
        let _restore = unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            let mut old = std::mem::zeroed();
            action.sa_sigaction = interrupted as *const () as usize;
            libc::sigemptyset(&mut action.sa_mask);
            assert_eq!(libc::sigaction(libc::SIGURG, &action, &mut old), 0);
            Restore(old)
        };
        std::thread::scope(|scope| {
            // SAFETY: this thread stays alive through the scoped sender's join, even on panic.
            let thread = unsafe { libc::pthread_self() };
            let reader = file.try_clone().unwrap();
            scope.spawn(move || {
                // Keep the FIFO empty across multiple signal deliveries so the waiting call
                // must survive interruptions before the atomic pid record becomes readable.
                for _ in 0..20 {
                    #[expect(clippy::disallowed_methods, reason = "no-timed-waits baseline: a test paces the signals it sends by a length of time")]
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    // SAFETY: the scope keeps the targeted thread alive.
                    assert_eq!(unsafe { libc::pthread_kill(thread, libc::SIGURG) }, 0);
                }
                writeln!(file, "12345").unwrap();
            });
            assert_eq!(entered(&reader), 12345);
            assert!(
                SIGNALS.load(Ordering::Relaxed) > 0,
                "signal reached the waiting thread"
            );
        });
    }
}
