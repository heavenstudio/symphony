//! Where a project's build of a Symphony binary is, found as the recipes `symphony init` installs
//! find it (`shipped/symphony.just`): this checkout's `.target/bin/NAME` link, else its main
//! checkout's (from a linked worktree), else `NAME` on `PATH`. The project's build setup makes the
//! links; nothing here builds. The heartbeat and the memory guard run the checkout's
//! `.target/bin/symphony` alone, and the beat the board's binary that hook finds
//! ([`crate::hook`]).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The folder of a checkout's links to its builds.
pub const LINKS: &str = ".target/bin";

/// `root`'s link to its build of `name` when it runs.
pub fn link(root: &Path, name: &str) -> Option<PathBuf> {
    let link = root.join(LINKS).join(name);
    runnable(&link).then_some(link)
}

/// `root`'s link to its build of `name`, else, for a linked worktree, its main checkout's.
pub fn linked(root: &Path, name: &str) -> Option<PathBuf> {
    link(root, name).or_else(|| link(&main_checkout(root)?, name))
}

/// The main checkout of the linked worktree at `root`: its common dir less `/.git`, read from the
/// worktree's own marker with no child process. None for a main checkout or no checkout.
fn main_checkout(root: &Path) -> Option<PathBuf> {
    let common = symphony_board::git::checkout_common_dir(root)?;
    if common.file_name()? != ".git" {
        return None;
    }
    let main = common.parent()?.to_path_buf();
    (root.canonicalize().ok()? != main).then_some(main)
}

/// `name` in the first folder of `path` (a `PATH` value) holding it runnable.
pub fn on_path(name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|folder| !folder.as_os_str().is_empty())
        .map(|folder| folder.join(name))
        .find(|candidate| runnable(candidate))
}

/// The project's build of `name`: its link ([`linked`]), else `name` on `path`.
pub fn find(root: &Path, name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    linked(root, name).or_else(|| on_path(name, path))
}

/// Whether `path` is a file with an execute bit set, through any link.
pub fn runnable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
#[path = "builds_tests.rs"]
mod tests;
