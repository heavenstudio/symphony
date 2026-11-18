//! Repository discovery and lifecycle commands choose a scratch board's durable PostgreSQL
//! identity. No address override is involved, so these are the machine reader's selection path.

use crate::{repository, run, scratch};
use std::path::{Path, PathBuf};
use symphony_board::cli::Env;
use symphony_board::server::{Engine, Home, Postgres};
use symphony_board::{NewSession, NewTask, Store};

struct Board {
    root: PathBuf,
    home: Home,
    pg: Postgres,
    env: Env,
}
impl Board {
    fn new(name: &str) -> Self {
        let root = repository(&scratch(name));
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let programs = symphony_board::server::postgres::programs(&workspace)
            .unwrap()
            .unwrap();
        std::fs::write(
            root.join("symphony.toml"),
            format!(
                "[project]\nid = \"pg\"\n[board]\npostgresql = {:?}\n",
                programs.display().to_string()
            ),
        )
        .unwrap();
        let home = Home::in_common(&root.join(".git"), None);
        home.initialize_backend(Engine::PostgreSql).unwrap();
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

#[test]
fn malformed_identity_refuses_repository_discovery_instead_of_starting_surrealdb() {
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
