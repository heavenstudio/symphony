//! Card helpers shared by the landing gate and Symphony. The gate depends on symphony, never
//! the reverse. A verdict branch qualifies for the document tier only if its generated files are
//! exactly what the commands make from its base plus its documents.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use symphony_board::lock::Recovered as _;

use crate::children::command;

/// Retry diagnostics shared by the lander and the app that keeps verified verdicts queued.
pub const RETRY_LAND: &str = "retry just land";
pub use symphony_board::land::DIRTY_MAIN;

fn git(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let done =
        symphony_board::sys::output(crate::children::git().args(args).current_dir(root)).ok()?;
    done.status.success().then_some(done.stdout)
}

fn text(root: &Path, args: &[&str]) -> Option<String> {
    git(root, args).map(|out| String::from_utf8_lossy(&out).into_owned())
}

/// A document page of `project` (Markdown an entrypoint shows, with no hidden or dot path
/// component), as the verdict exemption admits it and the app commits it for the owner.
pub fn is_doc(project: &crate::project::Project, path: &str) -> bool {
    project.is_page(path)
}

/// Documents holding an owner answer no commit has, read by the board's landing/sweep reader.
/// A read failure returns a diagnostic entry, preserving the gate's nonempty-means-keep contract;
/// disappearing checkouts are skipped.
pub fn owner_lines_left(root: &Path) -> Vec<String> {
    if !root.exists() {
        return Vec::new();
    }
    match symphony_board::sweep::git::answers(
        root,
        &symphony_board::project::documents_at(root),
        None,
    ) {
        Ok(answers) => answers
            .into_iter()
            .map(|answer| answer.file)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect(),
        Err(why) => vec![format!("owner answers could not be read: {why}")],
    }
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
        let _held = WORKTREES.lock().recover();
        let done = symphony_board::sys::output(
            crate::children::git()
                .args(["worktree", "add", "--detach"])
                .arg(&held.path)
                .arg(rev)
                .current_dir(root),
        )
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
        let _held = WORKTREES.lock().recover();
        let _ = symphony_board::sys::output(
            crate::children::git()
                .args(["worktree", "remove", "--force"])
                .arg(&self.path)
                .current_dir(&self.root),
        );
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

/// A generator run as a command in a private worktree; heavy outputs run only after an earlier
/// generator changed something. A project names its own in `[verdicts] generators`, and those the
/// regenerator the default `just land` starts runs in `[land] generators`.
pub use symphony_board::land::Generator;

/// The generators of the project at `root` ([`of_project`]).
pub fn generators(root: &Path) -> Vec<Generator> {
    of_project(&crate::project::Project::of(root))
}

/// The generators of `project`: its `symphony.toml`'s in their order, then its plugins'.
pub fn of_project(project: &crate::project::Project) -> Vec<Generator> {
    with_plugins(project, &crate::plugins::Plugins::for_project(project))
}

/// The generators of `project` with `plugins` as its plugins.
pub fn with_plugins(
    project: &crate::project::Project,
    plugins: &crate::plugins::Plugins,
) -> Vec<Generator> {
    project
        .verdicts
        .generators
        .iter()
        .cloned()
        .chain(plugins.generators().into_iter().map(|generator| Generator {
            name: generator.name().into(),
            outputs: generator.outputs().into(),
            command: generator.command(),
            when_changed: generator.when_changed(),
        }))
        .collect()
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

/// Whether `branch` is a verdict commit of the checkout at `root` whose every change but
/// `project`'s documents is what `generators` write from those documents: its record is in
/// `project`'s record folder, and running the generators on the default branch with its documents
/// makes exactly its other changes. `project` (the lander's is main's committed one) alone says
/// which changes are documents, never the checkout's own `symphony.toml`.
pub fn regenerates(
    root: &Path,
    branch: &str,
    project: &crate::project::Project,
    generators: &[Generator],
) -> bool {
    regenerates_inner(root, branch, project, generators).unwrap_or(false)
}

fn regenerates_inner(
    root: &Path,
    branch: &str,
    project: &crate::project::Project,
    generators: &[Generator],
) -> Option<bool> {
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
    let records = project.record_folder(root, Path::new(common.trim()))?;
    if !records.join(tip).exists() {
        return Some(false);
    }
    let base = text(root, &["merge-base", &project.default_branch, tip])?;
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
    let (docs, mut outputs): (Vec<_>, Vec<_>) =
        names.into_iter().partition(|name| is_doc(project, name));
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
        if !symphony_board::sys::output(command(program).args(args).current_dir(&wt.path))
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
