//! File-retention reasons, disposable output and the activity timestamps a sweep reads.

use super::{locks::saved_locks, stdout, succeeds};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

/// Ignored files that go with a merged worktree, as `land` lets them go with a landed one: caches a
/// checkout rebuilds, export output, and the golden images verify writes. The logs verify writes at
/// the top of the project's artifacts folder go too ([`left_behind`]).
pub const DISPOSABLE: [&str; 18] = [
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
    "*/.DS_Store",
    "tests/golden/*.actual.png",
    "tests/golden/*.expected.png",
    "tests/golden/*.diff.png",
];

/// Additional cache paths released when the worktree's work is finished (every task tied to it done
/// or cancelled, or none). What it ignores under the project's artifacts folder is released too,
/// set aside in the main checkout before the worktree goes (`sweep::aside`).
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

/// What an operating system's file manager writes into a folder a person opens there, at any depth:
/// nobody's work, so it goes with a worktree whether the project's ignore file names it or not. A
/// worktree reads the ignore file of its own branch, so a line added on the default branch later
/// reaches none that exists.
const OS_METADATA: [&str; 6] = [
    ".DS_Store",
    "*/.DS_Store",
    "Thumbs.db",
    "*/Thumbs.db",
    "desktop.ini",
    "*/desktop.ini",
];

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
/// are not disposable. `artifacts` is the project's artifacts folder (none: the project file does
/// not say, and nothing under any folder is evidence): the logs at its top go, and a worktree whose
/// work is `finished` lets what is ignored in it go, as it does its caches ([`FINISHED_OUTPUT`]). A
/// status git cannot give is something left behind: it keeps the worktree.
pub fn left_behind(path: &Path, finished: bool, artifacts: Option<&str>) -> Vec<String> {
    left_behind_with(path, finished, artifacts, &[])
}

/// [`left_behind`] where the project names ignored paths of its own that go with a merged
/// worktree: `disposable`, its `[board] sweep_disposable` ([`crate::project::disposable`]), each
/// matched as the board's own are ([`DISPOSABLE`]) and, as they are, nowhere under the artifacts
/// folder, nor a folder holding it, which git names whole.
pub fn left_behind_with(
    path: &Path,
    finished: bool,
    artifacts: Option<&str>,
    disposable: &[String],
) -> Vec<String> {
    file_status(path, finished, artifacts, disposable).left
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

/// Whether `name` is a file manager's metadata ([`OS_METADATA`]), untracked or ignored.
pub(in crate::sweep) fn os_metadata(name: &str) -> bool {
    OS_METADATA.iter().any(|pattern| fnmatch(pattern, name))
}

fn ignored_disposable(
    name: &str,
    finished: bool,
    artifacts: Option<&str>,
    project: &[String],
) -> bool {
    // What is below the artifacts folder, `` for the folder itself.
    let below = artifacts.and_then(|folder| {
        let rest = name.strip_prefix(folder)?;
        if rest.is_empty() || rest == "/" {
            Some("")
        } else {
            rest.strip_prefix('/')
        }
    });
    let root_log = below.is_some_and(|tail| !tail.contains('/') && tail.ends_with(".log"));
    let goes = |patterns: &[&str]| patterns.iter().any(|pattern| fnmatch(pattern, name));
    // Git names an ignored folder whole (`out/`), so one that holds the artifacts folder
    // (`out/shots`) holds its evidence, which a project's pattern never lets go with it: the
    // evidence is set aside from the folder's own names alone (`sweep::aside`).
    let holds_artifacts = artifacts.is_some_and(|folder| {
        name.ends_with('/') && format!("{}/", folder.trim_end_matches('/')).starts_with(name)
    });
    let named = !holds_artifacts && project.iter().any(|pattern| fnmatch(pattern, name));
    let disposable = below.is_none() && (goes(&DISPOSABLE) || named);
    root_log || disposable || (finished && (goes(&FINISHED_OUTPUT) || below.is_some()))
}

pub(in crate::sweep) fn file_status(
    path: &Path,
    finished: bool,
    artifacts: Option<&str>,
    disposable: &[String],
) -> FileStatus {
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
        if (matches!(status, "!!" | "??") && (regenerable(named) || os_metadata(named)))
            || (status == "!!" && ignored_disposable(named, finished, artifacts, disposable))
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

/// Remove only untracked regenerable output and a file manager's metadata ([`OS_METADATA`]), so
/// the non-forced worktree removal can still let Git protect every tracked edit and every other
/// untracked file. A pathspec's `*` crosses `/`, so each metadata name is cleaned at any depth.
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
            ".DS_Store",
            "*/.DS_Store",
            "Thumbs.db",
            "*/Thumbs.db",
            "desktop.ini",
            "*/desktop.ini",
        ],
    )
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
    fn the_projects_artifacts_folder_is_evidence_and_its_top_logs_go() {
        for folder in ["artifacts", "out/shots"] {
            let at = |rest: &str| format!("{folder}{rest}");
            let goes = |name: &str, finished| ignored_disposable(name, finished, Some(folder), &[]);
            assert!(goes(&at("/verify.log"), false), "{folder}: a top log goes");
            assert!(!goes(&at("/deep/verify.log"), false), "{folder}");
            assert!(!goes(&at("/a.png"), false), "{folder}: open work keeps it");
            for name in [at(""), at("/"), at("/a.png"), at("/deep/x/")] {
                assert!(goes(&name, true), "{folder}: finished work releases {name}");
            }
            assert!(!goes(&at("x/a.png"), true), "{folder}x is another folder");
        }
        // The folder is the project's alone: another one named artifacts is none of it.
        let other = |name: &str| ignored_disposable(name, true, Some("out/shots"), &[]);
        assert!(!other("artifacts/a.png"));
        assert!(!other("artifacts/verify.log"));
        // With no folder the project file names, nothing is evidence.
        for name in ["artifacts/a.png", "artifacts/verify.log"] {
            assert!(!ignored_disposable(name, true, None, &[]), "{name}");
        }
        assert!(ignored_disposable(".DS_Store", false, None, &[]));
    }

    #[test]
    fn a_file_managers_metadata_is_named_at_any_depth_and_nothing_like_it_is() {
        for name in [".DS_Store", "sim/.DS_Store", "a/b/Thumbs.db", "desktop.ini"] {
            assert!(os_metadata(name), "{name}");
        }
        for name in [
            "notes.DS_Store",
            ".DS_Store/",
            "sim/.DS_Store.bak",
            "Desktop.ini.txt",
        ] {
            assert!(!os_metadata(name), "{name}");
        }
    }

    #[test]
    fn what_the_project_names_goes_as_the_boards_own_does_and_never_its_artifacts() {
        let project = ["node_modules/", "*/node_modules/", "*.tsbuildinfo"].map(String::from);
        let goes = |name: &str| ignored_disposable(name, false, Some("artifacts"), &project);
        for name in [
            "node_modules/",
            "web/node_modules/",
            "apps/web/node_modules/",
            "tsconfig.tsbuildinfo",
            "web/tsconfig.app.tsbuildinfo",
        ] {
            assert!(goes(name), "{name}");
            assert!(
                !ignored_disposable(name, true, Some("artifacts"), &[]),
                "{name}: no project names it, and it is kept"
            );
        }
        // A file beside the folder, another folder and a local setting are none of them.
        for name in ["node_modules.txt", "dist/", ".env", "web/node_modules/x.js"] {
            assert!(!goes(name), "{name}");
        }
        // Open work's evidence stays evidence, whatever the project names.
        assert!(!goes("artifacts/node_modules/"));
        assert!(!goes("artifacts/shots/tsconfig.tsbuildinfo"));
    }

    #[test]
    fn what_the_project_names_never_takes_a_folder_that_holds_its_artifacts() {
        // Git names an ignored folder whole: `out/` stands for everything in it, evidence too.
        let project = ["out/", "o*/", "*/node_modules/"].map(String::from);
        let goes =
            |name: &str, finished| ignored_disposable(name, finished, Some("out/shots"), &project);
        for finished in [false, true] {
            assert!(!goes("out/", finished), "finished: {finished}");
        }
        // Beside the folder, inside the same parent, and beyond it, what the project names goes.
        for name in [
            "out/cache/",
            "out/web/node_modules/",
            "outer/",
            "web/node_modules/",
        ] {
            assert!(goes(name, false), "{name}");
        }
        // A folder whose name only begins like the parent's holds none of it.
        let shots = |name: &str| ignored_disposable(name, false, Some("out/shots"), &project);
        assert!(shots("o/"), "o/ matches o*/ and holds no out/shots");
        // With no artifacts folder there is no evidence to hold, and the named parent goes.
        assert!(ignored_disposable("out/", false, None, &project));
    }

    #[test]
    fn nothing_readable_is_touched_now() {
        let gone = std::env::temp_dir().join(format!("symphony-sweep-gone-{}", std::process::id()));
        assert_eq!(last_touched(&gone, 1234.5), 1234.5);
    }
}
