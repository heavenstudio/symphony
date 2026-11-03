//! A project: a root folder holding a committed `symphony.toml`. The file names what the Symphony
//! would otherwise assume, and every value it leaves out is the Symphony's generic default, never a
//! game's: no plugins, no generators, no check, the folder's name as the project's name and its
//! slug as the id, `docs/` with `docs/history` skipped, `docs/DECISIONS.md`, `artifacts/`, `main`,
//! the Symphony's own theme, every built-in transport and no panes. An empty file, or none, is a
//! plain project. An unknown table or key is refused by name, so a misspelt setting never passes
//! for its default.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::SystemTime;

use serde::Deserialize;

/// The project file at a project's root.
pub const FILE: &str = "symphony.toml";

/// The agent transports the Symphony ships: Claude's inbox socket, Codex's queue, and the inbox a
/// session without a channel reads.
pub const TRANSPORTS: [&str; 3] = ["claude-uds", "codex-queue", "inbox"];
/// The multiplexers agents' panes may run in; `none` for a project without one.
pub const PANES: [&str; 2] = ["herdr", "none"];
/// The prefix naming a path in the repository's common git folder.
const COMMON_DIR: &str = ".git/";

/// A project's settings: its `symphony.toml` over the Symphony's defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    /// The URL segment and the data folder.
    pub id: String,
    /// The documents, project-relative.
    pub docs: String,
    /// Folders under the documents that are never read as documents.
    pub history: Vec<String>,
    /// The decision register.
    pub register: String,
    /// Served files and the Symphony's private files.
    pub artifacts: String,
    pub default_branch: String,
    /// The plugins the project uses, by crate name.
    pub plugins: Vec<String>,
    pub theme: Theme,
    pub agents: Agents,
    pub verdicts: Verdicts,
    pub board: Board,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Theme {
    /// A theme package a plugin ships; `None` is the Symphony's own.
    pub package: Option<String>,
    pub icon: Option<String>,
    pub stylesheet: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agents {
    pub transports: Vec<String>,
    pub panes: String,
    /// Where agents' worktrees go, relative to the root (it may lie outside it).
    pub worktrees: Option<String>,
    pub launch: Launch,
}

/// The defaults a launch takes when it names none; `None` leaves the board's own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
    pub default: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// How the owner's edits are committed to the default branch: straight on it when no generator
/// changes an output, else on a branch with the outputs, checked and then landed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Verdicts {
    /// The command that must pass on a generated branch's tree before it lands; empty runs none.
    pub check: Vec<String>,
    /// The command that lands a generated branch, the branch's name appended. A project with
    /// generators names one.
    pub land: Vec<String>,
    pub generators: Vec<Generator>,
    /// Where a verdict commit's record goes, read by the project's landing hook; `None` writes none.
    pub record: Option<String>,
}

pub use crate::landing::Generator;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Board {
    /// Where the board's export is written, project-relative; `None` writes none.
    pub export: Option<String>,
    /// What a finished worktree may hold and still be removed.
    pub disposable: Vec<String>,
}

/// The file as written: every table and key optional, nothing else allowed.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct File {
    project: FileProject,
    plugins: FilePlugins,
    theme: FileTheme,
    agents: FileAgents,
    verdicts: FileVerdicts,
    board: FileBoard,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FileProject {
    name: Option<String>,
    id: Option<String>,
    docs: Option<String>,
    history: Option<Vec<String>>,
    register: Option<String>,
    artifacts: Option<String>,
    default_branch: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FilePlugins {
    crates: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FileTheme {
    package: Option<String>,
    icon: Option<String>,
    stylesheet: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FileAgents {
    transports: Option<Vec<String>>,
    panes: Option<String>,
    worktrees: Option<String>,
    launch: FileLaunch,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FileLaunch {
    default: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FileVerdicts {
    check: Vec<String>,
    land: Vec<String>,
    generators: Vec<Generator>,
    record: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FileBoard {
    export: Option<String>,
    disposable: Vec<String>,
}

impl Project {
    /// The plain project at `root`: every default, its name and id from the folder's name.
    pub fn plain(root: &Path) -> Project {
        Self::resolve(root, File::default()).expect("the defaults are a valid project")
    }

    /// The project at `root`: its `symphony.toml` over the defaults, or the plain project without
    /// one. A file that does not parse, or names a key or value the Symphony does not know, is an
    /// error naming the file and what is wrong.
    pub fn load(root: &Path) -> Result<Project, String> {
        let path = root.join(FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(root, &text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::plain(root)),
            Err(e) => Err(crate::text::os_error(&e, &path)),
        }
    }

    /// `text` as the `symphony.toml` of the project at `root`.
    pub fn parse(root: &Path, text: &str) -> Result<Project, String> {
        let file: File =
            toml::from_str(text).map_err(|e| format!("{FILE}: {}", e.to_string().trim_end()))?;
        Self::resolve(root, file).map_err(|e| format!("{FILE}: {e}"))
    }

    /// The project at `root` as [`Project::load`] reads it, cached until its file changes. A file
    /// that does not load leaves the plain project in its place and is reported once per version
    /// on stderr; the server and the lint refuse it through [`Project::load`].
    pub fn of(root: &Path) -> Arc<Project> {
        type Seen = BTreeMap<PathBuf, (Stamp, Arc<Project>)>;
        static SEEN: LazyLock<Mutex<Seen>> = LazyLock::new(|| Mutex::new(BTreeMap::new()));
        let stamp = Stamp::of(&root.join(FILE));
        if let Some((held, project)) = SEEN
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(root)
            && *held == stamp
        {
            return project.clone();
        }
        let project = Arc::new(Self::load(root).unwrap_or_else(|problem| {
            eprintln!("{}: {problem}; using the defaults", root.display());
            Self::plain(root)
        }));
        SEEN.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(root.to_path_buf(), (stamp, project.clone()));
        project
    }

    /// The project as the client's header and home read it: its name, its register and its
    /// plugins' navigation.
    pub fn view(&self) -> symphony_api::ProjectView {
        symphony_api::ProjectView {
            name: self.name.clone(),
            register: self.register.clone(),
            nav: crate::plugins::Plugins::for_project(self).nav(),
        }
    }

    /// The element carrying `view` in the page the server serves: JSON in a script element no
    /// browser runs, every `<` written `<` so no value can close it or open a comment in it.
    pub fn element(view: &symphony_api::ProjectView) -> String {
        let json = serde_json::to_string(view)
            .expect("a project view serializes")
            .replace('<', "\\u003c");
        format!(
            "<script type=\"application/json\" id=\"{}\">{json}</script>",
            symphony_api::PROJECT_ELEMENT
        )
    }

    /// The nearest folder at or above `start` holding a `symphony.toml`.
    pub fn find(start: &Path) -> Option<PathBuf> {
        start
            .ancestors()
            .find(|folder| folder.join(FILE).is_file())
            .map(Path::to_path_buf)
    }

    fn resolve(root: &Path, file: File) -> Result<Project, String> {
        let folder = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let p = file.project;
        let name = p.name.unwrap_or_else(|| folder.clone());
        let id = p.id.unwrap_or_else(|| slug(&name));
        if id.is_empty() || slug(&id) != id {
            return Err(format!(
                "[project] id {id:?} is not a slug (lowercase letters, digits and dashes)"
            ));
        }
        let docs = inside("[project] docs", p.docs.unwrap_or_else(|| "docs".into()))?;
        let history = p
            .history
            .unwrap_or_else(|| vec![format!("{docs}/history")])
            .into_iter()
            .map(|path| inside("[project] history", path))
            .collect::<Result<_, _>>()?;
        let register = inside(
            "[project] register",
            p.register.unwrap_or_else(|| format!("{docs}/DECISIONS.md")),
        )?;
        let artifacts = inside(
            "[project] artifacts",
            p.artifacts.unwrap_or_else(|| "artifacts".into()),
        )?;
        let default_branch = p.default_branch.unwrap_or_else(|| "main".into());
        if default_branch.trim().is_empty() {
            return Err("[project] default_branch is empty".into());
        }
        let theme = Theme {
            package: file.theme.package,
            icon: file
                .theme
                .icon
                .map(|path| inside("[theme] icon", path))
                .transpose()?,
            stylesheet: file
                .theme
                .stylesheet
                .map(|path| inside("[theme] stylesheet", path))
                .transpose()?,
        };
        let a = file.agents;
        let transports = a
            .transports
            .unwrap_or_else(|| TRANSPORTS.map(String::from).to_vec());
        let panes = a.panes.unwrap_or_else(|| "none".into());
        if !PANES.contains(&panes.as_str()) {
            return Err(format!(
                "[agents] panes {panes:?} is not one of {}",
                PANES.join(", ")
            ));
        }
        let agents = Agents {
            transports,
            panes,
            worktrees: a.worktrees,
            launch: Launch {
                default: a.launch.default,
                model: a.launch.model,
                effort: a.launch.effort,
            },
        };
        let v = file.verdicts;
        for generator in &v.generators {
            inside("[verdicts] generators outputs", generator.outputs.clone())?;
            if generator.command.is_empty() {
                return Err(format!(
                    "[verdicts] generator {} has no command",
                    generator.name
                ));
            }
        }
        if !v.generators.is_empty() && v.land.is_empty() {
            return Err(
                "[verdicts] generators need a land command: a generated branch never moves the \
                 default branch unchecked"
                    .into(),
            );
        }
        let verdicts = Verdicts {
            check: v.check,
            land: v.land,
            generators: v.generators,
            record: v
                .record
                .map(|path| inside("[verdicts] record", path))
                .transpose()?,
        };
        let board = Board {
            export: file
                .board
                .export
                .map(|path| inside("[board] export", path))
                .transpose()?,
            disposable: file.board.disposable,
        };
        Ok(Project {
            name,
            id,
            docs,
            history,
            register,
            artifacts,
            default_branch,
            plugins: file.plugins.crates,
            theme,
            agents,
            verdicts,
            board,
        })
    }

    /// Whether the project-relative `rel` is a document: under the documents, and not under one of
    /// the history folders.
    pub fn is_document_path(&self, rel: &str) -> bool {
        under(rel, &self.docs) && !self.history.iter().any(|folder| under(rel, folder))
    }

    /// The history folders, relative to the documents folder (`history`), for walks that start
    /// there.
    pub fn history_in_docs(&self) -> Vec<String> {
        self.history
            .iter()
            .filter_map(|folder| folder.strip_prefix(&format!("{}/", self.docs)))
            .map(str::to_string)
            .collect()
    }

    /// `rel` under the project's artifacts folder.
    pub fn artifact(&self, rel: &str) -> String {
        format!("{}/{rel}", self.artifacts)
    }

    /// Where the verdict record of the checkout at `root` goes: a `.git/` path in its common git
    /// folder, any other under the root.
    pub fn record_folder(&self, root: &Path, common_dir: &Path) -> Option<PathBuf> {
        let record = self.verdicts.record.as_deref()?;
        Some(match record.strip_prefix(COMMON_DIR) {
            Some(rest) => common_dir.join(rest),
            None => root.join(record),
        })
    }
}

/// Whether `rel` is `folder` or lies under it.
fn under(rel: &str, folder: &str) -> bool {
    rel == folder
        || rel
            .strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// `path` when it is relative and stays inside the project; else an error naming `key`.
fn inside(key: &str, path: String) -> Result<String, String> {
    let trimmed = path.trim_end_matches('/').to_string();
    let escapes = Path::new(&trimmed)
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir));
    if trimmed.is_empty() || escapes {
        return Err(format!("{key} {path:?} must be a path inside the project"));
    }
    Ok(trimmed)
}

/// A name as an id: lowercase letters and digits, each other run a single dash.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// A file's length and modification time, or nothing when it is absent.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp(Option<(u64, SystemTime)>);

impl Stamp {
    fn of(path: &Path) -> Self {
        Self(
            std::fs::metadata(path)
                .ok()
                .map(|m| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH))),
        )
    }
}
