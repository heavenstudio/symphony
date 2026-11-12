//! `symphony.toml` as written: every table and key optional, and a table or key this build does
//! not know kept by name ([`File::unknown`]) instead of refused where it is read, so
//! [`Project::load`] refuses it by name and [`Project::of`] leaves it out, reads the rest and names
//! it. An `[[entrypoints]]` table stays strict, as a generator's does: a misspelt key there would
//! change what is served, so it refuses the file where it is read.
//!
//! [`Project::load`]: super::Project::load
//! [`Project::of`]: super::Project::of

use std::collections::BTreeMap;

use serde::Deserialize;

use super::FILE;
use super::board::FileBoard;
use super::theme::FileTheme;
use crate::landing::Generator;
use toml::de::DeTable;

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
    pub(super) land: FileLand,
    pub(super) board: FileBoard,
    /// The tables and keys at the top of the file this build does not know.
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

impl File {
    /// Every table and key of the file this build does not know, each named where it stands and
    /// on its line (`[projct] (line 3)`, `[board] on_review (line 41)`): the top of the file first,
    /// then table by table in the file's shape, sorted within each. A key [`line`] does not reach is
    /// named without its line.
    pub(super) fn unknown(&self, text: &str) -> Vec<String> {
        let parsed = DeTable::parse(text).ok();
        let root = parsed.as_ref().map(|spanned| spanned.get_ref());
        let named = |path: &[&str], key: &str, stands: String| match root
            .and_then(|root| line(text, root, path, key))
        {
            Some(line) => format!("{stands} (line {line})"),
            None => stands,
        };
        let mut names: Vec<String> = self
            .unknown
            .iter()
            .map(|(key, value)| match value {
                toml::Value::Table(_) => named(&[], key, format!("[{key}]")),
                _ => named(&[], key, key.clone()),
            })
            .collect();
        let tables: [(&str, &[&str], &Unknown); 8] = [
            ("[project]", &["project"], &self.project.unknown),
            ("[plugins]", &["plugins"], &self.plugins.unknown),
            ("[theme]", &["theme"], &self.theme.unknown),
            ("[agents]", &["agents"], &self.agents.unknown),
            (
                "[agents.launch]",
                &["agents", "launch"],
                &self.agents.launch.unknown,
            ),
            ("[verdicts]", &["verdicts"], &self.verdicts.unknown),
            ("[land]", &["land"], &self.land.unknown),
            ("[board]", &["board"], &self.board.unknown),
        ];
        for (table, path, unknown) in tables {
            names.extend(
                unknown
                    .keys()
                    .map(|key| named(path, key, format!("{table} {key}"))),
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

/// The line `key` is written on in the table `path` leads to from `root`, the file as written
/// ([`DeTable::parse`]), when every table on the path is there.
fn line(text: &str, root: &DeTable<'_>, path: &[&str], key: &str) -> Option<usize> {
    let mut table = root;
    for name in path {
        let (_, value) = table.iter().find(|(k, _)| **k.get_ref() == **name)?;
        table = value.get_ref().as_table()?;
    }
    let start = table
        .iter()
        .find(|(k, _)| **k.get_ref() == *key)?
        .0
        .span()
        .start;
    Some(text[..start].matches('\n').count() + 1)
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

/// One `[[entrypoints]]` table: its URL and folder named, the rest defaulted, and nothing else
/// allowed, since a misspelt key would change what is served.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
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
}

/// The `[plugins]` table.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct FilePlugins {
    pub(super) crates: Vec<String>,
    pub(super) timing: bool,
    pub(super) loopback: bool,
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

/// The `[land]` table: what the regenerator the Symphony's default `just land` starts makes, whether
/// a land pushes and runs `[verdicts] check`, and how long each command it runs may take
/// (`symphony_board::land`, which reads it from the default branch itself).
#[derive(Deserialize)]
#[serde(default)]
pub(super) struct FileLand {
    pub(super) generators: Vec<Generator>,
    pub(super) push: bool,
    pub(super) timeout_seconds: u64,
    pub(super) check: bool,
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

impl Default for FileLand {
    fn default() -> FileLand {
        FileLand {
            generators: Vec::new(),
            push: true,
            timeout_seconds: symphony_board::land::TIMEOUT.as_secs(),
            check: true,
            unknown: Unknown::new(),
        }
    }
}
