//! `symphony model ROLE MODEL`: edit the main checkout's role settings in place, preserving
//! comments and order. `all MODEL --agent AGENT --assessor MODEL` also switches the launch agent
//! and its fallback model, with an explicit assessor override. Recognizable cross-agent names
//! warn; model names remain arbitrary text.
//! Checking and landing a settings change follow the project's settings-only policy.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use symphony_board::launch::defaults::{AGENTS, ASSESSOR, ROLES};
use toml::de::DeTable;

const USAGE: &str = "usage: symphony model ROLE MODEL [--agent AGENT] [--assessor MODEL]";

struct Edit<'a> {
    role: &'a str,
    model: &'a str,
    agent: Option<&'a str>,
    assessor: Option<&'a str>,
}

impl<'a> Edit<'a> {
    fn parsed(parsed: &'a super::Parsed) -> Result<Self, String> {
        let [role, model] = parsed.positionals.as_slice() else {
            return Err("expected ROLE and MODEL".into());
        };
        let edit = Self {
            role,
            model,
            agent: parsed.value("--agent"),
            assessor: parsed.value("--assessor"),
        };
        edit.validate()?;
        Ok(edit)
    }

    fn validate(&self) -> Result<(), String> {
        if self.role != "all" && !ROLES.contains(&self.role) {
            return Err(format!("unknown role {:?}", self.role));
        }
        if let Some(agent) = self.agent
            && !AGENTS.contains(&agent)
        {
            return Err(format!(
                "unknown agent {agent:?}; expected {}",
                AGENTS.join(", ")
            ));
        }
        if self.role != "all" && (self.agent.is_some() || self.assessor.is_some()) {
            return Err("--agent and --assessor are only available with all".into());
        }
        Ok(())
    }
}

pub(super) fn main(
    args: &[String],
    env: &BTreeMap<String, String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let flags = [
        super::Flag {
            name: "--agent",
            value: true,
        },
        super::Flag {
            name: "--assessor",
            value: true,
        },
    ];
    let parsed = match super::parse(args, &flags) {
        Ok(parsed) => parsed,
        Err(problem) => return super::refuse(err, USAGE, "model", &problem),
    };
    let edit = match Edit::parsed(&parsed) {
        Ok(edit) => edit,
        Err(problem) => return super::refuse(err, USAGE, "model", &problem),
    };
    let result = std::env::current_dir()
        .map_err(|error| error.to_string())
        .and_then(|here| change(&here, &edit, env, err));
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

fn change(
    here: &Path,
    edit: &Edit<'_>,
    env: &BTreeMap<String, String>,
    err: &mut dyn Write,
) -> Result<String, String> {
    let (top, _) = symphony_board::git::checkout(here)
        .ok_or_else(|| format!("{} is in no git checkout", here.display()))?;
    let file = symphony_board::builds::main_of(&top).join(crate::project::FILE);
    super::unasked_inside(&file, env)?;
    let before = crate::text::read_text(&file).map_err(|e| crate::text::os_error(&e, &file))?;
    let after = edited(&before, edit)?;
    let Edit {
        role,
        model,
        agent,
        assessor,
    } = edit;
    let details = format!(
        "{}{}",
        agent.map_or_else(String::new, |agent| format!(
            "; launch agent set to {agent:?}"
        )),
        assessor.map_or_else(String::new, |model| format!(
            "; assessor model set to {model:?}"
        ))
    );
    if before == after {
        crate::project::warn_models(&after, &file, err);
        return Ok(format!(
            "{}: {role} already {model:?}{details}",
            file.display()
        ));
    }
    crate::text::write_whole(&file, &after).map_err(|e| crate::text::os_error(&e, &file))?;
    crate::project::warn_models(&after, &file, err);
    Ok(format!(
        "{}: {role} model set to {model:?}{details}",
        file.display()
    ))
}

/// Replace only a value's source span. A missing key goes into the nearest explicit table;
/// dotted keys reach the remaining path, including when no agents table exists yet.
fn edited(text: &str, edit: &Edit<'_>) -> Result<String, String> {
    edit.validate()?;
    let roles = if edit.role == "all" {
        ROLES.as_slice()
    } else {
        std::slice::from_ref(&edit.role)
    };
    let mut text = text.to_string();
    if let Some(agent) = edit.agent {
        set(&mut text, &["agents", "launch", "default"], agent)?;
        set(&mut text, &["agents", "launch", "model"], edit.model)?;
    }
    for role in roles {
        let model = if *role == ASSESSOR {
            edit.assessor.unwrap_or(edit.model)
        } else {
            edit.model
        };
        set(&mut text, &["agents", "models", role], model)?;
    }
    Ok(text)
}

fn set(text: &mut String, path: &[&str], model: &str) -> Result<(), String> {
    // JSON and TOML share these string escapes, but TOML also requires DEL to be escaped.
    let quoted = serde_json::Value::String(model.into())
        .to_string()
        .replace('\u{7f}', "\\u007F");
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
                text.replace_range(value.span(), &quoted);
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
