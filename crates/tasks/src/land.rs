//! `just land SLUG` (`tasks land`): the Symphony's default landing. `symphony init` installs its
//! recipe in every project (`symphony.just`), and a `land` recipe of the project's own replaces it
//! there, everywhere. It lands one task's branch, named as its slug, on the default branch:
//!
//! A task a custom workflow holds lands by its graph instead, through the prepare and finalize
//! edges of this landing's adapter, with the same merge, check, move and push ([`adapter`]).
//!
//! 0. **Asked for.** It runs only as `just land SLUG`, whose recipe sets [`RECIPE`], and never
//!    where the project's justfile runs a `land` of its own ([`admit`]). The marker is a guard
//!    against running it by accident; the stamps below are the gate.
//! 1. **What it needs** ([`needs`]): the stamps the task needs at the branch's tip, its
//!    assessment's on the tip it read, else beside the catalogue's defaults (`review` alone in the
//!    Symphony's, so a task nobody assessed lands as before stamps), each other than `review` read
//!    there by the board's own reader. A missing one refuses, naming who gives it; one waiting on
//!    the owner alone holds the task blocked ([`Landing::lacks`]). Where `review` is not needed the
//!    task lands from review or blocked with no verdict, its landing line naming the tip its stamps
//!    stood on ([`needs::Needed::stood`]); where it is, by its approve:
//!
//!    **Approved.** The task's last verdict is an `approve`, which left it done, from a fresh
//!    reviewer: never a session that ever executed the task or one of its work subtasks (an
//!    instruction subtask, the review the reviewer claimed, is none), nor one that ever
//!    orchestrated it, nor its filer ([`independent`]). The approval covers the branch's tip:
//!    the commit the verdict names, else, when it names none of the branch's, every commit of the
//!    branch made before it and before the task last went to review (the board records no tip, so
//!    the landing says on the thread that it judged by time), or the commit the tip merges the
//!    default branch into, by a merge that settles documents alone, the task's tests then read
//!    at the tip whatever it needs ([`carried`]). A commit it does not cover refuses,
//!    naming it. An approve that does not do sends the task back to review for a fresh one; a
//!    conflict or a red on the tip as approved is said to its executor, which fixes it on the
//!    branch and sends the task back to review itself (a conflict in documents alone it settles
//!    by one merge the approve stands over, the task left done), and so is a worktree at a commit the land
//!    brings (on the branch, on another or detached) that holds an owner's answer no commit has
//!    ([`Landing::owner_lines`]): the owner's answer on a card the branch posted, which a land of
//!    its commits would leave behind. A worktree git cannot read for that refuses the land, said to
//!    the one landing. A tip the default branch holds already is
//!    finished only when this land recorded its merge ([`Record`]): a land interrupted after it
//!    moved the default branch pushes the commit it checked, writes its landed line and sweeps,
//!    nothing merged again; a tip merged by hand is refused, nothing pushed or recorded, the line
//!    saying how a land whose record was lost is finished by hand.
//! 2. **Merged and checked.** In a worktree of its own (`land` in the project's `[agents]
//!    worktrees`, else `symphony/tasks/land/worktree` in the git common dir), never the main
//!    checkout: one `--no-ff` merge commit, "Merge branch 'SLUG'", of the tip into the default
//!    branch, which keeps the default branch's side of every path inside the project's `[land]
//!    generators`' outputs that the branch changed, in a conflict or not ([`Landing::take_ours`]):
//!    what they make is the regenerator's, of the default branch's own sources, so no branch's
//!    build of it lands. The tip merged stays the one approved. Any other conflict refuses. The
//!    merge is committed through the project's git hooks, those git runs on a merge concluded
//!    after `--no-commit` ([`COMMIT_HOOKS`]): one that refuses is said as a red check is, naming
//!    the hooks. Then `[verdicts] check`, unless `[land] check = false`, must pass on the merge;
//!    with it off, a project's regenerator runs it on the default branch's tip after the land
//!    ([`Settings::main_check`]). A land runs no generator. This merge is one step,
//!    [`Landing::integrate`] with [`Merging::Tip`], the one seam a hosted pull-request merge would
//!    replace.
//! 3. **Moved.** The owner's answers are asked again, as in 1., for one saved while the land
//!    merged and checked. The land records its merge and the commit the check passed on, in
//!    `symphony/tasks/land/` and on the thread (which says `checked at` only when a check ran);
//!    then the main checkout, with no staged or tracked edit and on the default branch,
//!    fast-forwards to what passed, so the default branch's new tree is the tree the check passed
//!    on (the merge's, with none). The fast-forward runs in a process group of its own,
//!    which no terminal's signal reaches and the land forwards none to, so nothing but its timeout
//!    stops it part way; where the default branch is once it ends says whether it moved, and only
//!    a move that did not happen drops the record, restoring what it half-wrote in the main
//!    checkout ([`Landing::fast_forward`], [`Landing::restore`]). [`MOVING`] names the move
//!    meanwhile, so the next land restores what one killed outright left ([`Landing::recover`]).
//!    When the default branch moved meanwhile, the merge and the check run again on its new tip,
//!    at most [`TRIES`] times; a red then is said on the thread to the orchestrator. Once it
//!    moved, the Symphony saves a line given on the branch's page on the default branch; an
//!    owner's answer saved into a worktree while it moved is found after, and said on the thread
//!    to the orchestrator ([`Landing::owner_lines_after`]).
//! 4. **Pushed.** Unless `[land] push = false` or there is no `origin`: the default branch is
//!    fetched from `origin`; what it gained there is merged in the same way and checked again,
//!    never forced, its generated paths kept but where they conflict; then the default branch
//!    alone is pushed, no other branch and no tag. A push that fails leaves the landing done here
//!    and says so, with the command to retry.
//! 5. **Closed.** The board's `landed` writes `landed SLUG on main at SHA` on the thread and
//!    closes the task (an approved one stays done). What is slow is then started detached, and the
//!    land waits for nothing it does: the sweep of the task's finished worktrees
//!    ([`trigger::trigger`]) and, in a project with generators, the regenerator
//!    ([`regenerate::start`]), neither under the land's lock; then, last, the project's
//!    `[board] on_land` ([`crate::on_land`]), which so finds the regenerator started.
//!
//! Two lands of one repository wait for each other ([`LOCK`] in `symphony/tasks/` of the common
//! dir), with no timeout: the lock guards a merge, a check and a push, and is freed when its
//! holder's process ends however it ends. The merge's commit, with its hooks, and the check each
//! run in a process group of its own, ended whole past `[land] timeout_seconds` ([`TIMEOUT`]
//! unless the project says), and a fetch or a push within [`NETWORK`]; what is left of a group
//! once its first process ends is ended too.
//! A hang-up, an interrupt or a terminate signal (a terminal's Ctrl-C, `kill`) ends the group
//! running, or a land waiting for the lock, but the fast-forward, which it waits for, removes the
//! land's worktree and ends the land by that signal: before the default branch moved nothing
//! landed, and after it a run again finishes it. The group running is named in [`RUNNING`]
//! meanwhile, with when its first process started, so a land killed outright leaves its name: the
//! next land refuses while that group lives, or while any process works in the worktree it left,
//! and tells how to end the group only while its first process is the one the land started
//! ([`Landing::leftover_group`]). What the commit's hooks and the check print goes to
//! `symphony/tasks/land/SLUG.log`, the slug's `/` as `%2F` ([`file_stem`]). The settings are read
//! from the default branch as the main checkout has it committed, never from the branch landing, so
//! a branch cannot change its own check.

use crate::cli::Env;
use crate::on_review::{self, Started};
use crate::stamps::short;
use crate::sweep::git::{Answer, Listed, ancestor, answers, git, rev, succeeds, told, worktrees};
use crate::sweep::trigger;
use crate::sys::{self, Stdio};
use crate::{Error, Held, Message, Store, Task, Unfresh};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

pub mod adapter;
pub mod carried;
pub mod check_paths;
pub mod delivery;
pub mod dispatch;
pub mod evidence;
mod moving;
use moving::Move;
pub use moving::Moving;
pub mod needs;
use needs::Needed;
pub mod regenerate;

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
/// The file in the lands' folder naming the process group a land runs, while it runs: the group,
/// when its first process started ([`started`]), whether a later land waits for it (`wait`, the
/// fast-forward of the main checkout) or may end it (`end`), and its command, a tab between each.
pub const RUNNING: &str = "running";
/// The file in the lands' folder naming the fast-forward of the main checkout a land makes, while
/// it makes it ([`Moving`]).
pub const MOVING: &str = "moving";
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
/// What names a repository to git ([`sys::GIT_VARS`]): a child run in the land's worktree inherits
/// none of them, nor does the command `[board] on_land` names ([`crate::on_land`]).
pub(crate) use crate::sys::GIT_VARS;

/// A command the regenerator ([`regenerate`]) runs on the default branch's newest tip after a land,
/// whose changes inside `outputs` it commits and lands; a project names its own in `[land]
/// generators`, as `[verdicts] generators`. A land runs none, and keeps the default branch's side
/// of each path inside `outputs` a task's branch changed.
///
/// Consumers' code builds one by its fields (a project's test of its own manifest), and a release
/// never changes that code (a consumer moves its pin by its own landing): a field added here breaks
/// it, so what a generator gains goes beside it, as its key does ([`Settings::keys`]).
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

/// `keys` (`[land] keys`, a generator's name to its key's command) checked against `generators`:
/// a name no generator has, or a key with no command, is refused by its name. A generator is named
/// by its name alone, in its key and in what the regenerator says, so two of one name are refused
/// first, keyed or not.
pub fn keyed(
    generators: &[Generator],
    keys: BTreeMap<String, Vec<String>>,
) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut named = std::collections::BTreeSet::new();
    if let Some(twice) = generators.iter().find(|g| !named.insert(&g.name)) {
        return Err(format!(
            "[land] generators names {:?} twice: give each generator a name of its own",
            twice.name
        ));
    }
    for (name, key) in &keys {
        if !generators.iter().any(|g| &g.name == name) {
            return Err(format!(
                "[land] keys names {name:?}, which no [land] generator is"
            ));
        }
        if key.first().is_none_or(String::is_empty) {
            return Err(format!(
                "[land] keys gives the generator {name} a key with no command"
            ));
        }
    }
    Ok(keys)
}

/// What a land reads from the project's file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// `[project] default_branch`.
    pub default_branch: String,
    /// `[verdicts] check`: what must pass on the merge; empty runs none, as `[land] check = false`
    /// makes it.
    pub check: Vec<String>,
    /// `[verdicts] check` where `[land] check = false` keeps it out of the land: what the
    /// regenerator runs instead on what each round made of the default branch's tip
    /// ([`regenerate`]), so a red there is found after the land that brought it; empty where the
    /// land runs it.
    pub main_check: Vec<String>,
    /// `[agents] worktrees`, relative to the root.
    pub worktrees: Option<String>,
    /// `[land] generators`, in order: run after a land by the regenerator ([`regenerate`]), never
    /// by the land.
    pub generators: Vec<Generator>,
    /// `[land] keys`, a generator's name to its key: a command printing, as its last line, a key of
    /// everything the generator reads. On a tip whose key and outputs are a pair the generator
    /// passed on before, the regenerator does not run it ([`regenerate::Passed`]); a generator with
    /// none runs on every tip. Never in the generator's own table, so a Symphony older than keys,
    /// which refuses a key it does not know in a generator, leaves the table out and still reads
    /// the file ([`keyed`]).
    pub keys: BTreeMap<String, Vec<String>>,
    /// `[land] push`: whether a landing pushes the default branch to `origin`.
    pub push: bool,
    /// `[land] timeout_seconds`: how long each generator and the check may run; a fetch or a push
    /// has [`NETWORK`] at most. A land waits for another with no timeout ([`hold`]).
    pub timeout: Duration,
    /// `[land] check_paths`: what holds the project's tests and checks, reported to its assessor
    /// ([`check_paths`]); [`check_paths::DEFAULT`] unless
    /// the project names its own.
    pub check_paths: Vec<check_paths::Entry>,
    /// The folders the project's `[[entrypoints]]` serve documents from, where the Symphony writes
    /// the owner's answers ([`crate::project::documents`]); `docs` unless the project names its own.
    pub documents: Vec<String>,
    /// `[land.upstream]`: the repository whose history this one imports, where it names one. A
    /// land delivered by a pull request merges an import of it by a merge commit
    /// ([`delivery`]); with none, no head of another history is delivered.
    pub upstream: Option<Upstream>,
    /// `[land] watch_seconds`: how long the delivery watcher rests between two rounds of reading
    /// what the host made of each armed pull request ([`dispatch`]).
    pub watch: Duration,
    /// `[land] merge_within_minutes`: how long an armed pull request whose evidence stands may stay
    /// neither merged nor red before the watcher disarms it and says so ([`dispatch`]).
    pub merge_within: Duration,
}

/// The default of `[land] watch_seconds`: a host merges seconds after the check it waits on turns
/// green, so a merge is closed here within half a minute, at one read of each such pull request a
/// round.
pub const WATCH: Duration = Duration::from_secs(30);
/// The default of `[land] merge_within_minutes`.
pub const MERGE_WITHIN: Duration = Duration::from_secs(30 * 60);

/// `[land.upstream]`: the one repository a project imports from, as git reaches it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Upstream {
    /// What git fetches it from: never the project's own `origin`.
    pub url: String,
    /// Its default branch, which an import's commit must be on.
    pub branch: String,
}

impl Upstream {
    /// It, or why the land refuses it: a value git could read as an option, or one that names
    /// nothing.
    ///
    /// # Errors
    ///
    /// The line that refuses it, the table and key named.
    pub fn judged(self) -> Result<Upstream, String> {
        for (key, value) in [("url", &self.url), ("branch", &self.branch)] {
            if value.trim().is_empty() || value.starts_with('-') || value.trim() != value {
                return Err(format!(
                    "[land.upstream] {key} is {value:?}: it names the repository an import comes \
                     from, and its default branch"
                ));
            }
        }
        if !(self.branch.chars()).all(|c| c.is_ascii_alphanumeric() || "-._/".contains(c)) {
            return Err(format!(
                "[land.upstream] branch {:?} is not a plain branch name",
                self.branch
            ));
        }
        Ok(self)
    }
}

/// How long a generator or a land's check takes at most, unless the project names another `[land]
/// timeout_seconds`: an hour.
pub const TIMEOUT: Duration = Duration::from_secs(3600);
/// How long a fetch or a push may take at most, the project's timeout when shorter: five minutes.
pub const NETWORK: Duration = Duration::from_secs(300);

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            default_branch: "main".into(),
            check: Vec::new(),
            main_check: Vec::new(),
            worktrees: None,
            generators: Vec::new(),
            keys: BTreeMap::new(),
            push: true,
            timeout: TIMEOUT,
            check_paths: check_paths::defaults(),
            documents: crate::project::documents(""),
            upstream: None,
            watch: WATCH,
            merge_within: MERGE_WITHIN,
        }
    }
}

impl Settings {
    /// `text` as a project file: only the keys a land reads, the rest left to the Symphony's own
    /// reading, but the whole of `[land]`, which is the land's alone: a key it does not know there
    /// (a typo of `push`, say) is refused by its name, as is a generator with a key it does not
    /// know, no command or no outputs, a timeout of no time and a `check_paths` entry naming
    /// nothing inside the project ([`check_paths::Entry::parse`]).
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
            keys: BTreeMap<String, Vec<String>>,
            push: bool,
            timeout_seconds: u64,
            check: bool,
            check_paths: Option<Vec<String>>,
            /// Read by [`delivery::command`], and held to its shape here.
            delivery: Vec<String>,
            upstream: Option<Upstream>,
            watch_seconds: u64,
            merge_within_minutes: u64,
        }
        impl Default for Land {
            fn default() -> Land {
                Land {
                    generators: Vec::new(),
                    keys: BTreeMap::new(),
                    push: true,
                    timeout_seconds: TIMEOUT.as_secs(),
                    check: true,
                    check_paths: None,
                    delivery: Vec::new(),
                    upstream: None,
                    watch_seconds: WATCH.as_secs(),
                    merge_within_minutes: MERGE_WITHIN.as_secs() / 60,
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
        let keys =
            keyed(&generators, file.land.keys).map_err(|why| format!("{MANIFEST}: {why}"))?;
        if file.land.timeout_seconds == 0 {
            return Err(format!(
                "{MANIFEST}: [land] timeout_seconds is 0, which no generator or check runs in"
            ));
        }
        if file
            .land
            .delivery
            .first()
            .is_some_and(|program| program.trim().is_empty())
        {
            return Err(format!(
                "{MANIFEST}: [land] delivery names no command: leave it out to land here"
            ));
        }
        if file.land.watch_seconds == 0 {
            return Err(format!(
                "{MANIFEST}: [land] watch_seconds is 0: the watcher rests between its rounds"
            ));
        }
        if file.land.merge_within_minutes == 0 {
            return Err(format!(
                "{MANIFEST}: [land] merge_within_minutes is 0: a host is given time to merge"
            ));
        }
        let (check, main_check) = if file.land.check {
            (file.verdicts.check, Vec::new())
        } else {
            (Vec::new(), file.verdicts.check)
        };
        let check_paths = match &file.land.check_paths {
            Some(written) => {
                check_paths::parse(written).map_err(|why| format!("{MANIFEST}: {why}"))?
            }
            None => check_paths::defaults(),
        };
        let upstream = (file.land.upstream.map(Upstream::judged))
            .transpose()
            .map_err(|why| format!("{MANIFEST}: {why}"))?;
        Ok(Settings {
            default_branch,
            check,
            main_check,
            worktrees: file.agents.worktrees,
            generators,
            keys,
            push: file.land.push,
            timeout: Duration::from_secs(file.land.timeout_seconds),
            check_paths,
            documents: crate::project::documents(text),
            upstream,
            watch: Duration::from_secs(file.land.watch_seconds),
            merge_within: Duration::from_secs(file.land.merge_within_minutes.saturating_mul(60)),
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
/// outputs: by the project file's one rule for a path ([`crate::project::inside`]), a folder's
/// slash kept (so `gen/` is never `gen-x/`), or none for a path that rule refuses.
fn inside(path: &str) -> Option<String> {
    let slash = if path.ends_with('/') { "/" } else { "" };
    let inside = crate::project::inside("[land] generators outputs", path).ok()?;
    Some(inside + slash)
}

/// Whether the generator's `outputs` ([`inside`]) hold the project's `path`: the path itself, or
/// one inside the folder it names, so `gen` and `gen/` hold `gen/a` but never `gen-x/a` or
/// `gen.txt`.
pub fn owns(outputs: &str, path: &str) -> bool {
    match outputs.strip_suffix('/') {
        Some(_) => path.starts_with(outputs),
        None => path
            .strip_prefix(outputs)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/')),
    }
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
/// `approve` ([`crate::approved`]). Otherwise the one line saying why there is none, and, for open
/// work, who approves it and by which command.
pub fn approval(task: &Task, thread: &[Message]) -> Result<Approval, String> {
    let slug = &task.slug;
    if task.status != "done" {
        let approve = format!(
            "a fresh reviewer, one that did not write it, approves it naming the tip it read: \
             `just tasks verdict {slug} approve \"FULL_SHA: why\" --as HANDLE`"
        );
        let then = match task.status.as_str() {
            "cancelled" => String::new(),
            "review" => format!(": {approve}"),
            _ => format!(": it goes to review (`just tasks move {slug} review`), where {approve}"),
        };
        return Err(format!(
            "{slug} is {}, with no approving verdict standing{then}",
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

/// Whether `approval` is from a fresh reviewer of `task`, by the board's verdict rule
/// ([`Held::unfresh`]): never one that executed, orchestrated or filed the work.
pub fn independent(task: &Task, approval: &Approval, held: &Held) -> Result<(), String> {
    let (slug, by) = (&task.slug, &approval.by);
    let orchestrates = task.holders("orchestrator").contains(&by.as_str());
    let whose = match held.unfresh(by, &task.created_by, orchestrates) {
        None => return Ok(()),
        Some(Unfresh::Executed) => {
            return Err(format!(
                "{slug}'s approve is {by}'s, which executed it or work under it: a fresh \
                 reviewer, one that did not write or execute it, approves it"
            ));
        }
        Some(Unfresh::Orchestrated) => "which orchestrated it",
        Some(Unfresh::Filed) => "which filed it",
    };
    Err(format!(
        "{slug}'s approve is {by}'s, {whose}: a fresh reviewer approves it"
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

/// Why the checkout `main` has no branch `slug` to land, and the rename that gives it one: the land
/// takes the branch named as the task, so work on another is renamed to it. The branch named is
/// one whose tip is a commit `approved` (the approve's text, empty with none) names, when one is,
/// never the `default` branch; else the rename is said with `BRANCH`.
fn no_branch(main: &Path, slug: &str, default: &str, approved: &str) -> String {
    let at: Vec<(String, String)> = named(approved)
        .into_iter()
        .filter_map(|word| rev(main, &format!("{word}^{{commit}}")))
        .flat_map(|sha| {
            let points = format!("--points-at={sha}");
            let args = [
                "for-each-ref",
                &points,
                "--format=%(refname:short)",
                "refs/heads/",
            ];
            let names = run_git(main, &args).unwrap_or_default();
            (names.lines())
                .filter(|name| *name != default)
                .map(|name| (name.to_string(), sha.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    let lands = format!("the land takes the branch named as the task, {slug}");
    match at.first() {
        Some((branch, sha)) => format!(
            "no branch {slug} to land: {lands}, and the approved tip {} is on {branch}: `git \
             branch -m {branch} {slug}` renames it, then land again",
            short(sha)
        ),
        None => format!(
            "no branch {slug} to land: {lands}: rename the branch holding the approved work \
             (`git branch -m BRANCH {slug}`), then land again"
        ),
    }
}

/// What a verdict that approved `slug` says, run where `env` runs: that it is done on the board but
/// not on the default branch until it lands, and the line that lands it: `just land SLUG --as
/// HANDLE`, the Symphony's default landing, or the `land` recipe of the project's own that replaces
/// it ([`own_land`]). Outside a checkout, or where the justfile cannot be read for it, the default
/// landing is named. What an approve of a task says is [`approved_task`], which says this
/// wherever the host does not merge it.
pub fn approved(env: &Env, slug: &str) -> String {
    let (main, default) = lands_on(env);
    approved_here(env, slug, main.as_deref(), &default)
}

/// What a verdict that approved `task` says, run where `env` runs. Where the project delivers by
/// pull request ([`dispatch::bound`]) and the task is one the delivery takes
/// ([`on_review::delivery_task`]), nobody lands it: the host merges its pull request, named once
/// its dispatch bound one, and the board closes the task, `just land` being only what settles it
/// by hand. Anywhere else, a project whose `land` is a recipe of its own included, it is
/// [`approved`]'s line.
pub fn approved_task(env: &Env, task: &Task) -> String {
    let slug = &task.slug;
    let (main, default) = lands_on(env);
    let bound = (main.as_deref())
        .filter(|_| on_review::delivery_task(task.parent.as_deref(), task.instruction))
        .and_then(|main| Some((main, dispatch::bound(main, slug)?)))
        .filter(|(main, _)| !matches!(own_land(main, env), Ok(Some(_))))
        .map(|(_, bound)| bound);
    let Some(bound) = bound else {
        return approved_here(env, slug, main.as_deref(), &default);
    };
    let pull = bound.map_or_else(
        || "its pull request, bound by its dispatch,".into(),
        |b| format!("pull request #{} of {} ({})", b.pull, b.name, b.url),
    );
    format!(
        "{slug} is approved: done on the board, not on {default} until the host merges it; the \
         host merges {pull} once its checks pass, and the board closes the task; `just land \
         {slug} --as HANDLE` only settles it by hand"
    )
}

/// The main checkout of the repository `env` runs in, none outside one, and the default branch
/// its settings name, as a line says it.
fn lands_on(env: &Env) -> (Option<PathBuf>, String) {
    let main = crate::git::common_dir(&env.cwd)
        .ok()
        .and_then(|common| trigger::main_checkout(&common).map(Path::to_path_buf));
    let default = (main.as_deref())
        .and_then(|main| Settings::of(main).ok())
        .map_or_else(|| "the default branch".into(), |s| s.default_branch);
    (main, default)
}

/// [`approved`]'s line for a land made here, on `default` of the main checkout `main`.
fn approved_here(env: &Env, slug: &str, main: Option<&Path>, default: &str) -> String {
    let how = match main.and_then(|main| own_land(main, env).ok().flatten()) {
        Some(justfile) => format!(
            "the `land` recipe of the project's own lands it ({}): `just land {slug}`",
            justfile.display()
        ),
        None => format!("`just land {slug} --as HANDLE` lands it"),
    };
    format!("{slug} is approved: done on the board, not on {default} until it lands; {how}")
}

/// How an approval covers the tip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Covered {
    /// It names the tip.
    Named,
    /// It names none of the branch's commits, and each of the branch's `n` commits is older.
    ByTime(usize),
    /// It names the commit the tip merges the default branch into, a merge that settles documents
    /// alone ([`carried`]).
    Merged(carried::Carried),
}

/// How a land found its task ready ([`needs`]).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Judged {
    /// It needs `review`: a fresh reviewer's approve, covering the tip so, left it done.
    Approved(Covered),
    /// It needs no review: its stamps stand at the tip, and it is in review, blocked, or done by
    /// an approve. What its landing's line adds ([`Needed::stood`]).
    Stamped(String),
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
    match carried::over(dir, base, tip, &resolved) {
        Ok(carried) => return Ok(Covered::Merged(carried)),
        Err(Some(why)) => {
            return Err(format!(
                "{slug}'s tip {} is one merge over its approved commit, but {why}",
                short(tip)
            ));
        }
        Err(None) => {}
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

/// `slug` as the stem of a file name in the lands' folder, one a slug alone has and gives back: `%`
/// as `%25` and `/` as `%2F`, so `a/b-c` and `a-b/c`, which were both `a-b-c`, are two names.
pub fn file_stem(slug: &str) -> String {
    slug.replace('%', "%25").replace('/', "%2F")
}

/// `slug` again from its [`file_stem`].
pub fn slug_of(stem: &str) -> String {
    stem.replace("%2F", "/").replace("%25", "%")
}

/// When process `pid` started, as the kernel stamps it ([`sys::process_starts`]), in words equal
/// for that process alone, whatever took its number after it: none for one that ended or whose
/// start cannot be read.
pub fn started(pid: i64) -> Option<String> {
    match sys::process_starts(&[pid]).pop()? {
        sys::Start::At(at) => Some(format!("{at:.6}")),
        sys::Start::AfterBoot(clock) => Some(format!("{}+{:.2}", clock.boot, clock.at)),
        sys::Start::Ended | sys::Start::Unreadable => None,
    }
}

/// What a land records of its merge before it moves the default branch, in the lands' folder
/// ([`Record::path`]) and on the thread: the task, the tip it merged, its merge commit, and the
/// commit the check passed on (the merge, or the generators' commit after it), and the generated
/// paths whose conflict it took from the default branch to regenerate. A tip the default branch
/// holds is a land to finish ([`Landing::finish`]) only by such a record of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub slug: String,
    pub tip: String,
    pub merge: String,
    pub checked: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regenerated: Vec<String>,
}

impl Record {
    /// Where the record of `slug`'s land is kept in the lands' `folder`: `SLUG.merged`, by its
    /// [`file_stem`].
    pub fn path(folder: &Path, slug: &str) -> PathBuf {
        folder.join(format!("{}.merged", file_stem(slug)))
    }

    /// Where a land before [`file_stem`] kept it: the slug's slashes as dashes, a name two slugs
    /// can share, so the record there counts only as the one its `slug` names.
    pub(crate) fn dashed(folder: &Path, slug: &str) -> PathBuf {
        folder.join(format!("{}.merged", slug.replace('/', "-")))
    }

    /// The record of `slug`'s land kept in `folder`, if there is one of that task, under its name
    /// or the name a land before kept it by.
    pub fn read(folder: &Path, slug: &str) -> Option<Record> {
        [Record::path(folder, slug), Record::dashed(folder, slug)]
            .iter()
            .find_map(|path| {
                let text = fs::read_to_string(path).ok()?;
                crate::objects::parse::<Record>(&text).filter(|record| record.slug == slug)
            })
    }

    /// Kept in `folder`, whole or not at all.
    pub fn write(&self, folder: &Path) -> Result<(), String> {
        let path = Record::path(folder, &self.slug);
        let staged = folder.join(format!(".{}.{}", file_stem(&self.slug), std::process::id()));
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        fs::write(&staged, text).map_err(|e| at(&staged, &e))?;
        fs::rename(&staged, &path).map_err(|e| at(&path, &e))
    }

    /// The record of `slug`'s land in `folder` dropped: its land closed, or its merge never moved
    /// the default branch. One a land before kept under the dashed name goes too when it is this
    /// task's, never another's that shares the name.
    pub fn drop_in(folder: &Path, slug: &str) {
        let _ = fs::remove_file(Record::path(folder, slug));
        let dashed = Record::dashed(folder, slug);
        let ours = fs::read_to_string(&dashed)
            .ok()
            .and_then(|text| crate::objects::parse::<Record>(&text))
            .is_some_and(|record| record.slug == slug);
        if ours {
            let _ = fs::remove_file(dashed);
        }
    }
}

/// A group [`RUNNING`] names: its id, when its first process started (none when it was not read,
/// or the file is an earlier land's, which wrote no start), whether a later land waits for it
/// rather than ending it, and its command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub group: u32,
    pub start: Option<String>,
    pub waits: bool,
    pub command: String,
}

impl Named {
    /// As [`RUNNING`] holds it.
    pub fn line(&self) -> String {
        format!(
            "{}\tstarted {}\t{}\t{}\n",
            self.group,
            self.start.as_deref().unwrap_or("-"),
            if self.waits { "wait" } else { "end" },
            self.command
        )
    }

    /// What [`RUNNING`] holds as its group: this land's line, or an earlier land's, which named
    /// only the group and its command.
    pub fn parse(text: &str) -> Option<Named> {
        let text = text.trim_end_matches('\n');
        let mut fields = text.splitn(4, '\t');
        let group = fields.next()?.trim().parse().ok()?;
        let rest: Vec<&str> = fields.collect();
        if let [start, kind @ ("end" | "wait"), command] = rest.as_slice()
            && let Some(start) = start.strip_prefix("started ")
        {
            return Some(Named {
                group,
                start: (start != "-").then(|| start.to_string()),
                waits: *kind == "wait",
                command: (*command).to_string(),
            });
        }
        Some(Named {
            group,
            start: None,
            waits: false,
            command: rest.join("\t"),
        })
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
    land_with_clock(store, env, slug, actor, recorder, out, &Instant::now)
}

/// [`land`] with the monotonic clock its command timeouts read. A test can hold time still
/// through setup and advance it once the command it means to expire is ready. Signal polling
/// and the grace for ending a process group still use real time.
pub fn land_with_clock(
    store: &Store,
    env: &Env,
    slug: &str,
    actor: &str,
    recorder: Option<&Path>,
    out: &mut dyn Write,
    now: &(dyn Fn() -> Instant + Sync),
) -> Result<String, Stop> {
    landing(
        store,
        env,
        slug,
        actor,
        recorder,
        out,
        now,
        None,
        Mode::Land,
    )
}

/// [`land`] with `transport` answering a delivery's verbs in place of the command `[land]
/// delivery` names: how a test stands a host in. It changes nothing of a land that delivers
/// nothing.
pub fn land_by(
    store: &Store,
    env: &Env,
    slug: &str,
    actor: &str,
    out: &mut dyn Write,
    transport: &dyn delivery::Transport,
) -> Result<String, Stop> {
    land_by_with_clock(store, env, slug, actor, out, transport, &Instant::now)
}

/// [`land_by`] with the clock of [`land_with_clock`]: how a test expires a delivery's fetch once
/// the remote it waits on is known to hold it.
pub fn land_by_with_clock(
    store: &Store,
    env: &Env,
    slug: &str,
    actor: &str,
    out: &mut dyn Write,
    transport: &dyn delivery::Transport,
    now: &(dyn Fn() -> Instant + Sync),
) -> Result<String, Stop> {
    landing(
        store,
        env,
        slug,
        actor,
        None,
        out,
        now,
        Some(transport),
        Mode::Land,
    )
}

/// `tasks land SLUG --dispatch`: the task's head published to its pull request with its evidence
/// whatever it says, and the host's own merge of it armed ([`dispatch`]); started detached by the
/// board's writes that can change the evidence, never waited on.
///
/// # Errors
///
/// What a land refuses before it publishes, or the board failing.
pub fn dispatch(
    store: &Store,
    env: &Env,
    slug: &str,
    actor: &str,
    out: &mut dyn Write,
) -> Result<String, Stop> {
    landing(
        store,
        env,
        slug,
        actor,
        None,
        out,
        &Instant::now,
        None,
        Mode::Dispatch,
    )
}

/// [`dispatch`] with `transport` answering the delivery's verbs, as [`land_by`] does.
///
/// # Errors
///
/// As [`dispatch`].
pub fn dispatch_by(
    store: &Store,
    env: &Env,
    slug: &str,
    actor: &str,
    out: &mut dyn Write,
    transport: &dyn delivery::Transport,
) -> Result<String, Stop> {
    let mode = Mode::Dispatch;
    landing(
        store,
        env,
        slug,
        actor,
        None,
        out,
        &Instant::now,
        Some(transport),
        mode,
    )
}

/// One round of the delivery watcher for `slug`, armed at the host, at `clock` (seconds since
/// 1970), under the land's lock ([`dispatch::watch`]).
fn watch_round(
    store: &Store,
    env: &Env,
    slug: &str,
    actor: &str,
    out: &mut dyn Write,
    transport: Option<&dyn delivery::Transport>,
    clock: i64,
) -> Result<String, Stop> {
    let mode = Mode::Watch(clock);
    landing(
        store,
        env,
        slug,
        actor,
        None,
        out,
        &Instant::now,
        transport,
        mode,
    )
}

/// What a [`landing`] does once it holds the land's lock.
#[derive(Clone, Copy)]
enum Mode {
    /// A land: `tasks land SLUG`.
    Land,
    /// A dispatch at review ([`dispatch`]).
    Dispatch,
    /// A round of the delivery watcher at a wall clock, in whole seconds since 1970.
    Watch(i64),
}

#[expect(
    clippy::too_many_arguments,
    reason = "the land's two entries, said once"
)]
fn landing(
    store: &Store,
    env: &Env,
    slug: &str,
    actor: &str,
    recorder: Option<&Path>,
    out: &mut dyn Write,
    now: &(dyn Fn() -> Instant + Sync),
    transport: Option<&dyn delivery::Transport>,
    mode: Mode,
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
    let delivery = delivery::command(&main).map_err(Stop::Refused)?;
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
    let _held =
        hold(&common, slug, interrupts.as_ref(), out).map_err(|line| ended(Stop::Refused(line)))?;
    let folder = trigger::folder(&common).join(FOLDER);
    fs::create_dir_all(&folder).map_err(|e| Stop::Refused(at(&folder, &e)))?;
    let tree = worktree(&main, &settings, &folder, FOLDER, "worktree").map_err(Stop::Refused)?;
    let log = folder.join(format!("{}.log", file_stem(slug)));
    // A land's log is its own; a dispatch or a round of the watcher, which runs no command, adds
    // to the one there.
    match mode {
        Mode::Land => File::create(&log).map(drop),
        Mode::Dispatch | Mode::Watch(_) => OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .map(drop),
    }
    .map_err(|e| Stop::Refused(at(&log, &e)))?;
    let landing = Landing {
        store,
        env,
        slug,
        actor,
        main,
        settings,
        running: folder.join(RUNNING),
        task: true,
        folder,
        tree,
        log,
        interrupts: interrupts.as_ref(),
        now,
        recorder,
        named: false,
        transport,
        delivery,
        mode,
        out,
    };
    let landed = match mode {
        Mode::Land => landing.land(),
        Mode::Dispatch => landing.dispatched(),
        Mode::Watch(clock) => landing.watched(clock),
    };
    landed.map_err(ended)
}

fn at(path: &Path, error: &std::io::Error) -> String {
    format!("{}: {error}", path.display())
}

/// Where the worktree `name` of the main checkout `main` goes: in the project's `[agents]
/// worktrees` folder, made when missing, else as `fallback` in the lands' `folder`.
fn worktree(
    main: &Path,
    settings: &Settings,
    folder: &Path,
    name: &str,
    fallback: &str,
) -> Result<PathBuf, String> {
    match &settings.worktrees {
        Some(worktrees) => {
            let parent = main.join(worktrees);
            fs::create_dir_all(&parent).map_err(|e| at(&parent, &e))?;
            Ok(parent
                .canonicalize()
                .map_err(|e| at(&parent, &e))?
                .join(name))
        }
        None => Ok(folder.join(fallback)),
    }
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
    // A head a pull request squashed is in no history: its receipt says it landed.
    let lands = trigger::folder(&common).join(FOLDER);
    if let Some(receipt) = delivery::Receipt::read(&lands, slug).filter(|r| r.head == tip) {
        return Err(Error::Refused(format!(
            "{slug}'s tip {} is on {default} already, squashed as {} by pull request #{}: it does \
             not go back to review, and what it still needs goes in a new task",
            short(&tip),
            short(&receipt.squash),
            receipt.pull
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
            "`tasks land` runs only as `just land SLUG`, the recipe that moves and pushes the \
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
    names.sort_unstable();
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
/// however long that is, since it is freed when its holder's process ends and a land holds it
/// for its merge, its move and its push alone; once `interrupts` took a signal, the line naming
/// the land that holds it.
fn hold(
    common: &Path,
    slug: &str,
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
    let mut said = false;
    loop {
        match file.try_lock() {
            Ok(()) => break,
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
                #[expect(
                    clippy::disallowed_methods,
                    reason = "no-timed-waits baseline: tries the land's lock again until it is free, where a blocking lock is the event"
                )]
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(TryLockError::Error(e)) => return Err(at(&path, &e)),
        }
    }
    // Named in place, as the lock is the file's, in one write: a land waiting meanwhile reads it
    // empty ("another land") or whole.
    let held = sys::FileLock(file);
    let name = format!("the land of {slug} (pid {})", std::process::id());
    let mut file = &held.0;
    let _ = file.set_len(0);
    let _ = file.seek(SeekFrom::Start(0));
    let _ = file.write_all(name.as_bytes());
    Ok(held)
}

/// The hooks `git commit` runs that can refuse a commit, in the order it runs them.
const COMMIT_HOOKS: [&str; 3] = ["pre-commit", "prepare-commit-msg", "commit-msg"];

/// Which of [`COMMIT_HOOKS`] a commit in the checkout `dir` runs: those git finds there (in the
/// repository's hooks folder, or the one `core.hooksPath` names) as executable files.
fn commit_hooks(dir: &Path) -> Vec<&'static str> {
    use std::os::unix::fs::PermissionsExt;
    let mut args = vec!["rev-parse"];
    let paths = COMMIT_HOOKS.map(|hook| format!("hooks/{hook}"));
    for path in &paths {
        args.extend(["--git-path", path]);
    }
    let found = run_git(dir, &args).unwrap_or_default();
    COMMIT_HOOKS
        .into_iter()
        .zip(found.lines())
        .filter(|(_, path)| {
            fs::metadata(dir.join(path))
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
        .map(|(hook, _)| hook)
        .collect()
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
    /// The default branch moved to `top`, whose first commit made here is `merge`.
    Moved {
        merge: String,
        top: String,
        /// The generated paths the merge took from the default branch.
        taken: Vec<String>,
    },
    /// The default branch moved under it, to this commit: nothing changed.
    Raced(String),
}

/// What a land says of the generated paths `taken` its merge took from the default branch
/// `default`, inside the `generators`' outputs, which the regenerator makes: after the merge's
/// name on the thread, nothing when there are none. A generator's outputs are named whole, the
/// paths themselves when there are few.
fn taken_said(taken: &[String], default: &str, generators: &[Generator]) -> String {
    if taken.is_empty() {
        return String::new();
    }
    format!(
        ", its generated paths ({}) taken from {default}, for the regenerator to make",
        generated_named(taken, generators)
    )
}

/// The generated paths `taken` as a list, as a conflict names its files: each, when there are few,
/// else the `generators`' outputs that hold them.
fn generated_named(taken: &[String], generators: &[Generator]) -> String {
    let named: Vec<&str> = if taken.len() <= NAMED_PATHS {
        taken.iter().map(String::as_str).collect()
    } else {
        let mut outputs: Vec<&str> = Vec::new();
        for path in taken {
            let output = generators
                .iter()
                .find(|g| owns(&g.outputs, path))
                .map_or(path.as_str(), |g| g.outputs.as_str());
            if !outputs.contains(&output) {
                outputs.push(output);
            }
        }
        outputs
    };
    named.join(", ")
}

/// How many generated paths a land names one by one before it names the outputs that hold them.
const NAMED_PATHS: usize = 3;

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

impl Pushed {
    /// What a landed line says of its push, after what landed: where it went, why none was made,
    /// or why it failed and the command that pushes by hand, which ends the line.
    fn said(&self) -> String {
        match self {
            Pushed::Done => format!("; pushed to {REMOTE}"),
            Pushed::Off => "; not pushed: [land] push is off".into(),
            Pushed::NoRemote => format!("; not pushed: no {REMOTE} remote"),
            Pushed::Failed(why, retry) => {
                format!("; NOT PUSHED, landed here only: {why}. Push it with: {retry}")
            }
        }
    }
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
    now: &(dyn Fn() -> Instant + Sync),
    wait: impl FnOnce() -> T,
) -> (T, Ended) {
    use std::sync::mpsc::{RecvTimeoutError, channel};
    let (waited, ended) = std::thread::scope(|scope| {
        let (done, finished) = channel::<()>();
        let watchdog = scope.spawn(move || {
            let deadline = now() + limit;
            let ended = loop {
                let left = deadline.saturating_duration_since(now());
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
        #[expect(
            clippy::disallowed_methods,
            reason = "no-timed-waits baseline: looks again for a process group's end, which no wait reports"
        )]
        std::thread::sleep(POLL);
    }
    if sys::group_alive(group) {
        sys::end_group(group, true);
    }
}

/// The pid a land's worktree lock reason names (`symphony land: SLUG (pid N, started S)`), and
/// when that process started ([`started`]): none when the land could not read it, or the reason is
/// an earlier land's (`(pid N)`), which wrote no start. The sweep reads a land's lock by it too.
pub(crate) fn locked_by(reason: &str) -> Option<(i64, Option<String>)> {
    let (_, held) = reason.rsplit_once("(pid ")?;
    let held = held.strip_suffix(')')?;
    match held.split_once(", started ") {
        Some((pid, start)) => Some((
            pid.parse().ok()?,
            (!start.is_empty() && start != "-").then(|| start.to_string()),
        )),
        None => Some((held.parse().ok()?, None)),
    }
}

/// The reason the worktree of `slug`'s land is locked with, naming this process and when it
/// started, so a later land tells this one from a process that took its number after it.
fn lock_reason(slug: &str) -> String {
    let pid = std::process::id();
    let start = started(i64::from(pid));
    format!(
        "{REASON}: {slug} (pid {pid}, started {})",
        start.as_deref().unwrap_or("-")
    )
}

/// Whether the land a worktree's lock names (`pid`, started at `start`) still runs: its pid names
/// the process it started as, when its start was recorded, else merely a live process.
fn lands_still(pid: i64, start: Option<&str>) -> bool {
    match start {
        Some(start) => started(pid).as_deref() == Some(start),
        None => sys::pid_alive(pid),
    }
}

/// The paths `dir`'s git answers `args` with, NUL-separated (`-z`): each whole, a space at an end
/// kept.
fn git_z(dir: &Path, args: &[&str]) -> Result<Vec<String>, String> {
    let done = git(dir, args).ok_or_else(|| format!("git cannot run in {}", dir.display()))?;
    if !done.status.success() {
        let said = String::from_utf8_lossy(&done.stderr);
        let said = said
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("failed");
        return Err(format!("git {}: {}", args.join(" "), said.trim()));
    }
    Ok(done
        .stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect())
}

/// How many paths a git command run on a list of them is given at once, well inside any limit on
/// a command line.
const PATHS_AT_ONCE: usize = 200;

/// `git ARGS -- PATHS` in `dir`, the paths taken literally (`--literal-pathspecs`), a share of them
/// at a time.
fn on_paths(dir: &Path, args: &[&str], paths: &[String]) -> Result<(), String> {
    for share in paths.chunks(PATHS_AT_ONCE) {
        let mut all = vec!["--literal-pathspecs"];
        all.extend_from_slice(args);
        all.push("--");
        all.extend(share.iter().map(String::as_str));
        run_git(dir, &all)?;
    }
    Ok(())
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
    /// The file naming the group it runs while it runs ([`RUNNING`] in the lands' folder, for a
    /// land and for the regenerator's move of the default branch).
    running: PathBuf,
    /// Whether `slug` is a task: its thread hears the land, and what the land runs is recorded
    /// for it. The regenerator's is none.
    task: bool,
    /// The signals the land takes, when it holds the process's one scope.
    interrupts: Option<&'a sys::Interrupts>,
    /// The monotonic clock the command timeouts read.
    now: &'a (dyn Fn() -> Instant + Sync),
    /// The `tasks` running the land, which records a review command it starts.
    recorder: Option<&'a Path>,
    /// Whether the task's approve names the tip it lands, so a conflict in documents alone is
    /// its executor's to settle by one merge the approve stands over ([`carried`]).
    named: bool,
    /// What answers a delivery's verbs in place of the command `[land] delivery` names
    /// ([`delivery::Transport`]): a test's.
    transport: Option<&'a dyn delivery::Transport>,
    /// `[land] delivery` ([`delivery::command`]): empty where the land merges here and pushes.
    delivery: Vec<String>,
    /// What it is: a land; a dispatch ([`dispatch`]), which runs detached with its lines in its
    /// log alone, so what stops it is said on the task's thread as well; or a watcher's round,
    /// which says a refusal on the thread once at a head ([`Landing::refuse`]).
    mode: Mode,
    out: &'a mut dyn Write,
}

impl Landing<'_> {
    /// The task's `tip` merged onto the default branch from `base` and the branch moved there,
    /// tried again from each commit a race moved the branch to, [`TRIES`] times: the merge, the
    /// branch's new top and the generated paths the merge took, `base` left at the commit the
    /// last try merged from; else the refusal of a branch that moved under every try.
    fn merge_until_moved(
        &mut self,
        base: &mut String,
        tip: &str,
    ) -> Result<(String, String, Vec<String>), Stop> {
        let (slug, default) = (self.slug, self.settings.default_branch.clone());
        for attempt in 0..TRIES {
            self.go_on(None).map_err(Stop::Refused)?;
            match self
                .integrate(base, &Merging::Tip(tip), attempt > 0)
                .map_err(|refusal| self.refuse(refusal))?
            {
                Attempt::Moved { merge, top, taken } => return Ok((merge, top, taken)),
                Attempt::Raced(now) => {
                    // The last try's move is told by the refusal below.
                    if attempt + 1 < TRIES {
                        self.say(&format!(
                            "land {slug}: {default} moved to {}; merging and checking again",
                            short(&now)
                        ));
                    }
                    *base = now;
                }
            }
        }
        let line = format!(
            "{default} moved {TRIES} times while {slug} was merged; nothing landed: run the land \
             again"
        );
        Err(self.refuse(Refusal::here(line)))
    }

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

    /// The group `group` runs `command` for this land, named in [`RUNNING`] with when its first
    /// process started until it is gone, so a land killed outright leaves the next one its name; a
    /// group the next land `waits` for, never ends.
    fn running(&self, group: u32, command: &str, waits: bool) {
        let named = Named {
            group,
            start: started(i64::from(group)),
            waits,
            command: command.into(),
        };
        let _ = fs::write(&self.running, named.line());
    }

    /// The group named in [`RUNNING`] is gone.
    fn ran(&self) {
        let _ = fs::remove_file(&self.running);
    }

    /// The group a land killed outright left named in [`RUNNING`], while any process of it lives: the
    /// line refusing to land beside it. Only while the group's first process is the one the land
    /// started, by its start, is the group proven the land's, and only then does the line say how
    /// to end it: no pid is reused while a group of that number lives, so a first process of
    /// another start means the land's group is gone, and the number another's. With the first
    /// process gone, or its start never read, the processes in the group may be anyone's, and the
    /// line says how to look. One that ended is forgotten.
    fn leftover_group(&self) -> Result<(), String> {
        let path = self.running.clone();
        let Some(named) = fs::read_to_string(&path)
            .ok()
            .and_then(|text| Named::parse(&text))
        else {
            self.ran();
            return Ok(());
        };
        let Named {
            group,
            start,
            waits,
            command,
        } = named;
        if !sys::group_alive(group) {
            self.ran();
            return Ok(());
        }
        match (start, started(i64::from(group))) {
            (Some(start), Some(now)) if start == now && waits => Err(format!(
                "the process group {group} an earlier land started (`{command}`) still moves the \
                 main checkout: wait for it to end, then run the land again, which restores what \
                 it half-wrote should it end without moving it (were it to hang, `kill -TERM \
                 -{group}` ends it)"
            )),
            (Some(start), Some(now)) if start == now => Err(format!(
                "the process group {group} an earlier land started (`{command}`) outlived it and \
                 still runs: end it (`kill -TERM -{group}`), then run the land again"
            )),
            (Some(_), Some(_)) => {
                self.ran();
                Ok(())
            }
            _ => Err(format!(
                "the process group {group} an earlier land started (`{command}`) may still run, \
                 but its first process is gone or was never dated, so nothing proves the \
                 processes in group {group} now are the land's: `ps -A -o pid,pgid,lstart,command` \
                 lists them with their group and start; end those that are the land's, then \
                 remove {} and run the land again",
                path.display()
            )),
        }
    }

    fn branch(&self) -> String {
        format!("refs/heads/{}", self.settings.default_branch)
    }

    /// Say `refusal` where it goes, the task sent back to review first when its approve does not
    /// do; the stop it makes.
    fn refuse(&self, refusal: Refusal) -> Stop {
        if !refusal.thread || !self.task {
            if matches!(self.mode, Mode::Dispatch) && self.task {
                // Nobody reads a dispatch's log: a task its dispatch left undelivered says so.
                let slug = self.slug;
                self.post(
                    &format!(
                        "land {slug}: its dispatch stopped: {}; `just land {slug} --dispatch` \
                         dispatches it again, as a later write to it does",
                        refusal.line
                    ),
                    Some("@orchestrator"),
                );
            }
            return Stop::Refused(refusal.line);
        }
        // A watcher round stops where the last one did until what stopped it changes: what it
        // refuses is said once at the branch's head, whoever said it first (a land, a dispatch).
        // Kept only where a round can read it, a land delivered by a pull request, whose end
        // forgets it: a land that merges here keeps none, as nothing there would remove it.
        if self.delivers() {
            let tip = rev(&self.main, &format!("refs/heads/{}", self.slug)).unwrap_or_default();
            let new = dispatch::said(&self.folder, self.slug, &format!("{tip} {}", refusal.line));
            if !new && matches!(self.mode, Mode::Watch(_)) {
                return Stop::Refused(refusal.line);
            }
        }
        let slug = self.slug;
        let mut said = format!("land refused: {}", refusal.line);
        if refusal.reopen {
            match self.store.move_to(slug, "review", self.actor) {
                Ok(task) => {
                    said.push_str(&format!("; {slug} is back in review"));
                    said.push_str(&self.verifying(&task));
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

    /// What a move of `task` back to review by a land starts, as one on the command line does:
    /// the project's `on_review`. What its line adds, nothing where none starts.
    fn verifying(&self, task: &Task) -> String {
        if !on_review::delivery_task(task.parent.as_deref(), task.instruction) {
            return String::new();
        }
        match on_review::trigger(self.env, &self.main, self.slug, self.recorder) {
            Started::Command(args) => format!(", its verify started ({})", args.join(" ")),
            Started::Refused(why) => format!(", its verify did not start ({why})"),
            Started::None => String::new(),
        }
    }

    fn post(&self, text: &str, to: Option<&str>) {
        if self.task {
            let _ = self.store.say(self.slug, self.actor, text, to);
        }
    }

    fn land(mut self) -> Result<String, Stop> {
        let slug = self.slug;
        self.leftover_group().map_err(Stop::Refused)?;
        self.recover().map_err(Stop::Refused)?;
        // A task a custom workflow holds lands by its graph alone ([`adapter`]).
        if let Some(position) = self.held()? {
            return self.land_held(&position);
        }
        if self.delivers() {
            // A merge asked of the host by a run before decides, before anything is read again.
            if let Some(done) = self.resumed() {
                return done;
            }
            // A slug is delivered once: one with a receipt of another delivery is refused here.
            self.repeated()?;
            // A pull request a run before bound, read before anything is judged: one merged or
            // closed since is settled, and one whose evidence no longer stands is told so.
            if let Some(done) = self.bound_before() {
                return done;
            }
            self.delivered_before();
            self.unchecked().map_err(|refusal| self.refuse(refusal))?;
        } else if self.task {
            delivery::left(&self.folder, slug, &self.settings.default_branch)
                .map_err(Stop::Refused)?;
        }
        let task = self.store.get(slug)?;
        let thread = self.store.thread(slug)?;
        let branch_tip = rev(&self.main, &format!("refs/heads/{slug}"));
        // Which stamps: the assessment's on the tip it read, else beside the catalogue's defaults;
        // with no assessment, the defaults, which are `review` alone in the Symphony's catalogue.
        let mut needed = Needed::of(&task, &self.main, branch_tip.as_deref())
            .map_err(|line| self.refuse(Refusal::here(line)))?;
        let staff = |line: String| {
            Refusal::reopening(
                line,
                "@orchestrator",
                "Staff a fresh reviewer, which approves naming the tip it read".into(),
            )
        };
        // Ready: where `review` is needed, done by an approve the land goes by, the approval check
        // as before stamps; where it is not, in review or blocked, or done by an approve.
        let approved = if needed.review() {
            let approval = approval(&task, &thread).map_err(|line| {
                let stale = (branch_tip.as_deref()).and_then(|tip| needed.stale(tip));
                self.refuse(match (&needed.needs, stale) {
                    (None, _) => Refusal::thread(line),
                    (Some(_), None) => Refusal::to(
                        line,
                        "@orchestrator",
                        "Staff a fresh reviewer, which approves naming the tip it read".into(),
                    ),
                    (Some(_), Some(stale)) => Refusal::to(
                        format!("{line}; {stale}"),
                        "@orchestrator",
                        "Start an assessor for the tip, or staff a fresh reviewer, which approves \
                         naming it"
                            .into(),
                    ),
                })
            })?;
            let held = self.store.ever_held(slug)?;
            independent(&task, &approval, &held).map_err(|line| self.refuse(staff(line)))?;
            Some(approval)
        } else {
            if let Some((line, to)) = needs::not_ready(slug, &task.status) {
                return Err(self.refuse(match to {
                    Some(to) => Refusal::to(
                        line,
                        to,
                        "Move it to review once the work is ready, then land it".into(),
                    ),
                    None => Refusal::thread(line),
                }));
            }
            if task.status == "done" {
                approval(&task, &thread).map_err(|line| self.refuse(Refusal::thread(line)))?;
            }
            None
        };
        let tip = branch_tip.clone().ok_or_else(|| {
            let named = approved.as_ref().map_or("", |approval| &approval.text);
            let line = no_branch(&self.main, slug, &self.settings.default_branch, named);
            self.refuse(Refusal::thread(line))
        })?;
        self.clean().map_err(|line| self.refuse(line.into()))?;
        let mut base = self.base().map_err(|line| self.refuse(line.into()))?;
        let default = self.settings.default_branch.clone();
        if ancestor(&self.main, &tip, &base) {
            return self.finish(&tip, &base, approved.as_ref(), &needed);
        }
        self.owner_lines(&base, &tip)
            .map_err(|refusal| self.refuse(refusal))?;
        let raised = self.raised(&base, &tip, &needed)?;
        let judged = match &approved {
            Some(approval) => {
                let covered = covered(&self.main, slug, &base, &tip, approval).map_err(|line| {
                    self.refuse(Refusal::reopening(
                        line,
                        "@reviewer",
                        format!(
                            "Read what the approve does not cover, then approve the tip naming \
                             it: `just tasks verdict {slug} approve \"{tip}: …\"`"
                        ),
                    ))
                })?;
                Judged::Approved(covered)
            }
            None => Judged::Stamped(needed.stood(&tip)),
        };
        self.named = matches!(judged, Judged::Approved(Covered::Named));
        // An approve standing over a merge it did not read says the change was read; that it
        // works as merged is the tests' to say, read at the tip whatever the task's needs name.
        if let Judged::Approved(Covered::Merged(_)) = &judged {
            needed.require(needs::TESTS);
        }
        // Standing on the tip: each stamp but `review` by its reader.
        let evidence =
            needs::Evidence::of(self.store, self.env, &self.main, slug, &thread, &needed);
        let (met, missing) = needs::standing(&needed, &tip, &evidence);
        if !missing.is_empty() {
            return Err(self.lacks(&task, &tip, &missing));
        }
        if let (Some(approval), Judged::Approved(covered)) = (&approved, &judged) {
            self.say(&format!(
                "land {slug}: approved by {} at {}, {}",
                approval.by,
                sys::local_minute(approval.at),
                match covered {
                    Covered::Named => format!("naming its tip {}", short(&tip)),
                    Covered::ByTime(_) => {
                        format!("before every commit to its tip {}", short(&tip))
                    }
                    Covered::Merged(carried) => carried.said(&default),
                }
            ));
        }
        if needed.told() {
            self.say(&needed.line(slug, &tip, &met));
        }
        self.say_raised(&raised);
        if self.delivers() {
            return self.deliver(&task, &tip, &judged);
        }
        let (merge, top, taken) = self.merge_until_moved(&mut base, &tip)?;
        self.owner_lines_after(&base, &tip);
        self.close(&merge, &top, &taken, &judged, false)
    }

    /// The refusal of a land whose task lacks the stamps `missing` at `tip` ([`needs::refusal`]),
    /// said on the thread to its executor when a stamp of its is missing. One waiting on the owner
    /// alone is said to its orchestrator, and waits: held blocked from review
    /// ([`Store::hold_blocked_on`], which keeps its slot, the block naming the owner's stamp,
    /// `check:owner`, which the line reads as his decision, [`crate::waits_on::OWNER_APPROVE`]),
    /// blocked or done as it is (a task its approve left done stays done, the
    /// approve standing), the owner giving the stamp on its page.
    fn lacks(&self, task: &Task, tip: &str, missing: &[needs::Missing]) -> Stop {
        let slug = self.slug;
        let (line, to) = needs::refusal(slug, tip, missing);
        let at = short(tip);
        let then = if !needs::owner_alone(missing) {
            format!(
                "Give them at {at}, then land it again: a commit after it goes back to review, \
                 where what it needs is read again"
            )
        } else {
            let waits = format!(
                "waiting on the owner's approve of {at}, which the owner gives on the task's page \
                 (its Approve {at}); land it again once it is given"
            );
            // The hold names what it waits on: the owner's approve stamp of the tip.
            let thing = format!("{}:{}", crate::waits_on::CHECK, needs::OWNER);
            let why = format!("the owner's approve of {at}");
            let on = crate::On {
                thing: &thing,
                why: Some(&why),
            };
            match task.status.as_str() {
                "review" => match self.store.hold_blocked_on(slug, self.actor, Some(on)) {
                    Ok(_) => format!("{slug} is held blocked, {waits}"),
                    Err(error) => format!(
                        "It stays in review, as the land could not hold it blocked ({error}), \
                         {waits}"
                    ),
                },
                "done" => format!("{slug} stays done, its approve standing, {waits}"),
                status => format!("{slug} stays {status}, {waits}"),
            }
        };
        self.refuse(Refusal::to(line, to, then))
    }

    /// The land of a tip the default branch holds already: only one this command started and
    /// interrupted after it moved the default branch (killed while it pushed, say), by the
    /// [`Record`] it kept of that merge, whose tip is the branch's still, whose merge brought that
    /// tip, and whose checked commit the default branch holds. Then the approve still stands, so
    /// no landing closed the task, and what is left is done once the approve covers the tip: the
    /// push of the checked commit, never of a newer tip no check passed on, the landed line and the
    /// sweep. A tip merged by hand, or by a land that did not record it, is refused: nothing is
    /// pushed, closed or said on the thread.
    /// A task that needs no review (`approval` none) is finished by its record alone, its landed
    /// line saying the stamps it needs (`needed`) stood at the tip.
    fn finish(
        &mut self,
        tip: &str,
        base: &str,
        approval: Option<&Approval>,
        needed: &Needed,
    ) -> Result<String, Stop> {
        let (slug, default) = (self.slug, self.settings.default_branch.clone());
        let record = Record::read(&self.folder, slug).filter(|record| {
            record.tip == tip
                && rev(&self.main, &format!("{}^2", record.merge)).as_deref() == Some(tip)
                && ancestor(&self.main, &record.merge, &record.checked)
                && ancestor(&self.main, &record.checked, base)
        });
        let Some(record) = record else {
            let merge = self.merged_by(tip, base).unwrap_or_else(|| tip.to_string());
            return Err(self.refuse(Refusal::here(format!(
                "{slug}'s tip {} is on {default} already, merged outside land: nothing to finish. \
                 Were it a land's merge whose record ({}) was lost, finish it by hand: push \
                 {default} once a check passed on what it holds (`git push {REMOTE} {default}`), \
                 then say the landing on the thread (`just tasks say {slug} \"landed {slug} on \
                 {default} at {}\"`) and sweep its worktrees (`just tasks sweep`)",
                short(tip),
                Record::path(&self.folder, slug).display(),
                &merge[..merge.len().min(12)]
            ))));
        };
        let before =
            rev(&self.main, &format!("{}^1", record.merge)).unwrap_or_else(|| tip.to_string());
        let judged = match approval {
            None => Judged::Stamped(needed.stood(tip)),
            Some(approval) => Judged::Approved(
                covered(&self.main, slug, &before, tip, approval).map_err(|line| {
                    self.refuse(Refusal::to(
                        format!(
                            "{slug}'s tip {} is on {default} already, by {}, but {line}",
                            short(tip),
                            short(&record.merge)
                        ),
                        "@orchestrator",
                        "Nothing is pushed or closed: what landed is not what was approved".into(),
                    ))
                })?,
            ),
        };
        self.say(&format!(
            "land {slug}: its tip {} is on {default} already, by this land's merge {}{}: \
             finishing the land (the push, the landed line, the sweep)",
            short(tip),
            short(&record.merge),
            self.checked_at(short(&record.checked))
        ));
        self.close(
            &record.merge,
            &record.checked,
            &record.regenerated,
            &judged,
            true,
        )
    }

    /// What the landed line says of the tokens the task and its subtasks took, as their beats
    /// recorded them ([`crate::usage::said`]); nothing when none were, or the board cannot say.
    fn tokens_said(&self) -> String {
        let slug = self.slug;
        let Ok(children) = self.store.children(slug) else {
            return String::new();
        };
        let slugs: Vec<&str> = std::iter::once(slug)
            .chain(children.iter().map(|child| child.slug.as_str()))
            .collect();
        (self.store.spent_on(&slugs).ok())
            .and_then(|rows| crate::usage::said(&rows))
            .map_or_else(String::new, |said| format!("; {said}"))
    }

    /// The landing's end once `merge` is on the default branch, at `top` here: the push, the
    /// landed line closing the task, the sweep and the regenerator's start; the line saying where
    /// it landed. `taken` are the generated paths the merge took from the default branch; `judged`
    /// how the task was found ready, which says what statuses its landing closes.
    fn close(
        &mut self,
        merge: &str,
        top: &str,
        taken: &[String],
        judged: &Judged,
        finishing: bool,
    ) -> Result<String, Stop> {
        let (slug, default) = (self.slug, self.settings.default_branch.clone());
        let mut landed = format!(
            "landed {slug} on {default} at {}",
            &merge[..merge.len().min(12)]
        );
        if let Judged::Stamped(stood) = judged {
            landed.push_str(stood);
        }
        if finishing {
            landed.push_str(", finishing a land interrupted after its merge");
        }
        landed.push_str(&taken_said(taken, &default, &self.settings.generators));
        landed.push_str(&self.tokens_said());
        // A signal from here on stops the land with its record kept, which a run again finishes.
        self.go_on(Some(merge)).map_err(Stop::Refused)?;
        let pushed = self.push(top).said();
        self.go_on(Some(merge)).map_err(Stop::Refused)?;
        let line = format!("{landed}{pushed}");
        // The thread also says how the approval was judged, before the push, whose retry command
        // ends the line.
        let said = match judged {
            Judged::Approved(Covered::ByTime(n)) => format!(
                "{landed}; judged by time, the approval naming no commit of the branch: its {} {} \
                 older than the approval{pushed}",
                commits(*n),
                if *n == 1 { "is" } else { "are" }
            ),
            Judged::Approved(Covered::Merged(carried)) => format!(
                "{landed}; its approve of {} stands over the one merge {} of {default} into \
                 it{pushed}",
                short(&carried.approved),
                short(&carried.merge)
            ),
            _ => line.clone(),
        };
        // The approval on the board is the gate here, not who runs the land: any session may. A
        // task sent back to review since it was read stays there: what it has now did not land.
        // One that needs no review closes from review or blocked, its executor's move to review
        // having said the work is ready, or done by an approve.
        let from: &[&str] = match judged {
            Judged::Approved(_) => &["done"],
            Judged::Stamped(_) => &["review", "blocked", "done"],
        };
        if let Err(error) = self.store.landed(slug, self.actor, &said, true, from) {
            return Err(Stop::Refused(format!(
                "{line}, but the board did not close {slug}: {error}"
            )));
        }
        // Closed: nothing is left to finish.
        Record::drop_in(&self.folder, slug);
        self.after(merge, true);
        Ok(line)
    }

    /// What follows a land of the task at `merge`, each started detached and waited for by
    /// nothing: the sweep of its finished worktrees, the regenerator where `regenerates`, and the
    /// project's `[board] on_land`.
    fn after(&mut self, merge: &str, regenerates: bool) {
        let slug = self.slug;
        // The task's finished worktrees go by a sweep started detached, as work finishing starts
        // one ([`trigger`]): it may walk and remove whole trees, which neither the land nor its
        // lock waits for.
        if trigger::trigger(
            self.env,
            &self.main,
            &format!("the land of {slug}"),
            Some(slug),
        ) {
            self.say(&format!(
                "land {slug}: its finished worktrees are swept in the background (its log \
                 {}/{})",
                crate::server::FOLDER,
                trigger::LOG
            ));
        }
        // What the generators make of the default branch is the regenerator's, which nothing here
        // waits for.
        if regenerates && let Some(said) = regenerate::start(self.env, &self.main, &self.settings) {
            self.say(&format!("land {slug}: {said}"));
        }
        // The project's `[board] on_land`, detached, and what it said at once: the release
        // cadence, here (docs/RELEASES.md). It starts after the regenerator, so a run it starts
        // finds that one started (`symphony-build release` waits for it).
        if let Ok(common) = crate::git::common_dir(&self.main) {
            match crate::on_land::trigger(self.env, &self.main, &common, slug, merge) {
                (Started::Command(args), heard) => {
                    self.say(&format!(
                        "on_land: started {} (its lines go to {})",
                        args.join(" "),
                        heard.output.display()
                    ));
                    for line in &heard.lines {
                        self.say(&format!("  {line}"));
                    }
                    if !heard.ended {
                        self.say(&format!(
                            "  … it runs on past {} s, detached",
                            crate::on_land::says(self.env).as_secs()
                        ));
                    }
                }
                (Started::Refused(why), _) => self.say(&format!("⚠ on_land: {why}")),
                (Started::None, _) => {}
            }
        }
    }

    /// The first merge on the default branch at `base` that brought `tip` (its second parent), if
    /// one did: what a land of it would have landed at.
    fn merged_by(&self, tip: &str, base: &str) -> Option<String> {
        let range = format!("{tip}..{base}");
        let listed = run_git(
            &self.main,
            &[
                "rev-list",
                "--merges",
                "--ancestry-path",
                "--parents",
                "--reverse",
                &range,
            ],
        )
        .ok()?;
        listed.lines().find_map(|line| {
            let commits: Vec<&str> = line.split(' ').collect();
            (commits.get(2) == Some(&tip)).then(|| commits[0].to_string())
        })
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
    /// of one a land left behind once the land its lock names has ended (its pid no longer the
    /// process of the start the lock names: [`lands_still`]) and no process works in it (`lsof`, as
    /// the sweep's guard asks; one that cannot say keeps it); one in its place that is not a land's
    /// is never touched.
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
                let ours = i64::from(std::process::id());
                if let Some((pid, _)) = locked_by(reason)
                    .filter(|(pid, start)| *pid != ours && lands_still(*pid, start.as_deref()))
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
                                 an earlier land left: once nothing does (`lsof +D {shown}` lists \
                                 each process with a file or its folder there, and none once \
                                 nothing works there), `git worktree remove --force --force \
                                 {shown}`, then run the land again"
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
        let reason = lock_reason(self.slug);
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
        self.logged_reading(command, dir, false).map(|(ran, _)| ran)
    }

    /// [`Landing::logged`], what `command` prints on its standard output read when `read`, else
    /// in the log with the rest.
    fn logged_reading(
        &self,
        command: &[String],
        dir: &Path,
        read: bool,
    ) -> Result<(Ran, String), String> {
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
        // No mark it was asked for by goes with it (`sys::MARKS`): a check or a generator is the
        // project's own command, which writes where the project says and lands nothing.
        let mut child = sys::command_with(&program, &self.env.vars);
        for name in GIT_VARS {
            child.env_remove(name);
        }
        // What the land runs is the task's, recorded for it and the session landing it, in a group
        // of its own, which its timeout ends whole. The regenerator's is no task's or session's.
        if self.task {
            child
                .env("TASKS_SESSION", self.actor)
                .env("TASKS_TASK", self.slug);
        } else {
            sys::sessionless(&mut child).env_remove("TASKS_TASK");
        }
        let stdout = if read {
            Stdio::piped()
        } else {
            log.try_clone().map_err(|e| at(&self.log, &e))?.into()
        };
        child
            .args(args)
            .current_dir(dir)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(log.try_clone().map_err(|e| at(&self.log, &e))?);
        if let Some(signal) = self.interrupted() {
            return Ok((Ran::Interrupted(signal), String::new()));
        }
        let mut child = child
            .spawn()
            .map_err(|e| format!("{}: {e}", command.join(" ")))?;
        let group = child.id();
        self.running(group, &command.join(" "), false);
        let limit = self.settings.timeout;
        let mut printed = Vec::new();
        let (status, ended) = watched(group, limit, self.interrupts, self.now, || {
            if let Some(mut stdout) = child.stdout.take() {
                let _ = stdout.read_to_end(&mut printed);
            }
            child.wait()
        });
        self.ran();
        let status = status.map_err(|e| format!("{}: {e}", command.join(" ")))?;
        let printed = String::from_utf8_lossy(&printed).into_owned();
        if read {
            let _ = write!(log, "{printed}");
        }
        let _ = writeln!(log, "exit {}", status.code().unwrap_or(-1));
        let ran = match ended {
            Ended::Late => {
                let _ = writeln!(log, "ended: it ran past the land's {} s", limit.as_secs());
                Ran::Late
            }
            Ended::Interrupted(signal) => {
                let _ = writeln!(log, "ended: the land took {}", signal_name(signal));
                Ran::Interrupted(signal)
            }
            Ended::No if status.success() => Ran::Passed,
            Ended::No => Ran::Failed,
        };
        Ok((ran, printed))
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

    /// Merge `merging` into `base` in the land's worktree, keeping the default branch's side of the
    /// generated paths a task's tip changed, and check, then move the default branch there when it
    /// is still at `base`. `again` is a run after it moved. Its merge of a task's tip
    /// ([`Merging::Tip`]) is the one place a task's branch is merged: the seam a hosted
    /// pull-request merge would replace, the approval before it and the record, move, push and
    /// close after it staying as they are.
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
        let merged = git(
            &path,
            &["merge", "--no-ff", "--no-commit", "--no-edit", what],
        )
        .is_some_and(|done| done.status.success());
        let files =
            git_z(&path, &["diff", "--name-only", "-z", "--diff-filter=U"]).unwrap_or_default();
        let generated = |file: &String| {
            self.settings
                .generators
                .iter()
                .any(|g| owns(&g.outputs, file))
        };
        // Conflicts in the generators' outputs alone are theirs to settle; any other refuses, and
        // so does a merge git would not begin.
        let begun = succeeds(&path, &["rev-parse", "--verify", "--quiet", "MERGE_HEAD"]);
        if !begun || (!merged && (files.is_empty() || !files.iter().all(generated))) {
            let _ = git(&path, &["merge", "--abort"]);
            return Err(self.unmerged(merging, what, base, &files));
        }
        // What the generators make is the regenerator's, from the default branch's own sources:
        // the merge of a task's tip keeps the default branch's side of each generated path, in a
        // conflict or not, so no branch's own build of them lands. What origin gained there is
        // the default branch's own (a regenerator's commit pushed elsewhere), kept but where it
        // conflicts.
        let tip = matches!(merging, Merging::Tip(_));
        let mut taken = match if tip {
            self.generated_changes(&path)
        } else {
            Ok(Vec::new())
        } {
            Ok(taken) => taken,
            Err(why) => {
                let _ = git(&path, &["merge", "--abort"]);
                return Err(why.into());
            }
        };
        for file in &files {
            if !taken.contains(file) {
                taken.push(file.clone());
            }
        }
        taken.sort_unstable();
        if !taken.is_empty() {
            if let Err(why) = self.take_ours(&path, &taken) {
                let _ = git(&path, &["merge", "--abort"]);
                if files.is_empty() {
                    return Err(format!(
                        "the generated paths {slug} changed could not be taken from {default}: \
                         {why}"
                    )
                    .into());
                }
                self.say(&format!(
                    "land {slug}: the conflicts in generated paths could not be taken from \
                     {default}: {why}"
                ));
                return Err(self.unmerged(merging, what, base, &files));
            }
            self.say(&format!(
                "land {slug}: {} generated paths ({}): taking {default}'s side there, for the \
                 regenerator to make",
                match merging {
                    Merging::Origin(_) => format!("what {REMOTE}/{default} gained conflicts in"),
                    Merging::Tip(_) if files.is_empty() => "it changed its".into(),
                    Merging::Tip(_) => "it conflicts in its".into(),
                },
                generated_named(&taken, &self.settings.generators)
            ));
        }
        // The merge is committed through the project's git hooks, as git commits a merge concluded
        // after `--no-commit` (pre-commit, prepare-commit-msg, commit-msg; pre-merge-commit is `git
        // merge`'s own), run as the check is: its output in the log, within the land's timeout.
        let commit = ["git", "commit", "--quiet", "-m", &message].map(String::from);
        let ran = self.logged(&commit, &path)?;
        if let Ran::Interrupted(signal) = ran {
            return Err(Refusal::here(self.halted(signal, None)));
        }
        if ran != Ran::Passed {
            let hooks = commit_hooks(&path);
            let (by, did) = match hooks.as_slice() {
                [] => ("git commit".to_string(), "failed on"),
                [hook] => (format!("the project's git hook {hook}"), "refused"),
                hooks => (
                    format!("one of the project's git hooks ({})", hooks.join(", ")),
                    "refused",
                ),
            };
            let did = if ran == Ran::Late {
                format!("{} on", self.failed(ran))
            } else {
                did.to_string()
            };
            let into = match merging {
                Merging::Tip(_) => slug.to_string(),
                Merging::Origin(_) => format!("{REMOTE}/{default}"),
            };
            let line = format!(
                "{by} {did} the merge of {into} into {default} at {}; its output is in {}",
                short(base),
                self.log.display()
            );
            // A hook's refusal is the branch's to answer, as a red check is; git's own failure with
            // no hook (a signature, an identity, the disk) is no branch's, said to the lander alone.
            return Err(if hooks.is_empty() {
                Refusal::here(line)
            } else {
                self.red(merging, again, line)
            });
        }
        let merge = rev(&path, "HEAD").ok_or("the merge left no HEAD")?;
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
                return Err(self.red(merging, again, line));
            }
        }
        let top = rev(&path, "HEAD").ok_or("the check left no HEAD")?;
        let checked = rev(&path, "HEAD^{tree}").ok_or("the check left no tree")?;
        drop(tree);
        self.clean()?;
        self.go_on(None)?;
        let recorded = matches!(merging, Merging::Tip(_));
        if let Merging::Tip(tip) = merging {
            // Asked again right before the move: the owner may have answered while it merged
            // and checked, and once the default branch holds the tip, nothing is refused.
            self.owner_lines(base, tip)?;
            // Kept before the default branch moves, so a land stopped after it is one to finish,
            // and only this one.
            let record = Record {
                slug: slug.into(),
                tip: (*tip).into(),
                merge: merge.clone(),
                checked: top.clone(),
                regenerated: taken.clone(),
            };
            record.write(&self.folder)?;
            self.post(
                &format!(
                    "land {slug}: merged {tip} as {merge}{}{}; moving {default} there",
                    taken_said(&taken, &default, &self.settings.generators),
                    self.checked_at(&top)
                ),
                None,
            );
        }
        match self.fast_forward(base, &top, recorded)? {
            Move::Done => {}
            Move::Raced(now) => return Ok(Attempt::Raced(now)),
            Move::Not { why, restored } => {
                let line = moving::not_moved(&default, &top, base, &why, &restored, recorded);
                // The thread heard the move begin: it hears that it did not happen.
                return Err(if recorded {
                    Refusal::thread(line)
                } else {
                    line.into()
                });
            }
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
                // The branch moved all the same: only what a land stopped now pushes is behind.
                if let Err(why) = record.write(&self.folder) {
                    self.say(&format!(
                        "land {slug}: its record does not name {} as checked ({why}): a land \
                         stopped before its push and run again pushes the commit it named before",
                        short(&top)
                    ));
                }
            }
        }
        Ok(Attempt::Moved { merge, top, taken })
    }

    /// The refusal of a merge of `what` (`merging`) into `base` that did not go, `files` its
    /// conflicts: the executor's to fix on its branch, or the thread's for what `origin` gained.
    fn unmerged(&self, merging: &Merging, what: &str, base: &str, files: &[String]) -> Refusal {
        let (slug, default) = (self.slug, &self.settings.default_branch);
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
        match merging {
            Merging::Tip(_)
                if self.named
                    && !files.is_empty()
                    && files.iter().all(|f| carried::document(f)) =>
            {
                Refusal::to(line, "@executor", self.settle_on_branch())
            }
            Merging::Tip(_) => Refusal::to(line, "@executor", self.fix_on_branch()),
            Merging::Origin(_) => Refusal::thread(line),
        }
    }

    /// The refusal of a merge of `merging` that a check or a commit hook judged red, `again` once
    /// the default branch moved: then the orchestrator's to look at; on the merge of the tip as
    /// approved, the executor's to fix on its branch; on what `origin` gained, the thread's.
    fn red(&self, merging: &Merging, again: bool, line: String) -> Refusal {
        match merging {
            _ if again => Refusal {
                to: Some("@orchestrator"),
                ..Refusal::thread(line)
            },
            Merging::Tip(_) => Refusal::to(line, "@executor", self.fix_on_branch()),
            Merging::Origin(_) => Refusal::thread(line),
        }
    }

    /// What the thread says of the commit `top` the land moves the default branch to: that the
    /// check passed there, nothing when the land runs none.
    fn checked_at(&self, top: &str) -> String {
        if self.settings.check.is_empty() {
            String::new()
        } else {
            format!(", checked at {top}")
        }
    }

    /// The generated paths the merge begun in `path` changes or leaves in conflict against the
    /// default branch's side (the worktree's `HEAD`): every path inside a generator's outputs that
    /// the index holds otherwise than `HEAD` does.
    fn generated_changes(&self, path: &Path) -> Result<Vec<String>, String> {
        if self.settings.generators.is_empty() {
            return Ok(Vec::new());
        }
        let mut args = vec![
            "--literal-pathspecs",
            "diff",
            "--cached",
            "--name-only",
            "--no-renames",
            "-z",
            "HEAD",
            "--",
        ];
        args.extend(self.settings.generators.iter().map(|g| g.outputs.as_str()));
        let mut changed = git_z(path, &args)?;
        changed.retain(|file| {
            self.settings
                .generators
                .iter()
                .any(|g| owns(&g.outputs, file))
        });
        changed.dedup();
        Ok(changed)
    }

    /// Settle each of `files`, inside a generator's outputs, in the merge begun in `path` by the
    /// default branch's side (the worktree's `HEAD`): its file where it has one, none where it has
    /// none, a conflict there resolved so. The merge is committed after, both parents kept, so its
    /// second parent is still the tip merged.
    fn take_ours(&self, path: &Path, files: &[String]) -> Result<(), String> {
        let mut held = Vec::new();
        for share in files.chunks(PATHS_AT_ONCE) {
            let mut args = vec![
                "--literal-pathspecs",
                "ls-tree",
                "-r",
                "-z",
                "--name-only",
                "HEAD",
                "--",
            ];
            args.extend(share.iter().map(String::as_str));
            held.extend(git_z(path, &args)?);
        }
        let (ours, gone): (Vec<String>, Vec<String>) =
            files.iter().cloned().partition(|file| held.contains(file));
        if !ours.is_empty() {
            on_paths(path, &["checkout", "-q", "HEAD"], &ours)?;
        }
        if !gone.is_empty() {
            on_paths(path, &["rm", "-q", "-f", "--ignore-unmatch"], &gone)?;
        }
        let left = git_z(path, &["diff", "--name-only", "-z", "--diff-filter=U"])?;
        if let Some(file) = left.first() {
            return Err(format!("{file} is still unmerged"));
        }
        Ok(())
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

    /// `git ARGS` in the main checkout (a fetch, a push, the fast-forward), by the caller's
    /// environment (its `PATH`, its keys, the filters' programs) less git's repository variables
    /// and the marks the land was asked for by (`sys::MARKS`), never asking for a password, in a
    /// process group of its own, which its timeout ends whole and no terminal's signal reaches.
    fn main_git(&self, args: &[&str]) -> sys::Command {
        let mut command = sys::command_with("git", &self.env.vars);
        for name in GIT_VARS {
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

    /// What `git ARGS` with a remote ([`Landing::main_git`]) answered within the land's network
    /// timeout ([`Settings::network`]), or why there is no answer, which its caller says of the
    /// remote it reached: `origin`, or the project's upstream.
    fn remote_output(&self, args: &[&str]) -> Result<std::process::Output, String> {
        if let Some(signal) = self.interrupted() {
            return Err(format!("not run, the land took {}", signal_name(signal)));
        }
        let mut command = self.main_git(args);
        let child = sys::spawn_output(&mut command).map_err(|e| e.to_string())?;
        let group = child.id();
        self.running(group, &format!("git {}", args.join(" ")), false);
        let limit = self.settings.network();
        let (done, ended) = watched(group, limit, self.interrupts, self.now, || {
            sys::wait_output(child)
        });
        self.ran();
        match ended {
            Ended::Late => Err(format!("no answer within {} s", limit.as_secs())),
            Ended::Interrupted(signal) => {
                Err(format!("ended, the land took {}", signal_name(signal)))
            }
            Ended::No => done.map_err(|e| e.to_string()),
        }
    }

    /// Whether `origin` answers and has no default branch.
    fn lacks_branch(&self) -> bool {
        let branch = self.branch();
        // `--exit-code`: 2 is an answer listing no such branch.
        self.remote_output(&["ls-remote", "--exit-code", "--heads", REMOTE, &branch])
            .is_ok_and(|done| done.status.code() == Some(2))
    }

    /// The worktrees holding an owner's answer that a land of `tip` onto `base` would leave behind,
    /// each with its answers ([`answers`]): an owner's dated line under a card of the project's
    /// documents, saved into the worktree and in no commit of it, that the tip does not hold on
    /// that card either. A worktree counts when its HEAD is a commit the land brings (`git rev-list
    /// TIP ^BASE`): the task's branch's, and one on another branch or detached at such a commit, as
    /// a slice's or a reviewer's at the tip. The Symphony saves only into a worktree on a branch,
    /// but a line stays where it was saved when its worktree is detached after. Why a worktree or
    /// the commits could not be read, when they could not: the land cannot tell, so it refuses.
    fn answers_left(&self, base: &str, tip: &str) -> Result<Vec<(Listed, Vec<Answer>)>, String> {
        let slug = self.slug;
        let listed = worktrees(&self.main)?;
        let range = format!("^{base}");
        let brought = told(&self.main, &["rev-list", tip, &range])
            .map_err(|why| format!("the commits {slug} brings could not be listed: {why}"))?;
        let brought = String::from_utf8_lossy(&brought.stdout).into_owned();
        let brought: BTreeSet<&str> = brought.split_whitespace().collect();
        let mut left = Vec::new();
        for entry in listed {
            if entry.bare
                || entry.prunable
                || !entry.path.is_dir()
                || !brought.contains(entry.head.as_str())
            {
                continue;
            }
            let against = (entry.head != tip).then_some(tip);
            let found = answers(&entry.path, &self.settings.documents, against).map_err(|why| {
                format!(
                    "the worktree {} could not be read for the owner's answers no commit has: {why}",
                    entry.path.display()
                )
            })?;
            if !found.is_empty() {
                left.push((entry, found));
            }
        }
        Ok(left)
    }

    /// The worktrees [`Landing::answers_left`] found, as a refusal names them: each with the
    /// branch it is on when that is not the task's, and its files.
    fn holding(&self, held: &[(Listed, Vec<Answer>)]) -> String {
        let said: Vec<String> = held
            .iter()
            .map(|(entry, found)| {
                let mut files: Vec<&str> = Vec::new();
                for answer in found {
                    if !files.contains(&answer.file.as_str()) {
                        files.push(&answer.file);
                    }
                }
                let on = match entry
                    .branch
                    .as_deref()
                    .and_then(|b| b.strip_prefix("refs/heads/"))
                {
                    Some(branch) if branch == self.slug => String::new(),
                    Some(branch) => format!(" on {branch}"),
                    None => format!(" detached at {}", short(&entry.head)),
                };
                format!("{}{on} ({})", entry.path.display(), files.join(", "))
            })
            .collect();
        said.join(", ")
    }

    /// Refused while a worktree the land brings holds an owner's answer no commit has
    /// ([`Landing::answers_left`]): the owner's answer the Symphony saved into a card the branch
    /// posted, which a land of its commits would leave behind. Said to the executor, which commits
    /// it on the branch and has the new tip approved. A worktree git cannot read refuses too, to
    /// the one landing alone: no branch's commit fixes git, and a land that cannot tell must not
    /// leave the answer behind. Asked before the merge and again right before the default branch
    /// moves, so an answer saved while the land merged and checked is not left behind either.
    /// Each raise `needed`'s gate decisions gave, read at `tip` against the default branch at
    /// `base` ([`needs::raises`]): the lines the landing says, or its refusal, to the executor,
    /// where the tip sets a limit above the default branch's to a number no decision gave.
    fn raised(&self, base: &str, tip: &str, needed: &Needed) -> Result<Vec<String>, Stop> {
        needs::raises(&self.main, base, tip, &needed.decisions).map_err(|line| {
            self.refuse(Refusal::to(
                line,
                "@executor",
                "Commit the limit at the number decided, its message the account, or ask the \
                 task's orchestrator for a decision of the number it holds; then land it again"
                    .into(),
            ))
        })
    }

    /// The landing's line naming each of `raised` ([`Landing::raised`]), where there is one.
    fn say_raised(&mut self, raised: &[String]) {
        if !raised.is_empty() {
            let slug = self.slug;
            self.say(&format!(
                "land {slug}: its gate decisions: {}",
                raised.join("; ")
            ));
        }
    }

    fn owner_lines(&self, base: &str, tip: &str) -> Result<(), Refusal> {
        let slug = self.slug;
        let held = self.answers_left(base, tip).map_err(|why| {
            Refusal::here(format!(
                "{why}: nothing landed, as {slug} may hold the owner's answer there. Run the land \
                 again once git reads it"
            ))
        })?;
        if held.is_empty() {
            return Ok(());
        }
        let elsewhere = held
            .iter()
            .any(|(entry, _)| entry.branch.as_deref() != Some(&format!("refs/heads/{slug}")));
        Err(Refusal::to(
            format!(
                "{slug}'s worktree {} holds the owner's answer no commit has, which a land of its \
                 commits would leave behind",
                self.holding(&held)
            ),
            "@executor",
            if elsewhere {
                format!(
                    "Commit each answer on its worktree's own branch and card (from a detached \
                     worktree, copy it onto the same card on {slug} and commit it there). If \
                     {slug}'s tip changes, move it back to review (`just tasks move {slug} review`) \
                     and ask its reviewer to approve the new tip naming it"
                )
            } else {
                format!(
                    "Commit it on {slug} there, then move it back to review (`just tasks move \
                     {slug} review`) and ask its reviewer to approve the new tip naming it"
                )
            },
        ))
    }

    /// After the default branch moved: an owner's answer [`Landing::answers_left`] finds now was
    /// saved while the default branch moved, after the last look. The land cannot refuse any more,
    /// so it says it on the thread to the orchestrator, which puts it on the default branch's card:
    /// from then on the Symphony saves a line on a landed branch's page on the default branch, and
    /// the sweep keeps the worktree holding it.
    fn owner_lines_after(&mut self, base: &str, tip: &str) {
        let (slug, default) = (self.slug, &self.settings.default_branch);
        let said = match self.answers_left(base, tip) {
            Ok(held) if held.is_empty() => return,
            Ok(held) => {
                let cards: Vec<&str> = held
                    .iter()
                    .flat_map(|(_, found)| found.iter().map(|a| a.card.as_str()))
                    .collect();
                format!(
                    "land {slug}: {} holds the owner's answer on {} saved while {slug} landed, \
                     which {default} does not hold: copy it onto that card on {default} and \
                     commit it there; the sweep keeps that worktree meanwhile",
                    self.holding(&held),
                    cards.join(", ")
                )
            }
            Err(why) => format!(
                "land {slug}: after it landed, {why}: read it by hand for an owner's answer \
                 {default} lacks; the sweep keeps a worktree it cannot read"
            ),
        };
        self.say(&said);
        self.post(&said, Some("@orchestrator"));
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

    /// What the executor does when the branch, its approve naming its tip, conflicts with the
    /// default branch in documents alone ([`carried::document`]): the one merge an approve
    /// stands over ([`carried`]), with no reviewer.
    fn settle_on_branch(&self) -> String {
        let (slug, default) = (self.slug, &self.settings.default_branch);
        format!(
            "Merge {default} into {slug} once, settling the conflict in those documents and \
             changing nothing else, run its tests at the new tip through the project's recorded \
             recipes, and land it again: an approve naming the commit merged into stands over \
             that one merge, so {slug} stays as it is and no reviewer reads it again. A change \
             beyond those conflicts, or a move back to review, asks for a fresh approve of the \
             new tip"
        )
    }

    /// `git ARGS` with the remote ([`Landing::remote_output`]): the line saying why it failed.
    fn remote(&self, args: &[&str]) -> Result<(), String> {
        let failed = |why: &str| format!("git {} {REMOTE}: {why}", args[0]);
        let done = self.remote_output(args).map_err(|why| failed(&why))?;
        if done.status.success() {
            return Ok(());
        }
        let said = String::from_utf8_lossy(&done.stderr);
        let said = said
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("failed");
        Err(failed(said))
    }
}

#[cfg(test)]
mod tests;
