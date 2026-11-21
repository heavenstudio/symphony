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
    pub(super) releases: FileReleases,
    pub(super) limits: FileLimits,
    pub(super) stamps: FileStamps,
    /// The delivery workflow, read by the board's reader of the file
    /// ([`symphony_board::workflow::written_from`], `super::workflow::require_supported`), which
    /// reads it as it reads its own rows: kept here as this one reading of the file left it, so the
    /// Symphony reads the file once and links one reader of the workflow and none for TOML.
    pub(super) workflow: Option<toml::Value>,
    /// Where tasks at pairs a newly committed workflow removed go when it activates, read as
    /// `workflow` is.
    pub(super) workflow_mappings: Option<toml::Value>,
    /// The approval authorities whose signed word meets the workflow's guards, each the public
    /// half of a key the project holds (docs/workflow/WORKFLOW.md, "Guards"), read as `workflow` is.
    pub(super) workflow_authorities: Option<toml::Value>,
    /// The tables and keys at the top of the file this build does not know.
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

impl File {
    /// Every table and key of the file this build does not know, each named where it stands and
    /// on its line (`[projct] (line 3)`, `[board] on_review (line 41)`): the top of the file first,
    /// then table by table in the file's shape, sorted within each. A key [`line`] does not reach
    /// is named without its line.
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
        let tables: [(&str, &[&str], &Unknown); 12] = [
            ("[project]", &["project"], &self.project.unknown),
            ("[plugins]", &["plugins"], &self.plugins.unknown),
            ("[theme]", &["theme"], &self.theme.unknown),
            ("[agents]", &["agents"], &self.agents.unknown),
            (
                "[agents.launch]",
                &["agents", "launch"],
                &self.agents.launch.unknown,
            ),
            (
                "[agents.models]",
                &["agents", "models"],
                &self.agents.models.unknown,
            ),
            ("[verdicts]", &["verdicts"], &self.verdicts.unknown),
            ("[land]", &["land"], &self.land.unknown),
            ("[board]", &["board"], &self.board.unknown),
            ("[releases]", &["releases"], &self.releases.unknown),
            ("[limits]", &["limits"], &self.limits.unknown),
            ("[stamps]", &["stamps"], &self.stamps.unknown),
        ];
        for (table, path, unknown) in tables {
            for key in unknown.keys() {
                names.push(named(path, key, format!("{table} {key}")));
            }
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
    pub(super) cargo: Vec<String>,
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
    /// `[plugins.process]`: a plugin's name and the command that starts its process.
    pub(super) process: BTreeMap<String, Vec<String>>,
    pub(super) timing: bool,
    pub(super) loopback: bool,
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

/// The `[agents]` table.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct FileAgents {
    pub(super) panes: Option<String>,
    pub(super) worktrees: Option<String>,
    pub(super) launch: FileLaunch,
    pub(super) models: FileModels,
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

/// The `[agents.models]` table: the model each role runs on, which a board launch reads itself
/// (`symphony_board::launch::Defaults`).
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct FileModels {
    pub(super) orchestrator: Option<String>,
    pub(super) executor: Option<String>,
    pub(super) reviewer: Option<String>,
    pub(super) assessor: Option<String>,
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

/// The `[limits]` table: the work-in-progress limits the board reads from the file itself
/// (`symphony_board::limits`), each known here by name so a misspelt one is refused. Their values
/// are any the file writes, so one the board cannot read leaves the rest of the project read
/// ([`super::Project::of`]); [`super::Project::parse`] refuses it with its line, as the board reads
/// it.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct FileLimits {
    pub(super) orchestrator_slots: Option<toml::Value>,
    pub(super) orchestrator_tasks: Option<toml::Value>,
    pub(super) low_volume: Option<toml::Value>,
    pub(super) machine_agents: Option<toml::Value>,
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

/// The `[stamps]` table: the stamps the project's land can require, each a table of its own
/// (`[stamps.review]`), which the board reads itself from the main checkout's committed file
/// (`symphony_board::stamps`). Each is known here by its name, the stamps the Symphony has a
/// reader for, so another is refused by name and line; what each holds is any the file writes, so
/// one the board cannot read leaves the rest of the project read ([`super::Project::of`]), and
/// [`super::Project::parse`] refuses it as the board says it.
#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct FileStamps {
    pub(super) tests: Option<toml::Value>,
    pub(super) review: Option<toml::Value>,
    pub(super) pictures: Option<toml::Value>,
    pub(super) owner: Option<toml::Value>,
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
    /// Each generator's key, by its name (`symphony_board::land::keyed`).
    pub(super) keys: BTreeMap<String, Vec<String>>,
    pub(super) push: bool,
    pub(super) timeout_seconds: u64,
    pub(super) check: bool,
    /// What holds the project's tests and checks, which a red's fix its orchestrator approves
    /// changes none of (`symphony_board::land::check_paths`); its defaults when left out.
    pub(super) check_paths: Option<Vec<String>>,
    /// The command that delivers a task by a hosted pull request, in place of the land's own
    /// merge and push (`symphony_board::land::delivery`); none, and the land merges here.
    pub(super) delivery: Vec<String>,
    /// The repository this one imports from (`symphony_board::land::Upstream`), strict as a
    /// generator is: a misspelt key would change what a delivery merges.
    pub(super) upstream: Option<symphony_board::land::Upstream>,
    /// How long the delivery watcher rests between two rounds (`symphony_board::land::dispatch`).
    pub(super) watch_seconds: u64,
    /// How long an armed pull request whose evidence stands may stay unmerged before the watcher
    /// disarms it.
    pub(super) merge_within_minutes: u64,
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

impl Default for FileLand {
    fn default() -> FileLand {
        FileLand {
            generators: Vec::new(),
            keys: BTreeMap::new(),
            push: true,
            timeout_seconds: symphony_board::land::TIMEOUT.as_secs(),
            check: true,
            check_paths: None,
            delivery: Vec::new(),
            upstream: None,
            watch_seconds: symphony_board::land::WATCH.as_secs(),
            merge_within_minutes: symphony_board::land::MERGE_WITHIN.as_secs() / 60,
            unknown: Unknown::new(),
        }
    }
}

/// The `[releases]` table: how often the Symphony's own release run cuts a release besides each day's,
/// whether a green release refreshes the machine and publishes on crates.io, the command that
/// publishes its GitHub Release, the one that verifies a commit on GitHub's machines, how often a
/// run starts while that one is named, and the command that verifies who bypasses the default
/// branch's rules
/// (docs/workflow/RELEASES.md).
#[derive(Deserialize)]
#[serde(default)]
pub(super) struct FileReleases {
    pub(super) every_landings: u32,
    pub(super) refresh: bool,
    pub(super) publish: bool,
    pub(super) github: Vec<String>,
    pub(super) hosted: Vec<String>,
    pub(super) hosted_every_minutes: u32,
    pub(super) rulesets: Vec<String>,
    pub(super) candidate_reruns: u32,
    pub(super) candidate_hours: u32,
    #[serde(flatten)]
    pub(super) unknown: Unknown,
}

impl Default for FileReleases {
    fn default() -> FileReleases {
        FileReleases {
            every_landings: super::Releases::EVERY_LANDINGS,
            refresh: true,
            publish: false,
            github: Vec::new(),
            hosted: Vec::new(),
            hosted_every_minutes: super::Releases::HOSTED_EVERY_MINUTES,
            rulesets: Vec::new(),
            candidate_reruns: super::Releases::CANDIDATE_RERUNS,
            candidate_hours: super::Releases::CANDIDATE_HOURS,
            unknown: Unknown::new(),
        }
    }
}
