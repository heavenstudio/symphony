//! The board's move from SQLite to SurrealDB (`tasks-cutover`, [`cutover`]), forward only.
//!
//! The board is live under dozens of sessions when it moves, so the move must lose nothing, leave the
//! old board refusing writes and kept as it was, and, whenever it fails, leave exactly one board taking
//! writes: the SQLite one, never both. Each test has a SQLite board of its own, in the schema and with
//! the rows the Python store kept (`tools/tasks_store.py`, as the board moved), and a database of its own
//! on the tests' SurrealDB server.

use crate::common::{client, server_addr};
use crate::sqlite::{self, TABLES, sample};
use rusqlite::Connection;
use rusqlite::types::Value as Sql;
use serde_json::{Map, Value, json};
use std::cell::{Cell, RefCell};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use symphony_board::cli::Env;
use symphony_board::cutover::{self, FROZEN};
use symphony_board::rows::Dump;
use symphony_board::rpc::Client;
use symphony_board::{NewSession, NewTask, Store};

/// One write to each table, to see the frozen board refuse them all.
const WRITES: [&str; 10] = [
    "INSERT INTO tasks (slug, title, status, created_by, created, updated) VALUES ('x', 'X', 'ready', 'o', 1, 1)",
    "UPDATE tasks SET title = 'Hop' WHERE slug = 'sim/jump'",
    "DELETE FROM tasks WHERE slug = 'sim/jump/land'",
    "INSERT INTO task_links VALUES ('sim/jump', 'doc', 'docs/x.md')",
    "UPDATE sessions SET last_seen = 0 WHERE handle = 'cobalt-otter'",
    "DELETE FROM roles",
    "INSERT INTO messages (slug, sender, kind, text, ts) VALUES ('sim/jump', 'o', 'msg', 'hi', 1)",
    "UPDATE deliveries SET acked = 0",
    "INSERT INTO barred_keys VALUES ('key-2')",
    "DELETE FROM task_sessions",
];

/// A SQLite board of a test's own, in a folder of its own, and the SurrealDB database it moves into.
struct Old {
    dir: PathBuf,
    path: PathBuf,
    db: Client,
}

impl Old {
    /// The sample board, in WAL mode as the store kept it.
    fn new(name: &str) -> Old {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("cutover-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lamp-works-tasks.db");
        sqlite::make(&path, &sample());
        Old {
            dir,
            path,
            db: client(),
        }
    }

    /// Another sample board, in a folder of its own, that moves into the same SurrealDB database as
    /// `into`.
    fn into(name: &str, into: &Old) -> Old {
        let mut old = Old::new(name);
        old.db = Client::new(&server_addr(), "sf", into.db.database());
        old
    }

    /// A copy of this board as it is now (frozen, once it moved), in a folder of its own, that moves
    /// into the same SurrealDB database: a dry run of the move on a copy.
    fn copy(&self, name: &str) -> Old {
        let copy = Old::into(name, self);
        fs::remove_dir_all(&copy.dir).unwrap();
        fs::create_dir_all(&copy.dir).unwrap();
        self.connect()
            .execute("VACUUM INTO ?1", [copy.path.display().to_string()])
            .unwrap();
        copy
    }

    fn connect(&self) -> Connection {
        Connection::open(&self.path).unwrap()
    }

    /// The environment of a run that reaches this test's database, as `TASKS_SERVER` and
    /// `TASKS_DATABASE` point a run at a board.
    fn env(&self) -> Env {
        Env {
            vars: [
                ("TASKS_SERVER".to_string(), server_addr()),
                ("TASKS_DATABASE".to_string(), self.db.database().to_string()),
            ]
            .into_iter()
            .collect(),
            cwd: self.dir.clone(),
        }
    }

    fn with(&self, name: &str, value: &str) -> Env {
        let mut env = self.env();
        env.vars.insert(name.into(), value.into());
        env
    }

    /// An environment whose SurrealDB board cannot be reached.
    fn unreachable(&self) -> Env {
        self.with("TASKS_SERVER", "127.0.0.1:1")
    }

    fn store(&self) -> Store {
        Store::open(Client::new(&server_addr(), "sf", self.db.database())).unwrap()
    }

    fn cutover(&self) -> Result<Dump, String> {
        cutover::cutover(&self.path, &self.env())
    }

    /// One statement on the old board, as a session still on the old store writes it.
    fn write(&self, sql: &str) -> Result<(), String> {
        self.connect().execute_batch(sql).map_err(|e| e.to_string())
    }

    /// Every row of the old board, as the copy reads it.
    fn rows(&self) -> Dump {
        let db = self.connect();
        let mut tables = Map::new();
        for table in TABLES {
            let mut statement = db
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                .unwrap();
            let names: Vec<String> = statement
                .column_names()
                .iter()
                .map(|n| n.to_string())
                .collect();
            let rows: Vec<Value> = statement
                .query_map([], |row| {
                    let mut object = Map::new();
                    for (i, name) in names.iter().enumerate() {
                        let value = match row.get::<_, Sql>(i)? {
                            Sql::Null => Value::Null,
                            Sql::Integer(n) => n.into(),
                            Sql::Real(x) => x.into(),
                            Sql::Text(s) => s.into(),
                            Sql::Blob(_) => panic!("no blob on the board"),
                        };
                        object.insert(name.clone(), value);
                    }
                    Ok(Value::Object(object))
                })
                .unwrap()
                .map(Result::unwrap)
                .collect();
            tables.insert(table.into(), rows.into());
        }
        Dump::from_json(&Value::Object(tables).to_string()).unwrap()
    }

    /// Whether the board carries the move's freeze.
    fn frozen(&self) -> bool {
        self.connect()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'sf_moved_%'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
            > 0
    }

    /// How many times the board's schema changed: a freeze, and its thaw, each change it.
    fn schema_version(&self) -> i64 {
        self.connect()
            .pragma_query_value(None, "schema_version", |row| row.get(0))
            .unwrap()
    }

    /// What a move killed after its freeze leaves: every table frozen as the move freezes it.
    fn freeze_as_a_killed_move_left_it(&self) {
        let db = self.connect();
        for table in TABLES {
            for op in ["INSERT", "UPDATE", "DELETE"] {
                db.execute_batch(&format!(
                    "CREATE TRIGGER sf_moved_{table}_{} BEFORE {op} ON {table} BEGIN SELECT RAISE(ABORT, '{FROZEN}'); END",
                    op.to_lowercase()
                ))
                .unwrap();
            }
        }
    }

    fn beside(&self, end: &str) -> PathBuf {
        self.dir.join(format!("lamp-works-tasks.db{end}"))
    }

    fn moved(&self) -> PathBuf {
        self.beside(".moved")
    }

    /// The files in the board's folder, less SQLite's own sidecars.
    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| !name.ends_with("-wal") && !name.ends_with("-shm"))
            .collect();
        names.sort();
        names
    }
}

impl Drop for Old {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn the_board_moves_whole_every_row_and_column() {
    let old = Old::new("whole");
    old.cutover().unwrap();
    let store = old.store();
    assert_eq!(store.dump().unwrap(), sample());
    let slugs: Vec<String> = store
        .tasks(None)
        .unwrap()
        .into_iter()
        .map(|t| t.slug)
        .collect();
    assert_eq!(slugs, ["sim/jump", "sim/jump/land"]);
    let said: Vec<String> = store
        .thread("sim/jump")
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert_eq!(said, ["go — now", "done\n✓"]);
}

#[test]
fn what_it_moved_is_every_row_of_every_table() {
    let old = Old::new("said");
    assert_eq!(old.cutover().unwrap(), sample());
}

#[test]
fn a_moved_board_refuses_every_write_saying_where_the_board_went() {
    let old = Old::new("frozen");
    old.cutover().unwrap();
    for sql in WRITES {
        let refused = old.write(sql).unwrap_err();
        assert!(refused.ends_with(FROZEN), "{sql}: {refused}");
    }
    assert_eq!(old.rows(), sample(), "the frozen board is kept as it moved");
}

#[test]
fn a_moved_board_is_marked_moved_and_one_not_moved_is_not() {
    let old = Old::new("marked");
    assert!(!old.moved().exists());
    old.cutover().unwrap();
    assert!(old.moved().is_file());
}

#[test]
fn a_board_already_moved_is_not_moved_again_over_what_was_written_since() {
    let old = Old::new("twice");
    old.cutover().unwrap();
    old.store()
        .create(&NewTask::new("sim/after", "After the move", "owner"))
        .unwrap();
    let refused = old.cutover().unwrap_err();
    assert!(refused.contains("already moved"), "{refused}");
    assert!(old.store().get("sim/after").is_ok());
}

/// fleet-vole (must-fix 8): the guard read the marker beside the board being moved, not the board it
/// moves into, so a second board (here a copy of the frozen one: a dry run of the move) replaced a
/// SurrealDB board written since its move, marked its own keys bound and carried its own sweep. The
/// SurrealDB board carries its move's record: a move into one written since is refused, and nothing of
/// the live board, its keys or its sweep changes.
#[test]
fn a_second_board_is_not_moved_over_a_board_written_since_its_move() {
    let old = Old::new("over");
    let (keys, reap) = (old.dir.join("keys"), old.dir.join("reap.json"));
    let env = |board: &Old| {
        let mut env = board.env();
        env.vars
            .insert("TASKS_KEYS".into(), keys.display().to_string());
        env.vars
            .insert("TASKS_REAP_STATE".into(), reap.display().to_string());
        env
    };
    cutover::cutover(&old.path, &env(&old)).unwrap();
    old.store()
        .create(&NewTask::new("sim/after", "After the move", "owner"))
        .unwrap();
    fs::write(keys.join("agent-2"), "").unwrap();
    fs::write(&reap, r#"{"idle": {"since": 1}}"#).unwrap();
    let copy = old.copy("over-copy");
    fs::write(copy.beside(".reap.json"), r#"{"idle": {"copy": 1}}"#).unwrap();
    let before = copy.schema_version();

    let refused = cutover::cutover(&copy.path, &env(&copy)).unwrap_err();
    assert!(refused.contains("written since"), "{refused}");
    assert!(
        refused.contains(&old.path.display().to_string()),
        "{refused}"
    );
    assert!(old.store().get("sim/after").is_ok());
    let mut bound: Vec<String> = fs::read_dir(&keys)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    bound.sort();
    assert_eq!(bound, ["agent-1", "agent-2"]);
    assert_eq!(
        fs::read_to_string(&reap).unwrap(),
        r#"{"idle": {"since": 1}}"#
    );
    assert!(!copy.moved().exists());
    assert_eq!(copy.schema_version(), before, "the copy is not touched");
}

/// The look and the record are both in the copy's one transaction: a board written between the move's
/// first look and its copy (a session's write landing then) is refused there, and keeps the write.
#[test]
fn a_move_into_a_board_written_since_its_move_is_refused_in_the_copy_itself() {
    let old = Old::new("guard");
    let store = old.store();
    assert_eq!(store.moved().unwrap(), None);
    store.move_in(&sample(), "/boards/a.db").unwrap();
    let moved = store.moved().unwrap().unwrap();
    assert_eq!(
        (moved.from.as_str(), moved.written_since),
        ("/boards/a.db", false)
    );
    store
        .create(&NewTask::new("sim/after", "After the move", "owner"))
        .unwrap();
    assert!(store.moved().unwrap().unwrap().written_since);
    let refused = store
        .move_in(&sample(), "/boards/b.db")
        .unwrap_err()
        .to_string();
    assert!(refused.contains("written since"), "{refused}");
    assert!(refused.contains("/boards/a.db"), "{refused}");
    assert!(store.get("sim/after").is_ok());
}

/// The record goes with the SurrealDB board, so a board whose marker was lost is not moved again over
/// what was written since either.
#[test]
fn a_board_whose_marker_was_lost_is_not_moved_again_over_what_was_written_since() {
    let old = Old::new("lost");
    old.cutover().unwrap();
    old.store()
        .create(&NewTask::new("sim/after", "After the move", "owner"))
        .unwrap();
    fs::remove_file(old.moved()).unwrap();
    let before = old.schema_version();
    let refused = old.cutover().unwrap_err();
    assert!(refused.contains("written since"), "{refused}");
    assert!(refused.contains("put it back"), "{refused}");
    assert!(old.store().get("sim/after").is_ok());
    assert!(!old.moved().exists());
    assert!(
        old.frozen(),
        "the old board is never thawed to take writes again"
    );
    assert_eq!(old.schema_version(), before);
}

/// A move thaws only SQLite boards it froze itself. A lost marker followed by
/// a concurrent SurrealDB write must leave an already frozen source frozen.
#[test]
fn a_write_landing_during_the_copy_leaves_a_board_a_finished_move_froze_frozen() {
    let old = Old::new("landing");
    old.cutover().unwrap();
    fs::remove_file(old.moved()).unwrap();
    let before = old.schema_version();
    let during = || {
        old.store()
            .create(&NewTask::new("sim/during", "During the copy", "owner"))
            .unwrap();
    };
    let refused = cutover::cutover_meanwhile(&old.path, &old.env(), &during).unwrap_err();
    assert_eq!(refused, written_since(&old), "the look's own refusal");
    assert!(old.frozen(), "never thawed");
    assert_eq!(old.schema_version(), before);
    assert!(old.write(WRITES[0]).unwrap_err().ends_with(FROZEN));
    assert!(old.store().get("sim/during").is_ok());
    assert!(!old.moved().exists());
}

/// The same write landing during the copy of a board this move froze (the SurrealDB board held another
/// board's move): the board was the one taking writes when the move began, so it is thawed and takes
/// them again.
#[test]
fn a_write_landing_during_the_copy_of_a_board_this_move_froze_thaws_it() {
    let old = Old::new("thawed");
    let other = Old::into("thawed-other", &old);
    other.cutover().unwrap();
    let during = || {
        old.store()
            .create(&NewTask::new("sim/during", "During the copy", "owner"))
            .unwrap();
    };
    let refused = cutover::cutover_meanwhile(&old.path, &old.env(), &during).unwrap_err();
    assert!(refused.contains("written since"), "{refused}");
    assert!(
        refused.ends_with("; the board stays in SQLite, taking writes again"),
        "{refused}"
    );
    assert!(!old.frozen());
    old.write(WRITES[0]).unwrap();
    assert!(!old.moved().exists());
}

/// A move killed after its freeze leaves the board frozen; a run after it that fails leaves it as it
/// found it, frozen, and says so: never that it was never frozen, and never thawed by a run that did not
/// freeze it.
#[test]
fn a_run_that_fails_on_a_board_a_move_before_it_froze_leaves_it_frozen_and_says_so() {
    let old = Old::new("found-frozen");
    old.freeze_as_a_killed_move_left_it();
    let refused = cutover::cutover(&old.path, &old.unreachable()).unwrap_err();
    assert!(
        refused
            .ends_with("; the board stays frozen, as this run found it: run tasks-cutover again"),
        "{refused}"
    );
    assert!(old.frozen());

    let full = |_: &Path, _: &[u8]| -> io::Result<()> { Err(io::Error::from_raw_os_error(28)) };
    let refused = cutover::cutover_writing(&old.path, &old.env(), &full).unwrap_err();
    assert!(refused.contains("No space left on device"), "{refused}");
    assert!(
        refused
            .ends_with("; the board stays frozen, as this run found it: run tasks-cutover again"),
        "{refused}"
    );
    assert!(old.frozen());
    assert!(!old.moved().exists());
    old.cutover().unwrap();
    assert_eq!(old.store().dump().unwrap(), sample());
}

/// The look's refusal for a board moved from `old`, written since.
fn written_since(old: &Old) -> String {
    let refused = old.cutover().unwrap_err();
    assert!(refused.contains("written since"), "{refused}");
    refused
}

/// A board moved, its marker lost, then `write` alone on the SurrealDB board, as a client of it: the
/// move's record holds every table, so a move again is refused whichever table the write touched.
fn a_move_again_is_refused_after(name: &str, write: &dyn Fn(&Store)) {
    let old = Old::new(name);
    old.cutover().unwrap();
    fs::remove_file(old.moved()).unwrap();
    let later = Store::with(
        Client::new(&server_addr(), "sf", old.db.database()),
        Box::new(|| 1_900_000_000.0),
        Box::new(|_| true),
        "this-host",
    )
    .unwrap();
    write(&later);
    let refused = old.cutover().unwrap_err();
    assert!(refused.contains("written since"), "{name}: {refused}");
}

/// `sql` alone, on the board's server.
fn only(sql: &'static str) -> impl Fn(&Store) {
    move |store| {
        store.client().query(sql, json!({})).unwrap();
    }
}

#[test]
fn a_beat_alone_after_the_move_holds_up_a_move_again() {
    a_move_again_is_refused_after("beat", &|store| store.touch("cobalt-otter").unwrap());
}

#[test]
fn a_registration_alone_after_the_move_holds_up_a_move_again() {
    a_move_again_is_refused_after("register", &|store| {
        store.register(&NewSession::new("worker")).unwrap();
    });
}

#[test]
fn a_task_alone_after_the_move_holds_up_a_move_again() {
    a_move_again_is_refused_after("task", &only("UPDATE task:⟨sim/jump⟩ SET title = 'Hop';"));
}

#[test]
fn a_link_alone_after_the_move_holds_up_a_move_again() {
    a_move_again_is_refused_after(
        "link",
        &only(
            "CREATE link:['sim/jump', 'doc', 'docs/x.md'] SET slug = 'sim/jump', kind = 'doc', target = 'docs/x.md';",
        ),
    );
}

#[test]
fn a_role_alone_after_the_move_holds_up_a_move_again() {
    a_move_again_is_refused_after("role", &only("DELETE role;"));
}

#[test]
fn a_message_alone_after_the_move_holds_up_a_move_again() {
    a_move_again_is_refused_after(
        "message",
        &only(
            "CREATE message:['sim/jump', 99] SET sid = 99, slug = 'sim/jump', sender = 'owner', kind = 'msg', text = 'hi', ts = 1.0;",
        ),
    );
}

#[test]
fn a_delivery_alone_after_the_move_holds_up_a_move_again() {
    a_move_again_is_refused_after("delivery", &only("UPDATE delivery SET acked = 2.0;"));
}

#[test]
fn a_barred_key_alone_after_the_move_holds_up_a_move_again() {
    a_move_again_is_refused_after(
        "barred",
        &only("CREATE barred_key:⟨key-2⟩ SET key = 'key-2';"),
    );
}

#[test]
fn a_task_session_alone_after_the_move_holds_up_a_move_again() {
    a_move_again_is_refused_after("task-session", &only("DELETE task_session;"));
}

#[test]
fn a_counter_alone_after_the_move_holds_up_a_move_again() {
    a_move_again_is_refused_after("counter", &only("UPSERT counter:tree SET last = 1;"));
}

/// A move into a board that holds a move and nothing written since loses nothing, so it goes on: a dry
/// run on a copy before the move does not hold up the move itself.
#[test]
fn a_board_moved_into_and_not_written_since_takes_another_move() {
    let old = Old::new("dry");
    let copy = old.copy("dry-copy");
    cutover::cutover(&copy.path, &copy.env()).unwrap();
    old.write(WRITES[0]).unwrap();
    old.cutover().unwrap();
    assert_eq!(old.store().dump().unwrap(), old.rows());
}

/// A move that failed after its copy (here the marker's disk was full) leaves its record in SurrealDB,
/// and nothing is written there since: run again, it goes to the end.
#[test]
fn a_move_that_failed_after_its_copy_runs_again_to_the_end() {
    let old = Old::new("again");
    let full = |_: &Path, _: &[u8]| -> io::Result<()> { Err(io::Error::from_raw_os_error(28)) };
    cutover::cutover_writing(&old.path, &old.env(), &full).unwrap_err();
    old.write(WRITES[0]).unwrap();
    old.cutover().unwrap();
    assert_eq!(old.store().dump().unwrap(), old.rows());
    assert!(old.moved().is_file());
}

/// fleet-vole (low 1): a copy killed while it staged its rows left its staging tables in SurrealDB for
/// good. The next move, holding the lock, removes any it finds.
#[test]
fn the_staging_tables_a_killed_copy_left_are_removed_by_the_next_move() {
    let old = Old::new("staged");
    let store = old.store();
    store
        .client()
        .query(
            "DEFINE TABLE copy_00000000deadbeef_message SCHEMALESS; CREATE copy_00000000deadbeef_message SET row = { text: 'left' };",
            json!({}),
        )
        .unwrap();
    old.cutover().unwrap();
    let tables = store
        .client()
        .query("RETURN object::keys((INFO FOR DB).tables);", json!({}))
        .unwrap();
    let left: Vec<&Value> = tables
        .last()
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .filter(|name| name.as_str().is_some_and(|name| name.starts_with("copy_")))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

/// fleet-vole (low 2): the marker's own file a killed move left stayed beside the board after the next
/// move. The next move, holding the lock, removes it.
#[test]
fn the_marker_file_a_killed_move_left_is_removed_by_the_next_move() {
    let old = Old::new("stale");
    fs::write(old.beside(".moved.99999"), "moved to Sur").unwrap();
    old.cutover().unwrap();
    assert_eq!(
        old.files(),
        [
            "lamp-works-tasks.db",
            "lamp-works-tasks.db.cutover",
            "lamp-works-tasks.db.moved"
        ]
    );
}

/// Frozen SQLite writes report the freeze without claiming the move finished. `just tasks` reads the
/// marker to distinguish an unfinished move from a completed one.
#[test]
fn while_the_board_moves_its_writes_are_refused_without_saying_it_moved() {
    let old = Old::new("moving");
    let said = RefCell::new(String::new());
    let write = |path: &Path, bytes: &[u8]| -> io::Result<()> {
        *said.borrow_mut() = old.write(WRITES[0]).unwrap_err();
        fs::write(path, bytes)
    };
    cutover::cutover_writing(&old.path, &old.env(), &write).unwrap();
    let said = said.into_inner();
    assert!(said.contains("frozen"), "{said}");
    assert!(!said.contains("moved to SurrealDB"), "{said}");
    assert!(said.contains("run just tasks again"), "{said}");
}

/// A move killed after its freeze leaves the old board frozen and unmarked (nothing takes writes until
/// it is run again), maybe a part of its copy in SurrealDB, and maybe its marker's own unfinished file:
/// run again, it goes to the end, and none of that reads as done.
#[test]
fn a_cutover_killed_after_the_freeze_runs_again_to_the_end() {
    let old = Old::new("killed");
    old.freeze_as_a_killed_move_left_it();
    fs::write(old.beside(".moved.99999"), "moved to Sur").unwrap();
    let mut part = sample();
    part.messages.truncate(1);
    part.roles.clear();
    old.store().copy(&part).unwrap();
    old.cutover().unwrap();
    assert_eq!(old.store().dump().unwrap(), sample());
    assert!(old.moved().is_file());
    assert!(old.frozen());
}

/// The server is reached, and the board's tables made in it, before anything is frozen: a cold start of
/// the server inside the freeze would hold every session on the old store refused meanwhile, and a
/// move killed then would leave it so.
#[test]
fn a_server_it_cannot_reach_leaves_the_board_never_frozen() {
    let old = Old::new("unreached");
    let before = old.schema_version();
    let refused = cutover::cutover(&old.path, &old.unreachable()).unwrap_err();
    assert!(
        refused.ends_with("; the board stays in SQLite, never frozen"),
        "{refused}"
    );
    assert_eq!(old.schema_version(), before, "no freeze and no thaw");
    assert!(!old.moved().exists());
    old.write(WRITES[0]).unwrap();
}

#[test]
fn a_board_with_a_table_the_move_does_not_know_is_neither_moved_nor_frozen() {
    let old = Old::new("table");
    old.write("CREATE TABLE notes (text TEXT)").unwrap();
    let before = old.schema_version();
    let refused = old.cutover().unwrap_err();
    assert!(refused.contains("notes"), "{refused}");
    assert_eq!(old.schema_version(), before);
    assert!(!old.moved().exists());
}

/// A column the copy does not know would be dropped: the board stays in SQLite, and is never frozen
/// for it.
#[test]
fn a_board_with_a_column_the_move_does_not_know_is_neither_moved_nor_frozen() {
    let old = Old::new("column");
    old.write("ALTER TABLE tasks ADD COLUMN notes TEXT")
        .unwrap();
    let before = old.schema_version();
    let refused = old.cutover().unwrap_err();
    assert!(refused.contains("notes"), "{refused}");
    assert_eq!(old.schema_version(), before);
    assert!(!old.moved().exists());
}

#[test]
fn the_idle_sweeps_state_moves_with_the_board() {
    let old = Old::new("reap");
    fs::write(old.beside(".reap.json"), r#"{"idle": {"amber-heron": 1}}"#).unwrap();
    let new = old.dir.join("sf-tasks").join("reap.json");
    let env = old.with("TASKS_REAP_STATE", &new.display().to_string());
    cutover::cutover(&old.path, &env).unwrap();
    assert_eq!(
        fs::read_to_string(&new).unwrap(),
        r#"{"idle": {"amber-heron": 1}}"#
    );
}

/// The heartbeat hook skips a full session's tool call unless its key has a file in the bound keys'
/// folder: the move writes one for every key the old board had bound to a session that has not left,
/// and no other, so no session it carried over reads lost.
#[test]
fn the_keys_the_board_had_bound_are_marked_bound_and_no_other() {
    let old = Old::new("keys");
    let keys = old.dir.join("keys");
    fs::create_dir_all(&keys).unwrap();
    fs::write(keys.join("stale"), "").unwrap();
    old.write("INSERT INTO sessions (handle, kind, connected, last_seen, left_at, agent) VALUES ('gone-heron', 'worker', 1, 1, 2, 'agent-3'), ('plain-heron', 'worker', 1, 1, NULL, '')")
        .unwrap();
    let env = old.with("TASKS_KEYS", &keys.display().to_string());
    cutover::cutover(&old.path, &env).unwrap();
    let mut names: Vec<String> = fs::read_dir(&keys)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["agent-1"]);
}

#[test]
fn a_board_that_does_not_exist_is_not_moved() {
    let old = Old::new("nowhere");
    let refused = cutover::cutover(&old.dir.join("nowhere.db"), &old.env()).unwrap_err();
    assert!(refused.starts_with("no SQLite board at "), "{refused}");
    assert!(!old.dir.join("nowhere.db").exists());
}

/// A copy SurrealDB refuses (here a status it does not know) fails once the board is frozen: it is
/// thawed, unmarked, and takes writes again.
#[test]
fn a_copy_the_board_refuses_leaves_the_sqlite_board_taking_writes_and_unmarked() {
    let old = Old::new("refused");
    old.write("UPDATE tasks SET status = 'parked' WHERE slug = 'sim/jump'")
        .unwrap();
    let refused = old.cutover().unwrap_err();
    assert!(
        refused.ends_with("; the board stays in SQLite, taking writes again"),
        "{refused}"
    );
    assert!(!old.frozen());
    assert!(!old.moved().exists());
    old.write(WRITES[0]).unwrap();
}

/// What SurrealDB holds after the copy is read back and checked row for row; a table that came back
/// otherwise (here a server that keeps every role's time a second on) is named, and the board stays.
#[test]
fn a_copy_that_does_not_match_the_board_names_the_table_and_leaves_it_in_sqlite() {
    let old = Old::new("mismatch");
    let store = old.store();
    store
        .client()
        .query(
            "DEFINE FIELD OVERWRITE since ON role TYPE float VALUE $value + 1;",
            serde_json::json!({}),
        )
        .unwrap();
    let refused = old.cutover().unwrap_err();
    assert!(
        refused.contains("roles (2 rows in SQLite, 2 in SurrealDB, 4 not in both)"),
        "{refused}"
    );
    assert!(!refused.contains("tasks ("), "{refused}");
    assert!(!old.frozen());
    assert!(!old.moved().exists());
    old.write(WRITES[0]).unwrap();
}

/// The marker is written to a separate file and renamed into place whole. A failed move removes it
/// and thaws SQLite, so only SQLite takes writes. Readers must never observe a partial marker.
#[test]
fn a_full_disk_writing_the_marker_leaves_no_marker_and_the_board_taking_writes() {
    let old = Old::new("full");
    let seen = Cell::new(false);
    let full = |path: &Path, bytes: &[u8]| -> io::Result<()> {
        fs::write(path, &bytes[..bytes.len() / 2])?;
        seen.set(old.moved().exists());
        Err(io::Error::from_raw_os_error(28))
    };
    let refused = cutover::cutover_writing(&old.path, &old.env(), &full).unwrap_err();
    assert!(refused.contains("No space left on device"), "{refused}");
    assert!(
        refused.ends_with("; the board stays in SQLite, taking writes again"),
        "{refused}"
    );
    assert!(!old.moved().exists());
    assert_eq!(
        old.files(),
        ["lamp-works-tasks.db", "lamp-works-tasks.db.cutover"],
        "nothing of the marker is left"
    );
    assert!(!seen.get(), "the marker was there while it was written");
    assert!(!old.frozen());
    old.write(WRITES[0]).unwrap();
}

/// fleet-vole (must-fix 6): nothing kept one run from another. A move holds its lock for its whole run;
/// a second refuses, naming the first by its pid, and changes nothing.
#[test]
fn a_second_cutover_while_one_runs_is_refused_naming_its_pid() {
    let old = Old::new("second");
    let held = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(old.beside(".cutover"))
        .unwrap();
    held.try_lock().unwrap();
    fs::write(old.beside(".cutover"), "4242\n").unwrap();
    let before = old.schema_version();
    let refused = old.cutover().unwrap_err();
    assert!(
        refused.contains("a move of this board is running (pid 4242)"),
        "{refused}"
    );
    assert_eq!(old.schema_version(), before);
    assert!(!old.moved().exists());
    drop(held);
    old.cutover().unwrap();
    assert!(old.moved().is_file());
}

fn run_main(args: &[&str], env: &Env) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cutover::main(&args, env, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn the_command_says_what_moved_and_where_the_frozen_board_is_kept() {
    let old = Old::new("main");
    let env = old.with("TASKS_DB", &old.path.display().to_string());
    let (code, out, err) = run_main(&[], &env);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        out,
        format!(
            "✅ tasks-cutover: 2 tasks, 2 links, 2 sessions, 2 roles, 2 messages, 1 deliveries, 1 barred keys, 2 task sessions moved to SurrealDB; {} refuses writes now, kept as it was as a read-only backup\n",
            old.path.display()
        )
    );
}

#[test]
fn help_alone_names_every_variable_and_moves_nothing() {
    let old = Old::new("help");
    let env = old.with("TASKS_DB", &old.path.display().to_string());
    let before = old.schema_version();
    for help in ["--help", "-h"] {
        assert_eq!(
            run_main(&[help], &env),
            (0, cutover::HELP.to_string(), String::new())
        );
    }
    for variable in ["TASKS_DB", "TASKS_SERVER", "TASKS_DATABASE"] {
        assert!(cutover::HELP.contains(variable), "{variable}");
    }
    assert_eq!(
        run_main(&["--help", "now"], &env),
        (2, String::new(), "usage: tasks-cutover\n".to_string())
    );
    assert_eq!(old.schema_version(), before);
    assert!(!old.moved().exists());
}

#[test]
fn a_board_it_cannot_find_is_a_refusal() {
    let env = Env {
        vars: Default::default(),
        cwd: std::env::temp_dir(),
    };
    let (code, out, err) = run_main(&[], &env);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(
        err.starts_with("❌ tasks-cutover: ")
            && err.contains("not inside a git repository")
            && err.ends_with("; run it in the repository whose board it moves\n"),
        "{err}"
    );
}

/// Outside a repository there is no SurrealDB board of its own to move into, so a board `TASKS_DB` names
/// moves only into a server `TASKS_SERVER` names.
#[test]
fn a_board_named_outside_a_repository_without_a_server_is_refused() {
    let old = Old::new("outside");
    let env = Env {
        vars: [("TASKS_DB".to_string(), old.path.display().to_string())]
            .into_iter()
            .collect(),
        cwd: std::env::temp_dir(),
    };
    let (code, out, err) = run_main(&[], &env);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(err.contains("not inside a git repository"), "{err}");
    assert!(!old.frozen());
    assert!(!old.beside(".cutover").exists());
}

#[test]
fn a_file_that_is_not_a_board_is_a_one_line_refusal() {
    let old = Old::new("garbage");
    fs::remove_file(&old.path).unwrap();
    fs::write(&old.path, "not a database at all ".repeat(200)).unwrap();
    let env = old.with("TASKS_DB", &old.path.display().to_string());
    let (code, out, err) = run_main(&[], &env);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(
        err.starts_with("❌ tasks-cutover: ")
            && err.contains("not a database")
            && err.lines().count() == 1,
        "{err}"
    );
}

#[test]
fn anything_else_is_its_usage() {
    let old = Old::new("usage");
    let env = old.with("TASKS_DB", &old.path.display().to_string());
    for args in [&["now"][..], &["cutover"], &["--board"]] {
        let (code, out, err) = run_main(args, &env);
        assert_eq!(
            (code, out.as_str(), err.as_str()),
            (2, "", "usage: tasks-cutover\n"),
            "{args:?}"
        );
    }
    assert!(!old.frozen());
}

/// Read freeze state on the run's own connection, waiting out a holder so
/// an exclusively held frozen board is identified correctly.
#[test]
fn a_board_held_exclusively_as_the_run_starts_is_still_found_frozen() {
    let old = Old::new("held");
    old.freeze_as_a_killed_move_left_it();
    let holder = old.connect();
    holder
        .execute_batch("PRAGMA locking_mode = EXCLUSIVE; BEGIN IMMEDIATE; COMMIT;")
        .unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(2500));
        drop(holder);
    });
    // The run waits out the holder for the backstop of the old store's own 30 s, so a loaded
    // machine's late release never reads as a board held past its wait.
    let busy = test_time::backstop(std::time::Duration::from_secs(30));
    let refused = cutover::cutover_waiting(&old.path, &old.unreachable(), busy).unwrap_err();
    release.join().unwrap();
    assert!(
        refused
            .ends_with("; the board stays frozen, as this run found it: run tasks-cutover again"),
        "{refused}"
    );
    assert!(old.frozen());
}

/// A freeze-state read held past its wait returns its error and leaves the
/// board frozen; unreadable state must never imply unfrozen state.
#[test]
fn a_board_held_past_the_wait_is_a_refusal_never_read_as_not_frozen() {
    let old = Old::new("held-long");
    old.freeze_as_a_killed_move_left_it();
    let holder = old.connect();
    holder
        .execute_batch("PRAGMA locking_mode = EXCLUSIVE; BEGIN IMMEDIATE; COMMIT;")
        .unwrap();
    let wait = std::time::Duration::from_millis(300);
    let refused = cutover::cutover_waiting(&old.path, &old.unreachable(), wait).unwrap_err();
    drop(holder);
    assert!(
        refused.ends_with("database is locked; nothing changed"),
        "{refused}"
    );
    assert!(old.frozen());
    assert!(!old.moved().exists());
}
