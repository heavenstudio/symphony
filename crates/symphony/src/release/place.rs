//! Where a run works, and what it may take over there (docs/RELEASES.md). Its checkout is a
//! worktree of the repository that the release run made, locked with [`LOCK_REASON`] and marked in its
//! admin folder with its real path ([`MARK`]). It is outside the main checkout and every other
//! worktree, and holds none. Anything else found in its place, a link included, is refused and
//! left exactly as it is. The build folder it keeps is cleared once it is older than
//! [`BUILD_KEPT_DAYS`]. The checkout a build older than the rename to `just release` made under its
//! name (`nightly` beside the checkout, [`LEGACY_LOCK_REASON`] and its mark) the first run of a
//! build that knows the new name moves to it ([`legacy`]), once no process works in it.

use std::fs;
use std::path::{Component, Path, PathBuf};

use super::git::{commit, git, git_until, short};
use super::group::{self, Bound};

/// The reason the release run's checkout is locked with, which keeps the board's sweep off it.
pub(crate) const LOCK_REASON: &str = "the Symphony release run's checkout (docs/RELEASES.md)";
/// The marker in the checkout's admin folder (`.git/worktrees/NAME/`): the checkout's real path.
const MARK: &str = "symphony-release";
/// When the build folder the release run keeps was last cleared, beside the checkout's marker.
pub(crate) const BUILT: &str = "symphony-release-built";
/// The days the build folder the release run keeps is built on incrementally before it is cleared.
pub(crate) const BUILD_KEPT_DAYS: f64 = 7.0;
/// The checkout's name before the rename, beside where it is now.
const LEGACY_NAME: &str = "nightly";
/// The reason a build older than the rename locked the checkout with.
pub(crate) const LEGACY_LOCK_REASON: &str = "the Symphony nightly's checkout (docs/RELEASES.md)";
/// [`MARK`] and [`BUILT`] as a build older than the rename named them.
const LEGACY_MARK: &str = "symphony-nightly";
const LEGACY_BUILT: &str = "symphony-nightly-built";
/// The git locks a killed checkout or reset leaves in a worktree's admin folder.
const GIT_LOCKS: [&str; 2] = ["index.lock", "HEAD.lock"];

/// `path` with its `.` and `NAME/..` parts folded, as written: no link is followed.
pub(crate) fn normal(path: &Path) -> PathBuf {
    let mut folded = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                folded.pop();
            }
            other => folded.push(other),
        }
    }
    folded
}

/// `path` as it is on disk: its longest existing part with every link resolved, the rest as
/// written.
pub(crate) fn real(path: &Path) -> PathBuf {
    let path = normal(path);
    let mut existing = path.as_path();
    let mut rest = Vec::new();
    loop {
        if let Ok(found) = existing.canonicalize() {
            let mut real = found;
            for part in rest.iter().rev() {
                real.push(part);
            }
            return real;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                existing = parent;
            }
            _ => return path,
        }
    }
}

/// Whether `path` is a link.
fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// A worktree `git worktree list` names: its real path, and its lock's reason when it is locked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) path: PathBuf,
    pub(crate) locked: Option<String>,
}

/// The worktrees `git worktree list --porcelain -z` printed.
fn listed(listing: &str) -> Vec<Listed> {
    let mut all = Vec::new();
    let mut current: Option<Listed> = None;
    for field in listing.split('\0') {
        if let Some(path) = field.strip_prefix("worktree ") {
            all.extend(current.take());
            current = Some(Listed {
                path: real(Path::new(path)),
                locked: None,
            });
        } else if let Some(tree) = current.as_mut() {
            if field == "locked" {
                tree.locked = Some(String::new());
            } else if let Some(reason) = field.strip_prefix("locked ") {
                tree.locked = Some(reason.to_string());
            }
        }
    }
    all.extend(current);
    all
}

/// Why `path` cannot be a folder of the release run's: it is one of `others` (the main checkout and
/// the worktrees not its own), lies inside one, or holds one.
fn crossed(path: &Path, others: &[PathBuf]) -> Option<String> {
    others.iter().find_map(|other| {
        if path == other {
            Some(format!(
                "{} is the worktree {}",
                path.display(),
                other.display()
            ))
        } else if path.starts_with(other) {
            Some(format!(
                "{} is inside the checkout {}",
                path.display(),
                other.display()
            ))
        } else if other.starts_with(path) {
            Some(format!(
                "{} holds the checkout {}",
                path.display(),
                other.display()
            ))
        } else {
            None
        }
    })
}

/// Whether the marker at `marker` names `path`.
fn marked(marker: &Path, path: &Path) -> bool {
    fs::read_to_string(marker).is_ok_and(|said| Path::new(said.trim_end_matches('\n')) == path)
}

/// The admin folder in the git common dir `common` of the worktree registered at the real path
/// `path`: the one whose `gitdir` names `path/.git`.
fn admin(common: &Path, path: &Path) -> Option<PathBuf> {
    let wanted = path.join(".git");
    fs::read_dir(common.join("worktrees"))
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|folder| {
            fs::read_to_string(folder.join("gitdir"))
                .is_ok_and(|said| real(Path::new(said.trim_end())) == wanted)
        })
}

/// Where a run works.
#[derive(Debug)]
pub(crate) struct Place {
    /// Its checkout's real path.
    pub(crate) checkout: PathBuf,
    /// The checkout's admin folder, when the release run made it before.
    admin: Option<PathBuf>,
}

/// The run's place, the checkout at `asked`, in the repository whose checkout is `root`, main
/// checkout `main` and common dir `common`: refused, and left as it is, when it is not the
/// release run's.
pub(crate) fn place(
    root: &Path,
    main: &Path,
    common: &Path,
    asked: &Path,
) -> Result<Place, String> {
    let asked = normal(asked);
    if is_link(&asked) {
        return Err(format!(
            "{} is a link: the release run's folders are its own, and it is left as it is; move it \
             away",
            asked.display()
        ));
    }
    let checkout = real(&asked);
    let worktrees = listed(&git(root, &["worktree", "list", "--porcelain", "-z"])?);
    let mut others = vec![real(main)];
    let mut mine = None;
    for tree in worktrees {
        if tree.path == checkout {
            mine = Some(tree);
        } else {
            others.push(tree.path);
        }
    }
    if let Some(why) = crossed(&checkout, &others) {
        return Err(format!(
            "{why}: the release run works outside the repository's checkouts, and leaves them as \
             they are"
        ));
    }
    let not_ours = |why: &str| {
        format!(
            "{} is not the release run's own checkout ({why}); it is left as it is: move it away or \
             name another --worktree",
            checkout.display()
        )
    };
    let exists = fs::symlink_metadata(&checkout).is_ok();
    let Some(tree) = mine else {
        if exists {
            return Err(not_ours("not a worktree of this repository"));
        }
        return Ok(Place {
            checkout,
            admin: None,
        });
    };
    if tree.locked.as_deref() != Some(LOCK_REASON) {
        return Err(not_ours("a worktree the release run did not lock"));
    }
    let admin = admin(common, &checkout)
        .filter(|admin| marked(&admin.join(MARK), &checkout))
        .ok_or_else(|| not_ours("a worktree without the release run's marker"))?;
    if exists {
        let pointed = fs::read_to_string(checkout.join(".git")).ok();
        let gitdir = pointed
            .as_deref()
            .and_then(|said| said.trim_end().strip_prefix("gitdir: "))
            .map(|dir| real(Path::new(dir)));
        if is_link(&checkout.join(".git")) || gitdir.as_ref() != Some(&real(&admin)) {
            return Err(not_ours("its .git is not the worktree's"));
        }
    }
    Ok(Place {
        checkout,
        admin: Some(admin),
    })
}

/// What a build older than the rename made at `nightly` beside the run's checkout, which the run
/// moves to the names this build gives it ([`legacy`], [`Legacy::moved`]).
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Legacy {
    /// The checkout that build locked and marked, where it is and where it goes.
    checkout: Option<(PathBuf, PathBuf)>,
    /// Why that checkout stays where it is: something is where it would go.
    left: Option<String>,
}

/// What [`Legacy::moved`] moves of what a build older than the rename made at `nightly` beside
/// `asked`, the run's checkout in the repository whose checkout is `root` and common dir `common`:
/// a checkout it locked with [`LEGACY_LOCK_REASON`] and marked, with nothing where it would go.
/// Anything else is left as it is; nothing is asked for its old name or through a link. Nothing is
/// written.
pub(crate) fn legacy(root: &Path, common: &Path, asked: &Path) -> Result<Legacy, String> {
    let asked = normal(asked);
    let Some(name) = (asked.file_name()).map(|name| name.to_string_lossy().into_owned()) else {
        return Ok(Legacy::default());
    };
    if name == LEGACY_NAME {
        return Ok(Legacy::default());
    }
    legacy_checkout(root, common, &asked.with_file_name(LEGACY_NAME), &asked)
}

/// [`legacy`]'s checkout: the one at `old`, which goes to `asked`.
fn legacy_checkout(root: &Path, common: &Path, old: &Path, asked: &Path) -> Result<Legacy, String> {
    if is_link(old) || is_link(asked) {
        return Ok(Legacy::default());
    }
    let (from, to) = (real(old), real(asked));
    let worktrees = listed(&git(root, &["worktree", "list", "--porcelain", "-z"])?);
    let ours = (worktrees.iter())
        .find(|tree| tree.path == from)
        .is_some_and(|tree| tree.locked.as_deref() == Some(LEGACY_LOCK_REASON))
        && fs::symlink_metadata(&from).is_ok_and(|meta| meta.is_dir())
        && admin(common, &from).is_some_and(|admin| marked(&admin.join(LEGACY_MARK), &from));
    if !ours {
        return Ok(Legacy::default());
    }
    if fs::symlink_metadata(&to).is_ok() || worktrees.iter().any(|tree| tree.path == to) {
        return Ok(Legacy {
            left: Some(format!(
                "left {}, the checkout a build older than the rename to `just release` made: {} is \
                 in the way",
                from.display(),
                to.display()
            )),
            ..Legacy::default()
        });
    }
    Ok(Legacy {
        checkout: Some((from, to)),
        ..Legacy::default()
    })
}

impl Legacy {
    /// The folders [`Legacy::moved`] moves, which no process may work in while it does.
    pub(crate) fn trees(&self) -> Vec<&Path> {
        (self.checkout.iter().map(|(from, _)| from.as_path())).collect()
    }

    /// Move it in the repository whose checkout is `root` and common dir `common`, within `until`:
    /// the checkout unlocked, moved by git with its build folder, locked with [`LOCK_REASON`] and
    /// marked again, its build's stamp renamed. What it moved, and a checkout it left because
    /// something is where it would go.
    pub(crate) fn moved(
        self,
        root: &Path,
        common: &Path,
        until: Bound<'_>,
    ) -> Result<Vec<String>, String> {
        let mut noted: Vec<String> = self.left.into_iter().collect();
        if let Some((from, to)) = &self.checkout {
            noted.push(moved_checkout(root, common, from, to, until)?);
        }
        Ok(noted)
    }
}

/// [`Legacy::moved`]'s checkout: the one at `from` to `to`, both real paths.
fn moved_checkout(
    root: &Path,
    common: &Path,
    from: &Path,
    to: &Path,
    until: Bound<'_>,
) -> Result<String, String> {
    let (at, there) = (from.to_string_lossy(), to.to_string_lossy());
    git_until(root, &["worktree", "unlock", &at], until)?;
    if let Err(why) = git_until(root, &["worktree", "move", &at, &there], until) {
        let relocked = ["worktree", "lock", "--reason", LEGACY_LOCK_REASON, &at];
        let _ = git_until(root, &relocked, until);
        return Err(format!("cannot move {at} to {there}: {why}"));
    }
    let admin = admin(common, to).ok_or_else(|| format!("git kept no admin folder for {there}"))?;
    let marker = admin.join(MARK);
    fs::write(&marker, format!("{there}\n")).map_err(|e| crate::text::os_error(&e, &marker))?;
    let _ = fs::remove_file(admin.join(LEGACY_MARK));
    if fs::symlink_metadata(admin.join(LEGACY_BUILT)).is_ok() {
        let _ = fs::rename(admin.join(LEGACY_BUILT), admin.join(BUILT));
    }
    git_until(
        root,
        &["worktree", "lock", "--reason", LOCK_REASON, &there],
        until,
    )?;
    Ok(format!(
        "moved the release run's checkout from {at} to {there}, where a build older than the \
         rename to `just release` made it"
    ))
}

/// Remove the git locks a killed git left in the admin folder `admin`, when no process holds one
/// open: what it removed. One held open stops the run.
fn stale_locks(admin: &Path) -> Result<Vec<String>, String> {
    let mut removed = Vec::new();
    for name in GIT_LOCKS {
        let lock = admin.join(name);
        if fs::symlink_metadata(&lock).is_err() {
            continue;
        }
        let holders = group::holders(&lock)
            .map_err(|e| format!("cannot tell whether {} is held: {e}", lock.display()))?;
        if !holders.is_empty() {
            let pids: Vec<String> = holders.iter().map(u32::to_string).collect();
            return Err(format!(
                "{} is held by process {}: a git runs in the release run's checkout",
                lock.display(),
                pids.join(", ")
            ));
        }
        fs::remove_file(&lock).map_err(|e| crate::text::os_error(&e, &lock))?;
        removed.push(format!(
            "removed {}, which an interrupted git left and no process holds",
            lock.display()
        ));
    }
    Ok(removed)
}

/// Clear the build folder `target` once the time `stamp` holds is [`BUILD_KEPT_DAYS`] before
/// `now`, and stamp `now`; a folder never stamped is stamped `now`. What it did, if anything.
pub(crate) fn prune_build(target: &Path, stamp: &Path, now: f64) -> Option<String> {
    let since = fs::read_to_string(stamp)
        .ok()
        .and_then(|said| said.trim().parse::<f64>().ok());
    let Some(since) = since else {
        let _ = fs::write(stamp, format!("{now}\n"));
        return None;
    };
    if now - since < BUILD_KEPT_DAYS * 86_400.0 {
        return None;
    }
    let _ = fs::write(stamp, format!("{now}\n"));
    match fs::symlink_metadata(target) {
        Err(_) => None,
        Ok(_) => Some(match fs::remove_dir_all(target) {
            Ok(()) => format!(
                "cleared {}, built on for {BUILD_KEPT_DAYS} days or more",
                target.display()
            ),
            Err(e) => format!(
                "cannot clear {}: {}",
                target.display(),
                crate::text::os_error(&e, target)
            ),
        }),
    }
}

impl Place {
    /// Bring the checkout to the commit `to`, clean, by `until`: made the first time (a detached
    /// worktree, locked and marked), later reset, every untracked and ignored file gone but its
    /// build folder (`.target/`), which keeps a run's build incremental until it is
    /// [`BUILD_KEPT_DAYS`] old at `now`. What it noted on the way.
    pub(crate) fn checkout(
        &mut self,
        root: &Path,
        common: &Path,
        to: &str,
        until: Bound<'_>,
        now: f64,
    ) -> Result<Vec<String>, String> {
        let path = self.checkout.clone();
        let at = path.to_string_lossy().into_owned();
        let mut noted = Vec::new();
        let exists = fs::symlink_metadata(&path).is_ok();
        match &self.admin {
            Some(admin) if exists => {
                noted.extend(stale_locks(admin)?);
                git_until(
                    &path,
                    &["checkout", "--quiet", "--force", "--detach", to],
                    until,
                )?;
                git_until(&path, &["clean", "-ffdxq", "-e", "/.target/"], until)?;
            }
            registered => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|e| crate::text::os_error(&e, parent))?;
                }
                let mut add = vec!["worktree", "add", "--quiet", "--detach", "--no-checkout"];
                if registered.is_some() {
                    // Its folder is gone but its registration, locked as the release run's, is not.
                    add.extend(["-f", "-f"]);
                }
                add.extend(["--lock", "--reason", LOCK_REASON, &at, to]);
                git_until(root, &add, until)?;
                let admin = admin(common, &path)
                    .ok_or_else(|| format!("git made no admin folder for {at}"))?;
                let marker = admin.join(MARK);
                fs::write(&marker, format!("{at}\n"))
                    .map_err(|e| crate::text::os_error(&e, &marker))?;
                self.admin = Some(admin);
                git_until(&path, &["reset", "--quiet", "--hard", to], until)?;
            }
        }
        let changed = changed(&path, to, until)?;
        if !changed.is_empty() {
            return Err(format!(
                "{} is not a clean checkout of {}: {}",
                path.display(),
                short(to),
                changed.join(", ")
            ));
        }
        if let Some(admin) = &self.admin {
            noted.extend(prune_build(&path.join(".target"), &admin.join(BUILT), now));
        }
        Ok(noted)
    }
}

/// How the checkout at `path` differs from a clean checkout of the commit `to`, within `until`:
/// another HEAD, and each path `git status --porcelain` names outside the build folder (`.target/`,
/// untracked where the repository does not ignore it). None when it is `to`, clean. What git
/// ignores is never named.
pub(crate) fn changed(path: &Path, to: &str, until: Bound<'_>) -> Result<Vec<String>, String> {
    let mut changed = Vec::new();
    let head = commit(path, "HEAD")?;
    if head != to {
        changed.push(format!("HEAD is {}", short(&head)));
    }
    let status = git_until(path, &["status", "--porcelain"], until)?;
    changed.extend(
        (status.lines())
            .filter(|line| {
                let named = line.get(3..).unwrap_or_default();
                named != ".target/" && !named.starts_with(".target/")
            })
            .map(|line| line.trim_start().to_string()),
    );
    Ok(changed)
}

#[cfg(test)]
#[path = "place_tests.rs"]
mod tests;
