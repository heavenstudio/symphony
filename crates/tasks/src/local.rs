//! The local settings file, `symphony.local.toml` beside the main checkout's `symphony.toml`: one
//! machine's or one person's preferences, which git does not version, read after the project file.
//! A key it writes replaces the committed one, and a key it leaves out keeps it. This is its one
//! reader: the board's limits ([`crate::limits::read_at`]), a launch's agent and models
//! ([`crate::launch::Defaults::configured`]) and the Symphony's project load all take what
//! [`beside`] read, so each takes the same override.
//!
//! It holds preferences alone: `[agents] launch` (`default`, `model`, `effort`),
//! `[agents.models]` (a role's model) and `[limits]`. A setting that holds a command or a program
//! the Symphony runs, or that decides what lands ([`PROTECTED`], [`PROTECTED_KEYS`]), is refused
//! there by name and never read from it: a file no commit records must not change the landing gate
//! or run a command, which would be a way around review that leaves no trace. Any other table or
//! key is refused by name too, as no setting a local file overrides.
//!
//! Whatever is wrong with it (a refused key, a value of the wrong type, text that is no TOML) is
//! said whole with its line, and then none of the file is read: the committed settings hold
//! ([`left_out`]), so a broken local file stops neither the board nor a launch. No file is no
//! override. A linked worktree has none of its own: every reader looks beside the main checkout's
//! project file.

use crate::launch::defaults::{AGENTS, ROLES};
use crate::limits;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use toml::Value;

/// The local settings file, beside the main checkout's project file.
pub const FILE: &str = "symphony.local.toml";

/// The tables a local file may write: `[agents]`'s `launch` and `models` alone, and `[limits]`.
pub const TABLES: [&str; 2] = ["agents", limits::TABLE];
/// The keys of `[agents] launch` a local file may write.
pub const LAUNCH: [&str; 3] = ["default", "model", "effort"];

/// The tables that hold commands or programs the Symphony runs, or decide what lands: refused in a
/// local file whole, each key by name.
pub const PROTECTED: [&str; 5] = ["plugins", "verdicts", "land", "releases", "stamps"];
/// The keys of other tables that do: `[project]`'s and `[board]`'s.
pub const PROTECTED_KEYS: [(&str, &[&str]); 2] = [
    ("project", &["cargo", "default_branch"]),
    (
        "board",
        &[
            "on_review",
            "on_land",
            "machine_work_reader",
            "postgresql",
            "assessments",
        ],
    ),
];

/// What a local file writes, each value as written, with the text its lines are found in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Local {
    /// The file, as a problem and `tasks limits` name it.
    pub path: PathBuf,
    /// `[agents] launch`'s `default`, `model` and `effort`, in [`LAUNCH`]'s order.
    pub launch: [Option<String>; 3],
    /// `[agents.models]`, in [`ROLES`]' order.
    pub models: [Option<String>; 4],
    /// `[limits]`, by key ([`limits::KEYS`]).
    pub limits: BTreeMap<&'static str, usize>,
    text: String,
}

impl Local {
    /// The line `key` is written on in the table `path` leads to.
    pub fn line(&self, path: &[&str], key: &str) -> Option<usize> {
        limits::line(&self.text, path, key)
    }

    /// Where it writes `key` of the table `path` leads to, spelt `at` (`[limits] low_volume`): the
    /// file's name before it and its line, as `tasks limits` says an overridden setting.
    pub fn written(&self, at: &str, path: &[&str], key: &str) -> limits::Written {
        limits::Written {
            at: format!("{FILE} {at}"),
            line: self.line(path, key),
        }
    }

    /// The model it writes for `role`, one of [`ROLES`].
    pub fn model_of(&self, role: &str) -> Option<&str> {
        let at = ROLES.iter().position(|known| *known == role)?;
        self.models[at].as_deref()
    }

    /// Each of `[agents] launch`'s three (`default`, `model`, `effort`) and of the four roles'
    /// models ([`ROLES`]' order) this file writes, put over the committed one; one it leaves out
    /// is left as it was. The one merge every reader of those settings makes.
    pub fn over(&self, launch: [&mut Option<String>; 3], models: [&mut Option<String>; 4]) {
        for (into, written) in launch.into_iter().zip(&self.launch) {
            if written.is_some() {
                into.clone_from(written);
            }
        }
        for (into, written) in models.into_iter().zip(&self.models) {
            if written.is_some() {
                into.clone_from(written);
            }
        }
    }
}

/// Where the local file of the project file at `project_file` is: beside it.
pub fn path_beside(project_file: &Path) -> PathBuf {
    project_file.with_file_name(FILE)
}

/// The local file beside the project file at `project_file`, as [`parse`] reads it: none when
/// there is no file.
///
/// # Errors
///
/// Everything wrong with it, each said whole: the file is then read by nobody ([`left_out`]).
pub fn beside(project_file: &Path) -> Result<Option<Local>, Vec<String>> {
    let path = path_beside(project_file);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&path, &text).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(vec![format!("cannot read {}: {error}", path.display())]),
    }
}

/// `problem` with a local file as a reader that runs on says it: none of the file is read.
pub fn left_out(problem: &str) -> String {
    format!("{problem}; none of {FILE} is read, the committed settings hold")
}

/// `text`, the local file at `path`: what it writes.
///
/// # Errors
///
/// Every problem with it, each naming the file, the setting and its line: text that is no TOML, a
/// setting it may not hold ([`PROTECTED`], [`PROTECTED_KEYS`], or any outside [`TABLES`]), a key
/// the Symphony does not know in a table it may hold, and a value of the wrong type or range.
pub fn parse(path: &Path, text: &str) -> Result<Local, Vec<String>> {
    let said = path.display();
    let file: toml::Table = toml::from_str(text)
        .map_err(|error| vec![format!("{said}: {}", error.to_string().trim_end())])?;
    let mut local = Local {
        path: path.to_path_buf(),
        text: text.to_string(),
        ..Local::default()
    };
    let mut problems = Vec::new();
    // `[table] key (line N)`, or `[table] (line N)` for the table itself.
    let named = |tables: &[&str], key: Option<&str>| {
        let at = match key {
            Some(key) => format!("[{}] {key}", tables.join(".")),
            None => format!("[{}]", tables.join(".")),
        };
        let line = match (key, tables.split_last()) {
            (Some(key), _) => limits::line(text, tables, key),
            (None, Some((table, above))) => limits::line(text, above, table),
            (None, None) => None,
        };
        limits::Written { at, line }.named()
    };
    let wrong = |name: String, value: &Value, why: &str| {
        format!("{said}: {name} is {}; {why}", limits::shown(value))
    };
    for (table, value) in &file {
        let Value::Table(keys) = value else {
            let name = limits::Written {
                at: table.clone(),
                line: limits::line(text, &[], table),
            }
            .named();
            problems.push(wrong(name, value, "a local file holds tables of settings"));
            continue;
        };
        let protected: Option<&[&str]> = if PROTECTED.contains(&table.as_str()) {
            Some(&[])
        } else {
            PROTECTED_KEYS
                .iter()
                .find(|(of, _)| of == table)
                .map(|(_, keys)| *keys)
        };
        match (table.as_str(), protected) {
            ("agents", _) => {
                for (key, value) in keys {
                    let (known, into): (&[&str], &mut [Option<String>]) = match key.as_str() {
                        "launch" => (&LAUNCH, &mut local.launch),
                        "models" => (&ROLES, &mut local.models),
                        _ => {
                            problems.push(not_local(&said, &named(&["agents"], Some(key))));
                            continue;
                        }
                    };
                    let tables = ["agents", key.as_str()];
                    let Value::Table(written) = value else {
                        let name = named(&["agents"], Some(key));
                        problems.push(wrong(name, value, "not a table"));
                        continue;
                    };
                    for (key, value) in written {
                        let name = named(&tables, Some(key));
                        let Some(at) = known.iter().position(|k| k == key) else {
                            problems.push(unknown(&said, &name));
                            continue;
                        };
                        match value {
                            Value::String(text)
                                if tables[1] == "launch"
                                    && at == 0
                                    && !text.trim().is_empty()
                                    && !AGENTS.contains(&text.as_str()) =>
                            {
                                problems.push(format!(
                                    "{said}: {name} {text:?} is not one of {}",
                                    AGENTS.join(", ")
                                ));
                            }
                            Value::String(text) => into[at] = Some(text.clone()),
                            other => problems.push(wrong(name, other, "not text")),
                        }
                    }
                }
            }
            (limits::TABLE, _) => {
                for (key, value) in keys {
                    let name = named(&[limits::TABLE], Some(key));
                    let Some((known, least, below)) = limits::LEAST
                        .iter()
                        .find(|(known, _, _)| known == key)
                        .copied()
                    else {
                        problems.push(unknown(&said, &name));
                        continue;
                    };
                    match limits::whole(value, least) {
                        Ok(n) => {
                            local.limits.insert(known, n);
                        }
                        Err(why) => problems.push(wrong(name, value, why.unwrap_or(below))),
                    }
                }
            }
            (_, Some(held)) => {
                if keys.is_empty() {
                    problems.push(if held.is_empty() {
                        refused(&said, &named(&[table], None))
                    } else {
                        not_local(&said, &named(&[table], None))
                    });
                }
                for key in keys.keys() {
                    let name = named(&[table], Some(key));
                    problems.push(if held.is_empty() || held.contains(&key.as_str()) {
                        refused(&said, &name)
                    } else {
                        not_local(&said, &name)
                    });
                }
            }
            (_, None) => problems.push(not_local(&said, &named(&[table], None))),
        }
    }
    if problems.is_empty() {
        Ok(local)
    } else {
        Err(problems)
    }
}

/// A setting that holds a command or decides what lands, refused by name.
fn refused(path: &dyn std::fmt::Display, name: &str) -> String {
    format!(
        "{path}: {name} is not read from a local file: it holds a command or a program the \
         Symphony runs, or decides what lands, and a file no commit records must not change \
         that; it stays in {}",
        crate::project::FILE
    )
}

/// A setting no local file overrides, or one misspelt: refused by name.
fn not_local(path: &dyn std::fmt::Display, name: &str) -> String {
    format!(
        "{path}: {name} is not a setting a local file overrides: it holds [agents] launch, \
         [agents.models] and [limits] alone"
    )
}

/// A key the Symphony does not know in a table a local file may hold, in the project file's words.
fn unknown(path: &dyn std::fmt::Display, name: &str) -> String {
    format!("{path}: {name} is not a setting the Symphony knows")
}

#[cfg(test)]
mod tests;
