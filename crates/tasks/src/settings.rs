//! The board's store of preferences: what a project was told on its board and no file holds,
//! read after the committed project file and before the local file ([`crate::local`]), so the
//! order a value is read in is the Symphony's default, `symphony.toml`, the store, then
//! `symphony.local.toml`. A board that was never told anything has no entry and reads as the
//! files alone.
//!
//! An entry is keyed by its setting, the tables and key the project file names it by, joined by
//! dots (`limits.low_volume`, `agents.launch.model`, `agents.models.reviewer`, `agents.panes`,
//! `agents.launch.on.codex`),
//! and keeps every change made to it, not only the last ([`Change`]): a removed entry keeps its
//! record and holds no value, so the file's value is in force again.
//!
//! It holds preferences alone, by the local file's own rules and lists ([`crate::local::judge`],
//! [`crate::local::PROTECTED`], [`crate::local::PROTECTED_KEYS`]): a value is judged when it is
//! written ([`judged`]), a refusal names the key and why, and nothing is written on one. Each entry
//! is judged again when it is read ([`Stored::of`]), alone: one that is refused, however it came
//! to be in the table, is said and never read, and the others hold.
//!
//! The rows are the store's ([`crate::Store::stored`], `store/stored_settings.rs`); this module is
//! what they mean.

use crate::limits::Written;
use crate::local::{Local, Place, judge};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use toml::Value;

/// The store as a problem and `tasks limits` name it.
pub const SAID: &str = "the board's store";

/// One change of an entry: what it held before and after (none: no entry), when, and the session
/// or page that wrote it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Json>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Json>,
    pub at: f64,
    pub by: String,
}

/// A stored entry: its setting, the value it holds (none once removed) and every change of it,
/// oldest first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Json>,
    /// How many times it was written: what a write of it is fenced on.
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub changes: Vec<Change>,
}

impl Entry {
    /// This entry after `by` wrote `after` (none: removed it) at `at`: the change recorded.
    pub(crate) fn changed(&self, after: Option<Json>, at: f64, by: &str) -> Entry {
        let mut changes = self.changes.clone();
        changes.push(Change {
            before: self.value.clone(),
            after: after.clone(),
            at,
            by: by.to_string(),
        });
        Entry {
            key: self.key.clone(),
            value: after,
            revision: self.revision + 1,
            changes,
        }
    }
}

/// `key` as the tables that lead to its setting and the setting's own key
/// (`agents.launch.model`: `["agents", "launch"]`, `model`).
///
/// # Errors
///
/// A key that names no table and key.
pub fn path(key: &str) -> Result<(Vec<&str>, &str), String> {
    let mut parts: Vec<&str> = key.split('.').collect();
    let last = parts.pop().unwrap_or_default();
    if parts.is_empty() || last.is_empty() || parts.iter().any(|part| part.is_empty()) {
        return Err(format!(
            "{SAID}: {key:?} names no setting: a setting is its table and key as the project \
             file names them, joined by dots (limits.low_volume, agents.models.reviewer)"
        ));
    }
    Ok((parts, last))
}

/// `key` as a problem and `tasks limits` name it: `[agents.models] reviewer`.
pub fn named(key: &str) -> String {
    match path(key) {
        Ok((tables, key)) if tables == ["agents", "launch"] => format!("[agents] launch {key}"),
        Ok((tables, key)) => format!("[{}] {key}", tables.join(".")),
        Err(_) => key.to_string(),
    }
}

/// `text`, a value as a command line gives it, as the value `key` holds: a whole number for a
/// limit, a switch for an agent's (`true` or `on`, `false` or `off`), text for any other. Text
/// that is neither stays text, which the setting's own rule refuses ([`judged`]).
pub fn value_of(key: &str, text: &str) -> Value {
    let tables = path(key).map(|(tables, _)| tables).unwrap_or_default();
    if tables == crate::launch::defaults::ON_TABLES {
        match text.trim() {
            "true" | "on" => return Value::Boolean(true),
            "false" | "off" => return Value::Boolean(false),
            _ => {}
        }
    }
    match text.trim().parse::<i64>() {
        Ok(n) if tables == [crate::limits::TABLE] => Value::Integer(n),
        _ => Value::String(text.to_string()),
    }
}

/// A stored value as the project file would write it; none for one of no kind a setting holds.
pub fn written(value: &Json) -> Option<Value> {
    match value {
        Json::String(text) => Some(Value::String(text.clone())),
        Json::Bool(b) => Some(Value::Boolean(*b)),
        Json::Number(n) => {
            (n.as_i64().map(Value::Integer)).or_else(|| n.as_f64().map(Value::Float))
        }
        _ => None,
    }
}

/// A value as the store keeps it.
pub fn kept(value: &Value) -> Json {
    match value {
        Value::Integer(n) => Json::from(*n),
        Value::Float(f) => Json::from(*f),
        Value::Boolean(b) => Json::from(*b),
        Value::String(text) => Json::from(text.as_str()),
        other => Json::from(crate::limits::shown(other)),
    }
}

/// `value` as `tasks limits` and a record show it: text in quotes, a number as it is.
pub fn shown(value: &Json) -> String {
    match value {
        Json::String(text) => format!("{text:?}"),
        other => other.to_string(),
    }
}

/// `key` holding `value`, judged as the store may hold it, by the rules the local file is read by
/// ([`crate::local::judge`]): what it writes.
///
/// # Errors
///
/// Why it is refused, naming the store and the setting: a key that names none, a setting that
/// holds a command or decides what lands, one the store does not keep, one the Symphony does not
/// know, and a value of the wrong type or out of its range.
pub fn judged(key: &str, value: &Value) -> Result<Local, String> {
    let (tables, last) = path(key)?;
    let mut table = toml::Table::new();
    table.insert(last.to_string(), value.clone());
    for name in tables.iter().rev() {
        let mut above = toml::Table::new();
        above.insert((*name).to_string(), Value::Table(table));
        table = above;
    }
    judge(Place::Store, SAID, &table, &|_, _| None).map_err(|problems| problems.join("; "))
}

/// What the store's entries write over the committed settings, each judged alone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stored {
    /// What the entries in force write, in the local file's shape, with `[agents] panes`.
    pub held: Local,
    /// The key of each entry in force.
    pub keys: Vec<String>,
    /// Each entry not read, and a store that could not be read, said whole.
    pub problems: Vec<String>,
}

impl Stored {
    /// What `entries` write: each that holds a value, judged; one refused is said and left out.
    pub fn of(entries: &[Entry]) -> Stored {
        let mut stored = Stored::default();
        for entry in entries {
            let Some(value) = &entry.value else {
                continue;
            };
            let judged = written(value)
                .ok_or_else(|| {
                    format!(
                        "{SAID}: {} is of no kind a setting holds",
                        named(&entry.key)
                    )
                })
                .and_then(|value| judged(&entry.key, &value));
            match judged {
                Ok(one) => {
                    stored.held.over_local(&one);
                    stored.keys.push(entry.key.clone());
                }
                Err(why) => stored.problems.push(format!(
                    "{why}; its entry is not read, the files' value holds"
                )),
            }
        }
        stored
    }

    /// A store that could not be read, as `why` says: nothing of it is read.
    pub fn unread(why: &str) -> Stored {
        Stored {
            problems: vec![format!(
                "{SAID} cannot be read ({why}); none of it is read, the files' settings hold"
            )],
            ..Stored::default()
        }
    }

    /// Whether no entry is in force.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Whether the entry `key` is in force.
    pub fn holds(&self, key: &str) -> bool {
        self.keys.iter().any(|held| held == key)
    }

    /// `[agents] panes` in force over `committed`, what the project file gives it: the stored
    /// value where the store holds one in force. The local file holds no panes setting, so this is
    /// the value every reader of it takes.
    pub fn panes<'a>(&'a self, committed: &'a str) -> &'a str {
        self.held.panes.as_deref().unwrap_or(committed)
    }

    /// Where the store writes `key`, as `tasks limits` says a stored setting:
    /// `the board's store [limits] low_volume`.
    pub fn written(key: &str) -> Written {
        Written {
            at: format!("{SAID} {}", named(key)),
            line: None,
        }
    }
}

#[cfg(test)]
mod tests;
