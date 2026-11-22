//! Owner edits on main, over a private index. Generated inputs travel with the verdict on a
//! separate branch; neither path ever stages the server checkout's other work. The uploads an
//! owner's new line names go into the same commit, at the path their link names
//! (`saved_uploads`), so a clone shows them.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{Arc, Mutex, mpsc};

use serde::{Deserialize, Serialize};

use crate::children::command;
use crate::landing::{PrivateWorktree, generators};
use crate::plugins::Plugins;
use crate::project::Project;

type Blobs = BTreeMap<String, String>;

const REPOSITORY_OVERRIDES: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
];

struct PublicationStage {
    id: u64,
    name: String,
    began: std::time::Instant,
}

impl PublicationStage {
    fn start(name: std::fmt::Arguments<'_>) -> Option<Self> {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if !*ENABLED.get_or_init(|| symphony_env::PUBLICATION_TRACE.os().is_some()) {
            return None;
        }
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let stage = Self {
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            name: name.to_string(),
            began: std::time::Instant::now(),
        };
        stage.report("started");
        Some(stage)
    }

    fn line(&self, state: &str) -> String {
        format!(
            "publication pid={} id={} thread={:?}/{:?} stage={:?} {state} elapsed={:.3}s\n",
            std::process::id(),
            self.id,
            std::thread::current().id(),
            std::thread::current().name().unwrap_or("worker"),
            self.name,
            self.began.elapsed().as_secs_f64()
        )
    }

    fn report(&self, state: &str) {
        // Bypass libtest's capture so an outer target timeout retains the stage
        // that started without finishing, even before the case watchdog fires.
        let _ = std::io::stderr().write_all(self.line(state).as_bytes());
    }
}

impl Drop for PublicationStage {
    fn drop(&mut self) {
        self.report("finished");
    }
}

/// Ordinary saves read repository metadata in process. Explicit discovery or
/// command-line configuration overrides retain Git's own interpretation.
fn repository_metadata(root: &Path) -> Option<gix::Repository> {
    let _stage = PublicationStage::start(format_args!("repository discovery {}", root.display()));
    if REPOSITORY_OVERRIDES
        .iter()
        .any(|key| std::env::var_os(key).is_some())
    {
        return None;
    }
    gix::discover_opts(
        root,
        Default::default(),
        gix::open::Options::default().bail_if_untrusted(true),
    )
    .ok()
}

fn common_dir(root: &Path) -> Result<PathBuf, String> {
    if let Some(repo) = repository_metadata(root) {
        return repo
            .common_dir()
            .canonicalize()
            .map_err(|e| format!("{}: {e}", repo.common_dir().display()));
    }
    git(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .map(PathBuf::from)
}

#[cfg(test)]
mod metadata_tests {
    use super::*;

    #[test]
    fn publication_report_names_the_pending_stage_without_multiline_log_injection() {
        let stage = std::mem::ManuallyDrop::new(PublicationStage {
            id: 7,
            name: "wait git pid=17 update-index\nnext line".into(),
            began: std::time::Instant::now(),
        });
        let pending = stage.line("started");
        assert!(
            pending.contains("stage=\"wait git pid=17 update-index\\nnext line\" started elapsed="),
            "{pending}"
        );
        assert_eq!(pending.lines().count(), 1);
        assert!(pending.starts_with(&format!("publication pid={} ", std::process::id())));
        let finished = stage.line("finished");
        assert!(finished.contains(" finished elapsed="), "{finished}");
    }

    #[test]
    fn fresh_verdict_metadata_uses_the_symphony_home() {
        let tree = crate::test_tree::Tree::new();
        git(&tree.root, &["init", "-q", "-b", "main"]).unwrap();
        let common = common_dir(&tree.root).unwrap();
        assert_eq!(home(&tree.root).unwrap(), common.join("symphony/server"));
        let backlog = Backlog::new(&tree.root).unwrap();
        assert_eq!(backlog.sequence, 1);
        assert!(backlog.path.starts_with(common.join("symphony/server")));
    }

    #[test]
    fn repository_metadata_handles_worktrees_without_spawning_git_and_honors_overrides() {
        const CHILD_ROOT: &str = "SYMPHONY_METADATA_ROOT";
        const CASE: &str = "commit::metadata_tests::repository_metadata_handles_worktrees_without_spawning_git_and_honors_overrides";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let root = PathBuf::from(root);
            let main = root.join("main");
            let expected = main.join(".git").canonicalize().unwrap();
            let mode = std::env::var("SYMPHONY_METADATA_MODE").unwrap();
            if mode == "override" {
                // Local discovery succeeds here, but the override selects main's
                // common directory and worktree lock instead of the bare repo's.
                let bare = root.join("bare");
                assert_eq!(common_dir(&bare).unwrap().canonicalize().unwrap(), expected);
                assert!(on_main(&bare));
                assert!(write_lock(&bare).unwrap().is_some());
                return;
            }
            if mode == "aliases" {
                for (target, is_main) in [("main", true), ("proposal", false)] {
                    std::fs::write(
                        expected.join("refs/heads/alias"),
                        format!("ref: refs/heads/{target}\n"),
                    )
                    .unwrap();
                    std::fs::write(expected.join("HEAD"), "ref: refs/heads/alias\n").unwrap();
                    assert_eq!(on_main(&main), is_main);
                }
                return;
            }
            for path in [&main, &root.join("linked"), &main.join("docs")] {
                assert_eq!(common_dir(path).unwrap(), expected);
                assert_eq!(home(path).unwrap(), expected.join("symphony/server"));
                assert!(write_lock(path).unwrap().is_some());
            }
            assert!(on_main(&main));
            assert!(on_main(&root.join("unborn")));
            assert!(!on_main(&root.join("linked")));
            assert!(!on_main(&root.join("detached")));
            assert!(write_lock(&root.join("bare")).unwrap().is_none());
            assert!(write_lock(&root.join("plain")).unwrap().is_none());
            // A project naming no record keeps none; one naming a `.git/` folder keeps it in the
            // common directory, from whichever checkout records it.
            record(&root.join("linked"), "unrecorded").unwrap();
            assert!(!expected.join("land/verdicts/unrecorded").exists());
            std::fs::write(
                root.join("linked").join(crate::project::FILE),
                "[verdicts]\nrecord = \".git/land/verdicts\"\n",
            )
            .unwrap();
            record(&root.join("linked"), "fixture").unwrap();
            assert!(expected.join("land/verdicts/fixture").is_file());
            // Each call reads HEAD again; there is no path-keyed metadata cache.
            std::fs::write(expected.join("HEAD"), "ref: refs/heads/proposal\n").unwrap();
            assert!(!on_main(&main));
            std::fs::write(expected.join("HEAD"), "ref: refs/heads/main\n").unwrap();
            return;
        }
        let tree = crate::test_tree::Tree::new();
        let command = |root: &Path, args: &[&str]| {
            let mut cmd = symphony_board::sys::Command::new("git");
            for key in REPOSITORY_OVERRIDES {
                cmd.env_remove(key);
            }
            let done = symphony_board::sys::output(
                cmd.args(args)
                    .current_dir(root)
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .env("GIT_CONFIG_NOSYSTEM", "1"),
            )
            .unwrap();
            assert!(done.status.success(), "{args:?}: {done:?}");
        };
        for name in ["main", "unborn", "bare", "plain"] {
            std::fs::create_dir(tree.path(name)).unwrap();
        }
        let main = tree.path("main");
        command(&main, &["init", "-q", "-b", "main"]);
        command(
            &main,
            &[
                "-c",
                "user.name=Metadata test",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "fixture",
            ],
        );
        command(
            &main,
            &[
                "worktree",
                "add",
                "-qb",
                "proposal",
                tree.path("linked").to_str().unwrap(),
            ],
        );
        command(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                tree.path("detached").to_str().unwrap(),
            ],
        );
        command(&tree.path("unborn"), &["init", "-q", "-b", "main"]);
        command(&tree.path("bare"), &["init", "--bare", "-q", "-b", "main"]);
        std::fs::create_dir(main.join("docs")).unwrap();
        for mode in ["without-git", "override", "aliases"] {
            let mut child = symphony_board::sys::Command::new(std::env::current_exe().unwrap());
            for key in REPOSITORY_OVERRIDES {
                child.env_remove(key);
            }
            child
                .args(["--exact", CASE, "--nocapture"])
                .env(CHILD_ROOT, &tree.root)
                .env("SYMPHONY_METADATA_MODE", mode)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1");
            if mode == "override" {
                child
                    .env("GIT_DIR", main.join(".git"))
                    .env("GIT_WORK_TREE", &main);
            } else if mode == "without-git" {
                child.env("PATH", tree.path("no-executables"));
            }
            let done = symphony_board::sys::output(&mut child).unwrap();
            assert!(done.status.success(), "{done:?}");
            assert!(
                String::from_utf8_lossy(&done.stdout).contains("1 passed"),
                "the metadata probe must run: {done:?}"
            );
        }
    }
}

/// The folder of queued verdict publications, in the [`home`] of the checkout's common dir.
const JOURNAL: &str = "verdict-publications";

/// The server's folder in the common dir of `root`'s checkout, where the verdicts' journals and
/// locks are kept.
fn home(root: &Path) -> Result<PathBuf, String> {
    Ok(common_dir(root)?.join(symphony_board::folders::SERVER))
}

/// Separate locks keep HTTP read/edit/write sections short while commit preparation may build
/// generated content. Every serving generation opens its own descriptor in the same common dir.
fn shared_lock(root: &Path, name: &str) -> Result<File, String> {
    let _stage = PublicationStage::start(format_args!("lock {name}"));
    let folder = home(root)?;
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let path = folder.join(name);
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let _wait = PublicationStage::start(format_args!("acquire lock {name}"));
    file.lock()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(file)
}

/// Serialize app writes with the lander's final main-checkout validation and swap.
pub fn write_lock(root: &Path) -> Result<Option<File>, String> {
    // The app also serves plain folders outside a repository; those have no main committer.
    let worktree = repository_metadata(root).map_or_else(
        || git(root, &["rev-parse", "--show-toplevel"]).is_ok(),
        |repo| repo.workdir().is_some(),
    );
    if !worktree {
        return Ok(None);
    }
    shared_lock(root, "verdict-writes.lock").map(Some)
}

/// Locked for the whole lifetime of a queued edit. Sequence order follows the save's short
/// write lock, so a retry waits only for earlier edits and cannot form a cycle with a later one.
struct Backlog {
    sequence: u64,
    path: PathBuf,
    _held: File,
}

impl Backlog {
    fn new(root: &Path) -> Result<Self, String> {
        let _stage = PublicationStage::start(format_args!("register backlog"));
        let _sequence = shared_lock(root, "verdict-sequence.lock")?;
        let folder = home(root)?.join("verdict-backlog");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let counter = folder.join("next");
        let previous = match std::fs::read_to_string(&counter) {
            Ok(text) => text
                .trim()
                .parse::<u64>()
                .map_err(|e| format!("{}: {e}", counter.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
            Err(e) => return Err(format!("{}: {e}", counter.display())),
        };
        let sequence = previous
            .checked_add(1)
            .ok_or("verdict sequence exhausted")?;
        let next = folder.join(format!("next.{}", std::process::id()));
        std::fs::write(&next, sequence.to_string())
            .and_then(|()| std::fs::rename(&next, &counter))
            .map_err(|e| format!("{}: {e}", counter.display()))?;
        let path = folder.join(format!("{sequence:020}"));
        let held = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        held.lock()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self {
            sequence,
            path,
            _held: held,
        })
    }
}

impl Drop for Backlog {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn older_backlogs(root: &Path, sequence: u64) -> Result<bool, String> {
    let folder = home(root)?.join("verdict-backlog");
    for entry in std::fs::read_dir(&folder).map_err(|e| format!("{}: {e}", folder.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.parse::<u64>().ok())
            .is_some_and(|earlier| earlier < sequence)
        {
            continue;
        }
        let held = match OpenOptions::new().read(true).write(true).open(&path) {
            Ok(held) => held,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        match held.try_lock() {
            Ok(()) => {} // A stopped process's stale marker holds no backlog.
            Err(std::fs::TryLockError::WouldBlock) => return Ok(true),
            Err(std::fs::TryLockError::Error(e)) => return Err(format!("{}: {e}", path.display())),
        }
    }
    Ok(false)
}

enum CommitError {
    Failed(String),
    Retry(String),
}

impl From<String> for CommitError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

impl From<CommitError> for String {
    fn from(error: CommitError) -> Self {
        match error {
            CommitError::Failed(message) | CommitError::Retry(message) => message,
        }
    }
}

impl std::fmt::Display for CommitError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed(message) | Self::Retry(message) => out.write_str(message),
        }
    }
}

/// A save's immutable edit stays queued through a merge conflict. Each attempt reads main again;
/// callers release the write mutex before returning here, so new HTTP saves can still enqueue.
fn retry_conflicts<T>(
    root: &Path,
    sequence: u64,
    mut work: impl FnMut() -> Result<T, CommitError>,
) -> Result<T, String> {
    let mut said = false;
    let mut previous_main = None;
    loop {
        match work() {
            Ok(done) => return Ok(done),
            Err(CommitError::Failed(message)) => return Err(message),
            Err(CommitError::Retry(message)) => {
                let main = git(root, &["rev-parse", &main_ref(root)])?;
                if !older_backlogs(root, sequence)?
                    && previous_main.as_deref() == Some(main.as_str())
                {
                    return Err(message);
                }
                previous_main = Some(main);
                if !said {
                    let _ = writeln!(
                        std::io::stderr(),
                        "Symphony: saved verdict queued for retry on fresh main in {}: {message}",
                        root.display()
                    );
                    said = true;
                }
                #[expect(
                    clippy::disallowed_methods,
                    reason = "no-timed-waits baseline: tries a commit again after a fixed pause, where a backoff is the helper"
                )]
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Edit {
    pub rel: String,
    pub base: String,
    pub new: String,
}

#[derive(Default, Serialize)]
pub struct Outcome {
    pub committed: bool,
    pub error: String,
    pub notes: Vec<String>,
}

fn run(
    root: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    index: Option<&Path>,
) -> Result<Output, String> {
    let mut command = command("git");
    command.args(args).current_dir(root);
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let child = {
        let _stage = PublicationStage::start(format_args!("spawn git {args:?}"));
        symphony_board::sys::spawn_piped(&mut command).map_err(|e| e.to_string())?
    };
    let mut child = child;
    // git refusing before it reads its input (an argument it does not know, say) has closed it, and
    // the write fails: git is waited for all the same, so what it said reaches the caller and its
    // process is reaped. The write's own error is the answer only where git said nothing, or
    // succeeded without having had its whole input.
    let mut written = Ok(());
    if let Some(input) = input {
        let _stage = PublicationStage::start(format_args!("write git input pid={}", child.id()));
        written = match child.stdin.take() {
            Some(mut pipe) => pipe.write_all(input),
            None => Err(std::io::Error::other("no command input")),
        };
    }
    let _wait = PublicationStage::start(format_args!("wait git pid={} {args:?}", child.id()));
    let done = symphony_board::sys::wait_output(child).map_err(|e| e.to_string())?;
    match written {
        Err(problem) if done.status.success() => Err(format!(
            "git answered, but its input was not written: {problem}"
        )),
        Err(problem) if done.stderr.trim_ascii().is_empty() => Err(format!(
            "git exited {}; its input was not written: {problem}",
            done.status
        )),
        _ => Ok(done),
    }
}

fn checked(done: Output) -> Result<String, String> {
    if done.status.success() {
        String::from_utf8(done.stdout).map_err(|e| e.to_string())
    } else {
        let said = String::from_utf8_lossy(&done.stderr).trim().to_string();
        Err(if said.is_empty() {
            format!("git exited {}", done.status)
        } else {
            said
        })
    }
}

#[cfg(test)]
mod input_tests;

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    checked(run(root, args, None, None)?).map(|s| s.trim_end_matches('\n').to_string())
}

/// The ref of the default branch of the project at `root` (`refs/heads/main`), where verdicts are
/// committed.
fn main_ref(root: &Path) -> String {
    format!("refs/heads/{}", Project::of(root).default_branch)
}

/// Whether the checkout at `root` is on its project's default branch.
pub fn on_main(root: &Path) -> bool {
    if let Some(repo) = repository_metadata(root)
        && let Ok(head) = repo.head()
    {
        // Git resolves chains of symbolic refs. Keep its behavior for those
        // unusual heads; ordinary, detached and unborn heads need no process.
        let chained = matches!(&head.kind, gix::head::Kind::Symbolic(reference)
            if matches!(reference.target, gix::refs::Target::Symbolic(_)));
        if !chained {
            return head
                .referent_name()
                .is_some_and(|name| name.as_bstr() == main_ref(root).as_str());
        }
    }
    git(root, &["symbolic-ref", "--quiet", "HEAD"]).is_ok_and(|branch| branch == main_ref(root))
}

fn merge(root: &Path, current: &str, base: &str, new: &str) -> Result<String, CommitError> {
    let plugins = &Plugins::of(root);
    let _stage = PublicationStage::start(format_args!("merge saved text"));
    // A worktree also provides a unique, RAII-owned scratch directory. No checkout is needed for
    // merge-file: create the three files under an ordinary per-call directory instead.
    let scratch = Scratch::new()?;
    for (name, text) in [("current", current), ("base", base), ("new", new)] {
        std::fs::write(scratch.0.join(name), text).map_err(|e| e.to_string())?;
    }
    let done = {
        let _child = PublicationStage::start(format_args!("git merge-file"));
        symphony_board::sys::output(
            command("git")
                .args(["merge-file", "-p"])
                .args(["current", "base", "new"].map(|n| scratch.0.join(n)))
                .current_dir(root),
        )
        .map_err(|e| e.to_string())?
    };
    if done
        .status
        .code()
        .is_some_and(|code| (1..=127).contains(&code))
    {
        if let Some(merged) = merge_history(current, base, new, plugins) {
            return mark_unseen_replies(current, new, &merged, plugins);
        }
        return Err(CommitError::Retry(checked(done).unwrap_err()));
    }
    let merged = checked(done).map_err(CommitError::Failed)?;
    // A clean line merge must keep the three-way choice of each slug's count. This admits
    // independent additions and removals, but catches duplicated moves and resurrected cards.
    match card_count_mismatch(current, base, new, &merged) {
        Some(false) => mark_unseen_replies(current, new, &merged, plugins),
        Some(true) => merge_history(current, base, new, plugins)
            .ok_or_else(|| {
                CommitError::Retry(
                    "the text merge disagrees with the three-way decision card counts".into(),
                )
            })
            .and_then(|merged| mark_unseen_replies(current, new, &merged, plugins)),
        None => Err(CommitError::Retry(
            "the decision card counts conflict".into(),
        )),
    }
}

/// A reply already on main cannot answer an owner entry absent from that version of the card.
/// Store that relation on the card, outside its immutable, chronologically ordered posts.
fn mark_unseen_replies(
    current: &str,
    new: &str,
    merged: &str,
    plugins: &Plugins,
) -> Result<String, CommitError> {
    use crate::register::{Who, entry_keys, parse_decisions, unseen_marker, unseen_reply};
    use std::collections::BTreeSet;

    let cards = |text: &str| {
        let mut cards = BTreeMap::new();
        for card in parse_decisions(&text.lines().collect::<Vec<_>>()) {
            cards.insert(card.slug.clone(), card);
        }
        cards
    };
    let current = cards(current);
    let new = cards(new);
    let lines: Vec<_> = merged.split_inclusive('\n').collect();
    let plain: Vec<_> = lines
        .iter()
        .map(|line| line.trim_end_matches('\n'))
        .collect();
    let mut insertions = BTreeMap::new();
    for card in parse_decisions(&plain) {
        let (Some(current), Some(new)) = (current.get(&card.slug), new.get(&card.slug)) else {
            continue;
        };
        let current_keys: BTreeSet<_> = entry_keys(&current.entries()).into_iter().collect();
        let new_keys: BTreeSet<_> = entry_keys(&new.entries()).into_iter().collect();
        let entries = card.entries();
        let keys = entry_keys(&entries);
        let mut pairs = BTreeSet::new();
        // Carry metadata from either side even when a clean text merge moved its lines.
        for source in [current, new, &card] {
            pairs.extend(source.raw.iter().filter_map(|line| unseen_reply(line)));
        }
        let mut markers: BTreeSet<String> = pairs
            .into_iter()
            .map(|(owner, reply)| unseen_marker(owner, reply))
            .collect();
        for (index, owner) in entries.iter().enumerate() {
            let key = &keys[index];
            if owner.who != Who::Owner || current_keys.contains(key) || !new_keys.contains(key) {
                continue;
            }
            for (reply, reply_key) in entries[index + 1..].iter().zip(&keys[index + 1..]) {
                if reply.who == Who::Agent
                    && reply
                        .verdict
                        .as_deref()
                        .is_none_or(|v| v == crate::register::QUESTION)
                    && current_keys.contains(reply_key)
                {
                    markers.insert(unseen_marker(key, reply_key));
                }
            }
        }
        for line in &card.raw {
            markers.remove(&format!("{line}\n"));
        }
        if !markers.is_empty() {
            insertions.insert(card.line, markers.into_iter().collect::<String>());
        }
    }
    if insertions.is_empty() {
        return Ok(merged.to_string());
    }
    let mut result = String::new();
    for (index, line) in lines.iter().enumerate() {
        if let Some(markers) = insertions.get(&index) {
            result.push_str(markers);
        }
        result.push_str(line);
    }
    crate::register::sort_register(&result, plugins)
        .map_err(|error| CommitError::Failed(error.to_string()))
}

fn card_counts(text: &str) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for card in crate::register::parse_decisions(&text.lines().collect::<Vec<_>>()) {
        *counts.entry(card.slug).or_default() += 1;
    }
    counts
}

/// None means both sides changed a count differently; otherwise report whether the text
/// merge disagrees with the unambiguous three-way count choice for any slug.
fn card_count_mismatch(current: &str, base: &str, new: &str, merged: &str) -> Option<bool> {
    let counts = [current, base, new, merged].map(card_counts);
    let slugs: std::collections::BTreeSet<_> = counts.iter().flat_map(BTreeMap::keys).collect();
    let mut mismatch = false;
    for slug in slugs {
        let [current, base, new, merged] = counts
            .each_ref()
            .map(|counts| counts.get(slug).copied().unwrap_or_default());
        let expected = if current == base {
            new
        } else if new == base || new == current {
            current
        } else {
            return None;
        };
        if merged != expected {
            mismatch = true;
        }
    }
    Some(mismatch)
}

struct History<'a> {
    header: &'a str,
    entries: Vec<(&'a str, String)>,
    metadata: Vec<&'a str>,
}

impl<'a> History<'a> {
    fn additions<'b>(&'b self, base: &Self) -> Option<Vec<&'b (&'a str, String)>> {
        let mut existing = base.entries.iter().peekable();
        let mut added = Vec::new();
        for entry in &self.entries {
            if existing.peek().is_some_and(|next| {
                next.0 == entry.0 && next.1.trim_end_matches('\n') == entry.1.trim_end_matches('\n')
            }) {
                existing.next();
            } else {
                added.push(entry);
            }
        }
        existing.next().is_none().then_some(added)
    }
}

#[derive(PartialEq, Eq)]
enum HistoryPart {
    Text(String),
    Card(String, String),
}

struct Histories<'a> {
    skeleton: Vec<HistoryPart>,
    cards: BTreeMap<String, History<'a>>,
}

/// Remove only complete added cards and their generated topic wrapper. Adjacent blank
/// separators belong to the added block; all other document text must still match exactly.
fn same_skeleton(
    current: &[HistoryPart],
    base: &[HistoryPart],
    added: &BTreeMap<String, String>,
) -> bool {
    if current == base {
        return true;
    }
    let mut skips = BTreeMap::<usize, Vec<usize>>::new();
    for (index, part) in current.iter().enumerate() {
        let HistoryPart::Card(slug, _) = part else {
            continue;
        };
        let Some(topic) = added.get(slug) else {
            continue;
        };
        let mut first = index;
        while first > 0
            && matches!(&current[first - 1], HistoryPart::Text(line) if line.trim().is_empty())
        {
            first -= 1;
        }
        if first > 0
            && matches!(&current[first - 1], HistoryPart::Text(line) if line == &format!("### {topic}\n"))
        {
            first -= 1;
            while first > 0
                && matches!(&current[first - 1], HistoryPart::Text(line) if line.trim().is_empty())
            {
                first -= 1;
            }
        }
        let mut last = index + 1;
        let mut content_end = last;
        while last < current.len()
            && matches!(&current[last], HistoryPart::Text(line) if line.trim().is_empty() || crate::register::is_indented(line))
        {
            if matches!(&current[last], HistoryPart::Text(line) if crate::register::is_indented(line))
            {
                content_end = last + 1;
            }
            last += 1;
        }
        for start in first..=index {
            skips.entry(start).or_default().extend(content_end..=last);
        }
    }
    // Match unchanged text or jump over a complete added block. Memoize positions so a
    // page with many added cards cannot multiply separator choices exponentially.
    let mut pending = vec![(0, 0)];
    let mut seen = std::collections::BTreeSet::new();
    while let Some((i, j)) = pending.pop() {
        if !seen.insert((i, j)) {
            continue;
        }
        if i == current.len() && j == base.len() {
            return true;
        }
        if i < current.len() && j < base.len() && current[i] == base[j] {
            pending.push((i + 1, j + 1));
        }
        if let Some(ends) = skips.get(&i) {
            pending.extend(ends.iter().map(|end| (*end, j)));
        }
    }
    false
}

/// Group headings, spacing between cards and card order in a register are generated. A
/// shared order keyed by slug exposes changes to card text, topic or membership independently
/// of regrouping. Everything outside the marked region remains byte-for-byte significant.
fn canonical_register(
    text: &str,
    order: &BTreeMap<String, usize>,
    plugins: &Plugins,
) -> Option<String> {
    use crate::register::{REGION_END, REGION_START, parse_decisions, sort_register};

    let lines: Vec<_> = text.split_inclusive('\n').collect();
    let starts: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim_end_matches('\n') == REGION_START)
        .map(|(i, _)| i)
        .collect();
    let ends: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim_end_matches('\n') == REGION_END)
        .map(|(i, _)| i)
        .collect();
    if starts.is_empty() && ends.is_empty() {
        return Some(text.to_string());
    }
    if starts.len() > 1 && starts.len() == ends.len() {
        let mut result = String::new();
        let mut after = 0;
        for (&start, &end) in starts.iter().zip(&ends) {
            if start < after || start >= end {
                return None;
            }
            result.push_str(&lines[after..start].concat());
            result.push_str(&canonical_register(
                &lines[start..=end].concat(),
                order,
                plugins,
            )?);
            after = end + 1;
        }
        result.push_str(&lines[after..].concat());
        return Some(result);
    }
    let ([start], [end]) = (starts.as_slice(), ends.as_slice()) else {
        return None;
    };
    if start >= end {
        return None;
    }
    // Validate the region with the same parser and loss checks as the app's regrouping.
    sort_register(text, plugins).ok()?;
    let body = &lines[start + 1..*end];
    let plain: Vec<_> = body
        .iter()
        .map(|line| line.trim_end_matches('\n'))
        .collect();
    let mut cards = parse_decisions(&plain);
    let mut covered = vec![false; body.len()];
    for card in &cards {
        let index = card.line - 1;
        covered[index..index + 1 + card.raw.len()].fill(true);
    }
    if body.iter().enumerate().any(|(i, line)| {
        !covered[i]
            && !line.trim().is_empty()
            && !line.starts_with("## ")
            && !line.starts_with("### ")
    }) {
        return None;
    }
    symphony_board::order::sort_by(&mut cards, &|a, b| {
        order
            .get(&a.slug)
            .cmp(&order.get(&b.slug))
            .then_with(|| a.slug.cmp(&b.slug))
    });
    let mut canonical = lines[..=*start].concat();
    for card in cards {
        canonical.push_str(&format!("\n### {}\n\n", card.topic));
        let index = card.line - 1;
        canonical.push_str(&body[index..index + 1 + card.raw.len()].concat());
        canonical.push('\n');
    }
    canonical.push_str(&lines[*end..].concat());
    Some(canonical)
}

impl<'a> Histories<'a> {
    fn read(text: &'a str) -> Option<Self> {
        use crate::register::{card_line, unseen_reply};
        use std::collections::BTreeSet;
        use symphony_docs::syntax::{cards as shared_cards, entry_line, is_continuation};

        let mut skeleton = Vec::new();
        let mut cards = BTreeMap::new();
        // Lines are classified as the shared reading reads them, without their `\n` or `\r\n`
        // (the merge keeps their bytes), and a fence under a card is its thread's.
        let bare = |line: &'a str| {
            let line = line.strip_suffix('\n').unwrap_or(line);
            line.strip_suffix('\r').unwrap_or(line)
        };
        let plain: Vec<_> = text.split_inclusive('\n').map(bare).collect();
        let openers: BTreeSet<_> = shared_cards(&plain).iter().map(|c| c.line - 1).collect();
        let dated = |line: &'a str| {
            let at = line.find('[')? + 1;
            Some(&line[at..at + entry_line(bare(line))?.ts?.len()])
        };
        let hidden = |line: &'a str| unseen_reply(bare(line)).is_some();
        let mut lines = text.split_inclusive('\n').enumerate().peekable();
        while let Some((index, line)) = lines.next() {
            let Some(card) = card_line(bare(line)).filter(|_| openers.contains(&index)) else {
                skeleton.push(HistoryPart::Text(line.to_string()));
                continue;
            };
            if cards.contains_key(card.slug) {
                return None;
            }
            skeleton.push(HistoryPart::Card(
                card.slug.to_string(),
                format!(
                    "{} {}",
                    &line[..card.mark.start],
                    line[card.mark.end..].trim_end_matches('\n')
                ),
            ));
            let mut entries = Vec::new();
            let mut metadata = Vec::new();
            while let Some(&(_, next)) = lines.peek() {
                if hidden(next) {
                    metadata.push(lines.next()?.1);
                    continue;
                }
                let Some(timestamp) = dated(next) else {
                    break;
                };
                let mut raw = lines.next()?.1.to_string();
                // A continuation (`is_continuation`, even a dated-looking one) moves unchanged with
                // its entry; a hidden marker between them, skipped when read, joins the metadata.
                loop {
                    let mut following = lines.clone();
                    let mut gap = String::new();
                    let mut markers = Vec::new();
                    while let Some(&(_, line)) = following.peek() {
                        if hidden(line) {
                            markers.push(line);
                        } else if line.trim().is_empty() {
                            gap.push_str(line);
                        } else {
                            break;
                        }
                        following.next();
                    }
                    if !following
                        .peek()
                        .is_some_and(|&(_, line)| is_continuation(bare(line)))
                    {
                        break;
                    }
                    raw.push_str(&gap);
                    raw.push_str(following.next()?.1);
                    metadata.extend(markers);
                    lines = following;
                }
                entries.push((timestamp, raw));
            }
            cards.insert(
                card.slug.to_string(),
                History {
                    header: line,
                    entries,
                    metadata,
                },
            );
        }
        Some(Self { skeleton, cards })
    }
}

/// Resolve append-only histories by slug, allowing generated register regrouping. Existing
/// entries retain their order, even when their timestamps do not. New events are inserted by
/// time within saved order; equal-minute additions follow entries already present on main.
fn merge_history(current: &str, base: &str, new: &str, plugins: &Plugins) -> Option<String> {
    // Preserve main's stable order among cards that end up in the same generated group.
    let current_lines: Vec<_> = current.lines().collect();
    let mut order = BTreeMap::new();
    for (index, card) in crate::register::parse_decisions(&current_lines)
        .into_iter()
        .enumerate()
    {
        order.insert(card.slug, index);
    }
    let current_text = canonical_register(current, &order, plugins)?;
    let base_text = canonical_register(base, &order, plugins)?;
    let new_text = canonical_register(new, &order, plugins)?;
    let current = Histories::read(&current_text)?;
    let base = Histories::read(&base_text)?;
    let new = Histories::read(&new_text)?;
    let mut added = BTreeMap::new();
    for card in crate::register::parse_decisions(&current_text.lines().collect::<Vec<_>>()) {
        if !base.cards.contains_key(&card.slug) {
            added.insert(card.slug, card.topic);
        }
    }
    if !same_skeleton(&current.skeleton, &base.skeleton, &added) || new.skeleton != base.skeleton {
        return None;
    }
    let mut merged = String::new();
    for part in &current.skeleton {
        let slug = match part {
            HistoryPart::Text(line) => {
                merged.push_str(line);
                continue;
            }
            HistoryPart::Card(slug, _) => slug,
        };
        let c = current.cards.get(slug)?;
        let Some(b) = base.cards.get(slug) else {
            merged.push_str(c.header);
            for line in &c.metadata {
                merged.push_str(line);
            }
            for (_, raw) in &c.entries {
                merged.push_str(raw);
            }
            continue;
        };
        let n = new.cards.get(slug)?;
        // A preceding stale save can have merged an older event before an entry in this
        // save's base. Existing entries must remain intact and in order, rather than at the
        // start of the block, so subsequent queued saves can build on that merged history.
        let current_additions = c.additions(b)?;
        let new_additions = n.additions(b)?;
        let header = if c.header.trim_end_matches('\n') == b.header.trim_end_matches('\n') {
            n.header
        } else if n.header.trim_end_matches('\n') == b.header.trim_end_matches('\n')
            || n.header.trim_end_matches('\n') == c.header.trim_end_matches('\n')
        {
            c.header
        } else {
            return None;
        };
        merged.push_str(header);
        let metadata: std::collections::BTreeSet<_> =
            c.metadata.iter().chain(&n.metadata).collect();
        for line in metadata {
            merged.push_str(line);
        }
        let mut entries: Vec<_> = c.entries.iter().collect();
        // Match immutable events already on main occurrence by occurrence. Keeping their
        // identities also lets later saved additions stay after an event shared by both sides.
        let mut shared = BTreeMap::<&str, Vec<_>>::new();
        for entry in current_additions.into_iter().rev() {
            shared
                .entry(entry.1.trim_end_matches('\n'))
                .or_default()
                .push(entry);
        }
        let mut after = 0;
        for entry in new_additions {
            if let Some(existing) = shared
                .get_mut(entry.1.trim_end_matches('\n'))
                .and_then(Vec::pop)
            {
                let index = entries
                    .iter()
                    .position(|candidate| std::ptr::eq(*candidate, existing))?;
                if index < after {
                    return None;
                }
                after = index + 1;
            } else {
                // Preserve the save's order even when a quoted dated line or a clock skew
                // makes the next addition's timestamp earlier than the preceding one.
                let index = entries
                    .iter()
                    .enumerate()
                    .skip(after)
                    .find(|(_, existing)| existing.0 > entry.0)
                    .map_or(entries.len(), |(index, _)| index);
                entries.insert(index, entry);
                after = index + 1;
            }
        }
        for (_, raw) in entries {
            if !merged.ends_with('\n') {
                merged.push('\n');
            }
            merged.push_str(raw);
        }
    }
    crate::register::sort_register(&merged, plugins).ok()
}

struct Scratch(PathBuf);
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
impl Scratch {
    fn new() -> Result<Self, String> {
        loop {
            let path = std::env::temp_dir().join(format!(
                "review-commit-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn apply(root: &Path, parent: &str, edits: &[Edit]) -> Result<(Blobs, Blobs), CommitError> {
    let mut blobs = BTreeMap::new();
    let mut before = BTreeMap::new();
    let project = Project::of(root);
    for edit in edits {
        if !project.is_page(&edit.rel) {
            let under: Vec<String> = project
                .entrypoints
                .iter()
                .map(|entry| crate::entrypoint::join(&entry.folder, "*.md"))
                .collect();
            return Err(format!(
                "{} is not a design document under {}: the app commits only those",
                edit.rel,
                under.join(" or ")
            )
            .into());
        }
        if !blobs.contains_key(&edit.rel) {
            let text = checked(run(
                root,
                &["show", &format!("{parent}:{}", edit.rel)],
                None,
                None,
            )?)
            .map_err(|e| {
                format!(
                    "{} is not on main, so the verdict on it cannot land: {e}",
                    edit.rel
                )
            })?;
            before.insert(edit.rel.clone(), text.clone());
            blobs.insert(edit.rel.clone(), text);
        }
        let text = merge(root, &blobs[&edit.rel], &edit.base, &edit.new)
            .map_err(|e| match e {
                CommitError::Retry(why) => CommitError::Retry(format!("the verdict on {} conflicts with main: the text it was written against has changed there: {why}", edit.rel)),
                other => other,
            })?;
        blobs.insert(edit.rel.clone(), text);
    }
    Ok((blobs, before))
}

fn main_worktrees(root: &Path) -> Result<Vec<PathBuf>, String> {
    let _stage = PublicationStage::start(format_args!("main worktree mutex and listing"));
    let _held = crate::landing::WORKTREES
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let listing = git(root, &["worktree", "list", "--porcelain"])?;
    let main = main_ref(root);
    let mut path = None;
    let mut found = Vec::new();
    for line in listing.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        }
        if line.strip_prefix("branch ") == Some(main.as_str())
            && let Some(p) = &path
        {
            found.push(p.clone());
        }
    }
    Ok(found)
}

fn sync(root: &Path, rel: &str, old: &str, new: &str) -> Result<(), String> {
    let _stage = PublicationStage::start(format_args!("sync {} {rel}", root.display()));
    let file = root.join(rel);
    let current = std::fs::read_to_string(&file).ok();
    if current.as_deref() == Some(old) {
        git(root, &["checkout", "-q", "HEAD", "--", rel])?;
        return Ok(());
    }
    let mut problem = None;
    if let Some(current) = current
        && current != new
    {
        // The committed document is the trusted current side, including cards added on
        // main. The checkout contributes only work queued or edited after the saved base.
        match merge(root, new, old, &current) {
            Ok(merged) => std::fs::write(file, merged).map_err(|e| e.to_string())?,
            Err(e) => {
                problem = Some(format!(
                    "{rel} in {} has uncommitted edits the verdict does not merge into; its file is left as it is: {e}",
                    root.display()
                ))
            }
        }
    }
    git(root, &["reset", "-q", "HEAD", "--", rel])?;
    problem.map_or(Ok(()), Err)
}

/// The uploads an owner's save names that its base did not: each [`crate::uploads::reference`] on
/// a line `new` holds more often than `base` (the line the owner wrote: a register line, a note
/// under it, a callout's), whose stored bytes in the checkout at `root` are what its name promises
/// ([`upload_bytes`]). Asks, thread messages and new tasks never come here, so their uploads stay
/// working evidence in the store. Each comes as the path its link names from the repository root,
/// `artifacts/uploads/<sha256>.<ext>` whatever the project's artifacts folder is called (so a clone
/// and GitHub resolve the owner's words as written), with those bytes; beside them, a note for
/// each one left out, as the verdict is saved without it.
fn saved_uploads(root: &Path, edits: &[Edit]) -> (BTreeMap<String, Vec<u8>>, Vec<String>) {
    static LINK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(&format!(
            r"!?\[[^\]]*\]\({}[^)\s]*\)",
            regex::escape(crate::uploads::ROUTE)
        ))
        .expect("an upload link pattern")
    });
    let mut found = BTreeMap::new();
    let mut refused = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for edit in edits {
        let mut held: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for line in edit.base.lines() {
            *held.entry(line).or_default() += 1;
        }
        for line in edit.new.lines() {
            if let Some(count) = held.get_mut(line).filter(|count| **count > 0) {
                *count -= 1;
                continue;
            }
            for link in LINK.find_iter(line) {
                let Some(reference) = crate::uploads::reference(link.as_str()) else {
                    continue;
                };
                let name = &reference.url[crate::uploads::ROUTE.len()..];
                let path = &reference.url[1..];
                if !seen.insert(path.to_string()) {
                    continue;
                }
                let Some((stored, _)) = crate::uploads::path(root, name) else {
                    continue;
                };
                match upload_bytes(&stored, name) {
                    Ok(bytes) => {
                        found.insert(path.to_string(), bytes);
                    }
                    Err(why) => refused.push(format!(
                        "{path}: not committed with the verdict, which is saved without it: {why}"
                    )),
                }
            }
        }
    }
    (found, refused)
}

/// The bytes the store holds at `stored` for the upload `name` (`<sha256>.<ext>`), when they are
/// what the name promises: a regular file, never a link or anything else, of at most
/// [`crate::uploads::MAX_BYTES`], whose SHA-256 is the name's. Anything else in the ignored store (a
/// link to another file, a file planted or grown there) is not what the owner attached, and is never
/// committed in their name; these bytes, read once, are what the commit holds.
fn upload_bytes(stored: &Path, name: &str) -> Result<Vec<u8>, String> {
    use sha2::Digest;
    use std::io::Read;
    let max = crate::uploads::MAX_BYTES;
    let over = |bytes: u64| format!("{bytes} bytes, over the {max} an upload holds");
    let kind = std::fs::symlink_metadata(stored)
        .map_err(|e| format!("not in the store ({e})"))?
        .file_type();
    if !kind.is_file() {
        return Err("the store holds a link or another kind of file there, not the upload".into());
    }
    let mut bytes = Vec::new();
    File::open(stored)
        .and_then(|file| file.take(max as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("unreadable ({e})"))?;
    if bytes.len() > max {
        return Err(over(bytes.len() as u64));
    }
    let sha: String = sha2::Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if name.split('.').next() != Some(sha.as_str()) {
        return Err("its bytes are not the ones its name was made from".into());
    }
    Ok(bytes)
}

/// Of `uploads` ([`saved_uploads`]), those a text of `texts` still names: what the commit shows.
fn shown_uploads<T: Clone>(uploads: &BTreeMap<String, T>, texts: &[&str]) -> BTreeMap<String, T> {
    let mut shown = BTreeMap::new();
    for (path, bytes) in uploads {
        let link = format!("/{path}");
        if texts.iter().any(|text| text.contains(&link)) {
            shown.insert(path.clone(), bytes.clone());
        }
    }
    shown
}

/// After main gained the upload `path`, the main checkout `root` takes it: its index entry from
/// HEAD, and its file where none is (a project whose artifacts folder has another name stores it
/// elsewhere). The file named by its content is the committed one, so the checkout stays clean and
/// nothing else of it is touched.
fn sync_upload(root: &Path, path: &str) -> Result<(), String> {
    let _stage = PublicationStage::start(format_args!("sync upload {} {path}", root.display()));
    if root.join(path).is_file() {
        git(root, &["reset", "-q", "HEAD", "--", path])?;
    } else {
        git(root, &["checkout", "-q", "HEAD", "--", path])?;
    }
    Ok(())
}

/// Marks `commit` as the owner's verdict where the project's landing hook reads it
/// (`[verdicts] record`); a project without a record writes none.
fn record(root: &Path, commit: &str) -> Result<(), String> {
    let Some(folder) = Project::of(root).record_folder(root, &common_dir(root)?) else {
        return Ok(());
    };
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    std::fs::write(folder.join(commit), "Symphony verdict\n").map_err(|e| e.to_string())
}

/// What `edits` make of the commit `parent`, written to the object store over a private index:
/// the tree, each edited page's text there and before, and the uploads the new lines name.
struct Made {
    tree: String,
    blobs: Blobs,
    before: Blobs,
    uploads: BTreeMap<String, Vec<u8>>,
    /// Whether the tree is `parent`'s own: the edits change nothing there.
    same: bool,
}

fn made(
    root: &Path,
    parent: &str,
    edits: &[Edit],
    saved: &BTreeMap<String, Vec<u8>>,
) -> Result<Made, CommitError> {
    let (blobs, before) = apply(root, parent, edits)?;
    let texts: Vec<&str> = blobs.values().map(String::as_str).collect();
    let uploads = shown_uploads(saved, &texts);
    let scratch = Scratch::new()?;
    let index = scratch.0.join("index");
    checked(run(root, &["read-tree", parent], None, Some(&index))?)?;
    let mut entries = Vec::new();
    for (rel, text) in &blobs {
        let blob = checked(run(
            root,
            &["hash-object", "-w", "--stdin"],
            Some(text.as_bytes()),
            None,
        )?)?;
        entries.push((rel, blob));
    }
    for (path, bytes) in &uploads {
        // The bytes checked against the name, hashed as `git add` would store them at that
        // path, past the ignore.
        let blob = checked(run(
            root,
            &["hash-object", "-w", "--stdin", &format!("--path={path}")],
            Some(bytes),
            None,
        )?)?;
        entries.push((path, blob));
    }
    for (path, blob) in entries {
        checked(run(
            root,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("100644,{},{}", blob.trim(), path),
            ],
            None,
            Some(&index),
        )?)?;
    }
    let tree = checked(run(root, &["write-tree"], None, Some(&index))?)?
        .trim()
        .to_string();
    let same = tree == git(root, &["rev-parse", &format!("{parent}^{{tree}}")])?;
    Ok(Made {
        tree,
        blobs,
        before,
        uploads,
        same,
    })
}

fn commit_inner(root: &Path, edits: &[Edit], message: &str) -> Result<Outcome, CommitError> {
    delivered_by_the_host(root)?;
    let source = root.canonicalize().map_err(|e| e.to_string())?;
    let (saved, refused) = saved_uploads(root, edits);
    let mut last = String::new();
    for _ in 0..5 {
        let parent = git(root, &["rev-parse", "--verify", "-q", &main_ref(root)])?;
        let Made {
            tree,
            blobs,
            before,
            uploads,
            same,
        } = made(root, &parent, edits, &saved)?;
        if same {
            return Ok(Outcome::default());
        }
        let commit = git(root, &["commit-tree", &tree, "-p", &parent, "-m", message])?;
        record(root, &commit)?;
        let trees = main_worktrees(root)?;
        let moved = run(
            root,
            &[
                "update-ref",
                "-m",
                message,
                &main_ref(root),
                &commit,
                &parent,
            ],
            None,
            None,
        )?;
        if !moved.status.success() {
            last = String::from_utf8_lossy(&moved.stderr).trim().to_string();
            if git(root, &["rev-parse", &main_ref(root)])? == parent {
                return Err(format!("git refused to move main: {last}").into());
            }
            continue;
        }
        let mut notes = refused.clone();
        for wt in trees {
            let written = wt == source || wt.canonicalize().is_ok_and(|path| path == source);
            for (rel, text) in &blobs {
                // The source checkout contains the queued save, rather than main's old
                // blob. Merge only edits made after that save onto the committed result.
                let old = if written {
                    &edits
                        .iter()
                        .rev()
                        .find(|edit| &edit.rel == rel)
                        .expect("blob has an edit")
                        .new
                } else {
                    &before[rel]
                };
                if let Err(e) = sync(&wt, rel, old, text) {
                    notes.push(e);
                }
            }
            for path in uploads.keys() {
                if let Err(e) = sync_upload(&wt, path) {
                    notes.push(format!(
                        "{path} in {}: the verdict committed it, but the checkout did not take it: {e}",
                        wt.display()
                    ));
                }
            }
        }
        return Ok(Outcome {
            committed: true,
            notes,
            ..Outcome::default()
        });
    }
    Err(CommitError::Retry(format!(
        "main kept moving under the commit (5 tries): {last}"
    )))
}

/// Refuse a commit made here on the default branch of a project that delivers by pull request
/// (`[land] delivery`, `symphony_board::land::delivery`): there the host alone writes that
/// branch, and a commit made here would be one the remote never holds.
///
/// A save there is kept and delivered by a pull request of its own ([`pending`]); this is asked
/// by the commit every other path ends in, so nothing commits on the default branch by a path
/// that did not ask [`delivers`] first.
///
/// # Errors
///
/// The project delivers by pull request: the line the commit is refused with.
pub fn delivered_by_the_host(root: &Path) -> Result<(), String> {
    if !delivers(root) {
        return Ok(());
    }
    Err(format!(
        "not committed: this project delivers by pull request ([land] delivery), so nothing is \
         committed to {} here: a save waits for the pull request that delivers it",
        Project::of(root).default_branch
    ))
}

/// Whether the project at `root` delivers by pull request: its default branch commits a
/// `[land] delivery` command, so the host alone writes that branch and a save waits for a pull
/// request of its own ([`pending`]).
pub fn delivers(root: &Path) -> bool {
    let at = format!("{}:symphony.toml", main_ref(root));
    git(root, &["show", &at])
        .ok()
        .and_then(|text| symphony_board::land::delivery::of(&text).ok())
        .is_some_and(|command| !command.is_empty())
}

/// Commit edits already written into `root`, then sync main checkouts without losing local edits.
pub fn commit_verdicts(root: &Path, edits: &[Edit], message: &str) -> Outcome {
    let _stage = PublicationStage::start(format_args!("commit verdicts {message:?}"));
    let result = (|| {
        let backlog = Backlog::new(root)?;
        retry_conflicts(root, backlog.sequence, || {
            let _commits = shared_lock(root, "verdict-commits.lock")?;
            let _file = write_lock(root)?;
            commit_inner(root, edits, message)
        })
    })();
    result.unwrap_or_else(|error| Outcome {
        error,
        ..Outcome::default()
    })
}

fn changed(root: &Path) -> Result<Vec<String>, String> {
    let done = checked(run(
        root,
        &["status", "--porcelain", "-z", "-uall"],
        None,
        None,
    )?)?;
    let mut records = done.split('\0').filter(|s| !s.is_empty());
    let mut names = Vec::new();
    while let Some(record) = records.next() {
        names.push(
            record
                .get(3..)
                .ok_or("malformed status record")?
                .to_string(),
        );
        if record.starts_with('R') || record.starts_with('C') {
            records.next();
        }
    }
    Ok(names)
}

fn generate(root: &Path, heavy: bool) -> Result<(), String> {
    let mut wrote = false;
    for generator in generators(root) {
        if generator.when_changed && (!heavy || !wrote) {
            continue;
        }
        let (program, args) = generator.command.split_first().ok_or("empty generator")?;
        let done = symphony_board::sys::output(command(program).args(args).current_dir(root))
            .map_err(|e| e.to_string())?;
        if !done.status.success() {
            return Err(format!(
                "{} failed on it: {}",
                generator.name,
                String::from_utf8_lossy(&done.stderr)
            ));
        }
        wrote |= changed(root)?
            .iter()
            .any(|n| n.starts_with(&generator.outputs));
    }
    Ok(())
}

fn prepared(
    root: &Path,
    edits: &[Edit],
    heavy: bool,
) -> Result<(PrivateWorktree, Vec<String>), CommitError> {
    let parent = git(root, &["rev-parse", &main_ref(root)])?;
    let (blobs, _) = apply(root, &parent, edits)?;
    let wt = PrivateWorktree::new(root, &parent)?;
    for (rel, text) in &blobs {
        std::fs::write(wt.path.join(rel), text).map_err(|e| e.to_string())?;
    }
    generate(&wt.path, heavy)?;
    let outputs = changed(&wt.path)?
        .into_iter()
        .filter(|n| !blobs.contains_key(n))
        .collect();
    Ok((wt, outputs))
}

type Waiting = Arc<Mutex<BTreeMap<u64, Edit>>>;

/// One queued save: the edit, the number it waits under ([`Waiting`]) and its place in the
/// backlog, held until the job is done.
#[derive(Clone)]
struct Job {
    id: u64,
    edit: Edit,
    backlog: Arc<Backlog>,
}

/// The three locks a publication writes main's checkout under, taken in the one order that keeps
/// it deadlock-free: the commits lock, this server's saves, then the write lock a save and the
/// lander share (a save takes the last two in that order too). Released in reverse, the fields
/// being declared last-taken first.
struct PublicationLocks<'a> {
    _file: Option<File>,
    _held: std::sync::MutexGuard<'a, ()>,
    _commits: File,
}

impl<'a> PublicationLocks<'a> {
    fn take(root: &Path, writes: &'a Mutex<()>) -> Result<Self, String> {
        let commits = shared_lock(root, "verdict-commits.lock")?;
        let held = writes.lock().unwrap_or_else(|e| e.into_inner());
        let file = write_lock(root)?;
        Ok(Self {
            _file: file,
            _held: held,
            _commits: commits,
        })
    }
}

type QueuedJob = Arc<Mutex<OutputJob>>;

fn report(root: &Path, message: &str) {
    let message = format!("Symphony: {message}");
    let _ = writeln!(std::io::stderr(), "{message}");
    // Failure to reach the board does not hide the original failure in the server log.
    let slug = format!(
        "tools/review-app-verdict-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    // The app's session is found on the store it files on, held to the project as every reach in
    // this process is (`crate::taskview::serve`): a `tasks` run in the folder would look the board
    // up for itself, and without the folder's `.git` find the board of the checkout above it.
    let filed = (|| {
        let store = crate::taskview::connect(root)?;
        let by = crate::verdicts::app_handle(&store).map_err(|e| e.to_string())?;
        store
            .create(&symphony_board::NewTask {
                slug,
                title: "Symphony could not commit an owner verdict cleanly".into(),
                body: message,
                by,
                priority: 0,
                ..symphony_board::NewTask::default()
            })
            .map_err(|e| e.to_string())?;
        Ok::<_, String>(())
    })();
    if let Err(e) = filed {
        let _ = writeln!(
            std::io::stderr(),
            "Symphony: could not file the verdict failure: {e}"
        );
    }
}

fn app_actor(root: &Path) -> Result<String, String> {
    crate::verdicts::app_actor(&crate::taskcli::CliBoard::new(root)).map_err(|e| e.to_string())
}

struct Released {
    checkout: PathBuf,
    rel: String,
    restore_base: String,
    taken: String,
}

fn release(
    root: &Path,
    branch: &str,
    edit: &Edit,
    waiting: &Waiting,
    ahead: &Ahead,
    id: u64,
) -> Result<Vec<Released>, String> {
    use std::collections::BTreeSet;
    let source = root.canonicalize().map_err(|e| e.to_string())?;
    let project = Project::of(root);
    let base = git(root, &["merge-base", &project.default_branch, branch])?;
    let names = git(
        root,
        &["diff", "--no-renames", "--name-only", &base, branch],
    )?;
    let branch_pages: BTreeSet<String> = names
        .lines()
        .filter(|name| project.is_page(name))
        .map(str::to_owned)
        .collect();
    let pending = waiting.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let output_pages = ahead.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut pages = branch_pages.clone();
    pages.extend(
        pending
            .values()
            .filter(|edit| project.is_page(&edit.rel) && output_pages.contains_key(&edit.rel))
            .map(|edit| edit.rel.clone()),
    );
    let mut released = Vec::new();
    let result = (|| {
        for checkout in main_worktrees(root)? {
            let is_source = checkout.canonicalize().is_ok_and(|path| path == source);
            for rel in &pages {
                // Ordinary untracked files do not make main dirty and are never taken.
                if git(
                    &checkout,
                    &["status", "--porcelain", "--untracked-files=no", "--", rel],
                )?
                .is_empty()
                {
                    continue;
                }
                let head = checked(run(
                    &checkout,
                    &["show", &format!("HEAD:{rel}")],
                    None,
                    None,
                )?)?;
                let on_branch = branch_pages.contains(rel);
                let branch_text = if on_branch {
                    checked(run(
                        root,
                        &["show", &format!("{branch}:{rel}")],
                        None,
                        None,
                    )?)?
                } else {
                    head.clone()
                };
                let Ok(current) = std::fs::read_to_string(checkout.join(rel)) else {
                    continue;
                };
                let owned = if is_source {
                    true
                } else {
                    let mut expected = branch_text.clone();
                    for (_, queued) in pending.iter().filter(|(queued_id, queued)| {
                        queued.rel == *rel && (!on_branch || **queued_id > id)
                    }) {
                        expected = merge(root, &expected, &queued.base, &queued.new)?;
                    }
                    current == branch_text || current == expected
                };
                if !owned {
                    continue;
                }
                git(&checkout, &["checkout", "-q", "HEAD", "--", rel])?;
                released.push(Released {
                    checkout: checkout.clone(),
                    rel: rel.clone(),
                    // This branch already publishes its saved edit. Other queued pages are
                    // absent from it, so restore their full local changes relative to main.
                    restore_base: if on_branch && is_source && *rel == edit.rel {
                        edit.new.clone()
                    } else if on_branch {
                        branch_text
                    } else {
                        head
                    },
                    taken: current,
                });
            }
        }
        Ok::<_, String>(())
    })();
    match result {
        Ok(()) => Ok(released),
        Err(e) => {
            restore(root, released, false)
                .map_err(|restore| format!("{e}; restoring released verdicts failed: {restore}"))?;
            Err(e)
        }
    }
}

fn restore(root: &Path, released: Vec<Released>, landed: bool) -> Result<(), String> {
    for r in released {
        let file = r.checkout.join(&r.rel);
        let current = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
        if landed {
            if r.taken != r.restore_base {
                std::fs::write(file, merge(root, &current, &r.restore_base, &r.taken)?)
                    .map_err(|e| e.to_string())?;
            }
        } else {
            let head = checked(run(
                &r.checkout,
                &["show", &format!("HEAD:{}", r.rel)],
                None,
                None,
            )?)?;
            if current == head {
                std::fs::write(file, r.taken).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Deserialize, Serialize)]
struct Publication {
    branch: String,
    actor: String,
    checkout: PathBuf,
    tip: String,
}

fn output_branch(
    root: &Path,
    edit: &Edit,
    id: u64,
) -> Result<Option<(Publication, PrivateWorktree)>, CommitError> {
    let (wt, made) = prepared(root, std::slice::from_ref(edit), true)?;
    if made.is_empty() {
        return Ok(None);
    }
    let branch = format!(
        "docs/verdict-{}-{id}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    git(&wt.path, &["checkout", "-q", "-b", &branch])?;
    let mut files = changed(&wt.path)?;
    let mut args = vec!["add", "--"];
    args.extend(files.iter().map(String::as_str));
    git(&wt.path, &args)?;
    // The uploads the owner's line names travel with it, past the ignore. Copied after the outputs
    // were read, so an upload alone never makes a generated branch.
    let edited = std::fs::read_to_string(wt.path.join(&edit.rel)).map_err(|e| e.to_string())?;
    let (saved, refused) = saved_uploads(root, std::slice::from_ref(edit));
    for note in &refused {
        report(root, note);
    }
    let uploads = shown_uploads(&saved, &[edited.as_str()]);
    if !uploads.is_empty() {
        for (path, bytes) in &uploads {
            let copy = wt.path.join(path);
            if let Some(folder) = copy.parent() {
                std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
            }
            // Whatever stood there (a link main tracks, say) is replaced, never written through.
            match std::fs::remove_file(&copy) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(format!("{}: {e}", copy.display()).into());
                }
                _ => {}
            }
            std::fs::write(&copy, bytes).map_err(|e| format!("{}: {e}", copy.display()))?;
        }
        let mut args = vec!["add", "-f", "--"];
        args.extend(uploads.keys().map(String::as_str));
        git(&wt.path, &args)?;
        files.extend(uploads.into_keys());
    }
    git(
        &wt.path,
        &[
            "commit",
            "-q",
            "-m",
            &format!(
                "docs: owner verdicts and the content they make stale\n\n{}",
                files.join("\n")
            ),
        ],
    )?;
    let tip = git(&wt.path, &["rev-parse", "HEAD"])?;
    record(root, &tip)?;
    let actor = app_actor(root)?;
    let check = Project::of(&wt.path).verdicts.check.clone();
    if let Some((program, args)) = check.split_first() {
        let done = symphony_board::sys::output(
            command(program)
                .args(args)
                .current_dir(&wt.path)
                .env("TASKS_SESSION", &actor),
        )
        .map_err(|e| e.to_string())?;
        if !done.status.success() {
            return Err(format!(
                "branch {branch}: {} failed: {}{}",
                check.join(" "),
                String::from_utf8_lossy(&done.stdout),
                String::from_utf8_lossy(&done.stderr)
            )
            .into());
        }
    }
    Ok(Some((
        Publication {
            branch,
            actor,
            checkout: wt.path.clone(),
            tip,
        },
        wt,
    )))
}

enum PublicationAttempt {
    Landed,
    Retry,
    Dirty(String),
}

/// Only land decides whether main can advance. A retry keeps the same branch, checkout and
/// verifier records; it never enters the text-conflict retry or generates another branch.
fn publish(
    root: &Path,
    publication: &Publication,
    edit: &Edit,
    waiting: &Waiting,
    ahead: &Ahead,
    id: u64,
    writes: &Mutex<()>,
) -> Result<PublicationAttempt, String> {
    let land = Project::of(&publication.checkout).verdicts.land.clone();
    let Some((program, args)) = land.split_first() else {
        return Err(format!(
            "branch {}: {} names no [verdicts] land command",
            publication.branch,
            crate::project::FILE
        ));
    };
    let released = {
        let _locks = PublicationLocks::take(root, writes)?;
        release(root, &publication.branch, edit, waiting, ahead, id)?
    };
    // Land may verify a moving-main candidate. Neither that work nor the branch's initial
    // verify can hold the app's write locks; land owns its publication CAS and bounded retry.
    let done = symphony_board::sys::output(
        command(program)
            .args(args)
            .arg(&publication.branch)
            .env("TASKS_SESSION", &publication.actor)
            .current_dir(&publication.checkout),
    );
    let landed = done.as_ref().is_ok_and(|done| done.status.success());
    {
        let _locks = PublicationLocks::take(root, writes)?;
        restore(root, released, landed)?;
    }
    let done =
        done.map_err(|e| format!("branch {}: {}: {e}", publication.branch, land.join(" ")))?;
    if landed {
        return Ok(PublicationAttempt::Landed);
    }
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&done.stdout),
        String::from_utf8_lossy(&done.stderr)
    );
    if said.contains(crate::landing::RETRY_LAND) || said.contains(crate::landing::DIRTY_MAIN) {
        let _ = writeln!(
            std::io::stderr(),
            "Symphony: branch {} remains queued: {said}",
            publication.branch
        );
        return Ok(if said.contains(crate::landing::DIRTY_MAIN) {
            PublicationAttempt::Dirty(said)
        } else {
            PublicationAttempt::Retry
        });
    }
    Err(format!(
        "branch {} holds the verdict and its content: {said}",
        publication.branch
    ))
}

#[derive(Deserialize, Serialize)]
struct QueuedPublication {
    edit: Edit,
    publication: Option<Publication>,
    #[serde(default)]
    dirty_retry: bool,
    #[serde(default)]
    dirty_reported: bool,
    /// A save of a project that delivers by pull request, and how far its delivery went
    /// ([`pending`]): its entry is kept until a receipt proves its delivery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    delivery: Option<pending::Delivery>,
}

/// The locked journal retains immutable edits and the verified checkout across a bounded land
/// retry or a server handover. Another serving generation may resume it after this worker closes.
struct OutputJob {
    job: Job,
    path: PathBuf,
    held: JournalLease,
    saved: QueuedPublication,
    checkout: Option<PrivateWorktree>,
    /// Its journal entry outlives this job: a save still waiting for its pull request.
    kept: bool,
}

struct JournalLease(File);

impl Drop for JournalLease {
    fn drop(&mut self) {
        // A concurrent fork may still hold the open file description until exec.
        let _ = self.0.unlock();
    }
}

impl OutputJob {
    fn folder(root: &Path) -> Result<PathBuf, String> {
        let folder = home(root)?.join(JOURNAL);
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        Ok(folder)
    }

    fn new(root: &Path, job: Job) -> Result<Self, String> {
        let path = Self::folder(root)?.join(format!("{:020}.json", job.backlog.sequence));
        let saved = QueuedPublication {
            edit: job.edit.clone(),
            publication: None,
            dirty_retry: false,
            dirty_reported: false,
            delivery: delivers(root).then(|| pending::Delivery::saved(root)),
        };
        let held = Self::write_journal(&path, &saved)?;
        Ok(Self {
            saved,
            job,
            path,
            held,
            checkout: None,
            kept: false,
        })
    }

    fn write_journal(path: &Path, saved: &QueuedPublication) -> Result<JournalLease, String> {
        let temporary = path.with_extension(format!(
            "{}-{}.pending",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.lock().map_err(|e| e.to_string())?;
        let mut held = JournalLease(file);
        serde_json::to_writer(&mut held.0, saved).map_err(|e| e.to_string())?;
        held.0.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, path).map_err(|e| e.to_string())?;
        Ok(held)
    }

    fn save(&mut self) -> Result<(), String> {
        self.held = Self::write_journal(&self.path, &self.saved)?;
        Ok(())
    }

    fn recover(root: &Path) -> Result<Vec<Self>, String> {
        let folder = Self::folder(root)?;
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(&folder).map_err(|e| e.to_string())? {
            match entry {
                Ok(entry) => paths.push(entry.path()),
                Err(error) => report_journal_error(root, &folder, &error.to_string()),
            }
        }
        paths.sort();
        let mut found = Vec::new();
        for path in paths {
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let file = match OpenOptions::new().read(true).write(true).open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    report_journal_error(root, &path, &error.to_string());
                    continue;
                }
            };
            let recovered = Self::recover_opened(root, &path, file);
            match recovered {
                Ok(Some(job)) => {
                    clear_journal_error(&path);
                    found.push(job);
                }
                Ok(None) => {
                    if !path.exists() {
                        clear_journal_error(&path);
                    }
                }
                Err(error) => report_journal_error(root, &path, &error),
            }
        }
        Ok(found)
    }

    fn recover_opened(root: &Path, path: &Path, file: File) -> Result<Option<Self>, String> {
        use std::os::unix::fs::MetadataExt as _;
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
            Err(std::fs::TryLockError::Error(e)) => return Err(e.to_string()),
        }
        let held = JournalLease(file);
        let current = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let locked = held.0.metadata().map_err(|e| e.to_string())?;
        // save() may have replaced the path while this reader waited on the old inode.
        // That old descriptor has no authority to adopt or remove the live publication.
        if (locked.dev(), locked.ino()) != (current.dev(), current.ino()) {
            return Ok(None);
        }
        let saved: QueuedPublication =
            serde_json::from_reader(&held.0).map_err(|e| format!("{}: {e}", path.display()))?;
        if let Some(publication) = &saved.publication {
            if run(
                root,
                &[
                    "merge-base",
                    "--is-ancestor",
                    &publication.tip,
                    &Project::of(root).default_branch,
                ],
                None,
                None,
            )?
            .status
            .success()
            {
                if publication.checkout.exists()
                    && git(&publication.checkout, &["rev-parse", "HEAD"])? == publication.tip
                {
                    PrivateWorktree::resume(root, publication.checkout.clone())?.remove();
                }
                std::fs::remove_file(path).map_err(|e| e.to_string())?;
                return Ok(None);
            }
            if git(&publication.checkout, &["rev-parse", "HEAD"])? != publication.tip
                || git(&publication.checkout, &["symbolic-ref", "--short", "HEAD"])?
                    != publication.branch
            {
                return Err(format!(
                    "queued branch {} changed after verification",
                    publication.branch
                ));
            }
        }
        let checkout = saved
            .publication
            .as_ref()
            .map(|publication| PrivateWorktree::resume(root, publication.checkout.clone()))
            .transpose()?;
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let backlog = Arc::new(Backlog::new(root)?);
        Ok(Some(Self {
            job: Job {
                id,
                edit: saved.edit.clone(),
                backlog,
            },
            path: path.to_path_buf(),
            held,
            saved,
            checkout,
            kept: false,
        }))
    }

    fn attempt(
        &mut self,
        root: &Path,
        waiting: &Waiting,
        ahead: &Ahead,
        writes: &Mutex<()>,
    ) -> Result<bool, String> {
        // A save the host delivers: committed on a branch of its own and kept until the remote
        // holds it. One journaled before the key was set goes the same way, never to main.
        if self.saved.publication.is_none() && (self.saved.delivery.is_some() || delivers(root)) {
            self.kept = self.deliver(root)?;
            return Ok(true);
        }
        if self.saved.publication.is_none() {
            let branch = retry_conflicts(root, self.job.backlog.sequence, || {
                output_branch(root, &self.job.edit, self.job.id)
            })?;
            if let Some((publication, mut checkout)) = branch {
                self.saved.publication = Some(publication);
                self.save()?;
                checkout.retain();
                self.checkout = Some(checkout);
            } else {
                let outcome = retry_conflicts(root, self.job.backlog.sequence, || {
                    let _locks = PublicationLocks::take(root, writes)?;
                    // A recovered draft may have been temporarily released by a publisher
                    // before the process stopped. Restore a clean source before the normal
                    // private-index commit, without replacing later uncommitted work.
                    let current = std::fs::read_to_string(root.join(&self.job.edit.rel)).ok();
                    let head = run(
                        root,
                        &["show", &format!("HEAD:{}", self.job.edit.rel)],
                        None,
                        None,
                    )?;
                    let head = if head.status.success() {
                        Some(checked(head)?)
                    } else {
                        None
                    };
                    if on_main(root) && current == head {
                        let path = root.join(&self.job.edit.rel);
                        if let Some(parent) = path.parent() {
                            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                        }
                        std::fs::write(path, &self.job.edit.new).map_err(|e| e.to_string())?;
                    }
                    commit_inner(
                        root,
                        std::slice::from_ref(&self.job.edit),
                        "docs: owner verdicts",
                    )
                })?;
                for note in outcome.notes {
                    report(root, &note);
                }
                return Ok(true);
            }
        }
        let result = publish(
            root,
            self.saved
                .publication
                .as_ref()
                .expect("publication prepared"),
            &self.job.edit,
            waiting,
            ahead,
            self.job.id,
            writes,
        )?;
        match result {
            PublicationAttempt::Landed => Ok(true),
            PublicationAttempt::Retry => Ok(false),
            PublicationAttempt::Dirty(why) => {
                let outside = {
                    let _file = write_lock(root)?;
                    let pending = waiting.lock().unwrap_or_else(|e| e.into_inner());
                    let pages: std::collections::BTreeSet<_> =
                        pending.values().map(|edit| edit.rel.as_str()).collect();
                    let mut outside = std::collections::BTreeSet::new();
                    for checkout in main_worktrees(root)? {
                        for rel in git(&checkout, &["diff", "--name-only", "HEAD", "--"])?.lines() {
                            if !pages.contains(rel) {
                                outside.insert(rel.to_owned());
                            }
                        }
                    }
                    outside
                };
                let report_now =
                    !self.saved.dirty_reported && (self.saved.dirty_retry || !outside.is_empty());
                self.saved.dirty_retry = true;
                self.saved.dirty_reported |= report_now;
                self.save()?;
                if report_now {
                    let branch = &self
                        .saved
                        .publication
                        .as_ref()
                        .expect("publication prepared")
                        .branch;
                    report(
                        root,
                        &format!(
                            "verified publication {branch} remains queued after dirty-main refusal; paths outside queued pages: {outside:?}; {why}"
                        ),
                    );
                }
                Ok(false)
            }
        }
    }

    fn finished(&mut self) -> Result<(), String> {
        if self.kept {
            return Ok(());
        }
        std::fs::remove_file(&self.path).map_err(|e| e.to_string())?;
        clear_journal_error(&self.path);
        if let Some(checkout) = self.checkout.take() {
            checkout.remove();
        }
        Ok(())
    }

    fn failed_draft(&mut self) -> Result<(), String> {
        let failed = self.path.with_extension("failed");
        std::fs::rename(&self.path, &failed).map_err(|e| e.to_string())?;
        clear_journal_error(&self.path);
        self.path = failed;
        Ok(())
    }
}

type Ahead = Arc<Mutex<BTreeMap<String, usize>>>;

fn journal_reports() -> &'static Mutex<std::collections::BTreeSet<PathBuf>> {
    static REPORTED: std::sync::OnceLock<Mutex<std::collections::BTreeSet<PathBuf>>> =
        std::sync::OnceLock::new();
    REPORTED.get_or_init(Mutex::default)
}

fn report_journal_error(root: &Path, path: &Path, error: &str) {
    if !journal_reports()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(path.to_path_buf())
    {
        return;
    }
    let marker = path.with_extension("reported");
    match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&marker)
    {
        Ok(mut file) => {
            let _ = writeln!(file, "{error}");
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return,
        Err(e) => {
            let _ = writeln!(
                std::io::stderr(),
                "Symphony: cannot mark {} reported: {e}",
                path.display()
            );
        }
    }
    report(
        root,
        &format!("queued publication journal {}: {error}", path.display()),
    );
}

fn clear_journal_error(path: &Path) {
    let _ = std::fs::remove_file(path.with_extension("reported"));
    journal_reports()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(path);
}

fn recover_outputs(
    root: &Path,
    queued: &mut std::collections::VecDeque<QueuedJob>,
    waiting: &Waiting,
    ahead: &Ahead,
) {
    // A branch-only Symphony server must not adopt main's publication queue or rewrite
    // its own clean document files with a main-checkout save.
    if !on_main(root) {
        return;
    }
    match OutputJob::recover(root) {
        Ok(jobs) => {
            for job in jobs {
                waiting
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(job.job.id, job.job.edit.clone());
                *ahead
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .entry(job.job.edit.rel.clone())
                    .or_default() += 1;
                queued.push_back(Arc::new(Mutex::new(job)));
            }
            symphony_board::order::sort_by_key(queued.make_contiguous(), &|job| {
                job.lock().unwrap_or_else(|e| e.into_inner()).path.clone()
            });
        }
        Err(error) => {
            let path = home(root)
                .map(|home| home.join(JOURNAL))
                .unwrap_or_else(|_| root.join(crate::project::Project::of(root).artifact(JOURNAL)));
            report_journal_error(root, &path, &error);
        }
    }
}

fn finish_output(job: &Job, waiting: &Waiting, ahead: &Ahead) {
    waiting
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&job.id);
    let mut ahead = ahead.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(n) = ahead.get_mut(&job.edit.rel) {
        *n -= 1;
        if *n == 0 {
            ahead.remove(&job.edit.rel);
        }
    }
}

fn finish_job(root: &Path, job: &mut OutputJob, outcome: Result<(), String>) {
    match outcome {
        Ok(()) => {
            if let Err(error) = job.finished() {
                report(root, &error);
            }
        }
        Err(error) => {
            // A failed draft has no published branch that necessarily holds the edit.
            // Keep its immutable data for the filed task, but do not reverify it on saves.
            let draft = job.saved.publication.is_none();
            let cleanup = if draft {
                job.failed_draft()
            } else {
                job.finished()
            };
            let note = if draft {
                format!("; immutable edit retained in {}", job.path.display())
            } else {
                String::new()
            };
            let cleanup = cleanup
                .err()
                .map(|why| format!("; publication cleanup failed: {why}"))
                .unwrap_or_default();
            report(root, &format!("{error}{note}{cleanup}"));
        }
    }
}

fn publication_pass(
    root: &Path,
    queued: &mut std::collections::VecDeque<QueuedJob>,
    waiting: &Waiting,
    ahead: &Ahead,
    writes: &Mutex<()>,
) {
    while let Some(queued_job) = queued.pop_front() {
        let mut job = queued_job.lock().unwrap_or_else(|e| e.into_inner());
        match job.attempt(root, waiting, ahead, writes) {
            Ok(false) => {
                drop(job);
                queued.push_front(queued_job);
                break;
            }
            outcome => {
                finish_output(&job.job, waiting, ahead);
                finish_job(root, &mut job, outcome.map(|_| ()));
            }
        }
    }
}

/// The output worker's loop: a `pass` for each message that wakes it, handed the jobs of that
/// message and of every one sent before the pass began, until every sender is gone. A message is
/// a job, or a wake with none (`None`); a burst of wakes sent while a pass is pending or running
/// asks for one more pass, never one each.
fn passes<T>(messages: &mpsc::Receiver<Option<T>>, mut pass: impl FnMut(Vec<T>)) {
    while let Ok(first) = messages.recv() {
        pass(
            first
                .into_iter()
                .chain(messages.try_iter().flatten())
                .collect(),
        );
    }
}

/// A fast commit worker and an output worker; saves only enqueue their immutable before/after edit.
/// Same-file edits wait behind any generated branch they were written against.
pub struct Committer {
    send: Mutex<Option<mpsc::Sender<QueuedJob>>>,
    waiting: Waiting,
    workers: Mutex<Vec<std::thread::JoinHandle<()>>>,
    root: PathBuf,
    /// What wakes the output worker for a pass with no new save ([`Committer::wake`]).
    wake: Mutex<Option<mpsc::Sender<Option<QueuedJob>>>>,
}
impl Committer {
    pub fn start(root: &Path, writes: Arc<Mutex<()>>) -> Self {
        let source = root.to_path_buf();
        let (send, recv) = mpsc::channel::<QueuedJob>();
        let (jobs, output_jobs) = mpsc::channel::<Option<QueuedJob>>();
        let wake = jobs.clone();
        let waiting: Waiting = Arc::default();
        let ahead = Arc::new(Mutex::new(BTreeMap::<String, usize>::new()));
        let mut recovered = std::collections::VecDeque::new();
        recover_outputs(root, &mut recovered, &waiting, &ahead);
        let output_root = root.to_path_buf();
        let output_writes = writes.clone();
        let output_waiting = waiting.clone();
        let output_ahead = ahead.clone();
        let output_worker = std::thread::spawn(move || {
            let mut queued = recovered;
            publication_pass(
                &output_root,
                &mut queued,
                &output_waiting,
                &output_ahead,
                &output_writes,
            );
            passes(&output_jobs, |jobs| {
                // A different serving generation may have released a queued publication
                // since startup. Every subsequent save can adopt it once that lease ends.
                recover_outputs(&output_root, &mut queued, &output_waiting, &output_ahead);
                queued.extend(jobs);
                publication_pass(
                    &output_root,
                    &mut queued,
                    &output_waiting,
                    &output_ahead,
                    &output_writes,
                );
            });
        });
        let root = root.to_path_buf();
        let worker_waiting = waiting.clone();
        let commit_worker = std::thread::spawn(move || {
            for queued_job in recv {
                let Job { id, edit, backlog } = queued_job
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .job
                    .clone();
                let deferred = ahead
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .contains_key(&edit.rel);
                // Delivered by the host: the output worker commits it on a branch of its own.
                let prepared = if deferred || delivers(&root) {
                    Ok(true)
                } else {
                    retry_conflicts(&root, backlog.sequence, || {
                        prepared(&root, std::slice::from_ref(&edit), false)
                            .map(|(_, made)| !made.is_empty())
                    })
                };
                let outcome = match prepared {
                    Ok(true) => {
                        *ahead
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .entry(edit.rel.clone())
                            .or_default() += 1;
                        if let Err(e) = jobs.send(Some(queued_job)) {
                            report(&root, &e.to_string());
                        }
                        continue;
                    }
                    Ok(false) => {
                        let outcome = retry_conflicts(&root, backlog.sequence, || {
                            let _locks = PublicationLocks::take(&root, &writes)?;
                            commit_inner(&root, std::slice::from_ref(&edit), "docs: owner verdicts")
                        });
                        match outcome {
                            Ok(outcome) => {
                                for note in outcome.notes {
                                    report(&root, &note);
                                }
                                Ok(())
                            }
                            Err(why) => Err(why),
                        }
                    }
                    Err(e) => Err(e),
                };
                finish_job(
                    &root,
                    &mut queued_job.lock().unwrap_or_else(|e| e.into_inner()),
                    outcome,
                );
                worker_waiting
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
                // A plain save supplies its pass after checkout synchronization. Waking
                // before that commit lets land observe the save's temporary tracked dirt.
                let _ = jobs.send(None);
            }
        });
        Self {
            send: Mutex::new(Some(send)),
            waiting,
            workers: Mutex::new(vec![commit_worker, output_worker]),
            root: source,
            wake: Mutex::new(Some(wake)),
        }
    }

    /// Ask the output worker for one more pass over the journal, with no new save: what takes
    /// up the saves kept for delivery by pull request again ([`pending`]) once something they
    /// wait on may have moved. Wakes asked while a pass is pending or running ask for that one
    /// more pass together ([`passes`]). Nothing once the committer is closing.
    pub fn wake(&self) {
        if let Some(wake) = &*self.wake.lock().unwrap_or_else(|e| e.into_inner()) {
            let _ = wake.send(None);
        }
    }

    pub fn submit(&self, edit: Edit) -> Result<(), String> {
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let backlog = Arc::new(Backlog::new(&self.root)?);
        // Publish the immutable edit before registering it as a page another publication
        // may temporarily release. A crash can then recover even a not-yet-generated save.
        let job = Arc::new(Mutex::new(OutputJob::new(
            &self.root,
            Job {
                id,
                edit: edit.clone(),
                backlog,
            },
        )?));
        self.waiting
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, edit.clone());
        self.send
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .ok_or("verdict committer is closing")?
            .send(job)
            .map_err(|e| e.to_string())
    }

    /// Drain both paths before the server exits. A successful save must survive a graceful stop.
    pub fn close(&self) {
        self.send.lock().unwrap_or_else(|e| e.into_inner()).take();
        self.wake.lock().unwrap_or_else(|e| e.into_inner()).take();
        for worker in self
            .workers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
        {
            if worker.join().is_err() {
                let _ = writeln!(
                    std::io::stderr(),
                    "Symphony: verdict worker panicked while closing"
                );
            }
        }
    }
}

pub mod pending;

#[cfg(test)]
mod passes_tests;

#[cfg(test)]
mod publication_tests;

#[cfg(test)]
mod history_tests;

#[cfg(test)]
mod uploads_tests;

#[cfg(test)]
mod report_tests;
