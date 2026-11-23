//! An edit of one value of a project file in place: the value's own span replaced, every comment,
//! blank line and the order of the rest kept as written. What the model command writes
//! (`symphony model`) and what the Settings page commits when it makes a stored value the
//! project's (`crate::save::settings`).
//!
//! A key the file does not write goes into the nearest table it does write: an explicit table
//! takes a line under its header, an inline table one more member, and a table written only by
//! dotted keys (or none at all) the rest of the path as a dotted key under its explicit parent.
//! A value is written as TOML writes it: text quoted, a whole number and on or off bare.

use toml::Value;
use toml::de::DeTable;

/// What a value is, in a refusal: `a string`.
fn kind(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "a string",
        Value::Integer(_) => "an integer",
        Value::Boolean(_) => "a boolean",
        Value::Float(_) => "a float",
        Value::Datetime(_) => "a datetime",
        Value::Array(_) => "an array",
        Value::Table(_) => "a table",
    }
}

/// `value` as the file writes it, or none for a kind this edit does not write (a float, a
/// datetime, an array, a table).
fn written(value: &Value) -> Option<String> {
    match value {
        // JSON and TOML share these string escapes, but TOML also requires DEL to be escaped.
        Value::String(text) => Some(
            serde_json::Value::String(text.clone())
                .to_string()
                .replace('\u{7f}', "\\u007F"),
        ),
        Value::Integer(n) => Some(n.to_string()),
        Value::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

/// The value `text` writes at `path`, read as TOML reads it.
fn current(text: &str, path: &[&str]) -> Option<Value> {
    let table: toml::Table = toml::from_str(text).ok()?;
    let (last, tables) = path.split_last()?;
    let mut at = &table;
    for name in tables {
        at = at.get(*name)?.as_table()?;
    }
    at.get(*last).cloned()
}

/// `text`, a project file, with `value` at `path` (`["agents", "models", "reviewer"]`): its span
/// alone replaced where the file writes it, else added where [the module](self) says; unchanged,
/// its quoting kept, where it writes that value already.
///
/// # Errors
///
/// Text that is no TOML, a value of a kind this edit does not write, a value already written of
/// another kind, and a table on the path that is not one, each naming the project file and the
/// key.
pub(crate) fn set(text: &mut String, path: &[&str], value: &Value) -> Result<(), String> {
    let file = super::FILE;
    let quoted = written(value).ok_or_else(|| {
        format!(
            "{file}: {} is {}, which this edit does not write",
            path.join("."),
            kind(value)
        )
    })?;
    let parsed = DeTable::parse(text).map_err(|e| format!("{file}: {e}"))?;
    let mut table = parsed.get_ref();
    // (insertion point, path already reached, inline table needs a comma)
    let mut insert = (0, 0, None);
    for (depth, key) in path.iter().enumerate() {
        let Some(found) = table.get(*key) else {
            let (at, reached, inline) = insert;
            let key = path[reached..].join(".");
            let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
            let added = if let Some(comma) = inline {
                format!("{key} = {quoted}{}", if comma { ", " } else { "" })
            } else {
                let leading = if at > 0 && !text[..at].ends_with('\n') {
                    newline
                } else {
                    ""
                };
                format!("{leading}{key} = {quoted}{newline}")
            };
            text.insert_str(at, &added);
            return Ok(());
        };
        if depth + 1 == path.len() {
            // The key is of the kind written: a file that holds another there is refused.
            let held = current(text, path);
            if held.as_ref().map(kind) != Some(kind(value)) {
                return Err(format!(
                    "{file}: {} must be {}",
                    path.join("."),
                    kind(value)
                ));
            }
            // Keep even the original quoting when the value already matches.
            if held.as_ref() != Some(value) {
                text.replace_range(found.span(), &quoted);
            }
            return Ok(());
        }
        table = found
            .get_ref()
            .as_table()
            .ok_or_else(|| format!("{file}: {} must be a table", path[..=depth].join(".")))?;
        let span = found.span();
        if span.is_empty() {
            continue;
        }
        match text.as_bytes().get(span.start) {
            Some(b'{') => {
                // Before its first member, past the space that opens it: `{ a = 1 }` takes
                // `{ key = value, a = 1 }`.
                let opened = &text[span.start + 1..span.end];
                let first = opened.len() - opened.trim_start().len();
                let at = if table.is_empty() { 0 } else { first };
                insert = (span.start + 1 + at, depth + 1, Some(!table.is_empty()));
            }
            Some(b'[') => {
                let at = text[span.end..]
                    .find('\n')
                    .map_or(text.len(), |n| span.end + n + 1);
                insert = (at, depth + 1, None);
            }
            _ => {} // an implicit or dotted table: insert through its explicit parent
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
