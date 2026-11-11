//! The build output of finished work: a worktree's build folders ([`FOLDERS`]) go, the worktree
//! staying, once its work has landed and nobody builds there. The worktree sweep keeps many a
//! worktree whose work is done (a Claude Code agent's lock held by its orchestrator's live process,
//! files that did not land, the window), and each holds gigabytes of compiled code. A worktree's
//! build output goes only when all of these hold:
//!
//! * it is not the main checkout, nor the worktree running the sweep, nor where `main` is checked
//!   out, and holds no other worktree inside it;
//! * it is tied to a task, as the worktree sweep ties them (its branch, a session's role, a
//!   launch), and every task it is tied to is done or cancelled, one of them done: a task in
//!   progress, or open in any other status, keeps it;
//! * no agent bound to it (a session at it, or the Claude Code agent whose worktree it is) beat or
//!   made a tool call within the hour, whatever its health, and nothing touched it (its git admin
//!   files, its top folder) within the hour;
//! * its HEAD is merged work: an ancestor of `main` that came in through a merge (not on `main`'s
//!   first-parent line), or a commit made in this worktree (its HEAD's reflog says `commit`). A
//!   branch with no commits ahead of `main` is not merged work: its HEAD is a commit of `main`'s own
//!   line, where every new worktree starts;
//! * no process is working inside it (`lsof`; when that cannot be read, nothing goes).
//!
//! Right before removing, it reads the worktree listing and the board again and decides again (an
//! error reading them keeps the worktree's build), and asks again whether a process works inside.
//! Then, for each folder, it takes every Cargo build lock in it without waiting, so a Cargo
//! building there from any folder keeps it; under them it renames the folder aside
//! (`.target.clear.PID`), so a Cargo starting after builds in a new one, and removes that. A
//! folder is only ever one whose every part below the worktree is a real folder, never a link, and
//! whose real path is inside the worktree. What an interrupted seed or clear left beside a build
//! folder (`.target.seed.PID`, `.target.clear.PID`, its process gone) goes with it.

use super::{Facts, Index, Options, canonical, git, inside};
use crate::Store;
use crate::health::FINISHED;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// A worktree's build folders, from its top: this workspace's, and a consuming project's copy of it.
pub const FOLDERS: [&str; 2] = [".target", "symphony/.target"];
/// How long a worktree's agent and its git files must have been still before its build output goes,
/// in seconds.
pub const IDLE_SECONDS: f64 = 3600.0;

/// The repository's history as the rule reads it: each commit's ancestry, and `main`'s first-parent
/// line, read once a sweep and only once a worktree gets that far.
struct History<'a> {
    root: &'a Path,
    main: &'a str,
    first_parent: Option<BTreeSet<String>>,
    ancestry: BTreeMap<String, bool>,
}

impl History<'_> {
    /// Whether `head`, the HEAD of worktree `path`, is merged work.
    fn merged_work(&mut self, path: &Path, head: &str) -> bool {
        if head.is_empty() {
            return false;
        }
        let (root, main) = (self.root, self.main);
        if !*self
            .ancestry
            .entry(head.to_string())
            .or_insert_with(|| git::ancestor(root, head, main))
        {
            return false;
        }
        let line = self.first_parent.get_or_insert_with(|| {
            git::stdout(root, &["rev-list", "--first-parent", main])
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        });
        !line.contains(head) || committed_here(path, head)
    }
}

/// Whether the reflog of worktree `path`'s HEAD records `head` as a commit made there (`commit`,
/// `commit (amend)`, `commit (merge)`, `commit (initial)`); a fast-forward or a reset there is not.
pub fn committed_here(path: &Path, head: &str) -> bool {
    let Some(admin) = git::admin(path) else {
        return false;
    };
    let Ok(log) = fs::read_to_string(admin.join("logs/HEAD")) else {
        return false;
    };
    log.lines().any(|line| {
        let Some((entry, message)) = line.split_once('\t') else {
            return false;
        };
        entry.split(' ').nth(1) == Some(head) && message.starts_with("commit")
    })
}

/// Why the build output of worktree `path`, listed as `entry`, may go at `now`; none while it stays.
fn finished(
    index: &Index<'_>,
    history: &mut History<'_>,
    entry: &git::Listed,
    path: &Path,
    now: f64,
) -> Option<String> {
    let branch = entry
        .branch
        .as_deref()
        .and_then(|b| b.strip_prefix("refs/heads/"));
    let tied = index.tied(path, branch);
    if !tied
        .keys()
        .all(|slug| FINISHED.contains(&index.tasks[slug].status.as_str()))
    {
        return None;
    }
    let done = *tied
        .keys()
        .find(|slug| index.tasks[*slug].status == "done")?;
    if index
        .last_beat(path)
        .is_some_and(|at| now - at < IDLE_SECONDS)
        || now - git::last_touched(path, now) < IDLE_SECONDS
        || !history.merged_work(path, &entry.head)
    {
        return None;
    }
    let place = branch.map_or_else(|| "detached".to_string(), |b| format!("on {b}"));
    let short = entry.head.get(..12).unwrap_or(&entry.head);
    Some(format!(
        "task {done} is done; {place} at {short}, merged into main; no agent there for an hour"
    ))
}

/// Whether `folder`, below worktree `path` (canonical), is a real folder of that worktree: every part
/// of its path below `path` a folder and never a link, and its canonical path inside `path`. A
/// `symphony` or `.target` that links elsewhere is never followed out of the worktree.
fn within(path: &Path, folder: &Path) -> bool {
    let Ok(below) = folder.strip_prefix(path) else {
        return false;
    };
    let mut at = path.to_path_buf();
    for part in below.components() {
        let Component::Normal(name) = part else {
            return false;
        };
        at.push(name);
        if !fs::symlink_metadata(&at).is_ok_and(|meta| meta.is_dir()) {
            return false;
        }
    }
    fs::canonicalize(folder).is_ok_and(|real| real != path && real.starts_with(path))
}

/// The process an aside folder's name gives, `NAME.seed.PID` (a seed's staging,
/// `tools/seed-target.sh`) or `NAME.clear.PID` (a clear's, [`remove_unlocked`]), for the build
/// folder named `name`.
fn aside_pid(name: &str, other: &str) -> Option<i64> {
    let rest = other.strip_prefix(name)?;
    let pid = rest
        .strip_prefix(".seed.")
        .or_else(|| rest.strip_prefix(".clear."))?;
    pid.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| pid.parse().ok())
        .flatten()
        .filter(|&pid| pid > 0)
}

/// The build folders of worktree `path` there are, each a real folder of it ([`within`]), and
/// beside each what an interrupted seed or clear left (`NAME.seed.PID`, `NAME.clear.PID`) once its
/// process is gone (`alive`).
fn folders(path: &Path, alive: &dyn Fn(i64) -> bool) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for rel in FOLDERS {
        let folder = path.join(rel);
        if within(path, &folder) {
            found.push(folder.clone());
        }
        let (Some(parent), Some(name)) = (folder.parent(), folder.file_name()) else {
            continue;
        };
        let (name, Ok(entries)) = (name.to_string_lossy(), fs::read_dir(parent)) else {
            continue;
        };
        let mut left: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| {
                aside_pid(&name, &entry.file_name().to_string_lossy())
                    .is_some_and(|pid| !alive(pid))
            })
            .map(|entry| entry.path())
            .filter(|aside| within(path, aside))
            .collect();
        left.sort();
        found.extend(left);
    }
    found
}

/// The build locks Cargo holds in build folder `folder` while it builds there: each profile's
/// (`debug/`, `release/`, and a target's `TRIPLE/debug/`) `.cargo-lock`, `.cargo-build-lock` and
/// `.cargo-artifact-lock`.
fn build_locks(folder: &Path) -> Vec<PathBuf> {
    const LOCKS: [&str; 3] = [".cargo-lock", ".cargo-build-lock", ".cargo-artifact-lock"];
    let real_dirs = |dir: &Path| -> Vec<PathBuf> {
        fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|p| fs::symlink_metadata(p).is_ok_and(|meta| meta.is_dir()))
                    .collect()
            })
            .unwrap_or_default()
    };
    let profiles = real_dirs(folder);
    let nested: Vec<PathBuf> = profiles.iter().flat_map(|dir| real_dirs(dir)).collect();
    let mut locks: Vec<PathBuf> = profiles
        .iter()
        .chain(&nested)
        .flat_map(|dir| LOCKS.map(|lock| dir.join(lock)))
        .filter(|lock| fs::symlink_metadata(lock).is_ok_and(|meta| meta.is_file()))
        .collect();
    locks.sort();
    locks
}

/// Remove build folder `folder` of worktree `path`, unless a Cargo is building in it: holding
/// every build lock in it ([`build_locks`]), each taken without waiting, it is renamed aside in its
/// own folder (`NAME.clear.PID`, which `.gitignore` ignores and a later sweep removes should this
/// one stop), so a Cargo starting from then on builds in a new folder, and only then removed.
/// Whether it went.
fn remove_unlocked(path: &Path, folder: &Path, o: &Options) -> bool {
    if !within(path, folder) {
        return false;
    }
    let mut held = Vec::new();
    for lock in build_locks(folder) {
        let Ok(file) = fs::File::open(&lock) else {
            return false;
        };
        if file.try_lock().is_err() {
            return false;
        }
        held.push(file);
    }
    (o.pause)("locked", &folder.to_string_lossy());
    let Some(name) = folder.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return false;
    };
    let aside = if aside_pid(".target", &name).is_some() {
        folder.to_path_buf()
    } else {
        let aside = folder.with_file_name(format!("{name}.clear.{}", std::process::id()));
        if fs::rename(folder, &aside).is_err() {
            return false;
        }
        aside
    };
    drop(held);
    fs::remove_dir_all(&aside).is_ok() || !aside.exists()
}

/// Remove the build output of every worktree of `root`'s repository the rules let go (none when
/// `o.dry_run`), but those in `gone`, removed whole already: each build folder, and why. `working`
/// is the sweep's scan of the processes' working directories, made here if not yet.
#[expect(
    clippy::too_many_arguments,
    reason = "one step of `sweep`, sharing what the sweep read"
)]
pub(super) fn clear(
    root: &Path,
    here: &Path,
    main: &str,
    store: &Store,
    o: &Options,
    gone: &BTreeSet<PathBuf>,
    working: &mut Option<Option<Vec<PathBuf>>>,
    trees: &[git::Listed],
) -> Result<Vec<(PathBuf, String)>, String> {
    let paths: Vec<PathBuf> = trees.iter().map(|t| canonical(&t.path)).collect();
    let candidates: Vec<(&git::Listed, &PathBuf, Vec<PathBuf>)> = trees
        .iter()
        .zip(&paths)
        .enumerate()
        .filter(|(number, (entry, path))| {
            o.only.as_ref().is_none_or(|only| only.contains(*path))
                && *number != 0
                && *path != here
                && !gone.contains(*path)
                && !entry.bare
                && !entry.prunable
                && path.is_dir()
                && entry.branch.as_deref() != Some("refs/heads/main")
                && !paths.iter().any(|other| inside(other, path))
        })
        .map(|(_, (entry, path))| (entry, path, folders(path, &*o.alive)))
        .filter(|(_, _, found)| !found.is_empty())
        .collect();
    // No build folder to judge: the board is not read.
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let facts = Facts::read(store).map_err(|e| e.to_string())?;
    let index = Index::of(&facts, &paths);
    let mut history = History {
        root,
        main,
        first_parent: None,
        ancestry: BTreeMap::new(),
    };
    let mut cleared = Vec::new();
    for (entry, path, found) in candidates {
        let Some(why) = finished(&index, &mut history, entry, path, (o.now)()) else {
            continue;
        };
        let seen = working.get_or_insert_with(|| {
            (o.cwds)()
                .filter(|cwds| !cwds.is_empty())
                .map(|cwds| cwds.iter().map(|w| canonical(w)).collect())
        });
        if seen
            .as_ref()
            .is_none_or(|cwds| cwds.iter().any(|w| w.starts_with(path)))
        {
            continue;
        }
        if !o.dry_run {
            (o.pause)("judged", &path.to_string_lossy());
            // Read again right before removing: the listing and the board (an error keeps it), then
            // whether a process works inside now.
            if !still_finished(root, store, &mut history, entry, path, o).unwrap_or(false)
                || (o.occupied)(path) != Some(false)
            {
                continue;
            }
        }
        for folder in found {
            if !o.dry_run {
                (o.pause)("clear", &folder.to_string_lossy());
                if !remove_unlocked(path, &folder, o) {
                    continue;
                }
            }
            let listed = entry
                .path
                .join(folder.strip_prefix(path).unwrap_or(&folder));
            cleared.push((listed, why.clone()));
        }
    }
    Ok(cleared)
}

/// Whether worktree `path` is still at the HEAD and branch it was judged at, and its build output
/// still may go, on a new listing and a new read of the board.
fn still_finished(
    root: &Path,
    store: &Store,
    history: &mut History<'_>,
    judged: &git::Listed,
    path: &Path,
    o: &Options,
) -> Result<bool, String> {
    let trees = git::worktrees(root)?;
    let paths: Vec<PathBuf> = trees.iter().map(|t| canonical(&t.path)).collect();
    let Some(entry) = trees
        .iter()
        .zip(&paths)
        .find_map(|(t, p)| (p == path).then_some(t))
    else {
        return Ok(false);
    };
    if entry.head != judged.head || entry.branch != judged.branch {
        return Ok(false);
    }
    let facts = Facts::read(store).map_err(|e| e.to_string())?;
    let index = Index::of(&facts, &paths);
    Ok(finished(&index, history, entry, path, (o.now)()).is_some())
}
