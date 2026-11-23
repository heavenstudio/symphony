//! The board's store of preferences ([`crate::settings`]): one row a setting, `setting:⟨key⟩`,
//! holding its value and every change made to it, in the table an additive step of this contract
//! defines ([`crate::schema::ADDED`]). A board without the step reads as one with no entry, and a
//! write there is refused before anything is sent.
//!
//! A write judges its value first ([`crate::settings::judged`]) and writes nothing it refuses.
//! It reads the entry, decides in the process and writes the row whole, fenced on the revision it
//! read ([`sql::WRITE`]): two writes of one setting meet on its row, and the one that lost reads
//! and decides again, so no change is lost from the record. A switch that would turn off the
//! launch default the board has with it, the store's own over the project file's, is refused on
//! what that read found ([`off_refused`]).

use super::postgres::stored_settings as pg;
use super::statement::both;
use super::surreal::stored_settings as sql;
use super::{Error, Failure, Store, refused};
use crate::launch::defaults::{self, AGENT_KEY, AGENTS, ON_TABLES};
use crate::schema;
use crate::settings::{Entry, SAID, Stored, judged, kept, named, path};
use serde_json::{Value, json};
use std::path::Path;

/// The table the entries are rows of.
pub const TABLE: &str = "setting";

/// How many times a write reads and decides again after another write of its setting passed it.
const TRIES: usize = 8;

/// The additive step that defines [`TABLE`], counted from 1: 0 once a breaking step made the
/// table the contract's own, which every board of it has.
fn step() -> i64 {
    let at = schema::ADDED
        .iter()
        .position(|step| step.tables.contains(&TABLE));
    at.map_or(0, |at| i64::try_from(at + 1).unwrap_or(i64::MAX))
}

impl Store {
    /// Every stored entry, by key, those removed with them: one read, which finds none on a
    /// board without the step.
    pub fn stored_entries(&self) -> Result<Vec<Entry>, Error> {
        self.retrying(|| {
            let read = both(sql::ENTRIES, pg::ENTRIES);
            let read = self.request(read, json!({})).map(Some);
            let Some(found) = self.absent_board(read, None)? else {
                return Ok(Vec::new());
            };
            let entries = found.last().cloned().unwrap_or(Value::Null);
            crate::objects::read(&entries)
                .map_err(|e| Failure::Stop(Error::Db(format!("a stored setting: {e}"))))
        })
    }

    /// What the store's entries write over the committed settings, as every reader of the
    /// settings takes it: a store that cannot be read is said and none of it read.
    pub fn stored(&self) -> Stored {
        match self.stored_entries() {
            Ok(entries) => Stored::of(&entries),
            Err(why) => Stored::unread(&why.to_string()),
        }
    }

    /// Refused on a board that lacks the step the store's table comes with.
    fn keeping_settings(&self) -> Result<(), Error> {
        if self.additions()? < step() {
            return Err(refused(format!(
                "{SAID} is not on this board yet: its schema lacks the step that adds it, which \
                 the project's own build of the board adds at its next command: nothing was written"
            )));
        }
        Ok(())
    }

    /// `key`'s entry after `change` decided what it holds from what it held, written with the
    /// change recorded as `by`'s at this store's clock; unchanged, and nothing written, where
    /// `change` gives what it holds already.
    fn change_setting(
        &self,
        key: &str,
        by: &str,
        change: impl Fn(Option<&Entry>, &[Entry]) -> Result<Option<Value>, Error>,
    ) -> Result<(Entry, bool), Error> {
        self.keeping_settings()?;
        for _ in 0..TRIES {
            let entries = self.stored_entries()?;
            let found = entries.iter().find(|entry| entry.key == key).cloned();
            let after = change(found.as_ref(), &entries)?;
            let before = found.unwrap_or_else(|| Entry {
                key: key.to_string(),
                ..Entry::default()
            });
            if before.value == after {
                return Ok((before, false));
            }
            let written = before.changed(after, self.now(), by);
            let vars = json!({
                "key": key,
                "expected": before.revision,
                "doc": serde_json::to_value(&written).expect("an entry is JSON"),
            });
            let wrote: bool = self.retrying(|| {
                self.transact_dispatch(
                    both(sql::WRITE, pg::write()).naming(&[TABLE]),
                    vars.clone(),
                    false,
                )
            })?;
            if wrote {
                return Ok((written, true));
            }
        }
        Err(refused(format!(
            "{SAID}: {} changed under this write {TRIES} times: nothing was written by it",
            named(key)
        )))
    }

    /// Stores `value` for the setting `key` as `by`'s change, in force from the next read of the
    /// settings: the entry, and whether it changed (a value it held already writes nothing).
    /// Refused, with nothing written: a value [`judged`] refuses, naming the key and why; a switch
    /// that turns off the launch default the board has with it, the store's own `default` over
    /// the one `project_file` (the main checkout's) names ([`off_refused`]); and an empty `by`.
    pub fn store_setting(
        &self,
        key: &str,
        value: &toml::Value,
        by: &str,
        project_file: &Path,
    ) -> Result<(Entry, bool), Error> {
        judged(key, value).map_err(|why| refused(format!("{why}: nothing was written")))?;
        let by = writer(by)?;
        let after = kept(value);
        self.change_setting(key, by, |_, entries| {
            if let Some(why) = off_refused(key, &after, entries, project_file) {
                return Err(refused(format!("{why}: nothing was written")));
            }
            Ok(Some(after.clone()))
        })
    }

    /// Removes the entry of the setting `key` as `by`'s change: its record is kept, it holds no
    /// value, and the files' value is in force again. Any entry may be removed, one the store
    /// would not take today too. Refused where no entry holds a value for it.
    pub fn remove_setting(&self, key: &str, by: &str) -> Result<Entry, Error> {
        let by = writer(by)?;
        let removed = self.change_setting(key, by, |found, _| {
            if found.is_none_or(|entry| entry.value.is_none()) {
                return Err(refused(format!(
                    "{SAID}: {} has no entry: nothing to remove, the files' value holds",
                    named(key)
                )));
            }
            Ok(None)
        })?;
        Ok(removed.0)
    }

    /// Removes the entry of the setting `key` as `by`'s change, as [`Store::remove_setting`] does,
    /// only while it holds `value`: what follows a commit that made that value the project's, so
    /// a value stored again since stays. Whether it removed it; an entry holding another value, or
    /// none, is left as it is and nothing written.
    pub fn remove_setting_holding(
        &self,
        key: &str,
        value: &Value,
        by: &str,
    ) -> Result<bool, Error> {
        let by = writer(by)?;
        let (_, removed) = self.change_setting(key, by, |found, _| {
            Ok(found
                .and_then(|entry| entry.value.clone())
                .filter(|held| held != value))
        })?;
        Ok(removed)
    }
}

/// Why `after`, written for `key`, is refused where it is a switch turning off the launch default
/// the board has with `entries`, the store's: its `default` in force, else the one the project file
/// at `project_file` names, else the board's own. None for any other write: a `default` naming an
/// agent a switch turns off is taken, the default winning over the switch
/// ([`defaults::Defaults::overruled`]).
fn off_refused(key: &str, after: &Value, entries: &[Entry], project_file: &Path) -> Option<String> {
    let (tables, agent) = path(key).ok()?;
    if tables != ON_TABLES || after != &Value::Bool(false) {
        return None;
    }
    let stored = Stored::of(entries);
    let in_store = stored
        .holds("agents.launch.default")
        .then(|| defaults::agent(AGENT_KEY, stored.held.launch[0].as_deref()).ok())
        .flatten()
        .flatten();
    let text = std::fs::read_to_string(project_file).unwrap_or_default();
    let (own, chosen) = match (in_store, defaults::written_agent(&text)) {
        (Some(own), _) => (own, format!("{SAID} {AGENT_KEY}")),
        (None, Some(own)) => {
            let line = crate::limits::line(&text, &["agents", "launch"], "default");
            let at = crate::limits::Written {
                at: format!("{} {AGENT_KEY}", crate::project::FILE),
                line,
            };
            (own, at.said())
        }
        (None, None) => (AGENTS[0], "the board's own".to_string()),
    };
    (agent == own).then(|| {
        format!(
            "{SAID}: {}",
            defaults::default_off(&named(key), own, &chosen)
        )
    })
}

/// `by`, who a change is recorded as made by: refused empty.
fn writer(by: &str) -> Result<&str, Error> {
    let by = by.trim();
    if by.is_empty() {
        return Err(refused(format!(
            "{SAID}: a change says who made it (the session or the page): nothing was written"
        )));
    }
    Ok(by)
}
