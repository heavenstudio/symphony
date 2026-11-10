//! Where a gate integration-test harness keeps its scratch: one folder of its own under the temp folder,
//! `<temp>/<harness>/<pid>-<n>`. Making a folder and sweeping dead owners' folders list that folder alone,
//! never the temp folder, so their cost is the same whatever else sits there. A reused pid or any
//! uncertainty preserves a folder.

use std::fs;
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

#[path = "scratch.rs"]
mod dead_owners;

#[cfg(test)]
#[path = "scratch_tests.rs"]
mod tests;

/// `<base>/<harness>`, made on first use: a real directory (never a link) this user owns, so another user's
/// folder or a planted link never receives a harness's scratch or its sweep.
pub(crate) fn harness_root(base: &Path, harness: &str) -> io::Result<PathBuf> {
    let root = base.join(harness);
    match fs::DirBuilder::new().mode(0o700).create(&root) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let meta = fs::symlink_metadata(&root)?;
    // SAFETY: geteuid takes no arguments and cannot fail.
    let me = unsafe { libc::geteuid() };
    if !meta.file_type().is_dir() || meta.uid() != me {
        return Err(io::Error::other(format!(
            "{} is not a directory of uid {me}: {:?} of uid {}",
            root.display(),
            meta.file_type(),
            meta.uid()
        )));
    }
    Ok(root)
}

/// A new folder `<root>/<pid>-<n>`, `n` drawn from `next`. A name that exists may belong to an earlier owner
/// of this pid, or to a live one: it is preserved, never emptied, and the next counter is tried.
pub(crate) fn create(root: &Path, next: &AtomicUsize) -> io::Result<PathBuf> {
    loop {
        let n = next.fetch_add(1, Ordering::SeqCst);
        let path = root.join(format!("{}-{n}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
}

/// Drop cannot run after SIGKILL: remove the `<pid>-<n>` directories in the harness's `root` whose owners
/// are known dead, listing `root` alone.
pub(crate) fn reap_dead(root: &Path) {
    dead_owners::reap_dead_scratch(root, "");
}
