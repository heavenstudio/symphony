//! A project: a root folder holding a committed `symphony.toml`. The file names what the Symphony
//! would otherwise assume, and every value it leaves out is the Symphony's generic default, never a
//! game's: no plugins and none of their calls measured, no generators, no check, the folder's name
//! as the project's name and its slug as the id (`project` when the folder's has no letter or
//! digit), one entrypoint serving `docs/` at `/docs/` with `docs/history` skipped, the register
//! `DECISIONS.md` in the first entrypoint's folder, `artifacts/`, `main`, the Symphony's own theme,
//! every built-in transport and no panes. An empty file, or none, is a plain project. An unknown table or key is refused by
//! name where the project is loaded or linted ([`Project::load`]) and, where the Symphony runs with
//! the file as it is ([`Project::of`]), left out by name with the rest read, so a misspelt setting
//! never passes for its default unsaid, and a file a newer Symphony wrote keeps its settings under
//! an older one. An `[[entrypoints]]` table and a generator's are strict: a key there the Symphony
//! does not know refuses the file wherever it is read, since a misspelt one would change what is
//! served or built. An unknown renderer, a path outside the project (an entrypoint's folder
//! leading out of it through a link too), and an entrypoint or a file of one on a route another
//! answers are refused by name, so no file is hidden.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::SystemTime;

mod board;
mod file;
mod theme;

pub use crate::entrypoint::Entrypoint;
use crate::entrypoint::{SYMPHONY_ROUTES, below, join};
pub use board::{Board, WipGuidance};
use file::{File, FileEntrypoint};
pub use theme::Theme;

/// The project file at a project's root.
pub const FILE: &str = "symphony.toml";

/// The agent transports the Symphony knows: Claude's inbox socket, Codex's queue, and the inbox a
/// session without a channel reads.
pub const TRANSPORTS: [&str; 3] = ["claude-uds", "codex-queue", "inbox"];
/// What agents' terminals run in (`hc`, herdr-core, shown in the Herdr app); `none` for a project
/// without one.
pub const PANES: [&str; 2] = ["hc", "none"];
/// The id of a project whose folder's name has no letter or digit to make one of.
pub const DEFAULT_ID: &str = "project";
/// Where a project keeps its review guide, in an entrypoint's folder, in the order they are tried:
/// the Symphony's own `docs/REVIEW.md`, and a consuming project's `docs/workflow/REVIEW.md`.
pub const GUIDES: [&str; 2] = ["REVIEW.md", "workflow/REVIEW.md"];
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
    /// How its plugins' calls are measured.
    pub plugin_calls: PluginCalls,
    pub theme: Theme,
    pub agents: Agents,
    pub verdicts: Verdicts,
    pub land: Land,
    pub board: Board,
    pub releases: Releases,
}

/// How a project's plugin calls are measured, both off unless its file turns them on
/// (docs/PLUGIN_CALLS.md): `[plugins] timing` records each call's time, `[plugins] loopback` sends
/// each call through a JSON round trip inside the process and records both times
/// ([`crate::plugins::Plugins::at`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PluginCalls {
    pub timing: bool,
    pub loopback: bool,
}

impl PluginCalls {
    /// Whether a call is recorded: under either setting.
    pub fn recorded(self) -> bool {
        self.timing || self.loopback
    }
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

/// What the Symphony's default `just land` does besides merging and checking
/// (`symphony_board::land`, which reads the same table from the default branch).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Land {
    /// Run on the merge before its check; what they change inside their outputs is committed.
    pub generators: Vec<Generator>,
    /// Whether a landing pushes the default branch to `origin`.
    pub push: bool,
    /// How long each generator and the check may run, and a land waits for another, in seconds;
    /// a fetch or a push takes five minutes at most.
    pub timeout_seconds: u64,
}

impl Default for Land {
    fn default() -> Land {
        Land {
            generators: Vec::new(),
            push: true,
            timeout_seconds: symphony_board::land::TIMEOUT.as_secs(),
        }
    }
}

/// When the Symphony's own nightly (`symphony nightly`, docs/RELEASES.md) releases besides each night,
/// and what a green release does then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Releases {
    /// A run on main's tip starts once this many landings reached the default branch since the
    /// last release or the last run that checked something (`just nightly --due`); 0 starts none.
    pub every_landings: u32,
    /// Whether a green release refreshes the machine: each consuming project's pin and board,
    /// and the shared server through its own reload.
    pub refresh: bool,
}

impl Releases {
    /// The landings a release run waits for unless the project says: the owner's 20.
    pub const EVERY_LANDINGS: u32 = 20;
}

impl Default for Releases {
    fn default() -> Releases {
        Releases {
            every_landings: Releases::EVERY_LANDINGS,
            refresh: true,
        }
    }
}

/// `generators` of the table `table` checked: each writes inside the project, a folder's outputs
/// keeping their slash, and has a command.
fn checked_generators(table: &str, generators: &mut [Generator]) -> Result<(), String> {
    for generator in generators {
        // Outputs are prefixes of changed names: a folder keeps its slash, so `content/`
        // never matches `content-x/`.
        let slash = if generator.outputs.ends_with('/') {
            "/"
        } else {
            ""
        };
        generator.outputs = inside(
            &format!("{table} generators outputs"),
            generator.outputs.clone(),
        )? + slash;
        if generator.command.is_empty() {
            return Err(format!(
                "{table} generator {} has no command",
                generator.name
            ));
        }
    }
    Ok(())
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
        let project = match Self::read(root)? {
            Some(text) => Self::parse(root, &text)?,
            None => Self::plain(root),
        };
        project
            .check_files(root, &mut Vec::new())
            .map_err(|e| format!("{FILE}: {e}"))?;
        Ok(project)
    }

    /// The file at `root` as written, `None` when there is none.
    fn read(root: &Path) -> Result<Option<String>, String> {
        let path = root.join(FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(crate::text::os_error(&e, &path)),
        }
    }

    /// `text` as written.
    fn written(text: &str) -> Result<File, String> {
        toml::from_str(text).map_err(|e| format!("{FILE}: {}", e.to_string().trim_end()))
    }

    /// `text` as the `symphony.toml` of the project at `root`, a table or key the Symphony does not
    /// know refused by name and line. The files its entrypoints hold are [`Project::load`]'s to
    /// look at.
    pub fn parse(root: &Path, text: &str) -> Result<Project, String> {
        let file = Self::written(text)?;
        if let Some(refusal) = File::refusal(&file.unknown(text)) {
            return Err(refusal);
        }
        Self::resolve(root, file).map_err(|e| format!("{FILE}: {e}"))
    }

    /// The file at `root` as [`Project::of`] reads it, before its entrypoints' files are looked
    /// at: the project of the settings this build knows, and the names of the tables and keys it
    /// does not, left out ([`File::unknown`]).
    fn read_known(root: &Path) -> Result<(Project, Vec<String>), String> {
        let Some(text) = Self::read(root)? else {
            return Ok((Self::plain(root), Vec::new()));
        };
        let file = Self::written(&text)?;
        let unknown = file.unknown(&text);
        let project = Self::resolve(root, file).map_err(|e| format!("{FILE}: {e}"))?;
        Ok((project, unknown))
    }

    /// The project at `root` as [`Project::load`] reads it, cached until its file changes, or an
    /// entry appears, goes or is replaced in a folder whose entries the load looked at: an
    /// entrypoint's folder, or one where a file of an entrypoint would meet another route, or any
    /// folder under that. A table or key this build does not know is left out and reported once
    /// per version on stderr, the rest read as written, so a file a newer Symphony wrote keeps its
    /// settings; a project that does not load leaves the plain project in its place, reported the
    /// same way. `symphony-status --lint` refuses both ([`crate::lint::project_problems`]).
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
        let loaded = Self::read_known(root).and_then(|(project, unknown)| {
            let mut read = Vec::new();
            let checked = project.check_files(root, &mut read);
            read.sort();
            read.dedup();
            stamps.extend(read.into_iter().map(|path| stamped(root.join(path))));
            checked
                .map(|()| (project, unknown))
                .map_err(|e| format!("{FILE}: {e}"))
        });
        let project = Arc::new(match loaded {
            Ok((project, unknown)) => {
                if let Some(refusal) = File::refusal(&unknown) {
                    eprintln!("{}: {refusal}; left out, the rest read", root.display());
                }
                project
            }
            Err(problem) => {
                eprintln!("{}: {problem}; using the defaults", root.display());
                Self::plain(root)
            }
        });
        SEEN.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(root.to_path_buf(), (stamps, project.clone()));
        project
    }

    /// The project as the client's header and home read it: its theme package's name, its register
    /// (by its route when an entrypoint holds it, as every link names a document), its plugins'
    /// navigation and its entrypoints' URLs, at the server's root (the server sets the base). Its
    /// review guide is a file only the project's folder can show: [`Project::view_at`] reads it.
    pub fn view(&self) -> symphony_api::ProjectView {
        symphony_api::ProjectView {
            name: crate::theme::Package::of(self).name,
            base: String::new(),
            register: self
                .route(&self.register)
                .unwrap_or_else(|| self.register.clone()),
            guide: String::new(),
            nav: crate::plugins::Plugins::for_project(self).nav(),
            entrypoints: self.entrypoints.iter().map(|e| e.url.clone()).collect(),
        }
    }

    /// [`Project::view`] of the project at `root`, with its review guide's route.
    pub fn view_at(&self, root: &Path) -> symphony_api::ProjectView {
        symphony_api::ProjectView {
            guide: self.guide(root).unwrap_or_default(),
            ..self.view()
        }
    }

    /// The route of the project's review guide: [`GUIDES`] in its entrypoints' folders, the first
    /// a document of the project at `root`; none when it has none, so no link leads nowhere.
    pub fn guide(&self, root: &Path) -> Option<String> {
        self.entrypoints
            .iter()
            .flat_map(|entry| GUIDES.map(|name| join(&entry.folder, name)))
            .find(|file| self.is_document_path(file) && root.join(file).is_file())
            .and_then(|file| self.route(&file))
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
        let entrypoints = entrypoints(file.entrypoints, &file.plugins.crates)?;
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
        let theme = Theme::from_file(file.theme)?;
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
        checked_generators("[verdicts]", &mut v.generators)?;
        let mut land = file.land;
        checked_generators("[land]", &mut land.generators)?;
        if land.timeout_seconds == 0 {
            return Err("[land] timeout_seconds is 0, which no generator or check runs in".into());
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
        let board = Board::from_file(file.board)?;
        let project = Project {
            name,
            id,
            entrypoints,
            register,
            artifacts,
            default_branch,
            plugins: file.plugins.crates,
            plugin_calls: PluginCalls {
                timing: file.plugins.timing,
                loopback: file.plugins.loopback,
            },
            theme,
            agents,
            verdicts,
            land: Land {
                generators: land.generators,
                push: land.push,
                timeout_seconds: land.timeout_seconds,
            },
            board,
            releases: Releases {
                every_landings: file.releases.every_landings,
                refresh: file.releases.refresh,
            },
        };
        let reserved = project.reserved();
        let uploads = project.artifact(crate::uploads::DIR);
        for entry in &project.entrypoints {
            if below(&uploads, &entry.folder).is_some() {
                return Err(format!(
                    "[[entrypoints]] {} reads {}, the Symphony's uploads, which no entrypoint shows",
                    entry.label(),
                    entry.folder
                ));
            }
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
    /// entrypoint's own folder is not a meeting: that entrypoint serves it. The uploads folder on
    /// the uploads route holds nothing an entrypoint shows ([`Project::holding`]).
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

    /// The first problem with what the entrypoints read in the checkout at `root`: a folder that
    /// lies outside the project or in the uploads folder once its links are resolved, or a file an
    /// entrypoint's patterns include where a route another answers falls, named with that route
    /// (the other's pages win, so the file would never be served). `read` gets the
    /// project-relative path of every folder whose entries it looked at.
    fn check_files(&self, root: &Path, read: &mut Vec<String>) -> Result<(), String> {
        for entry in &self.entrypoints {
            read.push(entry.folder.clone());
            self.checked_bounds(root, entry)?;
        }
        for (entry, inside, who) in self.meetings() {
            if let Some(found) = self.first_included(root, entry, &inside, read) {
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

    /// The entrypoint with the longest URL above `route`, and the route's path inside it; none on
    /// a route the Symphony or a plugin answers, whose pages win.
    fn by_url<'a>(&self, route: &'a str) -> Option<(&Entrypoint, &'a str)> {
        self.entrypoints
            .iter()
            .filter_map(|entry| entry.inside_url(route).map(|inside| (entry, inside)))
            .max_by_key(|(entry, _)| entry.url.len())
            .filter(|_| {
                !self
                    .reserved()
                    .iter()
                    .any(|reserved| below(reserved, route).is_some())
            })
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
    /// it, and the file's path inside that folder. None holds the uploads folder: it is the
    /// Symphony's store of pasted files, never documents, listed or read by no walk.
    pub fn holding<'a>(&self, file: &'a str) -> Option<(&Entrypoint, &'a str)> {
        if below(&self.artifact(crate::uploads::DIR), file).is_some() {
            return None;
        }
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

    /// The first entrypoint's folder when the checkout at `root` holds none of the entrypoints'
    /// folders: a project with no documents yet, whose board and agents are served all the same.
    pub fn missing_documents(&self, root: &Path) -> Option<&str> {
        (!self
            .entrypoints
            .iter()
            .any(|entry| root.join(&entry.folder).is_dir()))
        .then(|| self.entrypoints[0].folder.as_str())
    }

    /// What a document page of the checkout at `root` says while it holds none of the entrypoints'
    /// folders ([`Project::missing_documents`]): that the project has no documents, and how to add
    /// them.
    pub fn no_documents(&self, root: &Path) -> Option<String> {
        self.missing_documents(root).map(|folder| {
            format!(
                "This project has no documents yet: create a {folder}/ folder with a Markdown file \
                 in it, or name the folder that holds them in {FILE}'s [[entrypoints]]."
            )
        })
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
    /// Once links are resolved, no folder outside the entrypoint's and no file outside the project
    /// is listed.
    pub fn walk(&self, root: &Path, entry: &Entrypoint, inside: &str, each: &mut dyn FnMut(&str)) {
        if let Some(bounds) = self.bounds(root, entry) {
            self.walk_in(root, &bounds, entry, inside, each);
        }
    }

    fn walk_in(
        &self,
        root: &Path,
        bounds: &Bounds,
        entry: &Entrypoint,
        inside: &str,
        each: &mut dyn FnMut(&str),
    ) {
        let (folders, files): (Vec<_>, Vec<_>) = self
            .children(root, bounds, entry, inside)
            .into_iter()
            .partition(|(_, folder)| *folder);
        for (name, _) in files {
            each(&join(inside, &name));
        }
        for (name, _) in folders {
            self.walk_in(root, bounds, entry, &join(inside, &name), each);
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
        let Some(bounds) = self.bounds(root, entry) else {
            return;
        };
        for (name, _) in self
            .children(root, &bounds, entry, inside)
            .into_iter()
            .filter(|(_, folder)| *folder)
        {
            each(&name);
        }
    }

    /// Calls `each` with the name of every file directly in the path `inside` `entry`'s folder
    /// that [`Project::walk`] lists, in name order.
    pub fn walk_files(
        &self,
        root: &Path,
        entry: &Entrypoint,
        inside: &str,
        each: &mut dyn FnMut(&str),
    ) {
        let Some(bounds) = self.bounds(root, entry) else {
            return;
        };
        for (name, _) in self
            .children(root, &bounds, entry, inside)
            .into_iter()
            .filter(|(_, folder)| !*folder)
        {
            each(&name);
        }
    }

    /// The file or folder at the path `inside` `entry`'s folder in the checkout at `root`, links
    /// resolved, when the entrypoint's walk reaches it: what a folder page lists, the document view
    /// opens, and nothing else.
    pub fn reachable(&self, root: &Path, entry: &Entrypoint, inside: &str) -> Option<PathBuf> {
        let bounds = self.bounds(root, entry)?;
        self.reach(root, &bounds, entry, inside, &mut Vec::new())?;
        root.join(entry.file(inside)).canonicalize().ok()
    }

    /// Whether the walk from `entry`'s folder reaches the path `inside` it, and as a folder: name by
    /// name, each one listed by [`Project::children`] in the folder before it. `read` gets every
    /// folder whose entries it looked at.
    fn reach(
        &self,
        root: &Path,
        bounds: &Bounds,
        entry: &Entrypoint,
        inside: &str,
        read: &mut Vec<String>,
    ) -> Option<bool> {
        let mut at = String::new();
        let mut folder = true;
        for name in inside.split('/').filter(|name| !name.is_empty()) {
            read.push(entry.file(&at));
            (_, folder) = self
                .children(root, bounds, entry, &at)
                .into_iter()
                .find(|(listed, _)| listed == name)?;
            at = join(&at, name);
        }
        Some(folder)
    }

    /// The first file at or under the path `inside` `entry`'s folder that the entrypoint's walk
    /// reaches and its patterns include, by its path in the folder. `read` gets every folder whose
    /// entries it looked at, from the entrypoint's folder down.
    fn first_included(
        &self,
        root: &Path,
        entry: &Entrypoint,
        inside: &str,
        read: &mut Vec<String>,
    ) -> Option<String> {
        let bounds = self.bounds(root, entry)?;
        if !self.reach(root, &bounds, entry, inside, read)? {
            return entry.includes(inside).then(|| inside.to_string());
        }
        self.first_in(root, &bounds, entry, inside, read)
    }

    /// The files and folders in the path `inside` `entry`'s folder, each with whether it is a
    /// folder, in name order: no name starting with a dot, no link to a folder or to a file
    /// [`Bounds::admits`] refuses, no folder in the uploads folder once links are resolved, an
    /// exclude pattern matches or another entrypoint reads. A folder outside the entrypoint's,
    /// once links are resolved, lists nothing.
    fn children(
        &self,
        root: &Path,
        bounds: &Bounds,
        entry: &Entrypoint,
        inside: &str,
    ) -> Vec<(String, bool)> {
        let folder = root.join(entry.file(inside));
        if !folder
            .canonicalize()
            .is_ok_and(|path| path.starts_with(&bounds.folder))
        {
            return Vec::new();
        }
        let Ok(entries) = std::fs::read_dir(folder) else {
            return Vec::new();
        };
        let mut names: Vec<(String, bool)> = entries
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                let folder = path.is_dir();
                let listed = if e.file_type().is_ok_and(|kind| kind.is_symlink()) {
                    path.is_file()
                        && path
                            .canonicalize()
                            .is_ok_and(|target| bounds.admits(&target))
                } else if folder {
                    !bounds.in_uploads(&path)
                } else {
                    path.is_file()
                };
                listed.then(|| (e.file_name().to_string_lossy().into_owned(), folder))
            })
            .filter(|(name, folder)| {
                let path = join(inside, name);
                !name.starts_with('.')
                    && (!*folder
                        || !entry.excluded(&path)
                            && self
                                .holding(&entry.file(&path))
                                .is_some_and(|(holder, _)| std::ptr::eq(holder, entry)))
            })
            .collect();
        names.sort();
        names
    }

    /// The bounds of `entry` in the checkout at `root`; `None` when its folder is missing, leads
    /// out of the checkout or into the uploads folder ([`Project::checked_bounds`]).
    fn bounds(&self, root: &Path, entry: &Entrypoint) -> Option<Bounds> {
        self.checked_bounds(root, entry).ok().flatten()
    }

    /// The bounds of `entry` in the checkout at `root`, `None` when its folder is missing; an error
    /// naming it when, links resolved, it lies outside the checkout or in the uploads folder.
    fn checked_bounds(&self, root: &Path, entry: &Entrypoint) -> Result<Option<Bounds>, String> {
        let (Ok(project), Ok(folder)) =
            (root.canonicalize(), root.join(&entry.folder).canonicalize())
        else {
            return Ok(None);
        };
        let uploads = root
            .join(self.artifact(crate::uploads::DIR))
            .canonicalize()
            .ok();
        let refused = |place: &str, why: &str| {
            Err(format!(
                "[[entrypoints]] {} folder {} lies {place} once its links are resolved{why}",
                entry.label(),
                folder_label(&entry.folder)
            ))
        };
        if !folder.starts_with(&project) {
            return refused("outside the project", "");
        }
        if uploads
            .as_ref()
            .is_some_and(|uploads| folder.starts_with(uploads))
        {
            return refused(
                &format!("in {}", self.artifact(crate::uploads::DIR)),
                ": the Symphony's uploads, which no entrypoint shows",
            );
        }
        Ok(Some(Bounds {
            project,
            folder,
            uploads,
        }))
    }

    /// [`Project::first_included`] under the folder `inside` the walk reached.
    fn first_in(
        &self,
        root: &Path,
        bounds: &Bounds,
        entry: &Entrypoint,
        inside: &str,
        read: &mut Vec<String>,
    ) -> Option<String> {
        read.push(entry.file(inside));
        let (folders, files): (Vec<_>, Vec<_>) = self
            .children(root, bounds, entry, inside)
            .into_iter()
            .partition(|(_, folder)| *folder);
        files
            .into_iter()
            .map(|(name, _)| join(inside, &name))
            .find(|file| entry.includes(file))
            .or_else(|| {
                folders.into_iter().find_map(|(name, _)| {
                    self.first_in(root, bounds, entry, &join(inside, &name), read)
                })
            })
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

/// The `[[entrypoints]]` tables checked, their types against the renderers of the Symphony and of
/// `plugins` (refused when two of those share a name), or the convention when there are none.
fn entrypoints(
    written: Option<Vec<FileEntrypoint>>,
    plugins: &[String],
) -> Result<Vec<Entrypoint>, String> {
    let renderers = crate::renderers::of_project(plugins)?;
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
            types: Vec::new(),
            history: Vec::new(),
        };
        let types = table
            .types
            .unwrap_or_else(|| vec![crate::entrypoint::MARKDOWN.name.into()]);
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
        if types.is_empty() {
            return Err(format!(
                "[[entrypoints]] {label} types is empty: it would show nothing"
            ));
        }
        entry.types = crate::renderers::resolve(&label, &types, &renderers)?;
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

/// What a walk of an entrypoint stays in, links resolved: the checkout, which a linked file must
/// lie in, and the entrypoint's folder in it, which every folder listed lies in; never the
/// uploads folder, when there is one.
struct Bounds {
    project: PathBuf,
    folder: PathBuf,
    uploads: Option<PathBuf>,
}

impl Bounds {
    /// Whether a file resolving to `target` (links resolved) may be listed and read: in the
    /// checkout under no name starting with a dot (counted from the entrypoint's folder inside it,
    /// as the walk counts them), and never in the uploads folder, whose files no document
    /// replaces.
    fn admits(&self, target: &Path) -> bool {
        let shown = |rest: &Path| {
            !rest
                .components()
                .any(|name| name.as_os_str().to_string_lossy().starts_with('.'))
        };
        !self
            .uploads
            .as_ref()
            .is_some_and(|uploads| target.starts_with(uploads))
            && match target.strip_prefix(&self.folder) {
                Ok(rest) => shown(rest),
                Err(_) => target.strip_prefix(&self.project).is_ok_and(shown),
            }
    }

    /// Whether `path`, links resolved, lies in the uploads folder: a folder the walk never enters,
    /// whatever link above it leads there.
    fn in_uploads(&self, path: &Path) -> bool {
        self.uploads.as_ref().is_some_and(|uploads| {
            path.canonicalize()
                .is_ok_and(|resolved| resolved.starts_with(uploads))
        })
    }
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

/// A path's length and modification time, its own and then what it leads to (which differ for a
/// link), each nothing when it is absent.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp([Option<(u64, SystemTime)>; 2]);

impl Stamp {
    fn of(path: &Path) -> Self {
        let held = |m: std::fs::Metadata| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH));
        Self([
            std::fs::symlink_metadata(path).ok().map(held),
            std::fs::metadata(path).ok().map(held),
        ])
    }
}
