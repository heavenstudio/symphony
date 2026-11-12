//! The move in the live board's own shape: the SQLite board and the board's SurrealDB server (started by
//! the move, on disk) in a repository's git dir, reached from the repository as every agent reaches
//! them, with no `TASKS_*` variable naming either.

use crate::common::{Repository as Repo, Scratch, command_of, gone, parent_of};
use crate::helper::{answer, event_of};
use crate::sqlite;
use std::fs;
use std::io::{Read, Write};
use std::net::UdpSocket;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use symphony_board::cli::Env;
use symphony_board::server::{FOLDER, Home, Running};
use symphony_board::sys::{Child, Command, Stdio};
use symphony_board::{cli, cutover, rpc};

/// The project file of every repository here: its id names its SQLite board, `lamp-works-tasks.db`.
const PROJECT: &str = "[project]\nid = \"lamp-works\"\n";

/// A repository of its own ([`Repo`]) holding the sample SQLite board in its git dir.
fn holding_sqlite(name: &str) -> Repo {
    let repo = Repo::new(&format!("cutover-home-{name}"));
    fs::write(repo.dir.join("symphony.toml"), PROJECT).unwrap();
    sqlite::make(
        &repo.common().join("lamp-works-tasks.db"),
        &sqlite::sample(),
    );
    repo
}

/// `tasks ARGS` in the repository: its exit code, stdout and stderr.
fn tasks(env: &Env, args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cli::main(&args, env, &mut &b""[..], &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn a_board_moved_in_a_repository_is_the_one_its_tasks_reach_with_its_keys_and_sweep() {
    let repo = holding_sqlite("moved");
    let env = repo.env();
    let board = repo.common().join("lamp-works-tasks.db");
    fs::write(
        repo.common().join("lamp-works-tasks.db.reap.json"),
        r#"{"idle": {}}"#,
    )
    .unwrap();
    let (code, _, err) = tasks(&env, &["list"]);
    assert_eq!(code, 2);
    assert!(err.contains("has not moved to SurrealDB yet"), "{err}");

    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cutover::main(&[], &env, &mut out, &mut err);
    let (out, err) = (
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    );
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert!(
        out.contains(&format!("{} refuses writes now", board.display())),
        "{out}"
    );

    let (code, out, err) = tasks(&env, &["list"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains("sim/jump/land"), "{out}");
    let keys: Vec<String> = fs::read_dir(repo.common().join(FOLDER).join("keys"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(keys, ["agent-1"]);
    assert_eq!(
        fs::read_to_string(repo.common().join(FOLDER).join("reap.json")).unwrap(),
        r#"{"idle": {}}"#
    );
}

/// `tasks-cutover` in the repository, with `TASKS_DB` naming `board`: its exit code, stdout and stderr.
fn cutover_of(env: &Env, board: &Path) -> (i32, String, String) {
    let mut env = env.clone();
    env.vars
        .insert("TASKS_DB".into(), board.display().to_string());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cutover::main(&[], &env, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// `tasks-cutover` with `vars` set besides `env`'s: its exit code, stdout and stderr.
fn cutover_with(env: &Env, vars: &[(&str, &str)]) -> (i32, String, String) {
    let mut env = env.clone();
    for (name, value) in vars {
        env.vars.insert(name.to_string(), value.to_string());
    }
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cutover::main(&[], &env, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// The repository's own SurrealDB board's server, started as the binary starts it, and recorded.
fn own_server(repo: &Repo) -> Running {
    Home::in_common(&repo.common(), Some(repo.surreal()))
        .ensure()
        .unwrap()
}

/// Own the helper before readiness is awaited, so an assertion/startup failure also closes its
/// lifetime pipe. SIGKILL deliberately bypasses every fixture's Drop in the child.
struct TestProcess(Child);

impl Drop for TestProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn servers_end_when_the_fixture_process_is_killed_without_dropping_it() {
    for route in ["cutover", "own-server", "home"] {
        let events = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut child = TestProcess(
            crate::store::spawn_with_input(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--ignored",
                        "--exact",
                        "cutover::helper_fixture_process",
                        "--test-threads=1",
                    ])
                    .env("SF_CUTOVER_FIXTURE_ROUTE", route)
                    .env(
                        "SF_CUTOVER_FIXTURE_EVENTS",
                        events.local_addr().unwrap().to_string(),
                    )
                    .stdout(Stdio::null())
                    .stderr(Stdio::inherit()),
            )
            .unwrap(),
        );
        let mut message = [0; 8192];
        let size = event_of(&events, &mut child.0, &mut message)
            .unwrap_or_else(|why| panic!("{route}: the fixture's server must become ready: {why}"));
        let (dir, data, running): (PathBuf, String, Running) =
            serde_json::from_slice(&message[..size]).unwrap();
        assert!(rpc::healthy(&running.addr), "{route}: server is ready");
        assert!(
            command_of(running.pid).unwrap().contains(&data),
            "{route}: server serves the fixture"
        );

        // Kept until the helper is reaped: `wait` closes a child's input before it waits, and on
        // macOS a killed process's thread blocked on that input can wake to its end and drop its
        // fixture, stopping the server and removing the folder, before the kill has ended it.
        let input = child.0.stdin.take();
        child.0.kill().unwrap();
        assert_eq!(child.0.wait().unwrap().signal(), Some(9));
        drop(input);
        // A condition/deadline wait, never an assumed grace sleep: no recorded server can still
        // serve the data once its process is gone. No process-table scan or pattern kill is needed.
        assert!(
            gone(running.pid),
            "{route}: server {} survived its fixture process",
            running.pid
        );
        assert!(
            command_of(running.pid).is_none(),
            "{route}: a server still serves {data}"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}

/// A scratch board's server (a fixture repository's, in the temporary folder) is the one path the
/// board starts a server on for a test, and nothing in the test guards it: it is supervised, its
/// parent a shell of the board's while it runs, and ends with the process that started it when
/// that is killed, its Drop never run.
#[test]
fn a_scratch_boards_server_ends_when_the_process_that_started_it_is_killed() {
    let events = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut child = TestProcess(
        crate::store::spawn_with_input(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "cutover::helper_fixture_process",
                    "--test-threads=1",
                ])
                .env("SF_CUTOVER_FIXTURE_ROUTE", "scratch")
                .env(
                    "SF_CUTOVER_FIXTURE_EVENTS",
                    events.local_addr().unwrap().to_string(),
                )
                .stdout(Stdio::null())
                .stderr(Stdio::inherit()),
        )
        .unwrap(),
    );
    let mut message = [0; 8192];
    let size = event_of(&events, &mut child.0, &mut message)
        .unwrap_or_else(|why| panic!("the scratch board's server must become ready: {why}"));
    let (dir, data, running): (PathBuf, String, Running) =
        serde_json::from_slice(&message[..size]).unwrap();
    assert!(rpc::healthy(&running.addr));
    assert!(command_of(running.pid).unwrap().contains(&data));
    let supervisor = parent_of(running.pid).expect("the server's parent");
    assert_ne!(
        supervisor, 1,
        "a scratch board's server is never left parentless"
    );
    assert_ne!(
        supervisor,
        i64::from(child.0.id()),
        "nor the starter's own child"
    );

    let input = child.0.stdin.take();
    child.0.kill().unwrap();
    assert_eq!(child.0.wait().unwrap().signal(), Some(9));
    drop(input);
    assert!(
        gone(running.pid),
        "server {} survived the process that started it",
        running.pid
    );
    assert!(gone(supervisor), "its supervisor {supervisor} survived it");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn fixture_descendants_release_the_slot_while_the_server_is_running() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("fixture-slot-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let slot = dir.join("slot.lock");
    let events = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut child = TestProcess(
        crate::store::spawn_with_input(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "cutover::helper_fixture_process",
                    "--test-threads=1",
                ])
                .env("SF_CUTOVER_FIXTURE_ROUTE", "home")
                .env(
                    "SF_CUTOVER_FIXTURE_EVENTS",
                    events.local_addr().unwrap().to_string(),
                )
                .env("SF_CUTOVER_FIXTURE_SLOT", &slot)
                .stdout(Stdio::null())
                .stderr(Stdio::inherit()),
        )
        .unwrap(),
    );
    let can_lock = || {
        fs::File::options()
            .read(true)
            .write(true)
            .open(&slot)
            .unwrap()
            .try_lock()
            .is_ok()
    };
    let mut message = [0; 8192];
    let size = event_of(&events, &mut child.0, &mut message)
        .unwrap_or_else(|why| panic!("helper must hold the slot: {why}"));
    assert_eq!(&message[..size], b"locked");
    assert!(
        !can_lock(),
        "the helper must own the lock before starting the fixture"
    );
    child.0.stdin.as_mut().unwrap().write_all(b"g").unwrap();

    let size = event_of(&events, &mut child.0, &mut message)
        .unwrap_or_else(|why| panic!("fixture server must become ready: {why}"));
    let (_, _, running): (PathBuf, String, Running) =
        serde_json::from_slice(&message[..size]).unwrap();
    assert!(
        child.0.try_wait().unwrap().is_none(),
        "helper must remain alive"
    );
    assert!(
        rpc::healthy(&running.addr),
        "fixture server must remain alive"
    );
    assert!(
        can_lock(),
        "the running server or watchdog retained the fixture slot"
    );

    drop(child.0.stdin.take());
    assert!(child.0.wait().unwrap().success());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
#[ignore = "owned helper process for fixture lifecycle regressions"]
fn helper_fixture_process() {
    let route = std::env::var("SF_CUTOVER_FIXTURE_ROUTE").unwrap();
    let events = UdpSocket::bind("127.0.0.1:0").unwrap();
    events
        .connect(std::env::var("SF_CUTOVER_FIXTURE_EVENTS").unwrap())
        .unwrap();
    let slot = std::env::var_os("SF_CUTOVER_FIXTURE_SLOT").map(|path| {
        use std::os::fd::AsRawFd;
        unsafe extern "C" {
            fn fcntl(fd: std::ffi::c_int, command: std::ffi::c_int, ...) -> std::ffi::c_int;
        }
        let file = fs::File::create(path).unwrap();
        file.lock().unwrap();
        // Only this helper test runs in this process. Its fixture descendants must inherit the
        // slot, as real fixture descendants do; the outer test never owns a copy of its lock.
        // SAFETY: F_SETFD is 2 on both supported systems, and this process owns the descriptor.
        assert_eq!(unsafe { fcntl(file.as_raw_fd(), 2, 0) }, 0);
        answer(&events, b"locked");
        let mut go = [0];
        std::io::stdin().read_exact(&mut go).unwrap();
        assert_eq!(&go, b"g");
        file
    });
    let hold = |dir: &Path, data: String, running: Running| {
        drop(slot);
        answer(&events, &serde_json::to_vec(&(dir, data, running)).unwrap());
        let mut byte = [0];
        // The input ends when the test lets this helper go, or once it has reaped it killed.
        assert_eq!(std::io::stdin().read(&mut byte).unwrap(), 0);
    };
    if route == "home" {
        let scratch = Scratch::new("killed home's fixture");
        let running = scratch.home().ensure().unwrap();
        hold(&scratch.dir, scratch.data(), running);
    } else if route == "scratch" {
        // A board in the temporary folder, as a fixture repository's is, served by the pinned
        // `surreal` itself: no guard of the test's ends it, only the board's own supervision.
        let dir = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("scratch-board-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".git")).unwrap();
        let home = Home::in_common(&dir.join(".git"), Some(&crate::common::surreal()));
        assert!(symphony_board::server::scratch(home.dir()));
        let running = home.ensure().unwrap();
        let data = format!("surrealkv://{}", home.dir().join("data").display());
        hold(&dir, data, running);
    } else {
        let repo = holding_sqlite("killed cutover's fixture");
        let running = if route == "cutover" {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            assert_eq!(
                cutover::main(&[], &repo.env(), &mut out, &mut err),
                0,
                "{}",
                String::from_utf8_lossy(&err)
            );
            Home::in_common(&repo.common(), Some(repo.surreal()))
                .recorded()
                .unwrap()
        } else {
            assert_eq!(route, "own-server");
            own_server(&repo)
        };
        let data = format!(
            "surrealkv://{}",
            repo.common().join(FOLDER).join("data").display()
        );
        hold(&repo.dir, data, running);
    }
}

/// That a refused move left the repository's board as it found it: nothing locked or marked beside it,
/// or beside `named`, the path the run was given; and it takes writes, never frozen.
fn left_alone(repo: &Repo, named: &Path) {
    let board = repo.common().join("lamp-works-tasks.db");
    for path in [&board, named] {
        for end in [".cutover", ".moved"] {
            let mut beside = path.as_os_str().to_owned();
            beside.push(end);
            let beside = PathBuf::from(beside);
            assert!(!beside.exists(), "{}", beside.display());
        }
    }
    rusqlite::Connection::open(&board)
        .unwrap()
        .execute_batch("UPDATE tasks SET title = 'Hop' WHERE slug = 'sim/jump'")
        .expect("the repository's board takes writes, never frozen");
}

fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// While the board's database is held, the move is refused naming the hold before anything is
/// frozen, and no server starts: the SQLite board stays as it was, taking writes.
#[test]
fn a_held_board_refuses_the_move_and_starts_no_server() {
    let repo = holding_sqlite("held");
    let env = repo.env();
    let home = Home::in_common(&repo.common(), Some(repo.surreal()));
    let hold = home.hold("russet-lynx", "a swap").unwrap().hold;
    let (code, out, err) = cutover_with(&env, &[]);
    assert_eq!((code, out.as_str()), (2, ""), "{err}");
    assert!(
        err.contains(&format!(
            "the board's database is {hold}; nothing starts it until its holder releases it"
        )),
        "{err}"
    );
    assert!(
        err.contains("the board stays in SQLite, never frozen"),
        "{err}"
    );
    // Refused under the move's own lock, the board neither marked moved nor frozen.
    let board = repo.common().join("lamp-works-tasks.db");
    assert!(!repo.common().join("lamp-works-tasks.db.moved").exists());
    rusqlite::Connection::open(&board)
        .unwrap()
        .execute_batch("UPDATE tasks SET title = 'Hop' WHERE slug = 'sim/jump'")
        .expect("the board takes writes, never frozen");
    assert_eq!(home.recorded(), None, "the refused move started the board");
    assert_eq!(home.serving(), None);
    assert!(home.release("russet-lynx", None).unwrap().is_some());
}

/// A dry run on a frozen source copy cannot overwrite the live destination,
/// lose writes or change bound keys.
#[test]
fn a_copy_of_the_frozen_board_is_not_moved_over_the_live_one() {
    let repo = holding_sqlite("dry-run");
    let env = repo.env();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(cutover::main(&[], &env, &mut out, &mut err), 0);
    let (code, _, err) = tasks(
        &env,
        &[
            "create",
            "sim/after",
            "After the move",
            "--as",
            "cobalt-otter",
        ],
    );
    assert_eq!((code, err.as_str()), (0, ""));
    let home = repo.common().join(FOLDER);
    fs::write(home.join("keys").join("agent-2"), "").unwrap();
    fs::write(home.join("reap.json"), r#"{"idle": {"since": 1}}"#).unwrap();
    let copy = repo.dir.join("copy.db");
    rusqlite::Connection::open(repo.common().join("lamp-works-tasks.db"))
        .unwrap()
        .execute("VACUUM INTO ?1", [copy.display().to_string()])
        .unwrap();
    fs::write(
        repo.dir.join("copy.db.reap.json"),
        r#"{"idle": {"copy": 1}}"#,
    )
    .unwrap();

    let (code, out, err) = cutover_of(&env, &copy);
    assert_eq!((code, out.as_str()), (2, ""), "{err}");
    assert!(err.contains("is not this repository's board"), "{err}");
    let (code, out, _) = tasks(&env, &["list"]);
    assert_eq!(code, 0);
    assert!(out.contains("sim/after"), "{out}");
    assert_eq!(names(&home.join("keys")), ["agent-1", "agent-2"]);
    assert_eq!(
        fs::read_to_string(home.join("reap.json")).unwrap(),
        r#"{"idle": {"since": 1}}"#
    );
    assert!(!repo.dir.join("copy.db.moved").exists());
}

/// fleet-vole (low, b4ae5745): `TASKS_DB` naming another repository's board moved it into this
/// repository's SurrealDB board and marked it moved there, leaving that repository with a frozen board
/// and nothing to take its writes. Only this repository's board moves into its SurrealDB board (a board
/// of anywhere else needs `TASKS_SERVER`, a server of its own): the other board is refused before it is
/// locked, frozen or marked, and no server starts here.
#[test]
fn a_board_of_another_repository_is_not_moved_into_this_ones() {
    let (repo, other) = (holding_sqlite("here"), holding_sqlite("there"));
    let board = other.common().join("lamp-works-tasks.db");

    let (code, out, err) = cutover_of(&repo.env(), &board);
    assert_eq!((code, out.as_str()), (2, ""), "{err}");
    assert!(err.contains("is not this repository's board"), "{err}");
    assert!(err.contains(&board.display().to_string()), "{err}");
    assert!(!repo.common().join(FOLDER).exists(), "no server started");
    assert!(!other.common().join("lamp-works-tasks.db.cutover").exists());
    assert!(!other.common().join("lamp-works-tasks.db.moved").exists());
    rusqlite::Connection::open(&board)
        .unwrap()
        .execute_batch("UPDATE tasks SET title = 'Hop' WHERE slug = 'sim/jump'")
        .expect("the other repository's board takes writes, never frozen");
}

/// The repository's board by another path (a link to it, or another name of its file: fleet-vole, low A
/// at 603bb06b, the same board by its device and inode) is its own: it moves, and its lock and marker
/// are the ones in the git dir, which is where the binary looks.
#[test]
fn the_repositorys_board_by_another_path_moves_and_is_marked_in_its_git_dir() {
    for hard in [false, true] {
        let repo = holding_sqlite(if hard { "linked-hard" } else { "linked" });
        let (board, link) = (
            repo.common().join("lamp-works-tasks.db"),
            repo.dir.join("board.db"),
        );
        if hard {
            fs::hard_link(&board, &link).unwrap();
        } else {
            std::os::unix::fs::symlink(&board, &link).unwrap();
        }

        let (code, _, err) = cutover_of(&repo.env(), &link);
        assert_eq!((code, err.as_str()), (0, ""), "hard: {hard}");
        assert!(repo.common().join("lamp-works-tasks.db.moved").is_file());
        assert!(!repo.dir.join("board.db.moved").exists());
        assert!(!repo.dir.join("board.db.cutover").exists());
        let (code, out, _) = tasks(&repo.env(), &["list"]);
        assert_eq!(code, 0);
        assert!(out.contains("sim/jump/land"), "{out}");
    }
}

/// Refuse a repository source with TASKS_SERVER naming another repository's
/// server, before freezing or writing a marker.
#[test]
fn the_repositorys_board_does_not_move_into_another_repositorys_server() {
    let (here, there) = (
        holding_sqlite("server-here"),
        holding_sqlite("server-there"),
    );
    let mut env = there.env();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(cutover::main(&[], &env, &mut out, &mut err), 0);
    let theirs: Running = serde_json::from_str(
        &fs::read_to_string(there.common().join(FOLDER).join("server.json")).unwrap(),
    )
    .unwrap();

    env = here.env();
    env.vars.insert("TASKS_SERVER".into(), theirs.addr);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cutover::main(&[], &env, &mut out, &mut err);
    let (out, err) = (
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    );
    assert_eq!((code, out.as_str()), (2, ""), "{err}");
    assert!(err.contains("is a repository's own board"), "{err}");
    assert!(err.ends_with("unset TASKS_SERVER\n"), "{err}");
    left_alone(&here, &here.common().join("lamp-works-tasks.db"));
}

/// Resolve source ownership from the board path independently of the caller's
/// working directory before accepting a server override.
#[test]
fn a_repositorys_board_does_not_move_into_another_server_wherever_the_run_starts() {
    let (here, there) = (
        holding_sqlite("whence-here"),
        holding_sqlite("whence-there"),
    );
    let theirs = own_server(&there);
    let board = here.common().join("lamp-works-tasks.db");
    for cwd in [PathBuf::from("/"), there.dir.clone()] {
        let env = Env {
            cwd: cwd.clone(),
            ..here.env()
        };
        let (code, out, err) = cutover_with(
            &env,
            &[
                ("TASKS_DB", &board.display().to_string()),
                ("TASKS_SERVER", &theirs.addr),
            ],
        );
        assert_eq!((code, out.as_str()), (2, ""), "{}: {err}", cwd.display());
        assert!(err.contains("is a repository's own board"), "{err}");
        left_alone(&here, &board);
    }
}

/// Recognize repository boards through symlinks and hard links from any cwd.
#[test]
fn the_repositorys_board_by_a_link_does_not_move_into_another_server() {
    let there = holding_sqlite("links-there");
    let theirs = own_server(&there);
    for hard in [false, true] {
        let repo = holding_sqlite(if hard { "links-hard" } else { "links-soft" });
        let (board, link) = (
            repo.common().join("lamp-works-tasks.db"),
            repo.dir.join("link.db"),
        );
        if hard {
            fs::hard_link(&board, &link).unwrap();
        } else {
            std::os::unix::fs::symlink(&board, &link).unwrap();
        }
        for cwd in [repo.dir.clone(), PathBuf::from("/")] {
            let env = Env {
                cwd: cwd.clone(),
                ..repo.env()
            };
            let (code, out, err) = cutover_with(
                &env,
                &[
                    ("TASKS_DB", &link.display().to_string()),
                    ("TASKS_SERVER", &theirs.addr),
                ],
            );
            let at = format!("{} from {}", link.display(), cwd.display());
            assert_eq!((code, out.as_str()), (2, ""), "{at}: {err}");
            assert!(err.contains(&link.display().to_string()), "{at}: {err}");
            left_alone(&repo, &link);
        }
    }
}

/// Repository boards refuse every TASKS_SERVER override. Moving without an
/// override preserves bound keys and sweep state with the board.
#[test]
fn the_repositorys_board_does_not_move_with_its_own_server_named() {
    let repo = holding_sqlite("own-named");
    let own = own_server(&repo);
    fs::write(
        repo.common().join("lamp-works-tasks.db.reap.json"),
        r#"{"idle": {}}"#,
    )
    .unwrap();
    for database in [None, Some(symphony_board::DATABASE), Some("elsewhere")] {
        let mut vars = vec![("TASKS_SERVER", own.addr.as_str())];
        vars.extend(database.map(|name| ("TASKS_DATABASE", name)));
        let (code, out, err) = cutover_with(&repo.env(), &vars);
        assert_eq!((code, out.as_str()), (2, ""), "{database:?}: {err}");
        assert!(err.contains("is a repository's own board"), "{err}");
        left_alone(&repo, &repo.common().join("lamp-works-tasks.db"));
    }
    let (code, _, err) = cutover_with(&repo.env(), &[]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(names(&repo.common().join(FOLDER).join("keys")), ["agent-1"]);
    assert_eq!(
        fs::read_to_string(repo.common().join(FOLDER).join("reap.json")).unwrap(),
        r#"{"idle": {}}"#
    );
}

/// A move into the repository's own SurrealDB board when the record beside it says the board was
/// seen at a schema it no longer reads (here: a store that lost the board it held) is refused in the
/// rollback's words before anything is frozen or copied: the move would write into a store that went
/// back.
#[test]
fn a_move_into_a_board_below_its_record_is_refused_and_the_sqlite_board_never_frozen() {
    use symphony_board::live::seen;
    let repo = holding_sqlite("went-back");
    let env = repo.env();
    let board = repo.common().join("lamp-works-tasks.db");
    let this = symphony_board::Schema::this_build().version();
    let record = seen::path(&repo.common());
    fs::create_dir_all(record.parent().unwrap()).unwrap();
    fs::write(&record, format!("{this} 1800000000 seen x\n")).unwrap();
    let why = seen::Rollback {
        project: "lamp-works".into(),
        from: this,
        since: 1_800_000_000.0,
        to: None,
        record,
    }
    .refusal();
    let (code, out, err) = cutover_of(&env, &board);
    assert_eq!(
        (code, out.as_str(), err),
        (
            2,
            "",
            format!("❌ tasks-cutover: {why}; the board stays in SQLite, never frozen\n")
        )
    );
    // Not frozen, not marked moved: the board's commands still say it has not moved.
    let (code, _, err) = tasks(&env, &["list"]);
    assert_eq!(code, 2);
    assert!(err.contains("has not moved to SurrealDB yet"), "{err}");
    let running = own_server(&repo);
    let store = symphony_board::Store::unchecked(
        rpc::Client::new(&running.addr, "sf", "tasks"),
        Box::new(|| 0.0),
        Box::new(|_| true),
        "",
    );
    assert_eq!(
        store.board_version().unwrap(),
        None,
        "nothing made or copied"
    );
}
