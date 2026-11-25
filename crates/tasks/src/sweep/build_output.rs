//! The build output of work past its executor: a worktree's build folders ([`FOLDERS`], and what git
//! ignores in [`BUILDS`]) go, the worktree staying, once nobody builds there. The worktree sweep
//! keeps many a worktree whose work is in review or done (a Claude Code agent's lock held by its
//! orchestrator's live process, work not landed yet, files that did not land, the window), and each
//! holds gigabytes of compiled code that a build makes again: a reviewer builds in a worktree of its
//! own. A worktree's build output goes only when all of these hold:
//!
//! * it is not the main checkout, nor the worktree running the sweep, nor where `main` is checked
//!   out, and holds no other worktree inside it;
//! * it is work past its executor, or a land's or the release run's. Work: it is tied to a task, as the
//!   worktree sweep ties them (its branch, a session bound to it holding a role, an in-process
//!   agent's by its agent id included, a launch), and every task it is tied to is in review, done
//!   or cancelled ([`AFTER_WORK`]), merged or not: build output is not work. A task in progress,
//!   ready, blocked, or open in any other status, keeps it. A land's: its lock names a land whose
//!   process is gone (the `../symphony-wt/` worktree a land made and did not remove). The release run's:
//!   its lock is the release run's, and its build stays [`RELEASE_BUILD_DAYS`], as the run keeps it
//!   to build on run after run. A worktree tied to no task that is neither keeps its build: the
//!   owner's, or one a project made by hand, whose rebuild has no copy-on-write seed;
//! * its lock ([`Holder`]) lets it: none, a dead one, or Claude Code's of a live process (its
//!   orchestrator's) on a worktree tied to a task. A land that runs keeps it, as does a lock the
//!   sweep does not know;
//! * nothing ran there for the hour ([`IDLE_SECONDS`]): no agent bound to it (a session at it, or
//!   the Claude Code agent whose worktree it is) beat or made a tool call, whatever its health;
//!   nothing touched it (its git admin files, its top folder); and the board's recorder recorded no
//!   build or test in it that ran or ended within the hour, or runs now (`tasks run`, its
//!   operations in `symphony/tasks/work/` of the common dir), so a reviewer building in an
//!   executor's worktree keeps its build;
//! * no process is working inside it (`lsof`; when that cannot be read, nothing goes).
//!
//! Right before removing, it reads the worktree listing, the board and the recorder's operations
//! again and decides again (an error reading the listing or the board keeps the worktree's build),
//! and asks again whether a process works inside. Then, for each Cargo folder, it takes every Cargo
//! build lock in it without waiting, so a Cargo building there from any folder keeps it; under them
//! it renames the folder aside (`.target.clear.PID`), so a Cargo starting after builds in a new one,
//! and removes that. Of each other build folder, only what git ignores goes (`git clean -X`): what
//! it tracks, or holds untracked and not ignored, stays. A folder is only ever one whose every part
//! below the worktree is a real folder, never a link, and whose real path is inside the worktree.
//! What an interrupted seed or clear left beside a build folder (`.target.seed.PID`,
//! `.target.clear.PID`, its process gone) goes with it.

use super::holder::{Holder, RELEASE_BUILD_DAYS};
use super::{Facts, Index, Options, canonical, git, inside, size};
use crate::Store;
use crate::cli::machine_work::recorded;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// A worktree's Cargo build folders, from its top: this workspace's, and a consuming project's copy
/// of it.
pub const FOLDERS: [&str; 2] = [".target", "symphony/.target"];
/// A worktree's other build folders, from its top: a project's export or client build, and a
/// consuming project's copy of the Symphony's. Only what git ignores in them goes.
pub const BUILDS: [&str; 2] = ["build", "symphony/build"];
/// How long a worktree's agent and its git files must have been still before its build output goes,
/// in seconds.
pub const IDLE_SECONDS: f64 = 3600.0;
/// Where every task tied to a worktree must be for its build output to go: past its executor's
/// work.
pub const AFTER_WORK: [&str; 3] = ["review", "done", "cancelled"];

/// The builds and tests the board's recorder recorded in `root`'s repository: where each ran (its
/// checkout, canonical), when it last said it ran or ended, and whether it runs now. Read once a
/// pass; an operation the read could not take says nothing of any worktree.
pub(super) fn recorded_work(root: &Path) -> Vec<(PathBuf, f64, bool)> {
    let Ok(common) = crate::git::common_dir(root) else {
        return Vec::new();
    };
    recorded::read(&recorded::in_common(&common))
        .records
        .into_iter()
        .map(|(record, _)| {
            let last = [Some(record.beat_at), record.started_at, record.ended_at]
                .into_iter()
                .flatten()
                .fold(record.queued_at, f64::max);
            let runs = record.state != recorded::Step::Finished;
            (canonical(Path::new(&record.checkout)), last, runs)
        })
        .collect()
}

/// Why the build output of worktree `path`, listed as `entry`, may go at `now`; none while it stays.
/// `work` is what the recorder recorded ([`recorded_work`]).
fn idle(
    index: &Index<'_>,
    entry: &git::Listed,
    path: &Path,
    o: &Options,
    work: &[(PathBuf, f64, bool)],
) -> Option<String> {
    let branch = entry
        .branch
        .as_deref()
        .and_then(|b| b.strip_prefix("refs/heads/"));
    let tied = index.tied(path, branch);
    let status = |slug: &str| index.tasks[slug].status.as_str();
    if tied.keys().any(|slug| !AFTER_WORK.contains(&status(slug))) {
        return None;
    }
    let tasks = || {
        tied.keys()
            .map(|slug| format!("task {slug} is {}", status(slug)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let (what, still) = match Holder::of(entry.locked.as_deref(), &*o.alive, &*o.started) {
        Holder::Release => (
            "the release run's checkout".to_string(),
            RELEASE_BUILD_DAYS * 86_400.0,
        ),
        Holder::LandEnded(pid) => (
            format!("the worktree of a land that ended (pid {pid})"),
            IDLE_SECONDS,
        ),
        Holder::Land(_) | Holder::Foreign(_) => return None,
        // The owner's, or one a project made by hand: its build is no work of the board's.
        _ if tied.is_empty() => return None,
        Holder::Nobody | Holder::Gone(_) | Holder::Agent { .. } => (tasks(), IDLE_SECONDS),
    };
    let now = (o.now)();
    let ran = work
        .iter()
        .any(|(at, last, runs)| at.starts_with(path) && (*runs || now - last < IDLE_SECONDS));
    if ran
        || index
            .last_beat(path)
            .is_some_and(|at| now - at < IDLE_SECONDS)
        || now - git::last_touched(path, now) < still
    {
        return None;
    }
    let quiet = if still > IDLE_SECONDS {
        format!("nothing touched it for {RELEASE_BUILD_DAYS} days")
    } else {
        "nothing ran there for an hour".to_string()
    };
    Some(format!("{what}; {quiet}"))
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

/// A build folder of a worktree, and how it goes.
#[derive(Debug, Clone, PartialEq)]
enum Folder {
    /// A Cargo build folder, or what a seed or a clear left beside one: it goes whole, under its
    /// build locks.
    Cargo(PathBuf),
    /// Another build folder: what git ignores in it goes.
    Ignored(PathBuf),
}

impl Folder {
    fn path(&self) -> &Path {
        match self {
            Folder::Cargo(path) | Folder::Ignored(path) => path,
        }
    }
}

/// The build folders of worktree `path` there are, each a real folder of it ([`within`]): its Cargo
/// folders and beside each what an interrupted seed or clear left (`NAME.seed.PID`,
/// `NAME.clear.PID`) once its process is gone (`alive`), then its other build folders.
fn folders(path: &Path, alive: &dyn Fn(i64) -> bool) -> Vec<Folder> {
    let mut found = Vec::new();
    for rel in FOLDERS {
        let folder = path.join(rel);
        if within(path, &folder) {
            found.push(Folder::Cargo(folder.clone()));
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
        #[expect(
            clippy::disallowed_methods,
            reason = "a busy board path keeps std's in-place sort (crate::order): the sweep"
        )]
        left.sort_unstable();
        found.extend(left.into_iter().map(Folder::Cargo));
    }
    found.extend(
        BUILDS
            .iter()
            .map(|rel| path.join(rel))
            .filter(|folder| within(path, folder))
            .map(Folder::Ignored),
    );
    found
}

/// What git ignores in build folder `folder` of worktree `path`, as `git clean -X` would remove
/// it, each in full; none when git cannot say.
fn ignored_in(path: &Path, folder: &Path) -> Option<Vec<PathBuf>> {
    let rel = folder.strip_prefix(path).ok()?.to_str()?;
    let listed = git::stdout(
        path,
        &[
            "-c",
            "core.quotePath=off",
            "clean",
            "-n",
            "-d",
            "-X",
            "--",
            &format!("{rel}/"),
        ],
    )?;
    Some(
        listed
            .lines()
            .filter_map(|line| line.strip_prefix("Would remove "))
            .map(|name| path.join(name.trim_matches('"').trim_end_matches('/')))
            .collect(),
    )
}

/// What `folder` of worktree `path` holds that its clear removes: all of a Cargo folder, what git
/// ignores of another; none when git cannot say what it ignores.
fn clearable(path: &Path, folder: &Folder) -> Option<u64> {
    match folder {
        Folder::Cargo(at) => Some(size::bytes(at)),
        Folder::Ignored(at) => {
            let ignored = ignored_in(path, at)?;
            Some(ignored.iter().map(|each| size::bytes(each)).sum())
        }
    }
}

/// Remove what git ignores in build folder `folder` of worktree `path`: what it tracks, or holds
/// untracked and not ignored, stays. Whether it went.
fn remove_ignored(path: &Path, folder: &Path) -> bool {
    let Some(rel) = folder.strip_prefix(path).ok().and_then(Path::to_str) else {
        return false;
    };
    within(path, folder)
        && git::succeeds(
            path,
            &["clean", "-f", "-d", "-X", "-q", "--", &format!("{rel}/")],
        )
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
    #[expect(
        clippy::disallowed_methods,
        reason = "a busy board path keeps std's in-place sort (crate::order): the sweep"
    )]
    locks.sort_unstable();
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

/// A build folder the sweep removed, or in a dry run would: where, why, and what it held on disk
/// ([`size::bytes`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Cleared {
    pub folder: PathBuf,
    pub why: String,
    pub bytes: u64,
}

/// Remove the build output of every worktree of `root`'s repository the rules let go (none when
/// `o.dry_run`), but those in `gone`, removed whole already: each build folder, and why. `working`
/// is the sweep's scan of the processes' working directories, made here if not yet.
pub(super) fn clear(
    root: &Path,
    here: &Path,
    store: &Store,
    o: &Options,
    gone: &BTreeSet<PathBuf>,
    working: &mut Option<Option<Vec<PathBuf>>>,
    trees: &[git::Listed],
) -> Result<Vec<Cleared>, String> {
    let paths: Vec<PathBuf> = trees.iter().map(|t| canonical(&t.path)).collect();
    let candidates: Vec<(&git::Listed, &PathBuf, Vec<Folder>)> = trees
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
    let work = recorded_work(root);
    let mut cleared = Vec::new();
    for (entry, path, found) in candidates {
        let Some(why) = idle(&index, entry, path, o, &work) else {
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
            if !still_idle(root, store, entry, path, o).unwrap_or(false)
                || (o.occupied)(path) != Some(false)
            {
                continue;
            }
        }
        for folder in found {
            let Some(bytes) = clearable(path, &folder) else {
                continue;
            };
            let at = folder.path();
            // Nothing git ignores there: nothing to clear.
            if matches!(folder, Folder::Ignored(_)) && bytes == 0 {
                continue;
            }
            if !o.dry_run {
                (o.pause)("clear", &at.to_string_lossy());
                let went = match &folder {
                    Folder::Cargo(at) => remove_unlocked(path, at, o),
                    Folder::Ignored(at) => remove_ignored(path, at),
                };
                if !went {
                    continue;
                }
            }
            let listed = entry.path.join(at.strip_prefix(path).unwrap_or(at));
            cleared.push(Cleared {
                folder: listed,
                why: why.clone(),
                bytes,
            });
        }
    }
    Ok(cleared)
}

/// Whether worktree `path` is still at the HEAD, branch and lock it was judged at, and its build
/// output still may go, on a new listing, a new read of the board and of the recorder's operations.
fn still_idle(
    root: &Path,
    store: &Store,
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
    if entry.head != judged.head || entry.branch != judged.branch || entry.locked != judged.locked {
        return Ok(false);
    }
    let facts = Facts::read(store).map_err(|e| e.to_string())?;
    let index = Index::of(&facts, &paths);
    Ok(idle(&index, entry, path, o, &recorded_work(root)).is_some())
}
