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
use std::path::{Path, PathBuf};

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
    /// The repository's common dir, which holds its board (`sf-tasks/`).
    pub common: PathBuf,
}

/// The project a command runs in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub place: Place,
    /// What it is named by: its file's `[project] id`, else the registry's id for its main checkout,
    /// else the main checkout's folder name.
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

/// The `[project] id` the file at `path` names, if it names one.
fn file_id(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let file = text.parse::<toml::Table>().ok()?;
    file.get("project")?
        .get("id")?
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
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
    let text = std::fs::read_to_string(machine.join(REGISTRY)).ok()?;
    let registry: Registry = serde_json::from_str(&text).ok()?;
    let main = main_checkout(common);
    registry
        .projects
        .iter()
        .find(|entry| Some(entry.root.as_path()) == main)
        .or_else(|| {
            registry
                .projects
                .iter()
                .find(|entry| git::common_dir(&entry.root).is_ok_and(|theirs| theirs == common))
        })
        .map(|entry| entry.id.clone())
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
