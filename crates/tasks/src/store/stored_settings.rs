//! The board's store of preferences ([`crate::settings`]): one row a setting, `setting:⟨key⟩`,
//! holding its value and every change made to it, in the table an additive step of this contract
//! defines ([`crate::schema::ADDED`]). A board without the step reads as one with no entry, and a
//! write there is refused before anything is sent.
//!
//! A write judges its value first ([`crate::settings::judged`]) and writes nothing it refuses.
//! It reads the entry, decides in the process and writes the row whole, fenced on the revision it
//! read ([`pg::write`]): two writes of one setting meet on its row, and the one that lost reads
//! and decides again, so no change is lost from the record.

use super::postgres::stored_settings as pg;
use super::statement::Statement;
use super::{Error, Failure, Store, refused};
use crate::schema;
use crate::settings::{Entry, SAID, Stored, judged, kept, named};
use serde_json::{Value, json};

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
            let read = Statement::from(pg::ENTRIES);
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
        change: impl Fn(Option<&Entry>) -> Result<Option<Value>, Error>,
    ) -> Result<(Entry, bool), Error> {
        self.keeping_settings()?;
        for _ in 0..TRIES {
            let found = (self.stored_entries()?.into_iter()).find(|entry| entry.key == key);
            let after = change(found.as_ref())?;
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
                    Statement::from(pg::write()).naming(&[TABLE]),
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
    /// Refused, with nothing written: a value [`judged`] refuses, naming the key and why, and an
    /// empty `by`.
    pub fn store_setting(
        &self,
        key: &str,
        value: &toml::Value,
        by: &str,
    ) -> Result<(Entry, bool), Error> {
        judged(key, value).map_err(|why| refused(format!("{why}: nothing was written")))?;
        let by = writer(by)?;
        let after = kept(value);
        self.change_setting(key, by, |_| Ok(Some(after.clone())))
    }

    /// Removes the entry of the setting `key` as `by`'s change: its record is kept, it holds no
    /// value, and the files' value is in force again. Any entry may be removed, one the store
    /// would not take today too. Refused where no entry holds a value for it.
    pub fn remove_setting(&self, key: &str, by: &str) -> Result<Entry, Error> {
        let by = writer(by)?;
        let removed = self.change_setting(key, by, |found| {
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
