//! What a scratch owner's parent runs before it waits, and the stages an owner reports while
//! that parent waits for it to make its whole repository (`scratch_tests`).

use super::{machine_copy, prepare_app, script_source};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The pid of the app's preparation run by a scratch owner's parent ([`prepare_images`]), which
/// the owner takes as its own, so the parent's wait holds no run of the image at all.
pub(super) const PREPARED_APP_BY_PARENT: &str = "SYMPHONY_LIFECYCLE_PREPARED_APP";

/// When this process began as a scratch owner; set by the owner's entry alone.
pub(super) static OWNER: OnceLock<Instant> = OnceLock::new();

/// The parent's app preparation, in a scratch owner alone: anywhere else an exported value is
/// not taken, and the process runs and checks the image itself.
pub(super) fn parents_app() -> Option<u32> {
    OWNER.get()?;
    let pid = std::env::var(PREPARED_APP_BY_PARENT).ok()?;
    Some(
        pid.parse()
            .unwrap_or_else(|_| panic!("{PREPARED_APP_BY_PARENT} is no pid: {pid:?}")),
    )
}

/// In a scratch owner, the end of one stage of what its parent waits for, on stderr, which a
/// readiness timeout reports with the rest of the owner's output.
pub(super) fn stage(stage: &str) {
    if let Some(began) = OWNER.get() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        eprintln!(
            "· scratch owner {stage} at {} ms since Unix epoch, {} ms in",
            now.as_millis(),
            began.elapsed().as_millis()
        );
    }
}

/// The tracked scripts whose machine copies a repository's commands link.
pub(super) const COMMAND_SCRIPTS: [&str; 4] = ["build.sh", "open.sh", "ps.sh", "bash.sh"];

/// Every new image a fixture repository runs, each run once by this process.
pub(super) struct Images {
    /// The pid of the built app's preparation run ([`prepare_app`]).
    pub(super) app: u32,
    /// The machine copy of each of [`COMMAND_SCRIPTS`], in that order.
    pub(super) copies: [PathBuf; 4],
}

/// Run every new image a fixture repository runs, as setup: the built app and each command
/// script's machine copy, under no case deadline. A process that makes its own repository while
/// this one waits on it then meets no file's first run, which macOS assesses
/// before its first instruction, one file at a time machine-wide; given
/// [`PREPARED_APP_BY_PARENT`], it runs no app image at all.
pub(super) fn prepare_images(folder: &Path) -> Images {
    Images {
        app: prepare_app(&folder.join("prepare-app")),
        copies: COMMAND_SCRIPTS.map(|name| machine_copy::machine_copy(&script_source(name))),
    }
}
