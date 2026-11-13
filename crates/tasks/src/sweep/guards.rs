//! Definitely blocked candidates need no filesystem verdict. These checks are repeated each pass;
//! the acquired removal guard stays with the caller through every later deletion fence.

use super::{coalesce, git};
use crate::sys;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// Retain an existing HEAD writer, otherwise hold the nonblocking removal lock. Dry runs only
/// inspect the writer entry and never create or acquire a lock file.
pub(super) fn acquire(path: &Path, dry_run: bool) -> Result<Option<sys::FileLock>, String> {
    let admin = git::admin(path)
        .ok_or_else(|| format!("cannot locate reference locks for {}", path.display()))?;
    let head = admin.join("HEAD.lock");
    match fs::symlink_metadata(&head) {
        Ok(_) => return Err(format!("HEAD writer lock exists: {}", head.display())),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect {}: {error}", head.display())),
    }
    if dry_run {
        return Ok(None);
    }
    coalesce::tree_lock(path).map(Some).ok_or_else(|| {
        format!(
            "cannot take worktree removal lock {}",
            admin.join("symphony-sweep-remove.lock").display()
        )
    })
}
