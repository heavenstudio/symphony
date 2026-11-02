//! Card helpers shared by the landing gate and Symphony. The gate depends on symphony, never
//! the reverse. A verdict branch qualifies for the document tier only if its generated files are
//! exactly what the commands make from its base plus its documents.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

/// Retry diagnostics shared by the lander and the app that keeps verified verdicts queued.
pub const RETRY_LAND: &str = "retry just land";
pub const DIRTY_MAIN: &str = "has staged or tracked edits; refusing publication";

fn git(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let done = Command::new("git")
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    done.status.success().then_some(done.stdout)
}

fn text(root: &Path, args: &[&str]) -> Option<String> {
    git(root, args).map(|out| String::from_utf8_lossy(&out).into_owned())
}

/// `docs/**/*.md`, with no hidden or dot path component, as the verdict exemption admits it.
pub fn is_doc(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("docs/") else {
        return false;
    };
    let parts: Vec<_> = rest.split('/').collect();
    parts
        .last()
        .is_some_and(|name| name.len() > 3 && name.ends_with(".md"))
        && parts.iter().all(|p| !p.is_empty() && !p.starts_with('.'))
}

/// Documents holding an owner answer no commit has. An unreadable checkout is kept by the caller's
/// status check; disappearing checkouts are skipped, never allowed to crash the sweep.
pub fn owner_lines_left(root: &Path) -> Vec<String> {
    // This is another agent's checkout, unlike the generators' private worktrees. A diff may
    // refresh its index despite GIT_OPTIONAL_LOCKS, so disable that refresh explicitly.
    let diff = Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["-c", "diff.autoRefreshIndex=false"])
        .args(["diff", "-U0", "HEAD", "--", "docs"])
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|done| done.status.success())
        .map(|done| String::from_utf8_lossy(&done.stdout).into_owned())
        .unwrap_or_default();
    let mut file = "";
    let mut left = Vec::new();
    for line in diff.lines() {
        if let Some(name) = line.strip_prefix("+++ b/") {
            file = name;
        } else if line.starts_with('+')
            && !line.starts_with("+++")
            && line.split('—').next().unwrap_or("").contains("] owner")
            && !left.iter().any(|name| name == file)
        {
            left.push(file.to_string());
        }
    }
    left
}

static NEXT: AtomicU64 = AtomicU64::new(0);
// Git registers worktrees in several writes. Our two verdict workers must not list or mutate that
// registration while another of their commands has only half written it.
pub(crate) static WORKTREES: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A private worktree whose own files and registration go away together on every exit.
pub struct PrivateWorktree {
    root: PathBuf,
    home: PathBuf,
    pub path: PathBuf,
    retained: bool,
}

impl PrivateWorktree {
    pub fn new(root: &Path, rev: &str) -> Result<Self, String> {
        let home = loop {
            let home = std::env::temp_dir().join(format!(
                "verdict-outputs-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&home) {
                Ok(()) => break home,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        };
        let held = Self {
            root: root.to_path_buf(),
            path: home.join("wt"),
            home,
            retained: false,
        };
        let _held = WORKTREES.lock().unwrap_or_else(|e| e.into_inner());
        let done = Command::new("git")
            .args(["worktree", "add", "--detach"])
            .arg(&held.path)
            .arg(rev)
            .current_dir(root)
            .output()
            .map_err(|e| e.to_string())?;
        if !done.status.success() {
            return Err(format!(
                "could not make a worktree of {rev}: {}",
                String::from_utf8_lossy(&done.stderr).trim()
            ));
        }
        Ok(held)
    }

    /// A queued publication keeps this checkout and its verifier records across worker shutdown.
    pub(crate) fn retain(&mut self) {
        self.retained = true;
    }

    pub(crate) fn resume(root: &Path, path: PathBuf) -> Result<Self, String> {
        let home = path.parent().ok_or("missing publication checkout parent")?;
        let common = ["rev-parse", "--path-format=absolute", "--git-common-dir"];
        if path.file_name().is_none_or(|name| name != "wt")
            || !home
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("verdict-outputs-"))
            || text(root, &common).is_none()
            || text(root, &common) != text(&path, &common)
        {
            return Err(format!(
                "invalid retained publication checkout {}",
                path.display()
            ));
        }
        Ok(Self {
            root: root.to_path_buf(),
            home: home.to_path_buf(),
            path,
            retained: true,
        })
    }

    pub(crate) fn remove(mut self) {
        self.retained = false;
    }
}

impl Drop for PrivateWorktree {
    fn drop(&mut self) {
        if self.retained {
            return;
        }
        let _held = WORKTREES.lock().unwrap_or_else(|e| e.into_inner());
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&self.path)
            .current_dir(&self.root)
            .output();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

/// A generator run as a command in a private worktree; heavy outputs run only after an earlier
/// generator changed something. This is also the shape the review-app port can use.
pub struct Generator {
    pub name: String,
    pub outputs: String,
    pub command: Vec<String>,
    pub when_changed: bool,
}

pub fn default_generators() -> Vec<Generator> {
    vec![
        Generator {
            name: "content".into(),
            outputs: "content/".into(),
            command: vec!["just".into(), "content".into()],
            when_changed: false,
        },
        Generator {
            name: "hull-atlas".into(),
            outputs: "view/hull_atlas".into(),
            command: vec!["just".into(), "hull-atlas".into(), "build".into()],
            when_changed: true,
        },
    ]
}

fn changed(root: &Path) -> Option<Vec<String>> {
    // NUL framing retains spaces, quotes and non-ASCII file names exactly.
    let status = git(root, &["status", "--porcelain", "-z", "-uall"])?;
    let mut records = status.split(|b| *b == 0).filter(|s| !s.is_empty());
    let mut names = Vec::new();
    while let Some(record) = records.next() {
        names.push(String::from_utf8_lossy(record.get(3..)?).to_string());
        if record.first() == Some(&b'R') || record.first() == Some(&b'C') {
            records.next();
        }
    }
    Some(names)
}

pub fn regenerates(root: &Path, branch: &str, generators: &[Generator]) -> bool {
    regenerates_inner(root, branch, generators).unwrap_or(false)
}

fn regenerates_inner(root: &Path, branch: &str, generators: &[Generator]) -> Option<bool> {
    let tip = text(
        root,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("{branch}^{{commit}}"),
        ],
    )?;
    let tip = tip.trim();
    let common = text(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    if !Path::new(common.trim())
        .join("sf-land/verdicts")
        .join(tip)
        .exists()
    {
        return Some(false);
    }
    let base = text(root, &["merge-base", "main", tip])?;
    let names = git(
        root,
        &[
            "diff",
            "--no-renames",
            "--name-only",
            "-z",
            base.trim(),
            tip,
        ],
    )?;
    let names: Vec<_> = names
        .split(|b| *b == 0)
        .filter(|n| !n.is_empty())
        .map(|n| String::from_utf8_lossy(n).to_string())
        .collect();
    let (docs, mut outputs): (Vec<_>, Vec<_>) = names.into_iter().partition(|name| is_doc(name));
    if outputs.is_empty()
        || outputs
            .iter()
            .any(|n| !generators.iter().any(|g| n.starts_with(&g.outputs)))
    {
        return Some(false);
    }
    let wt = PrivateWorktree::new(root, base.trim()).ok()?;
    for doc in &docs {
        let shown = git(root, &["show", &format!("{tip}:{doc}")])?;
        let path = wt.path.join(doc);
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(path, shown).ok()?;
    }
    let mut wrote = false;
    for generator in generators {
        if generator.when_changed && !wrote {
            continue;
        }
        let (program, args) = generator.command.split_first()?;
        if !Command::new(program)
            .args(args)
            .current_dir(&wt.path)
            .stdin(Stdio::null())
            .output()
            .ok()?
            .status
            .success()
        {
            return Some(false);
        }
        wrote |= changed(&wt.path)?
            .iter()
            .any(|n| n.starts_with(&generator.outputs));
    }
    let mut made: Vec<_> = changed(&wt.path)?
        .into_iter()
        .filter(|n| !docs.contains(n))
        .collect();
    made.sort();
    outputs.sort();
    if made != outputs {
        return Some(false);
    }
    for name in outputs {
        if git(root, &["show", &format!("{tip}:{name}")])?
            != std::fs::read(wt.path.join(name)).ok()?
        {
            return Some(false);
        }
    }
    Some(true)
}
