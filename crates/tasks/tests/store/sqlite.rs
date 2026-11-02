//! The SQLite board as the Python store (`tools/tasks_store.py`) left it when the board moved to
//! SurrealDB, for the move's tests to make one: its schema, a sample board, and a board made from them
//! (`tests/store/cutover.rs`, and the move in a repository of its own in `tests/server/`).

use rusqlite::Connection;
use rusqlite::types::Value as Sql;
use serde_json::{Map, Value};
use std::path::Path;
use tasks::rows::{
    BarredKeyRow, DeliveryRow, Dump, LinkRow, MessageRow, RoleRow, SessionRow, TaskRow,
    TaskSessionRow,
};

/// A board at `path` in WAL mode, as the store kept it, holding `rows`.
pub fn make(path: &Path, rows: &Dump) {
    let db = Connection::open(path).unwrap();
    let mode: String = db
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    db.execute_batch(SCHEMA).unwrap();
    let tables = serde_json::to_value(rows).unwrap();
    for table in TABLES {
        for row in tables[table].as_array().unwrap() {
            insert(&db, table, row.as_object().unwrap());
        }
    }
}

/// The SQLite board's schema as the Python store left it.
const SCHEMA: &str = "
CREATE TABLE tasks (
    id INTEGER PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 2,
    parent TEXT,
    created_by TEXT NOT NULL,
    created REAL NOT NULL,
    updated REAL NOT NULL,
    instruction INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE task_links (
    slug TEXT NOT NULL,
    kind TEXT NOT NULL,
    target TEXT NOT NULL,
    PRIMARY KEY (slug, kind, target)
);
CREATE TABLE sessions (
    handle TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    parent TEXT,
    worktree TEXT NOT NULL DEFAULT '',
    branch TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    pid INTEGER,
    host TEXT NOT NULL DEFAULT '',
    affinity TEXT NOT NULL DEFAULT '',
    connected REAL NOT NULL,
    last_seen REAL NOT NULL,
    left_at REAL,
    agent TEXT,
    busy_until REAL,
    busy_call TEXT,
    last_tool REAL
);
CREATE TABLE roles (
    slug TEXT NOT NULL,
    handle TEXT NOT NULL,
    role TEXT NOT NULL,
    since REAL NOT NULL,
    expires REAL,
    PRIMARY KEY (slug, handle, role)
);
CREATE TABLE messages (
    id INTEGER PRIMARY KEY,
    slug TEXT NOT NULL,
    sender TEXT NOT NULL,
    recipient TEXT,
    kind TEXT NOT NULL,
    text TEXT NOT NULL,
    ts REAL NOT NULL
);
CREATE TABLE deliveries (
    message INTEGER NOT NULL,
    handle TEXT NOT NULL,
    acked REAL NOT NULL,
    PRIMARY KEY (message, handle)
);
CREATE TABLE barred_keys (
    key TEXT PRIMARY KEY
);
CREATE TABLE task_sessions (
    slug TEXT NOT NULL,
    handle TEXT NOT NULL,
    role TEXT NOT NULL,
    claude_session TEXT NOT NULL,
    worktree TEXT NOT NULL,
    branch TEXT NOT NULL DEFAULT '',
    pane TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    started REAL NOT NULL,
    closed REAL,
    closed_why TEXT NOT NULL DEFAULT '',
    restore TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (slug, handle)
);
CREATE INDEX messages_by_slug ON messages (slug, id);
";

pub const TABLES: [&str; 8] = [
    "tasks",
    "task_links",
    "sessions",
    "roles",
    "messages",
    "deliveries",
    "barred_keys",
    "task_sessions",
];

const T: f64 = 1_800_000_000.0;

/// A board for the tests: every table, every column, each optional column both empty and filled, text
/// SQLite and SurrealDB must carry byte for byte, and times with fractions a float must keep. In the
/// order of each table's key, as the SurrealDB board reads back.
pub fn sample() -> Dump {
    Dump {
        tasks: vec![
            TaskRow {
                id: 1,
                slug: "sim/jump".into(),
                title: "Jump".into(),
                body: "the jump drive\nnaïve — ✓ 'quoted' \"too\"".into(),
                status: "in_progress".into(),
                priority: 1,
                parent: None,
                created_by: "owner".into(),
                created: T + 0.25,
                updated: T + 100.5,
                instruction: false,
            },
            TaskRow {
                id: 3,
                slug: "sim/jump/land".into(),
                title: "Land".into(),
                body: String::new(),
                status: "ready".into(),
                priority: 2,
                parent: Some("sim/jump".into()),
                created_by: "cobalt-otter".into(),
                created: T + 200.125,
                updated: T + 200.125,
                instruction: true,
            },
        ],
        task_links: vec![
            LinkRow {
                slug: "sim/jump".into(),
                kind: "doc".into(),
                target: "docs/game/SPEC.md".into(),
            },
            LinkRow {
                slug: "sim/jump/land".into(),
                kind: "dep".into(),
                target: "sim/jump".into(),
            },
        ],
        sessions: vec![
            SessionRow {
                handle: "cobalt-otter".into(),
                kind: "orchestrator".into(),
                parent: None,
                worktree: "/wt/a".into(),
                branch: "main".into(),
                model: "opus".into(),
                pid: Some(4242),
                host: "mac".into(),
                affinity: "sim".into(),
                connected: T,
                last_seen: T + 5.5,
                left_at: None,
                agent: Some("agent-1".into()),
                busy_until: Some(T + 60.0),
                busy_call: Some("Bash".into()),
                last_tool: Some(T + 4.0),
            },
            SessionRow {
                handle: "ivory-tern".into(),
                kind: "worker".into(),
                parent: Some("cobalt-otter".into()),
                worktree: String::new(),
                branch: String::new(),
                model: String::new(),
                pid: None,
                host: String::new(),
                affinity: String::new(),
                connected: T + 1.0,
                last_seen: T + 2.0,
                left_at: Some(T + 3.0),
                agent: None,
                busy_until: None,
                busy_call: None,
                last_tool: None,
            },
        ],
        roles: vec![
            RoleRow {
                slug: "sim/jump".into(),
                handle: "ivory-tern".into(),
                role: "executor".into(),
                since: T + 10.0,
                expires: None,
            },
            RoleRow {
                slug: "sim/jump/land".into(),
                handle: "cobalt-otter".into(),
                role: "reserved".into(),
                since: T + 11.0,
                expires: Some(T + 7211.0),
            },
        ],
        messages: vec![
            MessageRow {
                id: 7,
                slug: "sim/jump".into(),
                sender: "cobalt-otter".into(),
                recipient: Some("ivory-tern".into()),
                kind: "msg".into(),
                text: "go — now".into(),
                ts: T + 20.0,
            },
            MessageRow {
                id: 9,
                slug: "sim/jump".into(),
                sender: "ivory-tern".into(),
                recipient: None,
                kind: "system".into(),
                text: "done\n✓".into(),
                ts: T + 30.75,
            },
        ],
        deliveries: vec![DeliveryRow {
            message: 7,
            handle: "ivory-tern".into(),
            acked: T + 21.0,
        }],
        barred_keys: vec![BarredKeyRow {
            key: "key-1".into(),
        }],
        task_sessions: vec![
            TaskSessionRow {
                slug: "sim/jump".into(),
                handle: "ivory-tern".into(),
                role: "executor".into(),
                claude_session: "claude-1".into(),
                worktree: "/wt/b".into(),
                branch: "sim/jump".into(),
                pane: "w1:p1".into(),
                model: "opus".into(),
                started: T + 12.0,
                closed: None,
                closed_why: String::new(),
                restore: String::new(),
            },
            TaskSessionRow {
                slug: "sim/jump/land".into(),
                handle: "cobalt-otter".into(),
                role: "reviewer".into(),
                claude_session: "claude-2".into(),
                worktree: "/wt/c".into(),
                branch: String::new(),
                pane: String::new(),
                model: String::new(),
                started: T + 13.0,
                closed: Some(T + 14.0),
                closed_why: "done".into(),
                restore: "claude --resume claude-2".into(),
            },
        ],
    }
}

fn insert(db: &Connection, table: &str, row: &Map<String, Value>) {
    let columns: Vec<&str> = row.keys().map(String::as_str).collect();
    let marks = vec!["?"; columns.len()].join(", ");
    let values: Vec<Sql> = row
        .values()
        .map(|value| match value {
            Value::Null => Sql::Null,
            Value::Bool(b) => Sql::Integer(i64::from(*b)),
            Value::Number(n) => n
                .as_i64()
                .map_or(Sql::Real(n.as_f64().unwrap()), Sql::Integer),
            Value::String(s) => Sql::Text(s.clone()),
            other => panic!("no {other} on the board"),
        })
        .collect();
    db.execute(
        &format!(
            "INSERT INTO {table} ({}) VALUES ({marks})",
            columns.join(", ")
        ),
        rusqlite::params_from_iter(values),
    )
    .unwrap();
}
