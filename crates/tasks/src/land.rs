//! `just land SLUG` (`tasks land`): the Symphony's default landing. `symphony init` installs its
//! recipe in every project (`symphony.just`), and a `land` recipe of the project's own replaces it
//! there, everywhere. It lands one task's branch, named as its slug, on the default branch:
//!
//! 0. **Asked for.** It runs only as `just land SLUG`, whose recipe sets [`RECIPE`], and never
//!    where the project's justfile runs a `land` of its own ([`admit`]). The marker is a guard
//!    against running it by accident; the approval below is the gate.
//! 1. **Approved.** The task's last verdict is an `approve`, which left it done, from a fresh
//!    reviewer: never a session that ever executed the task or one of its work subtasks (an
//!    instruction subtask, the review the reviewer claimed, is none), nor one that ever
//!    orchestrated it, nor its filer, but on a fast fix ([`crate::fast_fix`]), whose
//!    orchestrator's approve it goes by ([`independent`]). The approval covers the branch's tip:
//!    the commit the verdict names, else, when it names none of the branch's, every commit of the
//!    branch made before it and before the task last went to review (the board records no tip, so
//!    the landing says on the thread that it judged by time). A commit it does not cover refuses,
//!    naming it. An approve that does not do sends the task back to review for a fresh one; a
//!    conflict or a red on the tip as approved is said to its executor, which fixes it on the
//!    branch and sends the task back to review itself. A tip the default branch holds already is
//!    finished only when this land recorded its merge ([`Record`]): a land interrupted after it
//!    moved the default branch pushes the commit it checked, writes its landed line and sweeps,
//!    nothing merged again; a tip merged by hand is refused, nothing pushed or recorded.
//! 2. **Merged and checked.** In a worktree of its own (`land` in the project's `[agents]
//!    worktrees`, else `sf-tasks/land/worktree` in the git common dir), never the main checkout:
//!    one `--no-ff` merge commit, "Merge branch 'SLUG'", of the tip into the default branch; the
//!    project's `[land] generators`, whose changes, inside their outputs alone, are one commit
//!    after it; then `[verdicts] check`, which must pass on that tree. A conflict or a red refuses.
//! 3. **Moved.** The land records its merge and the commit the check passed on, in
//!    `sf-tasks/land/` and on the thread; then the main checkout, with no staged or tracked edit
//!    and on the default branch, fast-forwards to what passed, so the default branch's new tree is
//!    the tree the check passed on. When the default branch moved meanwhile, the merge and the
//!    check run again on its new tip, at most [`TRIES`] times; a red then is said on the thread to
//!    the orchestrator.
//! 4. **Pushed.** Unless `[land] push = false` or there is no `origin`: the default branch is
//!    fetched from `origin`; what it gained there is merged in the same way, generators and check
//!    again, never forced; then the default branch alone is pushed, no other branch and no tag. A
//!    push that fails leaves the landing done here and says so, with the command to retry.
//! 5. **Closed and swept.** The board's `landed` writes `landed SLUG on main at SHA` on the
//!    thread and closes the task (an approved one stays done), and the sweep removes its finished
//!    worktrees.
//!
//! Two lands of one repository wait for each other ([`LOCK`] in `sf-tasks/` of the common dir), for
//! the land's timeout at most. Each generator and the check run in a process group of their own,
//! ended whole past `[land] timeout_seconds` ([`TIMEOUT`] unless the project says), and a fetch or
//! a push within [`NETWORK`]; what is left of a group once its first process ends is ended too.
//! A hang-up, an interrupt or a terminate signal (a terminal's Ctrl-C, `kill`) ends the group
//! running, removes the land's worktree and ends the land by that signal: before the default
//! branch moved nothing landed, and after it a run again finishes it. The group running is named
//! in [`RUNNING`] meanwhile, so a land killed outright leaves its name: the next land refuses
//! while that group lives, or while any process works in the worktree it left. What the
//! generators and the check print goes to `sf-tasks/land/SLUG.log`. The settings are
//! read from the default branch as the main checkout has it committed, never from the branch
//! landing, so a branch cannot change its own check.

use crate::cli::Env;
use crate::on_review::{self, Started};
use crate::sweep::git::{ancestor, git, rev, succeeds, worktrees};
use crate::sweep::{self, trigger};
use crate::sys::{self, Stdio};
use crate::{Error, Held, Message, Store, Task};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

/// What a main checkout with staged or tracked edits refuses with; the verdict publisher reads it.
pub const DIRTY_MAIN: &str = "has staged or tracked edits; refusing publication";
/// The lock two lands of one repository take in turn, in the board's folder of the common dir.
pub const LOCK: &str = "land.lock";
/// The folder of the lands' logs there, and of their worktree when the project names no folder.
pub const FOLDER: &str = "land";
/// The project file, read from the default branch.
pub const MANIFEST: &str = "symphony.toml";
/// How many times a land merges and checks again as the default branch or `origin` moves.
pub const TRIES: usize = 3;
/// The remote a land pushes to.
pub const REMOTE: &str = "origin";
/// The start of the reason a land's worktree is locked with, which keeps it from the sweep while
/// the land it names runs.
pub(crate) const REASON: &str = "symphony land";
/// What the `land` recipe `symphony init` installs sets, by which `tasks land` knows `just land`
/// runs it: the board's commands, which an agent may run unasked, never land by accident. It is a
/// guard against a reflex, not a barrier: a command that sets it lands, and the approval
/// ([`independent`], [`covered`]) is the gate.
pub const RECIPE: &str = "SYMPHONY_LAND";
/// The file in the lands' folder naming the process group a land runs, while it runs.
pub const RUNNING: &str = "running";
/// How long a group asked to end has before it is made to.
const GRACE: Duration = Duration::from_secs(5);
/// How often a land's watchdog looks for a signal while a child runs.
const POLL: Duration = Duration::from_millis(50);
/// The signal that ended the last land this process ran, else 0 ([`interrupted`]).
static ENDED_BY: AtomicI32 = AtomicI32::new(0);

/// The signal that interrupted the land this process ran, if one did: the command line ends by it
/// once it has said why, as a command a signal ends does.
pub fn interrupted() -> Option<i32> {
    Some(ENDED_BY.load(Ordering::Relaxed)).filter(|signal| *signal != 0)
}

/// `signal` by its name.
fn signal_name(signal: i32) -> String {
    match signal {
        libc::SIGHUP => "SIGHUP".into(),
        libc::SIGINT => "SIGINT".into(),
        libc::SIGTERM => "SIGTERM".into(),
        other => format!("signal {other}"),
    }
}
/// What names a repository to git: a child run in the land's worktree inherits none of them.
const GIT_VARS: [&str; 8] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
];

/// A command a land runs on the merge before its check, whose changes inside `outputs` are
/// committed with it; a project names its own in `[land] generators`, as `[verdicts] generators`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generator {
    pub name: String,
    pub outputs: String,
    pub command: Vec<String>,
    /// Run only after an earlier generator changed something.
    #[serde(default)]
    pub when_changed: bool,
}

/// What a land reads from the project's file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// `[project] default_branch`.
    pub default_branch: String,
    /// `[verdicts] check`: what must pass on the merge; empty runs none.
    pub check: Vec<String>,
    /// `[agents] worktrees`, relative to the root.
    pub worktrees: Option<String>,
    /// `[land] generators`, in order.
    pub generators: Vec<Generator>,
    /// `[land] push`: whether a landing pushes the default branch to `origin`.
    pub push: bool,
    /// `[land] timeout_seconds`: how long each generator and the check may run, and a land waits
    /// for another; a fetch or a push has [`NETWORK`] at most.
    pub timeout: Duration,
}

/// How long a land's generators, check and wait for another land take at most, unless the project
/// names another `[land] timeout_seconds`: an hour.
pub const TIMEOUT: Duration = Duration::from_secs(3600);
/// How long a fetch or a push may take at most, the project's timeout when shorter: five minutes.
pub const NETWORK: Duration = Duration::from_secs(300);

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            default_branch: "main".into(),
            check: Vec::new(),
            worktrees: None,
            generators: Vec::new(),
            push: true,
            timeout: TIMEOUT,
        }
    }
}

impl Settings {
    /// `text` as a project file: only the keys a land reads, the rest left to the Symphony's own
    /// reading, but the whole of `[land]`, which is the land's alone: a key it does not know there
    /// (a typo of `push`, say) is refused by its name, as is a generator with a key it does not
    /// know, no command or no outputs, and a timeout of no time.
    pub fn parse(text: &str) -> Result<Settings, String> {
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct Manifest {
            project: Project,
            agents: Agents,
            verdicts: Verdicts,
            land: Land,
        }
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct Project {
            default_branch: Option<String>,
        }
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct Agents {
            worktrees: Option<String>,
        }
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct Verdicts {
            check: Vec<String>,
        }
        #[derive(Deserialize)]
        #[serde(default, deny_unknown_fields)]
        struct Land {
            generators: Vec<Generator>,
            push: bool,
            timeout_seconds: u64,
        }
        impl Default for Land {
            fn default() -> Land {
                Land {
                    generators: Vec::new(),
                    push: true,
                    timeout_seconds: TIMEOUT.as_secs(),
                }
            }
        }
        let file: Manifest = toml::from_str(text)
            .map_err(|error| format!("{MANIFEST}: {}", error.to_string().trim_end()))?;
        let default_branch = file.project.default_branch.unwrap_or_else(|| "main".into());
        if default_branch.trim().is_empty() {
            return Err(format!("{MANIFEST}: [project] default_branch is empty"));
        }
        let mut generators = file.land.generators;
        for generator in &mut generators {
            if generator.command.first().is_none_or(String::is_empty) {
                return Err(format!(
                    "{MANIFEST}: [land] generator {} has no command",
                    generator.name
                ));
            }
            generator.outputs = inside(&generator.outputs).ok_or_else(|| {
                format!(
                    "{MANIFEST}: [land] generator {} writes {:?}, not a path inside the project",
                    generator.name, generator.outputs
                )
            })?;
        }
        if file.land.timeout_seconds == 0 {
            return Err(format!(
                "{MANIFEST}: [land] timeout_seconds is 0, which no generator or check runs in"
            ));
        }
        Ok(Settings {
            default_branch,
            check: file.verdicts.check,
            worktrees: file.agents.worktrees,
            generators,
            push: file.land.push,
            timeout: Duration::from_secs(file.land.timeout_seconds),
        })
    }

    /// How long a fetch or a push may take: [`NETWORK`], or the project's timeout when shorter.
    pub fn network(&self) -> Duration {
        self.timeout.min(NETWORK)
    }

    /// The settings the main checkout `main` has committed at its head; the defaults with no file.
    pub fn of(main: &Path) -> Result<Settings, String> {
        let at = format!("HEAD:{MANIFEST}");
        if !succeeds(main, &["cat-file", "-e", &at]) {
            return Ok(Settings::default());
        }
        Settings::parse(&run_git(main, &["show", &at])?)
    }
}

/// `path` as a prefix of the project's changed paths, as the Symphony reads a generator's
/// outputs: its `.` parts dropped and a folder's slash kept (so `gen/` is never `gen-x/`), or none
/// for a path that is empty or leaves the project.
fn inside(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for part in Path::new(path).components() {
        match part {
            std::path::Component::Normal(name) => parts.push(name.to_string_lossy()),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    if parts.is_empty() {
        return None;
    }
    let slash = if path.ends_with('/') { "/" } else { "" };
    Some(parts.join("/") + slash)
}

/// The approval a land goes by: who gave it, when, what it said, and when the review it ends began
/// (the task's last move into review before it), if the thread says.
#[derive(Debug, Clone, PartialEq)]
pub struct Approval {
    pub by: String,
    pub at: f64,
    pub text: String,
    pub review_from: Option<f64>,
}

/// The approval standing on `task`, read from its `thread`: it is done, and its last verdict is an
/// `approve` ([`crate::approved`]). Otherwise the one line saying why there is none.
pub fn approval(task: &Task, thread: &[Message]) -> Result<Approval, String> {
    let slug = &task.slug;
    if task.status != "done" {
        return Err(format!(
            "{slug} is {}, with no approving verdict standing",
            task.status
        ));
    }
    if let Some(verdict) = crate::approved(thread) {
        let into_review = format!("{}review", crate::stages::ARROW);
        let review_from = thread
            .iter()
            .filter(|m| m.kind == "system" && m.ts <= verdict.ts)
            .rfind(|m| m.text.ends_with(&into_review))
            .map(|m| m.ts);
        return Ok(Approval {
            by: verdict.sender.clone(),
            at: verdict.ts,
            text: verdict.text.clone(),
            review_from,
        });
    }
    let verdicts: Vec<&Message> = thread.iter().filter(|m| m.kind == "verdict").collect();
    let approve = |m: &&Message| crate::approved(std::slice::from_ref(*m)).is_some();
    match verdicts.last() {
        Some(last) if verdicts.iter().any(approve) => Err(format!(
            "{slug} was closed after its approval: {}",
            first_line(&last.text)
        )),
        _ => Err(format!(
            "{slug} is done with no approving verdict from a reviewer"
        )),
    }
}

/// Whether `approval` is one the land goes by on `task`, whose roles `held` says who ever held
/// ([`Store::ever_held`]): never the approve of a session that executed it or a work subtask under
/// it, though the reviewer that claimed its review instruction may give it (RULES.md
/// `workflow/reviewers-not-the-owner`, as the board's verdict holds a reviewer to), and
/// never of one that orchestrated it, now or before, or filed it, but on a fast fix
/// ([`crate::fast_fix`]), which its orchestrator approves on its posted evidence when it did not
/// execute it. Otherwise the one line saying whose approve it is and whose it must be.
pub fn independent(task: &Task, approval: &Approval, held: &Held) -> Result<(), String> {
    let (slug, by) = (&task.slug, &approval.by);
    if held.executed.contains(by) {
        return Err(format!(
            "{slug}'s approve is {by}'s, which executed it or work under it: a fresh reviewer, one \
             that did not write or execute it, approves it"
        ));
    }
    if task.fast_fix().is_some() {
        return Ok(());
    }
    let whose =
        if held.orchestrated.contains(by) || task.holders("orchestrator").contains(&by.as_str()) {
            "which orchestrated it"
        } else if task.created_by == *by {
            "which filed it"
        } else {
            return Ok(());
        };
    Err(format!(
        "{slug}'s approve is {by}'s, {whose}: a fresh reviewer approves it, and its orchestrator \
         only a fast fix (filed with `just tasks create --fast-fix WHY`)"
    ))
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
}

/// The words of `text` that may name a commit: 7 to 40 lowercase hex digits, one a digit at least
/// (so a word such as "defaced" is none), between characters that are not letters, digits or `_`
/// (so an identifier such as `4b5e7b0_tip` is none).
pub fn named(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|word| {
            (7..=40).contains(&word.len())
                && word
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                && word.bytes().any(|b| b.is_ascii_digit())
        })
        .collect()
}

/// How an approval covers the tip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Covered {
    /// It names the tip.
    Named,
    /// It names none of the branch's commits, and each of the branch's `n` commits is older.
    ByTime(usize),
}

/// One commit as `git log` gives it.
#[derive(Debug, Clone, PartialEq)]
struct Commit {
    sha: String,
    time: f64,
    subject: String,
}

/// The commits of `range` in the checkout `dir`, newest first.
fn log(dir: &Path, range: &str) -> Result<Vec<Commit>, String> {
    let text = run_git(
        dir,
        &["log", "--topo-order", "--format=%H%x09%ct%x09%s", range],
    )?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            Some(Commit {
                sha: parts.next()?.to_string(),
                time: parts.next()?.parse().ok()?,
                subject: parts.next().unwrap_or_default().to_string(),
            })
        })
        .collect())
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

fn commits(n: usize) -> String {
    if n == 1 {
        "1 commit".into()
    } else {
        format!("{n} commits")
    }
}

/// Whether `approval` covers `tip`, the branch `slug`'s, over `base` in the checkout `dir`.
pub fn covered(
    dir: &Path,
    slug: &str,
    base: &str,
    tip: &str,
    approval: &Approval,
) -> Result<Covered, String> {
    let resolved: Vec<String> = named(&approval.text)
        .into_iter()
        .filter_map(|word| rev(dir, &format!("{word}^{{commit}}")))
        .collect();
    if resolved.iter().any(|sha| sha == tip) {
        return Ok(Covered::Named);
    }
    let branch = log(dir, &format!("{base}..{tip}"))?;
    if let Some(approved) = branch.iter().find(|c| resolved.contains(&c.sha)) {
        let after = log(dir, &format!("{}..{tip}", approved.sha))?;
        let first = after.last().ok_or("the tip is after the approved commit")?;
        return Err(format!(
            "{slug} has {} after the approved {}: {} {}",
            commits(after.len()),
            short(&approved.sha),
            short(&first.sha),
            first.subject
        ));
    }
    if let Some(gone) = resolved
        .iter()
        .find(|sha| !ancestor(dir, sha, tip) && !ancestor(dir, sha, base))
    {
        return Err(format!(
            "the approval of {slug} names {}, which its tip {} does not hold",
            short(gone),
            short(tip)
        ));
    }
    let late: Vec<&Commit> = branch.iter().filter(|c| c.time > approval.at).collect();
    if let Some(first) = late.last() {
        return Err(format!(
            "{slug} has {} made after its approval at {} (which names none of its commits, so \
             judged by time): {} {}",
            commits(late.len()),
            sys::local_minute(approval.at),
            short(&first.sha),
            first.subject
        ));
    }
    // Made while it was in review, maybe after its reviewer read it: only a named tip covers them.
    if let Some(from) = approval.review_from {
        let during: Vec<&Commit> = branch.iter().filter(|c| c.time > from).collect();
        if let Some(first) = during.last() {
            return Err(format!(
                "{slug} has {} made during its review (it went to review at {}), which an \
                 approval naming none of its commits does not cover: {} {}; its reviewer approves \
                 naming the tip",
                commits(during.len()),
                sys::local_minute(from),
                short(&first.sha),
                first.subject
            ));
        }
    }
    Ok(Covered::ByTime(branch.len()))
}

/// What a land records of its merge before it moves the default branch, in the lands' folder
/// ([`Record::path`]) and on the thread: the task, the tip it merged, its merge commit, and the
/// commit the check passed on (the merge, or the generators' commit after it). A tip the default
/// branch holds is a land to finish ([`Landing::finish`]) only by such a record of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub slug: String,
    pub tip: String,
    pub merge: String,
    pub checked: String,
}

impl Record {
    /// Where the record of `slug`'s land is kept in the lands' `folder`: `SLUG.merged`, its
    /// slashes as dashes.
    pub fn path(folder: &Path, slug: &str) -> PathBuf {
        folder.join(format!("{}.merged", slug.replace('/', "-")))
    }

    /// The record of `slug`'s land kept in `folder`, if there is one of that task.
    pub fn read(folder: &Path, slug: &str) -> Option<Record> {
        let text = fs::read_to_string(Record::path(folder, slug)).ok()?;
        serde_json::from_str::<Record>(&text)
            .ok()
            .filter(|record| record.slug == slug)
    }

    /// Kept in `folder`, whole or not at all.
    pub fn write(&self, folder: &Path) -> Result<(), String> {
        let path = Record::path(folder, &self.slug);
        let staged = folder.join(format!(
            ".{}.{}",
            self.slug.replace('/', "-"),
            std::process::id()
        ));
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        fs::write(&staged, text).map_err(|e| at(&staged, &e))?;
        fs::rename(&staged, &path).map_err(|e| at(&path, &e))
    }

    /// The record of `slug`'s land in `folder` dropped: its land closed, or its merge never moved
    /// the default branch.
    pub fn drop_in(folder: &Path, slug: &str) {
        let _ = fs::remove_file(Record::path(folder, slug));
    }
}

/// Why a land stopped short.
#[derive(Debug)]
pub enum Stop {
    /// Refused, with the one line saying why.
    Refused(String),
    /// The board failed.
    Board(Error),
}

impl From<Error> for Stop {
    fn from(error: Error) -> Stop {
        match error {
            Error::Refused(why) => Stop::Refused(why),
            error => Stop::Board(error),
        }
    }
}

/// Land `slug` as `actor`, from where `env` runs: what it did, said on `out` as it goes, then the
/// line saying where it landed. `recorder` is the `tasks` running it, which records the review
/// command a task the land sends back to review starts ([`on_review::trigger`]).
pub fn land(
    store: &Store,
    env: &Env,
    slug: &str,
    actor: &str,
    recorder: Option<&Path>,
    out: &mut dyn Write,
) -> Result<String, Stop> {
    let common = crate::git::common_dir(&env.cwd).map_err(|e| Stop::Refused(e.to_string()))?;
    let main = trigger::main_checkout(&common)
        .ok_or_else(|| {
            Stop::Refused(format!(
                "{} has no main checkout to land on",
                common.display()
            ))
        })?
        .to_path_buf();
    let settings = Settings::of(&main).map_err(Stop::Refused)?;
    // A hang-up, an interrupt or a terminate signal from here on ends what the land runs, and the
    // land by it once it has cleaned up. Another land of this process holds the one scope there
    // is (a test's, at once): this one then takes none, and a signal is that land's.
    let interrupts = sys::Interrupts::listen().ok();
    let ended = |stop: Stop| {
        if let Some(signal) = interrupts.as_ref().and_then(sys::Interrupts::signal) {
            ENDED_BY.store(signal, Ordering::Relaxed);
        }
        stop
    };
    let _held = hold(&common, slug, settings.timeout, interrupts.as_ref(), out)
        .map_err(|line| ended(Stop::Refused(line)))?;
    let folder = trigger::folder(&common).join(FOLDER);
    fs::create_dir_all(&folder).map_err(|e| Stop::Refused(at(&folder, &e)))?;
    let tree = match &settings.worktrees {
        Some(worktrees) => {
            let parent = main.join(worktrees);
            fs::create_dir_all(&parent).map_err(|e| Stop::Refused(at(&parent, &e)))?;
            parent
                .canonicalize()
                .map_err(|e| Stop::Refused(at(&parent, &e)))?
                .join(FOLDER)
        }
        None => folder.join("worktree"),
    };
    let log = folder.join(format!("{}.log", slug.replace('/', "-")));
    File::create(&log).map_err(|e| Stop::Refused(at(&log, &e)))?;
    let landed = Landing {
        store,
        env,
        slug,
        actor,
        main,
        settings,
        folder,
        tree,
        log,
        interrupts: interrupts.as_ref(),
        recorder,
        out,
    }
    .land();
    landed.map_err(ended)
}

fn at(path: &Path, error: &std::io::Error) -> String {
    format!("{}: {error}", path.display())
}

/// Whether `slug` may go back to review, a move asked from where `env` runs (none: from no
/// checkout): a task that is done does only while the default branch of its repository does not
/// hold its branch's tip, which landed already, by a land or by hand, and which no fresh approve
/// takes back. The board judges the rest ([`Store::move_to`]); it cannot see the repository.
pub fn reopenable(store: &Store, env: Option<&Env>, slug: &str) -> Result<(), Error> {
    let task = store.get(slug)?;
    if task.status != "done" {
        return Ok(());
    }
    let unknown = || {
        Error::Refused(format!(
            "{slug} is done: it goes back to review only from a checkout of its repository, which \
             says whether its tip is on the default branch already"
        ))
    };
    let common = env
        .and_then(|env| crate::git::common_dir(&env.cwd).ok())
        .ok_or_else(unknown)?;
    let main = trigger::main_checkout(&common).ok_or_else(unknown)?;
    let default = Settings::of(main).map_err(Error::Refused)?.default_branch;
    let Some(tip) = rev(main, &format!("refs/heads/{slug}")) else {
        return Ok(());
    };
    if ancestor(main, &tip, &format!("refs/heads/{default}")) {
        return Err(Error::Refused(format!(
            "{slug}'s tip {} is on {default} already, landed or merged by hand: it does not go \
             back to review, and what it still needs goes in a new task",
            short(&tip)
        )));
    }
    Ok(())
}

/// Whether the land `env` asks for may run: only as `just land SLUG`, whose recipe sets
/// [`RECIPE`], so `just tasks`, which an agent runs unasked, never lands; and never in a project
/// whose own `land` recipe replaces the Symphony's ([`own_land`]), which lands it everywhere.
/// Otherwise the one line saying how this project lands.
pub fn admit(env: &Env) -> Result<(), String> {
    if env.var(RECIPE).is_none() {
        return Err(
            "`tasks land` runs as `just land SLUG`, which asks before it moves and pushes the \
             default branch"
                .into(),
        );
    }
    let common = crate::git::common_dir(&env.cwd).map_err(|e| e.to_string())?;
    // With no main checkout the land refuses, naming it.
    let Some(main) = trigger::main_checkout(&common) else {
        return Ok(());
    };
    match own_land(main, env)? {
        Some(justfile) => Err(format!(
            "{} lands with a `land` recipe of its own, which replaces the Symphony's everywhere: \
             run `just land SLUG` there",
            justfile.display()
        )),
        None => Ok(()),
    }
}

/// The project's justfile at `root`, by any name `just` reads one by: `justfile` in any case, or
/// `.justfile`.
fn justfile(root: &Path) -> Option<PathBuf> {
    let mut names: Vec<String> = fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| matches!(name.to_lowercase().as_str(), "justfile" | ".justfile"))
        .collect();
    names.sort();
    names.first().map(|name| root.join(name))
}

/// The justfile of the main checkout `main` when the `land` it runs is a recipe of the project's
/// own: not the one `symphony.just` beside it holds, as `just` shows each (`just --show land`), so
/// however the project's justfile imports and orders them, the recipe `just land` runs is the one
/// judged. None with no justfile, or none whose recipes hold a `land`.
pub fn own_land(main: &Path, env: &Env) -> Result<Option<PathBuf>, String> {
    let Some(justfile) = justfile(main) else {
        return Ok(None);
    };
    let show = |file: &Path| -> Result<Option<String>, String> {
        let mut command = sys::command_with("just", &env.vars);
        command
            .arg("--justfile")
            .arg(file)
            .arg("--working-directory")
            .arg(main)
            .args(["--show", "land"])
            .current_dir(main);
        let done = sys::output(&mut command).map_err(|e| {
            format!(
                "cannot tell whether {} has a `land` of its own: just: {e}",
                justfile.display()
            )
        })?;
        if done.status.success() {
            return Ok(Some(String::from_utf8_lossy(&done.stdout).into_owned()));
        }
        let said = String::from_utf8_lossy(&done.stderr);
        if said.contains("does not contain recipe") {
            return Ok(None);
        }
        Err(format!(
            "cannot tell whether {} has a `land` of its own: just --show land: {}",
            justfile.display(),
            said.lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or("failed")
        ))
    };
    let Some(runs) = show(&justfile)? else {
        return Ok(None);
    };
    let recipes = main.join("symphony.just");
    let default = if recipes.is_file() {
        show(&recipes)?
    } else {
        None
    };
    Ok((default.as_deref() != Some(runs.as_str())).then_some(justfile))
}

/// The lock in `common` for the land of `slug`: taken at once, or once the land holding it ends,
/// within `patience`; past it, or once `interrupts` took a signal, the line naming the land that
/// holds it.
fn hold(
    common: &Path,
    slug: &str,
    patience: Duration,
    interrupts: Option<&sys::Interrupts>,
    out: &mut dyn Write,
) -> Result<sys::FileLock, String> {
    let folder = trigger::folder(common);
    fs::create_dir_all(&folder).map_err(|e| at(&folder, &e))?;
    let path = folder.join(LOCK);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| at(&path, &e))?;
    let holder = |file: &mut File| {
        let mut holder = String::new();
        let _ = file.seek(SeekFrom::Start(0));
        let _ = file.read_to_string(&mut holder);
        match holder.trim() {
            "" => "another land".to_string(),
            holder => holder.to_string(),
        }
    };
    let began = Instant::now();
    let mut said = false;
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) if began.elapsed() >= patience => {
                return Err(format!(
                    "{} has held {} for {} s, this land's timeout: run the land of {slug} again \
                     once it ends",
                    holder(&mut file),
                    path.display(),
                    patience.as_secs()
                ));
            }
            Err(TryLockError::WouldBlock) => {
                if let Some(signal) = interrupts.and_then(sys::Interrupts::signal) {
                    return Err(format!(
                        "interrupted by {} while waiting for {}: nothing landed",
                        signal_name(signal),
                        holder(&mut file)
                    ));
                }
                if !said {
                    let _ = writeln!(
                        out,
                        "land {slug}: waiting for {} to finish",
                        holder(&mut file)
                    );
                    let _ = out.flush();
                    said = true;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(TryLockError::Error(e)) => return Err(at(&path, &e)),
        }
    }
    let held = sys::FileLock(file);
    let mut file = &held.0;
    let _ = file.set_len(0);
    let _ = file.seek(SeekFrom::Start(0));
    let _ = write!(file, "the land of {slug} (pid {})", std::process::id());
    Ok(held)
}

/// `git ARGS` in `dir`: its output, trimmed, or the line saying why it failed.
fn run_git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let done = git(dir, args).ok_or_else(|| format!("git cannot run in {}", dir.display()))?;
    if done.status.success() {
        return Ok(String::from_utf8_lossy(&done.stdout).trim().to_string());
    }
    let said = String::from_utf8_lossy(&done.stderr);
    let said = said
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("failed");
    Err(format!("git {}: {}", args.join(" "), said.trim()))
}

/// The paths with a change in the checkout `dir`, untracked ones each named.
fn changed(dir: &Path) -> Result<Vec<String>, String> {
    let done = git(dir, &["status", "--porcelain", "-z", "-uall"])
        .filter(|done| done.status.success())
        .ok_or_else(|| format!("git status failed in {}", dir.display()))?;
    let mut records = done.stdout.split(|b| *b == 0).filter(|s| !s.is_empty());
    let mut names = Vec::new();
    while let Some(record) = records.next() {
        if let Some(name) = record.get(3..) {
            names.push(String::from_utf8_lossy(name).into_owned());
        }
        if matches!(record.first(), Some(b'R' | b'C')) {
            records.next();
        }
    }
    Ok(names)
}

/// `names` as words, the last two joined by "and".
fn and(names: &[&str]) -> String {
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        Some((last, _)) => (*last).to_string(),
        None => String::new(),
    }
}

/// What one merge and check ended in.
enum Attempt {
    /// The default branch moved to `top`, whose first commit made here is `merge`; `regenerated`
    /// names the generators' commit and what they regenerated, when they changed anything.
    Moved {
        merge: String,
        top: String,
        regenerated: Option<(String, String)>,
    },
    /// The default branch moved under it, to this commit: nothing changed.
    Raced(String),
}

/// Why a land refused, and where it is said.
struct Refusal {
    line: String,
    /// It is about the branch, so it goes on the thread too.
    thread: bool,
    /// Whom it is said to there: the executor, whose branch must change (a conflict, a red); the
    /// reviewer, whose approve does not cover the tip; the orchestrator, for a red after the
    /// default branch moved or an approve that does not count.
    to: Option<&'static str>,
    /// What the one it is said to does next, after the line.
    then: Option<String>,
    /// The approve does not count or does not cover the tip, so the land sends the task back to
    /// review for a fresh one ([`Store::move_to`]).
    reopen: bool,
}

impl Refusal {
    fn here(line: String) -> Refusal {
        Refusal {
            line,
            thread: false,
            to: None,
            then: None,
            reopen: false,
        }
    }

    fn thread(line: String) -> Refusal {
        Refusal {
            thread: true,
            ..Refusal::here(line)
        }
    }

    /// Said on the thread to `to`, which does `then`.
    fn to(line: String, to: &'static str, then: String) -> Refusal {
        Refusal {
            to: Some(to),
            then: Some(then),
            ..Refusal::thread(line)
        }
    }

    /// An approve that does not do, said to `to`, the task sent back to review for a fresh one.
    fn reopening(line: String, to: &'static str, then: String) -> Refusal {
        Refusal {
            reopen: true,
            ..Refusal::to(line, to, then)
        }
    }
}

impl From<String> for Refusal {
    fn from(line: String) -> Refusal {
        Refusal::here(line)
    }
}

impl From<&str> for Refusal {
    fn from(line: &str) -> Refusal {
        Refusal::here(line.into())
    }
}

/// What a merge brings in: the task's tip, or what `origin` gained.
enum Merging<'a> {
    Tip(&'a str),
    Origin(&'a str),
}

/// What the push came to.
enum Pushed {
    Done,
    Off,
    NoRemote,
    /// Why, and the command that pushes by hand.
    Failed(String, String),
}

/// What a generator or the check came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ran {
    Passed,
    Failed,
    /// It ran past the land's timeout, and was ended.
    Late,
    /// The land took this signal while it ran, and ended it.
    Interrupted(i32),
}

/// Why a watched child was ended, if it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ended {
    /// It ended by itself.
    No,
    /// It outlasted its limit.
    Late,
    /// The land took this signal.
    Interrupted(i32),
}

/// `wait` for a child started in a process group of its own, `group` (its pid), while a watchdog
/// ends the group should it outlast `limit` or `interrupts` take a signal: asked to end, then made
/// to [`GRACE`] on. Once `wait` returns, what is left of the group (a process the child started
/// in the background) is ended the same way, so nothing it started outlives it. What `wait`
/// returned, and why the watchdog ended it.
fn watched<T>(
    group: u32,
    limit: Duration,
    interrupts: Option<&sys::Interrupts>,
    wait: impl FnOnce() -> T,
) -> (T, Ended) {
    use std::sync::mpsc::{RecvTimeoutError, channel};
    let (waited, ended) = std::thread::scope(|scope| {
        let (done, finished) = channel::<()>();
        let watchdog = scope.spawn(move || {
            let deadline = Instant::now() + limit;
            let ended = loop {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break Ended::Late;
                }
                if finished.recv_timeout(left.min(POLL)) != Err(RecvTimeoutError::Timeout) {
                    return Ended::No;
                }
                if let Some(signal) = interrupts.and_then(sys::Interrupts::signal) {
                    break Ended::Interrupted(signal);
                }
            };
            sys::end_group(group, false);
            if finished.recv_timeout(GRACE) == Err(RecvTimeoutError::Timeout) {
                sys::end_group(group, true);
            }
            ended
        });
        let waited = wait();
        let _ = done.send(());
        (waited, watchdog.join().unwrap_or(Ended::No))
    });
    end_leftovers(group);
    (waited, ended)
}

/// End what is left of the process group `group` once its first process has ended: asked, then made
/// to [`GRACE`] on.
fn end_leftovers(group: u32) {
    if !sys::group_alive(group) {
        return;
    }
    sys::end_group(group, false);
    let asked = Instant::now();
    while sys::group_alive(group) && asked.elapsed() < GRACE {
        std::thread::sleep(POLL);
    }
    if sys::group_alive(group) {
        sys::end_group(group, true);
    }
}

/// The pid a land's worktree lock reason names (`symphony land: SLUG (pid N)`).
pub(crate) fn locked_by(reason: &str) -> Option<i64> {
    let (_, pid) = reason.rsplit_once("(pid ")?;
    pid.strip_suffix(')')?.parse().ok()
}

/// The land's worktree, removed when it is dropped.
struct Tree {
    main: PathBuf,
    path: PathBuf,
}

impl Drop for Tree {
    fn drop(&mut self) {
        clear(&self.main, &self.path);
    }
}

/// Remove the land's worktree at `path` of the checkout `main`, if git lists it.
fn clear(main: &Path, path: &Path) {
    let shown = path.to_string_lossy();
    let _ = git(main, &["worktree", "unlock", &shown]);
    let _ = git(main, &["worktree", "remove", "--force", "--force", &shown]);
    let _ = git(main, &["worktree", "prune"]);
}

/// Whether `a` and `b` name one place: their folders resolved, their last names equal.
fn same(a: &Path, b: &Path) -> bool {
    let resolved = |path: &Path| {
        let folder = path.parent()?.canonicalize().ok()?;
        Some(folder.join(path.file_name()?))
    };
    a == b || resolved(a).is_some_and(|a| Some(a) == resolved(b))
}

struct Landing<'a> {
    store: &'a Store,
    env: &'a Env,
    slug: &'a str,
    actor: &'a str,
    main: PathBuf,
    settings: Settings,
    /// The lands' folder: their logs, [`RUNNING`] and each land's [`Record`].
    folder: PathBuf,
    tree: PathBuf,
    log: PathBuf,
    /// The signals the land takes, when it holds the process's one scope.
    interrupts: Option<&'a sys::Interrupts>,
    /// The `tasks` running the land, which records a review command it starts.
    recorder: Option<&'a Path>,
    out: &'a mut dyn Write,
}

impl Landing<'_> {
    fn say(&mut self, line: &str) {
        let _ = writeln!(self.out, "{line}");
        let _ = self.out.flush();
    }

    /// The signal the land took, if it took one.
    fn interrupted(&self) -> Option<i32> {
        self.interrupts.and_then(sys::Interrupts::signal)
    }

    /// The line a land interrupted by `signal` stops with: before the default branch moved,
    /// nothing landed; after it (`merged`, by the merge it recorded), a run again finishes it.
    fn halted(&self, signal: i32, merged: Option<&str>) -> String {
        let (slug, default) = (self.slug, &self.settings.default_branch);
        match merged {
            None => format!(
                "interrupted by {}: nothing landed, {default} where it was",
                signal_name(signal)
            ),
            Some(merge) => format!(
                "interrupted by {} after {default} moved to the merge of {slug}, {}: run `just \
                 land {slug}` again to finish its push, its landed line and its sweep",
                signal_name(signal),
                short(merge)
            ),
        }
    }

    /// Refuse with [`Landing::halted`] once the land took a signal.
    fn go_on(&self, merged: Option<&str>) -> Result<(), String> {
        match self.interrupted() {
            Some(signal) => Err(self.halted(signal, merged)),
            None => Ok(()),
        }
    }

    /// The group `group` runs `command` for this land, named in [`RUNNING`] until it is gone, so a
    /// land killed outright leaves the next one its name.
    fn running(&self, group: u32, command: &str) {
        let _ = fs::write(self.folder.join(RUNNING), format!("{group}\t{command}\n"));
    }

    /// The group named in [`RUNNING`] is gone.
    fn ran(&self) {
        let _ = fs::remove_file(self.folder.join(RUNNING));
    }

    /// The group a land killed outright left named in [`RUNNING`], while any process of it lives: the
    /// line refusing to land beside it. One that ended is forgotten.
    fn leftover_group(&self) -> Result<(), String> {
        let path = self.folder.join(RUNNING);
        let Ok(text) = fs::read_to_string(&path) else {
            return Ok(());
        };
        let (group, command) = text
            .trim_end()
            .split_once('\t')
            .unwrap_or((text.trim(), ""));
        match group.parse::<u32>() {
            Ok(group) if sys::group_alive(group) => Err(format!(
                "the process group {group} an earlier land started (`{command}`) outlived it and \
                 still runs: end it (`kill -TERM -{group}`), then run the land again"
            )),
            _ => {
                self.ran();
                Ok(())
            }
        }
    }

    fn branch(&self) -> String {
        format!("refs/heads/{}", self.settings.default_branch)
    }

    /// Say `refusal` where it goes, the task sent back to review first when its approve does not
    /// do; the stop it makes.
    fn refuse(&self, refusal: Refusal) -> Stop {
        if !refusal.thread {
            return Stop::Refused(refusal.line);
        }
        let slug = self.slug;
        let mut said = format!("land refused: {}", refusal.line);
        if refusal.reopen {
            match self.store.move_to(slug, "review", self.actor) {
                Ok(task) => {
                    said.push_str(&format!("; {slug} is back in review"));
                    // As a move to review on the command line does: the project's `on_review`.
                    if on_review::delivery_task(task.parent.as_deref(), task.instruction) {
                        match on_review::trigger(self.env, &self.main, slug, self.recorder) {
                            Started::Command(args) => {
                                said.push_str(&format!(", its verify started ({})", args.join(" ")))
                            }
                            Started::Refused(why) => {
                                said.push_str(&format!(", its verify did not start ({why})"));
                            }
                            Started::None => {}
                        }
                    }
                }
                Err(error) => said.push_str(&format!(
                    "; it stays done, the land could not send it back to review ({error}): \
                     `just tasks move {slug} review` does"
                )),
            }
        }
        if let Some(then) = &refusal.then {
            said.push_str(&format!(". {then}"));
        }
        self.post(&said, refusal.to);
        Stop::Refused(refusal.line)
    }

    fn post(&self, text: &str, to: Option<&str>) {
        let _ = self.store.say(self.slug, self.actor, text, to);
    }

    fn land(mut self) -> Result<String, Stop> {
        let slug = self.slug;
        self.leftover_group().map_err(Stop::Refused)?;
        let task = self.store.get(slug)?;
        let thread = self.store.thread(slug)?;
        let approval =
            approval(&task, &thread).map_err(|line| self.refuse(Refusal::thread(line)))?;
        let held = self.store.ever_held(slug)?;
        independent(&task, &approval, &held).map_err(|line| {
            self.refuse(Refusal::reopening(
                line,
                "@orchestrator",
                "Staff a fresh reviewer, which approves naming the tip it read".into(),
            ))
        })?;
        let tip = rev(&self.main, &format!("refs/heads/{slug}"))
            .ok_or_else(|| self.refuse(Refusal::thread(format!("no branch {slug} to land"))))?;
        self.clean().map_err(|line| self.refuse(line.into()))?;
        let mut base = self.base().map_err(|line| self.refuse(line.into()))?;
        let default = self.settings.default_branch.clone();
        if ancestor(&self.main, &tip, &base) {
            return self.finish(&tip, &base, &approval);
        }
        let covered = covered(&self.main, slug, &base, &tip, &approval).map_err(|line| {
            self.refuse(Refusal::reopening(
                line,
                "@reviewer",
                format!(
                    "Read what the approve does not cover, then approve the tip naming it: \
                     `just tasks verdict {slug} approve \"{tip}: …\"`"
                ),
            ))
        })?;
        self.say(&format!(
            "land {slug}: approved by {} at {}, {}",
            approval.by,
            sys::local_minute(approval.at),
            match covered {
                Covered::Named => format!("naming its tip {}", short(&tip)),
                Covered::ByTime(_) => format!("before every commit to its tip {}", short(&tip)),
            }
        ));
        let mut landed = None;
        for attempt in 0..TRIES {
            self.go_on(None).map_err(Stop::Refused)?;
            match self
                .integrate(&base, &Merging::Tip(&tip), attempt > 0)
                .map_err(|refusal| self.refuse(refusal))?
            {
                Attempt::Moved {
                    merge,
                    top,
                    regenerated,
                } => {
                    landed = Some((merge, top, regenerated));
                    break;
                }
                Attempt::Raced(now) => {
                    // The last try's move is told by the refusal below.
                    if attempt + 1 < TRIES {
                        self.say(&format!(
                            "land {slug}: {default} moved to {}; merging and checking again",
                            short(&now)
                        ));
                    }
                    base = now;
                }
            }
        }
        let Some((merge, top, regenerated)) = landed else {
            let line = format!(
                "{default} moved {TRIES} times while {slug} was checked; nothing landed: run the \
                 land again"
            );
            return Err(self.refuse(Refusal::here(line)));
        };
        self.close(&merge, &top, regenerated.as_ref(), &covered, false)
    }

    /// The land of a tip the default branch holds already: only one this command started and
    /// interrupted after it moved the default branch (killed while it pushed, say), by the
    /// [`Record`] it kept of that merge, whose tip is the branch's still, whose merge brought that
    /// tip, and whose checked commit the default branch holds. Then the approve still stands, so
    /// no landing closed the task, and what is left is done once the approve covers the tip: the
    /// push of the checked commit, never of a newer tip no check passed on, the landed line and the
    /// sweep. A tip merged by hand, or by a land that did not record it, is refused: nothing is
    /// pushed, closed or said on the thread.
    fn finish(&mut self, tip: &str, base: &str, approval: &Approval) -> Result<String, Stop> {
        let (slug, default) = (self.slug, self.settings.default_branch.clone());
        let record = Record::read(&self.folder, slug).filter(|record| {
            record.tip == tip
                && rev(&self.main, &format!("{}^2", record.merge)).as_deref() == Some(tip)
                && ancestor(&self.main, &record.merge, &record.checked)
                && ancestor(&self.main, &record.checked, base)
        });
        let Some(record) = record else {
            return Err(self.refuse(Refusal::here(format!(
                "{slug}'s tip {} is on {default} already, merged outside land: nothing to finish",
                short(tip)
            ))));
        };
        let before =
            rev(&self.main, &format!("{}^1", record.merge)).unwrap_or_else(|| tip.to_string());
        let covered = covered(&self.main, slug, &before, tip, approval).map_err(|line| {
            self.refuse(Refusal::to(
                format!(
                    "{slug}'s tip {} is on {default} already, by {}, but {line}",
                    short(tip),
                    short(&record.merge)
                ),
                "@orchestrator",
                "Nothing is pushed or closed: what landed is not what was approved".into(),
            ))
        })?;
        self.say(&format!(
            "land {slug}: its tip {} is on {default} already, by this land's merge {}, checked at \
             {}: finishing the land (the push, the landed line, the sweep)",
            short(tip),
            short(&record.merge),
            short(&record.checked)
        ));
        self.close(&record.merge, &record.checked, None, &covered, true)
    }

    /// The landing's end once `merge` is on the default branch, at `top` here: the push, the
    /// landed line closing the task, and the sweep; the line saying where it landed.
    fn close(
        &mut self,
        merge: &str,
        top: &str,
        regenerated: Option<&(String, String)>,
        covered: &Covered,
        finishing: bool,
    ) -> Result<String, Stop> {
        let (slug, default) = (self.slug, self.settings.default_branch.clone());
        let mut landed = format!(
            "landed {slug} on {default} at {}",
            &merge[..merge.len().min(12)]
        );
        if finishing {
            landed.push_str(", finishing a land interrupted after its merge");
        }
        if let Some((commit, names)) = regenerated {
            landed.push_str(&format!(", {names} regenerated in {}", short(commit)));
        }
        // A signal from here on stops the land with its record kept, which a run again finishes.
        self.go_on(Some(merge)).map_err(Stop::Refused)?;
        let pushed = match self.push(top) {
            Pushed::Done => format!("; pushed to {REMOTE}"),
            Pushed::Off => "; not pushed: [land] push is off".into(),
            Pushed::NoRemote => format!("; not pushed: no {REMOTE} remote"),
            Pushed::Failed(why, retry) => {
                format!("; NOT PUSHED, landed here only: {why}. Push it with: {retry}")
            }
        };
        self.go_on(Some(merge)).map_err(Stop::Refused)?;
        let line = format!("{landed}{pushed}");
        // The thread also says how the approval was judged, before the push, whose retry command
        // ends the line.
        let said = match *covered {
            Covered::ByTime(n) => format!(
                "{landed}; judged by time, the approval naming no commit of the branch: its {} {} \
                 older than the approval{pushed}",
                commits(n),
                if n == 1 { "is" } else { "are" }
            ),
            Covered::Named => line.clone(),
        };
        // The approval on the board is the gate here, not who runs the land: any session may. A
        // task sent back to review since it was read stays there: what it has now did not land.
        if let Err(error) = self.store.landed(slug, self.actor, &said, true, &["done"]) {
            return Err(Stop::Refused(format!(
                "{line}, but the board did not close {slug}: {error}"
            )));
        }
        // Closed: nothing is left to finish.
        Record::drop_in(&self.folder, slug);
        let at_main = Env {
            vars: self.env.vars.clone(),
            cwd: self.main.clone(),
        };
        let request = sweep::Request {
            tasks: vec![slug],
            ..sweep::Request::default()
        };
        match sweep::command(self.store, &at_main, &request) {
            Ok(lines) => {
                // What it removed, cleared and deleted, and its summary with the bytes freed.
                let said = ["removed ", "cleared ", "deleted branch ", "worktree sweep:"];
                for swept in lines
                    .iter()
                    .filter(|l| said.iter().any(|start| l.starts_with(start)))
                {
                    self.say(swept);
                }
            }
            Err(why) => self.say(&format!("⚠ worktree sweep: {why}")),
        }
        Ok(line)
    }

    /// The default branch's tip as the main checkout has it.
    fn base(&self) -> Result<String, String> {
        rev(&self.main, &self.branch()).ok_or_else(|| {
            format!(
                "no branch {} in {}",
                self.settings.default_branch,
                self.main.display()
            )
        })
    }

    /// The main checkout has no staged or tracked edit and has the default branch checked out.
    fn clean(&self) -> Result<(), String> {
        let shown = self.main.display();
        let head = run_git(&self.main, &["symbolic-ref", "-q", "HEAD"]).unwrap_or_default();
        if head != self.branch() {
            return Err(format!(
                "the main checkout {shown} has {} checked out, not {}",
                head.strip_prefix("refs/heads/")
                    .unwrap_or("a detached head"),
                self.settings.default_branch
            ));
        }
        let status = run_git(
            &self.main,
            &["status", "--porcelain", "--untracked-files=no"],
        )?;
        if !status.is_empty() {
            return Err(format!("the main checkout {shown} {DIRTY_MAIN}"));
        }
        Ok(())
    }

    /// A worktree of its own at `base`, locked so the sweep keeps it while the land runs, in place
    /// of one a land left behind once the land its lock names has ended and no process works in it
    /// (`lsof`, as the sweep's guard asks; one that cannot say keeps it); one in its place that is
    /// not a land's is never touched.
    fn fresh(&self, base: &str) -> Result<Tree, String> {
        let shown = self.tree.to_string_lossy().into_owned();
        let listed = worktrees(&self.main)?;
        match listed.iter().find(|entry| same(&entry.path, &self.tree)) {
            Some(entry)
                if entry
                    .locked
                    .as_deref()
                    .is_some_and(|reason| reason.starts_with(REASON)) =>
            {
                let reason = entry.locked.as_deref().unwrap_or_default();
                if let Some(pid) = locked_by(reason)
                    .filter(|pid| *pid != i64::from(std::process::id()) && sys::pid_alive(*pid))
                {
                    return Err(format!(
                        "{shown} is the worktree of a land still running (pid {pid}, {reason}): \
                         wait for it to end"
                    ));
                }
                // Gone from the disk, nothing can work in it.
                if self.tree.exists() {
                    match process_cwd::occupied_with(sys::command("lsof"), &self.tree) {
                        Some(false) => {}
                        Some(true) => {
                            return Err(format!(
                                "a process still works in {shown}, the worktree an earlier land \
                                 left: end it, then run the land again"
                            ));
                        }
                        None => {
                            return Err(format!(
                                "lsof cannot say whether a process works in {shown}, the worktree \
                                 an earlier land left: once nothing does, `git worktree remove \
                                 --force --force {shown}`, then run the land again"
                            ));
                        }
                    }
                }
                clear(&self.main, &self.tree);
            }
            Some(_) => {
                return Err(format!(
                    "{shown} is another worktree, where a land makes its own: move it, or name \
                     another [agents] worktrees folder"
                ));
            }
            None if self.tree.exists() => {
                return Err(format!(
                    "{shown} is in the way of the land's worktree: move it, or name another \
                     [agents] worktrees folder"
                ));
            }
            None => {}
        }
        let reason = format!("{REASON}: {} (pid {})", self.slug, std::process::id());
        run_git(
            &self.main,
            &[
                "worktree", "add", "--quiet", "--detach", "--lock", "--reason", &reason, &shown,
                base,
            ],
        )?;
        Ok(Tree {
            main: self.main.clone(),
            path: self.tree.clone(),
        })
    }

    /// Run `command` in `dir`, as this land's actor for this task, its output in the log, within
    /// the land's timeout: what it came to, or why it could not run.
    fn logged(&self, command: &[String], dir: &Path) -> Result<Ran, String> {
        let (program, args) = command.split_first().ok_or("an empty command")?;
        let mut log = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.log)
            .map_err(|e| at(&self.log, &e))?;
        let _ = writeln!(log, "$ {} (in {})", command.join(" "), dir.display());
        let path = Path::new(program);
        let program = if path.is_absolute() || path.components().count() < 2 {
            path.to_path_buf()
        } else {
            dir.join(path)
        };
        let mut child = sys::command_with(&program, &self.env.vars);
        for name in GIT_VARS.iter().chain([&RECIPE]) {
            child.env_remove(name);
        }
        // What the land runs is the task's, recorded for it and the session landing it, in a group
        // of its own, which its timeout ends whole.
        child
            .args(args)
            .current_dir(dir)
            .env("TASKS_SESSION", self.actor)
            .env("TASKS_TASK", self.slug)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| at(&self.log, &e))?)
            .stderr(log.try_clone().map_err(|e| at(&self.log, &e))?);
        if let Some(signal) = self.interrupted() {
            return Ok(Ran::Interrupted(signal));
        }
        let mut child = child
            .spawn()
            .map_err(|e| format!("{}: {e}", command.join(" ")))?;
        let group = child.id();
        self.running(group, &command.join(" "));
        let limit = self.settings.timeout;
        let (status, ended) = watched(group, limit, self.interrupts, || child.wait());
        self.ran();
        let status = status.map_err(|e| format!("{}: {e}", command.join(" ")))?;
        let _ = writeln!(log, "exit {}", status.code().unwrap_or(-1));
        match ended {
            Ended::Late => {
                let _ = writeln!(log, "ended: it ran past the land's {} s", limit.as_secs());
                return Ok(Ran::Late);
            }
            Ended::Interrupted(signal) => {
                let _ = writeln!(log, "ended: the land took {}", signal_name(signal));
                return Ok(Ran::Interrupted(signal));
            }
            Ended::No => {}
        }
        Ok(if status.success() {
            Ran::Passed
        } else {
            Ran::Failed
        })
    }

    /// What `command` came to that is said of a refusal: `failed`, or ran past the timeout.
    fn failed(&self, ran: Ran) -> String {
        match ran {
            Ran::Late => format!(
                "ran past the land's timeout ([land] timeout_seconds, {} s)",
                self.settings.timeout.as_secs()
            ),
            Ran::Interrupted(signal) => format!("was ended by {}", signal_name(signal)),
            Ran::Passed | Ran::Failed => "failed".into(),
        }
    }

    /// Merge `merging` into `base` in the land's worktree, regenerate and check, then move the
    /// default branch there when it is still at `base`. `again` is a run after it moved.
    fn integrate(
        &mut self,
        base: &str,
        merging: &Merging,
        again: bool,
    ) -> Result<Attempt, Refusal> {
        let slug = self.slug;
        let default = self.settings.default_branch.clone();
        let (what, message) = match merging {
            Merging::Tip(tip) => (*tip, format!("Merge branch '{slug}'")),
            Merging::Origin(gained) => (
                *gained,
                format!("Merge remote-tracking branch '{REMOTE}/{default}'"),
            ),
        };
        let tree = self.fresh(base)?;
        let path = tree.path.clone();
        self.say(&format!(
            "land {slug}: merging {} into {default} at {} in {}",
            match merging {
                Merging::Tip(tip) => short(tip).to_string(),
                Merging::Origin(_) => format!("what {REMOTE}/{default} gained"),
            },
            short(base),
            path.display()
        ));
        if git(
            &path,
            &["merge", "--no-ff", "--no-edit", "-m", &message, what],
        )
        .is_none_or(|done| !done.status.success())
        {
            let conflicted =
                run_git(&path, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
            let _ = git(&path, &["merge", "--abort"]);
            let files: Vec<&str> = conflicted.lines().collect();
            let line = if files.is_empty() {
                format!(
                    "{} could not be merged into {default} at {}",
                    short(what),
                    short(base)
                )
            } else {
                let into = match merging {
                    Merging::Tip(_) => slug.to_string(),
                    Merging::Origin(_) => format!("{REMOTE}/{default}"),
                };
                format!(
                    "{into} conflicts with {default} at {}: {}",
                    short(base),
                    files.join(", ")
                )
            };
            return Err(match merging {
                Merging::Tip(_) => Refusal::to(line, "@executor", self.fix_on_branch()),
                Merging::Origin(_) => Refusal::thread(line),
            });
        }
        let merge = rev(&path, "HEAD").ok_or("the merge left no HEAD")?;
        let regenerated = self.generate(&path)?;
        if !self.settings.check.is_empty() {
            let check = self.settings.check.join(" ");
            self.say(&format!("land {slug}: checking ({check})"));
            let ran = self.logged(&self.settings.check.clone(), &path)?;
            if let Ran::Interrupted(signal) = ran {
                return Err(Refusal::here(self.halted(signal, None)));
            }
            if ran != Ran::Passed {
                let line = format!(
                    "the check `{check}` {} on {slug} merged into {default} at {}; its output is \
                     in {}",
                    if ran == Ran::Late {
                        self.failed(ran)
                    } else {
                        "is red".into()
                    },
                    short(base),
                    self.log.display()
                );
                // Red once the default branch moved, it is the orchestrator's to look at; red on
                // the merge of the tip as approved, the executor's to fix on its branch.
                return Err(match merging {
                    _ if again => Refusal {
                        to: Some("@orchestrator"),
                        ..Refusal::thread(line)
                    },
                    Merging::Tip(_) => Refusal::to(line, "@executor", self.fix_on_branch()),
                    Merging::Origin(_) => Refusal::thread(line),
                });
            }
        }
        let top = rev(&path, "HEAD").ok_or("the check left no HEAD")?;
        let checked = rev(&path, "HEAD^{tree}").ok_or("the check left no tree")?;
        drop(tree);
        self.clean()?;
        self.go_on(None)?;
        if let Merging::Tip(tip) = merging {
            // Kept before the default branch moves, so a land stopped after it is one to finish,
            // and only this one.
            let record = Record {
                slug: slug.into(),
                tip: (*tip).into(),
                merge: merge.clone(),
                checked: top.clone(),
            };
            record.write(&self.folder)?;
            self.post(
                &format!(
                    "land {slug}: merged {tip} as {merge}, checked at {top}; moving {default} there"
                ),
                None,
            );
        }
        let done = git(&self.main, &["merge", "--ff-only", "--quiet", &top]);
        if done.as_ref().is_none_or(|done| !done.status.success()) {
            if let Merging::Tip(_) = merging {
                Record::drop_in(&self.folder, slug);
            }
            let now = self.base()?;
            if now != base {
                return Ok(Attempt::Raced(now));
            }
            let said = done
                .map(|done| String::from_utf8_lossy(&done.stderr).trim().to_string())
                .unwrap_or_default();
            return Err(format!(
                "{default} could not move to {} in the main checkout: {}",
                short(&top),
                said.lines().next().unwrap_or("git merge --ff-only failed")
            )
            .into());
        }
        let moved = rev(&self.main, &format!("{}^{{tree}}", self.branch()));
        if moved.as_deref() != Some(checked.as_str()) {
            return Err(format!(
                "{default} moved to {} but its tree is not the one the check passed on",
                short(&top)
            )
            .into());
        }
        if let Merging::Origin(_) = merging {
            // What origin gained, merged and checked, is what a land stopped now pushes.
            if let Some(mut record) = Record::read(&self.folder, slug) {
                record.checked.clone_from(&top);
                let _ = record.write(&self.folder);
            }
        }
        Ok(Attempt::Moved {
            merge,
            top,
            regenerated,
        })
    }

    /// Run the project's generators in `path`, its merge checked out; commit what they changed,
    /// inside their outputs alone: that commit and the names of what changed.
    fn generate(&mut self, path: &Path) -> Result<Option<(String, String)>, Refusal> {
        let generators = self.settings.generators.clone();
        if generators.is_empty() {
            return Ok(None);
        }
        let slug = self.slug;
        let mut wrote = false;
        for generator in &generators {
            if generator.when_changed && !wrote {
                continue;
            }
            self.say(&format!(
                "land {slug}: regenerating {} ({})",
                generator.name,
                generator.command.join(" ")
            ));
            let ran = self.logged(&generator.command, path)?;
            if let Ran::Interrupted(signal) = ran {
                return Err(Refusal::here(self.halted(signal, None)));
            }
            if ran != Ran::Passed {
                return Err(Refusal::thread(format!(
                    "the generator {} {} on {slug} merged into {}; its output is in {}",
                    generator.name,
                    self.failed(ran),
                    self.settings.default_branch,
                    self.log.display()
                )));
            }
            wrote |= changed(path)?
                .iter()
                .any(|name| name.starts_with(&generator.outputs));
        }
        let made = changed(path)?;
        if made.is_empty() {
            return Ok(None);
        }
        if let Some(stray) = made
            .iter()
            .find(|name| !generators.iter().any(|g| name.starts_with(&g.outputs)))
        {
            return Err(Refusal::thread(format!(
                "the generators changed {stray}, outside their outputs, on {slug} merged into {}",
                self.settings.default_branch
            )));
        }
        let names: Vec<&str> = generators
            .iter()
            .filter(|g| made.iter().any(|name| name.starts_with(&g.outputs)))
            .map(|g| g.name.as_str())
            .collect();
        let names = and(&names);
        let mut add = vec!["add", "-A", "--"];
        add.extend(generators.iter().map(|g| g.outputs.as_str()));
        run_git(path, &add)?;
        let message = format!("Regenerate {names} after {slug}");
        run_git(path, &["commit", "--quiet", "--no-verify", "-m", &message])?;
        let commit = rev(path, "HEAD").ok_or("the commit left no HEAD")?;
        Ok(Some((commit, names)))
    }

    /// Push the default branch, at `top` here (the commit a check passed on), to `origin`: what it
    /// gained there merged first and checked again; never a force, no other branch, no tag, and
    /// never a commit no check passed on. An `origin` that holds `top` already has it pushed.
    fn push(&mut self, top: &str) -> Pushed {
        if !self.settings.push {
            return Pushed::Off;
        }
        if !succeeds(&self.main, &["remote", "get-url", REMOTE]) {
            return Pushed::NoRemote;
        }
        let slug = self.slug;
        let default = self.settings.default_branch.clone();
        let tracking = format!("refs/remotes/{REMOTE}/{default}");
        let fetch = format!("+{}:{tracking}", self.branch());
        let by_hand = format!("git push {REMOTE} {default}");
        let merge_by_hand = format!(
            "git fetch {REMOTE} && git merge {REMOTE}/{default} (in the main checkout), then {by_hand}"
        );
        let mut top = top.to_string();
        for attempt in 0..TRIES {
            let fetched = self.remote(&["fetch", "--quiet", "--no-tags", REMOTE, &fetch]);
            let there = rev(&self.main, &tracking);
            if fetched.is_ok()
                && there
                    .as_ref()
                    .is_some_and(|at| ancestor(&self.main, &top, at))
            {
                return Pushed::Done;
            }
            let gained = match fetched {
                Ok(()) => there.filter(|at| !ancestor(&self.main, at, &top)),
                // An `origin` with no default branch yet gained nothing: the push makes it.
                Err(_) if self.lacks_branch() => None,
                Err(why) => return Pushed::Failed(why, by_hand),
            };
            if let Some(gained) = gained {
                self.say(&format!(
                    "land {slug}: {REMOTE}/{default} gained {}; merging and checking it",
                    short(&gained)
                ));
                match self.integrate(&top, &Merging::Origin(&gained), attempt > 0) {
                    Ok(Attempt::Moved { top: now, .. }) => top = now,
                    // Moved here by hand meanwhile: what it holds now no check passed on.
                    Ok(Attempt::Raced(now)) => {
                        return Pushed::Failed(
                            format!(
                                "{default} moved to {} here while the land merged what {REMOTE} \
                                 gained, and no check passed on it",
                                short(&now)
                            ),
                            merge_by_hand,
                        );
                    }
                    Err(refusal) => {
                        if refusal.thread {
                            let said = format!("land of {slug} not pushed: {}", refusal.line);
                            self.post(&said, Some("@orchestrator"));
                        }
                        return Pushed::Failed(refusal.line, merge_by_hand);
                    }
                }
            }
            // The commit checked here, by its name, never what the branch holds by now, so a commit
            // made on it by hand meanwhile is not pushed unchecked.
            let push = format!("{top}:{}", self.branch());
            let Err(why) = self.remote(&["push", "--quiet", "--no-follow-tags", REMOTE, &push])
            else {
                return Pushed::Done;
            };
            // Refused because origin moved since the fetch: fetch, merge and check again. Any other
            // refusal finds origin where it was, and is the answer.
            let moved = self
                .remote(&["fetch", "--quiet", "--no-tags", REMOTE, &fetch])
                .is_ok()
                && rev(&self.main, &tracking).is_some_and(|at| !ancestor(&self.main, &at, &top));
            if !moved {
                return Pushed::Failed(why, by_hand);
            }
            if attempt + 1 < TRIES {
                self.say(&format!(
                    "land {slug}: {REMOTE}/{default} moved while pushing; fetching again"
                ));
            }
        }
        Pushed::Failed(
            format!("{REMOTE}/{default} kept moving while the land pushed"),
            merge_by_hand,
        )
    }

    /// `git ARGS` with the remote, in the main checkout, by the caller's environment (its `PATH`,
    /// its keys) less git's repository variables, never asking for a password, in a process group
    /// of its own, which its timeout ends whole.
    fn remote_git(&self, args: &[&str]) -> sys::Command {
        let mut command = sys::command_with("git", &self.env.vars);
        for name in GIT_VARS.iter().chain([&RECIPE]) {
            command.env_remove(name);
        }
        command
            .args(args)
            .current_dir(&self.main)
            .env("GIT_TERMINAL_PROMPT", "0")
            .process_group(0)
            .stdin(Stdio::null());
        command
    }

    /// What `git ARGS` with the remote ([`Landing::remote_git`]) answered within the land's
    /// network timeout ([`Settings::network`]), or the line saying why there is no answer.
    fn remote_output(&self, args: &[&str]) -> Result<std::process::Output, String> {
        if let Some(signal) = self.interrupted() {
            return Err(format!(
                "git {}: not run, the land took {}",
                args[0],
                signal_name(signal)
            ));
        }
        let mut command = self.remote_git(args);
        let child = sys::spawn_output(&mut command).map_err(|e| format!("git {}: {e}", args[0]))?;
        let group = child.id();
        self.running(group, &format!("git {}", args.join(" ")));
        let limit = self.settings.network();
        let (done, ended) = watched(group, limit, self.interrupts, || sys::wait_output(child));
        self.ran();
        match ended {
            Ended::Late => Err(format!(
                "git {} {REMOTE}: no answer within {} s",
                args[0],
                limit.as_secs()
            )),
            Ended::Interrupted(signal) => Err(format!(
                "git {} {REMOTE}: ended, the land took {}",
                args[0],
                signal_name(signal)
            )),
            Ended::No => done.map_err(|e| format!("git {}: {e}", args[0])),
        }
    }

    /// Whether `origin` answers and has no default branch.
    fn lacks_branch(&self) -> bool {
        let branch = self.branch();
        // `--exit-code`: 2 is an answer listing no such branch.
        self.remote_output(&["ls-remote", "--exit-code", "--heads", REMOTE, &branch])
            .is_ok_and(|done| done.status.code() == Some(2))
    }

    /// What the executor does when the branch, as approved, cannot land: fix it there and have its
    /// new tip approved.
    fn fix_on_branch(&self) -> String {
        let (slug, default) = (self.slug, &self.settings.default_branch);
        format!(
            "Merge {default} into {slug} and fix it there, then move it back to review (`just \
             tasks move {slug} review`) and ask its reviewer to approve the new tip naming it"
        )
    }

    /// `git ARGS` with the remote ([`Landing::remote_output`]): the line saying why it failed.
    fn remote(&self, args: &[&str]) -> Result<(), String> {
        let done = self.remote_output(args)?;
        if done.status.success() {
            return Ok(());
        }
        let said = String::from_utf8_lossy(&done.stderr);
        let said = said
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("failed");
        Err(format!("git {} {REMOTE}: {said}", args[0]))
    }
}

#[cfg(test)]
mod tests;
