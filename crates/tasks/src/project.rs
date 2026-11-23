//! Which project a command runs in, and so whose board it reaches ("How agents attach" in
//! `docs/engineering/analysis/studio-extraction.md`): the nearest `symphony.toml` walking up from
//! where it runs, never past the top of the checkout it is in, names the project; with no file
//! there, the checkout's main and the machine folder's registry name it. The board is the one in
//! the repository's common dir ([`crate::server::Home::in_common`]), with the session, its callback,
//! its roles and its heartbeat keys: the walk stops at the checkout's top, so it names the board's
//! project and never reaches another repository's board.
//!
//! The machine folder's rule ([`machine_folder`]) is here, where the board's command line reads the
//! registry; the Symphony's machine folder is the same one.

use crate::git;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

/// A project's file, at its root.
pub const FILE: &str = "symphony.toml";
/// Names the machine folder in place of the platform's: a test's own, never the owner's.
pub const MACHINE_DIR: &str = "SYMPHONY_MACHINE_DIR";
/// The registry of the projects the machine's server serves, in the machine folder.
pub const REGISTRY: &str = "projects.json";

/// Where a command's project is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// The folder holding its file, else the top of the checkout the command runs in.
    pub root: PathBuf,
    /// Its file, if it has one.
    pub file: Option<PathBuf>,
    /// The repository's common dir, which holds its board (`symphony/tasks/`).
    pub common: PathBuf,
}

/// The project a command runs in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub place: Place,
    /// What it is named by: its file's `[project] id` as [`id`] takes it, else the registry's id for
    /// its main checkout, else the main checkout's folder name.
    pub id: String,
}

/// Where the project of a command run in `cwd` is: the nearest folder from `cwd` up holding a
/// [`FILE`], or, first, the checkout's top (a folder holding `.git`) with none below it; and the
/// common dir of that folder's repository.
///
/// # Errors
///
/// When the folder is in no repository: a board lives in a repository's common dir.
pub fn locate(cwd: &Path) -> Result<Place, String> {
    let (root, file) = nearest(cwd);
    let common = git::common_dir(&root).map_err(|e| e.to_string())?;
    Ok(Place { root, file, common })
}

/// The same project boundary as [`locate`], read only from filesystem markers. A hook uses this
/// on every tool call: an unknown layout returns none instead of starting a fallback process.
pub fn locate_quick(cwd: &Path) -> Option<Place> {
    let (root, file) = nearest(cwd);
    let common = git::read(&root)?;
    Some(Place { root, file, common })
}

fn nearest(cwd: &Path) -> (PathBuf, Option<PathBuf>) {
    let here = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let mut found = (here.clone(), None);
    for folder in here.ancestors() {
        let file = folder.join(FILE);
        if file.is_file() {
            found = (folder.to_path_buf(), Some(file));
            break;
        }
        if folder.join(".git").symlink_metadata().is_ok() {
            found = (folder.to_path_buf(), None);
            break;
        }
    }
    found
}

/// The project of a command run in `cwd` ([`locate`]), named by its file, else by the registry in
/// the machine folder `machine` (none: no registry is read), else by its main checkout's folder.
///
/// # Errors
///
/// As [`locate`].
pub fn find(cwd: &Path, machine: Option<&Path>) -> Result<Project, String> {
    let place = locate(cwd)?;
    let id = place
        .file
        .as_deref()
        .and_then(file_id)
        .or_else(|| machine.and_then(|machine| registered(machine, &place.common)))
        .unwrap_or_else(|| {
            let main = main_checkout(&place.common).unwrap_or(&place.root);
            main.file_name().map_or_else(
                || main.display().to_string(),
                |name| name.to_string_lossy().into(),
            )
        });
    Ok(Project { place, id })
}

/// How a refusal to reach the board of a project whose folder is gone ends ([`at`]), and the
/// Symphony's word for one whose folder no longer holds its repository: what counts as a project
/// with no board ([`crate::board_health::absent`]).
pub const GONE: &str = "its board is never looked for in the folders above it";

/// The project whose folder is `root`, as the Symphony reaches a project's board from that folder
/// ([`crate::cli::board`]): [`find`] from it, refused when `root` is not there, or went while it
/// was looked up. [`find`] from a folder that is gone walks up its path to whatever checkout holds
/// it and names that repository's board, which a reach then starts: a server serving a project
/// removed under it would start another repository's board.
///
/// # Errors
///
/// When `root` is not a folder that is there, saying [`GONE`]; and as [`find`].
pub fn at(root: &Path, machine: Option<&Path>) -> Result<Project, String> {
    let gone = || format!("the project's folder {} is gone: {GONE}", root.display());
    if !root.is_dir() {
        return Err(gone());
    }
    let found = find(root, machine);
    if !root.is_dir() {
        return Err(gone());
    }
    found
}

/// The project whose folder is `root` ([`at`]), held to the repository whose common dir is
/// `repository`, the one the Symphony found from that folder when it began to serve it: refused,
/// saying [`GONE`], when the lookup finds another (a folder left without its `.git` leads it up to
/// the checkout that holds the folder) or that one is a repository no more ([`is_repository`]). A
/// reach takes the board in the common dir this checked, so a folder removed after the check leads
/// it to no other repository's board.
///
/// # Errors
///
/// As [`at`], and when the folder no longer holds `repository`.
pub fn held(root: &Path, machine: Option<&Path>, repository: &Path) -> Result<Project, String> {
    let project = at(root, machine)?;
    if project.place.common != repository || !is_repository(repository) {
        return Err(format!(
            "the project's folder {} no longer holds the repository it was served from, {}: \
             {GONE}",
            root.display(),
            repository.display()
        ));
    }
    Ok(project)
}

/// Whether the common dir `common` is a repository's still: it holds `HEAD`, which git keeps there
/// at every moment. A `.git` removed and made again by a board's start under way, which makes only
/// the board's own folders in it (SurrealDB makes every missing folder of its store's path), holds
/// none.
pub fn is_repository(common: &Path) -> bool {
    common.join("HEAD").is_file()
}

/// The `[project] id` the file at `path` names, as [`id`] reads it: none when it names none, does
/// not read, or names one the Symphony refuses (no slug), which the board reads as unwritten.
fn file_id(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let file = text.parse::<toml::Table>().ok()?;
    let written = file.get("project")?.get("id")?.as_str()?;
    id(written).ok().map(str::to_string)
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

/// `id` as a project's id, the one rule for `[project] id`: a slug of itself ([`slug`]: lowercase
/// letters and digits, single dashes between them), never empty. The Symphony's load judges the id
/// the file writes, and the one it makes of the name or the folder, by it; the board's own read of
/// the file ([`find`]) takes what it takes, and reads one it refuses as unwritten.
///
/// # Errors
///
/// For one that is no slug, naming the key.
pub fn id(id: &str) -> Result<&str, String> {
    if id.is_empty() || slug(id) != id {
        return Err(format!(
            "[project] id {id:?} is not a slug (lowercase letters, digits and dashes)"
        ));
    }
    Ok(id)
}

/// `written`, the `[project] default_branch` the project file writes (none: it writes none), as
/// every reader of the key takes it: as written, never trimmed, and none when unwritten, which is
/// [`crate::live::MAIN`]. The Symphony's load, the land ([`crate::land::Settings`]) and the live
/// board's guard ([`crate::live::default_branch`]) each read it by this.
///
/// # Errors
///
/// For one that is empty or blank, naming the key.
pub fn default_branch(written: Option<&str>) -> Result<Option<&str>, String> {
    match written {
        Some(branch) if branch.trim().is_empty() => Err("[project] default_branch is empty".into()),
        written => Ok(written),
    }
}

/// `path`, which the project file writes for `key`, as a path inside the project: relative, its
/// `.` parts dropped and its parts joined by `/`, as the repository names its files (`./docs/` is
/// `docs`). The one rule for a project-relative path in the project file: the Symphony's load
/// judges every such key by it, and the board's own reads of the file ([`Project::guide`],
/// [`board_path`], [`artifacts`], [`documents`]) take what it takes, so a path one refuses the
/// other never follows.
///
/// # Errors
///
/// When it leads out of the project (absolute, or with a `..` part) or names nothing in it (empty,
/// or `.` alone), naming `key`.
pub fn inside(key: &str, path: &str) -> Result<String, String> {
    path_in(key, path, false)
}

/// [`inside`], the project's root itself allowed when `root` is: `.`, read as the empty path (an
/// entrypoint's folder). The empty path written is refused all the same.
///
/// # Errors
///
/// As [`inside`], but for `.` when `root` is.
pub fn path_in(key: &str, path: &str, root: bool) -> Result<String, String> {
    let refused = || format!("{key} {path:?} must be a path inside the project");
    let mut parts = Vec::new();
    for part in Path::new(path).components() {
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

/// The board's guide of a project whose file names none, project-relative.
pub const GUIDE: &str = "TASKS.md";

impl Project {
    /// The project's board guide, project-relative, which says where its board lives and how it
    /// moves: its file's `[board] guide` as [`inside`] reads it, else [`GUIDE`]. A file that does
    /// not read names none, and nor does a guide the Symphony refuses (empty, or leading out of
    /// the project).
    pub fn guide(&self) -> String {
        self.place
            .file
            .as_deref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| text.parse::<toml::Table>().ok())
            .and_then(|file| {
                let guide = file.get("board")?.get("guide")?.as_str()?;
                inside("[board] guide", guide).ok()
            })
            .unwrap_or_else(|| GUIDE.to_string())
    }
}

/// Where the path `[board] KEY` names in the project file of the main checkout of the repository
/// whose common dir is `common` is, as [`inside`] reads it: one under `.git/` in the common dir,
/// any other under the main checkout. None when there is no main checkout, no file, it does not
/// read, or it names no path there that the Symphony takes (empty, or leading out of the project):
/// the project has none of that kind, and the board reads none (`[board] slot_locks`, `[board]
/// assessments`; the Symphony's `project::Board` refuses what [`inside`] does). Only that key is
/// read; the rest of the file is the Symphony's.
pub fn board_path(common: &Path, key: &str) -> Option<PathBuf> {
    let root = main_checkout(common)?;
    let text = std::fs::read_to_string(root.join(FILE)).ok()?;
    let file = text.parse::<toml::Table>().ok()?;
    let written = file.get("board")?.get(key)?.as_str()?;
    let path = inside(&format!("[board] {key}"), written).ok()?;
    Some(match path.strip_prefix(".git/") {
        Some(rest) => common.join(rest),
        None => root.join(path),
    })
}

/// The project's artifacts folder, project-relative, as the project file at `root` names it
/// (`[project] artifacts`, else `artifacts`) and the Symphony reads it (`Project::artifact`), by
/// [`inside`]. None when the file does not read, or names a folder [`inside`] refuses, as the
/// Symphony does.
pub fn artifacts(root: &Path) -> Option<String> {
    let named = match std::fs::read_to_string(root.join(FILE)) {
        Ok(text) => match text.parse::<toml::Table>().ok()?.get("project") {
            Some(project) => match project.get("artifacts") {
                Some(folder) => folder.as_str()?.to_string(),
                None => "artifacts".into(),
            },
            None => "artifacts".into(),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "artifacts".into(),
        Err(_) => return None,
    };
    inside("[project] artifacts", &named).ok()
}

/// The key of the ignored paths a project names as going with a merged worktree, in `[board]`.
pub const DISPOSABLE_KEY: &str = "sweep_disposable";

/// Whether `pattern` is one the sweep takes among a project's own disposable paths
/// ([`disposable`]): a path from the project's root as git names an ignored one, a folder ending in
/// `/`, matched as the board's own are ([`crate::sweep::git::fnmatch`]: `*` any run of characters,
/// a `/` too, and `?` any one).
///
/// # Errors
///
/// Why not: it is empty, leaves the project, is written in a way git names no path, or names
/// nothing of its own, so that every ignored file would go.
pub fn disposable_pattern(pattern: &str) -> Result<(), String> {
    let refused = |why: &str| Err(format!("{pattern:?} {why}"));
    if pattern.is_empty() {
        return refused("is empty");
    }
    if pattern.starts_with('/') {
        return refused("starts with `/`; it is a path from the project's root");
    }
    if pattern.contains('\\') || pattern.chars().any(char::is_control) {
        return refused("holds a backslash or a control character, which no path git names holds");
    }
    if pattern.split('/').any(|part| matches!(part, "." | "..")) {
        return refused("holds a `.` or `..` part; it names a path inside the project");
    }
    if !pattern.chars().any(|c| !matches!(c, '*' | '?' | '/' | '.')) {
        return refused("names nothing of its own, so every ignored file would go");
    }
    Ok(())
}

/// The ignored paths that go with a merged worktree beside the board's own
/// ([`crate::sweep::git::DISPOSABLE`]), as the project file at the main checkout `root` names them
/// (`[board] sweep_disposable`): another stack's dependency and build folders, which a checkout
/// makes again. None when there is no file, it names none, or it does not read: an array that is
/// not all patterns the sweep takes ([`disposable_pattern`], which the Symphony's lint refuses by)
/// names none, so nothing goes on a guess. Only that key is read; the rest of the file is the
/// Symphony's.
pub fn disposable(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join(FILE)) else {
        return Vec::new();
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let Some(written) = table
        .get("board")
        .and_then(|board| board.get(DISPOSABLE_KEY))
        .and_then(toml::Value::as_array)
    else {
        return Vec::new();
    };
    let mut patterns = Vec::new();
    for value in written {
        match value.as_str() {
            Some(pattern) if disposable_pattern(pattern).is_ok() => {
                if !patterns.iter().any(|known| known == pattern) {
                    patterns.push(pattern.to_string());
                }
            }
            _ => return Vec::new(),
        }
    }
    patterns
}

/// The key by which a project has a server of its own run the idle-session sweep, in `[board]`.
pub const OWN_SWEEP_KEY: &str = "own_sweep";

/// Whether the project file at `root` has a server of its own run the idle-session sweep for it
/// (`[board] own_sweep = true`): a project served by no other server, as one that runs a source
/// build with no release on the machine, whose finished agents' terminals nothing else would
/// close. False when there is no file, it does not read, or the key is unwritten or not `true`:
/// a server of its own sweeps on no guess, since a preview's board may be a copy that names
/// another board's terminals. Only that key is read; the rest of the file is the Symphony's.
pub fn own_sweep(root: &Path) -> bool {
    std::fs::read_to_string(root.join(FILE))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|table| table.get("board")?.get(OWN_SWEEP_KEY)?.as_bool())
        .unwrap_or(false)
}

/// The convention's documents folder, read when a project file names no `[[entrypoints]]`.
const DOCS: &str = "docs";

/// The folders the Symphony reads a project's documents from, project-relative ("" the project's
/// root), as the project file `text` names them in `[[entrypoints]] folder` and the Symphony reads
/// them (`Project::pathspecs`): `docs` when it names none, the convention's. Where an owner's line on
/// a card can be written, each folder by [`path_in`]. A file that does not read, or a folder it
/// refuses (which the Symphony refuses), reads as the project's root: every folder, never one too
/// few.
pub fn documents(text: &str) -> Vec<String> {
    let root = || vec![String::new()];
    let Ok(table) = text.parse::<toml::Table>() else {
        return root();
    };
    let Some(written) = table.get("entrypoints") else {
        return vec![DOCS.into()];
    };
    let Some(entries) = written.as_array() else {
        return root();
    };
    let mut folders = Vec::new();
    for entry in entries {
        let Some(folder) = entry.get("folder").and_then(toml::Value::as_str) else {
            return root();
        };
        match path_in("[[entrypoints]] folder", folder, true) {
            Ok(folder) => folders.push(folder),
            Err(_) => return root(),
        }
    }
    if folders.is_empty() {
        return root();
    }
    folders
}

/// [`documents`] as the project file at `root` names them: the convention's with no file there.
pub fn documents_at(root: &Path) -> Vec<String> {
    match std::fs::read_to_string(root.join(FILE)) {
        Ok(text) => documents(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => vec![DOCS.into()],
        Err(_) => vec![String::new()],
    }
}

/// The main checkout of the repository whose common dir is `common`: the folder holding it, when it
/// is a `.git` folder.
fn main_checkout(common: &Path) -> Option<&Path> {
    if common.file_name()? == ".git" {
        common.parent()
    } else {
        None
    }
}

/// The id the registry in `machine` gives the project whose repository's common dir is `common`:
/// the entry for its main checkout, else the first whose root is in that repository.
fn registered(machine: &Path, common: &Path) -> Option<String> {
    let projects = registered_roots(machine);
    let main = main_checkout(common);
    projects
        .iter()
        .find(|(_, root)| Some(root.as_path()) == main)
        .or_else(|| {
            projects
                .iter()
                .find(|(_, root)| git::common_dir(root).is_ok_and(|theirs| theirs == common))
        })
        .map(|(id, _)| id.clone())
}

/// Each project the registry in `machine` lists, its id and root, in its order: none when the
/// registry is missing or does not read.
pub fn registered_roots(machine: &Path) -> Vec<(String, PathBuf)> {
    #[derive(serde::Deserialize)]
    struct Registry {
        #[serde(default)]
        projects: Vec<Entry>,
    }
    #[derive(serde::Deserialize)]
    struct Entry {
        id: String,
        root: PathBuf,
    }
    std::fs::read_to_string(machine.join(REGISTRY))
        .ok()
        .and_then(|text| crate::objects::parse::<Registry>(&text))
        .map(|registry| {
            registry
                .projects
                .into_iter()
                .map(|entry| (entry.id, entry.root))
                .collect()
        })
        .unwrap_or_default()
}

/// The machine folder, by `var` (the environment's variables), on macOS when `macos`:
/// [`MACHINE_DIR`] when it names one, else the platform's data folder for the Symphony
/// (`~/Library/Application Support/Symphony/` on macOS, `$XDG_DATA_HOME/symphony` elsewhere, else
/// `~/.local/share/symphony`).
///
/// # Errors
///
/// When [`MACHINE_DIR`] names a relative path, or the platform's folder needs a `HOME` the
/// environment does not have.
pub fn machine_folder(
    var: impl Fn(&str) -> Option<OsString>,
    macos: bool,
) -> Result<PathBuf, String> {
    let absolute = |name: &str| {
        var(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    if let Some(named) = var(MACHINE_DIR).filter(|value| !value.is_empty()) {
        let path = PathBuf::from(&named);
        return if path.is_absolute() {
            Ok(path)
        } else {
            Err(format!(
                "{MACHINE_DIR} names {}, not an absolute folder",
                path.display()
            ))
        };
    }
    let home = absolute("HOME");
    let found = if macos {
        home.map(|home| home.join("Library/Application Support/Symphony"))
    } else {
        absolute("XDG_DATA_HOME")
            .map(|data| data.join("symphony"))
            .or_else(|| home.map(|home| home.join(".local/share/symphony")))
    };
    found.ok_or_else(|| "the machine folder needs HOME, an absolute folder".into())
}

/// The installation's identity file in the machine folder, which the Symphony makes once
/// (`symphony::machine::installation_id`).
pub const MACHINE_ID: &str = "machine-id";

/// The installation identity the machine folder `folder` holds, read and never made: `1\t`, 32 hex
/// digits and a newline, in a regular file reached without following a link. Anything else is an
/// error, never replaced here.
pub fn machine_id(folder: &Path) -> std::io::Result<String> {
    use std::io::Read as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(folder.join(MACHINE_ID))?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other("identity is not a regular file"));
    }
    let mut text = String::new();
    file.take(128).read_to_string(&mut text)?;
    let id = text
        .strip_prefix("1\t")
        .and_then(|text| text.strip_suffix('\n'))
        .filter(|id| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()));
    id.map(str::to_owned)
        .ok_or_else(|| std::io::Error::other("identity has invalid version or contents"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_inside_the_project_is_spelt_as_the_repository_names_it_and_any_other_is_refused() {
        for (path, spelt) in [
            ("docs", "docs"),
            ("./docs/", "docs"),
            ("a/./b//c", "a/b/c"),
            (".git/test-slots", ".git/test-slots"),
            ("./.git/x", ".git/x"),
            ("..x/y", "..x/y"),
        ] {
            assert_eq!(inside("[k] key", path).as_deref(), Ok(spelt), "{path}");
            assert_eq!(
                path_in("[k] key", path, true).as_deref(),
                Ok(spelt),
                "{path}"
            );
        }
        for path in ["", ".", "./", "../x", "a/../b", "/abs", "/", "x/.."] {
            assert_eq!(
                inside("[k] key", path),
                Err(format!(
                    "[k] key {path:?} must be a path inside the project"
                )),
                "{path}"
            );
        }
        // The root itself is a folder only where it is asked for, and never the empty path written.
        for path in [".", "./", "././"] {
            assert_eq!(path_in("[k] key", path, true).as_deref(), Ok(""), "{path}");
        }
        for path in ["", "../x", "/abs"] {
            assert!(path_in("[k] key", path, true).is_err(), "{path}");
        }
    }

    #[test]
    fn a_name_made_a_slug_keeps_lowercase_letters_and_digits_and_one_dash_for_each_other_run() {
        for (name, made) in [
            ("Lamp Shop", "lamp-shop"),
            ("  lamp__shop!! ", "lamp-shop"),
            ("-x-", "x"),
            ("ÁB c9", "b-c9"),
            ("v2.1", "v2-1"),
            ("", ""),
            ("!!", ""),
        ] {
            assert_eq!(slug(name), made, "{name:?}");
        }
    }

    #[test]
    fn a_project_id_is_a_slug_of_itself_and_any_other_is_refused_naming_the_key() {
        for id in ["lamp-shop", "x", "v2-1", "9"] {
            assert_eq!(super::id(id), Ok(id), "{id}");
        }
        for id in [
            "",
            " ",
            "Lamp",
            "lamp shop",
            "lamp_shop",
            "-lamp",
            "lamp-",
            "lamp--shop",
            " lamp",
        ] {
            assert_eq!(
                super::id(id),
                Err(format!(
                    "[project] id {id:?} is not a slug (lowercase letters, digits and dashes)"
                )),
                "{id:?}"
            );
        }
    }

    #[test]
    fn a_default_branch_is_as_written_none_unwritten_and_a_blank_one_refused() {
        assert_eq!(default_branch(None), Ok(None));
        for branch in ["main", "trunk", "release/2", " dev "] {
            assert_eq!(default_branch(Some(branch)), Ok(Some(branch)), "{branch:?}");
        }
        for branch in ["", " ", "\t", "\n "] {
            assert_eq!(
                default_branch(Some(branch)),
                Err("[project] default_branch is empty".into()),
                "{branch:?}"
            );
        }
    }

    #[test]
    fn the_files_id_is_a_slug_it_writes_and_none_for_any_other() {
        let (root, _) = checkout("file-id", None);
        let file = root.join(FILE);
        assert_eq!(file_id(&file), None, "no file");
        for (text, id) in [
            ("[project]\nid = \"lamp-shop\"\n", Some("lamp-shop")),
            ("[project]\nname = \"Lamp Shop\"\n", None),
            ("[project]\nid = \"\"\n", None),
            ("[project]\nid = \"  \"\n", None),
            ("[project]\nid = \"Lamp Shop\"\n", None),
            ("[project]\nid = \"lamp_shop\"\n", None),
            ("[project]\nid = 7\n", None),
            ("[project\n", None),
        ] {
            std::fs::write(&file, text).unwrap();
            assert_eq!(file_id(&file).as_deref(), id, "{text}");
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A scratch main checkout whose common dir is its `.git` folder, its project file `text`.
    fn checkout(name: &str, text: Option<&str>) -> (PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("symphony-project-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).unwrap();
        if let Some(text) = text {
            std::fs::write(root.join(FILE), text).unwrap();
        }
        let common = root.join(".git");
        (root, common)
    }

    #[test]
    fn the_board_guide_is_the_files_inside_the_project_else_the_default() {
        let (root, common) = checkout("guide", None);
        let project = Project {
            place: Place {
                root: root.clone(),
                file: Some(root.join(FILE)),
                common,
            },
            id: "x".into(),
        };
        for (guide, read) in [
            ("\"handbook/BOARD.md\"", "handbook/BOARD.md"),
            ("\"./docs/TASKS.md\"", "docs/TASKS.md"),
            ("\"../BOARD.md\"", GUIDE),
            ("\"docs/../../BOARD.md\"", GUIDE),
            ("\"/elsewhere/BOARD.md\"", GUIDE),
            ("\".\"", GUIDE),
            ("\"\"", GUIDE),
            ("7", GUIDE),
        ] {
            std::fs::write(root.join(FILE), format!("[board]\nguide = {guide}\n")).unwrap();
            assert_eq!(project.guide(), read, "{guide}");
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_board_path_is_the_files_inside_the_project_under_git_the_common_dir_and_none_outside() {
        let (root, common) = checkout("board-path", None);
        assert_eq!(board_path(&common, "slot_locks"), None, "no file");
        for (written, read) in [
            ("\".git/test-slots\"", Some(common.join("test-slots"))),
            ("\"./.git/test-slots/\"", Some(common.join("test-slots"))),
            ("\".git\"", Some(root.join(".git"))),
            ("\"slots\"", Some(root.join("slots"))),
            ("\"./out/./slots/\"", Some(root.join("out/slots"))),
            ("\"../slots\"", None),
            ("\".git/../slots\"", None),
            ("\"/var/slots\"", None),
            ("\".\"", None),
            ("\"\"", None),
            ("3", None),
        ] {
            std::fs::write(
                root.join(FILE),
                format!("[board]\nslot_locks = {written}\n"),
            )
            .unwrap();
            assert_eq!(board_path(&common, "slot_locks"), read, "{written}");
            assert_eq!(
                board_path(&common, "assessments"),
                None,
                "{written}: another key"
            );
        }
        std::fs::write(
            root.join(FILE),
            "[board]\nslot_locks = \"slots\"\nnot toml [",
        )
        .unwrap();
        assert_eq!(
            board_path(&common, "slot_locks"),
            None,
            "a file that does not read"
        );
        // A common dir that is no main checkout's `.git` folder has no main checkout's file.
        assert_eq!(board_path(&root, "slot_locks"), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_artifacts_folder_is_the_files_else_artifacts_and_none_outside_the_project() {
        let root =
            std::env::temp_dir().join(format!("symphony-project-artifacts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(artifacts(&root).as_deref(), Some("artifacts"), "no file");
        for (text, folder) in [
            ("[project]\nid = \"x\"\n", Some("artifacts")),
            ("[board]\nguide = \"G.md\"\n", Some("artifacts")),
            ("[project]\nartifacts = \"evidence\"\n", Some("evidence")),
            (
                "[project]\nartifacts = \"./out/./shots/\"\n",
                Some("out/shots"),
            ),
            ("[project]\nartifacts = \"../elsewhere\"\n", None),
            ("[project]\nartifacts = \"/tmp/shots\"\n", None),
            ("[project]\nartifacts = \"\"\n", None),
            ("[project]\nartifacts = 3\n", None),
            ("[project\n", None),
        ] {
            std::fs::write(root.join(FILE), text).unwrap();
            assert_eq!(artifacts(&root).as_deref(), folder, "{text}");
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_projects_disposable_paths_are_the_files_and_none_where_one_is_not_a_pattern() {
        let root = std::env::temp_dir().join(format!(
            "symphony-project-disposable-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert!(disposable(&root).is_empty(), "no file");
        for (text, patterns) in [
            ("[project]\nid = \"x\"\n", vec![]),
            ("[board]\nguide = \"G.md\"\n", vec![]),
            (
                "[board]\nsweep_disposable = [\"node_modules/\", \"*/node_modules/\", \"*.tsbuildinfo\"]\n",
                vec!["node_modules/", "*/node_modules/", "*.tsbuildinfo"],
            ),
            (
                "[board]\nsweep_disposable = [\"dist/\", \"dist/\"]\n",
                vec!["dist/"],
            ),
            // One that is no pattern, and the project names none: nothing goes on a guess.
            ("[board]\nsweep_disposable = [\"dist/\", \"*\"]\n", vec![]),
            ("[board]\nsweep_disposable = [\"dist/\", 3]\n", vec![]),
            ("[board]\nsweep_disposable = \"dist/\"\n", vec![]),
            ("[board\nsweep_disposable = [\"dist/\"]\n", vec![]),
        ] {
            std::fs::write(root.join(FILE), text).unwrap();
            assert_eq!(disposable(&root), patterns, "{text}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_server_of_its_own_sweeps_only_where_the_project_file_says_so() {
        let root =
            std::env::temp_dir().join(format!("symphony-project-own-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert!(!own_sweep(&root), "no file");
        for (text, sweeps) in [
            ("[board]\nown_sweep = true\n", true),
            ("[board]\nown_sweep = false\n", false),
            ("[board]\nguide = \"G.md\"\n", false),
            // Written as anything but `true`, or in a file that does not read: no sweep on a guess.
            ("[board]\nown_sweep = \"true\"\n", false),
            ("[board]\nown_sweep = 1\n", false),
            ("[agents]\nown_sweep = true\n", false),
            ("[board\nown_sweep = true\n", false),
        ] {
            std::fs::write(root.join(FILE), text).unwrap();
            assert_eq!(own_sweep(&root), sweeps, "{text}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_disposable_pattern_names_a_path_of_its_own_inside_the_project() {
        for pattern in [
            "node_modules/",
            "*/dist/",
            "web/.cache/*",
            "*.tsbuildinfo",
            ".env.local",
        ] {
            assert_eq!(disposable_pattern(pattern), Ok(()), "{pattern}");
        }
        for (pattern, why) in [
            ("", "is empty"),
            ("/dist/", "starts with `/`"),
            ("a\\b", "backslash"),
            ("../dist/", "`.` or `..` part"),
            ("web/./dist/", "`.` or `..` part"),
            ("*", "names nothing of its own"),
            ("*/", "names nothing of its own"),
            ("*/*.*", "names nothing of its own"),
            ("??", "names nothing of its own"),
        ] {
            let said = disposable_pattern(pattern).unwrap_err();
            assert!(said.contains(why), "{pattern}: {said}");
        }
    }

    #[test]
    fn the_documents_are_the_entrypoints_folders_else_docs_and_the_root_when_unreadable() {
        let entry = |folder: &str| format!("[[entrypoints]]\nurl = \"u\"\nfolder = {folder}\n");
        for (text, folders) in [
            ("[project]\nid = \"x\"\n".to_string(), vec!["docs"]),
            (String::new(), vec!["docs"]),
            (entry("\"spec\""), vec!["spec"]),
            (
                entry("\"./design/./notes/\"") + &entry("\"docs\""),
                vec!["design/notes", "docs"],
            ),
            (entry("\".\""), vec![""]),
            // What the Symphony refuses reads as every folder, never one too few.
            (entry("\"../elsewhere\""), vec![""]),
            (entry("\"/abs\""), vec![""]),
            (entry("\"\""), vec![""]),
            (entry("\"\"") + &entry("\"docs\""), vec![""]),
            (entry("3"), vec![""]),
            ("[[entrypoints]]\nurl = \"u\"\n".into(), vec![""]),
            ("entrypoints = []\n".into(), vec![""]),
            ("entrypoints = 3\n".into(), vec![""]),
            ("[project\n".into(), vec![""]),
        ] {
            assert_eq!(documents(&text), folders, "{text}");
        }
        let root =
            std::env::temp_dir().join(format!("symphony-project-documents-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(documents_at(&root), ["docs"], "no file");
        std::fs::write(root.join(FILE), entry("\"spec\"")).unwrap();
        assert_eq!(documents_at(&root), ["spec"]);
        std::fs::remove_file(root.join(FILE)).unwrap();
        std::fs::create_dir(root.join(FILE)).unwrap();
        assert_eq!(documents_at(&root), [""], "a file that does not read");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
