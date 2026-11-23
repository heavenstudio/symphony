//! The local settings file, `symphony.local.toml` beside the main checkout's `symphony.toml`: one
//! machine's or one person's preferences, which git does not version, read after the project file.
//! A key it writes replaces the committed one, and a key it leaves out keeps it. This is its one
//! reader: the board's limits ([`crate::limits::read_at`]), a launch's agent and models
//! ([`crate::launch::Defaults::configured`]) and the Symphony's project load all take what
//! [`beside`] read, so each takes the same override.
//!
//! It holds preferences alone: `[agents] launch` (`default`, `model`, `effort`, and the agents'
//! switches, `[agents.launch.on]`), `[agents.models]` (a role's model) and `[limits]`. A setting that holds a command or a program
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
//!
//! The board's store of preferences ([`crate::settings`]) is judged here too ([`judge`], by its
//! [`Place`]): one set of rules and one pair of lists says what either place may hold, so a setting
//! refused in the file is refused in the store by the same line. The store holds one preference
//! more, `[agents] panes` ([`PANES`]), which the local file has no place for.
//!
//! The launch default cannot be turned off ([`crate::launch::defaults`]): a local file that turns
//! off the default it names itself is refused whole with the line that says to pick another
//! default first ([`judge`]). One that turns off a default it does not name, the store's or the
//! project file's, is the places disagreeing, which every reader settles alike: the default wins
//! and the switch does not hold (`Defaults::overruled`).

use crate::launch::defaults::{
    AGENT_KEY, AGENTS, ON, ON_TABLES, ROLES, agent, default_off, switch,
};
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
/// local file whole, each key by name. The three workflow tables decide what lands (the stages, the
/// edges and their guards, the authorities whose word meets them); the two lists of tables among
/// them are refused in a local file as arrays all the same, which hold no table of settings.
pub const PROTECTED: [&str; 8] = [
    "plugins",
    "verdicts",
    "land",
    "releases",
    "stamps",
    "workflow",
    "workflow_mappings",
    "workflow_authorities",
];
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
            "external_postgresql",
            "assessments",
        ],
    ),
];

/// What `[agents] panes` may be, as the Symphony's load of the project file reads it.
pub const PANES: [&str; 2] = ["hc", "none"];

/// Where preferences over the committed settings are kept, which [`judge`] words its refusals for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// The local settings file ([`FILE`]).
    File,
    /// The board's store ([`crate::settings`]), which holds `[agents] panes` as well.
    Store,
}

impl Place {
    /// The place in a refusal: `a local file`, `the board's store`.
    fn a(self) -> &'static str {
        match self {
            Place::File => "a local file",
            Place::Store => "the board's store",
        }
    }

    /// What it holds, as a refusal of anything else lists it.
    fn holds(self) -> &'static str {
        match self {
            Place::File => "[agents] launch, [agents.models] and [limits]",
            Place::Store => "[agents] launch, [agents.models], [agents] panes and [limits]",
        }
    }

    /// Whether it keeps `key` of the table `path` leads to: a key of `[agents] launch`
    /// ([`LAUNCH`]), an agent's switch ([`AGENTS`]), a role's model ([`ROLES`]) or a key of
    /// `[limits]` in either place, and `[agents] panes` in the board's store alone. The one list of
    /// what each place keeps: [`judge`] takes a key of `[agents]` itself only where it says so, and
    /// the Settings page reads it for where a setting may be written.
    pub fn keeps(self, path: &[&str], key: &str) -> bool {
        match path {
            [limits::TABLE] => limits::KEYS.contains(&key),
            ["agents", "launch"] => LAUNCH.contains(&key),
            ["agents", "launch", ON] => AGENTS.contains(&key),
            ["agents", "models"] => ROLES.contains(&key),
            ["agents"] => self == Place::Store && key == "panes",
            _ => false,
        }
    }
}

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
    /// `[agents] panes`, which the board's store alone holds ([`Place::Store`]).
    pub panes: Option<String>,
    /// `[agents.launch.on]`: each agent's switch it writes, in [`AGENTS`]' order.
    pub on: [Option<bool>; 2],
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

    /// What `other` writes, put over what this one does: one entry of the board's store taken
    /// into the rest ([`crate::settings::Stored::of`]).
    pub(crate) fn over_local(&mut self, other: &Local) {
        let Local {
            launch,
            models,
            limits,
            panes,
            on,
            ..
        } = other;
        for (into, written) in self.on.iter_mut().zip(on) {
            if written.is_some() {
                *into = *written;
            }
        }
        for (into, written) in self.launch.iter_mut().zip(launch) {
            if written.is_some() {
                into.clone_from(written);
            }
        }
        for (into, written) in self.models.iter_mut().zip(models) {
            if written.is_some() {
                into.clone_from(written);
            }
        }
        self.limits.extend(limits.iter().map(|(key, n)| (*key, *n)));
        if panes.is_some() {
            self.panes.clone_from(panes);
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
/// there is no file. Every reader takes it from here, so each reads the same file or none.
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
    let said = path.display().to_string();
    let file: toml::Table = toml::from_str(text)
        .map_err(|error| vec![format!("{said}: {}", error.to_string().trim_end())])?;
    let mut local = judge(Place::File, &said, &file, &|tables, key| {
        limits::line(text, tables, key)
    })?;
    local.path = path.to_path_buf();
    local.text = text.to_string();
    Ok(local)
}

/// `file`, the settings `place` was handed, named `said` in a problem: what it writes. `line`
/// finds the line of a key in the tables that lead to it, where the place has lines.
///
/// # Errors
///
/// Every problem with it, each naming the place and the setting: a setting it may not hold
/// ([`PROTECTED`], [`PROTECTED_KEYS`], or any outside what the place holds), a key the Symphony
/// does not know in a table it may hold, and a value of the wrong type or range.
pub fn judge(
    place: Place,
    said: &str,
    file: &toml::Table,
    line: &dyn Fn(&[&str], &str) -> Option<usize>,
) -> Result<Local, Vec<String>> {
    let mut local = Local::default();
    let mut problems = Vec::new();
    // `[table] key (line N)`, or `[table] (line N)` for the table itself.
    let named = |tables: &[&str], key: Option<&str>| {
        let at = match key {
            Some(key) => format!("[{}] {key}", tables.join(".")),
            None => format!("[{}]", tables.join(".")),
        };
        let line = match key {
            Some(key) => line(tables, key),
            None => tables
                .split_last()
                .and_then(|(table, above)| line(above, table)),
        };
        limits::Written { at, line }.named()
    };
    let wrong = |name: String, value: &Value, why: &str| {
        format!("{said}: {name} is {}; {why}", limits::shown(value))
    };
    for (table, value) in file {
        let Value::Table(keys) = value else {
            let name = limits::Written {
                at: table.clone(),
                line: line(&[], table),
            }
            .named();
            let why = format!("{} holds tables of settings", place.a());
            problems.push(wrong(name, value, &why));
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
                        // A key of `[agents]` itself is judged only where the one list says the
                        // place keeps it ([`Place::keeps`]): none is taken that it does not name.
                        own if !place.keeps(&["agents"], own) => {
                            problems.push(not_local(place, said, &named(&["agents"], Some(own))));
                            continue;
                        }
                        "panes" => {
                            let name = named(&["agents"], Some(key));
                            match value {
                                Value::String(text) if PANES.contains(&text.as_str()) => {
                                    local.panes = Some(text.clone());
                                }
                                Value::String(text) => problems.push(format!(
                                    "{said}: {name} {text:?} is not one of {}",
                                    PANES.join(", ")
                                )),
                                other => problems.push(wrong(name, other, "not text")),
                            }
                            continue;
                        }
                        // One the list names and no arm here judges is refused all the same.
                        _ => {
                            problems.push(not_local(place, said, &named(&["agents"], Some(key))));
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
                        if tables[1] == "launch" && key == ON {
                            let Value::Table(switches) = value else {
                                let why = "it is a table of each agent's switch";
                                problems.push(wrong(name, value, why));
                                continue;
                            };
                            for (agent, value) in switches {
                                let name = named(&ON_TABLES, Some(agent));
                                match switch(&name, agent, value) {
                                    Ok((at, on)) => local.on[at] = Some(on),
                                    Err(why) => problems.push(format!("{said}: {why}")),
                                }
                            }
                            continue;
                        }
                        let Some(at) = known.iter().position(|k| k == key) else {
                            problems.push(unknown(said, &name));
                            continue;
                        };
                        match value {
                            Value::String(text) if tables[1] == "launch" && at == 0 => {
                                match agent(&name, Some(text)) {
                                    Ok(_) => into[at] = Some(text.clone()),
                                    Err(why) => problems.push(format!("{said}: {why}")),
                                }
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
                    let Some((known, _, _)) = limits::LEAST
                        .iter()
                        .find(|(known, _, _)| known == key)
                        .copied()
                    else {
                        problems.push(unknown(said, &name));
                        continue;
                    };
                    match limits::judged(known, value) {
                        Ok(n) => {
                            local.limits.insert(known, n);
                        }
                        Err(why) => problems.push(wrong(name, value, why)),
                    }
                }
            }
            (_, Some(held)) => {
                if keys.is_empty() {
                    problems.push(if held.is_empty() {
                        refused(place, said, &named(&[table], None))
                    } else {
                        not_local(place, said, &named(&[table], None))
                    });
                }
                for key in keys.keys() {
                    let name = named(&[table], Some(key));
                    problems.push(if held.is_empty() || held.contains(&key.as_str()) {
                        refused(place, said, &name)
                    } else {
                        not_local(place, said, &name)
                    });
                }
            }
            (_, None) => problems.push(not_local(place, said, &named(&[table], None))),
        }
    }
    // A file that turns off the default it names does not load; the store's entries are judged
    // one by one, and its write against the default in force (`Store::store_setting`).
    let own = agent(AGENT_KEY, local.launch[0].as_deref()).ok().flatten();
    if let Some(own) = own.filter(|_| place == Place::File) {
        let at = AGENTS.iter().position(|known| *known == own).unwrap_or(0);
        if local.on[at] == Some(false) {
            let chosen = named(&["agents", "launch"], Some("default"));
            let name = named(&ON_TABLES, Some(own));
            problems.push(format!("{said}: {}", default_off(&name, own, &chosen)));
        }
    }
    if problems.is_empty() {
        Ok(local)
    } else {
        Err(problems)
    }
}

/// A setting that holds a command or decides what lands, refused by name.
fn refused(place: Place, said: &str, name: &str) -> String {
    let (read, kept) = match place {
        Place::File => ("read from a local file", "a file no commit records"),
        Place::Store => ("kept in the board's store", "a place no commit records"),
    };
    format!(
        "{said}: {name} is not {read}: it holds a command or a program the Symphony runs, or \
         decides what lands, and {kept} must not change that; it stays in {}",
        crate::project::FILE
    )
}

/// A setting the place does not hold, or one misspelt: refused by name.
fn not_local(place: Place, said: &str, name: &str) -> String {
    let overrides = match place {
        Place::File => "a local file overrides",
        Place::Store => "the board's store keeps",
    };
    format!(
        "{said}: {name} is not a setting {overrides}: it holds {} alone",
        place.holds()
    )
}

/// A key the Symphony does not know in a table the place may hold, in the project file's words.
fn unknown(said: &str, name: &str) -> String {
    format!("{said}: {name} is not a setting the Symphony knows")
}

#[cfg(test)]
mod tests;
