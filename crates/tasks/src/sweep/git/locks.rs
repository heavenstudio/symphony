//! Exact-file locks protect references and dead worktree locks through final removal.

use super::{git, stdout};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A Git writer's lock, held until the final removal has finished. Never unlink a replacement
/// created after Git removed the original admin directory and another worktree reused its name.
pub(in crate::sweep) struct RefLock {
    path: PathBuf,
    file: fs::File,
}

impl RefLock {
    fn take(dir: &Path, reference: &str) -> Option<Self> {
        let path = stdout(
            dir,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                reference,
            ],
        )?;
        let mut path = PathBuf::from(path.trim()).into_os_string();
        path.push(".lock");
        let path = PathBuf::from(path);
        if reference != "HEAD" {
            fs::create_dir_all(path.parent()?).ok()?;
        }
        Self::create(path)
    }

    fn create(path: PathBuf) -> Option<Self> {
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .ok()?;
        Some(Self { path, file })
    }
}

/// A claimed worktree lock left for recovery prevents removal until recovered.
pub(in crate::sweep) const SAVED_LOCK: &str = ".symphony-sweep-lock-";
static LOCK_NAMES: AtomicU64 = AtomicU64::new(0);

fn lock_path(dir: &Path) -> Option<PathBuf> {
    stdout(dir, &["rev-parse", "--path-format=absolute", "--git-dir"])
        .map(|p| PathBuf::from(p.trim()).join("locked"))
}

/// The exact lock that was classified dead. Keeping its descriptor open prevents inode reuse.
pub(in crate::sweep) struct WorktreeLock {
    path: PathBuf,
    _file: fs::File,
    bytes: Vec<u8>,
    metadata: fs::Metadata,
    parent: fs::Metadata,
}

fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}

impl WorktreeLock {
    fn read(path: PathBuf) -> Option<Self> {
        let entry = fs::symlink_metadata(&path).ok()?;
        if !entry.is_file() {
            return None;
        }
        let mut file = fs::File::open(&path).ok()?;
        let before = file.metadata().ok()?;
        if !same_file(&entry, &before) {
            return None;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        let after = file.metadata().ok()?;
        if before.modified().ok()? != after.modified().ok()? || before.len() != after.len() {
            return None;
        }
        let parent = fs::metadata(path.parent()?).ok()?;
        Some(Self {
            path,
            _file: file,
            bytes,
            metadata: after,
            parent,
        })
    }

    pub(in crate::sweep) fn capture(dir: &Path, reason: &str) -> Option<Self> {
        let lock = Self::read(lock_path(dir)?)?;
        (std::str::from_utf8(&lock.bytes).ok()?.trim() == reason).then_some(lock)
    }

    fn matches(&self, other: &Self) -> bool {
        same_file(&self.metadata, &other.metadata)
            && self.metadata.modified().ok() == other.metadata.modified().ok()
            && self.bytes == other.bytes
    }

    /// Claim the directory entry before unlinking anything. A replacement between the validation
    /// and rename is moved back with a non-overwriting hard link. If another replacement already
    /// occupies the original path, retain both files; saved-lock detection keeps that tree later.
    pub(in crate::sweep) fn release(self, pause: impl Fn(&str)) -> Option<ReleasedLock> {
        let current = Self::read(self.path.clone())?;
        if !self.matches(&current) {
            return None;
        }
        let saved = self.path.parent()?.join(format!(
            "{SAVED_LOCK}{}-{}",
            std::process::id(),
            LOCK_NAMES.fetch_add(1, Ordering::Relaxed)
        ));
        let reserved = RefLock::create(saved.clone())?;
        pause("unlock");
        fs::rename(&self.path, &saved).ok()?;
        // reserved's descriptor still names the overwritten placeholder, so its Drop cannot
        // unlink the lock now stored at saved, even on any of the error paths below.
        drop(reserved);
        pause("lock-claimed");
        if !Self::read(saved.clone()).is_some_and(|claimed| self.matches(&claimed)) {
            if fs::hard_link(&saved, &self.path).is_ok() {
                let _ = fs::remove_file(&saved);
            }
            return None;
        }
        fs::remove_file(&saved).ok()?;
        Some(ReleasedLock {
            path: self.path,
            bytes: self.bytes,
            parent: self.parent,
        })
    }
}

/// Roll back a refused removal without ever overwriting a replacement lock or a reused admin dir.
pub(in crate::sweep) struct ReleasedLock {
    path: PathBuf,
    bytes: Vec<u8>,
    parent: fs::Metadata,
}

impl Drop for ReleasedLock {
    fn drop(&mut self) {
        let Some(parent) = self.path.parent() else {
            return;
        };
        if fs::metadata(parent).is_ok_and(|now| same_file(&self.parent, &now))
            && let Ok(mut file) = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&self.path)
        {
            let _ = file.write_all(&self.bytes);
        }
    }
}

pub(super) fn saved_locks(dir: &Path) -> Option<Vec<String>> {
    let lock = lock_path(dir)?;
    let mut saved = Vec::new();
    for entry in fs::read_dir(lock.parent()?).ok()? {
        let entry = entry.ok()?;
        if entry.file_name().to_string_lossy().starts_with(SAVED_LOCK) {
            saved.push(format!(
                "(saved worktree lock needs recovery: {})",
                entry.path().display()
            ));
        }
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "a busy board path keeps std's in-place sort (crate::order): the sweep"
    )]
    saved.sort_unstable();
    Some(saved)
}

impl Drop for RefLock {
    fn drop(&mut self) {
        if let (Ok(held), Ok(current)) = (self.file.metadata(), fs::symlink_metadata(&self.path))
            && held.dev() == current.dev()
            && held.ino() == current.ino()
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Freeze HEAD and its symbolic target through deletion, using Git's O_EXCL lockfile protocol
/// (https://git-scm.com/docs/api-lockfile). Readers still work. A writer already holding either
/// lock keeps the tree. Unknown reference backends cannot be guarded with file locks.
pub(in crate::sweep) fn hold_head(dir: &Path) -> Option<Vec<RefLock>> {
    if stdout(
        dir,
        &[
            "config",
            "--local",
            "--default",
            "files",
            "--get",
            "extensions.refStorage",
        ],
    )?
    .trim()
        != "files"
    {
        return None;
    }
    let mut held = Vec::new();
    let mut seen = Vec::new();
    let mut reference = "HEAD".to_string();
    loop {
        if seen.contains(&reference) {
            return None;
        }
        seen.push(reference.clone());
        held.push(RefLock::take(dir, &reference)?);
        let next = git(
            dir,
            &["symbolic-ref", "--quiet", "--no-recurse", &reference],
        )?;
        match next.status.code() {
            Some(0) => {
                reference = String::from_utf8(next.stdout).ok()?.trim().to_string();
                if reference.is_empty() {
                    return None;
                }
            }
            Some(1) => return Some(held),
            _ => return None,
        }
    }
}
