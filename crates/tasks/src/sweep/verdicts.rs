//! Reuse file verdicts on unchanged worktrees. Live board, lock, age and process decisions are
//! always made again, and a removal always re-reads status with no cached evidence.

use super::git;
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static WRITES: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Stamp {
    device: u64,
    inode: u64,
    seconds: i64,
    nanos: i64,
    length: u64,
}

fn from_metadata(meta: fs::Metadata) -> Stamp {
    Stamp {
        device: meta.dev(),
        inode: meta.ino(),
        seconds: meta.mtime(),
        nanos: meta.mtime_nsec(),
        length: meta.len(),
    }
}

fn stamp(path: &Path) -> Option<Stamp> {
    fs::metadata(path).ok().map(from_metadata)
}

fn entry_stamp(path: &Path) -> Option<Stamp> {
    fs::symlink_metadata(path).ok().map(from_metadata)
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Inputs {
    head: String,
    branch: Option<String>,
    finished: bool,
    folder: Stamp,
    index: Stamp,
    head_file: Stamp,
}

impl Inputs {
    fn read(entry: &git::Listed, finished: bool) -> Option<Self> {
        let admin = git::admin(&entry.path)?;
        Some(Self {
            head: entry.head.clone(),
            branch: entry.branch.clone(),
            finished,
            folder: stamp(&entry.path)?,
            index: stamp(&admin.join("index"))?,
            head_file: stamp(&admin.join("HEAD"))?,
        })
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Verdict {
    pub owner: Vec<String>,
    pub left: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    inputs: Inputs,
    verdict: Verdict,
    witnesses: Vec<(PathBuf, Stamp)>,
}

/// A kept directory needs one remaining file as evidence. Walking happens only on a fresh
/// verdict; a cache hit stats that witness, so deleting the last nested artifact invalidates it.
fn leaf(path: &Path) -> Option<PathBuf> {
    if !fs::symlink_metadata(path).ok()?.is_dir() {
        return Some(path.to_path_buf());
    }
    let mut entries: Vec<_> = fs::read_dir(path)
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    entries.sort_by_key(|entry| entry.file_name());
    entries.into_iter().find_map(|entry| leaf(&entry.path()))
}

fn witnesses(root: &Path, verdict: &Verdict) -> Option<Vec<(PathBuf, Stamp)>> {
    let mut paths = std::collections::BTreeMap::new();
    for name in verdict.left.iter().chain(&verdict.owner) {
        let path = root.join(name);
        paths.insert(path.clone(), entry_stamp(&path)?);
        let file = leaf(&path)?;
        paths.insert(file.clone(), entry_stamp(&file)?);
    }
    Some(paths.into_iter().collect())
}

pub(super) struct Verdicts {
    dry_run: bool,
}

impl Verdicts {
    pub fn new(dry_run: bool) -> Self {
        Self { dry_run }
    }

    pub fn read(&self, entry: &git::Listed, finished: bool) -> Verdict {
        let inputs = Inputs::read(entry, finished);
        let path = git::admin(&entry.path).map(|dir| dir.join("sf-sweep-verdict.json"));
        if let (Some(inputs), Some(path)) = (&inputs, &path)
            && let Some(saved) = fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
            && saved.inputs == *inputs
            && saved
                .witnesses
                .iter()
                .all(|(path, was)| entry_stamp(path).as_ref() == Some(was))
        {
            return saved.verdict;
        }
        let left = git::left_behind(&entry.path, finished);
        let owner = if left
            .iter()
            .any(|name| name == "docs" || name.contains("docs/"))
        {
            git::owner_lines_left(&entry.path)
        } else {
            Vec::new()
        };
        let verdict = Verdict { owner, left };
        if !self.dry_run
            && !verdict.left.iter().any(|s| s.starts_with('('))
            && let (Some(inputs), Some(path)) = (inputs, path)
            && Inputs::read(entry, finished).as_ref() == Some(&inputs)
            && let Some(witnesses) = witnesses(&entry.path, &verdict)
        {
            let temp = path.with_extension(format!(
                "{}-{}.tmp",
                std::process::id(),
                WRITES.fetch_add(1, Ordering::Relaxed)
            ));
            if let Ok(bytes) = serde_json::to_vec(&Saved {
                inputs,
                verdict: verdict.clone(),
                witnesses,
            }) {
                if fs::write(&temp, bytes).is_ok() {
                    let _ = fs::rename(&temp, &path);
                }
                let _ = fs::remove_file(temp);
            }
        }
        verdict
    }
}
