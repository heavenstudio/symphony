//! Reuse positive retention witnesses or a complete clean-input inventory. Unproven inputs
//! rescan, and collection races get one fresh retry so new blockers have current reasons.
//! Live board, lock, age, process and final removal status decisions are always made again.

use super::{
    git,
    verdict_inputs::{Policy, PolicyInputs, Stamp, entry_stamp, stamp},
    verdict_inventory::{Inventory, admin_safe},
};
use serde::{Deserialize, Serialize};
use std::cell::OnceCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

static WRITES: AtomicU64 = AtomicU64::new(0);

#[derive(PartialEq)]
enum CleanProof {
    Collect,
    Skip,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Inputs {
    head: String,
    branch: Option<String>,
    finished: bool,
    folder: Stamp,
    index: Stamp,
    head_file: Stamp,
    commondir: Option<Stamp>,
    config: String,
    /// The project's artifacts folder the verdict was read with.
    #[serde(default)]
    artifacts: Option<String>,
    /// The project's document folders the owner's answers were read in.
    #[serde(default)]
    documents: Vec<String>,
}

impl Inputs {
    fn read(
        entry: &git::Listed,
        finished: bool,
        artifacts: Option<&str>,
        documents: &[String],
        policy: &Policy,
    ) -> Option<Self> {
        let admin = git::admin(&entry.path)?;
        admin_safe(&entry.path).ok()?;
        let commondir = match fs::metadata(admin.join("commondir")) {
            Ok(meta) => Some(Stamp::of(meta)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return None,
        };
        Some(Self {
            head: entry.head.clone(),
            branch: entry.branch.clone(),
            finished,
            folder: stamp(&entry.path)?,
            index: stamp(&admin.join("index"))?,
            head_file: stamp(&admin.join("HEAD"))?,
            commondir,
            config: policy.config.clone(),
            artifacts: artifacts.map(str::to_string),
            documents: documents.to_vec(),
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
    #[serde(default)]
    inventory: Option<Inventory>,
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
    entries.sort_unstable_by_key(|entry| entry.file_name());
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
    /// The project's artifacts folder ([`git::left_behind`]).
    artifacts: Option<String>,
    /// The project's document folders, where the owner's answers are written
    /// ([`crate::project::documents`]).
    documents: Vec<String>,
    policy: OnceCell<Option<Policy>>,
}

impl Verdicts {
    pub fn new(
        root: &Path,
        dry_run: bool,
        artifacts: Option<&str>,
        documents: Vec<String>,
    ) -> Self {
        Self {
            dry_run,
            root: root.into(),
            artifacts: artifacts.map(str::to_string),
            documents,
            policy: OnceCell::new(),
        }
    }

    fn inputs(&self, entry: &git::Listed, finished: bool, policy: &Policy) -> Option<Inputs> {
        Inputs::read(
            entry,
            finished,
            self.artifacts.as_deref(),
            &self.documents,
            policy,
        )
    }

    pub fn read(&self, entry: &git::Listed, finished: bool) -> Verdict {
        let policy = self
            .policy
            .get_or_init(|| Policy::read(&self.root))
            .as_ref();
        let inputs = policy.and_then(|policy| self.inputs(entry, finished, policy));
        let path = git::admin(&entry.path).map(|dir| dir.join("symphony-sweep-verdict.json"));
        if let (Some(inputs), Some(path), Some(policy)) = (&inputs, &path, policy)
            && let Some(saved) = fs::read(path)
                .ok()
                .and_then(|bytes| crate::objects::parse::<Saved>(&bytes))
            && saved.inputs == *inputs
            && (!saved.verdict.left.is_empty() || saved.inventory.is_some())
            && saved
                .witnesses
                .iter()
                .all(|(path, was)| entry_stamp(path).as_ref() == Some(was))
            && saved
                .inventory
                .as_ref()
                .is_none_or(|inventory| inventory.matches(&entry.path))
            && saved.policy_now(&entry.path, policy).as_ref() == Some(&saved.policy)
        {
            return saved.verdict;
        }
        self.fresh(entry, finished, policy, CleanProof::Collect)
    }

    fn fresh(
        &self,
        entry: &git::Listed,
        finished: bool,
        policy: Option<&Policy>,
        proof: CleanProof,
    ) -> Verdict {
        let began = SystemTime::now();
        let inputs = policy.and_then(|policy| self.inputs(entry, finished, policy));
        let mut status = git::file_status(&entry.path, finished, self.artifacts.as_deref());
        let mut owner = Vec::new();
        if status
            .left
            .iter()
            .any(|name| git::under(name, &self.documents))
        {
            match git::answers(&entry.path, &self.documents, None) {
                Ok(found) => {
                    for answer in found {
                        if !owner.contains(&answer.file) {
                            owner.push(answer.file);
                        }
                    }
                }
                // Unread, it keeps the tree, and is read again next time: never kept as none.
                Err(why) => status.left.push(format!(
                    "(its documents could not be read for the owner's answers: {why})"
                )),
            }
        }
        let verdict = Verdict {
            owner,
            left: status.left,
        };
        if self.dry_run || verdict.left.iter().any(|name| name.starts_with('(')) {
            return verdict;
        }
        let (Some(inputs), Some(policy), Some(admin)) = (inputs, policy, git::admin(&entry.path))
        else {
            return verdict;
        };
        let inventory = if verdict.left.is_empty() {
            if proof == CleanProof::Skip {
                return verdict;
            }
            match Inventory::collect(&entry.path, &status.ignored_dirs, began) {
                Ok(inventory) => Some(inventory),
                Err(_) => return self.fresh(entry, finished, Some(policy), CleanProof::Skip),
            }
        } else {
            None
        };
        let witnesses = if inventory.is_some() {
            Vec::new()
        } else {
            let Some(witnesses) = witnesses(&entry.path, &verdict) else {
                return verdict;
            };
            witnesses
        };
        let mut saved = Saved {
            inputs,
            verdict,
            witnesses,
            policy: PolicyInputs::new(),
            inventory,
        };
        let current = saved.policy_now(&entry.path, policy);
        let stable = self.inputs(entry, finished, policy).as_ref() == Some(&saved.inputs)
            && saved.witnesses.iter().all(|(path, value)| {
                value.predates(began) && entry_stamp(path).as_ref() == Some(value)
            })
            && saved
                .inventory
                .as_ref()
                .is_none_or(|inventory| inventory.matches(&entry.path));
        if let Some(current) = current
            && stable
            && current
                .values()
                .flatten()
                .all(|value| value.predates(began))
        {
            saved.policy = current;
            saved.publish(&admin.join("symphony-sweep-verdict.json"));
        } else if saved.inventory.is_some() && proof == CleanProof::Collect {
            return self.fresh(entry, finished, Some(policy), CleanProof::Skip);
        }
        saved.verdict
    }
}

impl Saved {
    fn policy_now(&self, root: &Path, policy: &Policy) -> Option<PolicyInputs> {
        match &self.inventory {
            Some(inventory) => policy.for_directories(root, inventory.directories()),
            None => policy.inputs(root, &self.witnesses),
        }
    }

    fn publish(&self, path: &Path) {
        let temp = path.with_extension(format!(
            "{}-{}.tmp",
            std::process::id(),
            WRITES.fetch_add(1, Ordering::Relaxed)
        ));
        if let Ok(bytes) = serde_json::to_vec(self) {
            if fs::write(&temp, bytes).is_ok() {
                let _ = fs::rename(&temp, path);
            }
            let _ = fs::remove_file(temp);
        }
    }
}
