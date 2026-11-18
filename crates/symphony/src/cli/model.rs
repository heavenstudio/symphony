//! `symphony model ROLE MODEL`: edit the main checkout's role settings in place, preserving
//! comments and order. Model names are text, never checked against a provider's catalogue.
//! Checking and landing a settings change follow the project's settings-only policy. With
//! `--local` the edit goes to the local settings file beside it instead
//! (`symphony_board::local`), which git does not version and nothing lands: made when it is not
//! there, and never left as its readers would refuse it.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use symphony_board::launch::defaults::ROLES;
use symphony_board::local;
use toml::de::DeTable;

const USAGE: &str = "usage: symphony model ROLE MODEL [--local]";
const LOCAL: &str = "--local";

pub(super) fn main(
    args: &[String],
    env: &BTreeMap<String, String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let flags = [super::Flag {
        name: LOCAL,
        value: false,
    }];
    let parsed = match super::parse(args, &flags) {
        Ok(parsed) => parsed,
        Err(problem) => return super::refuse(err, USAGE, "model", &problem),
    };
    let [role, model] = parsed.positionals.as_slice() else {
        return super::refuse(err, USAGE, "model", "expected ROLE and MODEL");
    };
    if role != "all" && !ROLES.contains(&role.as_str()) {
        return super::refuse(err, USAGE, "model", &format!("unknown role {role:?}"));
    }
    let result = std::env::current_dir()
        .map_err(|error| error.to_string())
        .and_then(|here| change_in(&here, role, model, env, parsed.flag(LOCAL)));
    match result {
        Ok(line) => {
            let _ = writeln!(out, "{line}");
            0
        }
        Err(problem) => {
            let _ = writeln!(err, "symphony model: {problem}");
            1
        }
    }
}

#[cfg(test)]
fn change(
    here: &Path,
    role: &str,
    model: &str,
    env: &BTreeMap<String, String>,
) -> Result<String, String> {
    change_in(here, role, model, env, false)
}

/// The edit in the main checkout's project file, or with `local` in the local settings file
/// beside it, which starts empty when it is not there.
fn change_in(
    here: &Path,
    role: &str,
    model: &str,
    env: &BTreeMap<String, String>,
    local: bool,
) -> Result<String, String> {
    let (top, _) = symphony_board::git::checkout(here)
        .ok_or_else(|| format!("{} is in no git checkout", here.display()))?;
    let project = symphony_board::builds::main_of(&top).join(crate::project::FILE);
    let file = if local {
        local::path_beside(&project)
    } else {
        project
    };
    super::unasked_inside(&file, env)?;
    let before = match crate::text::read_text(&file) {
        Ok(text) => text,
        Err(e) if local && e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(crate::text::os_error(&e, &file)),
    };
    let after = if local {
        // A file its readers leave out is mended by hand first, each problem named on the line
        // it stands on, before any edit moves it.
        local::parse(&file, &before).map_err(|problems| problems.join("\n"))?;
        // The edit names the project file in what it refuses: here it is the local one.
        edited(&before, role, model)
            .map_err(|problem| problem.replacen(crate::project::FILE, local::FILE, 1))?
    } else {
        edited(&before, role, model)?
    };
    if before == after {
        return Ok(format!("{}: {role} already {model:?}", file.display()));
    }
    crate::text::write_whole(&file, &after).map_err(|e| crate::text::os_error(&e, &file))?;
    Ok(format!("{}: {role} model set to {model:?}", file.display()))
}

/// Replace only a value's source span. A missing key goes into the nearest explicit table;
/// dotted keys reach the remaining path, including when no agents table exists yet.
fn edited(text: &str, role: &str, model: &str) -> Result<String, String> {
    let roles = if role == "all" {
        ROLES.as_slice()
    } else if ROLES.contains(&role) {
        std::slice::from_ref(&role)
    } else {
        return Err(format!("unknown role {role:?}"));
    };
    // JSON and TOML share these string escapes, but TOML also requires DEL to be escaped.
    let quoted = serde_json::Value::String(model.into())
        .to_string()
        .replace('\u{7f}', "\\u007F");
    let mut text = text.to_string();
    for role in roles {
        set(&mut text, &["agents", "models", role], model, &quoted)?;
    }
    Ok(text)
}

fn set(text: &mut String, path: &[&str], model: &str, quoted: &str) -> Result<(), String> {
    let parsed = DeTable::parse(text).map_err(|e| format!("symphony.toml: {e}"))?;
    let mut table = parsed.get_ref();
    // (insertion point, path already reached, inline table needs a comma)
    let mut insert = (0, 0, None);
    for (depth, key) in path.iter().enumerate() {
        let Some(value) = table.get(*key) else {
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
            if value.get_ref().as_str().is_none() {
                return Err(format!(
                    "symphony.toml: {} must be a string",
                    path.join(".")
                ));
            }
            // Keep even the original quoting when the value already matches.
            if value.get_ref().as_str() != Some(model) {
                text.replace_range(value.span(), quoted);
            }
            return Ok(());
        }
        table = value.get_ref().as_table().ok_or_else(|| {
            format!(
                "symphony.toml: {} must be a table",
                path[..=depth].join(".")
            )
        })?;
        let span = value.span();
        if span.is_empty() {
            continue;
        }
        match text.as_bytes().get(span.start) {
            Some(b'{') => insert = (span.start + 1, depth + 1, Some(!table.is_empty())),
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
mod tests;
