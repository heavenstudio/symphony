//! `symphony.toml` as written: every table and key optional, and a table or key this build does
//! not know kept by name ([`File::unknown`]) instead of refused where it is read, so
//! [`Project::load`] refuses it by name and [`Project::of`] leaves it out, reads the rest and names
//! it.
//!
//! [`Project::load`]: super::Project::load
//! [`Project::of`]: super::Project::of

use std::collections::BTreeMap;

use serde::Deserialize;

use super::FILE;
use super::board::FileBoard;
use crate::landing::Generator;

/// The keys of one table this build does not know, by name, with what they hold.
pub(super) type Unknown = BTreeMap<String, toml::Value>;

/// The file as written: every table and key optional.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct File {
    pub(super) project: FileProject,
    pub(super) entrypoints: Option<Vec<FileEntrypoint>>,
    pub(super) plugins: FilePlugins,
    pub(super) theme: FileTheme,
    pub(super) agents: FileAgents,
    pub(super) verdicts: FileVerdicts,
    pub(super) board: FileBoard,
    /// The tables and keys at the top of the file this build does not know.
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

impl File {
    /// Every table and key of the file this build does not know, each named where it stands
    /// (`[projct]`, `[board] on_review`, `[[entrypoints]] /docs/ folders`): the top of the file
    /// first, then table by table in the file's shape, sorted within each.
    pub(super) fn unknown(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .unknown
            .iter()
            .map(|(key, value)| match value {
                toml::Value::Table(_) => format!("[{key}]"),
                _ => key.clone(),
            })
            .collect();
        let tables = [
            ("[project]", &self.project.unknown),
            ("[plugins]", &self.plugins.unknown),
            ("[theme]", &self.theme.unknown),
            ("[agents]", &self.agents.unknown),
            ("[agents.launch]", &self.agents.launch.unknown),
            ("[verdicts]", &self.verdicts.unknown),
            ("[board]", &self.board.unknown),
        ];
        for (table, unknown) in tables {
            names.extend(unknown.keys().map(|key| format!("{table} {key}")));
        }
        for entry in self.entrypoints.iter().flatten() {
            let url = &entry.url;
            names.extend(
                entry
                    .unknown
                    .keys()
                    .map(|key| format!("[[entrypoints]] {url} {key}")),
            );
        }
        names
    }

    /// What refuses a file naming `unknown`, when it names any: one message naming them all.
    pub(super) fn refusal(unknown: &[String]) -> Option<String> {
        let (named, are) = match unknown {
            [] => return None,
            [one] => (one.clone(), "is not a setting"),
            many => (many.join(", "), "are not settings"),
        };
        Some(format!("{FILE}: {named} {are} the Symphony knows"))
    }
}

/// The `[project]` table.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct FileProject {
    pub(super) name: Option<String>,
    pub(super) id: Option<String>,
    pub(super) register: Option<String>,
    pub(super) artifacts: Option<String>,
    pub(super) default_branch: Option<String>,
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

/// One `[[entrypoints]]` table: its URL and folder named, the rest defaulted.
#[derive(Deserialize)]
pub(super) struct FileEntrypoint {
    pub(super) url: String,
    pub(super) folder: String,
    #[serde(default)]
    pub(super) include: Option<Vec<String>>,
    #[serde(default)]
    pub(super) exclude: Vec<String>,
    #[serde(default)]
    pub(super) types: Option<Vec<String>>,
    #[serde(default)]
    pub(super) history: Vec<String>,
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

/// The `[plugins]` table.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct FilePlugins {
    pub(super) crates: Vec<String>,
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

/// The `[theme]` table.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct FileTheme {
    pub(super) package: Option<String>,
    pub(super) icon: Option<String>,
    pub(super) stylesheet: Option<String>,
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

/// The `[agents]` table.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct FileAgents {
    pub(super) transports: Option<Vec<String>>,
    pub(super) panes: Option<String>,
    pub(super) worktrees: Option<String>,
    pub(super) launch: FileLaunch,
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

/// The `[agents.launch]` table.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct FileLaunch {
    pub(super) default: Option<String>,
    pub(super) model: Option<String>,
    pub(super) effort: Option<String>,
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

/// The `[verdicts]` table.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct FileVerdicts {
    pub(super) check: Vec<String>,
    pub(super) land: Vec<String>,
    pub(super) generators: Vec<Generator>,
    pub(super) record: Option<String>,
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}
