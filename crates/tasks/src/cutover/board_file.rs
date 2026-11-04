//! Which file the SQLite board is: `<project id>-tasks.db` in a repository's git common dir, and
//! whether a board a run is handed is a repository's own.

use crate::git;
use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

/// The project file of a repository's main checkout, beside its git common dir.
const PROJECT_FILE: &str = "symphony.toml";

/// The SQLite board's file in the git common dir `common`: `<project id>-tasks.db`, by the id the
/// main checkout's project file names (`[project] id`). `None` for a project that names none, or
/// an id that is not a slug, which has no SQLite board.
pub fn board_name(common: &Path) -> Option<String> {
    project_id(common)
        .ok()
        .flatten()
        .map(|id| format!("{id}-tasks.db"))
}

/// The SQLite board in the git common dir `common`, by [`board_name`].
pub(super) fn board_in(common: &Path) -> Result<PathBuf, String> {
    let file = common.parent().unwrap_or(common).join(PROJECT_FILE);
    match project_id(common) {
        Ok(Some(id)) => Ok(common.join(format!("{id}-tasks.db"))),
        Ok(None) => Err(format!(
            "{} names no [project] id, so this repository has no SQLite board",
            file.display()
        )),
        Err(id) => Err(format!(
            "{} names [project] id {id:?}, which is not a slug (lowercase letters, digits and dashes), so it names no SQLite board",
            file.display()
        )),
    }
}

/// The `[project] id` of the project file beside the git common dir `common`: `None` when it names
/// none (or an empty one), the id as the error when it is not a slug, as the project file refuses it.
fn project_id(common: &Path) -> Result<Option<String>, String> {
    let Some(text) = common
        .parent()
        .and_then(|main| fs::read_to_string(main.join(PROJECT_FILE)).ok())
    else {
        return Ok(None);
    };
    let Ok(file) = text.parse::<toml::Table>() else {
        return Ok(None);
    };
    match file
        .get("project")
        .and_then(|project| project.get("id"))
        .and_then(toml::Value::as_str)
    {
        None | Some("") => Ok(None),
        Some(id) if is_slug(id) => Ok(Some(id.to_string())),
        Some(id) => Err(id.to_string()),
    }
}

/// Whether `id` is a slug: lowercase letters and digits, single dashes between them.
fn is_slug(id: &str) -> bool {
    id.split('-').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    })
}

/// The repository's board `board` is, at its place in the git common dir: the [`board_name`] of a
/// repository's common dir, whatever path or link names it; or the board of the repository the run
/// starts in, `here`, by another name of the same file. None for any other board.
pub(super) fn repository_board(board: &Path, here: Option<&Path>) -> Option<PathBuf> {
    if let Ok(found) = fs::canonicalize(board)
        && let Some(folder) = found.parent()
        && board_name(folder)
            .is_some_and(|name| found.file_name() == Some(std::ffi::OsStr::new(&name)))
        && git::common_dir(folder).is_ok_and(|common| common == folder)
    {
        return Some(found);
    }
    here.filter(|here| same_file(board, here))
        .map(Path::to_path_buf)
}

/// Whether `a` and `b` are one file, by its device and inode, whatever paths or links name it.
pub(super) fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(a), Ok(b)) => (a.dev(), a.ino()) == (b.dev(), b.ino()),
        _ => false,
    }
}
