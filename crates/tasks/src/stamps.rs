//! The stamps a project's land can require (docs/workflow/analysis/needed-stamps.md), and what a task needs
//! of them.
//!
//! The catalogue ([`Catalogue`]) is one list: the project's `[stamps]`, a table a stamp, read as
//! the board reads its own keys, from the main checkout's committed `symphony.toml`
//! ([`Catalogue::of`]), so a branch cannot change what it is held to; with none written, the
//! Symphony's own four ([`Catalogue::symphony`]). A project that writes any writes its whole list,
//! and a key a written stamp leaves out is the Symphony's own for that name. A stamp's name is also
//! how the land knows it stands, its reader ([`READERS`]): a name the Symphony has no reader for is
//! refused where the file is read, here and by the Symphony's own reading of the file.
//!
//! A task's needs are its assessment ([`Needs`]): the stamps it needs, the tip the assessment read
//! (none before the task has a branch), who made it and why, and, for a head that imports an
//! upstream's history, the upstream commit it imports. They are the board's own field
//! `fields._board.needs` ([`NEEDS`]), out of `set_field`'s reach, which only `just tasks needs`
//! writes ([`crate::Store::set_needs`]), each change a line on the task's thread ([`Needs::line`]).
//! The assessor that writes them is a role in name only: the write itself is refused to a session
//! that executed the task or work under it, orchestrates or orchestrated it, or filed it, the rule a
//! reviewer's verdict is held to ([`crate::Held::unfresh`]). Each stamp's reader, and which
//! stamps a task needs at a tip, are [`read`]'s; the task page's gates draw them, and the land reads
//! the needs and lands by them ([`crate::land::needs`]).

use crate::land::MANIFEST;
use crate::sweep::git::{git, succeeds};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub mod posted;
pub mod read;

/// The stamps the Symphony has a reader for, by name, in the order the catalogue is listed.
pub const READERS: [&str; 4] = ["tests", "review", "pictures", "owner"];
/// The board's own field holding a task's needs ([`crate::red::FIELDS`]`.`[`NEEDS`]).
pub const NEEDS: &str = "needs";
/// What `just tasks needs` names for a task that needs no stamp.
pub const NONE: &str = "none";

/// One stamp as the Symphony has it: its name, whether a task nobody assessed needs it, whether a
/// gate decision may waive it for one tip ([`crate::gates`]), how the land knows it stands, and
/// where it applies and where it does not.
struct Own {
    name: &'static str,
    default: bool,
    waivable: bool,
    met: &'static str,
    applies: &'static str,
    skips: &'static str,
}

/// The Symphony's own catalogue, in [`READERS`]' order (the design's "The default catalogue").
const OWN: [Own; 4] = [
    Own {
        name: "tests",
        waivable: false,
        default: false,
        met: "the task's recorded runs at the tip, its own or work under it: the newest run there \
              of each build and test command it ran is green, and one test run at least is among \
              them; the executor's to produce",
        applies: "any change to code, scripts, recipes, build settings, or a file a test reads (a \
                  fixture, a shipped skill whose install is tested): the tests closest to a change \
                  are what proves it works; a change of the values the project file gives its \
                  settings, which the settings check proves alone (`symphony settings-check`, \
                  recorded as a test run: every table and key known, each value of its type, no \
                  model or recipe judged against a list), with no other suite",
        skips: "documents and guides alone; pictures and other files nothing tests; a hotfix whose \
                break no test shows in the time it has, its tests coming with its postmortem",
    },
    Own {
        name: "review",
        waivable: false,
        default: true,
        met: "a fresh reviewer's approve naming the tip, or the commit the tip merges the default \
              branch into where that merge settles conflicts in documents alone, changes \
              nothing else and the tests are green at the tip, from a session that never \
              executed, orchestrated or filed the task",
        applies: "code whose mistake reaches past the task, into what every agent runs or every \
                  client reads (a store, a schema, a board command, the landing, a launcher, a \
                  hook, a recipe, a wire type, a route, a shared component); a change to a test, a \
                  check, a budget or a run step, which is any path [land] check_paths names, since \
                  a loosened test is how a red turns green unfixed; a shipped skill or a rule every \
                  agent follows; a setting the Symphony does not read yet or reads another way, \
                  which is code; a value of the project file that is a command or a program the \
                  Symphony runs or that decides what lands ([project] cargo and default_branch, \
                  [plugins] crates and process, [verdicts], [land], [releases], [stamps], the \
                  workflow's [workflow], [[workflow_mappings]] and [[workflow_authorities]], \
                  [board] on_review, on_land, machine_work_reader, postgresql, \
                  external_postgresql and assessments); a change that does more than one thing, \
                  or one the assessor cannot place",
        skips: "documents alone (a design, a reference page, a record); the values the project \
                file gives settings the Symphony already reads (a model, an agent, a limit, a \
                count, a name), written, changed or removed, with the guide's and the rules' \
                words that repeat those values and nothing else: the settings check green at the \
                tip stands in the reviewer's place, as `tests`; a fix of a few lines that \
                leaves the failing test its task names unchanged; a hotfix restoring something \
                wholly broken, read afterwards as its postmortem",
    },
    Own {
        name: "pictures",
        waivable: true,
        default: false,
        met: "a post on the task's thread naming the tip and showing at least one picture (a file \
              under the task's artifacts folder, or an upload); the executor's to produce",
        applies: "a change to what a page shows or how it lays out (a view, a style sheet, a theme \
                  token, a page's words): before and after, phone and wide, dark and light where \
                  colour changed",
        skips: "a change no page shows (what a page reads, caches or weighs, a route's answer), and \
                anything off the pages",
    },
    Own {
        name: "owner",
        waivable: false,
        default: false,
        met: "the owner's own approve naming the tip, given on the task's page",
        applies: "a result the owner asked, in the brief or on the thread, to see before it lands: \
                  a new page or look, a behaviour they will use themselves",
        skips: "anything else: a fresh reviewer stands for the owner, and nothing waits on the \
                owner unasked",
    },
];

/// One stamp of a catalogue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    /// Its name, one of [`READERS`]: how the land knows it stands.
    pub name: &'static str,
    /// Whether a task no assessment has read needs it.
    pub default: bool,
    /// Whether a gate decision may waive it for one tip ([`crate::gates`]): never `tests`, `review`
    /// or `owner` ([`crate::gates::NEVER`]).
    pub waivable: bool,
    /// How the land knows it stands: the Symphony's, as its reader is.
    pub met: &'static str,
    /// Where the assessor needs it.
    pub applies: String,
    /// Where it does not.
    pub skips: String,
}

/// The stamps a project's land can require ([`Catalogue::of`]), in [`READERS`]' order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalogue {
    pub stamps: Vec<Stamp>,
    /// Whether the project wrote its own (`[stamps]`), else it is the Symphony's.
    pub written: bool,
}

/// A stamp as `[stamps.NAME]` writes it: each key left out, or written empty, the Symphony's own.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Written {
    default: Option<bool>,
    waivable: Option<bool>,
    applies: Option<String>,
    skips: Option<String>,
}

#[derive(Default, Deserialize)]
struct Manifest {
    #[serde(default)]
    stamps: Option<BTreeMap<String, Written>>,
}

/// `value` unless it is empty or blank, else `own`.
fn or_own(value: Option<String>, own: &str) -> String {
    value
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| own.to_string())
}

impl Catalogue {
    /// The Symphony's own four, a project's with no `[stamps]`: `review` needed by default, which
    /// is the rule a land had before stamps.
    pub fn symphony() -> Catalogue {
        Catalogue {
            stamps: OWN
                .iter()
                .map(|own| Stamp {
                    name: own.name,
                    default: own.default,
                    waivable: own.waivable,
                    met: own.met,
                    applies: own.applies.into(),
                    skips: own.skips.into(),
                })
                .collect(),
            written: false,
        }
    }

    /// `text`, a project file: its `[stamps]`, else the Symphony's own. Refused, by the file and
    /// the table: a file that does not parse, a stamp the Symphony has no reader for (each named,
    /// with its line), a key a stamp does not have, and a table naming no stamp.
    pub fn parse(text: &str) -> Result<Catalogue, String> {
        let file: Manifest = toml::from_str(text)
            .map_err(|error| format!("{MANIFEST}: {}", error.to_string().trim_end()))?;
        let Some(mut written) = file.stamps else {
            return Ok(Catalogue::symphony());
        };
        let unknown: Vec<String> = written
            .keys()
            .filter(|name| !READERS.contains(&name.as_str()))
            .map(|name| match crate::limits::line(text, &["stamps"], name) {
                Some(line) => format!("{name:?} (line {line})"),
                None => format!("{name:?}"),
            })
            .collect();
        if !unknown.is_empty() {
            let (named, has) = match unknown.len() {
                1 => ("a stamp", "it has"),
                _ => ("stamps", "they have"),
            };
            return Err(format!(
                "{MANIFEST}: [stamps] names {named} the Symphony has no reader for, {}: {has} no \
                 way to stand on a tip; a stamp is one of {}",
                unknown.join(", "),
                READERS.join(", ")
            ));
        }
        if written.is_empty() {
            return Err(format!(
                "{MANIFEST}: [stamps] names no stamp: leave the table out for the Symphony's own \
                 four, or write each stamp the project's land can require"
            ));
        }
        let never: Vec<String> = (crate::gates::NEVER.iter())
            .filter(|name| {
                written
                    .get(**name)
                    .is_some_and(|w| w.waivable == Some(true))
            })
            .map(
                |name| match crate::limits::line(text, &["stamps", name], "waivable") {
                    Some(line) => format!("{name:?} (line {line})"),
                    None => format!("{name:?}"),
                },
            )
            .collect();
        if !never.is_empty() {
            return Err(format!(
                "{MANIFEST}: [stamps] marks waivable {}, which no gate decision waives: {}",
                never.join(", "),
                crate::gates::NEVER_WHY
            ));
        }
        let stamps = OWN
            .iter()
            .filter_map(|own| {
                let stamp = written.remove(own.name)?;
                Some(Stamp {
                    name: own.name,
                    default: stamp.default.unwrap_or(own.default),
                    waivable: stamp.waivable.unwrap_or(own.waivable),
                    met: own.met,
                    applies: or_own(stamp.applies, own.applies),
                    skips: or_own(stamp.skips, own.skips),
                })
            })
            .collect();
        Ok(Catalogue {
            stamps,
            written: true,
        })
    }

    /// The catalogue the main checkout `main` has committed at its head, as the land reads its
    /// settings there ([`crate::land::Settings::of`]): the Symphony's own with no file.
    pub fn of(main: &Path) -> Result<Catalogue, String> {
        Catalogue::committed(main, "HEAD")
    }

    /// The catalogue the commit `commit` of the main checkout `main`'s repository holds: the
    /// Symphony's own with no file there. What a land that merged is judged by is the one its
    /// merge's first parent holds, the head's being by then the branch's own.
    pub fn committed(main: &Path, commit: &str) -> Result<Catalogue, String> {
        let at = format!("{commit}:{MANIFEST}");
        if !succeeds(main, &["cat-file", "-e", &at]) {
            return Ok(Catalogue::symphony());
        }
        let shown = git(main, &["show", &at])
            .filter(|done| done.status.success())
            .ok_or_else(|| format!("git cannot read {at} in {}", main.display()))?;
        Catalogue::parse(&String::from_utf8_lossy(&shown.stdout))
    }

    /// The catalogue of the repository the folder `dir` is in ([`Catalogue::of`] its main
    /// checkout): the Symphony's own outside a repository, or in one with no main checkout.
    pub fn at(dir: &Path) -> Result<Catalogue, String> {
        match Catalogue::main_of(dir) {
            Some(main) => Catalogue::of(&main),
            None => Ok(Catalogue::symphony()),
        }
    }

    /// The main checkout of the repository the folder `dir` is in, whose head's manifest
    /// [`Catalogue::at`] reads: none outside a repository, or in one with no main checkout. A
    /// reader that keeps a catalogue keeps it while that head is the commit it read it at.
    pub fn main_of(dir: &Path) -> Option<PathBuf> {
        (crate::git::common_dir(dir).ok())
            .and_then(|common| crate::sweep::trigger::main_checkout(&common).map(Path::to_path_buf))
    }

    /// Its stamps' names, in order.
    pub fn names(&self) -> Vec<&'static str> {
        self.stamps.iter().map(|stamp| stamp.name).collect()
    }

    /// Why a gate decision may not waive the stamp `name`, none when it may: a stamp the catalogue
    /// lacks, one never waived ([`crate::gates::NEVER`]), or one the catalogue does not mark
    /// waivable.
    pub fn unwaivable(&self, name: &str) -> Option<String> {
        let waivable: Vec<&str> = (self.stamps.iter())
            .filter(|stamp| stamp.waivable)
            .map(|stamp| stamp.name)
            .collect();
        let these = match waivable.as_slice() {
            [] => "the catalogue marks none waivable".to_string(),
            names => format!("the catalogue's waivable stamps are {}", names.join(", ")),
        };
        if crate::gates::NEVER.contains(&name) {
            return Some(format!(
                "{name} is never waived: {}; {these}",
                crate::gates::NEVER_WHY
            ));
        }
        match self.stamps.iter().find(|stamp| stamp.name == name) {
            None => Some(format!(
                "the catalogue has no stamp {name:?}: its stamps are {} (`just tasks stamps` lists \
                 them); a raise names a number in a file, PATH:NAME",
                self.names().join(", ")
            )),
            Some(stamp) if !stamp.waivable => Some(format!(
                "the catalogue does not mark {name} waivable ([stamps.{name}] waivable); {these}"
            )),
            Some(_) => None,
        }
    }

    /// What a task no assessment has read needs: the stamps needed by default, in order.
    pub fn defaults(&self) -> Vec<String> {
        (self.stamps.iter())
            .filter(|stamp| stamp.default)
            .map(|stamp| stamp.name.to_string())
            .collect()
    }

    /// The stamps `named` as an assessment names them, in the catalogue's order and each once:
    /// [`NONE`] alone is none. Refused: nothing named, [`NONE`] beside a stamp, and a name the
    /// catalogue lacks.
    pub fn needs(&self, named: &[String]) -> Result<Vec<String>, String> {
        let names = self.names();
        let listed = names.join(", ");
        let named: Vec<&str> = named.iter().map(|name| name.trim()).collect();
        match named.as_slice() {
            [] => {
                return Err(format!(
                    "name the stamps the task needs, of {listed}, or {NONE} for none"
                ));
            }
            [only] if *only == NONE => return Ok(Vec::new()),
            _ if named.contains(&NONE) => {
                return Err(format!(
                    "{NONE} is no stamp: name it alone for a task that needs none, or name the \
                     stamps it needs"
                ));
            }
            _ => {}
        }
        if let Some(lacked) = named.iter().find(|name| !names.contains(name)) {
            return Err(format!(
                "the catalogue has no stamp {lacked:?}: its stamps are {listed} \
                 (`just tasks stamps` lists them)"
            ));
        }
        Ok(names
            .into_iter()
            .filter(|name| named.contains(name))
            .map(str::to_string)
            .collect())
    }
}

/// A task's needs, the board's own field ([`NEEDS`]): its assessment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Needs {
    /// The stamps it needs, in the catalogue's order; none for everything waived.
    #[serde(default)]
    pub stamps: Vec<String>,
    /// The whole commit the assessment read, none when it read the brief alone.
    #[serde(default)]
    pub tip: Option<String>,
    /// The session that made it.
    #[serde(default)]
    pub by: String,
    /// Why: one line for each stamp it left out or added.
    #[serde(default)]
    pub why: String,
    /// The whole upstream commit the head it read imports, where the assessor names it so: what
    /// a land delivered by a pull request merges by a merge commit rather than squashing
    /// (`crate::land::delivery`). Only ever with a tip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<String>,
}

/// `stamps` as a line says them: `tests, review`, or `none`.
pub fn said(stamps: &[String]) -> String {
    if stamps.is_empty() {
        NONE.into()
    } else {
        stamps.join(", ")
    }
}

/// The first seven characters of `sha`, as a line names a commit.
pub fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

impl Needs {
    /// What a task's `fields` record of its needs, when they record any.
    pub fn of(fields: &BTreeMap<String, BTreeMap<String, Value>>) -> Option<Needs> {
        let value = fields.get(crate::red::FIELDS)?.get(NEEDS)?;
        crate::objects::read(value).ok()
    }

    /// Where the assessment stands: `at 1a2b3c4`, or `from the brief` when it read no tip; with
    /// `, an import of 5e6f7a8` after it where it names one.
    pub fn read(&self) -> String {
        let read = match &self.tip {
            Some(tip) => format!("at {}", short(tip)),
            None => "from the brief".into(),
        };
        match &self.import {
            Some(import) => format!("{read}, an import of {import}"),
            None => read,
        }
    }

    /// The thread's line for these needs written over `was` (the stamps the task needed before:
    /// its last assessment's, else the catalogue's defaults): `needs tests, review at 1a2b3c4 (was
    /// review): WHY; by HANDLE`.
    pub fn line(&self, was: &[String]) -> String {
        format!(
            "needs {} {} (was {}): {}; by {}",
            said(&self.stamps),
            self.read(),
            said(was),
            self.why,
            self.by
        )
    }
}

impl crate::Task {
    /// Its needs, when an assessment wrote them ([`Needs::of`]).
    pub fn needs(&self) -> Option<Needs> {
        Needs::of(&self.fields)
    }
}

/// The task `slug` and every task under it, at any depth, whose runs are its own for the `tests`
/// stamp ([`read::tests`]): read a level at a time, so a task with no subtask costs one small read.
/// A level that cannot be read adds nothing.
pub fn family(store: &crate::Store, slug: &str) -> Vec<String> {
    let mut found = vec![slug.to_string()];
    let mut next = 0;
    while let Some(at) = found.get(next).cloned() {
        for child in store.children(&at).unwrap_or_default() {
            if !found.contains(&child.slug) {
                found.push(child.slug);
            }
        }
        next += 1;
    }
    found
}

#[cfg(test)]
mod tests;
