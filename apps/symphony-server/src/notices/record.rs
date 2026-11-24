//! The project's record of its notices, `notices.json` under its artifacts folder
//! ([`symphony::notices::RECORD`]): what the runner looked at, what it told, and each notice not
//! settled yet, so a restart neither tells an event again nor loses a notice it was to hand again.
//! It holds no address: a notice keeps a digest of the address it goes to under the machine's key
//! ([`symphony::notices::Key`]), and the address is read again from the person's profile at each
//! hand.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{ErrorKind, Read as _};
use std::path::Path;

/// What the runner keeps of a project's notices between two looks, and across a restart.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Record {
    /// The newest board line looked at: a question or a verdict after it is not told yet.
    pub line: i64,
    /// The cards waiting at the last look, by `DOC#SLUG`, each with who was told of it while it
    /// waits. A card that stops waiting leaves, so waiting again tells again.
    #[serde(default)]
    pub cards: BTreeMap<String, BTreeSet<String>>,
    /// The notices not settled yet, in the order they were made.
    #[serde(default)]
    pub pending: Vec<Pending>,
}

/// One notice to one person by one way, not settled yet: handed again with every field the same.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Pending {
    /// Its id, unique on the server and never given to another notice.
    pub id: String,
    /// Its kind's word.
    pub kind: String,
    /// The page it is about.
    pub link: String,
    /// The person told, by short name.
    pub person: String,
    /// The way's word.
    pub way: String,
    /// The digest of the address it goes to under the machine's key
    /// ([`symphony::notices::Key::digest`]): never the address.
    pub digest: String,
    /// How many times its program was started for it.
    pub hands: u32,
    /// When it is next handed, in seconds since the epoch.
    pub due: f64,
    /// Whether its program was started and its end not yet read: a hand a restart cut, handed
    /// again at once with the same id.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub handing: bool,
}

impl Record {
    /// The record at `path`: none where there is no file yet.
    ///
    /// # Errors
    ///
    /// When it cannot be read or does not parse, said by its path and the fault.
    pub(crate) fn read(path: &Path) -> Result<Option<Record>, String> {
        let mut text = String::new();
        match std::fs::File::open(path).and_then(|mut file| file.read_to_string(&mut text)) {
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(symphony::text::os_error(&error, path)),
            Ok(_) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|error| format!("{}: {error}", path.display())),
        }
    }

    /// Writes it at `path` whole, so a reader never sees half of it.
    ///
    /// # Errors
    ///
    /// When its folder or the file cannot be written.
    pub(crate) fn write(&self, path: &Path) -> Result<(), String> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(|e| symphony::text::os_error(&e, folder))?;
        }
        let next = path.with_extension(format!("json.{}", std::process::id()));
        let text = serde_json::to_string_pretty(self).expect("a record serializes") + "\n";
        let written = std::fs::write(&next, text).and_then(|()| std::fs::rename(&next, path));
        if written.is_err() {
            let _ = std::fs::remove_file(&next);
        }
        written.map_err(|e| symphony::text::os_error(&e, path))
    }
}

/// A new notice id: 32 hex digits of the machine's randomness, never given twice.
pub(crate) fn new_id() -> String {
    let mut bytes = [0u8; 16];
    let read = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
    if read.is_err() {
        // No randomness to read: the time, the process and a count, which no other notice of this
        // machine shares.
        static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let count = COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        return format!("{nanos:x}{:x}{count:x}", std::process::id());
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
