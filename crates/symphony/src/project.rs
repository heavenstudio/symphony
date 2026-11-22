//! A project: a root folder holding a committed `symphony.toml`. The file names what the Symphony
//! would otherwise assume, and every value it leaves out is the Symphony's generic default, never a
//! game's: no plugins and none of their calls measured, no generators, no check, the folder's name
//! as the project's name and its slug as the id (`project` when the folder's has no letter or
//! digit), one entrypoint serving `docs/` at `/docs/` with `docs/history` skipped, the register
//! `DECISIONS.md` in the first entrypoint's folder, `artifacts/`, `main`, the Symphony's own theme
//! and no panes. An empty file, or none, is a plain project. An unknown
//! table or key is refused by name where the project is loaded or linted ([`Project::load`]) and,
//! where the Symphony runs with the file as it is ([`Project::of`]), left out by name with the rest
//! read, so a misspelt setting never passes for its default unsaid, and a file a newer Symphony
//! wrote keeps its settings under an older one. An `[[entrypoints]]` table and a generator's are
//! strict: a key there the Symphony does not know refuses the file wherever it is read, since a
//! misspelt one would change what is served or built. An unknown renderer, a path outside the
//! project (an entrypoint's folder leading out of it through a link too), and an entrypoint or a
//! file of one on a route another answers are refused by name, so no file is hidden.
//!
//! Beside the main checkout's file, `symphony.local.toml` holds one machine's preferences, which
//! git does not version: the launch's agent, its model and effort, each role's model and the
//! limits, read after the project file by the board's one reader of it (`symphony_board::local`),
//! which this load takes too.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

mod board;
mod file;
mod model_warnings;
mod theme;
pub mod workflow;

pub use crate::entrypoint::Entrypoint;
use crate::entrypoint::{SYMPHONY_ROUTES, below, join};
pub use board::{Board, WipGuidance};
use file::{File, FileEntrypoint};
pub(crate) use model_warnings::warn as warn_models;
use symphony_board::project::{inside, path_in};
// A name as an id: the board's, which judges `[project] id` by it too.
pub use symphony_board::project::slug;
pub use theme::Theme;

/// The project file at a project's root.
pub const FILE: &str = "symphony.toml";
/// The local settings file beside the main checkout's project file, which git does not version
/// and whose preferences hold over the committed ones (`symphony_board::local`).
pub use symphony_board::local::FILE as LOCAL_FILE;

/// How long after a load a plugin's process refused ([`Project::of`]) it is read again: the pause
/// after which that process is started again, so each read again may start it.
pub const AGAIN: Duration = crate::plugins::process::PAUSE;

/// How long a read again keeps its claim at most ([`Project::of`]): past any wait of its load (a
/// plugin's declaration waits [`crate::plugins::process::WAIT`]), so the read's own result sets the
/// next; only a read that never ended (its thread panicked) is claimed again after it.
const HELD: Duration = Duration::from_secs(60);

/// A project as [`Project::of`] read it: each file its load looked at, as it was, the project, why
/// it is the plain project when it is, and when its load is read again, for a plugin's process.
struct Seen {
    stamps: Vec<(PathBuf, Stamp)>,
    project: Arc<Project>,
    problem: Option<String>,
    again: Option<Instant>,
}

/// Each project [`Project::of`] read, by its root.
static SEEN: LazyLock<Mutex<BTreeMap<PathBuf, Seen>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

/// `mutex` held, whatever a thread that panicked holding it left.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What agents' terminals run in (`hc`, herdr-core, shown in the Herdr app); `none` for a project
/// without one.
pub const PANES: [&str; 2] = ["hc", "none"];
/// The id of a project whose folder's name has no letter or digit to make one of.
pub const DEFAULT_ID: &str = "project";
/// Where a project keeps its review guide, in an entrypoint's folder, in the order they are tried:
/// the Symphony's own `docs/workflow/REVIEW.md`, and a consuming project's `docs/workflow/REVIEW.md`.
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
    /// The command that runs Cargo when the server rebuilds itself from this checkout, a program
    /// named with a folder the checkout's own; empty is the Symphony's default: its own workspace's
    /// `tools/cargo.sh`, else plain `cargo`.
    pub cargo: Vec<String>,
    /// The plugins the project uses, by crate name.
    pub plugins: Vec<String>,
    /// The ones of them that answer from a process of their own (`[plugins.process]`), by name; the
    /// rest are linked into the host.
    pub plugin_processes: BTreeMap<String, PluginProcess>,
    /// How its plugins' calls are measured.
    pub plugin_calls: PluginCalls,
    pub theme: Theme,
    pub agents: Agents,
    pub verdicts: Verdicts,
    pub land: Land,
    pub board: Board,
    pub releases: Releases,
}

/// How a plugin that answers from a process of its own is started (`[plugins.process]`,
/// docs/engineering/PLUGIN_PROTOCOL.md): its command, a program the file names by a relative path holding a `/`
/// taken under the project's root, run in that root without the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginProcess {
    pub command: Vec<String>,
    /// The project's root, where it runs.
    pub root: PathBuf,
    /// Where its stderr goes, and its starts and ends: `plugins/NAME.log` in the artifacts folder.
    pub log: PathBuf,
}

impl PluginProcess {
    /// The plugins the `symphony.toml` at `root` names a process for, its `[plugins.process]` read
    /// alone and quietly as [`PluginCalls::read`] reads its settings: a file that does not parse,
    /// or no file, names none. What a plugin command reads before it runs
    /// ([`crate::plugins::command`]), so a command of a plugin this process holds loads no project,
    /// and a linked copy of a plugin named here never answers for it, even while the project does
    /// not load.
    pub fn named_at(root: &Path) -> Vec<String> {
        #[derive(Default, serde::Deserialize)]
        #[serde(default)]
        struct Settings {
            plugins: PluginSettings,
        }
        #[derive(Default, serde::Deserialize)]
        #[serde(default)]
        struct PluginSettings {
            // Each plugin's command is the project's to read: here only which are named.
            process: BTreeMap<String, serde::de::IgnoredAny>,
        }
        std::fs::read_to_string(root.join(FILE))
            .ok()
            .and_then(|text| toml::from_str::<Settings>(&text).ok())
            .map_or_else(Vec::new, |settings| {
                settings.plugins.process.into_keys().collect()
            })
    }
}

/// How a project's plugin calls are measured, off unless its file turns it on
/// (docs/engineering/PLUGIN_CALLS.md): `[plugins] timing` records each call's time
/// ([`crate::plugins::Plugins::at`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PluginCalls {
    pub timing: bool,
}

impl PluginCalls {
    /// Whether a call is recorded: under `timing`.
    pub fn recorded(self) -> bool {
        self.timing
    }

    /// The `[plugins] timing` of the `symphony.toml` at `root`, read alone and quietly: nothing else
    /// in the file is read or checked and nothing is said of it, and a file that does not parse, a
    /// value that is not `true` or `false`, or no file, leaves it off. What a plugin command reads
    /// before it runs ([`crate::plugins::command`]), so with it off it loads no project.
    pub fn read(root: &Path) -> PluginCalls {
        #[derive(Default, serde::Deserialize)]
        #[serde(default)]
        struct Settings {
            plugins: PluginSettings,
        }
        #[derive(Default, serde::Deserialize)]
        #[serde(default)]
        struct PluginSettings {
            timing: bool,
        }
        std::fs::read_to_string(root.join(FILE))
            .ok()
            .and_then(|text| toml::from_str::<Settings>(&text).ok())
            .map(|settings| PluginCalls {
                timing: settings.plugins.timing,
            })
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agents {
    pub panes: String,
    /// Where agents' worktrees go, relative to the root (it may lie outside it).
    pub worktrees: Option<String>,
    pub launch: Launch,
    pub models: Models,
}

/// The defaults a launch takes when it names none; `None` leaves the board's own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
    /// The agent, one a launch starts as the board reads it (`symphony_board::launch::defaults::agent`):
    /// none for one unwritten or blank.
    pub default: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// `[agents.models]`: the model of the project's agent each role runs on, which a board launch and
/// `just tasks limits` read themselves (`symphony_board::launch::Defaults`); `None` leaves
/// `[agents] launch`'s model, else the board's own, for an assessor the smallest its agent offers
/// first, and for an orchestrator whoever starts it chooses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Models {
    pub orchestrator: Option<String>,
    pub executor: Option<String>,
    pub reviewer: Option<String>,
    pub assessor: Option<String>,
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

/// What the Symphony's default `just land` does besides merging, and what the regenerator it starts
/// makes (`symphony_board::land`, which reads the same table from the default branch).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Land {
    /// Run on the default branch's newest tip by the regenerator a land starts, never by the
    /// land; what they change inside their outputs is committed and landed.
    pub generators: Vec<Generator>,
    /// `[land] keys`: a generator's name to the command printing its key, beside the generators,
    /// never one of their fields (`symphony_board::land::Settings::keys`).
    pub keys: BTreeMap<String, Vec<String>>,
    /// Whether a landing pushes the default branch to `origin`.
    pub push: bool,
    /// How long each generator and the check may run, in seconds; a fetch or a push takes five
    /// minutes at most, and a land waits for another with no timeout.
    pub timeout_seconds: u64,
    /// Whether a land runs `[verdicts] check` on its merge.
    pub check: bool,
}

impl Default for Land {
    fn default() -> Land {
        Land {
            generators: Vec::new(),
            keys: BTreeMap::new(),
            push: true,
            timeout_seconds: symphony_board::land::TIMEOUT.as_secs(),
            check: true,
        }
    }
}

/// When the Symphony's own release run (`symphony-build release`, docs/workflow/RELEASES.md) releases
/// besides each day's, and what a green release does then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Releases {
    /// A run on main's tip starts once this many landings reached the default branch since the
    /// last release or the last run that checked something (`just release --due`); 0 starts none.
    pub every_landings: u32,
    /// Whether a green release refreshes the machine: the release installed, each registered
    /// project's board moved to its schema, and `current` pointed at it, which a release's server
    /// follows by its own reload.
    pub refresh: bool,
    /// Whether a green release, once pushed, publishes on crates.io each package whose version
    /// crates.io lacks; off unless the project says, so nothing is published but on its word.
    pub publish: bool,
    /// The command a green release, once pushed, asks for its GitHub Release, run as
    /// `COMMAND… --request FILE` (the GitHub adapter's `symphony-github release`); none, and no
    /// program is run and nothing is published there.
    pub github: Vec<String>,
    /// The command a release run asks to verify its commit on GitHub's machines in place of
    /// verifying it here, run as `COMMAND… --request FILE` (the GitHub adapter's
    /// `symphony-github verify`); none, and no program is run and every run verifies here.
    pub hosted: Vec<String>,
    /// While `hosted` names a command, a run on main's tip also starts at a landing once this many
    /// minutes passed since the last run started and the tip is not the commit last judged: the
    /// hosted clock, set from how long a run there takes. Such a run verifies nothing here. 0
    /// starts none by the clock.
    pub hosted_every_minutes: u32,
    /// The command each release run runs as a check of its own, `rulesets`, after the checks
    /// (the GitHub adapter's `symphony-github rulesets --verify`, where its settings publish pull
    /// requests): red unless it exits 0; none, and the run has no such check.
    pub rulesets: Vec<String>,
    /// How many reruns of a release candidate may come back red before it is dropped and main's
    /// tip is run again; 0 holds no candidate.
    pub candidate_reruns: u32,
    /// How many hours a release candidate is held, from when it was first judged, before it is
    /// dropped; 0 holds no candidate.
    pub candidate_hours: u32,
}

impl Releases {
    /// The landings a release run waits for unless the project says: the owner's 20.
    pub const EVERY_LANDINGS: u32 = 20;
    /// The least minutes between the starts of two runs of the hosted clock unless the project
    /// says; one run at a time holds the release's lock, so a longer run is followed by the next.
    pub const HOSTED_EVERY_MINUTES: u32 = 30;
    /// The red reruns a release candidate may take unless the project says.
    pub const CANDIDATE_RERUNS: u32 = 3;
    /// The hours a release candidate is held unless the project says.
    pub const CANDIDATE_HOURS: u32 = 24;

    /// The minutes of the hosted clock: none while `hosted` names no command or the minutes are 0.
    pub fn hosted_clock(&self) -> Option<u32> {
        (!self.hosted.is_empty() && self.hosted_every_minutes > 0)
            .then_some(self.hosted_every_minutes)
    }

    /// Whether a red run holds its commit as the release candidate: neither bound is 0.
    pub fn holds_candidates(&self) -> bool {
        self.candidate_reruns > 0 && self.candidate_hours > 0
    }
}

impl Default for Releases {
    fn default() -> Releases {
        Releases {
            every_landings: Releases::EVERY_LANDINGS,
            refresh: true,
            publish: false,
            github: Vec::new(),
            hosted: Vec::new(),
            hosted_every_minutes: Releases::HOSTED_EVERY_MINUTES,
            rulesets: Vec::new(),
            candidate_reruns: Releases::CANDIDATE_RERUNS,
            candidate_hours: Releases::CANDIDATE_HOURS,
        }
    }
}

/// `generators` of the table `table` checked: each writes inside the project, a folder's outputs
/// keeping their slash, and has a command as the land reads one (`symphony_board::land::commanded`).
fn checked_generators(table: &str, generators: &mut [Generator]) -> Result<(), String> {
    for generator in generators {
        // Outputs are prefixes of changed names: a folder keeps its slash, so `content/`
        // never matches `content-x/`.
        let slash = if generator.outputs.ends_with('/') {
            "/"
        } else {
            ""
        };
        generator.outputs =
            inside(&format!("{table} generators outputs"), &generator.outputs)? + slash;
        symphony_board::land::commanded(table, generator)?;
    }
    Ok(())
}

/// A project file read for [`Project::of`]: the project of the settings this build knows, and the
/// names of the tables and keys it does not.
type Known = (Project, Vec<String>);

impl Project {
    /// The plain project at `root`: every default, its name and id from the folder's name.
    pub fn plain(root: &Path) -> Project {
        Self::resolve(root, File::default()).expect("the defaults are a valid project")
    }

    /// The project at `root`: its `symphony.toml` over the defaults, or the plain project without
    /// one, with what its local settings file writes over that (`symphony_board::local`: the
    /// launch's agent, model and effort and each role's model). A file that does not parse, names
    /// a key or value the Symphony does not know, or leaves
    /// an entrypoint a file on a route another answers is an error naming the file and what is
    /// wrong.
    pub fn load(root: &Path) -> Result<Project, String> {
        let mut project = match Self::read(root)? {
            Some(text) => Self::parse(root, &text)?,
            None => Self::plain(root),
        };
        project
            .check_files(root, &mut Vec::new())
            .map_err(|e| format!("{FILE}: {e}"))?;
        // A local file that is refused leaves the committed settings, as the board reads them:
        // what is wrong with it is [`Project::local_problems`]' to say.
        if let Ok(Some(local)) = Self::local(root) {
            project.over(&local);
        }
        Ok(project)
    }

    /// The local settings file of the project at `root` ([`LOCAL_FILE`]): beside the project file
    /// of its main checkout, which a linked worktree reads too, having none of its own.
    pub fn local_file(root: &Path) -> PathBuf {
        symphony_board::local::path_beside(&symphony_board::builds::main_of(root).join(FILE))
    }

    /// What the local settings file of the project at `root` writes, as the board reads it.
    pub(crate) fn local(root: &Path) -> Result<Option<symphony_board::local::Local>, Vec<String>> {
        symphony_board::local::beside(&symphony_board::builds::main_of(root).join(FILE))
    }

    /// The launch's agent, model and effort and the roles' models `local` writes, over the
    /// committed ones: the one merge the board's readers make too
    /// (`symphony_board::local::Local::over`).
    fn over(&mut self, local: &symphony_board::local::Local) {
        let (launch, models) = (&mut self.agents.launch, &mut self.agents.models);
        local.over(
            [&mut launch.default, &mut launch.model, &mut launch.effort],
            [
                &mut models.orchestrator,
                &mut models.executor,
                &mut models.reviewer,
                &mut models.assessor,
            ],
        );
    }

    /// What is wrong with the local settings file of the project at `root`: each setting it may
    /// not hold or the Symphony cannot read, by name and line, as the board says it and then reads
    /// none of the file; and the file being tracked by git, which is to version none of it. None
    /// with no such file. What the settings check and the lint refuse.
    pub fn local_problems(root: &Path) -> Vec<String> {
        let mut problems = Self::local(root).err().unwrap_or_default();
        let file = Self::local_file(root);
        let tracked = file.parent().is_some_and(|folder| {
            symphony_board::sys::output(crate::children::git().arg("-C").arg(folder).args([
                "ls-files",
                "--error-unmatch",
                "--",
                LOCAL_FILE,
            ]))
            .is_ok_and(|done| done.status.success())
        });
        if tracked {
            problems.push(format!(
                "{}: git tracks it, and a local settings file is one machine's alone: remove it \
                 from the index (git rm --cached {LOCAL_FILE}) and ignore it",
                file.display()
            ));
        }
        problems
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
    /// know refused by name and line, a limit the board cannot read (`[limits]`'s, or `[board]
    /// orchestrator_slots`) refused as the board says it, with its line, and a `[stamps]` catalogue the board would refuse (an empty
    /// one, a key no stamp has) refused as it says it. The files its entrypoints hold are
    /// [`Project::load`]'s to look at.
    pub fn parse(root: &Path, text: &str) -> Result<Project, String> {
        let mut file = Self::written(text)?;
        workflow::require_supported(&mut file)?;
        if let Some(refusal) = File::refusal(&file.unknown(text)) {
            return Err(refusal);
        }
        let project = Self::resolve(root, file).map_err(|e| format!("{FILE}: {e}"))?;
        // The board reads `[limits]` and `[board] orchestrator_slots` itself and takes a default for
        // a value it cannot read, so the board runs on whatever the file says; the lint and a load
        // refuse that value here.
        if let Some(problem) = symphony_board::limits::parse(FILE, text).problems.first() {
            return Err(problem.clone());
        }
        // The board reads `[stamps]` itself, from the main checkout's committed file, and refuses
        // a catalogue it cannot read where a command needs it; the lint and a load refuse it here.
        symphony_board::stamps::Catalogue::parse(text)?;
        Ok(project)
    }

    /// The file at `root` as [`Project::of`] reads it, before its entrypoints' files are looked
    /// at: the project of the settings this build knows, the names of the tables and keys it
    /// does not, left out ([`File::unknown`]).
    fn read_known(root: &Path) -> Result<Known, String> {
        let Some(text) = Self::read(root)? else {
            return Ok((Self::plain(root), Vec::new()));
        };
        let mut file = Self::written(&text)?;
        workflow::require_supported(&mut file)?;
        let unknown = file.unknown(&text);
        let project = Self::resolve(root, file).map_err(|e| format!("{FILE}: {e}"))?;
        Ok((project, unknown))
    }

    /// The project at `root` as [`Project::load`] reads it, cached until its file or its local
    /// settings file ([`Project::local_file`]) changes, or an entry appears, goes or is replaced in a folder whose entries the load looked at: an
    /// entrypoint's folder, or one where a file of an entrypoint would meet another route, or any
    /// folder under that. A table or key this build does not know is left out and reported, once
    /// per version on stderr, the rest read as written, so a file a newer Symphony wrote keeps its
    /// settings; a project that does not load leaves the plain project in its place, reported the
    /// same way, and says why while it does ([`Project::problem`]). A load refused while a plugin of
    /// the project answering from its process has not declared (its program missing, or slow to
    /// declare at a start) is no setting of the project's: it is read again [`AGAIN`] after, until
    /// it loads, by one read at a time on a thread of its own, the claim held until that read ends
    /// ([`HELD`] at most), so while it is read again every reader keeps the plain project and none
    /// waits on the process; what that read finds is kept only while the files it was started for
    /// are still what is held ([`Project::keep`]). The first read, and the first after a file the
    /// load looked at changes, are no read again: each is the reader's own and waits on the
    /// process's declaration, [`crate::plugins::process::WAIT`] at most, a server's live loop as
    /// much as a request. `symphony-status --lint` refuses each
    /// ([`crate::lint::project_problems`]).
    pub fn of(root: &Path) -> Arc<Project> {
        Self::seen(root).0
    }

    /// Why the project at `root` is the plain project in [`Project::of`]'s place, while its file
    /// does not load: none once it does.
    pub fn problem(root: &Path) -> Option<String> {
        Self::seen(root).1
    }

    /// [`Project::of`], and why it is the plain project when it is.
    fn seen(root: &Path) -> (Arc<Project>, Option<String>) {
        if let Some(held) = lock(&SEEN).get_mut(root)
            && held
                .stamps
                .iter()
                .all(|(path, held)| *held == Stamp::of(path))
        {
            if held.again.is_some_and(|at| Instant::now() >= at) {
                // Read again on a thread of its own, which may wait on a process's declaration:
                // every reader, this one too, keeps what was read meanwhile, none waits. What it
                // read is kept only while the files are still the ones it was started for.
                let (again, said, started) = (
                    root.to_path_buf(),
                    held.problem.clone(),
                    held.stamps.clone(),
                );
                let reading = std::thread::Builder::new()
                    .name("project read again".into())
                    .spawn(move || {
                        let read = Self::load_seen(&again, said.as_deref());
                        Self::keep(&again, &started, read);
                    });
                // Claimed until that read ends, its own result setting the next; a read that
                // cannot start is left to the next pause.
                held.again = Some(Instant::now() + if reading.is_ok() { HELD } else { AGAIN });
            }
            return (held.project.clone(), held.problem.clone());
        }
        let read = Self::load_seen(root, None);
        let seen = (read.project.clone(), read.problem.clone());
        lock(&SEEN).insert(root.to_path_buf(), read);
        seen
    }

    /// `read`, a read again of the project at `root` started while [`Project::of`] held it as read
    /// from the files `started` stamps, kept in its place only while it still holds that: a reader
    /// that found a file changed meanwhile read the project afresh, and a read of the files before
    /// would only cost the next reader one more load. Whether it was kept.
    fn keep(root: &Path, started: &[(PathBuf, Stamp)], read: Seen) -> bool {
        let mut seen = lock(&SEEN);
        let current = seen.get(root).is_some_and(|held| held.stamps == started);
        if current {
            seen.insert(root.to_path_buf(), read);
        }
        current
    }

    /// The project at `root` read for [`Project::of`] to keep, and why it is the plain project when
    /// it is; `said`, the problem a read before said, is not said again.
    fn load_seen(root: &Path, said: Option<&str>) -> Seen {
        let stamped = |path: PathBuf| {
            let stamp = Stamp::of(&path);
            (path, stamp)
        };
        let mut stamps = vec![stamped(root.join(FILE)), stamped(Self::local_file(root))];
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
        let (project, problem, again) = match loaded {
            Ok((mut project, unknown)) => {
                if let Some(refusal) = File::refusal(&unknown) {
                    eprintln!("{}: {refusal}; left out, the rest read", root.display());
                }
                match Self::local(root) {
                    Ok(Some(local)) => project.over(&local),
                    Ok(None) => {}
                    Err(problems) => {
                        for problem in &problems {
                            eprintln!("{}", symphony_board::local::left_out(problem));
                        }
                    }
                }
                (project, None, None)
            }
            Err(problem) => {
                // A load read again for a plugin's process says its problem again only when it
                // changed.
                if said != Some(problem.as_str()) {
                    eprintln!("{}: {problem}; using the defaults", root.display());
                }
                let again =
                    crate::plugins::process::undeclared(root).then(|| Instant::now() + AGAIN);
                (Self::plain(root), Some(problem), again)
            }
        };
        Seen {
            stamps,
            project: Arc::new(project),
            problem,
            again,
        }
    }

    /// The project as the client's header and home read it: its theme package's name, what its
    /// header opens with where that is not the Symphony's own mark and wordmark, its register
    /// (by its route when an entrypoint holds it, as every link names a document), its plugins'
    /// navigation and its entrypoints' URLs, at the server's root (the server sets the base). Its
    /// review guide, and whether its register's file is there, only the project's folder can show:
    /// [`Project::view_at`] reads them.
    pub fn view(&self) -> symphony_api::ProjectView {
        let package = crate::theme::Package::of(self);
        symphony_api::ProjectView {
            wordmark: package.wordmark().unwrap_or_default().to_string(),
            mark: package.mark().to_string(),
            name: package.name,
            base: String::new(),
            register: self
                .route(&self.register)
                .unwrap_or_else(|| self.register.clone()),
            guide: String::new(),
            nav: crate::plugins::Plugins::for_project(self).nav(),
            entrypoints: self.entrypoints.iter().map(|e| e.url.clone()).collect(),
        }
    }

    /// [`Project::view`] of the project at `root`, with its review guide's route, and no register
    /// when its file is not there (a new project has none yet), so no link leads nowhere.
    pub fn view_at(&self, root: &Path) -> symphony_api::ProjectView {
        let view = self.view();
        symphony_api::ProjectView {
            guide: self.guide(root).unwrap_or_default(),
            register: if root.join(&self.register).is_file() {
                view.register.clone()
            } else {
                String::new()
            },
            ..view
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
        symphony_board::project::id(&id)?;
        let artifacts = inside(
            "[project] artifacts",
            &p.artifacts.unwrap_or_else(|| "artifacts".into()),
        )?;
        let plugin_processes =
            plugin_processes(root, &artifacts, &file.plugins.crates, file.plugins.process)?;
        let entrypoints = entrypoints(file.entrypoints, &file.plugins.crates, &plugin_processes)?;
        let register = inside(
            "[project] register",
            &p.register
                .unwrap_or_else(|| join(&entrypoints[0].folder, "DECISIONS.md")),
        )?;
        let default_branch = symphony_board::project::default_branch(p.default_branch.as_deref())?
            .unwrap_or(symphony_board::live::MAIN)
            .to_string();
        let cargo = p.cargo;
        if cargo
            .first()
            .is_some_and(|program| program.trim().is_empty())
        {
            return Err("[project] cargo names no program".into());
        }
        let theme = Theme::from_file(file.theme)?;
        let a = file.agents;
        let panes = a.panes.unwrap_or_else(|| "none".into());
        if !PANES.contains(&panes.as_str()) {
            return Err(format!(
                "[agents] panes {panes:?} is not one of {}",
                PANES.join(", ")
            ));
        }
        let agents = Agents {
            panes,
            worktrees: a.worktrees,
            launch: Launch {
                default: symphony_board::launch::defaults::agent(
                    symphony_board::launch::defaults::AGENT_KEY,
                    a.launch.default.as_deref(),
                )?
                .map(String::from),
                model: a.launch.model,
                effort: a.launch.effort,
            },
            models: Models {
                orchestrator: a.models.orchestrator,
                executor: a.models.executor,
                reviewer: a.models.reviewer,
                assessor: a.models.assessor,
            },
        };
        let mut v = file.verdicts;
        checked_generators("[verdicts]", &mut v.generators)?;
        let mut land = file.land;
        checked_generators("[land]", &mut land.generators)?;
        let keys = symphony_board::land::keyed(&land.generators, std::mem::take(&mut land.keys))?;
        if land.timeout_seconds == 0 {
            return Err("[land] timeout_seconds is 0, which no generator or check runs in".into());
        }
        if let Some(written) = &land.check_paths {
            symphony_board::land::check_paths::parse(written)?;
        }
        if land
            .delivery
            .first()
            .is_some_and(|program| program.trim().is_empty())
        {
            return Err("[land] delivery names no command: leave it out to land here".into());
        }
        if (file.releases.github.first()).is_some_and(|program| program.trim().is_empty()) {
            return Err(
                "[releases] github names no command: leave it out to publish no GitHub Release"
                    .into(),
            );
        }
        if (file.releases.hosted.first()).is_some_and(|program| program.trim().is_empty()) {
            return Err(
                "[releases] hosted names no command: leave it out to verify a release here".into(),
            );
        }
        if (file.releases.rulesets.first()).is_some_and(|program| program.trim().is_empty()) {
            return Err(
                "[releases] rulesets names no command: leave it out to verify no rulesets".into(),
            );
        }
        if let Some(upstream) = land.upstream.take() {
            upstream.judged()?;
        }
        if land.watch_seconds == 0 {
            return Err("[land] watch_seconds is 0: the watcher rests between its rounds".into());
        }
        if land.merge_within_minutes == 0 {
            return Err("[land] merge_within_minutes is 0: a host is given time to merge".into());
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
                .map(|path| inside("[verdicts] record", &path))
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
            cargo,
            plugin_processes,
            plugins: file.plugins.crates,
            plugin_calls: PluginCalls {
                timing: file.plugins.timing,
            },
            theme,
            agents,
            verdicts,
            land: Land {
                generators: land.generators,
                keys,
                push: land.push,
                timeout_seconds: land.timeout_seconds,
                check: land.check,
            },
            board,
            releases: Releases {
                every_landings: file.releases.every_landings,
                refresh: file.releases.refresh,
                publish: file.releases.publish,
                github: file.releases.github,
                hosted: file.releases.hosted,
                hosted_every_minutes: file.releases.hosted_every_minutes,
                rulesets: file.releases.rulesets,
                candidate_reruns: file.releases.candidate_reruns,
                candidate_hours: file.releases.candidate_hours,
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

    /// The route of the project-relative `file` in the project at `root`, as [`Project::of`]
    /// names it ([`Project::route`]), found with no plugin's process started or waited on while
    /// the project loads without one: a route is an entrypoint's URL and the file's path in its
    /// folder, which no declaration changes, so a command run once (`symphony reply`) costs no
    /// process. Only an entrypoint naming a type no renderer but a process's handles needs the
    /// declaration: then the project is read as [`Project::of`] reads it.
    pub fn route_in(root: &Path, file: &str) -> Option<String> {
        match Self::unasked(root) {
            Some(project) => project.route(file),
            None => Self::of(root).route(file),
        }
    }

    /// The project at `root` as [`Project::of`] reads it with its `[plugins.process]` left out, so
    /// no process is asked; none when its file names no process, or does not load without one.
    fn unasked(root: &Path) -> Option<Project> {
        let mut written = Self::written(&Self::read(root).ok()??).ok()?;
        if written.plugins.process.is_empty() {
            return None;
        }
        written.plugins.process.clear();
        let project = Self::resolve(root, written).ok()?;
        project.check_files(root, &mut Vec::new()).ok()?;
        Some(project)
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

    /// The HTML page at `route` in the checkout at `root`, links resolved, with the entrypoint
    /// serving it: when that entrypoint shows it ([`Entrypoint::html_page`]) and its walk reaches
    /// it, and is a file. None for any other route: what the app's frame of an HTML document may
    /// load as that document, and nothing else.
    pub fn html_page(&self, root: &Path, route: &str) -> Option<(&Entrypoint, PathBuf)> {
        let (entry, inside) = self.serving(route)?;
        if !entry.html_page(inside) {
            return None;
        }
        let file = self.reachable(root, entry, inside)?;
        file.is_file().then_some((entry, file))
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

/// The `[plugins.process]` table of the project at `root`: each plugin's command, its program
/// resolved under the root when it is a relative path, and its log in the artifacts folder. A name
/// `[plugins] crates` does not list, a command with no program, or a relative program with a `..`
/// part, which could leave the checkout, is refused by name.
fn plugin_processes(
    root: &Path,
    artifacts: &str,
    plugins: &[String],
    written: BTreeMap<String, Vec<String>>,
) -> Result<BTreeMap<String, PluginProcess>, String> {
    let processes = written.into_iter().map(|(name, mut command)| {
        if !plugins.contains(&name) {
            return Err(format!(
                "[plugins.process] names {name}, which [plugins] crates does not list"
            ));
        }
        let Some(program) = command.first_mut().filter(|p| !p.trim().is_empty()) else {
            return Err(format!("[plugins.process] {name} names no program"));
        };
        if program.contains('/') && Path::new(program.as_str()).is_relative() {
            let parts = Path::new(program.as_str()).components();
            if parts
                .into_iter()
                .any(|part| part == std::path::Component::ParentDir)
            {
                return Err(format!(
                    "[plugins.process] {name} names {program:?}: a relative program is a \
                         path inside the checkout, with no `..`"
                ));
            }
            // Its `.` parts folded: `./bin/plugin` is the checkout's `bin/plugin`.
            let under: PathBuf = root.join(program.as_str()).components().collect();
            *program = under.to_string_lossy().into_owned();
        }
        let log = root
            .join(artifacts)
            .join("plugins")
            .join(format!("{name}.log"));
        let process = PluginProcess {
            command,
            root: root.to_path_buf(),
            log,
        };
        Ok((name, process))
    });
    let mut found = BTreeMap::new();
    for process in processes {
        let (name, process) = process?;
        found.insert(name, process);
    }
    Ok(found)
}

/// The `[[entrypoints]]` tables checked, their types against the renderers of the Symphony and of
/// `plugins` (refused when two of those share a name), or the convention when there are none. A
/// plugin answering from its process (`processes`) gives the renderers it declared; one that cannot
/// say them refuses the file only when a table names a type none of the others handles.
fn entrypoints(
    written: Option<Vec<FileEntrypoint>>,
    plugins: &[String],
    processes: &BTreeMap<String, PluginProcess>,
) -> Result<Vec<Entrypoint>, String> {
    let wanted: Vec<String> = written
        .iter()
        .flatten()
        .flat_map(|table| table.types.iter().flatten().cloned())
        .collect();
    let renderers = crate::renderers::of_project(plugins, processes, &wanted)?;
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
        let types = table.types.unwrap_or_else(|| {
            crate::entrypoint::DEFAULT_TYPES
                .iter()
                .map(|renderer| renderer.name.into())
                .collect()
        });
        let label = entry.label();
        entry.folder = path_in(
            &format!("[[entrypoints]] {label} folder"),
            &table.folder,
            true,
        )?;
        let history = table
            .history
            .into_iter()
            .map(|folder| path_in(&format!("[[entrypoints]] {label} history"), &folder, false))
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

/// A path's length and modification time, its own and then what it leads to (which differ for a
/// link), each nothing when it is absent: what tells a file from the one before it at that path, as
/// a project's file or a plugin's program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stamp([Option<(u64, SystemTime)>; 2]);

impl Stamp {
    pub(crate) fn of(path: &Path) -> Self {
        let held = |m: std::fs::Metadata| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH));
        Self([
            std::fs::symlink_metadata(path).ok().map(held),
            std::fs::metadata(path).ok().map(held),
        ])
    }
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
