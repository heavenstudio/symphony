//! Save-time chat outbox, beside the ephemeral asks. A board outage cannot discard a saved owner
//! message. The shared server retries these records; it never scans documents to discover chat.

use crate::asks::{AskStore, text_sql};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// New ask stores include the outbox, so a chat save needs no second schema setup.
pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS callbacks (id INTEGER PRIMARY KEY AUTOINCREMENT, text TEXT NOT NULL, ask TEXT);";

// A receipt key contains this table's id, so it must never be reused after a flush. Check the
// required columns and declaration, allowing additional columns for newer writers. Publication
// also proves actual sequence advancement: a word in a default or comment is not AUTOINCREMENT.
const SHAPE: &str = r#"SELECT
    (SELECT count(*) FROM pragma_table_info('callbacks') WHERE name = 'id' AND upper(type) = 'INTEGER' AND pk = 1) = 1
    AND (SELECT count(*) FROM pragma_table_info('callbacks') WHERE pk > 0) = 1
    AND (SELECT count(*) FROM pragma_table_info('callbacks') WHERE name = 'text' AND upper(type) = 'TEXT' AND "notnull" = 1 AND pk = 0) = 1
    AND EXISTS (SELECT 1 FROM sqlite_schema WHERE name = 'callbacks' AND type = 'table' AND instr(upper(sql), 'AUTOINCREMENT') > 0) AS core,
    (SELECT count(*) FROM pragma_table_info('callbacks') WHERE name = 'ask' AND upper(type) = 'TEXT' AND "notnull" = 0 AND pk = 0) = 1 AS asking,
    (SELECT count(*) FROM pragma_table_info('callbacks') WHERE name = 'ask') = 0 AS legacy"#;
const SCHEMA_GUARD: &str = "callback_schema_ready";

#[derive(serde::Deserialize)]
struct Shape {
    core: i64,
    asking: i64,
    legacy: i64,
}

fn shape(connection: &AskStore) -> Result<Shape, String> {
    connection
        .rows::<Shape>(SHAPE)
        .map_err(|e| e.to_string())?
        .pop()
        .ok_or_else(|| "chat journal schema check returned no result".into())
}

fn invalid_schema(path: &Path) -> String {
    format!(
        "chat journal {}: callbacks needs an AUTOINCREMENT INTEGER primary key id, non-null TEXT text and nullable TEXT ask; restore or repair this journal before retrying",
        path.display()
    )
}

/// The journal of the project at `root`, where this process's `ASKS_DB` puts it, else in the
/// project's artifacts folder ([`crate::asks::default_path`]).
fn journal(root: &Path) -> PathBuf {
    crate::asks::default_path(root, std::env::var("ASKS_DB").ok().as_deref())
}

/// The journal at `path`, opened and brought to its columns, and the key its records' events are
/// named by.
fn database(path: &Path) -> Result<(AskStore, String), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let connection = AskStore::open(path).map_err(|e| e.to_string())?;
    // A journal made before asks has no `ask` column: it gains one, its records keeping none.
    let columns = shape(&connection)?;
    if columns.core != 1 || (columns.asking != 1 && columns.legacy != 1) {
        return Err(invalid_schema(path));
    }
    if columns.legacy == 1 {
        match connection.run("ALTER TABLE callbacks ADD COLUMN ask TEXT;") {
            Ok(_) => {}
            // Another publisher can finish the same additive migration after our column read.
            Err(crate::asks::AskError::Store(why))
                if why.contains("duplicate column name: ask") => {}
            Err(error) => return Err(error.to_string()),
        }
        if shape(&connection)?.asking != 1 {
            return Err(invalid_schema(path));
        }
    }
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let key = Sha256::digest(path.to_string_lossy().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok((connection, key))
}

pub fn publish(root: &Path, text: &str) -> Result<(), String> {
    publish_asking(root, text, None)
}

/// [`publish`], with the ask the save opens or joins ([`symphony_board::NewAsk`]): the shared
/// server hands both to the board in one write ([`symphony_board::Store::owner_save`]). Empty `text`
/// delivers no chat and only opens the ask.
pub fn publish_asking(
    root: &Path,
    text: &str,
    ask: Option<&symphony_board::NewAsk>,
) -> Result<(), String> {
    let path = journal(root);
    publish_at(&path, text, ask)
}

fn publish_at(path: &Path, text: &str, ask: Option<&symphony_board::NewAsk>) -> Result<(), String> {
    let ask = match ask {
        Some(ask) => text_sql(&serde_json::to_string(ask).map_err(|e| e.to_string())?),
        None => "NULL".into(),
    };
    // Schema validation and insertion share one process and writer transaction. A file's
    // presence alone says nothing about whether its outbox can retain a replayable receipt.
    let insert = format!(
        "BEGIN IMMEDIATE;\nCREATE TEMP TABLE callback_guard (ok INTEGER NOT NULL CONSTRAINT {SCHEMA_GUARD} CHECK (ok = 1), prior INTEGER);\nINSERT INTO callback_guard SELECT core AND asking, coalesce((SELECT seq FROM main.sqlite_sequence WHERE name = 'callbacks'), 0) FROM ({SHAPE});\nINSERT INTO callbacks(text, ask) VALUES ({}, {ask});\nINSERT INTO callback_guard(ok) SELECT EXISTS (SELECT 1 FROM main.sqlite_sequence WHERE name = 'callbacks' AND seq >= last_insert_rowid() AND last_insert_rowid() > (SELECT prior FROM callback_guard LIMIT 1));\nCOMMIT;",
        text_sql(text)
    );
    if let Some(connection) = AskStore::existing(path) {
        match connection.run(&insert) {
            Ok(_) => return Ok(()),
            // A failed guard commits no intent: sqlite3's -bail closes and rolls back this
            // transaction, including when the sequence check follows the outbox INSERT.
            // Initialization handles an absent table or its legacy column.
            Err(error) if missing_schema(&error) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    let (connection, _) = database(path)?;
    connection.run(&insert).map_err(|error| {
        if missing_schema(&error) {
            invalid_schema(path)
        } else {
            error.to_string()
        }
    })?;
    // Native delivery belongs to the server clock, outside the save lock and HTTP writer.
    Ok(())
}

fn missing_schema(error: &crate::asks::AskError) -> bool {
    matches!(error, crate::asks::AskError::Store(why)
        if why.contains(SCHEMA_GUARD) || why.contains("no such table: main.sqlite_sequence"))
}

/// The shared, lease-holding server's journal reader. Private servers leave the journal alone,
/// even with an explicit ASKS_DB: they must not deliver owner chat to a preview's board.
/// An unchanged empty SQLite database (including its WAL) needs no
/// sqlite3 child at all. Compare the stamp from before the read: a concurrent publish then always
/// makes the next tick read again, including when a different process wrote the journal.
/// It reads the journal its project had when it was made, never one found from the folder again,
/// which a folder left without its `.git` would find in the checkout that holds it.
pub struct Flusher {
    path: PathBuf,
    empty: Option<Stamp>,
}

#[derive(PartialEq)]
struct Stamp(Vec<Option<(SystemTime, u64)>>);

impl Flusher {
    pub fn new(root: &Path) -> Self {
        Self {
            path: journal(root),
            empty: None,
        }
    }

    /// Hand the journal's records to the board of the project at `root`, each deleted once the
    /// board holds it.
    pub fn flush(&mut self, root: &Path) -> Result<(), String> {
        let stamp = self.stamp()?;
        if self.empty.as_ref() == Some(&stamp) {
            return Ok(());
        }
        self.empty = None;
        if flush(&self.path, root)? {
            self.empty = Some(stamp);
        }
        Ok(())
    }

    fn stamp(&self) -> Result<Stamp, String> {
        let mut wal = self.path.as_os_str().to_os_string();
        wal.push("-wal");
        [self.path.clone(), PathBuf::from(wal)]
            .into_iter()
            .map(|path| match std::fs::metadata(&path) {
                Ok(meta) => meta
                    .modified()
                    .map(|at| Some((at, meta.len())))
                    .map_err(|e| format!("{}: {e}", path.display())),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(format!("{}: {e}", path.display())),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Stamp)
    }
}

/// Hand the records of the journal at `path` to the board of the project at `root`: whether the
/// journal was empty.
fn flush(path: &Path, root: &Path) -> Result<bool, String> {
    let (connection, key) = database(path)?;
    #[derive(serde::Deserialize)]
    struct Record {
        id: i64,
        text: String,
        ask: Option<String>,
    }
    let records: Vec<Record> = connection
        .rows("SELECT id, text, ask FROM callbacks ORDER BY id;")
        .map_err(|e| e.to_string())?;
    if records.is_empty() {
        return Ok(true);
    }
    let store = crate::taskview::connect(root)?;
    for Record { id, text, ask } in records {
        let ask: Option<symphony_board::NewAsk> = ask
            .map(|ask| serde_json::from_slice(ask.as_bytes()))
            .transpose()
            .map_err(|e| format!("journal record {id}'s ask: {e}"))?;
        let event = format!("{key}-{id}");
        match store.owner_save(&event, &text, ask.as_ref()) {
            // An ask the board refuses would hold the journal for good: the chat still goes.
            Err(symphony_board::Error::Refused(why)) if ask.is_some() => {
                eprintln!("ASK not opened for journal record {id}: {why}");
                store
                    .owner_save(&event, &text, None)
                    .map_err(|e| e.to_string())?;
            }
            done => done.map_err(|e| e.to_string())?,
        }
        connection
            .run(&format!("DELETE FROM callbacks WHERE id = {id};"))
            .map_err(|e| e.to_string())?;
    }
    // Read once more next tick before caching empty; a concurrent publisher may have inserted.
    Ok(false)
}

#[cfg(test)]
#[path = "callbacks_tests.rs"]
mod tests;
