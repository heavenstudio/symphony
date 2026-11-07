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
/// whose `commondir` names the common dir. None for anything else, or anything unreadable.
fn read(cwd: &Path) -> Option<PathBuf> {
    let here = cwd.canonicalize().ok()?;
    for folder in here.ancestors() {
        let dot = folder.join(".git");
        if dot.is_dir() {
            return dot.canonicalize().ok();
        }
        if dot.is_file() {
            let text = std::fs::read_to_string(&dot).ok()?;
            let gitdir = folder.join(text.trim().strip_prefix("gitdir:")?.trim());
            let common = std::fs::read_to_string(gitdir.join("commondir")).ok()?;
            return gitdir.join(common.trim()).canonicalize().ok();
        }
    }
    None
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
