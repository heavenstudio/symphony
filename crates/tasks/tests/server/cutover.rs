//! The move in the live board's own shape: the SQLite board and the board's SurrealDB server (started by
//! the move, on disk) in a repository's git dir, reached from the repository as every agent reaches
//! them, with no `TASKS_*` variable naming either.

use crate::common::{GuardedSurreal, Scratch, command_of, gone, signal};
use crate::sqlite;
use std::fs;
use std::io::{Read, Write};
use std::net::UdpSocket;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tasks::cli::Env;
use tasks::server::{FOLDER, Home, Running};
use tasks::sys::{Child, Command, Stdio};

/// A repository of its own holding the sample SQLite board in its git dir. The server a test started
/// there is stopped when it goes, by the pid its address file records, if that process serves it;
/// its pipe watchdog also stops it when the test process ends without running Drop.
struct Repo {
    dir: PathBuf,
    surreal: GuardedSurreal,
}

impl Repo {
    fn new(name: &str) -> Repo {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("cutover-home-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let done = tasks::sys::output(Command::new("git").args(["init", "-q"]).arg(&dir)).unwrap();
        assert!(done.status.success());
        let dir = dir.canonicalize().unwrap();
        sqlite::make(&dir.join(".git/star-foundry-tasks.db"), &sqlite::sample());
        let surreal = GuardedSurreal::new(&dir);
        Repo { dir, surreal }
    }

    fn common(&self) -> PathBuf {
        self.dir.join(".git")
    }

    fn env(&self) -> Env {
        let surreal = self.surreal.path().display().to_string();
        Env {
            vars: [("SF_SURREAL".to_string(), surreal)].into_iter().collect(),
            cwd: self.dir.clone(),
        }
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        self.surreal.shutdown();
        let home = self.common().join(FOLDER);
        let recorded = fs::read_to_string(home.join("server.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Running>(&text).ok());
        if let Some(running) = recorded {
            let data = format!("surrealkv://{}", home.join("data").display());
            if command_of(running.pid).is_some_and(|line| line.contains(&data)) {
                signal(running.pid, "KILL");
                gone(running.pid);
            }
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// `tasks ARGS` in the repository: its exit code, stdout and stderr.
fn tasks(env: &Env, args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = tasks::cli::main(&args, env, &mut &b""[..], &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn a_board_moved_in_a_repository_is_the_one_its_tasks_reach_with_its_keys_and_sweep() {
    let repo = Repo::new("moved");
    let env = repo.env();
    let board = repo.common().join("star-foundry-tasks.db");
    fs::write(
        repo.common().join("star-foundry-tasks.db.reap.json"),
        r#"{"idle": {}}"#,
    )
    .unwrap();
    let (code, _, err) = tasks(&env, &["list"]);
    assert_eq!(code, 2);
    assert!(err.contains("has not moved to SurrealDB yet"), "{err}");

    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = tasks::cutover::main(&[], &env, &mut out, &mut err);
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
    let code = tasks::cutover::main(&[], &env, &mut out, &mut err);
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
    let code = tasks::cutover::main(&[], &env, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// The repository's own SurrealDB board's server, started as the binary starts it, and recorded.
fn own_server(repo: &Repo) -> Running {
    Home::in_common(&repo.common(), Some(repo.surreal.path()))
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
        events
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
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
        let size = events
            .recv(&mut message)
            .expect("the fixture's server must become ready");
        let (dir, data, running): (PathBuf, String, Running) =
            serde_json::from_slice(&message[..size]).unwrap();
        assert!(
            tasks::rpc::healthy(&running.addr),
            "{route}: server is ready"
        );
        assert!(
            command_of(running.pid).unwrap().contains(&data),
            "{route}: server serves the fixture"
        );

        child.0.kill().unwrap();
        assert_eq!(child.0.wait().unwrap().signal(), Some(9));
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

#[test]
fn fixture_descendants_release_the_slot_while_the_server_is_running() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("fixture-slot-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let slot = dir.join("slot.lock");
    let events = UdpSocket::bind("127.0.0.1:0").unwrap();
    events
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
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
    let size = events
        .recv(&mut message)
        .expect("helper must hold the slot");
    assert_eq!(&message[..size], b"locked");
    assert!(
        !can_lock(),
        "the helper must own the lock before starting the fixture"
    );
    child.0.stdin.as_mut().unwrap().write_all(b"g").unwrap();

    let size = events
        .recv(&mut message)
        .expect("fixture server must become ready");
    let (_, _, running): (PathBuf, String, Running) =
        serde_json::from_slice(&message[..size]).unwrap();
    assert!(
        child.0.try_wait().unwrap().is_none(),
        "helper must remain alive"
    );
    assert!(
        tasks::rpc::healthy(&running.addr),
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
        events.send(b"locked").unwrap();
        let mut go = [0];
        std::io::stdin().read_exact(&mut go).unwrap();
        assert_eq!(&go, b"g");
        file
    });
    let hold = |dir: &Path, data: String, running: Running| {
        drop(slot);
        events
            .send(&serde_json::to_vec(&(dir, data, running)).unwrap())
            .unwrap();
        let mut byte = [0];
        assert_eq!(std::io::stdin().read(&mut byte).unwrap(), 0);
    };
    if route == "home" {
        let scratch = Scratch::new("killed home's fixture");
        let running = scratch.home().ensure().unwrap();
        hold(&scratch.dir, scratch.data(), running);
    } else {
        let repo = Repo::new("killed cutover's fixture");
        let running = if route == "cutover" {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            assert_eq!(
                tasks::cutover::main(&[], &repo.env(), &mut out, &mut err),
                0,
                "{}",
                String::from_utf8_lossy(&err)
            );
            Home::in_common(&repo.common(), Some(repo.surreal.path()))
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
    let board = repo.common().join("star-foundry-tasks.db");
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

/// A dry run on a frozen source copy cannot overwrite the live destination,
/// lose writes or change bound keys.
#[test]
fn a_copy_of_the_frozen_board_is_not_moved_over_the_live_one() {
    let repo = Repo::new("dry-run");
    let env = repo.env();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(tasks::cutover::main(&[], &env, &mut out, &mut err), 0);
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
    rusqlite::Connection::open(repo.common().join("star-foundry-tasks.db"))
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
    let (repo, other) = (Repo::new("here"), Repo::new("there"));
    let board = other.common().join("star-foundry-tasks.db");

    let (code, out, err) = cutover_of(&repo.env(), &board);
    assert_eq!((code, out.as_str()), (2, ""), "{err}");
    assert!(err.contains("is not this repository's board"), "{err}");
    assert!(err.contains(&board.display().to_string()), "{err}");
    assert!(!repo.common().join(FOLDER).exists(), "no server started");
    assert!(
        !other
            .common()
            .join("star-foundry-tasks.db.cutover")
            .exists()
    );
    assert!(!other.common().join("star-foundry-tasks.db.moved").exists());
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
        let repo = Repo::new(if hard { "linked-hard" } else { "linked" });
        let (board, link) = (
            repo.common().join("star-foundry-tasks.db"),
            repo.dir.join("board.db"),
        );
        if hard {
            fs::hard_link(&board, &link).unwrap();
        } else {
            std::os::unix::fs::symlink(&board, &link).unwrap();
        }

        let (code, _, err) = cutover_of(&repo.env(), &link);
        assert_eq!((code, err.as_str()), (0, ""), "hard: {hard}");
        assert!(repo.common().join("star-foundry-tasks.db.moved").is_file());
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
    let (here, there) = (Repo::new("server-here"), Repo::new("server-there"));
    let mut env = there.env();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(tasks::cutover::main(&[], &env, &mut out, &mut err), 0);
    let theirs: Running = serde_json::from_str(
        &fs::read_to_string(there.common().join(FOLDER).join("server.json")).unwrap(),
    )
    .unwrap();

    env = here.env();
    env.vars.insert("TASKS_SERVER".into(), theirs.addr);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = tasks::cutover::main(&[], &env, &mut out, &mut err);
    let (out, err) = (
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    );
    assert_eq!((code, out.as_str()), (2, ""), "{err}");
    assert!(err.contains("is a repository's own board"), "{err}");
    assert!(err.ends_with("unset TASKS_SERVER\n"), "{err}");
    left_alone(&here, &here.common().join("star-foundry-tasks.db"));
}

/// Resolve source ownership from the board path independently of the caller's
/// working directory before accepting a server override.
#[test]
fn a_repositorys_board_does_not_move_into_another_server_wherever_the_run_starts() {
    let (here, there) = (Repo::new("whence-here"), Repo::new("whence-there"));
    let theirs = own_server(&there);
    let board = here.common().join("star-foundry-tasks.db");
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
    let there = Repo::new("links-there");
    let theirs = own_server(&there);
    for hard in [false, true] {
        let repo = Repo::new(if hard { "links-hard" } else { "links-soft" });
        let (board, link) = (
            repo.common().join("star-foundry-tasks.db"),
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
    let repo = Repo::new("own-named");
    let own = own_server(&repo);
    fs::write(
        repo.common().join("star-foundry-tasks.db.reap.json"),
        r#"{"idle": {}}"#,
    )
    .unwrap();
    for database in [None, Some(tasks::DATABASE), Some("elsewhere")] {
        let mut vars = vec![("TASKS_SERVER", own.addr.as_str())];
        vars.extend(database.map(|name| ("TASKS_DATABASE", name)));
        let (code, out, err) = cutover_with(&repo.env(), &vars);
        assert_eq!((code, out.as_str()), (2, ""), "{database:?}: {err}");
        assert!(err.contains("is a repository's own board"), "{err}");
        left_alone(&repo, &repo.common().join("star-foundry-tasks.db"));
    }
    let (code, _, err) = cutover_with(&repo.env(), &[]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(names(&repo.common().join(FOLDER).join("keys")), ["agent-1"]);
    assert_eq!(
        fs::read_to_string(repo.common().join(FOLDER).join("reap.json")).unwrap(),
        r#"{"idle": {}}"#
    );
}
