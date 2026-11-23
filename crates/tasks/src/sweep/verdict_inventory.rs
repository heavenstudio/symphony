//! A complete, bounded metadata inventory proves an unchanged empty verdict. Every relevant
//! directory witnesses new/deleted/renamed paths; every tracked file witnesses content and mode.
//! Ignored cuts come from the same native status as the verdict, and never hide tracked paths.

use super::{
    git,
    verdict_inputs::{Stamp, entry_stamp},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

// The current source footprint is 3,547 indexed files plus 387 directories. This bounds proof
// size at about twice that footprint; overflow falls back, without raising any runner budget.
const MAX_ENTRIES: usize = 8192;
const MAX_LIST_BYTES: usize = MAX_ENTRIES * 1024;
const VERSION: u32 = 1;

/// A changed collection needs a current verdict; unsupported inputs cannot prove reuse.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Unproven {
    Unknown,
    Changed,
}

/// Complete relative-path metadata under the supported native index and output policy.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Inventory {
    version: u32,
    entries: Vec<(PathBuf, Stamp)>,
}

impl Inventory {
    /// Capture only metadata older than the fresh-status window, then validate it again.
    pub fn collect(
        root: &Path,
        ignored_dirs: &[String],
        began: SystemTime,
    ) -> Result<Self, Unproven> {
        admin_safe(root)?;
        let output = git::read(root, &["ls-files", "--stage", "-z"]).ok_or(Unproven::Unknown)?;
        let tracked = tracked_paths(&output.stdout)?;
        let mut ignored = BTreeSet::new();
        for directory in ignored_dirs {
            let path = PathBuf::from(directory.strip_suffix('/').ok_or(Unproven::Unknown)?);
            if !relative(&path) || path.as_os_str().is_empty() {
                return Err(Unproven::Unknown);
            }
            ignored.insert(path);
        }
        let mut capture = Capture {
            root,
            began,
            ignored,
            entries: BTreeMap::new(),
        };
        capture.walk(Path::new(""))?;
        for path in tracked {
            capture.tracked(&path)?;
        }
        let inventory = Self {
            version: VERSION,
            entries: capture.entries.into_iter().collect(),
        };
        if !inventory.matches(root) {
            return Err(Unproven::Changed);
        }
        Ok(inventory)
    }

    /// Every captured directory supplies its own ignore/attribute inputs to the policy proof.
    pub fn directories(&self) -> impl Iterator<Item = &Path> {
        self.entries
            .iter()
            .filter(|(_, value)| value.is_directory())
            .map(|(path, _)| path.as_path())
    }

    /// Validate the saved inputs without recursively enumerating the checkout.
    pub fn matches(&self, root: &Path) -> bool {
        self.version == VERSION
            && self.entries.len() <= MAX_ENTRIES
            && self
                .entries
                .first()
                .is_some_and(|(path, value)| path.as_os_str().is_empty() && value.is_directory())
            && admin_safe(root).is_ok()
            && self.entries.iter().all(|(path, value)| {
                relative(path) && entry_stamp(&root.join(path)).as_ref() == Some(value)
            })
    }
}

fn relative(path: &Path) -> bool {
    !path.is_absolute()
        && path.to_str().is_some()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// Split-index entries live outside the captured index. Decline reuse while any shared-index
/// file exists instead of treating the index pointer's metadata as that external file's identity.
pub(super) fn admin_safe(root: &Path) -> Result<(), Unproven> {
    let admin = git::admin(root).ok_or(Unproven::Unknown)?;
    let entries = fs::read_dir(admin).map_err(|_| Unproven::Unknown)?;
    for entry in entries {
        let entry = entry.map_err(|_| Unproven::Unknown)?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(git::SAVED_LOCK) {
            return Err(Unproven::Changed);
        }
        if name.starts_with("sharedindex.") {
            return Err(Unproven::Unknown);
        }
    }
    Ok(())
}

fn tracked_paths(bytes: &[u8]) -> Result<Vec<PathBuf>, Unproven> {
    if bytes.len() > MAX_LIST_BYTES || (!bytes.is_empty() && !bytes.ends_with(&[0])) {
        return Err(Unproven::Unknown);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Unproven::Unknown)?;
    let mut paths = BTreeSet::new();
    for entry in text.split_terminator('\0') {
        let (header, name) = entry.split_once('\t').ok_or(Unproven::Unknown)?;
        let parts: Vec<_> = header.split_ascii_whitespace().collect();
        if parts.len() != 3
            || !matches!(parts[0], "100644" | "100755" | "120000")
            || parts[2] != "0"
            || !matches!(parts[1].len(), 40 | 64)
            || !parts[1].bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(Unproven::Unknown);
        }
        let path = PathBuf::from(name);
        if name.is_empty() || !relative(&path) || !paths.insert(path) || paths.len() > MAX_ENTRIES {
            return Err(Unproven::Unknown);
        }
    }
    Ok(paths.into_iter().collect())
}

struct Capture<'root> {
    root: &'root Path,
    began: SystemTime,
    ignored: BTreeSet<PathBuf>,
    entries: BTreeMap<PathBuf, Stamp>,
}

impl Capture<'_> {
    fn metadata(&self, path: &Path) -> Result<fs::Metadata, Unproven> {
        fs::symlink_metadata(self.root.join(path)).map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                Unproven::Changed
            } else {
                Unproven::Unknown
            }
        })
    }

    fn record(&mut self, path: &Path, meta: fs::Metadata) -> Result<(), Unproven> {
        if !relative(path) || self.entries.len() >= MAX_ENTRIES && !self.entries.contains_key(path)
        {
            return Err(Unproven::Unknown);
        }
        let value = Stamp::of(meta);
        if !value.predates(self.began) || self.entries.get(path).is_some_and(|old| *old != value) {
            return Err(Unproven::Changed);
        }
        self.entries.insert(path.to_path_buf(), value);
        Ok(())
    }

    fn walk(&mut self, path: &Path) -> Result<(), Unproven> {
        let meta = self.metadata(path)?;
        let directory = meta.is_dir();
        let name = path.to_str().ok_or(Unproven::Unknown)?;
        let named = if directory {
            format!("{name}/")
        } else {
            name.into()
        };
        if !name.is_empty()
            && (git::regenerable(&named)
                || git::os_metadata(&named)
                || directory && self.ignored.contains(path))
        {
            return Ok(());
        }
        if path.file_name().is_some_and(|name| name == ".git") {
            if path != Path::new(".git") || directory {
                return Err(Unproven::Unknown);
            }
            return self.record(path, meta);
        }
        if !directory && !meta.is_file() && !meta.file_type().is_symlink() {
            return Err(Unproven::Unknown);
        }
        self.record(path, meta)?;
        if directory {
            let mut names = Vec::new();
            let entries = fs::read_dir(self.root.join(path)).map_err(|_| Unproven::Unknown)?;
            for entry in entries {
                names.push(entry.map_err(|_| Unproven::Unknown)?.file_name());
                if names.len() > MAX_ENTRIES {
                    return Err(Unproven::Unknown);
                }
            }
            names.sort_unstable();
            for name in names {
                self.walk(&path.join(name))?;
            }
        }
        Ok(())
    }

    fn tracked(&mut self, path: &Path) -> Result<(), Unproven> {
        let mut parents: Vec<_> = path.ancestors().skip(1).collect();
        parents.reverse();
        for parent in parents {
            let meta = self.metadata(parent)?;
            if !meta.is_dir() {
                return Err(Unproven::Changed);
            }
            self.record(parent, meta)?;
        }
        let meta = self.metadata(path)?;
        if !meta.is_file() && !meta.file_type().is_symlink() {
            return Err(Unproven::Changed);
        }
        self.record(path, meta)
    }
}

#[cfg(test)]
#[path = "verdict_inventory_tests.rs"]
mod tests;
