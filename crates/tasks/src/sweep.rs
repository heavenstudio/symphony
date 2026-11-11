//! The worktree sweep: remove the worktrees nobody needs any more, merged into main, holding nothing
//! that did not land, with no live owner: `just tasks sweep
//! [--dry-run] [--recent-hours H] [--task SLUG]...`, which `just worktree-sweep` runs.
//!
//! `just land` runs it after every landing and prints only its summary, and work finishing starts it
//! by itself ([`trigger`]). A worktree goes only when all of these hold:
//!
//! * it is not the main checkout, not the worktree running the sweep, not where `main` is checked
//!   out, and holds no other worktree inside it;
//! * nothing touched it lately: its admin dir's HEAD, index and logs/HEAD and its top folder are all
//!   older than the window (2 h; `--recent-hours` or `SF_SWEEP_RECENT_HOURS`). The worktrees of a
//!   task just done or cancelled whose sessions' panes are closed (`--task`) have no window;
//! * it has no lock, or a dead one: Claude Code's `claude agent agent-X (pid N start <date>)` whose
//!   pid is not running (the kernel says, `kill(pid, 0)`), or runs with another start time (a reused
//!   pid; `ps -p N`, in the C locale and in UTC as Claude Code writes it, says). A pid no process
//!   could have is a foreign lock. A lock in any other form, or a start `ps` cannot give or we cannot
//!   read, keeps it;
//! * no board session that is live or stale is bound to it: by `session.worktree`, or, for a session
//!   bound to Claude Code agent X (`session.agent`), as the worktree named `agent-X` (a subagent's
//!   session is registered at the main checkout, and a resumed agent's worktree has no lock); the
//!   closed sessions of a task just finished (`--task`) do not count there;
//! * its HEAD is an ancestor of `main` (detached or on a branch). A branch with no commits of its own
//!   (a reviewer's, an executor's not started, one switched to another branch) is judged like any
//!   other: the window, the sessions and the processes are what protect an agent about to start;
//! * no task in concept, design, ready, in_progress, review or blocked has the branch's name as its
//!   slug, or as its slug with the last `/` spelled `-` (`tools/python-zero-merge-risk` for
//!   `tools/python-zero/merge-risk`), has a role held by a session bound to it, either way (of any
//!   health: a lost session's work may be restored there), or was launched there
//!   (`task_session.worktree`);
//! * it holds no owner verdict that no commit has (an owner line the Symphony wrote into a docs/
//!   file, the one place the owner's answer to a card on this branch lives until the agent commits it);
//! * no process has its working directory inside it (`lsof`; when that cannot be read or lists
//!   nothing, nothing is removed);
//! * nothing in it is left behind: no tracked change, no untracked file, and no ignored file beyond
//!   caches and root verify logs. Regenerable `.target/`, `symphony/.target/`, `symphony/build/`
//!   and `.codex/hooks*` go with it; artifacts under a handle still keep it.
//!
//! Right before removing, it refreshes board sessions and launches, the worktree listing and lock,
//! activity, HEAD and what is left behind, then searches that tree alone for process working
//! directories (by inode, so lsof's escaped path display cannot hide one). HEAD and its symbolic
//! targets stay locked through removal, excluding a new clean commit after the final read.
//! A dead worktree lock is lifted and
//! put back if the removal is refused; and `git worktree remove`
//! runs without `--force`, so git itself refuses one that gained a change or an untracked file in
//! between. Then `git worktree prune`, and each `worktree-agent-*` branch merged into main that no
//! worktree holds is deleted, only if it still points where it did when listed and no live or stale
//! session still binds its agent's detached worktree. In-process agents must use their own
//! `agent-ID` worktree; their id does not bind an arbitrarily named checkout. One sweep runs at a
//! time (`sf-tasks/sweep.lock` in the git common dir). Contenders append a request and exit before
//! connecting to the board; the holder runs another pass for requests received during its pass.
//! Positive witnesses or bounded complete clean inventories reuse unchanged metadata/policy;
//! unknown inputs rescan. Cache hits stat saved entries without walking. Both final status guards
//! remain fresh; an early unavailable HEAD/removal guard retains the tree before lookup.
//! Foreground `--task` cleanup visits only its finished worktrees, guarded per tree, and asks for
//! a detached full pass.
//!
//! Of the worktrees that stay, it then removes the build output of finished work: a worktree's
//! build folders ([`build_output::FOLDERS`]) once every task it is tied to is done or cancelled,
//! one of them done, its HEAD is merged work (never a branch with no commits ahead of `main`), no
//! agent bound to it beat within the hour and no process works inside it ([`build_output`]). Tests: `symphony/crates/tasks/tests/sweep/`.

pub mod build_output;
mod coalesce;
mod command;
pub mod git;
mod guards;
pub mod process;
pub mod trigger;
mod verdict_inputs;
mod verdict_inventory;
mod verdicts;

pub use command::{Request, command, command_connect};

use crate::health::FINISHED;
use crate::rows::TaskSessionRow;
use crate::{Error, Session, Store, Task, sys};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The agent branches deleted once merged and free.
pub const AGENT_BRANCHES: &str = "refs/heads/worktree-agent-*";
/// Where a task may still be worked on or reviewed: claimed concept and design work keeps its stage.
pub const LIVE_TASK: [&str; 6] = [
    "concept",
    "design",
    "ready",
    "in_progress",
    "review",
    "blocked",
];
pub const RECENT_ENV: &str = "SF_SWEEP_RECENT_HOURS";
pub const RECENT_HOURS: f64 = 2.0;

/// What the board says that the sweep decides on, refreshed before each removal or ref deletion.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub sessions: Vec<Session>,
    pub tasks: Vec<Task>,
    pub launches: Vec<TaskSessionRow>,
}

impl Facts {
    pub fn read(store: &Store) -> Result<Facts, Error> {
        let board = store.board()?;
        Ok(Facts {
            sessions: board.sessions,
            tasks: board.tasks,
            launches: store.all_task_sessions()?,
        })
    }
}

/// A task just done or cancelled: the worktrees its closed sessions were launched in (its own and
/// those of the tasks under it), which go without the window, and those sessions, whose binding to
/// them does not count.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Finished {
    pub worktrees: BTreeSet<PathBuf>,
    pub handles: BTreeSet<String>,
}

impl Finished {
    /// Of task `slug`; none while it is not done or cancelled.
    pub fn of(store: &Store, slug: &str) -> Result<Finished, Error> {
        let mut found = Finished::default();
        if !FINISHED.contains(&store.get(slug)?.status.as_str()) {
            return Ok(found);
        }
        let mut slugs = vec![slug.to_string()];
        slugs.extend(store.descendants(slug)?.into_iter().map(|t| t.slug));
        for each in &slugs {
            for launched in store.task_sessions(each)? {
                if launched.closed.is_some() && !launched.worktree.is_empty() {
                    found
                        .worktrees
                        .insert(canonical(Path::new(&launched.worktree)));
                    found.handles.insert(launched.handle);
                }
            }
        }
        Ok(found)
    }
}

type Probe = Box<dyn Fn(i64) -> bool>;
type Dates = Box<dyn Fn(i64) -> Option<String>>;
type Scan = Box<dyn Fn() -> Option<Vec<PathBuf>>>;
type Occupied = Box<dyn Fn(&Path) -> Option<bool>>;
type Pause = Box<dyn Fn(&str, &str)>;

/// How a sweep runs. `pause(stage, what)` runs between the checks and each removal ("remove", the
/// path; "checked", after status; "unlock", before claiming a dead lock; "lock-claimed", after
/// its atomic rename; "unlocked", after releasing it and before the final guard) or branch deletion
/// ("delete", the branch), and before a build folder is cleared ("judged", the worktree, before
/// the rule is read again; "clear", the folder): what happens there is what the re-checks guard
/// against.
pub struct Options {
    pub dry_run: bool,
    /// The window of recent activity, in hours.
    pub recent_hours: f64,
    pub finished: Finished,
    /// An explicit task cleanup visits only these worktrees, with no repository-wide pruning.
    pub only: Option<BTreeSet<PathBuf>>,
    pub alive: Probe,
    pub started: Dates,
    pub cwds: Scan,
    pub occupied: Occupied,
    pub now: Box<dyn Fn() -> f64>,
    pub pause: Pause,
}

impl Options {
    /// The machine's own: its process table, `ps` in `vars`, a sessionless `lsof`, its clock.
    pub fn machine(vars: &BTreeMap<String, String>) -> Options {
        let vars = vars.clone();
        Options {
            dry_run: false,
            recent_hours: RECENT_HOURS,
            finished: Finished::default(),
            only: None,
            alive: Box::new(process::pid_alive),
            started: Box::new(move |pid| process::process_start(pid, &vars)),
            cwds: Box::new(|| process_cwd::process_cwds_with(sys::command("lsof"))),
            occupied: Box::new(|tree| process_cwd::occupied_with(sys::command("lsof"), tree)),
            now: Box::new(trigger::now),
            pause: Box::new(|_, _| {}),
        }
    }
}

/// What a sweep did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sweep {
    pub removed: Vec<(PathBuf, String)>,
    /// Merged, but holding files that did not land.
    pub kept: Vec<(PathBuf, Vec<String>)>,
    /// Merged, but in use.
    pub spared: Vec<(PathBuf, String)>,
    /// The build folders of finished work, removed from worktrees that stay ([`build_output`]).
    pub cleared: Vec<(PathBuf, String)>,
    pub branches: Vec<String>,
    pub total: usize,
    pub dry_run: bool,
}

impl Sweep {
    /// One line per removal, worktree kept and branch deleted, then the summary.
    pub fn lines(&self) -> Vec<String> {
        let verb = if self.dry_run {
            "would remove"
        } else {
            "removed"
        };
        let mut out: Vec<String> = self
            .removed
            .iter()
            .map(|(path, why)| format!("{verb} {} ({why})", path.display()))
            .collect();
        out.extend(self.kept.iter().map(|(path, left)| {
            let more = if left.len() > 5 { " …" } else { "" };
            format!(
                "kept {}: merged, but holds files that did not land: {}{more}",
                path.display(),
                left.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
            )
        }));
        out.extend(
            self.spared
                .iter()
                .map(|(path, why)| format!("kept {}: {why}", path.display())),
        );
        let clear = if self.dry_run {
            "would clear"
        } else {
            "cleared"
        };
        out.extend(
            self.cleared
                .iter()
                .map(|(path, why)| format!("{clear} build output {} ({why})", path.display())),
        );
        let deleted = if self.dry_run {
            "would delete"
        } else {
            "deleted"
        };
        out.extend(
            self.branches
                .iter()
                .map(|b| format!("{deleted} branch {b} (merged into main, no worktree holds it)")),
        );
        out.push(self.summary());
        out
    }

    pub fn summary(&self) -> String {
        let (removed, deleted) = if self.dry_run {
            ("would remove", "would delete")
        } else {
            ("removed", "deleted")
        };
        let mut said = format!(
            "worktree sweep: {removed} {} of {} worktrees, {deleted} {} merged worktree-agent-* branch{}",
            self.removed.len(),
            self.total,
            self.branches.len(),
            if self.branches.len() == 1 { "" } else { "es" }
        );
        if !self.kept.is_empty() {
            said += &format!(
                "; kept {} merged for files that did not land",
                self.kept.len()
            );
        }
        if !self.spared.is_empty() {
            said += &format!("; kept {} in use", self.spared.len());
        }
        if !self.cleared.is_empty() {
            said += &format!(
                "; {} {} build folder{} of finished work",
                if self.dry_run {
                    "would clear"
                } else {
                    "cleared"
                },
                self.cleared.len(),
                if self.cleared.len() == 1 { "" } else { "s" }
            );
        }
        said
    }
}

/// `path` with its links resolved, or as it is when it cannot be (gone).
pub fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Whether `path` is inside `outer`, not `outer` itself.
fn inside(path: &Path, outer: &Path) -> bool {
    path != outer && path.starts_with(outer)
}

/// The board's facts by worktree.
struct Index<'a> {
    /// Live and stale sessions: their worktree and handle.
    bound: Vec<(PathBuf, &'a str)>,
    /// Every session, whatever its health, by its worktree.
    sessions_at: BTreeMap<PathBuf, Vec<&'a str>>,
    /// The newest beat or tool call of the sessions at each worktree, whatever their health.
    beats_at: BTreeMap<PathBuf, f64>,
    /// Every launch, closed or not, by its worktree.
    launched_at: BTreeMap<PathBuf, Vec<&'a TaskSessionRow>>,
    tasks: BTreeMap<&'a str, &'a Task>,
    /// The tasks whose slug, with its last `/` as `-`, is a branch's name, by that name.
    spelled: BTreeMap<String, Vec<&'a Task>>,
}

/// The branch name `slug` is worked on under when its last `/` is spelled `-`
/// (`tools/python-zero-merge-risk` for `tools/python-zero/merge-risk`); none without a `/`.
pub(crate) fn spelled(slug: &str) -> Option<String> {
    slug.rsplit_once('/')
        .map(|(above, last)| format!("{above}-{last}"))
}

/// Where a session works: its `session.worktree`, and, for a session bound to the Claude Code agent
/// X, the worktree among `worktrees` named `agent-X`. A subagent's session is registered at the
/// main checkout by its orchestrator, and a resumed agent's worktree has no lock: its agent id is
/// what ties it to its own worktree.
fn places(session: &Session, worktrees: &[PathBuf]) -> Vec<PathBuf> {
    let mut places = Vec::new();
    if !session.row.worktree.is_empty() {
        places.push(canonical(Path::new(&session.row.worktree)));
    }
    if let Some(id) = session.row.agent.as_deref().filter(|id| !id.is_empty()) {
        let name = format!("agent-{id}");
        for wt in worktrees {
            if wt.file_name().is_some_and(|n| *n == *name) && !places.contains(wt) {
                places.push(wt.clone());
            }
        }
    }
    places
}

impl<'a> Index<'a> {
    /// The facts, a session's agent id read against the repository's `worktrees`.
    fn of(facts: &'a Facts, worktrees: &[PathBuf]) -> Index<'a> {
        let mut index = Index {
            bound: Vec::new(),
            sessions_at: BTreeMap::new(),
            beats_at: BTreeMap::new(),
            launched_at: BTreeMap::new(),
            tasks: facts.tasks.iter().map(|t| (t.slug.as_str(), t)).collect(),
            spelled: BTreeMap::new(),
        };
        for task in &facts.tasks {
            if let Some(name) = spelled(&task.slug) {
                index.spelled.entry(name).or_default().push(task);
            }
        }
        for s in &facts.sessions {
            let beat = s.row.last_seen.max(s.row.last_tool.unwrap_or(f64::MIN));
            for at in places(s, worktrees) {
                if s.health.holds() {
                    index.bound.push((at.clone(), &s.row.handle));
                }
                index
                    .beats_at
                    .entry(at.clone())
                    .and_modify(|newest| *newest = newest.max(beat))
                    .or_insert(beat);
                index.sessions_at.entry(at).or_default().push(&s.row.handle);
            }
        }
        for launched in facts.launches.iter().filter(|l| !l.worktree.is_empty()) {
            let at = canonical(Path::new(&launched.worktree));
            index.launched_at.entry(at).or_default().push(launched);
        }
        index
    }

    /// When an agent at `path` last beat or made a tool call, whatever its health; none when no
    /// session works there.
    fn last_beat(&self, path: &Path) -> Option<f64> {
        self.beats_at.get(path).copied()
    }

    /// Whether a live or stale session other than `unbound` is bound to `path`.
    fn bound(&self, path: &Path, unbound: &BTreeSet<String>) -> bool {
        self.bound
            .iter()
            .any(|(at, handle)| at == path && !unbound.contains(*handle))
    }

    /// Every task `path` is tied to: those its branch names (as its slug, or its slug with the last
    /// `/` spelled `-`), those its sessions hold a role on, and those launched there, by slug.
    fn tied(&self, path: &Path, branch: Option<&str>) -> BTreeMap<&'a str, String> {
        let mut tied: BTreeMap<&'a str, String> = BTreeMap::new();
        if let Some(&task) = branch.and_then(|b| self.tasks.get(b)) {
            tied.insert(
                task.slug.as_str(),
                format!("task {} is {}", task.slug, task.status),
            );
        }
        if let Some(branch) = branch {
            for &task in self.spelled.get(branch).into_iter().flatten() {
                tied.entry(task.slug.as_str()).or_insert_with(|| {
                    format!(
                        "task {} is {}, whose slug branch {branch} spells with its last / as -",
                        task.slug, task.status
                    )
                });
            }
        }
        for &handle in self.sessions_at.get(path).into_iter().flatten() {
            for &task in self.tasks.values() {
                if task.roles.iter().any(|r| r.handle == handle) {
                    tied.entry(task.slug.as_str()).or_insert_with(|| {
                        format!(
                            "task {} is {}, held by {handle}, whose session is bound here",
                            task.slug, task.status
                        )
                    });
                }
            }
        }
        for &launched in self.launched_at.get(path).into_iter().flatten() {
            if let Some(&task) = self.tasks.get(launched.slug.as_str()) {
                tied.entry(task.slug.as_str()).or_insert_with(|| {
                    format!(
                        "task {} is {}, launched here for {}",
                        task.slug, task.status, launched.handle
                    )
                });
            }
        }
        tied
    }

    /// Why a task still live keeps `path`, if one does.
    fn live(&self, path: &Path, branch: Option<&str>) -> Option<String> {
        self.tied(path, branch)
            .into_iter()
            .find(|(slug, _)| LIVE_TASK.contains(&self.tasks[slug].status.as_str()))
            .map(|(_, why)| why)
    }

    /// Whether every task `path` is tied to is done or cancelled (or it is tied to none).
    fn finished(&self, path: &Path, branch: Option<&str>) -> bool {
        self.tied(path, branch)
            .keys()
            .all(|slug| FINISHED.contains(&self.tasks[slug].status.as_str()))
    }
}

/// Remove every worktree of `root`'s repository that the rules let go (none when `dry_run`); see the
/// module's doc. `here` is the worktree running the sweep, which keeps itself.
pub fn sweep(root: &Path, here: &Path, store: &Store, o: &Options) -> Result<Sweep, String> {
    let mut result = Sweep {
        dry_run: o.dry_run,
        ..Sweep::default()
    };
    let trees = git::worktrees(root)?;
    result.total = trees.len();
    let Some(main) = git::rev(root, "refs/heads/main") else {
        return Ok(result);
    };
    let here = canonical(here);
    let paths: Vec<PathBuf> = trees.iter().map(|t| canonical(&t.path)).collect();
    let facts = Facts::read(store).map_err(|e| e.to_string())?;
    let index = Index::of(&facts, &paths);
    let nobody = BTreeSet::new();
    // The processes' directories, asked once and only when a worktree gets that far.
    let mut working: Option<Option<Vec<PathBuf>>> = None;
    let mut gone: BTreeSet<PathBuf> = BTreeSet::new();
    let mut ancestry = BTreeMap::new();
    let verdicts = verdicts::Verdicts::new(root, o.dry_run);
    for (number, (entry, path)) in trees.iter().zip(&paths).enumerate() {
        if o.only.as_ref().is_some_and(|only| !only.contains(path)) {
            continue;
        }
        if number == 0 || *path == here || entry.bare || entry.prunable || !path.is_dir() {
            continue;
        }
        if entry.branch.as_deref() == Some("refs/heads/main")
            || paths.iter().any(|other| inside(other, path))
        {
            continue;
        }
        let finishing = o.finished.worktrees.contains(path);
        let window = if finishing { 0.0 } else { o.recent_hours };
        let now = (o.now)();
        if now - git::last_touched(path, now) < window * 3600.0 {
            continue;
        }
        let mut why = "no lock".to_string();
        if let Some(reason) = &entry.locked {
            match process::dead_lock(reason, &*o.alive, &*o.started) {
                None => continue,
                Some(dead) => why = dead,
            }
        }
        let head = entry.head.as_str();
        let unbound = if finishing {
            &o.finished.handles
        } else {
            &nobody
        };
        if index.bound(path, unbound)
            || head.is_empty()
            || !*ancestry
                .entry(head)
                .or_insert_with(|| git::ancestor(root, head, &main))
        {
            continue;
        }
        let listed = &entry.path;
        let branch = entry
            .branch
            .as_deref()
            .and_then(|b| b.strip_prefix("refs/heads/"));
        if let Some(held) = index.live(path, branch) {
            result.spared.push((listed.clone(), held));
            continue;
        }
        let _removal_guard = match guards::acquire(path, o.dry_run) {
            Ok(held) => held,
            Err(why) => {
                result.spared.push((listed.clone(), why));
                continue;
            }
        };
        let seen = working.get_or_insert_with(|| {
            (o.cwds)()
                .filter(|cwds| !cwds.is_empty())
                .map(|cwds| cwds.iter().map(|w| canonical(w)).collect())
        });
        match seen {
            None => {
                result.spared.push((
                    listed.clone(),
                    "cannot tell whether a process is working inside it".into(),
                ));
                continue;
            }
            Some(cwds) if cwds.iter().any(|w| w.starts_with(path)) => {
                result
                    .spared
                    .push((listed.clone(), "a process is working inside it".into()));
                continue;
            }
            Some(_) => {}
        }
        let finished = index.finished(path, branch);
        let verdict = verdicts.read(entry, finished);
        let owner = verdict.owner;
        if !owner.is_empty() {
            result.spared.push((
                listed.clone(),
                format!("holds an owner verdict no commit has: {}", owner.join(", ")),
            ));
            continue;
        }
        let left = verdict.left;
        if !left.is_empty() {
            result.kept.push((listed.clone(), left));
            continue;
        }
        let place = branch.map_or_else(|| "detached".to_string(), |b| format!("on {b}"));
        let short = head.get(..12).unwrap_or(head);
        let why = format!("{place} at {short}, merged into main, clean, {why}");
        if !o.dry_run {
            (o.pause)("remove", &listed.to_string_lossy());
        }
        if !remove(root, listed, entry, store, o)? {
            continue;
        }
        result.removed.push((listed.clone(), why));
        gone.insert(path.clone());
    }
    result.cleared =
        build_output::clear(root, &here, &main, store, o, &gone, &mut working, &trees)?;
    if !o.dry_run && o.only.is_none() {
        let _ = git::git(root, &["worktree", "prune"]);
    }
    if o.only.is_none() {
        result.branches = agent_branches(root, &main, &trees, &paths, &gone, store, o)?;
    }
    Ok(result)
}

/// Remove `path` if it is still at `head` with nothing left behind; git checks cleanliness again
/// itself (no `--force`). Only the exact dead lock is lifted; refusal restores it without replacing
/// another owner's lock. The caller holds the per-tree removal guard throughout.
fn remove(
    root: &Path,
    path: &Path,
    judged: &git::Listed,
    store: &Store,
    o: &Options,
) -> Result<bool, String> {
    let shown = path.to_string_lossy();
    // Re-read after the slow, shared scan and earlier removals: a launch, checkout or lock
    // acquired in that window must protect this tree, even when its HEAD is still merged.
    let trees = git::worktrees(root)?;
    let paths: Vec<_> = trees.iter().map(|t| canonical(&t.path)).collect();
    let Some(entry) = trees.iter().find(|t| t.path == path) else {
        return Ok(false);
    };
    if entry.head != judged.head
        || entry.branch != judged.branch
        || entry.prunable
        || !path.is_dir()
        || paths.iter().any(|other| inside(other, path))
    {
        return Ok(false);
    }
    let facts = Facts::read(store).map_err(|e| e.to_string())?;
    let index = Index::of(&facts, &paths);
    let nobody = BTreeSet::new();
    let finishing = o.finished.worktrees.contains(&canonical(path));
    let unbound = if finishing {
        &o.finished.handles
    } else {
        &nobody
    };
    let branch = entry
        .branch
        .as_deref()
        .and_then(|b| b.strip_prefix("refs/heads/"));
    if index.bound(path, unbound) || index.live(path, branch).is_some() {
        return Ok(false);
    }
    let finished = index.finished(path, branch);
    if !git::left_behind(path, finished).is_empty() {
        return Ok(false);
    }
    (o.pause)("checked", &shown);
    // After the status: a commit or checkout made while it ran leaves the tree clean. The
    // lock must also be the current one before we decide whether to lift it.
    let current = git::worktrees(root)?;
    let Some(entry) = current.iter().find(|t| t.path == path) else {
        return Ok(false);
    };
    if entry.head != judged.head
        || entry.branch != judged.branch
        || entry.prunable
        || current.iter().any(|t| inside(&canonical(&t.path), path))
    {
        return Ok(false);
    }
    let lock = entry.locked.as_deref();
    let observed_lock = match lock {
        Some(reason) => {
            let Some(observed) = git::WorktreeLock::capture(path, reason) else {
                return Ok(false);
            };
            Some(observed)
        }
        None => None,
    };
    if lock.is_some_and(|reason| process::dead_lock(reason, &*o.alive, &*o.started).is_none()) {
        return Ok(false);
    }
    let facts = Facts::read(store).map_err(|e| e.to_string())?;
    let current_paths: Vec<_> = current.iter().map(|t| canonical(&t.path)).collect();
    let index = Index::of(&facts, &current_paths);
    if index.bound(path, unbound)
        || index.live(path, branch).is_some()
        || index.finished(path, branch) != finished
    {
        return Ok(false);
    }
    let now = (o.now)();
    let window = if finishing { 0.0 } else { o.recent_hours };
    if now - git::last_touched(path, now) < window * 3600.0 {
        return Ok(false);
    }
    if o.dry_run {
        return Ok((o.occupied)(path) == Some(false));
    }
    let _released_lock = match observed_lock {
        Some(observed) => match observed.release(|stage| (o.pause)(stage, &shown)) {
            Some(released) => Some(released),
            None => return Ok(false),
        },
        None => None,
    };
    (o.pause)("unlocked", &shown);
    let Some(_head) = git::hold_head(path) else {
        return Ok(false);
    };
    // The last snapshot is taken with HEAD and its branch held through git's removal.
    // A clean detached commit after the earlier status must not lose its only reflog.
    let current = git::worktrees(root)?;
    let Some(entry) = current.iter().find(|t| t.path == path) else {
        return Ok(false);
    };
    let at = canonical(path);
    if entry.head != judged.head
        || entry.branch != judged.branch
        || entry.locked.is_some()
        || entry.prunable
        || current.iter().any(|t| inside(&canonical(&t.path), &at))
    {
        return Ok(false);
    }
    let current_paths: Vec<_> = current.iter().map(|t| canonical(&t.path)).collect();
    let facts = Facts::read(store).map_err(|e| e.to_string())?;
    let index = Index::of(&facts, &current_paths);
    if index.bound(&at, unbound)
        || index.live(&at, branch).is_some()
        || index.finished(&at, branch) != finished
    {
        return Ok(false);
    }
    if !git::left_behind(path, finished).is_empty() {
        return Ok(false);
    }
    let now = (o.now)();
    if now - git::last_touched(path, now) < window * 3600.0 || (o.occupied)(path) != Some(false) {
        return Ok(false);
    }
    // No status or board scan after this tree's process scan. Ref writers remain excluded
    // until removal completes; git itself checks tracked and untracked changes once more.
    Ok(git::clean_regenerable(path)
        && git::succeeds(root, &["worktree", "remove", &shown])
        && !path.exists())
}

/// Delete each `worktree-agent-*` branch merged into main that no worktree left holds, only if it
/// still points where it did when listed: a branch that moved since is not what was judged merged.
fn agent_branches(
    root: &Path,
    main: &str,
    trees: &[git::Listed],
    paths: &[PathBuf],
    gone: &BTreeSet<PathBuf>,
    store: &Store,
    o: &Options,
) -> Result<Vec<String>, String> {
    let held: BTreeSet<&str> = trees
        .iter()
        .zip(paths)
        .filter(|(_, path)| !gone.contains(*path))
        .filter_map(|(entry, _)| entry.branch.as_deref())
        .collect();
    let listing = git::git(
        root,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            AGENT_BRANCHES,
        ],
    )
    .filter(|done| done.status.success())
    .map(|done| String::from_utf8_lossy(&done.stdout).into_owned())
    .unwrap_or_default();
    let mut deleted = Vec::new();
    for line in listing.lines() {
        let Some((reference, sha)) = line.split_once(' ') else {
            continue;
        };
        if held.contains(reference) || !git::ancestor(root, sha, main) {
            continue;
        }
        let name = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        if !o.dry_run {
            (o.pause)("delete", name);
        }
        // A detached agent still owns its worktree-agent-ID ref. Refresh both the session
        // bindings and the checkout list: an agent can detach or check out this ref mid-sweep.
        let current = git::worktrees(root)?;
        if current
            .iter()
            .any(|t| t.branch.as_deref() == Some(reference))
            && !o.dry_run
        {
            continue;
        }
        let current_paths: Vec<_> = current.iter().map(|t| canonical(&t.path)).collect();
        let facts = Facts::read(store).map_err(|e| e.to_string())?;
        let protected = name.strip_prefix("worktree-agent-").is_some_and(|id| {
            let own_tree = format!("agent-{id}");
            facts.sessions.iter().any(|s| {
                s.health.holds()
                    && places(s, &current_paths).iter().any(|p| {
                        current_paths.contains(p)
                            && current_paths.first() != Some(p)
                            && (!o.dry_run || !gone.contains(p))
                            && (s.row.agent.as_deref() == Some(id)
                                || p.file_name().is_some_and(|n| n == own_tree.as_str()))
                    })
            })
        });
        if protected {
            continue;
        }
        if o.dry_run || git::succeeds(root, &["update-ref", "-d", reference, sha]) {
            deleted.push(name.to_string());
        }
    }
    Ok(deleted)
}

/// The window of recent activity: the flag, else `SF_SWEEP_RECENT_HOURS`, else 2 h; refused when it
/// is not a number of hours, 0 or more.
pub fn recent_hours(vars: &BTreeMap<String, String>, flag: Option<f64>) -> Result<f64, String> {
    let hours = match (flag, vars.get(RECENT_ENV).map(|v| v.trim())) {
        (Some(hours), _) => hours,
        (None, Some(text)) if !text.is_empty() => text
            .parse::<f64>()
            .map_err(|e| format!("{RECENT_ENV}={text:?} is not a number of hours ({e})"))?,
        _ => RECENT_HOURS,
    };
    if hours >= 0.0 {
        Ok(hours)
    } else {
        Err(format!(
            "the window of recent activity must be 0 hours or more, not {hours}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(window: Option<&str>) -> BTreeMap<String, String> {
        window
            .map(|w| (RECENT_ENV.to_string(), w.to_string()))
            .into_iter()
            .collect()
    }

    #[test]
    fn the_window_is_the_flag_else_the_environment_else_two_hours() {
        assert_eq!(recent_hours(&vars(None), None), Ok(RECENT_HOURS));
        for blank in ["", "  "] {
            assert_eq!(recent_hours(&vars(Some(blank)), None), Ok(RECENT_HOURS));
        }
        assert_eq!(recent_hours(&vars(Some(" 0.5 ")), None), Ok(0.5));
        assert_eq!(recent_hours(&vars(Some("0.5")), Some(4.0)), Ok(4.0));
        assert_eq!(recent_hours(&vars(Some("soon")), Some(0.0)), Ok(0.0));
        let said = recent_hours(&vars(Some("soon")), None).unwrap_err();
        assert!(
            said.starts_with("SF_SWEEP_RECENT_HOURS=\"soon\" is not a number of hours"),
            "{said}"
        );
        for wrong in ["-0.5", "NaN"] {
            assert!(recent_hours(&vars(Some(wrong)), None).is_err(), "{wrong}");
        }
    }

    #[test]
    fn a_slug_is_spelled_as_a_branch_with_its_last_slash_as_a_dash() {
        assert_eq!(
            spelled("tools/python-zero/merge-risk").as_deref(),
            Some("tools/python-zero-merge-risk")
        );
        assert_eq!(spelled("sim/jump").as_deref(), Some("sim-jump"));
        assert_eq!(spelled("sim/").as_deref(), Some("sim-"));
        assert_eq!(spelled("jump"), None);
    }

    #[test]
    fn the_summary_counts_in_the_singular_and_the_plural() {
        let mut done = Sweep {
            total: 3,
            branches: vec!["worktree-agent-a".into()],
            ..Sweep::default()
        };
        assert_eq!(
            done.summary(),
            "worktree sweep: removed 0 of 3 worktrees, deleted 1 merged worktree-agent-* branch"
        );
        done.dry_run = true;
        done.kept
            .push((PathBuf::from("/wt/a"), vec!["notes.txt".into()]));
        done.spared.push((
            PathBuf::from("/wt/b"),
            "a process is working inside it".into(),
        ));
        assert_eq!(
            done.summary(),
            "worktree sweep: would remove 0 of 3 worktrees, would delete 1 merged worktree-agent-* branch; kept 1 merged for files that did not land; kept 1 in use"
        );
    }

    #[test]
    fn a_kept_worktree_names_five_files_at_most() {
        let left: Vec<String> = (1..=7).map(|n| format!("f{n}")).collect();
        let done = Sweep {
            kept: vec![(PathBuf::from("/wt/a"), left)],
            ..Sweep::default()
        };
        assert_eq!(
            done.lines()[0],
            "kept /wt/a: merged, but holds files that did not land: f1, f2, f3, f4, f5 …"
        );
    }
}
