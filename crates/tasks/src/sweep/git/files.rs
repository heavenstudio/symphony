//! File-retention reasons, disposable output and the activity timestamps a sweep reads.

use super::{locks::saved_locks, stdout, succeeds};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

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

/// Additional cache paths released when the worktree's work is finished (every task tied to it done
/// or cancelled, or none). What it ignores under `artifacts/` is released too, set aside in the main
/// checkout before the worktree goes (`sweep::aside`).
pub const FINISHED_OUTPUT: [&str; 3] = [".godot/*", ".venv/*", ".target/*"];

/// Build output, untracked or ignored, that goes with a worktree: its build folders, what a seed
/// staged or a clear set aside beside one (`.target.seed.PID`, `.target.clear.PID`), the client's
/// build and the Codex app's imported hooks.
const REGENERABLE: [&str; 11] = [
    ".target/",
    ".target/*",
    ".target.seed.*",
    ".target.clear.*",
    "symphony/.target/",
    "symphony/.target/*",
    "symphony/.target.seed.*",
    "symphony/.target.clear.*",
    "symphony/build/",
    "symphony/build/*",
    ".codex/hooks*",
];

/// Where the owner's verdicts are written into a worktree.
const DOCS: &str = "docs";
/// What marks an owner's line in a register: `[2026-10-06 10:00] owner · approved — …`.
const OWNER_LINE: &str = "] owner";

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
    file_status(path, finished).left
}

/// Keep the native ignored-directory decisions beside the verdict, so a clean proof can prune
/// known disposable output without a second recursive Git enumeration.
pub(in crate::sweep) struct FileStatus {
    pub left: Vec<String>,
    pub ignored_dirs: Vec<String>,
}

impl FileStatus {
    fn blocked(reason: &str) -> Self {
        Self {
            left: vec![reason.into()],
            ignored_dirs: Vec::new(),
        }
    }
}

pub(in crate::sweep) fn regenerable(name: &str) -> bool {
    REGENERABLE.iter().any(|pattern| fnmatch(pattern, name))
}

fn ignored_disposable(name: &str, finished: bool) -> bool {
    let root_log = name
        .strip_prefix("artifacts/")
        .is_some_and(|tail| !tail.contains('/') && tail.ends_with(".log"));
    let goes = |patterns: &[&str]| patterns.iter().any(|pattern| fnmatch(pattern, name));
    let disposable = !name.starts_with("artifacts/") && goes(&DISPOSABLE);
    let evidence = name == "artifacts" || name.starts_with("artifacts/");
    root_log || disposable || (finished && (goes(&FINISHED_OUTPUT) || evidence))
}

pub(in crate::sweep) fn file_status(path: &Path, finished: bool) -> FileStatus {
    let Some(text) = stdout(
        path,
        &[
            "status",
            "--porcelain",
            "--ignored=matching",
            "--untracked-files=all",
        ],
    ) else {
        return FileStatus::blocked("(git status could not be read there)");
    };
    let Some(mut kept) = saved_locks(path) else {
        return FileStatus::blocked("(worktree lock state could not be read there)");
    };
    let mut ignored_dirs = Vec::new();
    for line in text.lines() {
        let status = line.get(..2).unwrap_or(line);
        let raw = line.get(3..).unwrap_or("");
        let named = raw.trim_matches('"');
        if (matches!(status, "!!" | "??") && regenerable(named))
            || (status == "!!" && ignored_disposable(named, finished))
        {
            // Quoted or non-ASCII status paths need a lossless decoder before they can justify
            // pruning. Leave those for the bounded inventory to read or conservatively refuse.
            if status == "!!"
                && named.ends_with('/')
                && raw == named
                && named.is_ascii()
                && !named.contains('\\')
                && !named.chars().any(char::is_control)
            {
                ignored_dirs.push(named.into());
            }
            continue;
        }
        kept.push(named.to_string());
    }
    FileStatus {
        left: kept,
        ignored_dirs,
    }
}

/// Remove only untracked regenerable output, so the non-forced worktree removal can still let
/// Git protect every tracked edit and every other untracked file.
pub(in crate::sweep) fn clean_regenerable(path: &Path) -> bool {
    succeeds(
        path,
        &[
            "clean",
            "-fdx",
            "--",
            ".target/",
            ".target.seed.*",
            ".target.clear.*",
            "symphony/.target/",
            "symphony/.target.seed.*",
            "symphony/.target.clear.*",
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
pub(in crate::sweep) fn admin(path: &Path) -> Option<PathBuf> {
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
    fn nothing_readable_is_touched_now() {
        let gone = std::env::temp_dir().join(format!("sf-sweep-gone-{}", std::process::id()));
        assert_eq!(last_touched(&gone, 1234.5), 1234.5);
    }
}
