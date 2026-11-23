//! Repository discovery and lifecycle commands choose a scratch board's durable PostgreSQL
//! identity and make a board not made yet on PostgreSQL. No address override is involved, so these
//! are the machine reader's selection path.

use crate::children::Child as _;
use crate::{repository, run, scratch};
use std::path::{Path, PathBuf};
use symphony_board::cli::Env;
use symphony_board::server::{Engine, Home, Postgres};
use symphony_board::sys::{self, Command};
use symphony_board::{NewSession, NewTask, Store};

struct Board {
    root: PathBuf,
    home: Home,
    pg: Postgres,
    env: Env,
}
impl Board {
    fn new(name: &str) -> Self {
        let board = Board::unmade(name);
        board.home.initialize_backend(Engine::PostgreSql).unwrap();
        board
    }
    /// A repository whose board is not made yet, its project file naming the machine's
    /// PostgreSQL.
    fn unmade(name: &str) -> Self {
        let root = repository(&scratch(name));
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let programs = symphony_board::server::postgres::discover_programs(&workspace).unwrap();
        std::fs::write(
            root.join("symphony.toml"),
            format!(
                "[project]\nid = \"pg\"\n[board]\npostgresql = {:?}\n",
                programs.display().to_string()
            ),
        )
        .unwrap();
        let home = Home::in_common(&root.join(".git"));
        let pg = home.postgresql(&programs);
        let env = Env {
            cwd: root.clone(),
            vars: [
                ("TASKS_SWEEP".into(), "off".into()),
                ("TASKS_DISPATCH".into(), "inline".into()),
            ]
            .into(),
        };
        Self {
            root,
            home,
            pg,
            env,
        }
    }
    fn store(&self) -> Store {
        symphony_board::cli::connect(&self.env).unwrap()
    }
}
impl Drop for Board {
    fn drop(&mut self) {
        if let Some(hold) = self.pg.held() {
            let _ = self.pg.release(&hold.by, None);
        }
        let _ = self.pg.stop();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_board_not_made_yet_is_made_on_postgresql_by_its_first_command_and_serves() {
    let board = Board::unmade("pg-made-by-a-command");
    let dir = board.home.dir().to_path_buf();
    assert!(board.home.unmade().unwrap());
    assert_eq!(board.home.backend().unwrap(), Engine::PostgreSql);
    // The Symphony's read starts nothing and makes nothing: the project has no board.
    let read = symphony_board::cli::board(&board.env).err().unwrap();
    assert!(symphony_board::board_health::absent(&read), "{read}");
    assert!(read.contains("on PostgreSQL"), "{read}");
    // Nor does a reach that starts no server.
    assert!(symphony_board::cli::running_board(&board.env).is_err());
    assert!(board.home.unmade().unwrap());
    // Its first command makes it: the identity first, then the cluster, and no SurrealDB store.
    let listed = run(&board.env, &["list"], "");
    assert_eq!(listed.code, 0, "{}", listed.err);
    let identity: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("backend.json")).unwrap()).unwrap();
    assert_eq!(
        identity,
        serde_json::json!({"format": 1, "engine": "postgresql"})
    );
    assert!(dir.join("postgresql/PG_VERSION").is_file());
    assert!(!dir.join("data").exists());
    assert!(!board.home.unmade().unwrap());
    assert_eq!(board.home.backend().unwrap(), Engine::PostgreSql);
    // It serves: what one command writes, the next reads, and so does the Symphony.
    board
        .store()
        .create(&NewTask::new("pg/first", "Made on PostgreSQL", "owner"))
        .unwrap();
    assert!(run(&board.env, &["list"], "").out.contains("pg/first"));
    let read = symphony_board::cli::board(&board.env).unwrap();
    assert!(
        read.client.addr().starts_with("postgresql:"),
        "{}",
        read.client.addr()
    );
    assert!(board.pg.current().unwrap().is_some());
    board.home.stop_board().unwrap();
    assert!(board.pg.current().unwrap().is_none());
}

#[test]
fn a_board_not_made_yet_on_a_machine_without_postgresql_is_refused_with_its_setup() {
    let board = Board::unmade("pg-not-installed");
    let empty = scratch("pg-not-installed-programs");
    std::fs::write(
        board.root.join("symphony.toml"),
        format!(
            "[project]\nid = \"pg\"\n[board]\npostgresql = {:?}\n",
            empty.display().to_string()
        ),
    )
    .unwrap();
    let ran = run(&board.env, &["list"], "");
    assert_ne!(ran.code, 0);
    let said = ran.err.trim();
    assert!(
        said.contains("is not made yet and is made on PostgreSQL"),
        "{said}"
    );
    assert!(said.contains("install PostgreSQL 18 or later"), "{said}");
    assert!(said.contains("brew install postgresql@18"), "{said}");
    assert_eq!(said.lines().count(), 1, "{said}");
    // Never a SurrealDB board in its place: nothing was made.
    assert!(board.home.unmade().unwrap());
    for made in ["backend.json", "postgresql", "data", "server.json"] {
        assert!(!board.home.dir().join(made).exists(), "{made}: {said}");
    }
}

#[test]
fn repository_commands_hold_snapshot_release_and_reclaim_the_selected_postgresql_board() {
    let board = Board::new("pg-lifecycle-cli");
    board
        .store()
        .create(&NewTask::new("pg/one", "Kept by lifecycle", "owner"))
        .unwrap();
    assert!(run(&board.env, &["list"], "").out.contains("pg/one"));
    assert_eq!(run(&board.env, &["schema"], "").code, 0);
    assert_eq!(run(&board.env, &["migrate"], "").code, 0);
    let store = board.store();
    let handle = store
        .register(&NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    store.bind_agent("pg-lifecycle", &handle, false).unwrap();
    let before = store.marker().unwrap();
    let event = serde_json::json!({"agent_id":"pg-lifecycle", "hook_event_name":"PreToolUse", "tool_name":"Bash", "tool_use_id":"first", "cwd":board.root});
    assert_eq!(run(&board.env, &["beat"], &event.to_string()).code, 0);
    assert!(
        store.marker().unwrap() > before,
        "a repository heartbeat actually reached PostgreSQL"
    );
    drop(store);
    let held = run(
        &board.env,
        &["database", "hold", "snapshot", "--as", "worker"],
        "",
    );
    assert_eq!(held.code, 0, "{}", held.err);
    assert!(held.out.contains("PostgreSQL"));
    assert!(board.pg.current().unwrap().is_none());
    assert_ne!(run(&board.env, &["list"], "").code, 0);
    let taken = run(&board.env, &["snapshot"], "");
    assert_eq!(taken.code, 0, "{}", taken.err);
    assert!(taken.out.contains("PostgreSQL snapshot"));
    assert!(
        run(&board.env, &["snapshot", "--list"], "")
            .out
            .contains("postgresql-snapshots")
    );
    assert_ne!(
        run(&board.env, &["database", "release", "--as", "other"], "").code,
        0
    );
    assert_eq!(
        run(&board.env, &["database", "release", "--as", "worker"], "").code,
        0
    );
    assert_eq!(run(&board.env, &["list"], "").code, 0);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        symphony_board::reclaim_main(&[], &board.env, &mut out, &mut err),
        0,
        "{}",
        String::from_utf8_lossy(&err)
    );
    assert!(String::from_utf8(out).unwrap().contains("VACUUM FULL"));
    assert_eq!(
        board.store().get("pg/one").unwrap().title,
        "Kept by lifecycle"
    );
    assert!(!board.home.dir().join("data").exists());
}

/// `program ARGS` as a process of its own in `board`'s repository, which reaches the board by
/// discovery alone: its exit code, what it said and what it refused.
fn process(board: &Board, program: &str, args: &[&str]) -> (Option<i32>, String, String) {
    let mut command = Command::of(program);
    command.args(args).current_dir(&board.root);
    for name in [
        "TASKS_SERVER",
        "TASKS_DATABASE",
        "TASKS_SESSION",
        "TASKS_TASK",
    ] {
        command.env_remove(name);
    }
    for (name, value) in &board.env.vars {
        command.env(name, value);
    }
    let ran = sys::output(&mut command).unwrap();
    (
        ran.status.code(),
        String::from_utf8(ran.stdout).unwrap(),
        String::from_utf8(ran.stderr).unwrap(),
    )
}

/// A scratch board's PostgreSQL ends with the command that started it, so each command of a run
/// meets the server of the one before on its way out. Every one answers, a write, reads and the
/// whole-board export alike, and the export is one document however often it is read.
#[test]
fn commands_back_to_back_on_a_scratch_postgresql_board_each_answer() {
    const TASKS: &str = env!("CARGO_BIN_EXE_tasks");
    const COPY: &str = env!("CARGO_BIN_EXE_tasks-copy");
    let board = Board::new("pg-lifecycle-back-to-back");
    let (code, out, err) = process(&board, TASKS, &["register", "--kind", "worker"]);
    assert_eq!(code, Some(0), "{out}{err}");
    let mut exports = Vec::new();
    for round in 0..6 {
        let (code, out, err) = process(&board, TASKS, &["list"]);
        assert_eq!(code, Some(0), "list {round}: {out}{err}");
        let (code, out, err) = process(&board, TASKS, &["schema"]);
        assert_eq!(code, Some(0), "schema {round}: {out}{err}");
        let (code, out, err) = process(&board, COPY, &["--board", "--out"]);
        assert_eq!(code, Some(0), "export {round}: {err}");
        exports.push(out);
    }
    let dump = symphony_board::rows::Dump::from_json(&exports[0]).unwrap();
    assert_eq!(dump.sessions.len(), 1, "the session the first command made");
    assert!(exports.iter().all(|export| export == &exports[0]));
}

#[test]
fn malformed_identity_refuses_repository_discovery_and_starts_nothing() {
    let board = Board::new("pg-lifecycle-identity");
    let identity = board.home.dir().join("backend.json");
    let original = std::fs::read(&identity).unwrap();
    std::fs::write(&identity, "unknown").unwrap();
    let refused = run(&board.env, &["list"], "");
    std::fs::write(&identity, original).unwrap();
    assert_ne!(refused.code, 0);
    assert!(refused.err.contains("backend identity"), "{}", refused.err);
    assert!(board.pg.current().unwrap().is_none());
    assert!(!board.home.dir().join("data").exists());
}

#[test]
fn steady_repository_schema_migrate_and_list_use_one_connection_each() {
    let board = Board::new("pg-lifecycle-connections");
    board
        .store()
        .create(&NewTask::new("pg/one", "One connection", "owner"))
        .unwrap();
    board.pg.stop().unwrap();
    std::fs::write(
        board.pg.data().join("postgresql.auto.conf"),
        "log_connections = 'authorization'\n",
    )
    .unwrap();
    board.pg.ensure().unwrap();
    let log = board.pg.log();
    let before = std::fs::metadata(&log).unwrap().len() as usize;
    for command in ["schema", "migrate", "list"] {
        let ran = run(&board.env, &[command], "");
        assert_eq!(ran.code, 0, "{command}: {}", ran.err);
    }
    // A final log event follows every command's authorization event in the server's output
    // pipe. Waiting for it avoids racing the supervisor that appends that pipe to the log.
    let client = symphony_board::pg::Client::new(&board.pg.socket(), "tasks");
    client
        .simple("DO $$ BEGIN RAISE LOG 'pg-lifecycle-connections-ended'; END $$")
        .unwrap();
    let mut output = String::new();
    test_time::until_true(
        "the final PostgreSQL connection-count event is logged",
        test_time::FLOOR,
        || {
            output = std::fs::read_to_string(&log).unwrap()[before..].to_string();
            output.contains("pg-lifecycle-connections-ended")
        },
    )
    .unwrap();
    let count = output
        .lines()
        .filter(|line| {
            line.contains("connection authorized:")
                && line.contains("database=tasks")
                && line.contains("application_name=symphony-board")
        })
        .count();
    assert_eq!(
        count, 4,
        "three commands and the final observer each connect once: {output}"
    );
}
