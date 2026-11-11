//! `just land SLUG` (`tasks land`): the Symphony's default landing. `symphony init` installs its
//! recipe in every project (`symphony.just`), and a `land` recipe of the project's own replaces it
//! there, everywhere. It lands one task's branch, named as its slug, on the default branch:
//!
//! 1. **Approved.** The task's last verdict is its reviewer's `approve`, which left it done, and the
//!    approval covers the branch's tip: the commit the verdict names, else, when it names none of
//!    the branch's, every commit of the branch made before it (the board records no tip, so the
//!    landing says on the thread that it judged by time). A commit after it refuses, naming it.
//! 2. **Merged and checked.** In a worktree of its own (`land` in the project's `[agents]
//!    worktrees`, else `sf-tasks/land/worktree` in the git common dir), never the main checkout:
//!    one `--no-ff` merge commit, "Merge branch 'SLUG'", of the tip into the default branch; the
//!    project's `[land] generators`, whose changes, inside their outputs alone, are one commit
//!    after it; then `[verdicts] check`, which must pass on that tree. A conflict or a red refuses.
//! 3. **Moved.** The main checkout, with no staged or tracked edit and on the default branch,
//!    fast-forwards to what passed, so the default branch's new tree is the tree the check passed
//!    on. When the default branch moved meanwhile, the merge and the check run again on its new
//!    tip, at most [`TRIES`] times; a red then is said on the thread to the orchestrator.
//! 4. **Pushed.** Unless `[land] push = false` or there is no `origin`: the default branch is
//!    fetched from `origin`; what it gained there is merged in the same way, generators and check
//!    again, never forced; then the default branch alone is pushed, no other branch and no tag. A
//!    push that fails leaves the landing done here and says so, with the command to retry.
//! 5. **Closed and swept.** The board's `landed` writes `landed SLUG on main at SHA` on the
//!    thread and closes the task (an approved one stays done), and the sweep removes its finished
//!    worktrees.
//!
//! Two lands of one repository wait for each other ([`LOCK`] in `sf-tasks/` of the common dir).
//! What the generators and the check print goes to `sf-tasks/land/SLUG.log`. The settings are
//! read from the default branch as the main checkout has it committed, never from the branch
//! landing, so a branch cannot change its own check.

use crate::cli::Env;
use crate::sweep::git::{ancestor, git, rev, succeeds, worktrees};
use crate::sweep::{self, trigger};
use crate::sys::{self, Stdio};
use crate::{Error, LANDS_FROM, Message, Store, Task};
use serde::Deserialize;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

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
/// The start of the reason a land's worktree is locked with, which keeps it from the sweep.
const REASON: &str = "symphony land";
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
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            default_branch: "main".into(),
            check: Vec::new(),
            worktrees: None,
            generators: Vec::new(),
            push: true,
        }
    }
}

impl Settings {
    /// `text` as a project file: only the keys a land reads, the rest left to the Symphony's own
    /// reading. A generator with a key it does not know, no command or no outputs is refused.
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
        #[serde(default)]
        struct Land {
            generators: Vec<Generator>,
            push: bool,
        }
        impl Default for Land {
            fn default() -> Land {
                Land {
                    generators: Vec::new(),
                    push: true,
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
        Ok(Settings {
            default_branch,
            check: file.verdicts.check,
            worktrees: file.agents.worktrees,
            generators,
            push: file.land.push,
        })
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

/// The approval a land goes by: who gave it, when, and what it said.
#[derive(Debug, Clone, PartialEq)]
pub struct Approval {
    pub by: String,
    pub at: f64,
    pub text: String,
}

/// Whether a verdict's `text` is `verdict`, alone or before its `: text`.
fn is_verdict(text: &str, verdict: &str) -> bool {
    text.strip_prefix(verdict)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(':'))
}

/// The approval standing on `task`, read from its `thread`: its last verdict is an `approve` and
/// it is done. Otherwise the one line saying why there is none.
pub fn approval(task: &Task, thread: &[Message]) -> Result<Approval, String> {
    let slug = &task.slug;
    let verdicts: Vec<&Message> = thread.iter().filter(|m| m.kind == "verdict").collect();
    let approved = verdicts
        .iter()
        .rposition(|m| is_verdict(&m.text, "approve"));
    if task.status != "done" {
        return Err(format!(
            "{slug} is {}, with no approving verdict standing",
            task.status
        ));
    }
    match approved {
        Some(at) if at + 1 == verdicts.len() => {
            let verdict = verdicts[at];
            Ok(Approval {
                by: verdict.sender.clone(),
                at: verdict.ts,
                text: verdict.text.clone(),
            })
        }
        Some(_) => Err(format!(
            "{slug} was closed after its approval: {}",
            first_line(&verdicts[verdicts.len() - 1].text)
        )),
        None => Err(format!(
            "{slug} is done with no approving verdict from a reviewer"
        )),
    }
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
    Ok(Covered::ByTime(branch.len()))
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
/// line saying where it landed.
pub fn land(
    store: &Store,
    env: &Env,
    slug: &str,
    actor: &str,
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
    let _held = hold(&common, slug, out).map_err(Stop::Refused)?;
    let settings = Settings::of(&main).map_err(Stop::Refused)?;
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
    Landing {
        store,
        env,
        slug,
        actor,
        main,
        settings,
        tree,
        log,
        out,
    }
    .land()
}

fn at(path: &Path, error: &std::io::Error) -> String {
    format!("{}: {error}", path.display())
}

/// The lock in `common` for the land of `slug`: taken at once, or once the land holding it ends.
fn hold(common: &Path, slug: &str, out: &mut dyn Write) -> Result<sys::FileLock, String> {
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
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            let mut holder = String::new();
            let _ = file.read_to_string(&mut holder);
            let holder = holder.trim();
            let _ = writeln!(
                out,
                "land {slug}: waiting for {} to finish",
                if holder.is_empty() {
                    "another land"
                } else {
                    holder
                }
            );
            let _ = out.flush();
            file.lock().map_err(|e| at(&path, &e))?;
        }
        Err(TryLockError::Error(e)) => return Err(at(&path, &e)),
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

/// Why one merge and check refused, and where it is said.
struct Refusal {
    line: String,
    /// It is about the branch, so it goes on the thread too.
    thread: bool,
    /// Said there to the orchestrator: a red after the default branch moved.
    orchestrator: bool,
}

impl Refusal {
    fn here(line: String) -> Refusal {
        Refusal {
            line,
            thread: false,
            orchestrator: false,
        }
    }

    fn thread(line: String) -> Refusal {
        Refusal {
            line,
            thread: true,
            orchestrator: false,
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
    tree: PathBuf,
    log: PathBuf,
    out: &'a mut dyn Write,
}

impl Landing<'_> {
    fn say(&mut self, line: &str) {
        let _ = writeln!(self.out, "{line}");
        let _ = self.out.flush();
    }

    fn branch(&self) -> String {
        format!("refs/heads/{}", self.settings.default_branch)
    }

    /// Say `refusal` where it goes; the stop it makes.
    fn refuse(&self, refusal: Refusal) -> Stop {
        if refusal.thread {
            self.post(
                &format!("land refused: {}", refusal.line),
                refusal.orchestrator,
            );
        }
        Stop::Refused(refusal.line)
    }

    fn post(&self, text: &str, orchestrator: bool) {
        let to = orchestrator.then_some("@orchestrator");
        let _ = self.store.say(self.slug, self.actor, text, to);
    }

    fn land(mut self) -> Result<String, Stop> {
        let slug = self.slug;
        let task = self.store.get(slug)?;
        let thread = self.store.thread(slug)?;
        let approval =
            approval(&task, &thread).map_err(|line| self.refuse(Refusal::thread(line)))?;
        let tip = rev(&self.main, &format!("refs/heads/{slug}"))
            .ok_or_else(|| self.refuse(Refusal::thread(format!("no branch {slug} to land"))))?;
        self.clean().map_err(|line| self.refuse(line.into()))?;
        let mut base = self.base().map_err(|line| self.refuse(line.into()))?;
        let default = self.settings.default_branch.clone();
        if ancestor(&self.main, &tip, &base) {
            let line = format!("{slug}'s tip {} is already on {default}", short(&tip));
            return Err(self.refuse(Refusal::thread(line)));
        }
        let covered = covered(&self.main, slug, &base, &tip, &approval)
            .map_err(|line| self.refuse(Refusal::thread(line)))?;
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
                    self.say(&format!(
                        "land {slug}: {default} moved to {}; merging and checking again",
                        short(&now)
                    ));
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
        let mut landed = format!(
            "landed {slug} on {default} at {}",
            &merge[..merge.len().min(12)]
        );
        if let Some((commit, names)) = &regenerated {
            landed.push_str(&format!(", {names} regenerated in {}", short(commit)));
        }
        let pushed = match self.push(&top) {
            Pushed::Done => format!("; pushed to {REMOTE}"),
            Pushed::Off => "; not pushed: [land] push is off".into(),
            Pushed::NoRemote => format!("; not pushed: no {REMOTE} remote"),
            Pushed::Failed(why, retry) => {
                format!("; NOT PUSHED, landed here only: {why}. Push it with: {retry}")
            }
        };
        let line = format!("{landed}{pushed}");
        // The thread also says how the approval was judged, before the push, whose retry command
        // ends the line.
        let said = match covered {
            Covered::ByTime(n) => format!(
                "{landed}; judged by time, the approval naming no commit of the branch: its {} {} \
                 older than the approval{pushed}",
                commits(n),
                if n == 1 { "is" } else { "are" }
            ),
            Covered::Named => line.clone(),
        };
        // The approval on the board is the gate here, not who runs the land: any session may.
        if let Err(error) = self
            .store
            .landed(slug, self.actor, &said, true, &LANDS_FROM)
        {
            return Err(Stop::Refused(format!(
                "{line}, but the board did not close {slug}: {error}"
            )));
        }
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
                for swept in lines
                    .iter()
                    .filter(|l| l.starts_with("removed ") || l.starts_with("worktree sweep:"))
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
    /// of one a land left behind; one in its place that is not a land's is never touched.
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

    /// Run `command` in `dir`, as this land's actor for this task, its output in the log: whether
    /// it passed, or why it could not run.
    fn logged(&self, command: &[String], dir: &Path) -> Result<bool, String> {
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
        for name in GIT_VARS {
            child.env_remove(name);
        }
        // What the land runs is the task's, recorded for it and the session landing it.
        child
            .args(args)
            .current_dir(dir)
            .env("TASKS_SESSION", self.actor)
            .env("TASKS_TASK", self.slug)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| at(&self.log, &e))?)
            .stderr(log.try_clone().map_err(|e| at(&self.log, &e))?);
        let status = child
            .spawn()
            .and_then(|mut child| child.wait())
            .map_err(|e| format!("{}: {e}", command.join(" ")))?;
        let _ = writeln!(log, "exit {}", status.code().unwrap_or(-1));
        Ok(status.success())
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
            return Err(Refusal::thread(line));
        }
        let merge = rev(&path, "HEAD").ok_or("the merge left no HEAD")?;
        let regenerated = self.generate(&path)?;
        if !self.settings.check.is_empty() {
            let check = self.settings.check.join(" ");
            self.say(&format!("land {slug}: checking ({check})"));
            if !self.logged(&self.settings.check.clone(), &path)? {
                return Err(Refusal {
                    line: format!(
                        "the check `{check}` is red on {slug} merged into {default} at {}; its \
                         output is in {}",
                        short(base),
                        self.log.display()
                    ),
                    thread: true,
                    orchestrator: again,
                });
            }
        }
        let top = rev(&path, "HEAD").ok_or("the check left no HEAD")?;
        let checked = rev(&path, "HEAD^{tree}").ok_or("the check left no tree")?;
        drop(tree);
        self.clean()?;
        let done = git(&self.main, &["merge", "--ff-only", "--quiet", &top]);
        if done.as_ref().is_none_or(|done| !done.status.success()) {
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
            if !self.logged(&generator.command, path)? {
                return Err(Refusal::thread(format!(
                    "the generator {} failed on {slug} merged into {}; its output is in {}",
                    generator.name,
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

    /// Push the default branch, at `top` here, to `origin`: what it gained there merged first and
    /// checked again; never a force, no other branch, no tag.
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
        let push = format!("{0}:{0}", self.branch());
        let by_hand = format!("git push {REMOTE} {default}");
        let merge_by_hand = format!(
            "git fetch {REMOTE} && git merge {REMOTE}/{default} (in the main checkout), then {by_hand}"
        );
        let mut top = top.to_string();
        for attempt in 0..TRIES {
            let fetched = self.remote(&["fetch", "--quiet", "--no-tags", REMOTE, &fetch]);
            let gained = match fetched {
                Ok(()) => rev(&self.main, &tracking).filter(|at| !ancestor(&self.main, at, &top)),
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
                    Ok(Attempt::Raced(now)) => {
                        top = now;
                        continue;
                    }
                    Err(refusal) => {
                        if refusal.thread {
                            let said = format!("land of {slug} not pushed: {}", refusal.line);
                            self.post(&said, true);
                        }
                        return Pushed::Failed(refusal.line, merge_by_hand);
                    }
                }
            }
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
            self.say(&format!(
                "land {slug}: {REMOTE}/{default} moved while pushing; fetching again"
            ));
        }
        Pushed::Failed(
            format!("{REMOTE}/{default} kept moving while the land pushed"),
            merge_by_hand,
        )
    }

    /// `git ARGS` with the remote, in the main checkout, never asking for a password.
    fn remote_git(&self, args: &[&str]) -> sys::Command {
        let mut command = sys::command("git");
        for name in GIT_VARS {
            command.env_remove(name);
        }
        command
            .args(args)
            .current_dir(&self.main)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null());
        command
    }

    /// Whether `origin` answers and has no default branch.
    fn lacks_branch(&self) -> bool {
        let branch = self.branch();
        let mut command =
            self.remote_git(&["ls-remote", "--exit-code", "--heads", REMOTE, &branch]);
        // `--exit-code`: 2 is an answer listing no such branch.
        sys::output(&mut command).is_ok_and(|done| done.status.code() == Some(2))
    }

    /// `git ARGS` with the remote ([`Landing::remote_git`]): the line saying why it failed.
    fn remote(&self, args: &[&str]) -> Result<(), String> {
        let mut command = self.remote_git(args);
        let done = sys::output(&mut command).map_err(|e| format!("git {}: {e}", args[0]))?;
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
