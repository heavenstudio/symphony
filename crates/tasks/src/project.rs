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
    /// The repository's common dir, which holds its board (`symphony/tasks/`).
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

/// The board's guide of a project whose file names none, project-relative.
pub const GUIDE: &str = "TASKS.md";

impl Project {
    /// The project's board guide, project-relative, which says where its board lives and how it
    /// moves: its file's `[board] guide`, else [`GUIDE`]. A file that does not read names none.
    pub fn guide(&self) -> String {
        self.place
            .file
            .as_deref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| text.parse::<toml::Table>().ok())
            .and_then(|file| {
                file.get("board")?
                    .get("guide")?
                    .as_str()
                    .filter(|guide| !guide.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| GUIDE.to_string())
    }
}

/// Where the path `[board] KEY` names in the project file of the main checkout of the repository
/// whose common dir is `common` is: one under `.git/` in the common dir, any other under the main
/// checkout. None when there is no main checkout, no file, it does not read, or it names no path
/// there: the project has none of that kind, and the board reads none (`[board] slot_locks`,
/// `[board] assessments`; the Symphony's `project::Board` checks them). Only that key is read; the
/// rest of the file is the Symphony's.
pub fn board_path(common: &Path, key: &str) -> Option<PathBuf> {
    let root = main_checkout(common)?;
    let text = std::fs::read_to_string(root.join(FILE)).ok()?;
    let file = text.parse::<toml::Table>().ok()?;
    let path = file
        .get("board")?
        .get(key)?
        .as_str()
        .filter(|path| !path.is_empty())?;
    Some(match path.strip_prefix(".git/") {
        Some(rest) => common.join(rest),
        None => root.join(path),
    })
}

/// The project's artifacts folder, project-relative, as the project file at `root` names it
/// (`[project] artifacts`, else `artifacts`) and the Symphony reads it (`Project::artifact`): its
/// `.` parts dropped, its parts joined by `/`. None when the file does not read, or names a
/// folder that is empty or leaves the project, which the Symphony refuses.
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
    let mut parts = Vec::new();
    for part in Path::new(&named).components() {
        match part {
            std::path::Component::Normal(name) => parts.push(name.to_str()?.to_string()),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The convention's documents folder, read when a project file names no `[[entrypoints]]`.
const DOCS: &str = "docs";

/// The folders the Symphony reads a project's documents from, project-relative ("" the project's
/// root), as the project file `text` names them in `[[entrypoints]] folder` and the Symphony reads
/// them (`Project::pathspecs`): `docs` when it names none, the convention's. Where an owner's line on
/// a card can be written. A file that does not read, or a folder outside the project (which the
/// Symphony refuses), reads as the project's root: every folder, never one too few.
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
        let mut parts = Vec::new();
        for part in Path::new(folder).components() {
            match part {
                std::path::Component::Normal(name) => match name.to_str() {
                    Some(name) => parts.push(name),
                    None => return root(),
                },
                std::path::Component::CurDir => {}
                _ => return root(),
            }
        }
        folders.push(parts.join("/"));
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
