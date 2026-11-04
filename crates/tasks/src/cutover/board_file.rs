//! Which file the SQLite board is: `<project id>-tasks.db` in a repository's git common dir, and
//! whether a board a run is handed is a repository's own.

use crate::git;
use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

/// The project file of a repository's main checkout, beside its git common dir.
const PROJECT_FILE: &str = "symphony.toml";

/// The SQLite board's file in the git common dir `common`: `<project id>-tasks.db`, by the id the
/// main checkout's project file names (`[project] id`). `None` for a project that names none,
/// which has no SQLite board.
pub fn board_name(common: &Path) -> Option<String> {
    let text = fs::read_to_string(common.parent()?.join(PROJECT_FILE)).ok()?;
    let file: toml::Table = text.parse().ok()?;
    let id = file.get("project")?.get("id")?.as_str()?;
    (!id.is_empty()).then(|| format!("{id}-tasks.db"))
}

/// The SQLite board in the git common dir `common`, by [`board_name`].
pub(super) fn board_in(common: &Path) -> Result<PathBuf, String> {
    board_name(common)
        .map(|name| common.join(name))
        .ok_or_else(|| {
            let file = common.parent().unwrap_or(common).join(PROJECT_FILE);
            format!(
                "{} names no [project] id, so this repository has no SQLite board",
                file.display()
            )
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
