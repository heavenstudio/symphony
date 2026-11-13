//! Commands always discover the explicitly selected checkout and leave its index untouched.

use crate::sys::{Command, Stdio};
use std::path::Path;
use std::process::Output;

/// `git ARGS` in `dir`: none when git cannot be started there (the folder is gone).
pub fn git(dir: &Path, args: &[&str]) -> Option<Output> {
    crate::sys::output(command(dir).args(args)).ok()
}

fn command(dir: &Path) -> Command {
    let mut command = crate::sys::command("git");
    command.current_dir(dir).stdin(Stdio::null());
    // A hook or launcher can inherit the caller's repository and index. Each sweep operation
    // must instead discover the repository belonging to its explicit working directory.
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_PREFIX",
    ] {
        command.env_remove(name);
    }
    command
}

/// What `git ARGS` printed, when it succeeded.
pub(crate) fn stdout(dir: &Path, args: &[&str]) -> Option<String> {
    read(dir, args).map(|done| String::from_utf8_lossy(&done.stdout).into_owned())
}

/// A read's original bytes, including NUL-delimited names, without refreshing the writer's index.
pub(crate) fn read(dir: &Path, args: &[&str]) -> Option<Output> {
    // These reads inspect other agents' trees. A diff can refresh the index even when
    // optional locks are disabled, so both settings are needed to leave the writer's index alone.
    crate::sys::output(
        command(dir)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args(["-c", "diff.autoRefreshIndex=false"])
            .args(args),
    )
    .ok()
    .filter(|done| done.status.success())
}

/// Whether `git ARGS` succeeded.
pub fn succeeds(dir: &Path, args: &[&str]) -> bool {
    git(dir, args).is_some_and(|done| done.status.success())
}

/// The commit `rev` names, if it names one.
pub fn rev(dir: &Path, rev: &str) -> Option<String> {
    stdout(dir, &["rev-parse", "--verify", "--quiet", rev]).map(|out| out.trim().to_string())
}
