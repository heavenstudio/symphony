//! Reuse positive retention evidence only. An empty verdict always rescans, so new nested files
//! receive a fresh reason without walking the whole checkout to prove that nothing changed.
//! Live board, lock, age, process and final removal status decisions are always made again.

use super::{
    git,
    verdict_inputs::{Policy, PolicyInputs, Stamp, entry_stamp, stamp},
};
use serde::{Deserialize, Serialize};
use std::cell::OnceCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

static WRITES: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Inputs {
    head: String,
    branch: Option<String>,
    finished: bool,
    folder: Stamp,
    index: Stamp,
    head_file: Stamp,
    config: String,
}

impl Inputs {
    fn read(entry: &git::Listed, finished: bool, policy: &Policy) -> Option<Self> {
        let admin = git::admin(&entry.path)?;
        Some(Self {
            head: entry.head.clone(),
            branch: entry.branch.clone(),
            finished,
            folder: stamp(&entry.path)?,
            index: stamp(&admin.join("index"))?,
            head_file: stamp(&admin.join("HEAD"))?,
            config: policy.config.clone(),
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
    policy: PolicyInputs,
}

/// A kept directory needs one remaining file as evidence. Walking happens only on a fresh
/// verdict; a cache hit stats that witness, so deleting the last nested artifact invalidates it.
fn leaf(path: &Path) -> Option<PathBuf> {
    if !fs::symlink_metadata(path).ok()?.is_dir() {
        return Some(path.to_path_buf());
    }
    if path.join(".git").exists() {
        return None;
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
    let names = verdict.left.iter().chain(&verdict.owner);
    for name in names {
        let path = root.join(name);
        paths.insert(path.clone(), entry_stamp(&path)?);
        let file = leaf(&path)?;
        paths.insert(file.clone(), entry_stamp(&file)?);
        let parents = file.strip_prefix(root).ok()?.parent()?.ancestors();
        for parent in parents {
            let path = root.join(parent);
            paths.insert(path.clone(), entry_stamp(&path)?);
        }
    }
    Some(paths.into_iter().collect())
}

pub(super) struct Verdicts {
    dry_run: bool,
    root: PathBuf,
    policy: OnceCell<Option<Policy>>,
}

impl Verdicts {
    pub fn new(root: &Path, dry_run: bool) -> Self {
        Self {
            dry_run,
            root: root.into(),
            policy: OnceCell::new(),
        }
    }

    pub fn read(&self, entry: &git::Listed, finished: bool) -> Verdict {
        let began = SystemTime::now();
        let policy = self
            .policy
            .get_or_init(|| Policy::read(&self.root))
            .as_ref();
        let inputs = policy.and_then(|policy| Inputs::read(entry, finished, policy));
        let path = git::admin(&entry.path).map(|dir| dir.join("sf-sweep-verdict.json"));
        if let (Some(inputs), Some(path)) = (&inputs, &path)
            && let Some(saved) = fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
            && !saved.verdict.left.is_empty()
            && saved.inputs == *inputs
            && policy
                .and_then(|policy| policy.inputs(&entry.path, &saved.witnesses))
                .as_ref()
                == Some(&saved.policy)
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
            && let Some(policy) = policy
            && Inputs::read(entry, finished, policy).as_ref() == Some(&inputs)
            && let Some(witnesses) = witnesses(&entry.path, &verdict)
            && witnesses.iter().all(|(_, stamp)| stamp.predates(began))
            && let Some(policy) = policy.inputs(&entry.path, &witnesses)
            && policy.values().flatten().all(|stamp| stamp.predates(began))
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
                policy,
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
