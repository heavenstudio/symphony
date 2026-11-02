//! Save-time chat outbox, beside the ephemeral asks. A board outage cannot discard a saved owner
//! message. The shared server retries these records; it never scans documents to discover chat.

use crate::asks::{AskStore, text_sql};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

fn database(root: &Path) -> Result<(AskStore, String), String> {
    let path = crate::asks::default_path(root, std::env::var("ASKS_DB").ok().as_deref());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let connection = AskStore::open(&path).map_err(|e| e.to_string())?;
    connection.run("CREATE TABLE IF NOT EXISTS callbacks (id INTEGER PRIMARY KEY AUTOINCREMENT, text TEXT NOT NULL);").map_err(|e| e.to_string())?;
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let key = Sha256::digest(path.to_string_lossy().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok((connection, key))
}

pub fn publish(root: &Path, text: &str) -> Result<(), String> {
    let (connection, _) = database(root)?;
    connection
        .run(&format!(
            "INSERT INTO callbacks(text) VALUES ({});",
            text_sql(text)
        ))
        .map_err(|e| e.to_string())?;
    // Native delivery belongs to the server clock, outside the save lock and HTTP writer.
    Ok(())
}

/// The shared, lease-holding server's journal reader. Private servers leave the journal alone,
/// even with an explicit ASKS_DB: they must not deliver owner chat to a preview's board.
/// An unchanged empty SQLite database (including its WAL) needs no
/// sqlite3 child at all. Compare the stamp from before the read: a concurrent publish then always
/// makes the next tick read again, including when a different process wrote the journal.
pub struct Flusher {
    path: PathBuf,
    empty: Option<Stamp>,
}

#[derive(PartialEq)]
struct Stamp(Vec<Option<(SystemTime, u64)>>);

impl Flusher {
    pub fn new(root: &Path) -> Self {
        Self {
            path: crate::asks::default_path(root, std::env::var("ASKS_DB").ok().as_deref()),
            empty: None,
        }
    }

    pub fn flush(&mut self, root: &Path) -> Result<(), String> {
        let stamp = self.stamp()?;
        if self.empty.as_ref() == Some(&stamp) {
            return Ok(());
        }
        self.empty = None;
        if flush(root)? {
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

fn flush(root: &Path) -> Result<bool, String> {
    let (connection, key) = database(root)?;
    #[derive(serde::Deserialize)]
    struct Record {
        id: i64,
        text: String,
    }
    let records: Vec<Record> = connection
        .rows("SELECT id, text FROM callbacks ORDER BY id;")
        .map_err(|e| e.to_string())?;
    if records.is_empty() {
        return Ok(true);
    }
    let store = crate::taskview::connect(root)?;
    for Record { id, text } in records {
        store
            .owner_callback(&format!("{key}-{id}"), &text)
            .map_err(|e| e.to_string())?;
        connection
            .run(&format!("DELETE FROM callbacks WHERE id = {id};"))
            .map_err(|e| e.to_string())?;
    }
    // Read once more next tick before caching empty; a concurrent publisher may have inserted.
    Ok(false)
}
