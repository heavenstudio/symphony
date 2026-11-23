//! A settings edit: an edit of the project file (`symphony.toml`) that changes preferences alone,
//! which the Settings page commits when it makes a value the board's store holds the project's
//! (`crate::save::settings`). The committer takes no other edit of a file that is not a design
//! document: [`changes`] reads what the edit changes, and every key it changes must be one the
//! store keeps, holding a value the store would take ([`symphony_board::settings::judged`]), by the
//! same rules and lists the store and the local file are judged by. So a commit made here never
//! changes a setting that holds a command or decides what lands, whatever the edit's text.
//!
//! Its commit's message names each key, the value before and the value after ([`message`]); its
//! task, where the project delivers by pull request, is worded for it ([`task`]); and once its
//! commit stands, each store entry it made the project's is removed while it still holds the
//! value committed ([`cleared`]): a value stored again since is a newer change, and stays.

use std::collections::BTreeMap;

use serde_json::Value as Json;
use symphony_board::settings::{judged, kept, named};
use toml::Value;

use super::Edit;
use crate::project::FILE;

/// One key a settings edit changes: its key in the store (`limits.low_volume`), what the file
/// wrote before (none: not written) and what it writes after.
#[derive(Clone, Debug, PartialEq)]
pub struct Changed {
    pub key: String,
    pub before: Option<Value>,
    pub after: Value,
}

/// Every leaf of `table`, by its dotted path: a table's keys below it, any other value a leaf.
fn leaves(table: &toml::Table, above: &str, into: &mut BTreeMap<String, Value>) {
    for (key, value) in table {
        let path = if above.is_empty() {
            key.clone()
        } else {
            format!("{above}.{key}")
        };
        match value {
            Value::Table(inner) => leaves(inner, &path, into),
            other => {
                into.insert(path, other.clone());
            }
        }
    }
}

/// `text`, the project file, read as TOML; an empty text is an empty file.
fn read(text: &str, side: &str) -> Result<toml::Table, String> {
    toml::from_str(text).map_err(|e| format!("{FILE} {side} the edit: {}", e.message()))
}

/// What `edit` changes, when it is an edit of the project file; none for one of another file.
/// Each key it changes, in the order of their paths.
///
/// # Errors
///
/// Why the committer takes no such edit: text that is no TOML on either side, a key it removes,
/// and a key it changes that the board's store would not keep with that value, in the store's own
/// words.
pub fn changes(edit: &Edit) -> Option<Result<Vec<Changed>, String>> {
    (edit.rel == FILE).then(|| {
        let mut before = BTreeMap::new();
        leaves(&read(&edit.base, "before")?, "", &mut before);
        let mut after = BTreeMap::new();
        leaves(&read(&edit.new, "after")?, "", &mut after);
        if let Some(gone) = before.keys().find(|key| !after.contains_key(*key)) {
            return Err(format!(
                "a settings edit of {FILE} removes nothing, and this one removes {gone}"
            ));
        }
        let mut changed = Vec::new();
        for (key, value) in after {
            let was = before.remove(&key);
            if was.as_ref() == Some(&value) {
                continue;
            }
            judged(&key, &value).map_err(|why| {
                format!("a settings edit of {FILE} changes preferences alone: {why}")
            })?;
            changed.push(Changed {
                key,
                before: was,
                after: value,
            });
        }
        Ok(changed)
    })
}

/// `value` as a message says it: text in quotes, a number or a switch as written.
fn said(value: &Value) -> String {
    match value {
        Value::String(text) => format!("{text:?}"),
        Value::Integer(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Float(f) => f.to_string(),
        other => format!("{other:?}"),
    }
}

/// The commit's message: `settings: [limits] low_volume from 4 to 3`, each key it changes with
/// the value before (`none written` where the file wrote none) and after.
pub fn message(changes: &[Changed]) -> String {
    let each: Vec<String> = changes
        .iter()
        .map(|change| {
            let before = change.before.as_ref().map_or("none written".into(), said);
            format!(
                "{} from {before} to {}",
                named(&change.key),
                said(&change.after)
            )
        })
        .collect();
    format!("settings: {}", each.join("; "))
}

/// The title and the body of the owner's task that delivers the settings edit committed on
/// `branch` as `tip`, where the project delivers by pull request.
pub(super) fn task(
    changes: &[Changed],
    branch: &str,
    tip: &str,
    default: &str,
) -> (String, String) {
    let message = message(changes);
    let what = message.trim_start_matches("settings: ");
    (
        format!("Deliver the owner's settings change: {what}"),
        format!(
            "The owner made a stored setting the project's on the Settings page ({what}), and \
             this project delivers by pull request, so the change is not on {default}: the branch \
             {branch} holds it, as the one commit {tip} of {FILE}, which changes preferences \
             alone. Its work is done: nothing on the branch is edited but to settle a conflict \
             with {default}, by merging {default} into the branch and never by rebasing it: the \
             delivery is proved by a head that holds that commit. Assess that diff for what it \
             needs (a change of settings' values lands on the settings check), then land it, \
             which delivers it by its pull request. The board's store keeps the value in force \
             meanwhile, and its entry is removed once the delivery's receipt proves the commit \
             on the remote {default}."
        ),
    )
}

/// Each store entry `changes` made the project's removed, while it still holds the value
/// committed: an entry stored again since, or removed, is left as it is.
///
/// # Errors
///
/// The board could not be reached, or an entry could not be removed.
pub fn cleared(store: &symphony_board::Store, changes: &[Changed]) -> Result<(), String> {
    for change in changes {
        let value: Json = kept(&change.after);
        store
            .remove_setting_holding(&change.key, &value, crate::postpone::OWNER)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
