//! The Settings page's read ([`symphony_api::SettingsPage`]): everything in force for the project
//! at a root, each value with where it was written, as a list of entries in one shape. It reads
//! and never writes, runs no program and reaches no outside service: a program an entry needs is
//! looked for on the search path, and the GitHub exchange's settings are read from the commit of
//! the default branch, as that program reads them.
//!
//! A value of the project file is the one the file writes while the project loads, else its
//! default; while the file does not load the project runs on every default, and the page says so
//! ([`Project::problem`]). The limits, the launch's agent and the roles' models are the board's to
//! read, from the main checkout's file with the local settings file over it
//! (`symphony_board::limits`, `symphony_board::launch::Defaults`), and their sources are named as
//! `just tasks limits` names them.
//!
//! A setting's class comes from the lists the local settings file's reader holds
//! (`symphony_board::local::PROTECTED`, `PROTECTED_KEYS`) and from no other list ([`class`]): what
//! that file refuses as holding a command or deciding what lands is reviewed, and every other
//! key of the project file is a preference. The scopes a setting may be written in come from the
//! same reader ([`scopes`]): the machine's where the local file may hold the key.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use symphony_api::{
    EntryNeed, EntryState, Setting, SettingClass, SettingKind, SettingScope, SettingSource,
    SettingsEntry, SettingsFile, SettingsPage, SettingsSection,
};
use symphony_board::launch::Defaults;
use symphony_board::launch::defaults::{AGENTS, Chosen, ROLES};
use symphony_board::limits::{self, Written};
use symphony_board::local::{self, Local};
use symphony_board::{alarm, builds};
use toml::Value;

use crate::project::{FILE, LOCAL_FILE, PANES, Project, Releases};

/// The GitHub exchange's settings file, at the root of the tree the default branch commits
/// (docs/GITHUB.md), and its program.
pub const GITHUB_FILE: &str = "symphony-github.toml";
const GITHUB_PROGRAM: &str = "symphony-github";

/// What of this machine the read looks at: where its programs are found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Machine {
    /// The machine folder and the search path a program is looked for in.
    pub beyond: builds::Beyond,
    /// The program that drives Herdr's terminals (`HC_BIN`, else `hc`).
    pub hc: String,
}

impl Machine {
    /// This process's.
    pub fn of_process() -> Machine {
        Machine {
            beyond: builds::Beyond::of_process(),
            hc: std::env::var("HC_BIN")
                .ok()
                .and_then(|hc| hc.split_whitespace().next().map(str::to_string))
                .unwrap_or_else(|| "hc".into()),
        }
    }

    /// A search path alone, and `hc` by its name.
    pub fn path(path: Option<OsString>) -> Machine {
        Machine {
            beyond: builds::Beyond {
                machine: None,
                path,
            },
            hc: "hc".into(),
        }
    }

    /// `program` as this machine finds it: a path as it is, a name on the search path.
    fn find(&self, program: &str) -> Option<PathBuf> {
        if program.contains('/') {
            let path = PathBuf::from(program);
            return builds::runnable(&path).then_some(path);
        }
        builds::on_path(program, self.beyond.path.as_deref())
    }
}

/// The class of `key` in the top-level table `table` of the project file: reviewed where the
/// local settings file's reader refuses it as holding a command or deciding what lands, the whole
/// table or the key by name; a preference otherwise.
pub fn class(table: &str, key: &str) -> SettingClass {
    let reviewed = local::PROTECTED.contains(&table)
        || local::PROTECTED_KEYS
            .iter()
            .any(|(of, keys)| *of == table && keys.contains(&key));
    if reviewed {
        SettingClass::Reviewed
    } else {
        SettingClass::Preference
    }
}

/// The scopes `key` of the table `path` leads to may be written in: the project's, and the
/// machine's too where the local settings file may hold it.
pub fn scopes(path: &[&str], key: &str) -> Vec<SettingScope> {
    let machine = match path {
        [table] => *table == limits::TABLE && limits::KEYS.contains(&key),
        ["agents", "launch"] => local::LAUNCH.contains(&key),
        ["agents", "models"] => ROLES.contains(&key),
        _ => false,
    };
    let mut scopes = vec![SettingScope::Project];
    if machine {
        scopes.push(SettingScope::Machine);
    }
    scopes
}

/// A source no file wrote: the default, with what it is.
fn default(what: &str) -> SettingSource {
    SettingSource {
        scope: None,
        said: if what.is_empty() {
            "default".into()
        } else {
            format!("default: {what}")
        },
        file: None,
        line: None,
        over: None,
    }
}

/// `written`, as the board names a setting in force, as a source: the local file's when its name
/// opens it, else the project file's, named before it.
fn source(written: &Written) -> SettingSource {
    let local = written.at.starts_with(LOCAL_FILE);
    let (scope, file, said) = if local {
        (SettingScope::Machine, LOCAL_FILE, written.said())
    } else {
        (
            SettingScope::Project,
            FILE,
            format!("{FILE} {}", written.said()),
        )
    };
    SettingSource {
        scope: Some(scope),
        said,
        file: Some(file.into()),
        line: written.line.and_then(|line| u32::try_from(line).ok()),
        over: None,
    }
}

/// A value as the page shows it: text as it is, a number, `on` or `off`, a list's texts with
/// spaces between (a command reads as typed), and a table by its keys.
fn shown(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Integer(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Boolean(true) => "on".into(),
        Value::Boolean(false) => "off".into(),
        Value::Datetime(when) => when.to_string(),
        Value::Array(items) => items.iter().map(shown).collect::<Vec<_>>().join(" "),
        Value::Table(table) => table.keys().cloned().collect::<Vec<_>>().join(", "),
    }
}

/// A project file as written: its text, for the lines, and its tables. Empty while there is no
/// file or it does not load, so every value reads as its default.
#[derive(Default)]
struct FileRead {
    text: String,
    table: toml::Table,
}

impl FileRead {
    fn of(text: Option<String>) -> FileRead {
        let text = text.unwrap_or_default();
        let table = toml::from_str(&text).unwrap_or_default();
        FileRead { text, table }
    }

    /// What the file writes for `key` of the table `path` leads to.
    fn get(&self, path: &[&str], key: &str) -> Option<&Value> {
        let mut table = &self.table;
        for name in path {
            table = table.get(*name)?.as_table()?;
        }
        table.get(key)
    }

    /// Where `key` is written, spelt `at`, or none when the file does not write it.
    fn written(&self, at: &str, path: &[&str], key: &str) -> Option<Written> {
        self.get(path, key).map(|_| Written {
            at: at.into(),
            line: limits::line(&self.text, path, key),
        })
    }

    /// `key` of the table `path` leads to as a setting: the value the file writes, else `unset`,
    /// what holds with none written.
    fn setting(
        &self,
        path: &[&str],
        key: &str,
        name: &str,
        kind: SettingKind,
        unset: &str,
    ) -> Setting {
        let at = format!("[{}] {key}", path.join("."));
        let value = self.get(path, key);
        Setting {
            name: name.into(),
            kind,
            value: value.map_or_else(|| unset.to_string(), shown),
            choices: Vec::new(),
            source: self
                .written(&at, path, key)
                .map_or_else(|| default(""), |written| source(&written)),
            scopes: scopes(path, key),
            class: class(path.first().copied().unwrap_or_default(), key),
            key: at,
        }
    }

    /// The top-level table `table` whole, said as `sentence`: on the line its header stands on,
    /// or the default with none written.
    fn sentence(&self, table: &str, name: &str, sentence: String) -> Setting {
        // A list of tables is spelt as the file spells one.
        let at = match self.table.get(table) {
            Some(Value::Array(_)) => format!("[[{table}]]"),
            _ if table == "entrypoints" => format!("[[{table}]]"),
            _ => format!("[{table}]"),
        };
        Setting {
            name: name.into(),
            kind: SettingKind::Sentence,
            value: sentence,
            choices: Vec::new(),
            source: self.written(&at, &[], table).map_or_else(
                || default(&format!("no {at} written")),
                |written| source(&written),
            ),
            scopes: vec![SettingScope::Project],
            class: class(table, ""),
            key: at,
        }
    }
}

/// An entry with a name and what it is for, and nothing else yet.
fn entry(id: &str, name: &str, about: &str) -> SettingsEntry {
    SettingsEntry {
        id: id.into(),
        name: name.into(),
        about: about.into(),
        state: None,
        needs: Vec::new(),
        settings: Vec::new(),
        source: None,
    }
}

fn state(word: &str, on: Option<bool>, why: impl Into<String>) -> Option<EntryState> {
    Some(EntryState {
        word: word.into(),
        on,
        why: why.into(),
    })
}

/// Whether this machine has `program`, as an entry's need; `missing` is what its absence means.
fn program(machine: &Machine, program: &str, missing: &str) -> EntryNeed {
    let found = machine.find(program);
    EntryNeed {
        what: format!("the {program} program"),
        met: found.is_some(),
        said: match found {
            Some(path) => format!("Found on this machine: {}.", path.display()),
            None => format!("Not found on this machine: {missing}."),
        },
    }
}

/// The settings of the project at `root`, as `machine` finds its programs.
pub fn page(root: &Path, machine: &Machine) -> SettingsPage {
    let main = builds::main_of(root);
    let project = Project::of(root);
    let problem = Project::problem(root);
    let project_file = root.join(FILE);
    let board_file = main.join(FILE);
    let local_file = local::path_beside(&board_file);
    let text = std::fs::read_to_string(&project_file).ok();
    let mut problems = Vec::new();
    if let Some(problem) = &problem {
        problems.push(format!(
            "{problem}; the project runs on every default until it loads"
        ));
    }
    // What the file writes counts only while the project loads: else every value is its default.
    let file = FileRead::of(text.clone().filter(|_| problem.is_none()));
    // The board's own reading: the main checkout's file, whatever the project's load says.
    let board_text = std::fs::read_to_string(&board_file).unwrap_or_default();
    let board = FileRead::of(Some(board_text));
    let read = limits::read_at(&board_file);
    problems.extend(read.problems.iter().cloned());
    let held = local::beside(&board_file).ok().flatten();
    let defaults = Defaults::configured(&main).unwrap_or_else(|problem| {
        problems.push(format!(
            "{problem}; the launch and the models read as the board's own"
        ));
        Defaults::default()
    });
    let files = vec![
        SettingsFile {
            name: FILE.into(),
            scope: SettingScope::Project,
            path: project_file.display().to_string(),
            found: text.is_some(),
        },
        SettingsFile {
            name: LOCAL_FILE.into(),
            scope: SettingScope::Machine,
            path: local_file.display().to_string(),
            found: local_file.is_file(),
        },
    ];
    let sections = vec![
        SettingsSection {
            id: "integrations".into(),
            title: "Integrations and tools".into(),
            about: "Each one says whether it is on and whether this machine has what it needs."
                .into(),
            entries: vec![
                agent(machine, &defaults, "claude", "Claude Code"),
                agent(machine, &defaults, "codex", "Codex"),
                herdr(machine, &file, &project),
                github(machine, &main),
                plugins(&file, &project),
            ],
        },
        SettingsSection {
            id: "models".into(),
            title: "Models and limits".into(),
            about: "What a launch starts, the model each role runs on and the limits the board \
                    holds work to: the project's, or this machine's where its local file writes \
                    one."
                .into(),
            entries: vec![
                launch(&board, &defaults, held.as_ref()),
                models(&board, &defaults, held.as_ref()),
                limits_entry(&board, &read),
                alarm_entry(&read),
            ],
        },
        SettingsSection {
            id: "project".into(),
            title: "The project".into(),
            about: "What the pages call it, what they serve and where its files are kept.".into(),
            entries: vec![
                identity(&file, &project),
                documents(&file, &project),
                look(&file),
            ],
        },
        SettingsSection {
            id: "reviewed".into(),
            title: "Shown here, changed by a reviewed task".into(),
            about: "These run a command on this machine or decide what lands. The page shows \
                    them and never writes them."
                .into(),
            entries: vec![
                landing(&file, &project),
                commands(&file),
                stamps(&board),
                releases(&file),
                workflow(&file),
            ],
        },
        SettingsSection {
            id: "device".into(),
            title: "This device".into(),
            about: "What this browser keeps, for this project alone.".into(),
            entries: vec![appearance()],
        },
    ];
    SettingsPage {
        files,
        problems,
        sections,
    }
}

/// One of the two agents a launch starts: on, as nothing turns one off, and whether its program
/// is here.
fn agent(machine: &Machine, defaults: &Defaults, agent: &str, name: &str) -> SettingsEntry {
    let default = defaults.agent.unwrap_or(AGENTS[0]) == agent;
    SettingsEntry {
        state: state(
            "On",
            Some(true),
            if default {
                "The launch default: a launch starts it unless its caller names the other."
            } else {
                "A launch starts it when its caller names it."
            },
        ),
        needs: vec![program(machine, agent, "a launch of it here would fail")],
        ..entry(
            agent,
            name,
            &format!("Agents the board launches as {name}."),
        )
    }
}

/// Herdr's panes: `[agents] panes`.
fn herdr(machine: &Machine, file: &FileRead, project: &Project) -> SettingsEntry {
    let on = project.agents.panes == PANES[0];
    let mut panes = file.setting(
        &["agents"],
        "panes",
        "What agents' terminals run in",
        SettingKind::Choice,
        &project.agents.panes,
    );
    panes.choices = PANES.iter().map(|pane| (*pane).to_string()).collect();
    SettingsEntry {
        state: state(
            if on { "On" } else { "Off" },
            Some(on),
            if on {
                "Agents' terminals open in Herdr, and the pages can show them."
            } else {
                "Agents' terminals open in no panes the pages can show."
            },
        ),
        needs: vec![program(
            machine,
            &machine.hc,
            "no terminal opens in Herdr here",
        )],
        settings: vec![panes],
        ..entry(
            "herdr",
            "Herdr panes",
            "Agents' terminals in Herdr, which the pages can show and go to.",
        )
    }
}

/// `file` of the tree the branch `branch` commits in the repository of the checkout `main`: none
/// when that tree holds none, or there is no such repository or commit.
fn committed(main: &Path, branch: &str, file: &str) -> Result<Option<String>, String> {
    // No repository, or no commit of the branch yet, holds no file.
    let Ok(repo) = gix::open(main) else {
        return Ok(None);
    };
    let Ok(id) = repo.rev_parse_single(branch) else {
        return Ok(None);
    };
    let commit = id
        .object()
        .map_err(|e| e.to_string())?
        .peel_to_commit()
        .map_err(|e| e.to_string())?;
    let tree = commit.tree().map_err(|e| e.to_string())?;
    let Some(entry) = tree.lookup_entry_by_path(file).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let blob = repo.find_blob(entry.id()).map_err(|e| e.to_string())?;
    String::from_utf8(blob.data.clone())
        .map(Some)
        .map_err(|e| e.to_string())
}

/// The default branch as the GitHub program names it: `[project] default_branch` of the
/// `symphony.toml` the main checkout's HEAD commits, `main` where none is committed or it names
/// none. An edit of the working file names no branch until it is committed.
fn committed_branch(main: &Path) -> Result<String, String> {
    let text = committed(main, "HEAD", "symphony.toml")?.unwrap_or_default();
    let table = toml::from_str::<toml::Table>(&text)
        .map_err(|e| format!("symphony.toml at HEAD: {}", e.message()))?;
    Ok(table
        .get("project")
        .and_then(|project| project.get("default_branch"))
        .and_then(Value::as_str)
        .unwrap_or("main")
        .to_string())
}

/// What the page can say of the GitHub exchange without running its program.
enum Exchange {
    /// Nothing turns it on: no committed file, or one that leaves `enabled` out or false.
    Off(String),
    /// The committed file, or the branch's name, does not read: the program runs as off.
    Unread(String),
    /// The committed file writes `enabled = true`, with what else it writes. Whether the program
    /// accepts the file is its own to say: one it refuses runs as off.
    Written(String),
}

/// The GitHub exchange, by what the default branch's committed settings write. The page runs no
/// program and holds no copy of its rules, so it says what the file writes and claims the
/// exchange on for none: a file its program refuses runs as off, which that program's own status
/// tells. Its settings decide what lands and what is written to an outside service, so they are
/// reviewed.
fn github(machine: &Machine, main: &Path) -> SettingsEntry {
    let branch = committed_branch(main);
    let on_branch = match &branch {
        Ok(branch) => format!("{GITHUB_FILE} on {branch}"),
        Err(_) => GITHUB_FILE.to_string(),
    };
    let text = match &branch {
        Ok(branch) => committed(main, &format!("refs/heads/{branch}"), GITHUB_FILE),
        Err(problem) => Err(problem.clone()),
    };
    let table = match &text {
        Ok(Some(text)) => toml::from_str::<toml::Table>(text)
            .map(Some)
            .map_err(|e| e.message().to_string()),
        Ok(None) => Ok(None),
        Err(problem) => Err(problem.clone()),
    };
    let enabled = match &table {
        Ok(Some(table)) => table.get("enabled").and_then(Value::as_bool) == Some(true),
        _ => false,
    };
    let exchange = match &table {
        Err(problem) => Exchange::Unread(format!("Off: {on_branch} does not read ({problem}).")),
        Ok(None) => Exchange::Off(format!("Off: no {on_branch}.")),
        Ok(Some(table)) => {
            let word = |key: &str| table.get(key).map_or_else(|| "off".to_string(), shown);
            let writes = format!(
                "repository {}, issues {}, pull requests {}, releases {}",
                word("repository"),
                word("issues"),
                word("pull_requests"),
                word("releases")
            );
            if enabled {
                Exchange::Written(format!(
                    "{on_branch} writes enabled = true; {writes}. Whether the exchange runs is \
                     its program's to say: a file it refuses, or a token it lacks, leaves it \
                     off (docs/GITHUB_SETUP.md)."
                ))
            } else {
                Exchange::Off(format!("Off: {on_branch} does not enable it; {writes}."))
            }
        }
    };
    let written = match (&text, &table) {
        (Ok(Some(text)), Ok(Some(table))) if table.contains_key("enabled") => Some(Written {
            at: "enabled".into(),
            line: limits::line(text, &[], "enabled"),
        }),
        _ => None,
    };
    let found = builds::of_main(main, GITHUB_PROGRAM, &machine.beyond);
    SettingsEntry {
        state: match exchange {
            Exchange::Off(why) | Exchange::Unread(why) => state("Off", Some(false), why),
            Exchange::Written(why) => state("Enabled in its file", None, why),
        },
        needs: vec![EntryNeed {
            what: format!("the {GITHUB_PROGRAM} program"),
            met: found.is_some(),
            said: match found {
                Some(path) => format!("Found on this machine: {}.", path.display()),
                None => "Not found on this machine: nothing is exchanged from here.".into(),
            },
        }],
        settings: vec![Setting {
            key: "enabled".into(),
            name: "What its committed file writes for the exchange".into(),
            kind: SettingKind::Switch,
            value: if enabled { "on" } else { "off" }.into(),
            choices: Vec::new(),
            source: match written {
                Some(written) => SettingSource {
                    scope: Some(SettingScope::Project),
                    said: format!("{on_branch}, {}", written.said()),
                    file: Some(GITHUB_FILE.into()),
                    line: written.line.and_then(|line| u32::try_from(line).ok()),
                    over: None,
                },
                None => default(&format!("no {on_branch}")),
            },
            scopes: vec![SettingScope::Project],
            class: SettingClass::Reviewed,
        }],
        ..entry(
            "github",
            "GitHub",
            "Issues exchanged, lands delivered by pull request, releases published.",
        )
    }
}

/// The project's plugins: `[plugins]`.
fn plugins(file: &FileRead, project: &Project) -> SettingsEntry {
    let count = project.plugins.len();
    let processes = project.plugin_processes.len();
    SettingsEntry {
        state: state(
            &match count {
                0 => "None selected".to_string(),
                n => format!("{n} selected"),
            },
            None,
            match count {
                0 => "No plugin answers for this project.".to_string(),
                _ => format!(
                    "{}; {processes} from a process of its own, the rest linked into the host.",
                    project.plugins.join(", ")
                ),
            },
        ),
        settings: vec![
            file.setting(
                &["plugins"],
                "crates",
                "The plugins selected",
                SettingKind::List,
                "none",
            ),
            file.setting(
                &["plugins"],
                "process",
                "The plugins answering from a process of their own",
                SettingKind::List,
                "none",
            ),
            file.setting(
                &["plugins"],
                "timing",
                "Record each plugin call's time",
                SettingKind::Switch,
                "off",
            ),
            file.setting(
                &["plugins"],
                "loopback",
                "Send each call through a JSON round trip",
                SettingKind::Switch,
                "off",
            ),
        ],
        ..entry(
            "plugins",
            "Plugins",
            "The project's own components, routes and themes.",
        )
    }
}

/// A setting the board reads with the local file over the committed one: the local file's value
/// and line where it writes one that is not empty, over the committed one's when that is written
/// too; else the committed one's; else `unset`, said by `chosen`.
fn overridden(
    board: &FileRead,
    held: Option<&Local>,
    (at, path, key): (&str, &[&str], &str),
    name: &str,
    kind: SettingKind,
    value: Option<String>,
    unset: (&str, &str),
) -> Setting {
    let committed = board.written(at, path, key).map(|written| source(&written));
    let local = held.filter(|local| {
        let wrote = match path {
            ["agents", "launch"] => local::LAUNCH
                .iter()
                .position(|known| *known == key)
                .and_then(|at| local.launch[at].as_deref()),
            _ => local.model_of(key),
        };
        wrote.is_some_and(|value| !value.trim().is_empty())
    });
    let source = match (local, committed) {
        (Some(local), committed) => SettingSource {
            over: committed.map(|committed| committed.said),
            ..source(&local.written(at, path, key))
        },
        (None, Some(committed)) if value.is_some() => committed,
        _ => default(unset.1),
    };
    Setting {
        key: at.into(),
        name: name.into(),
        kind,
        value: value.unwrap_or_else(|| unset.0.into()),
        choices: Vec::new(),
        source,
        scopes: scopes(path, key),
        class: class(path.first().copied().unwrap_or_default(), key),
    }
}

const LAUNCH: [&str; 2] = ["agents", "launch"];
const MODELS: [&str; 2] = ["agents", "models"];

/// What a launch starts when its caller names none: `[agents] launch`.
fn launch(board: &FileRead, defaults: &Defaults, held: Option<&Local>) -> SettingsEntry {
    let resolved = defaults.resolve(None, None);
    let mut agent = overridden(
        board,
        held,
        ("[agents] launch default", &LAUNCH, "default"),
        "The agent a launch starts",
        SettingKind::Choice,
        defaults.agent.map(str::to_string),
        (AGENTS[0], "the board's own"),
    );
    agent.choices = AGENTS.iter().map(|agent| (*agent).to_string()).collect();
    SettingsEntry {
        settings: vec![
            agent,
            overridden(
                board,
                held,
                ("[agents] launch model", &LAUNCH, "model"),
                "Its model",
                SettingKind::Text,
                defaults.model.clone(),
                (
                    &resolved.model,
                    &format!("the board's own for {}", resolved.agent),
                ),
            ),
            overridden(
                board,
                held,
                ("[agents] launch effort", &LAUNCH, "effort"),
                "Its effort",
                SettingKind::Text,
                defaults.effort.clone(),
                (
                    resolved.effort.as_deref().unwrap_or("the agent's own"),
                    &format!("the board's own for {}", resolved.agent),
                ),
            ),
        ],
        ..entry(
            "launch",
            "What a launch starts",
            &format!(
                "A launch whose caller names nothing starts {}.",
                resolved.said()
            ),
        )
    }
}

/// The model each role runs on: `[agents.models]`, else where the board takes it from.
fn models(board: &FileRead, defaults: &Defaults, held: Option<&Local>) -> SettingsEntry {
    let settings = ROLES
        .iter()
        .map(|role| {
            let (agent, model, chosen) = defaults.model_of(role);
            let at = format!("[agents.models] {role}");
            let name = format!("The {role}'s model");
            let written = |value: Option<String>, unset: (&str, &str)| {
                overridden(
                    board,
                    held,
                    (&at, &MODELS, role),
                    &name,
                    SettingKind::Text,
                    value,
                    unset,
                )
            };
            let model = model.unwrap_or_default();
            match chosen {
                Chosen::Role => written(Some(model), ("", "")),
                Chosen::Launch => written(None, (&model, "[agents] launch model")),
                Chosen::Board => written(None, (&model, &format!("the board's own for {agent}"))),
                Chosen::Smallest => {
                    written(None, (&model, &format!("the smallest {agent} offers")))
                }
                Chosen::Starter => written(None, ("not set", "whoever starts one chooses")),
            }
        })
        .collect();
    SettingsEntry {
        settings,
        ..entry(
            "models",
            "The model each role runs on",
            "What a board launch passes for the role, and an agent starting a subagent.",
        )
    }
}

/// The work-in-progress limits: `[limits]`, as the board reads them.
fn limits_entry(board: &FileRead, read: &limits::Read) -> SettingsEntry {
    let held = read.limits;
    let none = |value: Option<usize>, unset: &str| {
        value.map_or_else(|| unset.to_string(), |n| n.to_string())
    };
    let rows = [
        (
            limits::SLOTS,
            "Tasks in progress an orchestrator holds",
            none(held.orchestrator_slots, "no cap"),
            "no cap",
        ),
        (
            limits::TASKS,
            "Open tasks an orchestrator holds the role on",
            held.orchestrator_tasks.to_string(),
            "",
        ),
        (
            limits::LOW,
            "Open slots below which an orchestrator hears low volume",
            held.low_volume.to_string(),
            "",
        ),
        (
            limits::MACHINE_AGENTS,
            "Agents this machine runs",
            none(held.machine_agents, "no limit"),
            "no limit",
        ),
    ];
    let settings = rows
        .into_iter()
        .map(|(key, name, value, unset)| {
            let at = format!("[{}] {key}", limits::TABLE);
            let mut from = read.written.get(key).map_or_else(|| default(unset), source);
            // A local file's value holds over the committed one, where that is written too.
            if from.scope == Some(SettingScope::Machine) {
                from.over = limits::parse(FILE, &board.text)
                    .written
                    .get(key)
                    .map(|written| source(written).said);
            }
            Setting {
                key: at,
                name: name.into(),
                kind: SettingKind::Number,
                value,
                choices: Vec::new(),
                source: from,
                scopes: scopes(&[limits::TABLE], key),
                class: class(limits::TABLE, key),
            }
        })
        .collect();
    SettingsEntry {
        settings,
        ..entry(
            "limits",
            "Work-in-progress limits",
            "What the board holds its starts and its launches to.",
        )
    }
}

/// The pipeline alarm's thresholds: `[board]`'s three, as the board reads them.
fn alarm_entry(read: &limits::Read) -> SettingsEntry {
    let alarm = read.limits.alarm;
    let names = [
        "Review over this many times the work in progress raises it",
        "Hours review stays over the ratio before it raises",
        "Hours review stays at or under it before it clears",
    ];
    let settings = alarm::KEYS
        .iter()
        .zip([alarm.ratio, alarm.raise_hours, alarm.clear_hours])
        .zip(names)
        .map(|((key, value), name)| Setting {
            key: format!("[board] {key}"),
            name: name.into(),
            kind: SettingKind::Number,
            value: value.to_string(),
            choices: Vec::new(),
            source: read.written.get(key).map_or_else(|| default(""), source),
            scopes: scopes(&["board"], key),
            class: class("board", key),
        })
        .collect();
    SettingsEntry {
        settings,
        ..entry(
            "alarm",
            "The pipeline alarm",
            "When review piling up over the work in progress is said to every orchestrator.",
        )
    }
}

/// The project's name, id and folders: `[project]`, `[agents] worktrees` and `[board]`'s own.
fn identity(file: &FileRead, project: &Project) -> SettingsEntry {
    let text = SettingKind::Text;
    SettingsEntry {
        settings: vec![
            file.setting(
                &["project"],
                "name",
                "The name the pages show",
                text,
                &project.name,
            ),
            file.setting(
                &["project"],
                "id",
                "Its id, the address it is served under",
                text,
                &project.id,
            ),
            file.setting(
                &["project"],
                "default_branch",
                "The branch tasks land on",
                text,
                &project.default_branch,
            ),
            file.setting(
                &["project"],
                "register",
                "The decision register",
                text,
                &project.register,
            ),
            file.setting(
                &["project"],
                "artifacts",
                "The folder of served and private files",
                text,
                &project.artifacts,
            ),
            file.setting(
                &["project"],
                "cargo",
                "The command that runs Cargo for a rebuild",
                SettingKind::List,
                "the Symphony's own",
            ),
            file.setting(
                &["agents"],
                "worktrees",
                "The folder of a land's worktrees",
                text,
                "in the git common dir",
            ),
            file.setting(
                &["board"],
                "guide",
                "The board's guide",
                text,
                project.board.guide(),
            ),
            file.setting(
                &["board"],
                "swept_cap_gb",
                "Gigabytes of swept evidence kept before a check is asked",
                SettingKind::Number,
                "5",
            ),
            file.setting(
                &["board"],
                "stale_writer_hours",
                "Hours an unused build holds the board's move",
                SettingKind::Number,
                "24",
            ),
            file.setting(
                &["board"],
                "slot_locks",
                "The folder of the project's test-slot locks",
                text,
                "none",
            ),
            file.setting(
                &["board"],
                "machine_wip",
                "Machines given build and test advice",
                SettingKind::List,
                "none",
            ),
        ],
        ..entry(
            "identity",
            "Name and folders",
            "What the project is called and where its files are kept.",
        )
    }
}

/// What the project serves: `[[entrypoints]]`, said as a sentence.
fn documents(file: &FileRead, project: &Project) -> SettingsEntry {
    let served = project
        .entrypoints
        .iter()
        .map(|entry| format!("{}/ at /{}", entry.folder.trim_end_matches('/'), entry.url))
        .collect::<Vec<_>>()
        .join(", ");
    SettingsEntry {
        settings: vec![file.sentence(
            "entrypoints",
            "The folders served",
            format!("Serves {served}."),
        )],
        ..entry(
            "documents",
            "Documents",
            "The folders the project's documents are served and read from.",
        )
    }
}

/// The project's look: `[theme]`.
fn look(file: &FileRead) -> SettingsEntry {
    let text = SettingKind::Text;
    SettingsEntry {
        settings: vec![
            file.setting(
                &["theme"],
                "package",
                "The theme package",
                text,
                "the Symphony's own",
            ),
            file.setting(
                &["theme"],
                "name",
                "The name in the tab",
                text,
                "the package's, else the project's",
            ),
            file.setting(
                &["theme"],
                "icon",
                "The icon of the header and the tab",
                text,
                "the package's, else the Symphony's mark",
            ),
            file.setting(
                &["theme"],
                "stylesheet",
                "A stylesheet served after the package's",
                text,
                "none",
            ),
            file.setting(
                &["theme"],
                "tokens",
                "The tokens set over the package's",
                SettingKind::List,
                "none",
            ),
            file.setting(
                &["theme"],
                "layouts",
                "The regions given a variant",
                SettingKind::List,
                "none",
            ),
        ],
        ..entry(
            "theme",
            "Theme",
            "How the project's pages look, on every device.",
        )
    }
}

/// What a land does and how the owner's saves are committed: `[land]` and `[verdicts]`.
fn landing(file: &FileRead, project: &Project) -> SettingsEntry {
    let land = &project.land;
    let named = |generators: &[crate::project::Generator]| match generators {
        [] => "no generator".to_string(),
        [one] => format!("one generator, {}", one.name),
        many => format!(
            "{} generators, {}",
            many.len(),
            many.iter()
                .map(|g| g.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let delivery = match file.get(&["land"], "delivery") {
        Some(command) => format!("delivered by `{}`", shown(command)),
        None => "merged here".to_string(),
    };
    let verdicts = &project.verdicts;
    let check = match verdicts.check.as_slice() {
        [] => "no check".to_string(),
        command => format!("checked by `{}`", command.join(" ")),
    };
    SettingsEntry {
        settings: vec![
            file.sentence(
                "land",
                "What a land does",
                format!(
                    "{}; {} {}; {}; {}.",
                    if land.check {
                        "Runs the check"
                    } else {
                        "No check"
                    },
                    if land.push { "pushes" } else { "does not push" },
                    project.default_branch,
                    named(&land.generators),
                    delivery
                ),
            ),
            file.sentence(
                "verdicts",
                "How the owner's saves are committed",
                format!(
                    "A generated branch is {check}; {}.",
                    named(&verdicts.generators)
                ),
            ),
        ],
        ..entry(
            "landing",
            "Landing",
            "What a land runs and what it requires before it merges.",
        )
    }
}

/// The commands and programs the board runs, and where its store is: `[board]`'s reviewed keys.
fn commands(file: &FileRead) -> SettingsEntry {
    let rows = [
        ("on_review", "Started when a task moves to review"),
        ("on_land", "Started once a land closed a task"),
        ("machine_work_reader", "The project's machine-work reader"),
        (
            "postgresql",
            "The folder of the machine's PostgreSQL programs",
        ),
        ("external_postgresql", "A hosted board's PostgreSQL"),
        ("assessments", "The folder of landing assessments"),
    ];
    SettingsEntry {
        settings: rows
            .into_iter()
            .map(|(key, name)| file.setting(&["board"], key, name, SettingKind::List, "none"))
            .collect(),
        ..entry(
            "commands",
            "The board's commands",
            "What the board starts on this machine, and where its store is.",
        )
    }
}

/// The stamps a land can require: `[stamps]`, as the board reads its catalogue.
fn stamps(board: &FileRead) -> SettingsEntry {
    use symphony_board::stamps::Catalogue;
    let said = match Catalogue::parse(&board.text) {
        Ok(catalogue) => {
            let names = catalogue.names().join(", ");
            if catalogue.written {
                format!("The project's: {names}.")
            } else {
                format!("The Symphony's four: {names}.")
            }
        }
        Err(problem) => format!("Cannot be read: {problem}."),
    };
    SettingsEntry {
        settings: vec![board.sentence("stamps", "Stamps a land can require", said)],
        ..entry(
            "stamps",
            "Stamps",
            "What a task's land may need standing on its tip.",
        )
    }
}

/// The release run: `[releases]`.
fn releases(file: &FileRead) -> SettingsEntry {
    let number = SettingKind::Number;
    SettingsEntry {
        settings: vec![
            file.setting(
                &["releases"],
                "every_landings",
                "Landings before a release run starts",
                number,
                &Releases::EVERY_LANDINGS.to_string(),
            ),
            file.setting(
                &["releases"],
                "refresh",
                "A green release refreshes the machine",
                SettingKind::Switch,
                "on",
            ),
            file.setting(
                &["releases"],
                "publish",
                "A green release publishes on crates.io",
                SettingKind::Switch,
                "off",
            ),
            file.setting(
                &["releases"],
                "github",
                "The command asked for its GitHub Release",
                SettingKind::List,
                "none",
            ),
            file.setting(
                &["releases"],
                "rulesets",
                "The command a run runs as its rulesets check",
                SettingKind::List,
                "none",
            ),
            file.setting(
                &["releases"],
                "hosted",
                "The command that verifies a run on GitHub's machines",
                SettingKind::List,
                "none",
            ),
            file.setting(
                &["releases"],
                "hosted_every_minutes",
                "Least minutes between the starts of two hosted runs",
                number,
                &Releases::HOSTED_EVERY_MINUTES.to_string(),
            ),
            file.setting(
                &["releases"],
                "candidate_reruns",
                "Red reruns a release candidate takes",
                number,
                &Releases::CANDIDATE_RERUNS.to_string(),
            ),
            file.setting(
                &["releases"],
                "candidate_hours",
                "Hours a release candidate is held",
                number,
                &Releases::CANDIDATE_HOURS.to_string(),
            ),
        ],
        ..entry(
            "releases",
            "Releases",
            "When a release run starts and what a green one does.",
        )
    }
}

/// The delivery workflow, said as a sentence with where it is written: an entry with no setting,
/// so the page claims no class for it and offers no control.
fn workflow(file: &FileRead) -> SettingsEntry {
    let written = file.written("[workflow]", &[], "workflow");
    let mappings = file.get(&[], "workflow_mappings").is_some();
    let authorities = file.get(&[], "workflow_authorities").is_some();
    let also = match (mappings, authorities) {
        (true, true) => ", with mappings and approval authorities",
        (true, false) => ", with mappings",
        (false, true) => ", with approval authorities",
        (false, false) => "",
    };
    SettingsEntry {
        state: state(
            if written.is_some() {
                "Custom"
            } else {
                "Built in"
            },
            None,
            match written {
                Some(_) => format!(
                    "The project's own workflow{also}, which the board activates once the default branch commits it."
                ),
                None => "The board's own workflow: no [workflow] written.".to_string(),
            },
        ),
        source: Some(written.map_or_else(
            || default("no [workflow] written"),
            |written| source(&written),
        )),
        ..entry(
            "workflow",
            "Workflow",
            "The stages a task moves through and what each move requires.",
        )
    }
}

/// The device's appearance, which the browser keeps: no file writes it.
fn appearance() -> SettingsEntry {
    SettingsEntry {
        settings: vec![Setting {
            key: "appearance".into(),
            name: "The theme of this project's pages here".into(),
            kind: SettingKind::Text,
            value: "kept in this browser".into(),
            choices: Vec::new(),
            source: SettingSource {
                scope: Some(SettingScope::Device),
                said: "this browser, set from the header's Appearance".into(),
                file: None,
                line: None,
                over: None,
            },
            scopes: vec![SettingScope::Device],
            class: SettingClass::Preference,
        }],
        ..entry(
            "appearance",
            "Appearance",
            "A theme of this device's own over the project's.",
        )
    }
}
