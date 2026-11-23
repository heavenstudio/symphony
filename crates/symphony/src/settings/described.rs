//! The entries the Symphony does not say itself, each in the one shape its own take: a program's,
//! the answer of one command the page's read runs to read it ([`program`]), and a project's
//! plugins', each from a describer of a plugin the project selects, linked or answering from its
//! process ([`plugins`]). Each is drawn by the component that draws the Symphony's own.
//!
//! A description declares and runs nothing, and its class is not its describer's to lower
//! ([`judged`]): a setting the Symphony would run or that decides what lands (a key the local
//! settings file refuses, the class [`super::class`] gives its key) is shown reviewed whatever its
//! describer says, and so is every setting of a program whose settings decide what lands. A
//! describer may mark a setting of its own reviewed, and none is written in the board's store or
//! said to come from it, as the store keeps the Symphony's own preferences alone. A program
//! absent, or one that fails, and a describer that cannot say, are said in the entry's place, and
//! the page is drawn all the same.

use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use symphony_api::{EntryNeed, EntryState, Setting, SettingClass, SettingScope, SettingsEntry};
use symphony_board::{builds, sys};

use super::{Machine, entry};
use crate::plugins::Plugins;

/// How long a program has to answer: a plugin call's wait ([`crate::plugins::process::WAIT`]).
pub const ANSWERS_IN: Duration = crate::plugins::process::WAIT;

/// A program that describes itself on the page: run as this machine finds it, with the words that
/// ask for its description, in the project's main checkout.
pub struct Program {
    /// Its name, as the main checkout's build or the search path holds it.
    pub name: &'static str,
    /// The words after it that ask for its entry.
    pub args: &'static [&'static str],
    /// Its entry's id on the page, whatever id its answer gives.
    pub id: &'static str,
    /// Its entry's name and what it is for, said in its place while it does not answer.
    pub title: &'static str,
    pub about: &'static str,
    /// Whether its settings decide what lands or what reaches an outside service: each is then
    /// reviewed, whatever it says.
    pub lands: bool,
}

/// The GitHub exchange (docs/operations/GITHUB.md): `symphony-github describe`. Its settings
/// decide what lands and what is written to GitHub.
pub const GITHUB: Program = Program {
    name: "symphony-github",
    args: &["describe"],
    id: "github",
    title: "GitHub",
    about: "Issues exchanged, lands delivered by pull request, releases published.",
    lands: true,
};

/// `program`'s entry, its own answer as this machine's build of it gives it for the project whose
/// main checkout is `main`, judged ([`judged`]), with the program as its first need; absent, or
/// failing, its entry says so and what holds. Its id is `program`'s.
pub fn program(machine: &Machine, main: &Path, program: &Program) -> SettingsEntry {
    let Some(found) = builds::of_main(main, program.name, &machine.beyond) else {
        return SettingsEntry {
            state: unknown(format!(
                "Unknown: its state is its program's to say, and this machine has no {}: nothing \
                 is exchanged from here.",
                program.name
            )),
            needs: vec![EntryNeed {
                what: format!("the {} program", program.name),
                met: false,
                said: format!(
                    "Not found on this machine: {}.",
                    builds::missing(main, program.name)
                ),
            }],
            ..entry(program.id, program.title, program.about)
        };
    };
    let need = EntryNeed {
        what: format!("the {} program", program.name),
        met: true,
        said: format!("Found on this machine: {}.", found.display()),
    };
    let asked = format!("{} {}", program.name, program.args.join(" "));
    let answer = run(
        sys::command(&found).args(program.args).current_dir(main),
        &asked,
        ANSWERS_IN,
    )
    .and_then(|printed| {
        serde_json::from_slice::<SettingsEntry>(&printed)
            .map_err(|e| format!("{asked} printed what does not read as an entry: {e}"))
    });
    match answer {
        Ok(described) => {
            let mut judged = judged(described, program.id.into(), program.lands);
            judged.needs.insert(0, need);
            judged
        }
        Err(why) => SettingsEntry {
            state: unknown(format!("Unknown: its program could not say: {why}.")),
            needs: vec![need],
            ..entry(program.id, program.title, program.about)
        },
    }
}

/// What `command`, said as `asked`, printed on its stdout, once it exited 0 within `within`; else
/// why not: it did not start, it exited otherwise (its code and the first line it said on
/// stderr), or it gave no answer in time, and is killed.
fn run(command: &mut sys::Command, asked: &str, within: Duration) -> Result<Vec<u8>, String> {
    let child = sys::spawn_output(command).map_err(|e| format!("{asked} did not start: {e}"))?;
    let pid = i64::from(child.id());
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(sys::wait_output(child));
    });
    let done = match receive.recv_timeout(within) {
        Ok(done) => done.map_err(|e| format!("{asked} could not be read: {e}"))?,
        Err(_) => {
            sys::terminate(pid, true);
            let said = if within.subsec_millis() == 0 {
                format!("{} s", within.as_secs())
            } else {
                format!("{} ms", within.as_millis())
            };
            return Err(format!("{asked} gave no answer in {said}"));
        }
    };
    if done.status.success() {
        return Ok(done.stdout);
    }
    let said = String::from_utf8_lossy(&done.stderr);
    let first = (said.lines().map(str::trim)).find(|line| !line.is_empty());
    let code = done
        .status
        .code()
        .map_or_else(|| "was killed".to_string(), |code| format!("exited {code}"));
    Err(match first {
        Some(first) => format!("{asked} {code}: {first}"),
        None => format!("{asked} {code}"),
    })
}

/// The entries of every describer of `plugins`, in the project's order of its plugins and each
/// plugin's of its describers, for the project at `root`, each judged ([`judged`]) and its id the
/// plugin's name and the describer's (`plugin-NAME-ID`), one an earlier entry took counted on
/// ([`anchor`]). One that cannot say is an entry of its id saying why.
pub fn plugins(plugins: &Plugins, root: &Path) -> Vec<SettingsEntry> {
    let mut taken = HashSet::new();
    plugins
        .describers()
        .into_iter()
        .map(|(plugin, describer)| {
            let id = anchor(&mut taken, format!("plugin-{plugin}-{}", describer.id()));
            match describer.describe(root) {
                Ok(described) => judged(described, id, false),
                Err(why) => SettingsEntry {
                    state: unknown(format!(
                        "Unknown: the plugin {plugin} could not say: {why}."
                    )),
                    ..entry(
                        &id,
                        describer.id(),
                        &format!("Described by the plugin {plugin}."),
                    )
                },
            }
        })
        .collect()
}

/// `id`, the page's anchor of an entry, as none of `taken` is, which it then holds: where an entry
/// before took it, `id` and the first count from 2 that none did (`plugin-x-y-z-2`). Two
/// describers of one id give one anchor, and so do plugin `x`'s describer `y-z` and plugin `x-y`'s
/// describer `z`.
fn anchor(taken: &mut HashSet<String>, id: String) -> String {
    let id = if taken.contains(&id) {
        (2..)
            .map(|count| format!("{id}-{count}"))
            .find(|counted| !taken.contains(counted))
            .expect("a count is free")
    } else {
        id
    };
    taken.insert(id.clone());
    id
}

/// `described` as the page shows it: its id `id`, none of its settings written in the board's
/// store or come from it ([`unstored`]), and each setting reviewed that its describer may not call
/// a preference, every one where `all` says so, each then written in the project's scope alone
/// ([`reviewed`]).
pub fn judged(described: SettingsEntry, id: String, all: bool) -> SettingsEntry {
    SettingsEntry {
        id,
        settings: described
            .settings
            .into_iter()
            .map(|setting| {
                let setting = unstored(setting);
                if all || protected(&setting.key) {
                    reviewed(setting)
                } else {
                    setting
                }
            })
            .collect(),
        ..described
    }
}

/// `setting` with nothing of the board's store, whatever its describer says: the store keeps the
/// Symphony's own preferences, each drawn in the Symphony's own entry, and no description's. So
/// it loses the board's scope, and the page draws no control for it ([`super::stored::list`]
/// lists only a setting whose scopes hold the board's) and names no store it is written in; and a
/// source its describer put in the board's store is no file's, which the page tags as the default,
/// its describer's own words beside the tag. A describer that gave the board's scope alone leaves
/// its setting with none: the page draws its value and its source, as any setting it writes in no
/// scope, and no control.
fn unstored(mut setting: Setting) -> Setting {
    setting.scopes.retain(|scope| *scope != SettingScope::Board);
    if setting.source.scope == Some(SettingScope::Board) {
        setting.source.scope = None;
    }
    setting
}

/// `setting`, reviewed: a reviewed task changes it in the committed file, so it is written in the
/// project's scope alone.
fn reviewed(setting: Setting) -> Setting {
    Setting {
        class: SettingClass::Reviewed,
        scopes: vec![SettingScope::Project],
        ..setting
    }
}

/// Whether `key`, as the page spells a setting (`[land] delivery`, `[agents.launch] model`,
/// `[land]` for a table said as a sentence, `[[workflow_mappings]] guard` in a list of tables, or
/// the store's `land.delivery`), is reviewed: the class [`super::class`] gives the key of its
/// top-level table that holds it, its own key or, below a table of that table, the lower table's
/// name, and the table said whole by its name alone.
pub fn protected(key: &str) -> bool {
    let key = key.trim();
    let bracketed = (key.strip_prefix("[[").and_then(|rest| rest.split_once("]]")))
        .or_else(|| key.strip_prefix('[').and_then(|rest| rest.split_once(']')));
    let (tables, last) = match bracketed {
        Some((tables, last)) => (tables, last.trim()),
        None => key.rsplit_once('.').unwrap_or(("", key)),
    };
    let mut path = tables.split('.').map(str::trim);
    let top = path.next().unwrap_or_default();
    super::class(top, path.next().unwrap_or(last)) == SettingClass::Reviewed
}

/// A state neither on nor off, `why` said.
fn unknown(why: String) -> Option<EntryState> {
    Some(EntryState {
        word: "Unknown".into(),
        on: None,
        why,
    })
}

#[cfg(test)]
#[path = "described_tests.rs"]
mod tests;
