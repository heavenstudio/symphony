//! What the sweep asks git and the file system about a repository's worktrees: the listing, what a
//! worktree holds that did not land (`land._left_behind`'s measure), an owner verdict no commit has
//! (`docs_worktrees.owner_lines_left`), and when an agent last touched it.

use crate::sys::{Command, Stdio};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;
use std::{fs, os::unix::fs::MetadataExt};

/// Ignored files that go with a merged worktree, as `land` lets them go with a landed one: caches a
/// checkout rebuilds, export output, and the logs and golden images verify writes.
pub const DISPOSABLE: [&str; 19] = [
    ".godot/",
    ".godot/*",
    ".target/",
    "symphony/.target/",
    "symphony/build/",
    ".codex/hooks*",
    ".venv/",
    ".venv/*",
    "build/*",
    "__pycache__/",
    "*/__pycache__/",
    "*.pyc",
    "*.import",
    ".DS_Store",
    "artifacts/*.log",
    "*/.DS_Store",
    "tests/golden/*.actual.png",
    "tests/golden/*.expected.png",
    "tests/golden/*.diff.png",
];

/// Additional cache paths released when the worktree's task is finished.
pub const FINISHED_OUTPUT: [&str; 3] = [".godot/*", ".venv/*", ".target/*"];

const REGENERABLE: [&str; 7] = [
    ".target/",
    ".target/*",
    "symphony/.target/",
    "symphony/.target/*",
    "symphony/build/",
    "symphony/build/*",
    ".codex/hooks*",
];

/// Where the owner's verdicts are written into a worktree.
const DOCS: &str = "docs";
/// What marks an owner's line in a register: `[2026-10-06 10:00] owner · approved — …`.
const OWNER_LINE: &str = "] owner";

/// One worktree as `git worktree list --porcelain` lists it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Listed {
    pub path: PathBuf,
    /// "" when it has none (a bare repository).
    pub head: String,
    /// `refs/heads/NAME`; none when detached.
    pub branch: Option<String>,
    /// The lock's reason, "" for a lock with none; none when it is not locked.
    pub locked: Option<String>,
    pub bare: bool,
    pub prunable: bool,
}

/// `git ARGS` in `dir`: none when git cannot be started there (the folder is gone).
pub fn git(dir: &Path, args: &[&str]) -> Option<Output> {
    crate::sys::output(command(dir).args(args)).ok()
}

fn command(dir: &Path) -> Command {
    let mut command = crate::sys::command("git");
    command.current_dir(dir).stdin(Stdio::null());
    // A hook or launcher can inherit the caller's repository and index. Each sweep operation
    // must instead discover the repository belonging to its explicit working directory.
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_PREFIX",
    ] {
        command.env_remove(name);
    }
    command
}

/// What `git ARGS` printed, when it succeeded.
fn stdout(dir: &Path, args: &[&str]) -> Option<String> {
    // These reads inspect other agents' trees. A diff can refresh the index even when
    // optional locks are disabled, so both settings are needed to leave the writer's index alone.
    crate::sys::output(
        command(dir)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args(["-c", "diff.autoRefreshIndex=false"])
            .args(args),
    )
    .ok()
    .filter(|done| done.status.success())
    .map(|done| String::from_utf8_lossy(&done.stdout).into_owned())
}

/// Whether `git ARGS` succeeded.
pub fn succeeds(dir: &Path, args: &[&str]) -> bool {
    git(dir, args).is_some_and(|done| done.status.success())
}

/// The commit `rev` names, if it names one.
pub fn rev(dir: &Path, rev: &str) -> Option<String> {
    stdout(dir, &["rev-parse", "--verify", "--quiet", rev]).map(|out| out.trim().to_string())
}

/// A Git writer's lock, held until the final removal has finished. Never unlink a replacement
/// created after Git removed the original admin directory and another worktree reused its name.
pub(super) struct RefLock {
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

const SAVED_LOCK: &str = ".sf-sweep-lock-";
static LOCK_NAMES: AtomicU64 = AtomicU64::new(0);

fn lock_path(dir: &Path) -> Option<PathBuf> {
    stdout(dir, &["rev-parse", "--path-format=absolute", "--git-dir"])
        .map(|p| PathBuf::from(p.trim()).join("locked"))
}

/// The exact lock that was classified dead. Keeping its descriptor open prevents inode reuse.
pub(super) struct WorktreeLock {
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

    pub(super) fn capture(dir: &Path, reason: &str) -> Option<Self> {
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
    pub(super) fn release(self, pause: impl Fn(&str)) -> Option<ReleasedLock> {
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
pub(super) struct ReleasedLock {
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

fn saved_locks(dir: &Path) -> Option<Vec<String>> {
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
    saved.sort();
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
pub(super) fn hold_head(dir: &Path) -> Option<Vec<RefLock>> {
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

/// Whether `rev` is `main` or behind it.
pub fn ancestor(root: &Path, rev: &str, main: &str) -> bool {
    succeeds(root, &["merge-base", "--is-ancestor", rev, main])
}

/// The top of the worktree `cwd` is in, or git's own words for why there is none.
pub fn toplevel(cwd: &Path) -> Result<PathBuf, String> {
    let args = ["rev-parse", "--show-toplevel"];
    let done = git(cwd, &args)
        .ok_or_else(|| format!("git {}: {} cannot be read", args.join(" "), cwd.display()))?;
    if !done.status.success() {
        let said = String::from_utf8_lossy(&done.stderr).trim().to_string();
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            if said.is_empty() { "failed" } else { &said }
        ));
    }
    Ok(PathBuf::from(
        String::from_utf8_lossy(&done.stdout).trim().to_string(),
    ))
}

/// Every worktree of `root`'s repository, the main checkout first, as git lists them.
pub fn worktrees(root: &Path) -> Result<Vec<Listed>, String> {
    let done = git(root, &["worktree", "list", "--porcelain"])
        .filter(|done| done.status.success())
        .ok_or("git worktree list --porcelain failed")?;
    Ok(parse_worktrees(&String::from_utf8_lossy(&done.stdout)))
}

/// `git worktree list --porcelain` read: one block per worktree, a line per attribute.
pub fn parse_worktrees(text: &str) -> Vec<Listed> {
    let mut found = Vec::new();
    for block in text.trim().split("\n\n") {
        let mut entry = Listed::default();
        let mut named = false;
        for line in block.lines() {
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "worktree" => {
                    entry.path = PathBuf::from(value);
                    named = true;
                }
                "HEAD" => entry.head = value.to_string(),
                "branch" => entry.branch = Some(value.to_string()),
                "locked" => entry.locked = Some(value.to_string()),
                "bare" => entry.bare = true,
                "prunable" => entry.prunable = true,
                _ => {}
            }
        }
        if named {
            found.push(entry);
        }
    }
    found
}

/// Whether `text` matches `pattern` as Python's `fnmatch.fnmatchcase` matches it, for the patterns
/// above: `*` any run of characters (a `/` too), `?` any one; anything else itself.
pub fn fnmatch(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    // The classic two-pointer match, going back to the last `*` on a mismatch.
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((at, from)) = star {
            pi = at + 1;
            ti = from + 1;
            star = Some((at, from + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// What in worktree `path` did not land: tracked changes, untracked files, and ignored files that
/// are not disposable. A worktree whose work is `finished` lets its caches go too
/// ([`FINISHED_OUTPUT`]). A status git cannot give is something left behind: it keeps the worktree.
pub fn left_behind(path: &Path, finished: bool) -> Vec<String> {
    let Some(text) = stdout(
        path,
        &[
            "status",
            "--porcelain",
            "--ignored=matching",
            "--untracked-files=all",
        ],
    ) else {
        return vec!["(git status could not be read there)".into()];
    };
    let Some(mut kept) = saved_locks(path) else {
        return vec!["(worktree lock state could not be read there)".into()];
    };
    for line in text.lines() {
        let status = line.get(..2).unwrap_or(line);
        let named = line.get(3..).unwrap_or("").trim_matches('"');
        let goes = |patterns: &[&str]| patterns.iter().any(|p| fnmatch(p, named));
        let root_log = named
            .strip_prefix("artifacts/")
            .is_some_and(|name| !name.contains('/') && name.ends_with(".log"));
        let disposable = !named.starts_with("artifacts/") && goes(&DISPOSABLE);
        if matches!(status, "!!" | "??") && goes(&REGENERABLE) {
            continue;
        }
        if status == "!!" && (root_log || disposable || (finished && goes(&FINISHED_OUTPUT))) {
            continue;
        }
        kept.push(named.to_string());
    }
    kept
}

/// Remove only untracked regenerable output, so the non-forced worktree removal can still let
/// Git protect every tracked edit and every other untracked file.
pub(super) fn clean_regenerable(path: &Path) -> bool {
    succeeds(
        path,
        &[
            "clean",
            "-fdx",
            "--",
            ".target/",
            "symphony/.target/",
            "symphony/build/",
            ".codex/hooks*",
        ],
    )
}

/// The files under docs/ in worktree `path` that hold an owner line no commit has: the owner's
/// answer, written into the worktree's file, that removing the worktree would lose.
pub fn owner_lines_left(path: &Path) -> Vec<String> {
    let diff = stdout(path, &["diff", "-U0", "HEAD", "--", DOCS]).unwrap_or_default();
    let mut left: Vec<String> = Vec::new();
    let mut current = "";
    for line in diff.lines() {
        if let Some(file) = line.strip_prefix("+++ b/") {
            current = file;
        } else if line.starts_with('+')
            && !line.starts_with("+++")
            && line.split('—').next().unwrap_or("").contains(OWNER_LINE)
            && !left.iter().any(|f| f == current)
        {
            left.push(current.to_string());
        }
    }
    left
}

/// The latest time an agent at work in `path` changed what the sweep reads: its admin dir's HEAD,
/// index and logs/HEAD (a checkout, a stage, a commit) and its top folder (a file made or removed
/// there). Nothing readable (gone mid-sweep) is `now`: busy, never idle.
pub fn last_touched(path: &Path, now: f64) -> f64 {
    let admin = admin(path);
    let mut candidates = vec![path.to_path_buf()];
    if let Some(admin) = admin {
        candidates.extend(["HEAD", "index", "logs/HEAD"].map(|name| admin.join(name)));
    }
    candidates
        .iter()
        .filter_map(|candidate| modified(candidate))
        .reduce(f64::max)
        .unwrap_or(now)
}

/// A worktree's own admin folder, without spawning a process per worktree.
pub(super) fn admin(path: &Path) -> Option<PathBuf> {
    let dot = path.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = fs::read_to_string(dot).ok()?;
    let named = Path::new(text.trim().strip_prefix("gitdir: ")?);
    Some(if named.is_absolute() {
        named.to_path_buf()
    } else {
        path.join(named)
    })
}

/// When `path` was last modified, in seconds since the epoch.
pub fn modified(path: &Path) -> Option<f64> {
    let when = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(when.duration_since(UNIX_EPOCH).ok()?.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_star_matches_any_run_a_slash_included_and_the_rest_matches_itself() {
        for (pattern, text) in [
            ("artifacts/*", "artifacts/solar-swift/"),
            ("artifacts/*", "artifacts/a/b.json"),
            ("artifacts/*.log", "artifacts/slynx-verify.log"),
            ("*/__pycache__/", "sim/deep/__pycache__/"),
            ("*.pyc", "a/b.pyc"),
            (".godot/", ".godot/"),
            ("a?c", "abc"),
            ("**", ""),
        ] {
            assert!(fnmatch(pattern, text), "{pattern} {text}");
        }
        for (pattern, text) in [
            ("artifacts/*", "artifact/x"),
            (".godot/", ".godot/cache"),
            ("*.pyc", "a.pyc.txt"),
            ("a?c", "ac"),
            ("artifacts/*.log", "artifacts/probe.json"),
        ] {
            assert!(!fnmatch(pattern, text), "{pattern} {text}");
        }
    }

    #[test]
    fn the_listing_reads_each_worktrees_attributes() {
        let listed = parse_worktrees(
            "worktree /r\nHEAD aaa\nbranch refs/heads/main\n\nworktree /w\nHEAD bbb\ndetached\nlocked claude agent x (pid 1 start y)\n\nworktree /g\nHEAD ccc\ndetached\nlocked\nprunable gitdir file points to non-existent location\n\nworktree /b\nbare\n",
        );
        assert_eq!(listed.len(), 4);
        assert_eq!(listed[0].branch.as_deref(), Some("refs/heads/main"));
        assert_eq!(
            listed[1].locked.as_deref(),
            Some("claude agent x (pid 1 start y)")
        );
        assert_eq!(listed[1].branch, None);
        assert_eq!(
            (listed[2].locked.as_deref(), listed[2].prunable),
            (Some(""), true)
        );
        assert!(listed[3].bare && listed[3].head.is_empty());
    }

    #[test]
    fn nothing_readable_is_touched_now() {
        let gone = std::env::temp_dir().join(format!("sf-sweep-gone-{}", std::process::id()));
        assert_eq!(last_touched(&gone, 1234.5), 1234.5);
    }
}
