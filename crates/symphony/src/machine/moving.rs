//! What a move of a registered project's board to a release's schema shares, whoever moves it: a
//! release's refresh (`crates/symphony-build/src/release/refresh.rs`) or the machine's server
//! trying a held update again ([`super::held`]). The board's store is snapshotted through its
//! running database just before it moves, into `snapshots/before-schema-N-RELEASE.sql` in its
//! folder (`pg_dump`'s text of the board's database, which `psql` restores), the newest
//! [`KEPT_SNAPSHOTS`] kept; and the release's own `tasks migrate` runs in the project's main
//! checkout without the variables [`UNSET`] names.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The snapshots a project's board keeps of its store before a schema move, newest first.
pub const KEPT_SNAPSHOTS: usize = 5;

/// The variables no command run in a project keeps, beside the repository git was pointed at
/// (`symphony_board::sys::GIT_VARS`: each command names its own folder): the board, session and
/// task a run of the Symphony's names (a project's command reaches its own), the landing's marker,
/// and the build folder and toolchain, which are the project's own.
pub const UNSET: [&str; 9] = [
    "TASKS_SERVER",
    "TASKS_DATABASE",
    "TASKS_SESSION",
    "TASKS_TASK",
    "TASKS_RUN_PARENT",
    "TASKS_WORK",
    "SYMPHONY_LAND",
    "CARGO_TARGET_DIR",
    "RUSTUP_TOOLCHAIN",
];

/// Where the snapshot of the board in `folder` (its board folder, in its git common dir) taken
/// before its move to `target` by `release` goes.
pub fn snapshot_file(folder: &Path, target: i64, release: &str) -> PathBuf {
    folder
        .join("snapshots")
        .join(format!("before-schema-{target}-{release}.sql"))
}

/// Remove from `folder` the snapshots but the newest `kept`; nothing else there is touched.
pub fn prune(folder: &Path, kept: usize) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let mut found: Vec<(SystemTime, PathBuf)> = (entries.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("before-schema-"))
        })
        .filter_map(|path| Some((fs::metadata(&path).ok()?.modified().ok()?, path)))
        .collect();
    found.sort();
    let gone = found.len().saturating_sub(kept);
    for (_, path) in found.into_iter().take(gone) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
#[path = "moving_tests.rs"]
mod tests;
