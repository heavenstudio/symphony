//! A project: a root folder holding a committed `symphony.toml`. The file names what the Symphony
//! would otherwise assume, and every value it leaves out is the Symphony's generic default, never a
//! game's: no plugins, no generators, no check, the folder's name as the project's name and its
//! slug as the id (`project` when the folder's has no letter or digit), one entrypoint serving
//! `docs/` at `/docs/` with `docs/history` skipped, the register `DECISIONS.md` in the first
//! entrypoint's folder, `artifacts/`, `main`, the Symphony's own theme, every built-in transport
//! and no panes. An empty file, or none, is a plain project. An unknown table, key or renderer, a
//! path outside the project, and an entrypoint or a file of one on a route another answers are
//! refused by name, so a misspelt setting never passes for its default and no file is hidden.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::SystemTime;

use serde::Deserialize;

pub use crate::entrypoint::Entrypoint;
use crate::entrypoint::{RENDERERS, SYMPHONY_ROUTES, below, join};

/// The project file at a project's root.
pub const FILE: &str = "symphony.toml";

/// The agent transports the Symphony ships: Claude's inbox socket, Codex's queue, and the inbox a
/// session without a channel reads.
pub const TRANSPORTS: [&str; 3] = ["claude-uds", "codex-queue", "inbox"];
/// The multiplexers agents' panes may run in; `none` for a project without one.
pub const PANES: [&str; 2] = ["herdr", "none"];
/// The id of a project whose folder's name has no letter or digit to make one of.
pub const DEFAULT_ID: &str = "project";
/// The prefix naming a path in the repository's common git folder.
const COMMON_DIR: &str = ".git/";

/// A project's settings: its `symphony.toml` over the Symphony's defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    /// The URL segment and the data folder.
    pub id: String,
    /// Where the documents are served and read from, in the file's order; never empty.
    pub entrypoints: Vec<Entrypoint>,
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
    entrypoints: Option<Vec<FileEntrypoint>>,
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
    register: Option<String>,
    artifacts: Option<String>,
    default_branch: Option<String>,
}

/// One `[[entrypoints]]` table: its URL and folder named, the rest defaulted.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileEntrypoint {
    url: String,
    folder: String,
    #[serde(default)]
    include: Option<Vec<String>>,
    #[serde(default)]
    exclude: Vec<String>,
    #[serde(default)]
    types: Option<Vec<String>>,
    #[serde(default)]
    history: Vec<String>,
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
    /// one. A file that does not parse, names a key or value the Symphony does not know, or leaves
    /// an entrypoint a file on a route another answers is an error naming the file and what is
    /// wrong.
    pub fn load(root: &Path) -> Result<Project, String> {
        let project = Self::read(root)?;
        project
            .check_files(root)
            .map_err(|e| format!("{FILE}: {e}"))?;
        Ok(project)
    }

    /// The file as [`Project::load`] reads it, before its entrypoints' files are looked at.
    fn read(root: &Path) -> Result<Project, String> {
        let path = root.join(FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(root, &text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::plain(root)),
            Err(e) => Err(crate::text::os_error(&e, &path)),
        }
    }

    /// `text` as the `symphony.toml` of the project at `root`. The files its entrypoints hold are
    /// [`Project::load`]'s to look at.
    pub fn parse(root: &Path, text: &str) -> Result<Project, String> {
        let file: File =
            toml::from_str(text).map_err(|e| format!("{FILE}: {}", e.to_string().trim_end()))?;
        Self::resolve(root, file).map_err(|e| format!("{FILE}: {e}"))
    }

    /// The project at `root` as [`Project::load`] reads it, cached until its file changes, or an
    /// entry appears or goes where a file of an entrypoint would meet another route. A project that
    /// does not load leaves the plain project in its place and is reported once per version on
    /// stderr; `symphony-status --lint` refuses it ([`crate::lint::project_problems`]).
    pub fn of(root: &Path) -> Arc<Project> {
        type Seen = BTreeMap<PathBuf, (Vec<(PathBuf, Stamp)>, Arc<Project>)>;
        static SEEN: LazyLock<Mutex<Seen>> = LazyLock::new(|| Mutex::new(BTreeMap::new()));
        if let Some((stamps, project)) = SEEN
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(root)
            && stamps.iter().all(|(path, held)| *held == Stamp::of(path))
        {
            return project.clone();
        }
        let stamped = |path: PathBuf| {
            let stamp = Stamp::of(&path);
            (path, stamp)
        };
        let mut stamps = vec![stamped(root.join(FILE))];
        let loaded = Self::read(root).and_then(|project| {
            stamps.extend(project.meeting_paths().map(|path| stamped(root.join(path))));
            project
                .check_files(root)
                .map(|()| project)
                .map_err(|e| format!("{FILE}: {e}"))
        });
        let project = Arc::new(loaded.unwrap_or_else(|problem| {
            eprintln!("{}: {problem}; using the defaults", root.display());
            Self::plain(root)
        }));
        SEEN.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(root.to_path_buf(), (stamps, project.clone()));
        project
    }

    /// The project as the client's header and home read it: its name, its register (by its route
    /// when an entrypoint holds it, as every link names a document), its plugins' navigation and
    /// its entrypoints' URLs.
    pub fn view(&self) -> symphony_api::ProjectView {
        symphony_api::ProjectView {
            name: self.name.clone(),
            register: self
                .route(&self.register)
                .unwrap_or_else(|| self.register.clone()),
            nav: crate::plugins::Plugins::for_project(self).nav(),
            entrypoints: self.entrypoints.iter().map(|e| e.url.clone()).collect(),
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
        let id = p.id.unwrap_or_else(|| match (&p.name, slug(&folder)) {
            (Some(named), _) => slug(named),
            (None, found) if found.is_empty() => DEFAULT_ID.to_string(),
            (None, found) => found,
        });
        let name = p.name.unwrap_or_else(|| folder.clone());
        if id.is_empty() || slug(&id) != id {
            return Err(format!(
                "[project] id {id:?} is not a slug (lowercase letters, digits and dashes)"
            ));
        }
        let entrypoints = entrypoints(file.entrypoints)?;
        let register = inside(
            "[project] register",
            p.register
                .unwrap_or_else(|| join(&entrypoints[0].folder, "DECISIONS.md")),
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
        let mut v = file.verdicts;
        for generator in &mut v.generators {
            // Outputs are prefixes of changed names: a folder keeps its slash, so `content/`
            // never matches `content-x/`.
            let slash = if generator.outputs.ends_with('/') {
                "/"
            } else {
                ""
            };
            generator.outputs =
                inside("[verdicts] generators outputs", generator.outputs.clone())? + slash;
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
        let project = Project {
            name,
            id,
            entrypoints,
            register,
            artifacts,
            default_branch,
            plugins: file.plugins.crates,
            theme,
            agents,
            verdicts,
            board,
        };
        let reserved = project.reserved();
        for entry in &project.entrypoints {
            if let Some(route) = reserved
                .iter()
                .find(|route| below(route, &entry.url).is_some())
            {
                return Err(format!(
                    "[[entrypoints]] {} lies on /{route}, a route the Symphony answers",
                    entry.label()
                ));
            }
        }
        Ok(project)
    }

    /// The routes the Symphony and the project's plugins answer, each with everything under it.
    pub fn reserved(&self) -> Vec<String> {
        SYMPHONY_ROUTES
            .iter()
            .map(|route| (*route).to_string())
            .chain(
                crate::plugins::Plugins::for_project(self)
                    .nav()
                    .into_iter()
                    .map(|item| item.href.trim_matches('/').to_string())
                    .filter(|page| !page.is_empty()),
            )
            .collect()
    }

    /// Each place a route another answers falls inside an entrypoint's URL: the entrypoint, the
    /// path inside its folder a file there would be served from, and who answers instead. Another
    /// entrypoint's own folder is not a meeting: that entrypoint serves it.
    fn meetings(&self) -> Vec<(&Entrypoint, String, String)> {
        let mut found = Vec::new();
        let reserved = self.reserved();
        for entry in &self.entrypoints {
            for route in &reserved {
                if let Some(inside) = below(&entry.url, route).filter(|inside| !inside.is_empty()) {
                    found.push((
                        entry,
                        inside.to_string(),
                        "a route the Symphony answers".to_string(),
                    ));
                }
            }
            for other in &self.entrypoints {
                if let Some(inside) = below(&entry.url, &other.url)
                    .filter(|inside| !inside.is_empty() && entry.file(inside) != other.folder)
                {
                    found.push((
                        entry,
                        inside.to_string(),
                        format!(
                            "the entrypoint over {} answers",
                            folder_label(&other.folder)
                        ),
                    ));
                }
            }
        }
        found
    }

    /// The project-relative folders whose entries [`Project::check_files`] reads.
    fn meeting_paths(&self) -> impl Iterator<Item = String> + '_ {
        self.meetings()
            .into_iter()
            .flat_map(|(entry, inside, _)| [entry.folder.clone(), entry.file(&inside)])
    }

    /// A file an entrypoint's patterns include where a route another answers falls, named with
    /// that route: the other's pages win, so the file would never be served.
    fn check_files(&self, root: &Path) -> Result<(), String> {
        for (entry, inside, who) in self.meetings() {
            if let Some(found) = first_included(root, entry, &inside) {
                return Err(format!(
                    "[[entrypoints]] {} would serve {} at /{}, where {who}; move or exclude it",
                    entry.label(),
                    entry.file(&found),
                    entry.route(&found)
                ));
            }
        }
        Ok(())
    }

    /// The entrypoint with the longest URL above `route`, and the route's path inside it.
    fn by_url<'a>(&self, route: &'a str) -> Option<(&Entrypoint, &'a str)> {
        self.entrypoints
            .iter()
            .filter_map(|entry| entry.inside_url(route).map(|inside| (entry, inside)))
            .max_by_key(|(entry, _)| entry.url.len())
    }

    /// The entrypoint serving `route` and the route's path inside it: the one with the longest URL
    /// above it, when no other entrypoint holds the file it maps to (that file has its own route).
    pub fn serving<'a>(&self, route: &'a str) -> Option<(&Entrypoint, &'a str)> {
        let (entry, inside) = self.by_url(route)?;
        let file = entry.file(inside);
        let (holder, _) = self.holding(&file)?;
        std::ptr::eq(holder, entry).then_some((entry, inside))
    }

    /// The entrypoint holding the project-relative `file`, the one with the longest folder above
    /// it, and the file's path inside that folder.
    pub fn holding<'a>(&self, file: &'a str) -> Option<(&Entrypoint, &'a str)> {
        self.entrypoints
            .iter()
            .filter_map(|entry| entry.inside_folder(file).map(|inside| (entry, inside)))
            .max_by_key(|(entry, _)| entry.folder.len())
    }

    /// The route of the project-relative `file`, a document, folder or any file an entrypoint's
    /// folder holds: that entrypoint's URL and the file's path in its folder. `None` outside every
    /// entrypoint.
    pub fn route(&self, file: &str) -> Option<String> {
        self.holding(file)
            .map(|(entry, inside)| entry.route(inside))
    }

    /// The project-relative file `route` stands for, through the entrypoint with the longest URL
    /// above it; a route no entrypoint is above is itself.
    pub fn file(&self, route: &str) -> String {
        match self.by_url(route) {
            Some((entry, inside)) => entry.file(inside),
            None => route.into(),
        }
    }

    /// Whether the project-relative `file` is a document page: Markdown an entrypoint shows, its
    /// history folders included, as the owner's saves and the verdict exemption admit it.
    pub fn is_page(&self, file: &str) -> bool {
        self.holding(file)
            .is_some_and(|(entry, inside)| entry.page(inside))
    }

    /// The refusal of a path that names no document page: `path must be a markdown file under
    /// docs/`, naming each entrypoint.
    pub fn not_a_document(&self) -> String {
        let under: Vec<String> = self
            .entrypoints
            .iter()
            .map(|entry| entry.label().trim_start_matches('/').to_string())
            .map(|label| if label.is_empty() { "/".into() } else { label })
            .collect();
        format!("path must be a markdown file under {}", under.join(" or "))
    }

    /// Whether the project-relative `file` is read as a document: Markdown an entrypoint shows,
    /// outside its history folders.
    pub fn is_document_path(&self, file: &str) -> bool {
        self.holding(file)
            .is_some_and(|(entry, inside)| entry.document(inside))
    }

    /// The pathspecs naming every entrypoint's folder, `.` for one at the project's root.
    pub fn pathspecs(&self) -> Vec<String> {
        self.entrypoints
            .iter()
            .map(|entry| folder_label(&entry.folder).to_string())
            .collect()
    }

    /// Every entrypoint's history folders, project-relative, in the file's order.
    pub fn history(&self) -> Vec<String> {
        self.entrypoints
            .iter()
            .flat_map(|entry| entry.history.iter().map(|folder| entry.file(folder)))
            .collect()
    }

    /// Every file the entrypoints show in the checkout at `root`, project-relative and sorted,
    /// each with the entrypoint showing it. A folder another entrypoint reads is that one's; a
    /// name starting with a dot, and a folder an exclude pattern matches, are passed over whole.
    pub fn shown(&self, root: &Path) -> Vec<(&Entrypoint, String)> {
        let mut found = Vec::new();
        for entry in &self.entrypoints {
            self.walk(root, entry, "", &mut |inside| {
                if entry.shows(inside) {
                    found.push((entry, entry.file(inside)));
                }
            });
        }
        found.sort_by(|a, b| a.1.cmp(&b.1));
        found
    }

    /// Calls `each` with every file under the path `inside` `entry`'s folder that the entrypoint
    /// holds, by its path inside the folder, as [`Project::shown`] passes folders over: a folder's
    /// own files by name, then each folder in it by name, as Python's `os.walk` gives them.
    pub fn walk(&self, root: &Path, entry: &Entrypoint, inside: &str, each: &mut dyn FnMut(&str)) {
        let (folders, files): (Vec<_>, Vec<_>) = self
            .children(root, entry, inside)
            .into_iter()
            .partition(|(_, folder)| *folder);
        for (name, _) in files {
            each(&join(inside, &name));
        }
        for (name, _) in folders {
            self.walk(root, entry, &join(inside, &name), each);
        }
    }

    /// Calls `each` with the name of every folder directly in the path `inside` `entry`'s folder
    /// that [`Project::walk`] goes into, in name order.
    pub fn walk_folders(
        &self,
        root: &Path,
        entry: &Entrypoint,
        inside: &str,
        each: &mut dyn FnMut(&str),
    ) {
        for (name, _) in self
            .children(root, entry, inside)
            .into_iter()
            .filter(|(_, folder)| *folder)
        {
            each(&name);
        }
    }

    /// The names in the path `inside` `entry`'s folder, each with whether it is a folder, in name
    /// order: no name starting with a dot, no link to a folder, no folder an exclude pattern
    /// matches or another entrypoint reads.
    fn children(&self, root: &Path, entry: &Entrypoint, inside: &str) -> Vec<(String, bool)> {
        let Ok(entries) = std::fs::read_dir(root.join(entry.file(inside))) else {
            return Vec::new();
        };
        let mut names: Vec<(String, bool)> = entries
            .flatten()
            .filter(|e| !(e.path().is_dir() && e.file_type().is_ok_and(|kind| kind.is_symlink())))
            .map(|e| {
                (
                    e.file_name().to_string_lossy().into_owned(),
                    e.path().is_dir(),
                )
            })
            .filter(|(name, folder)| {
                let path = join(inside, name);
                !name.starts_with('.')
                    && (!*folder
                        || !entry
                            .exclude
                            .iter()
                            .any(|pattern| crate::entrypoint::matches(pattern, &path))
                            && self
                                .holding(&entry.file(&path))
                                .is_some_and(|(holder, _)| std::ptr::eq(holder, entry)))
            })
            .collect();
        names.sort();
        names
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

/// The `[[entrypoints]]` tables checked, or the convention when there are none.
fn entrypoints(written: Option<Vec<FileEntrypoint>>) -> Result<Vec<Entrypoint>, String> {
    let Some(written) = written else {
        return Ok(vec![Entrypoint::convention()]);
    };
    if written.is_empty() {
        return Err(
            "[[entrypoints]] is empty: leave it out for the convention, docs/ at /docs/".into(),
        );
    }
    let mut found: Vec<Entrypoint> = Vec::new();
    for table in written {
        let mut entry = Entrypoint {
            url: url(&table.url)?,
            folder: String::new(),
            include: table.include.unwrap_or_else(|| vec!["**".into()]),
            exclude: table.exclude,
            types: table
                .types
                .unwrap_or_else(|| vec![crate::entrypoint::MARKDOWN.name.into()]),
            history: Vec::new(),
        };
        let label = entry.label();
        entry.folder = path_in(
            &format!("[[entrypoints]] {label} folder"),
            table.folder,
            true,
        )?;
        let history = table
            .history
            .into_iter()
            .map(|folder| path_in(&format!("[[entrypoints]] {label} history"), folder, false))
            .collect::<Result<_, _>>()?;
        if entry.include.is_empty() {
            return Err(format!(
                "[[entrypoints]] {label} include is empty: it would show nothing"
            ));
        }
        if entry.types.is_empty() {
            return Err(format!(
                "[[entrypoints]] {label} types is empty: it would show nothing"
            ));
        }
        if let Some(unknown) = entry
            .types
            .iter()
            .find(|name| !RENDERERS.iter().any(|r| r.name == name.as_str()))
        {
            return Err(format!(
                "[[entrypoints]] {label} type {unknown:?}: no renderer handles it (the renderers \
                 are {})",
                RENDERERS.map(|r| r.name).join(", ")
            ));
        }
        if let Some(other) = found.iter().find(|other| other.url == entry.url) {
            return Err(format!(
                "[[entrypoints]] {label} is named twice, over {} and {}",
                folder_label(&other.folder),
                folder_label(&entry.folder)
            ));
        }
        if let Some(other) = found.iter().find(|other| other.folder == entry.folder) {
            return Err(format!(
                "[[entrypoints]] {label} reads {}, which {} already serves",
                folder_label(&entry.folder),
                other.label()
            ));
        }
        found.push(Entrypoint { history, ..entry });
    }
    Ok(found)
}

/// An entrypoint's URL as its route: no slashes around it, empty at the root; refused when it
/// climbs, names a dot folder or a branch's `@` scope, or holds what a URL path escapes.
fn url(written: &str) -> Result<String, String> {
    let refused =
        || format!("[[entrypoints]] url {written:?} must be a path such as \"/\" or \"/docs/\"");
    let parts: Vec<&str> = written.split('/').filter(|part| !part.is_empty()).collect();
    if !written.starts_with('/')
        || parts.iter().any(|part| {
            part.starts_with('.')
                || part.starts_with('@')
                || part
                    .chars()
                    .any(|c| c.is_whitespace() || "?#%\\".contains(c))
        })
    {
        return Err(refused());
    }
    Ok(parts.join("/"))
}

/// A folder as a message names it: `.` for the project's root.
fn folder_label(folder: &str) -> &str {
    if folder.is_empty() { "." } else { folder }
}

/// The first file at or under the path `inside` `entry`'s folder that its patterns include, by
/// its path in the folder; names starting with a dot are passed over.
fn first_included(root: &Path, entry: &Entrypoint, inside: &str) -> Option<String> {
    let path = root.join(entry.file(inside));
    if path.is_file() {
        return entry.includes(inside).then(|| inside.to_string());
    }
    let mut names: Vec<String> = std::fs::read_dir(&path)
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names
        .into_iter()
        .find_map(|name| first_included(root, entry, &join(inside, &name)))
}

/// `path` when it is relative and names something inside the project, spelled as the repository
/// names its files (`./docs/` is `docs`), so every comparison with a listed file holds; else an
/// error naming `key`.
fn inside(key: &str, path: String) -> Result<String, String> {
    path_in(key, path, false)
}

/// [`inside`], the project's root itself (`.`, written as the empty path) allowed when `root` is.
fn path_in(key: &str, path: String, root: bool) -> Result<String, String> {
    let refused = || format!("{key} {path:?} must be a path inside the project");
    let mut parts = Vec::new();
    for part in Path::new(&path).components() {
        match part {
            Component::Normal(name) => parts.push(name.to_string_lossy()),
            Component::CurDir => {}
            _ => return Err(refused()),
        }
    }
    if parts.is_empty() && (!root || path.is_empty()) {
        return Err(refused());
    }
    Ok(parts.join("/"))
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
