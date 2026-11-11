//! The task board's move from SQLite to SurrealDB, run once by whoever lands slice 3 of
//! `tools/docs-app-rust-port` (`tasks-cutover`, docs/workflow/TASKS.md#the-move-to-surrealdb). It
//! goes one way: nothing moves the board back.
//!
//! The SurrealDB board is the one `tasks-copy --board` reaches from here ([`crate::cli::board_client`]):
//! this repository's own, unless `TASKS_SERVER` names another (a test's, a dry run's). Only this
//! repository's own board moves into its own SurrealDB board: `<project id>-tasks.db` in the git common
//! dir ([`board_name`]), which `TASKS_DB` may name by another path or another name of its file. Whether a board is a
//! repository's own is decided from where it is, wherever the run starts, and a repository's own board
//! is refused any `TASKS_SERVER` (another would freeze it and mark it moved with its rows elsewhere);
//! any other board moves only into a server `TASKS_SERVER` names, and never by a hard link. Whether a
//! move froze the board is read on the run's own connection, an error a refusal. A move holds `<board>.cutover` locked for its whole run, its
//! pid written in it, and a second refuses, naming that pid. Holding it, it refuses a board marked
//! moved and removes what a killed run left (its marker's own file, its staging tables in SurrealDB).
//! Before anything is frozen it refuses a file that is not a board and a table or a column it does not
//! know (it would leave them behind), and it reaches the server and makes the board's tables there,
//! refusing a SurrealDB board that holds a move and was written since ([`Store::moved`]): a move into
//! it would lose that, whatever marker was lost. Then it freezes the old board: a trigger on every table
//! refuses every write, saying the board is frozen for its move, so nothing a session still on the old
//! store writes is lost between the copy and the switch. It copies every row, checking in the copy's one
//! transaction that the SurrealDB board was not written since a move it holds and recording this one
//! there ([`Store::move_in`]), reads them back and checks them row for row, carries the idle sweep's
//! state (`<board>.reap.json`) and marks bound the keys the old board had bound (the heartbeat
//! hook's look), and last writes `<board>.moved`, whole or not at all (a file of its own, renamed into place), which
//! is what lets the binary take the board in the common dir. The frozen board stays where it is, as it
//! was: the read-only backup of the board as it moved.
//!
//! A move that fails takes its marker away and leaves the old board as it found it: thawed if it froze
//! it, so the SQLite board is again the one taking writes; frozen if a move before it froze it, for that
//! board may be the one a finished move left (its marker lost, sessions writing to SurrealDB), and a run
//! that stopped knows no better. A move killed part way leaves the old board frozen and unmarked, which
//! loses nothing: run again, it goes to the end, and nothing but `.moved` counts as the move done. What
//! it recorded in SurrealDB holds it up only once that board has been written since, which nothing but a
//! finished move lets happen.

mod board_file;

use crate::cli::{self, Env};
use crate::rows::Dump;
use crate::{Moved, Store, git, server, sys};
pub use board_file::board_name;
use board_file::{board_in, repository_board, same_file};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use serde_json::{Map, Value};
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What every write to the frozen SQLite board is refused with, from the freeze on: true whether the
/// move has finished, is running or was cut short, which the binary, reading the marker, tells apart.
pub const FROZEN: &str = "the task board is frozen for its move to SurrealDB: merge main, then run just tasks again, which says whether the move finished";
const USAGE: &str = "usage: tasks-cutover";
/// The tables the move knows, with how it says them.
const TABLES: [(&str, &str); 8] = [
    ("tasks", "tasks"),
    ("task_links", "links"),
    ("sessions", "sessions"),
    ("roles", "roles"),
    ("messages", "messages"),
    ("deliveries", "deliveries"),
    ("barred_keys", "barred keys"),
    ("task_sessions", "task sessions"),
];
/// The freeze's triggers are this, the table, `_` and the operation.
const FREEZE: &str = "sf_moved_";
/// How long a statement on the old board waits out a write in flight: the old store's own wait.
const BUSY: Duration = Duration::from_secs(30);
/// What a run that stops says of a board it found frozen (a move before it froze it, and did not
/// finish): it leaves it so, never thawing a board it did not freeze.
const STAYS_FROZEN: &str = "the board stays frozen, as this run found it: run tasks-cutover again";
/// Where a move runs when it cannot find a repository.
const IN_THE_REPOSITORY: &str = "run it in the repository whose board it moves";

/// `tasks-cutover`: what it moved goes to `out`, a refusal to `err`; the exit code is returned.
pub fn main(args: &[String], env: &Env, out: &mut dyn io::Write, err: &mut dyn io::Write) -> i32 {
    if !args.is_empty() {
        let _ = writeln!(err, "{USAGE}");
        return 2;
    }
    let moved = board_path(env).and_then(|board| cutover(&board, env).map(|dump| (board, dump)));
    match moved {
        Ok((board, dump)) => {
            let _ = writeln!(
                out,
                "✅ tasks-cutover: {} moved to SurrealDB; {} refuses writes now, kept as it was as a read-only backup",
                counts(&dump),
                board.display()
            );
            0
        }
        Err(why) => {
            let _ = writeln!(err, "❌ tasks-cutover: {why}");
            2
        }
    }
}

/// `TASKS_DB`, else the board in the git common dir every worktree of this repository shares.
pub fn board_path(env: &Env) -> Result<PathBuf, String> {
    if let Some(named) = env.var("TASKS_DB") {
        return Ok(named.into());
    }
    let common = git::common_dir(&env.cwd).map_err(|e| format!("{e}; {IN_THE_REPOSITORY}"))?;
    board_in(&common)
}

/// Move the SQLite board at `board` into the SurrealDB board `env` reaches, replacing what is there:
/// every row it moved.
pub fn cutover(board: &Path, env: &Env) -> Result<Dump, String> {
    cutover_writing(board, env, &write_whole)
}

/// As [`cutover`], with `write` writing the marker's bytes to a new file (a test's full disk).
pub fn cutover_writing(
    board: &Path,
    env: &Env,
    write: &dyn Fn(&Path, &[u8]) -> io::Result<()>,
) -> Result<Dump, String> {
    run(board, env, write, &|| {}, BUSY)
}

/// As [`cutover`], with `meanwhile` run once the board is frozen, before the copy (a test's write
/// landing in SurrealDB then).
pub fn cutover_meanwhile(board: &Path, env: &Env, meanwhile: &dyn Fn()) -> Result<Dump, String> {
    run(board, env, &write_whole, meanwhile, BUSY)
}

/// As [`cutover`], with each statement on the old board waiting `busy` for a write in flight rather
/// than [`BUSY`] (a test's holder outlasting it).
pub fn cutover_waiting(board: &Path, env: &Env, busy: Duration) -> Result<Dump, String> {
    run(board, env, &write_whole, &|| {}, busy)
}

fn run(
    board: &Path,
    env: &Env,
    write: &dyn Fn(&Path, &[u8]) -> io::Result<()>,
    meanwhile: &dyn Fn(),
    busy: Duration,
) -> Result<Dump, String> {
    if !board.is_file() {
        return Err(format!("no SQLite board at {}", board.display()));
    }
    let board = &own_board(board, env)?;
    let _held = lock(board)?;
    refuse_moved(board)?;
    // A board a move before this one froze stays frozen whatever this run does, short of finishing;
    // read on the run's own connection, once it has one.
    let found = std::cell::Cell::new(false);
    let never = |why: String| {
        if found.get() {
            format!("{why}; {STAYS_FROZEN}")
        } else {
            format!("{why}; the board stays in SQLite, never frozen")
        }
    };
    let marker = Marker::of(board);
    marker.sweep();
    let mut old = Old::open(board, busy).map_err(never)?;
    found.set(old.frozen()?);
    let found_frozen = found.get();
    let unknown = old.unknown().map_err(never)?;
    if !unknown.is_empty() {
        return Err(never(format!(
            "the board at {} has tables the move does not know ({}), which it would leave behind",
            board.display(),
            unknown.join(", ")
        )));
    }
    old.rows().map_err(never)?; // a column the copy does not know, before anything is frozen
    let store = cli::board_client(env)
        .and_then(|client| Store::open(client).map_err(|e| e.to_string()))
        .map_err(never)?;
    if let Some(moved) = store.moved().map_err(|e| never(e.to_string()))?
        && moved.written_since
    {
        return Err(written_since(&moved, board));
    }
    store.remove_staging().map_err(|e| never(e.to_string()))?;
    old.freeze().map_err(never)?;
    meanwhile();
    let stopped = match copy(&old, &store, board, env, &marker, write) {
        Ok(dump) => return Ok(dump),
        Err(stopped) => stopped,
    };
    marker.take_away();
    match stopped {
        // SurrealDB took a write after a move into it: the look's refusal, the board as it was.
        Stopped::WrittenSince(why) if found_frozen => Err(why),
        Stopped::WrittenSince(why) | Stopped::Failed(why) if found_frozen => {
            Err(format!("{why}; {STAYS_FROZEN}"))
        }
        Stopped::WrittenSince(why) | Stopped::Failed(why) => match old.thaw() {
            Ok(()) => Err(format!(
                "{why}; the board stays in SQLite, taking writes again"
            )),
            Err(thaw) => Err(format!(
                "{why}; the board at {} could not be thawed ({thaw}), so it refuses writes and nothing takes them: run tasks-cutover again",
                board.display()
            )),
        },
    }
}

/// Why a move stopped once the board was frozen.
enum Stopped {
    /// The copy's own check found the SurrealDB board holding a move and written since: it takes
    /// writes, and the copy changed nothing. Why, as the look before the freeze says it.
    WrittenSince(String),
    Failed(String),
}

impl From<String> for Stopped {
    fn from(why: String) -> Stopped {
        Stopped::Failed(why)
    }
}

/// The frozen board's rows into SurrealDB, checked, then what goes with them, then the marker: each step
/// one a run killed after it does again.
fn copy(
    old: &Old,
    store: &Store,
    board: &Path,
    env: &Env,
    marker: &Marker,
    write: &dyn Fn(&Path, &[u8]) -> io::Result<()>,
) -> Result<Dump, Stopped> {
    let dump = old.rows()?;
    if let Err(refused) = store.move_in(&dump, &board.display().to_string()) {
        return Err(match (&refused, store.moved()) {
            (crate::Error::Refused(_), Ok(Some(moved))) if moved.written_since => {
                Stopped::WrittenSince(written_since(&moved, board))
            }
            _ => Stopped::Failed(refused.to_string()),
        });
    }
    let back = store.dump().map_err(|e| e.to_string())?;
    if let Some(differ) = mismatch(&dump, &back) {
        return Err(format!("the copy does not match the board: {differ}").into());
    }
    if let Some(state) = reap_state(env)? {
        carry(&beside(board, ".reap.json"), &state)?;
    }
    if let Some(folder) = cli::keys_folder(env, &env.cwd) {
        cli::mark_exactly(&folder, &bound_keys(&dump))
            .map_err(|e| format!("cannot mark the bound keys in {}: {e}", folder.display()))?;
    }
    marker.write(write)?;
    Ok(dump)
}

/// `<board><end>`, beside it.
fn beside(board: &Path, end: &str) -> PathBuf {
    let mut name = board.file_name().unwrap_or_default().to_os_string();
    name.push(end);
    board.with_file_name(name)
}

/// `bytes` in a new file at `path`, on the disk before it returns.
fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// The board a move of `board` takes, decided from where the board itself is, wherever the run starts.
/// A repository's own board ([`repository_board`]) moves only into that repository's own SurrealDB
/// board, which a run in that repository reaches with no `TASKS_SERVER`: naming any server, its own
/// included, buys nothing (a move with one makes no bound keys and carries no sweep, and an address file
/// may be stale), and another would freeze the board and mark it moved with its rows elsewhere. It is
/// taken at its place in the common dir, whose lock and marker are the ones the binary and every other
/// move read. Any other board moves only into a server `TASKS_SERVER` names (a test's, a dry run's on a
/// copy), and never by another name of a file (a hard link), which may be a repository's board and is
/// frozen with it. Every refusal comes before anything is locked, frozen or marked.
fn own_board(board: &Path, env: &Env) -> Result<PathBuf, String> {
    let here = git::common_dir(&env.cwd)
        .map_err(|e| format!("{e}; {IN_THE_REPOSITORY}"))
        .and_then(|common| board_in(&common));
    let theirs = repository_board(board, here.as_deref().ok());
    if let Some(named) = env.var("TASKS_SERVER") {
        if let Some(theirs) = theirs {
            return Err(format!(
                "{} is a repository's own board ({}), which moves only into that repository's own SurrealDB board, the one a run there reaches with no TASKS_SERVER: TASKS_SERVER={named} would freeze it and mark it moved with its rows elsewhere (a dry run moves a copy of the board, into a server of its own); unset TASKS_SERVER",
                board.display(),
                theirs.display()
            ));
        }
        return match fs::metadata(board) {
            Ok(file) if file.nlink() > 1 => Err(format!(
                "{} is one of {} names of one file, which may be a repository's board and would be frozen with it: a dry run moves a copy of the board, never a link to it",
                board.display(),
                file.nlink()
            )),
            _ => Ok(board.into()),
        };
    }
    let here = here?;
    match theirs {
        Some(theirs) if same_file(&theirs, &here) => Ok(here),
        _ => Err(format!(
            "{} is not this repository's board ({}): a move goes into this repository's SurrealDB board, which only its own board moves into (a board of anywhere else moves only into a server TASKS_SERVER names)",
            board.display(),
            here.display()
        )),
    }
}

/// Refused when `board` is marked moved: moving it again would lose what was written since.
fn refuse_moved(board: &Path) -> Result<(), String> {
    let moved = beside(board, ".moved");
    if moved.exists() {
        return Err(format!(
            "the board at {} already moved to SurrealDB ({} says so), and moving it again would lose what was written there since",
            board.display(),
            moved.display()
        ));
    }
    Ok(())
}

/// Why a move of `board` into a SurrealDB board holding `moved`, written since, is refused.
fn written_since(moved: &Moved, board: &Path) -> String {
    let (before, after) = crate::store::WRITTEN_SINCE;
    let mut why = format!("{before}{}{after}; nothing changed", moved.from);
    if moved.from == board.display().to_string() {
        let _ = write!(
            why,
            " (the move was this board's, and finished: if {} is gone, put it back)",
            beside(board, ".moved").display()
        );
    }
    why
}

/// Whether a move froze the SQLite board at `board`: its triggers are there. A file that cannot be read
/// as a board is not frozen.
pub(crate) fn frozen(board: &Path) -> bool {
    let found = Connection::open_with_flags(board, OpenFlags::SQLITE_OPEN_READ_ONLY).and_then(|db| {
        db.busy_timeout(Duration::from_secs(1))?;
        db.query_row(
            &format!("SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND name GLOB '{FREEZE}*'"),
            [],
            |row| row.get::<_, i64>(0),
        )
    });
    found.is_ok_and(|triggers| triggers > 0)
}

/// `<board>.cutover`, locked for as long as it is held, this process's pid written in it: the one move
/// into the SurrealDB board `board` stands for that runs ([`home_board`]).
fn lock(board: &Path) -> Result<crate::sys::FileLock, String> {
    let path = beside(board, ".cutover");
    let cannot = |e: io::Error| format!("cannot lock {}: {e}", path.display());
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(cannot)?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            let mut held = String::new();
            let _ = file.read_to_string(&mut held);
            let pid = Some(held.trim())
                .filter(|pid| !pid.is_empty())
                .unwrap_or("not written yet");
            return Err(format!(
                "a move of this board is running (pid {pid}), holding {}: let it end",
                path.display()
            ));
        }
        Err(TryLockError::Error(e)) => return Err(cannot(e)),
    }
    let mut file = crate::sys::FileLock(file);
    file.0
        .set_len(0)
        .and_then(|()| writeln!(file.0, "{}", std::process::id()))
        .map_err(cannot)?;
    Ok(file)
}

/// `<board>.moved`, and the file of this run's own it is written to first.
struct Marker {
    moved: PathBuf,
    fresh: PathBuf,
}

impl Marker {
    fn of(board: &Path) -> Marker {
        Marker {
            moved: beside(board, ".moved"),
            fresh: beside(board, &format!(".moved.{}", std::process::id())),
        }
    }

    /// The marker's own files of runs killed before they renamed theirs into place (`.moved.<pid>`):
    /// under the lock, none is a running move's.
    fn sweep(&self) {
        let (Some(folder), Some(name)) = (self.moved.parent(), self.moved.file_name()) else {
            return;
        };
        let prefix = format!("{}.", name.to_string_lossy());
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let pid = name.strip_prefix(&prefix).unwrap_or_default();
            if !pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit()) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// The marker, whole or not at all: written to a file of its own, then renamed into place.
    fn write(&self, write: &dyn Fn(&Path, &[u8]) -> io::Result<()>) -> Result<(), String> {
        let text = format!(
            "moved to SurrealDB at {}\n",
            sys::local_minute(crate::store::now())
        );
        write(&self.fresh, text.as_bytes())
            .and_then(|()| fs::rename(&self.fresh, &self.moved))
            .map_err(|e| {
                format!(
                    "cannot mark the board moved in {}: {e}",
                    self.moved.display()
                )
            })
    }

    /// No marker, whatever of it a failed run left: the binary takes the board only once the move is
    /// done. Only a move writes it, one at a time, and it was not there when this one began.
    fn take_away(&self) {
        let _ = fs::remove_file(&self.fresh);
        let _ = fs::remove_file(&self.moved);
    }
}

/// The SQLite board, open for the move.
struct Old {
    path: PathBuf,
    db: Connection,
}

impl Old {
    /// The board at `path`, which must be there: it is never made. Each statement waits `busy` for a
    /// write in flight.
    fn open(path: &Path, busy: Duration) -> Result<Old, String> {
        let cannot =
            |e: rusqlite::Error| format!("cannot open the board at {}: {e}", path.display());
        let db =
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE).map_err(cannot)?;
        db.busy_timeout(busy).map_err(cannot)?;
        Ok(Old {
            path: path.into(),
            db,
        })
    }

    fn cannot(&self, why: &str) -> String {
        format!("cannot read the board at {}: {why}", self.path.display())
    }

    /// Whether a move froze the board: its triggers are there, read on this run's own connection (its wait
    /// for a write in flight), and an error is a refusal, never "not frozen".
    fn frozen(&self) -> Result<bool, String> {
        self.db
            .query_row(
                &format!("SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND name GLOB '{FREEZE}*'"),
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|triggers| triggers > 0)
            .map_err(|e| format!("{}; nothing changed", self.cannot(&e.to_string())))
    }

    /// The tables of the board the move does not know, which it would leave behind.
    fn unknown(&self) -> Result<Vec<String>, String> {
        let mut found = self
            .db
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<String>, _>>()
            })
            .map_err(|e| self.cannot(&e.to_string()))?;
        found.retain(|name| {
            !name.starts_with("sqlite_") && !TABLES.iter().any(|(table, _)| table == name)
        });
        found.sort();
        Ok(found)
    }

    /// Every row of every table, read in one transaction, refused whole if the copy would drop any of it.
    fn rows(&self) -> Result<Dump, String> {
        let text = |e: rusqlite::Error| self.cannot(&e.to_string());
        let read = self.db.unchecked_transaction().map_err(text)?;
        let mut tables = Map::new();
        for (table, _) in TABLES {
            let rows = rows_of(&read, table).map_err(|e| self.cannot(&e))?;
            tables.insert(table.into(), Value::Array(rows));
        }
        read.commit().map_err(text)?;
        Dump::from_tables(tables)
    }

    /// A trigger on every table that refuses every write, saying where the board went; a board a killed
    /// move froze is frozen again as it was. `BEGIN IMMEDIATE` waits out a write in flight, and none
    /// starts after it.
    fn freeze(&mut self) -> Result<(), String> {
        let path = self.path.display().to_string();
        let cannot = |e: rusqlite::Error| format!("cannot freeze the board at {path}: {e}");
        let freeze = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(cannot)?;
        for (table, _) in TABLES {
            for op in ["INSERT", "UPDATE", "DELETE"] {
                let lower = op.to_lowercase();
                freeze
                    .execute_batch(&format!(
                        "CREATE TRIGGER IF NOT EXISTS {FREEZE}{table}_{lower} BEFORE {op} ON {table} BEGIN SELECT RAISE(ABORT, '{FROZEN}'); END;"
                    ))
                    .map_err(cannot)?;
            }
        }
        freeze.commit().map_err(cannot)
    }

    fn thaw(&mut self) -> Result<(), rusqlite::Error> {
        let thaw = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let triggers = thaw
            .prepare("SELECT name FROM sqlite_master WHERE type = 'trigger'")?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<String>, _>>()?;
        for name in triggers.iter().filter(|name| name.starts_with(FREEZE)) {
            thaw.execute_batch(&format!("DROP TRIGGER \"{name}\";"))?;
        }
        thaw.commit()
    }
}

/// `table`'s rows in the order they were written, each an object keyed by its columns' names.
fn rows_of(db: &Connection, table: &str) -> Result<Vec<Value>, String> {
    let text = |e: rusqlite::Error| e.to_string();
    let mut statement = db
        .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
        .map_err(text)?;
    let names: Vec<String> = statement
        .column_names()
        .into_iter()
        .map(String::from)
        .collect();
    let mut rows = statement.query([]).map_err(text)?;
    let mut read = Vec::new();
    while let Some(row) = rows.next().map_err(text)? {
        let mut object = Map::new();
        for (i, name) in names.iter().enumerate() {
            let value = row.get_ref(i).map_err(text)?;
            object.insert(name.clone(), json(value, table, name)?);
        }
        read.push(Value::Object(object));
    }
    Ok(read)
}

/// A value of the board as JSON holds it; one it cannot hold is refused, naming where it is.
fn json(value: ValueRef<'_>, table: &str, column: &str) -> Result<Value, String> {
    let refused =
        |what: &str| format!("{table}.{column} holds {what}, which the copy cannot carry");
    Ok(match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(n) => n.into(),
        ValueRef::Real(x) => serde_json::Number::from_f64(x)
            .map(Value::Number)
            .ok_or_else(|| refused("a number that is not finite"))?,
        ValueRef::Text(text) => std::str::from_utf8(text)
            .map_err(|_| refused("text that is not UTF-8"))?
            .into(),
        ValueRef::Blob(_) => return Err(refused("bytes")),
    })
}

/// How the two boards differ, table by table; None when every table holds the same rows.
fn mismatch(old: &Dump, new: &Dump) -> Option<String> {
    let (old, new) = (
        serde_json::to_value(old).ok()?,
        serde_json::to_value(new).ok()?,
    );
    let rows = |board: &Value, table: &str| -> Vec<String> {
        let mut rows: Vec<String> = board[table]
            .as_array()
            .map(|rows| rows.iter().map(Value::to_string).collect())
            .unwrap_or_default();
        rows.sort();
        rows
    };
    let differ: Vec<String> = TABLES
        .iter()
        .filter_map(|(table, _)| {
            let (a, b) = (rows(&old, table), rows(&new, table));
            let both = common(&a, &b);
            (both != a.len() || both != b.len()).then(|| {
                format!(
                    "{table} ({} rows in SQLite, {} in SurrealDB, {} not in both)",
                    a.len(),
                    b.len(),
                    a.len() + b.len() - 2 * both
                )
            })
        })
        .collect();
    (!differ.is_empty()).then(|| differ.join("; "))
}

/// How many rows two sorted lists share, a row as often as both hold it.
fn common(a: &[String], b: &[String]) -> usize {
    let (mut i, mut j, mut both) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                both += 1;
                i += 1;
                j += 1;
            }
        }
    }
    both
}

/// Where the idle sweep keeps its state (`crate::launch::reap`): `TASKS_REAP_STATE`; for a board
/// `TASKS_SERVER` names without it (a test's), nowhere the move writes; else `sf-tasks/reap.json` in the
/// git common dir, beside the board's server.
fn reap_state(env: &Env) -> Result<Option<PathBuf>, String> {
    if let Some(named) = env.var("TASKS_REAP_STATE") {
        return Ok(Some(named.into()));
    }
    if env.var("TASKS_SERVER").is_some() {
        return Ok(None);
    }
    let common = git::common_dir(&env.cwd).map_err(|e| e.to_string())?;
    Ok(Some(common.join(server::FOLDER).join("reap.json")))
}

/// The file at `from`, if there is one, to `to`, whole.
fn carry(from: &Path, to: &Path) -> Result<(), String> {
    let cannot = |e: io::Error| format!("cannot carry {} to {}: {e}", from.display(), to.display());
    let data = match fs::read(from) {
        Ok(data) => data,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(cannot(e)),
    };
    if let Some(folder) = to.parent() {
        fs::create_dir_all(folder).map_err(cannot)?;
    }
    let fresh = beside(to, &format!(".{}", std::process::id()));
    fs::write(&fresh, data)
        .and_then(|()| fs::rename(&fresh, to))
        .map_err(cannot)
}

/// The keys the board has bound to a session that has not left.
fn bound_keys(dump: &Dump) -> Vec<String> {
    let mut keys: Vec<String> = dump
        .sessions
        .iter()
        .filter(|session| session.left_at.is_none())
        .filter_map(|session| session.agent.clone())
        .filter(|key| !key.is_empty())
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// `2 tasks, 2 links, …`: how many rows of each table moved.
fn counts(dump: &Dump) -> String {
    let moved = [
        dump.tasks.len(),
        dump.task_links.len(),
        dump.sessions.len(),
        dump.roles.len(),
        dump.messages.len(),
        dump.deliveries.len(),
        dump.barred_keys.len(),
        dump.task_sessions.len(),
    ];
    TABLES
        .iter()
        .zip(moved)
        .map(|((_, said), n)| format!("{n} {said}"))
        .collect::<Vec<_>>()
        .join(", ")
}
