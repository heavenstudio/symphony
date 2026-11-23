//! `settings list`, `settings set KEY VALUE` and `settings remove KEY`: the board's store of
//! preferences ([`crate::settings`], [`crate::Store::store_setting`]) from the command line. A
//! key is its setting's tables and key as the project file names them, joined by dots
//! (`limits.low_volume`, `agents.launch.model`, `agents.models.reviewer`, `agents.panes`,
//! `agents.launch.on.codex`).

use super::{Context, Stop};
use crate::Store;
use crate::settings::{Change, Entry, Stored, named, shown, value_of};
use crate::sys::local_minute;
use serde_json::{Value, json};

pub(super) fn run(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let (key, value) = (c.parsed.get("key"), c.parsed.get("value"));
    match (c.arg("action").as_str(), key, value) {
        ("list", None, None) => list(store, c),
        ("set", Some(key), Some(value)) => {
            let actor = c.actor()?;
            // The project file whose launch default a switch is judged against, if any.
            let file = crate::limits::file_of(c.env).unwrap_or_default();
            let (entry, changed) =
                store.store_setting(key, &value_of(key, value), &actor, &file)?;
            let held = entry.value.as_ref().map(shown).unwrap_or_default();
            c.say(&if changed {
                format!(
                    "{} is {held} in the board's store, in force from the next read of the \
                     settings unless symphony.local.toml writes it",
                    named(key)
                )
            } else {
                format!(
                    "{} is {held} in the board's store already: nothing written",
                    named(key)
                )
            });
            Ok(())
        }
        ("remove", Some(key), None) => {
            let actor = c.actor()?;
            store.remove_setting(key, &actor)?;
            c.say(&format!(
                "{} has no entry in the board's store now: the files' value holds",
                named(key)
            ));
            Ok(())
        }
        (other, _, _) => Err(Stop::Refused(format!(
            "settings takes list, set KEY VALUE or remove KEY, not {other:?} with those arguments"
        ))),
    }
}

/// Every entry with its record, those removed too, and each that is not read with why.
fn list(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let entries = store.stored_entries()?;
    if c.parsed.flag("json") {
        let listed: Vec<Value> = entries
            .iter()
            .map(|entry| {
                let alone = Stored::of(std::slice::from_ref(entry));
                json!({
                    "key": entry.key,
                    "value": entry.value,
                    "read": alone.holds(&entry.key),
                    "refused": alone.problems.first(),
                    "changes": entry.changes,
                })
            })
            .collect();
        c.say(&serde_json::to_string_pretty(&listed).expect("entries are JSON"));
        return Ok(());
    }
    for line in lines(&entries) {
        c.say(&line);
    }
    Ok(())
}

/// What `settings list` prints of `entries`.
pub fn lines(entries: &[Entry]) -> Vec<String> {
    if entries.is_empty() {
        return vec![
            "the board's store holds no setting: every value is the files' (just tasks limits)"
                .into(),
        ];
    }
    let mut lines = Vec::new();
    for entry in entries {
        let alone = Stored::of(std::slice::from_ref(entry));
        let name = named(&entry.key);
        lines.push(match (&entry.value, alone.problems.first()) {
            (None, _) => format!("{name}  removed: the files' value holds"),
            (Some(value), None) => format!("{name}  {}", shown(value)),
            (Some(value), Some(why)) => format!("{name}  {}  NOT READ: {why}", shown(value)),
        });
        let held = |value: &Option<Value>| value.as_ref().map_or("none".to_string(), shown);
        for Change {
            before,
            after,
            at,
            by,
        } in &entry.changes
        {
            lines.push(format!(
                "  {}  {by}: {} -> {}",
                local_minute(*at),
                held(before),
                held(after)
            ));
        }
    }
    lines
}

#[cfg(test)]
mod tests;
