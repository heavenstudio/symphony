//! The git common dir every worktree of the repository shares, where the board's server keeps its
//! data. It is read from the `.git` files themselves, with no `git` process, as a hook that runs on
//! every tool call needs; a layout it does not know asks git.

use std::fmt;
use std::path::{Path, PathBuf};

/// `cwd` is in no git repository.
#[derive(Debug, Clone, PartialEq)]
pub struct NotARepository(pub String);

impl fmt::Display for NotARepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn common_dir(cwd: &Path) -> Result<PathBuf, NotARepository> {
    match read(cwd) {
        Some(found) => Ok(found),
        None => ask_git(cwd),
    }
}

/// The nearest `.git` above `cwd`: a folder is the common dir, a worktree's file names its gitdir,
/// whose `commondir` names the common dir. None for anything else, or anything unreadable: what a
/// caller that may start no `git` process reads ([`crate::slots::of_env`], on every command).
pub(crate) fn read(cwd: &Path) -> Option<PathBuf> {
    let here = resolve_existing(cwd)?;
    for folder in here.ancestors() {
        let dot = folder.join(".git");
        if dot.symlink_metadata().is_ok() {
            return checkout_common_dir(folder);
        }
    }
    None
}

/// The checkout `cwd` is in, read as [`read`] reads, with no `git` process: its top folder (the
/// nearest above `cwd` holding a `.git`), and the branch checked out there, "" for a detached head.
/// None for a folder in no checkout, or whose nearest marker cannot identify its repository.
pub fn checkout(cwd: &Path) -> Option<(PathBuf, String)> {
    let here = cwd.canonicalize().ok()?;
    for folder in here.ancestors() {
        let dot = folder.join(".git");
        if dot.symlink_metadata().is_err() {
            continue;
        }
        checkout_common_dir(folder)?;
        let gitdir = if dot.is_dir() {
            dot
        } else if dot.is_file() {
            let text = std::fs::read_to_string(&dot).ok()?;
            folder.join(text.trim().strip_prefix("gitdir:")?.trim())
        } else {
            return None;
        };
        let head = std::fs::read_to_string(gitdir.join("HEAD")).ok()?;
        let branch = head.trim().strip_prefix("ref: refs/heads/").unwrap_or("");
        return Some((folder.to_path_buf(), branch.to_string()));
    }
    None
}

/// Whether `top` is a linked worktree's top folder (its `.git` is a file), never the main checkout,
/// whose `.git` is the common dir itself.
pub fn linked(top: &Path) -> bool {
    top.join(".git").is_file()
}

/// Canonicalize the existing prefix, retaining missing child components for a deleted event cwd.
fn resolve_existing(path: &Path) -> Option<PathBuf> {
    if let Ok(real) = path.canonicalize() {
        return Some(real);
    }
    let name = path.file_name()?;
    Some(resolve_existing(path.parent()?)?.join(name))
}

/// The common dir of this checkout's own marker alone, with no parent lookup or child process.
/// A separate git dir (a submodule's too) needs no `commondir`; a dangling or invalid marker
/// never falls through to an enclosing repository.
pub fn checkout_common_dir(top: &Path) -> Option<PathBuf> {
    let top = top.canonicalize().ok()?;
    let dot = top.join(".git");
    let marker = std::fs::metadata(&dot).ok()?;
    if marker.is_dir() {
        return dot.canonicalize().ok();
    }
    if !marker.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(dot).ok()?;
    let destination = text.trim().strip_prefix("gitdir:")?.trim();
    if destination.is_empty() {
        return None;
    }
    let gitdir = top.join(destination).canonicalize().ok()?;
    if !gitdir.is_dir() {
        return None;
    }
    match std::fs::read_to_string(gitdir.join("commondir")) {
        Ok(common) => gitdir
            .join(common.trim())
            .canonicalize()
            .ok()
            .filter(|p| p.is_dir()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(gitdir),
        Err(_) => None,
    }
}

fn ask_git(cwd: &Path) -> Result<PathBuf, NotARepository> {
    let not = |why: String| {
        NotARepository(format!(
            "{} is not inside a git repository ({why})",
            cwd.display()
        ))
    };
    let out = crate::sys::output(crate::sys::command("git").arg("-C").arg(cwd).args([
        "rev-parse",
        "--path-format=absolute",
        "--git-common-dir",
    ]))
    .map_err(|problem| not(problem.to_string()))?;
    if !out.status.success() {
        return Err(not(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    let found = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    found
        .canonicalize()
        .map_err(|problem| not(problem.to_string()))
}
