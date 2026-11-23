//! What the Settings page reads of the board's store ([`symphony_board::settings`]): its entries,
//! read after the project file and before the local file, and the saves of the project file kept
//! for delivery by pull request ([`crate::commit::pending`]). From them the page names a stored
//! value's source, and lists what it writes ([`list`]): one item for each setting it draws a
//! control for, those whose scopes hold [`SettingScope::Board`], with the store's entry for it,
//! its record and a commit of it still waiting.
//!
//! A setting the page draws is named by its key as the project file's lines name it
//! (`[agents] launch model`), and its key in the store is the one [`symphony_board::settings::named`]
//! says that way ([`store_key`]): the page holds no list of its own of what the store keeps.

use std::path::Path;

use serde_json::Value as Json;
use symphony_api::{SettingScope, SettingsSection, StoredChange, StoredSetting};
use symphony_board::local::Local;
use symphony_board::settings::{Change, Entry, Stored, named};

use crate::commit::pending;
use crate::project::FILE;

/// What the board holds for the page: the store's entries, or why it could not be read, and the
/// saves of the project file waiting for delivery by pull request.
#[derive(Clone, Debug)]
pub struct Board {
    pub entries: Result<Vec<Entry>, String>,
    pub waiting: Vec<pending::Save>,
}

impl Default for Board {
    /// A board never told anything: no entry, nothing waiting.
    fn default() -> Board {
        Board {
            entries: Ok(Vec::new()),
            waiting: Vec::new(),
        }
    }
}

impl Board {
    /// The board of the project at `root`, as the server reads it for the page.
    pub fn of(root: &Path) -> Board {
        let entries = crate::taskview::connect(root)
            .and_then(|store| store.stored_entries().map_err(|e| e.to_string()));
        Board {
            entries,
            waiting: pending::saves(root)
                .into_iter()
                .filter(|save| save.file == FILE)
                .collect(),
        }
    }

    /// What the entries write over the committed settings, as every reader of the settings takes
    /// them: a store that could not be read is said, and none of it read.
    pub fn stored(&self) -> Stored {
        match &self.entries {
            Ok(entries) => Stored::of(entries),
            Err(why) => Stored::unread(why),
        }
    }
}

/// The key in the store of the setting the page names `at` (`[agents] launch model`:
/// `agents.launch.model`), the one [`named`] says as `at`; none for a name no key is said as.
pub fn store_key(at: &str) -> Option<String> {
    let (tables, key) = at.strip_prefix('[')?.split_once("] ")?;
    let key = match (tables, key.strip_prefix("launch ")) {
        ("agents", Some(launch)) => format!("agents.launch.{launch}"),
        _ => format!("{tables}.{key}"),
    };
    (named(&key) == at).then_some(key)
}

/// A stored value as the page shows a value: text as it is, a whole number, on or off.
pub fn shown(value: &Json) -> String {
    match value {
        Json::String(text) => text.clone(),
        Json::Bool(true) => "on".into(),
        Json::Bool(false) => "off".into(),
        other => other.to_string(),
    }
}

/// Whether the local file `local` writes the setting the store keeps as `key`: then its value is
/// in force over the store's. A value it writes empty is none.
pub fn local_writes(local: &Local, key: &str) -> bool {
    let wrote = |value: Option<&str>| value.is_some_and(|value| !value.trim().is_empty());
    match key.split('.').collect::<Vec<_>>().as_slice() {
        ["agents", "launch", leaf] => symphony_board::local::LAUNCH
            .iter()
            .position(|known| known == leaf)
            .is_some_and(|at| wrote(local.launch[at].as_deref())),
        ["agents", "models", role] => wrote(local.model_of(role)),
        ["limits", leaf] => local.limits.keys().any(|held| held == leaf),
        _ => false,
    }
}

fn change(change: &Change) -> StoredChange {
    StoredChange {
        before: change.before.as_ref().map(shown),
        after: change.after.as_ref().map(shown),
        at: symphony_board::sys::local_minute(change.at),
        by: change.by.clone(),
    }
}

/// What the page writes, from `sections` as drawn: one item for each setting whose scopes hold
/// the board's store, with what `board` holds for it; `local` the local file, which stands over
/// the store where it writes the setting, and `default` the default branch, as a waiting commit
/// is said.
pub fn list(
    sections: &[SettingsSection],
    board: &Board,
    local: Option<&Local>,
    default: &str,
) -> Vec<StoredSetting> {
    let entries = board.entries.as_deref().unwrap_or_default();
    (sections.iter().flat_map(|section| &section.entries))
        .flat_map(|entry| &entry.settings)
        .filter(|setting| setting.scopes.contains(&SettingScope::Board))
        .filter_map(|setting| {
            let key = store_key(&setting.key)?;
            let entry = entries.iter().find(|entry| entry.key == key);
            let alone = entry.map(|entry| Stored::of(std::slice::from_ref(entry)));
            let read = alone.as_ref().is_some_and(|alone| alone.holds(&key));
            let over = local.is_some_and(|local| local_writes(local, &key));
            let waiting = (board.waiting.iter())
                .find(|save| save.keys.contains(&key))
                .map(|save| format!("{}: {}", save.branch, save.state.said(default)));
            Some(StoredSetting {
                setting: setting.key.clone(),
                value: entry.and_then(|entry| entry.value.as_ref()).map(shown),
                held: read && !over,
                refused: alone.and_then(|alone| alone.problems.first().cloned()),
                changes: entry
                    .map_or_else(Vec::new, |entry| entry.changes.iter().map(change).collect()),
                waiting,
                key,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "stored_tests.rs"]
mod tests;
