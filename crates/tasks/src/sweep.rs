//! The worktree sweep: remove the worktrees nobody needs any more, merged into main, holding nothing
//! that did not land, with no live owner: `just tasks sweep
//! [--dry-run] [--recent-hours H] [--task SLUG]...`.
//!
//! `just land` runs it for the task it landed (`--task`) and prints what it removed and cleared and
//! its summary, which names the bytes freed; work finishing starts a full pass by itself
//! ([`trigger`]). Every worktree git lists is the sweep's, wherever it is (`../symphony-wt/*`
//! included). A worktree goes only when all of these hold:
//!
//! * it is not the main checkout, not the worktree running the sweep, not where `main` is checked
//!   out, and holds no other worktree inside it;
//! * nothing touched it lately: its admin dir's HEAD, index and logs/HEAD and its top folder are all
//!   older than the window (2 h; `--recent-hours` or `SYMPHONY_SWEEP_RECENT_HOURS`). The worktrees
//!   of a task just done or cancelled (`--task`) have no window: those its closed launches ran in,
//!   and those its executors' and reviewers' sessions work in (by `session.worktree`, or as Claude
//!   Code agent X for `agent-X`) once none of those sessions is live or stale, since an in-process
//!   agent has no launch record, and those on its branch (its slug, or the slug with its last `/`
//!   as `-`), since an agent that left has released its roles;
//! * no board session that is live or stale is bound to it: by `session.worktree`, or, for a session
//!   bound to Claude Code agent X (`session.agent`), as the worktree named `agent-X` (a subagent's
//!   session is registered at the main checkout, and a resumed agent's worktree has no lock); the
//!   closed sessions of a task just finished (`--task`) do not count there;
//! * its HEAD is an ancestor of `main` (detached or on a branch), or is exactly a head a pull
//!   request squashed into main, proved by the receipt its land kept ([`delivered`]). A branch
//!   with no commits of its own (a reviewer's, an executor's not started, one switched to another
//!   branch) is judged like any other: the window, the sessions and the processes are what
//!   protect an agent about to start;
//! * no task in concept, design, ready, in_progress, review or blocked has the branch's name as its
//!   slug, or as its slug with the last `/` spelled `-` (`tools/python-zero-merge-risk` for
//!   `tools/python-zero/merge-risk`), has a role held by a session bound to it, either way (of any
//!   health: a lost session's work may be restored there), or was launched there
//!   (`task_session.worktree`);
//! * its lock holds nobody ([`holder`]): it has none; or Claude Code's, whose pid is not running or
//!   runs with another start (a reused pid; `ps -p N`, in the C locale and in UTC as Claude Code
//!   writes it, says); or a land's whose pid is not the process that started when the lock says
//!   (not running, when it says no start); or Claude Code's of a live process once
//!   the board shows its agent finished. That process is the session that started the agent, its
//!   orchestrator, which outlives every agent it started, so the lock alone never says the agent
//!   still works: the board does, when a session is bound to the worktree (by its worktree, or as
//!   the agent the lock and the worktree's name give), none of them is live or stale, and every task
//!   tied to the worktree, at least one, is done or cancelled ([`Index::agent_finished`]). A lock
//!   the board cannot answer for keeps it: an agent it knows nothing of, a land that runs, the
//!   release run's checkout, any other reason, a start that cannot be read;
//! * it holds no owner verdict that no commit has (an owner line the Symphony wrote under a card of
//!   a document, in a folder the main checkout's project file serves documents from, the one place
//!   the owner's answer to a card on this branch lives until the agent commits it: [`git::answers`]);
//!   a worktree whose documents git cannot read for them is kept, saying so;
//! * no process has its working directory inside it (`lsof`; when that cannot be read or lists
//!   nothing, nothing is removed);
//! * nothing in it is left behind: no tracked change, no untracked file, and no ignored file beyond
//!   caches and root verify logs. Regenerable `.target/`, `symphony/.target/` (and beside each a
//!   seed's staging, `.target.seed.PID`, or a clear's, `.target.clear.PID`), `symphony/build/` and
//!   `.codex/hooks*` go with it, so does a file manager's metadata, untracked or ignored
//!   (`.DS_Store`, `Thumbs.db`, `desktop.ini`), and so does what the project itself names
//!   (`[board] sweep_disposable` of the main checkout's manifest, [`crate::project::disposable`]:
//!   another stack's dependency and build folders). Once its work is finished (every task tied to
//!   it done or cancelled, or none), its other caches go too, and what git ignores under the project's
//!   artifacts folder (`[project] artifacts` of the main checkout's manifest, `artifacts/` when it
//!   names none, [`crate::project::artifacts`]: the evidence its thread names, a test's leftovers)
//!   is moved into the main checkout's `FOLDER/swept/NAME/` ([`aside`]) right before git removes
//!   the worktree, whatever came there since it was judged, and moved back if the removal is
//!   refused; before then, its artifacts keep it. A move that cannot be made (another disk, a name
//!   taken) keeps the worktree, saying why: evidence is moved, never copied, replaced or removed.
//!   Its build output is never set aside. Nothing removes what was set aside: each sweep that may
//!   set aside measures it against the project's cap ([`kept`]) and, past it, says so and names the
//!   largest folders there, each in use or with no reference found ([`mentions`] read once for
//!   that), for a check by hand.
//!
//! Right before removing, it refreshes board sessions and launches, the worktree listing and lock,
//! activity, HEAD and what is left behind, then searches that tree alone for process working
//! directories (by inode, so lsof's escaped path display cannot hide one). HEAD and its symbolic
//! targets stay locked through removal, excluding a new clean commit after the final read.
//! A lock that holds nobody is lifted and
//! put back if the removal is refused; and `git worktree remove`
//! runs without `--force`, so git itself refuses one that gained a change or an untracked file in
//! between. Then `git worktree prune`; the branch a removed worktree had checked out is deleted once
//! merged into main or squashed into it by a receipt's proof, its head then kept under the
//! delivery's own reference, if no worktree holds it and it still points where it did, unless a
//! land's record of its merge names it (`just land SLUG` run again finishes from it and the
//! branch), a delivery of it by pull request not yet asked to merge reads its tip again, or its
//! task is done by an approve no landing has closed (the guard against moving it back to review
//! reads the branch's tip, [`crate::land::reopenable`]), each said as `kept branch`; and each
//! `worktree-agent-*` branch merged into main, or squashed into it, that no
//! worktree holds is deleted, only if it still points where it did when listed and no live or stale
//! session still binds its agent's detached worktree. The comments on documents a pass wrote into a
//! removed worktree's files, all in main now, are kept under main's from then on
//! ([`Store::held_landed`]), where the count of a later pass reads them, once no other listed
//! worktree holds its branch; a board that refuses it is said, the sweep going on. In-process
//! agents must use their own `agent-ID` worktree; their id does not bind an arbitrarily named
//! checkout. One sweep runs at a time (`symphony/tasks/sweep.lock` in the git common dir).
//! Contenders append a request and exit before connecting to the board; the holder runs another
//! pass for requests received during its pass. Positive witnesses or bounded complete clean
//! inventories reuse unchanged metadata/policy; unknown inputs rescan. Cache hits stat saved
//! entries without walking. Both final status guards remain fresh; an early unavailable
//! HEAD/removal guard retains the tree before lookup. Foreground `--task` cleanup visits only its
//! finished worktrees, guarded per tree, and asks for a detached full pass.
//!
//! Of the worktrees that stay, it then removes the build output of work past its executor: a
//! worktree's build folders ([`build_output::FOLDERS`], and what git ignores in
//! [`build_output::BUILDS`]) once every task it is tied to is in review, done or cancelled, or it
//! is a land's that ended or the release run's, and nothing ran there for an hour, the recorder's
//! builds and tests included ([`build_output`]); a worktree tied to no task keeps its build.
//!
//! Each removal and clear says what it held on disk ([`size`]), and the summary the bytes freed. A
//! dry run removes nothing and says, for every worktree it visits ([`report`]), what it holds on
//! disk, the tasks tied to it and their stage, whether its HEAD is in main, and what the sweep would
//! do with it and why: a worktree with tracked edits or commits not in main is never removed,
//! whatever its task's stage, and is listed with its size for an orchestrator to decide. It then
//! says what the main checkout holds set aside, which nothing empties, and its summary what a
//! sweep would free of what they hold, which a sweep on the same state frees. Tests:
//! `crates/tasks/tests/sweep/`.

mod aside;
pub mod build_output;
mod coalesce;
mod command;
pub mod delivered;
pub mod git;
mod guards;
pub mod holder;
pub mod kept;
pub mod process;
pub mod report;
pub mod size;
pub mod trigger;
mod verdict_inputs;
mod verdict_inventory;
mod verdicts;

pub use aside::{SWEPT, swept};
pub use command::{Request, command, command_connect};

use crate::maps::Collect as _;
use crate::rows::TaskSessionRow;
use crate::workflow::operate::{self, Operating};
use crate::{Error, Session, Store, Task, sys};
use build_output::Cleared;
use delivered::Delivered;
use holder::Holder;
use report::Row;
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
pub const RECENT_ENV: &str = symphony_env::SWEEP_RECENT_HOURS.name;
pub const RECENT_HOURS: f64 = 2.0;

/// What the board says that the sweep decides on, refreshed before each removal or ref deletion.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub sessions: Vec<Session>,
    pub tasks: Vec<Task>,
    pub launches: Vec<TaskSessionRow>,
    /// The custom workflow that holds some of `tasks`, by whose positions they are finished or
    /// still worked on; none where it holds none.
    pub operating: Option<Operating>,
}

impl Facts {
    pub fn read(store: &Store) -> Result<Facts, Error> {
        let board = store.board()?;
        Ok(Facts {
            operating: store.operating_for(&board.tasks)?,
            sessions: board.sessions,
            tasks: board.tasks,
            launches: store.all_task_sessions()?,
        })
    }
}

/// A task just done or cancelled: the worktrees its closed sessions were launched in (its own and
/// those of the tasks under it), which go without the window, and those sessions, whose binding to
/// them does not count; the worktrees its executors and reviewers work in, by their sessions, once
/// none of those is live or stale; and those on its branches, whoever worked there (an agent that
/// left has released its roles). Those go without the window too.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Finished {
    pub worktrees: BTreeSet<PathBuf>,
    pub handles: BTreeSet<String>,
    /// The Claude Code agents those executors and reviewers are (`session.agent`): their own
    /// `agent-ID` worktrees.
    pub agents: BTreeSet<String>,
    /// The branches it and the tasks under it are worked on: each slug, and each with its last `/`
    /// spelled `-`.
    pub branches: BTreeSet<String>,
}

impl Finished {
    /// Of task `slug`; none while it is not done or cancelled.
    pub fn of(store: &Store, slug: &str) -> Result<Finished, Error> {
        let mut found = Finished::default();
        if !store.finished(&store.get(slug)?)? {
            return Ok(found);
        }
        let mut slugs = vec![slug.to_string()];
        slugs.extend(store.descendants(slug)?.into_iter().map(|t| t.slug));
        for each in &slugs {
            found.branches.insert(each.clone());
            found.branches.extend(spelled(each));
            for launched in store.task_sessions(each)? {
                if launched.closed.is_some() && !launched.worktree.is_empty() {
                    found
                        .worktrees
                        .insert(canonical(Path::new(&launched.worktree)));
                    found.handles.insert(launched.handle);
                }
            }
        }
        // An in-process agent (a Claude Code subagent) has no launch record: its session says where
        // it worked, and its health whether it still does.
        let board = store.board()?;
        let mut workers: BTreeSet<&str> = BTreeSet::new();
        workers.extend(
            board
                .tasks
                .iter()
                .filter(|task| slugs.contains(&task.slug))
                .flat_map(|task| &task.roles)
                .filter(|role| matches!(role.role.as_str(), "executor" | "reviewer"))
                .map(|role| role.handle.as_str()),
        );
        for session in &board.sessions {
            if !workers.contains(session.row.handle.as_str()) || session.health.holds() {
                continue;
            }
            if !session.row.worktree.is_empty() {
                found
                    .worktrees
                    .insert(canonical(Path::new(&session.row.worktree)));
            }
            if let Some(agent) = session.row.agent.as_deref().filter(|id| !id.is_empty()) {
                found.agents.insert(agent.to_string());
            }
        }
        Ok(found)
    }

    /// Whether worktree `path` (canonical), on `branch` (its name; none when detached), is one of
    /// them: one their sessions worked in, the `agent-ID` worktree of one of their agents, or one on
    /// one of their branches.
    pub fn covers(&self, path: &Path, branch: Option<&str>) -> bool {
        self.worktrees.contains(path)
            || branch.is_some_and(|name| self.branches.contains(name))
            || path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix("agent-"))
                .is_some_and(|id| self.agents.contains(id))
    }
}

type Probe = Box<dyn Fn(i64) -> bool>;
type Dates = Box<dyn Fn(i64) -> Option<String>>;
type Scan = Box<dyn Fn() -> Option<Vec<PathBuf>>>;
type Occupied = Box<dyn Fn(&Path) -> Option<bool>>;
type Pause = Box<dyn Fn(&str, &str)>;
/// A branch a removed worktree had checked out that stays, and why.
pub type Kept = (String, String);
/// What may still name a path under the artifacts folder, each with where it is said
/// ([`mentions`]), and what could not be read, and why.
type Said = (Vec<(String, String)>, Vec<String>);

/// How a sweep runs. `pause(stage, what)` runs between the checks and each removal ("remove", the
/// path; "checked", after status; "unlock", before claiming a lock that holds nobody;
/// "lock-claimed", after its atomic rename; "unlocked", after releasing it and before the final
/// guard; "aside", after its artifacts were set aside and before git removes it) or branch deletion
/// ("delete", the branch), and before a build folder is cleared
/// ("judged", the worktree, before the rule is read again; "clear", the folder, before its locks
/// are taken; "locked", the folder, under them): what happens there is what the re-checks guard
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
    /// The build folders of idle work, removed from worktrees that stay ([`build_output`]).
    pub cleared: Vec<Cleared>,
    /// Why that pass removed nothing, when the board could not be read for it; the rest of the
    /// sweep went on.
    pub uncleared: Option<String>,
    pub branches: Vec<String>,
    /// The branches of the removed worktrees, merged into main, deleted with them.
    pub left_branches: Vec<String>,
    /// The branches of removed worktrees that stay, merged into main all the same, and why.
    pub kept_branches: Vec<Kept>,
    /// Of the branches deleted, those a pull request squashed into main, each with how its receipt
    /// says so ([`Delivered::proof`]); the rest are in main's history.
    pub squashed: BTreeMap<String, String>,
    /// The main checkout's folder of evidence set aside ([`aside::swept`]), measured against its
    /// cap ([`kept`]): said by a dry run, and by a sweep while it is past its cap.
    pub set_aside: Option<kept::Measured>,
    /// Why the project's cap could not be read, the default taken.
    pub cap_unread: Option<String>,
    /// The branches of removed worktrees whose comments passed on into their documents stay under
    /// the worktree's name, and the board's refusal ([`Store::held_landed`]): never their words.
    pub unmoved: Vec<(String, String)>,
    pub total: usize,
    pub dry_run: bool,
    /// What the removed worktrees and the cleared build folders held on disk.
    pub freed: u64,
    /// Every worktree the pass visited, as a dry run reports it ([`report`]).
    pub rows: Vec<Row>,
}

impl Sweep {
    /// One line per removal, worktree kept and branch deleted, then (in a dry run) one line per
    /// worktree visited, then the summary.
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
        out.extend(self.cleared.iter().map(|c| {
            format!(
                "{clear} build output {} ({}; {})",
                c.folder.display(),
                c.why,
                size::shown(c.bytes)
            )
        }));
        let deleted = if self.dry_run {
            "would delete"
        } else {
            "deleted"
        };
        let landed = |b: &String| {
            self.squashed
                .get(b)
                .map_or("merged into main", String::as_str)
        };
        out.extend(
            self.left_branches
                .iter()
                .map(|b| format!("{deleted} branch {b} ({}, its worktree removed)", landed(b))),
        );
        out.extend(
            self.branches
                .iter()
                .map(|b| format!("{deleted} branch {b} ({}, no worktree holds it)", landed(b))),
        );
        out.extend(
            self.kept_branches
                .iter()
                .map(|(b, why)| format!("kept branch {b}: {why}")),
        );
        if self.dry_run {
            out.extend(self.rows.iter().map(Row::line));
        }
        out.extend(self.unmoved.iter().map(|(b, why)| {
            format!("the comments passed on into {b} stay under its name, not main's: {why}")
        }));
        if let Some(why) = &self.cap_unread {
            out.push(format!("set aside: {why}; its cap is {} GB", kept::CAP_GB));
        }
        if let Some(measured) = &self.set_aside
            && (self.dry_run || measured.over())
        {
            out.push(measured.line());
        }
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
        if !self.left_branches.is_empty() {
            said += &format!(
                "; {deleted} {} merged branch{} of removed worktrees",
                self.left_branches.len(),
                if self.left_branches.len() == 1 {
                    ""
                } else {
                    "es"
                }
            );
        }
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
                "; {} {} build folder{} of idle work",
                if self.dry_run {
                    "would clear"
                } else {
                    "cleared"
                },
                self.cleared.len(),
                if self.cleared.len() == 1 { "" } else { "s" }
            );
        }
        if let Some(why) = &self.uncleared {
            said += &format!("; cleared no build output: {why}");
        }
        if self.dry_run {
            let held: u64 = self.rows.iter().filter_map(|row| row.bytes).sum();
            let measured = self.rows.iter().filter(|row| row.bytes.is_some()).count();
            said += &format!(
                "; would free {} of the {} its {measured} worktree{} measured hold",
                size::shown(self.freed),
                size::shown(held),
                if measured == 1 { "" } else { "s" }
            );
        } else if !self.removed.is_empty() || !self.cleared.is_empty() {
            said += &format!("; freed {}", size::shown(self.freed));
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

/// Whether a worktree among `paths` is inside `outer`.
fn holds_another(paths: &[PathBuf], outer: &Path) -> bool {
    paths.iter().any(|other| inside(other, outer))
}

/// How long ago `path` was last touched, in seconds, when that is within `window` hours of `now`:
/// the recent activity that keeps a worktree. A window of 0 hours keeps only a touch read as
/// ahead of the clock.
fn touched_within(path: &Path, now: f64, window: f64) -> Option<f64> {
    let touched = now - git::last_touched(path, now);
    (touched < window * 3600.0).then_some(touched)
}

/// A worktree as git lists it at one of [`remove`]'s re-reads, with every worktree's path
/// resolved.
#[derive(Debug, PartialEq)]
struct Relisted {
    entry: git::Listed,
    paths: Vec<PathBuf>,
}

/// The worktree `path` among `trees`, while it is as `judged` was: listed still, at that head on
/// that branch, its folder not gone for git, and no worktree inside `inside_of`, the caller's
/// spelling of its path. None once any of those changed.
fn as_judged(
    trees: Vec<git::Listed>,
    path: &Path,
    judged: &git::Listed,
    inside_of: &Path,
) -> Option<Relisted> {
    let paths: Vec<PathBuf> = trees.iter().map(|t| canonical(&t.path)).collect();
    let entry = trees.into_iter().find(|t| t.path == path)?;
    let changed = entry.head != judged.head
        || entry.branch != judged.branch
        || entry.prunable
        || holds_another(&paths, inside_of);
    (!changed).then_some(Relisted { entry, paths })
}

/// The worktree list read again, and `path` in it while it is [`as_judged`].
fn relisted(
    root: &Path,
    path: &Path,
    judged: &git::Listed,
    inside_of: &Path,
) -> Result<Option<Relisted>, String> {
    Ok(as_judged(git::worktrees(root)?, path, judged, inside_of))
}

/// The board's facts read again, and what they say of the worktree `at` ([`Index::frees`]): none
/// while the board holds it, or while `also`, the caller's own condition on the same read, does.
/// `also` is asked first, whatever the board says.
fn board_frees(
    store: &Store,
    paths: &[PathBuf],
    at: &Path,
    branch: Option<&str>,
    unbound: &BTreeSet<String>,
    also: impl FnOnce(&Index<'_>) -> bool,
) -> Result<Option<bool>, String> {
    let facts = Facts::read(store).map_err(|e| e.to_string())?;
    let index = Index::of(&facts, paths);
    let held = also(&index);
    Ok(index.frees(at, branch, unbound).filter(|_| !held))
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
    /// The custom workflow by whose positions the tasks it holds are finished or still worked on.
    operating: Option<&'a Operating>,
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
            tasks: facts
                .tasks
                .iter()
                .map(|t| (t.slug.as_str(), t))
                .collect_map(),
            spelled: BTreeMap::new(),
            operating: facts.operating.as_ref(),
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

    /// The live or stale session other than `unbound` bound to `path`, if one is.
    fn bound_by(&self, path: &Path, unbound: &BTreeSet<String>) -> Option<&'a str> {
        self.bound
            .iter()
            .find(|(at, handle)| at == path && !unbound.contains(*handle))
            .map(|(_, handle)| *handle)
    }

    /// Whether a live or stale session other than `unbound` is bound to `path`.
    fn bound(&self, path: &Path, unbound: &BTreeSet<String>) -> bool {
        self.bound_by(path, unbound).is_some()
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

    /// The tasks `path` is tied to, each `slug (status)`, for a dry run's report.
    fn stages(&self, path: &Path, branch: Option<&str>) -> Vec<String> {
        self.tied(path, branch)
            .keys()
            .map(|slug| format!("{slug} ({})", self.tasks[slug].status))
            .collect()
    }

    /// Whether task `slug` is finished by its recorded facts ([`operate::finished`]): a task a
    /// custom workflow holds by its terminal position, never the status it projects to.
    fn done(&self, slug: &str) -> bool {
        operate::finished(self.operating, self.tasks[slug])
    }

    /// Whether task `slug` may still be worked on or reviewed: one a custom workflow holds until
    /// its position is terminal, whatever status it projects to; any other at a [`LIVE_TASK`]
    /// status.
    fn working(&self, slug: &str) -> bool {
        let task = self.tasks[slug];
        if crate::store::workflow::position_held(&task.fields) {
            return !self.done(slug);
        }
        LIVE_TASK.contains(&task.status.as_str())
    }

    /// Why a task still live keeps `path`, if one does.
    fn live(&self, path: &Path, branch: Option<&str>) -> Option<String> {
        self.tied(path, branch)
            .into_iter()
            .find(|(slug, _)| self.working(slug))
            .map(|(_, why)| why)
    }

    /// Whether every task `path` is tied to is finished (or it is tied to none).
    fn finished(&self, path: &Path, branch: Option<&str>) -> bool {
        self.tied(path, branch).keys().all(|slug| self.done(slug))
    }

    /// Whether the board lets `path` go, and how: none while a live or stale session other than
    /// `unbound` is bound to it or a task still live keeps it, else whether every task it is tied
    /// to is finished ([`Index::finished`]), which decides what a removal sets aside.
    fn frees(&self, path: &Path, branch: Option<&str>, unbound: &BTreeSet<String>) -> Option<bool> {
        let held = self.bound(path, unbound) || self.live(path, branch).is_some();
        (!held).then(|| self.finished(path, branch))
    }

    /// Why the agent that worked in `path` has finished, as the board shows it: a session is bound
    /// to it (whatever its health), none of them other than `unbound` is live or stale, and every
    /// task tied to it, at least one, is done or cancelled. None while it may still work there, or
    /// the board does not know it.
    fn agent_finished(
        &self,
        path: &Path,
        branch: Option<&str>,
        unbound: &BTreeSet<String>,
    ) -> Option<String> {
        let mut handles = self.sessions_at.get(path).cloned().unwrap_or_default();
        #[expect(
            clippy::disallowed_methods,
            reason = "a busy board path keeps std's in-place sort (crate::order): the sweep"
        )]
        handles.sort_unstable();
        handles.dedup();
        if handles.is_empty() || self.bound(path, unbound) {
            return None;
        }
        let tied = self.tied(path, branch);
        if tied.is_empty() || !tied.keys().all(|slug| self.done(slug)) {
            return None;
        }
        let (sessions, are) = if handles.len() == 1 {
            ("session", "is")
        } else {
            ("sessions", "are")
        };
        Some(format!(
            "its {sessions} {} {are} neither live nor stale, and {}",
            handles.join(", "),
            tied.keys()
                .map(|slug| format!("task {slug} is {}", self.tasks[slug].status))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    /// Why `holder`, the lock of worktree `path`, lets it go (`no lock`, why it is dead, why its
    /// agent finished), or why it keeps it.
    fn lets_go(
        &self,
        holder: &Holder,
        path: &Path,
        branch: Option<&str>,
        unbound: &BTreeSet<String>,
    ) -> Result<String, String> {
        match holder {
            Holder::Nobody => Ok("no lock".into()),
            Holder::Gone(why) => Ok(why.clone()),
            Holder::LandEnded(pid) => Ok(format!(
                "the land's lock, pid {pid}, names a land that ended"
            )),
            Holder::Agent { agent, pid } => {
                if !path.file_name().is_some_and(|name| name == agent.as_str()) {
                    return Err(format!(
                        "locked by Claude Code agent {agent} (pid {pid}), which is not this \
                         worktree's"
                    ));
                }
                match self.agent_finished(path, branch, unbound) {
                    Some(why) => Ok(format!(
                        "Claude Code's lock of pid {pid}, the session that started {agent} and \
                         outlives it: {why}"
                    )),
                    None => Err(format!(
                        "locked by Claude Code agent {agent} of the live process {pid}, and the \
                         board does not show that agent finished"
                    )),
                }
            }
            Holder::Land(pid) => Err(format!("a land runs there (pid {pid})")),
            Holder::Release => {
                Err("the release run's checkout, which it reuses on each run".into())
            }
            Holder::Foreign(reason) => Err(format!(
                "locked by something the sweep does not know: {reason:?}"
            )),
        }
    }
}

/// Whether `head` is in `main`, once per commit a sweep.
fn merged(root: &Path, head: &str, main: &str, ancestry: &mut BTreeMap<String, bool>) -> bool {
    !head.is_empty()
        && *ancestry
            .entry(head.to_string())
            .or_insert_with(|| git::ancestor(root, head, main))
}

/// How `head` landed in `main`, if it did: in its history, or squashed into it by a pull request,
/// which only a receipt git bears out says ([`delivered`]).
fn landed(
    root: &Path,
    head: &str,
    main: &str,
    ancestry: &mut BTreeMap<String, bool>,
    delivered: &mut Delivered,
) -> Option<String> {
    if merged(root, head, main, ancestry) {
        return Some("merged into main".into());
    }
    delivered.proof(head, main)
}

/// What a dry run says of `head` and `main`: merged, squashed into it, or how many commits it has
/// that main has not.
fn merge_said(
    root: &Path,
    head: &str,
    main: &str,
    ancestry: &mut BTreeMap<String, bool>,
    delivered: &mut Delivered,
) -> String {
    if head.is_empty() {
        return "no HEAD".into();
    }
    if let Some(how) = landed(root, head, main, ancestry, delivered) {
        return how;
    }
    let range = format!("{main}..{head}");
    match git::stdout(root, &["rev-list", "--count", &range]).map(|n| n.trim().to_string()) {
        Some(n) if n == "1" => "not merged: 1 commit not in main".into(),
        Some(n) if !n.is_empty() => format!("not merged: {n} commits not in main"),
        _ => "not merged into main".into(),
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
    // Where a removed worktree's artifacts go: the main checkout, which git lists first, in the
    // artifacts folder its project file names. With none read, no folder is evidence, and a
    // worktree holding one keeps it.
    let checkout = trees.first().filter(|t| !t.bare).map(|t| t.path.clone());
    let artifacts = checkout.as_deref().and_then(crate::project::artifacts);
    let facts = Facts::read(store).map_err(|e| e.to_string())?;
    let index = Index::of(&facts, &paths);
    let nobody = BTreeSet::new();
    // The processes' directories, asked once and only when a worktree gets that far.
    let mut working: Option<Option<Vec<PathBuf>>> = None;
    let mut gone: BTreeSet<PathBuf> = BTreeSet::new();
    let mut freed_refs: Vec<(PathBuf, String, String)> = Vec::new();
    let mut ancestry = BTreeMap::new();
    let mut delivered = Delivered::of(root);
    // What this pass set aside, by its folder's name: the tasks tied to its worktree.
    let mut set_aside_from: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // Where the owner's answers are written, as the main checkout's project file names its
    // documents: every folder when there is no checkout to read it from.
    let documents = checkout
        .as_deref()
        .map_or_else(|| vec![String::new()], crate::project::documents_at);
    // The ignored paths the project itself names as going with a merged worktree, read from the
    // main checkout's file as the artifacts folder is, so a branch cannot name its own.
    let disposable = checkout
        .as_deref()
        .map(crate::project::disposable)
        .unwrap_or_default();
    let verdicts = verdicts::Verdicts::new(
        root,
        o.dry_run,
        artifacts.as_deref(),
        documents,
        disposable.clone(),
    );
    for (number, (entry, path)) in trees.iter().zip(&paths).enumerate() {
        if o.only.as_ref().is_some_and(|only| !only.contains(path)) {
            continue;
        }
        let listed = &entry.path;
        let branch = entry
            .branch
            .as_deref()
            .and_then(|b| b.strip_prefix("refs/heads/"));
        // What a dry run says each linked worktree holds, measured once.
        let held = (o.dry_run && number != 0).then(|| size::bytes(path));
        let verdict: String = 'judged: {
            if number == 0 {
                break 'judged "kept: the main checkout".into();
            }
            if *path == here {
                break 'judged "kept: the sweep runs there".into();
            }
            if entry.bare || entry.prunable || !path.is_dir() {
                break 'judged "kept: its folder is gone; git worktree prune forgets it".into();
            }
            if entry.branch.as_deref() == Some("refs/heads/main") {
                break 'judged "kept: main is checked out there".into();
            }
            if holds_another(&paths, path) {
                break 'judged "kept: another worktree is inside it".into();
            }
            let finishing = o.finished.covers(path, branch);
            let window = if finishing { 0.0 } else { o.recent_hours };
            if let Some(touched) = touched_within(path, (o.now)(), window) {
                break 'judged format!(
                    "kept: touched {:.0} min ago, within the {window} h window of recent activity",
                    touched.max(0.0) / 60.0
                );
            }
            let unbound = if finishing {
                &o.finished.handles
            } else {
                &nobody
            };
            if let Some(handle) = index.bound_by(path, unbound) {
                break 'judged format!("kept: session {handle} is bound there, live or stale");
            }
            let head = entry.head.as_str();
            let Some(how) = landed(root, head, &main, &mut ancestry, &mut delivered) else {
                break 'judged "kept for an orchestrator to decide: its HEAD is not in main".into();
            };
            if let Some(held) = index.live(path, branch) {
                result.spared.push((listed.clone(), held.clone()));
                break 'judged format!("kept: {held}");
            }
            let holder = Holder::of(entry.locked.as_deref(), &*o.alive, &*o.started);
            let lock = match index.lets_go(&holder, path, branch, unbound) {
                Ok(lock) => lock,
                Err(why) => {
                    result.spared.push((listed.clone(), why.clone()));
                    break 'judged format!("kept: {why}");
                }
            };
            let _removal_guard = match guards::acquire(path, o.dry_run) {
                Ok(held) => held,
                Err(why) => {
                    result.spared.push((listed.clone(), why.clone()));
                    break 'judged format!("kept: {why}");
                }
            };
            let seen = working.get_or_insert_with(|| {
                (o.cwds)()
                    .filter(|cwds| !cwds.is_empty())
                    .map(|cwds| cwds.iter().map(|w| canonical(w)).collect())
            });
            let occupied = match seen {
                None => Some("cannot tell whether a process is working inside it"),
                Some(cwds) if cwds.iter().any(|w| w.starts_with(path)) => {
                    Some("a process is working inside it")
                }
                Some(_) => None,
            };
            if let Some(why) = occupied {
                result.spared.push((listed.clone(), why.into()));
                break 'judged format!("kept: {why}");
            }
            let finished = index.finished(path, branch);
            let verdict = verdicts.read(entry, finished);
            let owner = verdict.owner;
            if !owner.is_empty() {
                let why = format!("holds an owner verdict no commit has: {}", owner.join(", "));
                result.spared.push((listed.clone(), why.clone()));
                break 'judged format!("kept: {why}");
            }
            let left = verdict.left;
            if !left.is_empty() {
                let shown = left.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
                result.kept.push((listed.clone(), left));
                break 'judged format!(
                    "kept for an orchestrator to decide: holds files that did not land: {shown}"
                );
            }
            let place = branch.map_or_else(|| "detached".to_string(), |b| format!("on {b}"));
            let short = head.get(..12).unwrap_or(head);
            if !o.dry_run {
                (o.pause)("remove", &listed.to_string_lossy());
            }
            let kept_in = Evidence {
                checkout: checkout.as_deref(),
                folder: artifacts.as_deref(),
                disposable: &disposable,
            };
            let went = match remove(root, listed, entry, store, o, kept_in, held)? {
                Removal::Went(went) => went,
                Removal::Kept(why) => {
                    result.spared.push((listed.clone(), why.clone()));
                    break 'judged format!("kept: {why}");
                }
                Removal::Changed => {
                    break 'judged
                        "kept: it changed while the sweep looked; the next pass reads it again"
                            .into();
                }
            };
            // Gone, all it held in main: the comments passed into its documents, kept under its id
            // (its branch, `/` spelled `~`, as the Symphony names a worktree), are main's now. Not
            // while another listed worktree holds its branch (`git worktree add --force`), whose
            // id they share: a pass may have written into that one, and a line main lacks would
            // count as main's. The last of them to go moves them.
            let shared = (trees.iter().zip(&paths)).any(|(other, at)| {
                at != path && !gone.contains(at) && other.branch == entry.branch
            });
            if !o.dry_run
                && !shared
                && let Some(branch) = branch
                && let Err(error) = store.held_landed(&branch.replace('/', "~"))
            {
                result.unmoved.push((branch.to_string(), error.to_string()));
            }
            let mut why = format!("{place} at {short}, {how}, clean, {lock}");
            if let (Some(to), Some(folder)) = (&went.aside, &artifacts) {
                let moved = if o.dry_run { "would move" } else { "moved" };
                why += &format!("; its {folder}/ {moved} to {}", to.display());
                if let (false, Some(name)) = (o.dry_run, to.file_name()) {
                    let tasks = index.tied(path, branch).into_keys().map(str::to_string);
                    set_aside_from.insert(name.to_string_lossy().into_owned(), tasks.collect());
                }
            }
            why += &format!("; {}", size::shown(went.bytes));
            result.freed += went.bytes;
            result.removed.push((listed.clone(), why.clone()));
            gone.insert(path.clone());
            if let Some(reference) = &entry.branch {
                freed_refs.push((path.clone(), reference.clone(), entry.head.clone()));
            }
            let verb = if o.dry_run { "would remove" } else { "removed" };
            break 'judged format!("{verb}: {why}");
        };
        result.rows.push(Row {
            path: listed.clone(),
            bytes: held,
            tasks: index.stages(path, branch),
            merged: o
                .dry_run
                .then(|| merge_said(root, &entry.head, &main, &mut ancestry, &mut delivered)),
            verdict,
        });
    }
    // A board that cannot be read for it costs that pass alone, never the removals said above, the
    // prune or the branches.
    match build_output::clear(root, &here, store, o, &gone, &mut working, &trees) {
        Ok(cleared) => result.cleared = cleared,
        Err(why) => result.uncleared = Some(why),
    }
    let clear = if o.dry_run { "would clear" } else { "cleared" };
    for cleared in &result.cleared {
        result.freed += cleared.bytes;
        // Its worktree's row: the deepest that holds it, as both are listed, since a worktree may
        // lie inside the main checkout's folder (`.claude/worktrees/agent-ID`).
        if let Some(row) = result
            .rows
            .iter_mut()
            .filter(|row| cleared.folder.starts_with(&row.path))
            .max_by_key(|row| row.path.components().count())
        {
            row.verdict += &format!(
                "; {clear} its build output {} ({}): {}",
                cleared.folder.display(),
                size::shown(cleared.bytes),
                cleared.why
            );
        }
    }
    if !o.dry_run && o.only.is_none() {
        let _ = git::git(root, &["worktree", "prune"]);
    }
    let mut landings = Landings {
        main: &main,
        ancestry: &mut ancestry,
        delivered: &mut delivered,
        squashed: BTreeMap::new(),
    };
    (result.left_branches, result.kept_branches) =
        left_branches(root, &mut landings, &freed_refs, store, o)?;
    if o.only.is_none() {
        result.branches = agent_branches(root, &mut landings, &trees, &paths, &gone, store, o)?;
    }
    result.squashed = landings.squashed;
    (result.squashed)
        .retain(|b, _| result.left_branches.contains(b) || result.branches.contains(b));
    if let (Some(checkout), Some(folder)) = (&checkout, &artifacts) {
        let stage = |slug: &str| index.tasks.get(slug).map(|task| task.status.clone());
        let open: Vec<String> = (index.tasks.keys())
            .filter(|slug| !index.done(slug))
            .map(|slug| slug.to_string())
            .collect();
        let read = || mentions(root, &main, folder, store, &open);
        (result.set_aside, result.cap_unread) =
            measure_set_aside(root, checkout, folder, set_aside_from, &stage, &read, o);
    }
    Ok(result)
}

/// The documents whose lines [`mentions`] reads, on the default branch.
const DOCS: &str = "docs/";

/// What may still name a path under the artifacts folder `folder`, read once for a measure past
/// its cap, each with the place it is said in: the posts on the threads of the `open` tasks that
/// name the folder, one read of the board, and the lines of [`DOCS`] that do as the default
/// branch's commit `main` holds them, one `git grep`; with what could not be read, and why.
fn mentions(root: &Path, main: &str, folder: &str, store: &Store, open: &[String]) -> Said {
    let needle = format!("{folder}/");
    let (mut said, mut unsearched) = (Vec::new(), Vec::new());
    match store.threads(open) {
        Ok(threads) => {
            for (slug, posts) in threads {
                said.extend(
                    posts
                        .into_iter()
                        .filter(|post| post.text.contains(&needle))
                        .map(|post| (format!("the thread of {slug}"), post.text)),
                );
            }
        }
        Err(error) => {
            unsearched.push(format!(
                "the open tasks' threads could not be read: {error}"
            ));
        }
    }
    match docs_naming(root, main, &needle) {
        Ok(lines) => said.extend(lines),
        Err(why) => unsearched.push(format!("{DOCS} on main could not be searched: {why}")),
    }
    (said, unsearched)
}

/// The lines of [`DOCS`] holding `needle` as the commit `main` holds them, each with its file's
/// path; why `git grep` could not say, when it could not.
fn docs_naming(root: &Path, main: &str, needle: &str) -> Result<Vec<(String, String)>, String> {
    // `-z` ends each file's name, `COMMIT:PATH`, with a NUL; git grep exits 1 when nothing matches.
    let done = git::git(
        root,
        &["grep", "-I", "-z", "-F", "-e", needle, main, "--", DOCS],
    )
    .ok_or("git did not start")?;
    if !(done.status.success() || done.status.code() == Some(1)) {
        return Err(String::from_utf8_lossy(&done.stderr).trim().to_string());
    }
    let commit = format!("{main}:");
    Ok(String::from_utf8_lossy(&done.stdout)
        .lines()
        .filter_map(|line| {
            let (file, text) = line.split_once('\0')?;
            let file = file.strip_prefix(&commit).unwrap_or(file);
            Some((file.to_string(), text.to_string()))
        })
        .collect())
}

/// The main checkout `main`'s folder of evidence set aside, measured against the project's cap
/// ([`kept`]), and why the cap could not be read, if it could not. Past the cap, what still
/// references each of its largest folders is looked up in what `mentions` reads, once. A sweep that
/// is no dry run keeps the measure in its record, beside where each folder there came from (`from`,
/// this pass's, added to the record's; a folder no longer there forgotten); a dry run writes
/// nothing.
fn measure_set_aside(
    root: &Path,
    main: &Path,
    folder: &str,
    from: BTreeMap<String, Vec<String>>,
    stage: &dyn Fn(&str) -> Option<String>,
    mentions: &dyn Fn() -> Said,
    o: &Options,
) -> (Option<kept::Measured>, Option<String>) {
    let (cap, unread) = match kept::cap(main) {
        Ok(cap) => (cap, None),
        Err(why) => (kept::CAP_GB, Some(why)),
    };
    let swept = aside::swept(main, folder);
    let record_at = crate::git::common_dir(root)
        .ok()
        .map(|common| kept::path(&common));
    let mut record = record_at
        .as_deref()
        .map(kept::Record::read)
        .unwrap_or_default();
    record.from.extend(from);
    record
        .from
        .retain(|name, _| std::fs::symlink_metadata(swept.join(name)).is_ok());
    let mut measured = kept::measure(&swept, cap, &record.from, stage, (o.now)());
    if measured.over() {
        let (said, unsearched) = mentions();
        measured.look_up(folder, &said, unsearched);
    }
    if !o.dry_run
        && let Some(at) = &record_at
    {
        record.measured = Some(measured.clone());
        let _ = record.write(at);
    }
    (Some(measured), unread)
}

/// A worktree the sweep removed, or in a dry run would: what went on disk, and where its artifacts
/// were set aside.
struct Went {
    bytes: u64,
    aside: Option<PathBuf>,
}

/// What [`remove`] came to.
enum Removal {
    /// Removed, or in a dry run would be.
    Went(Went),
    /// It changed while the sweep looked: the next pass reads it again.
    Changed,
    /// It stays, for this reason, until it changes.
    Kept(String),
}

/// Where a finished worktree's evidence is kept: the main checkout, in the project's artifacts
/// folder. Either none, and a worktree holding such evidence stays.
#[derive(Clone, Copy)]
struct Evidence<'a> {
    checkout: Option<&'a Path>,
    folder: Option<&'a str>,
    /// The ignored paths the project names as going with a merged worktree, which are no evidence.
    disposable: &'a [String],
}

/// Remove `path` if it is still at `head` with nothing left behind; git checks cleanliness again
/// itself (no `--force`). Only the exact lock judged to hold nobody is lifted; refusal restores it
/// without replacing another owner's lock. The caller holds the per-tree removal guard throughout.
/// A finished worktree's artifacts are set aside where `kept_in` says, right before git removes
/// it, whatever came there since it was judged, and moved back if git refuses; a set-aside that
/// cannot be made (another disk, a name taken) keeps the worktree, saying why. What it holds is
/// measured before any lock is lifted or taken; `held` is what a dry run measured already.
fn remove(
    root: &Path,
    path: &Path,
    judged: &git::Listed,
    store: &Store,
    o: &Options,
    kept_in: Evidence<'_>,
    held: Option<u64>,
) -> Result<Removal, String> {
    let shown = path.to_string_lossy();
    // Re-read after the slow, shared scan and earlier removals: a launch, checkout or lock
    // acquired in that window must protect this tree, even when its HEAD is still merged.
    let Some(Relisted { entry, paths }) = relisted(root, path, judged, path)? else {
        return Ok(Removal::Changed);
    };
    // Asked here alone: this is the first read of the folder since it was judged, and the two
    // re-reads below each follow one that fails on a folder gone (the status of `left_behind`,
    // HEAD held by `hold_head`).
    if !path.is_dir() {
        return Ok(Removal::Changed);
    }
    let nobody = BTreeSet::new();
    let at = canonical(path);
    let branch = entry
        .branch
        .as_deref()
        .and_then(|b| b.strip_prefix("refs/heads/"));
    let finishing = o.finished.covers(&at, branch);
    let unbound = if finishing {
        &o.finished.handles
    } else {
        &nobody
    };
    // The first read of the board here says whether its tasks are finished, which the two below
    // compare theirs with. It and the next ask the board by `path`, as git lists it, and the last
    // by `at`, its resolved path, which is how the board's facts are keyed: the two are one path
    // wherever git lists a worktree resolved.
    let Some(finished) = board_frees(store, &paths, path, branch, unbound, |_| false)? else {
        return Ok(Removal::Changed);
    };
    if !git::left_behind_with(path, finished, kept_in.folder, kept_in.disposable).is_empty() {
        return Ok(Removal::Changed);
    }
    (o.pause)("checked", &shown);
    // After the status: a commit or checkout made while it ran leaves the tree clean. The
    // lock must also be the current one before we decide whether to lift it: so a lock is no
    // change here, unlike at the last re-read, and no status follows, the one above having just
    // run.
    let Some(Relisted { entry, paths }) = relisted(root, path, judged, path)? else {
        return Ok(Removal::Changed);
    };
    let lock = entry.locked.as_deref();
    let observed_lock = match lock {
        Some(reason) => {
            let Some(observed) = git::WorktreeLock::capture(path, reason) else {
                return Ok(Removal::Changed);
            };
            Some(observed)
        }
        None => None,
    };
    // Only here is the lock's holder asked again, on the same read of the board as the rest: it
    // is the lock just captured, which the release below lifts.
    let lock_keeps = |index: &Index<'_>| {
        let holder = Holder::of(lock, &*o.alive, &*o.started);
        index.lets_go(&holder, &at, branch, unbound).is_err()
    };
    if board_frees(store, &paths, path, branch, unbound, lock_keeps)? != Some(finished) {
        return Ok(Removal::Changed);
    }
    // The window is asked from here on, not at the first re-read: the sweep's judging asked it
    // last, and what was touched since shows after the status above.
    let window = if finishing { 0.0 } else { o.recent_hours };
    if touched_within(path, (o.now)(), window).is_some() {
        return Ok(Removal::Changed);
    }
    // A finished worktree's evidence is set aside, never removed with it. Listed now to say where
    // it goes and what stays on disk; set aside right before the removal, listed again then, when
    // only what is new or changed since is opened again.
    let keep_in = match (finished, kept_in.checkout, kept_in.folder) {
        (true, Some(main), Some(folder)) => Some((main, folder)),
        _ => None,
    };
    let evidence = match keep_in {
        Some((_, folder)) => match aside::list(path, folder, aside::Seen::default()) {
            Ok(evidence) => evidence,
            Err(why) => return Ok(Removal::Kept(why)),
        },
        None => aside::Listed::default(),
    };
    if let (Some((main, folder)), false) = (keep_in, evidence.entries.is_empty())
        && let Err(why) = aside::movable(main, folder, path)
    {
        return Ok(Removal::Kept(why));
    }
    // Measured here, before a lock is lifted or HEAD held: a big worktree takes a while to walk.
    let bytes = held
        .unwrap_or_else(|| size::bytes(path))
        .saturating_sub(aside::bytes(path, &evidence.entries));
    if o.dry_run {
        if (o.occupied)(path) != Some(false) {
            return Ok(Removal::Changed);
        }
        let to = keep_in
            .filter(|_| !evidence.entries.is_empty())
            .and_then(|(main, folder)| aside::destination(main, folder, path));
        return Ok(Removal::Went(Went { bytes, aside: to }));
    }
    let _released_lock = match observed_lock {
        Some(observed) => match observed.release(|stage| (o.pause)(stage, &shown)) {
            Some(released) => Some(released),
            None => return Ok(Removal::Changed),
        },
        None => None,
    };
    (o.pause)("unlocked", &shown);
    let Some(_head) = git::hold_head(path) else {
        return Ok(Removal::Changed);
    };
    // The last snapshot is taken with HEAD and its branch held through git's removal.
    // A clean detached commit after the earlier status must not lose its only reflog.
    // A worktree inside it is looked for under `at`, its resolved path, where the two re-reads
    // above look under `path`, as git lists it; see the first read of the board.
    let Some(Relisted { entry, paths }) = relisted(root, path, judged, &at)? else {
        return Ok(Removal::Changed);
    };
    // Any lock is a change here alone: the one judged was lifted above, so this one was taken
    // since, by someone else.
    if entry.locked.is_some() {
        return Ok(Removal::Changed);
    }
    if board_frees(store, &paths, &at, branch, unbound, |_| false)? != Some(finished) {
        return Ok(Removal::Changed);
    }
    if !git::left_behind_with(path, finished, kept_in.folder, kept_in.disposable).is_empty() {
        return Ok(Removal::Changed);
    }
    // The processes are asked here alone (and by a dry run, which stops before this): theirs is
    // the last scan, after which nothing is read again.
    if touched_within(path, (o.now)(), window).is_some() || (o.occupied)(path) != Some(false) {
        return Ok(Removal::Changed);
    }
    // No status or board scan after this tree's process scan. Ref writers remain excluded
    // until removal completes; git itself checks tracked and untracked changes once more. What
    // its artifacts folder holds now goes aside, whatever came since the listing above, which
    // read every file there: this look under the locks opens only those new or changed since.
    let set_aside = match keep_in {
        Some((main, folder)) => match aside::Aside::take(main, folder, path, evidence.seen) {
            Ok(set_aside) => set_aside,
            Err(why) => return Ok(Removal::Kept(why)),
        },
        None => None,
    };
    if set_aside.is_some() {
        (o.pause)("aside", &shown);
    }
    let removed = git::clean_regenerable(path)
        && git::succeeds(root, &["worktree", "remove", &shown])
        && !path.exists();
    if !removed {
        if let Some(set_aside) = set_aside {
            set_aside.put_back();
        }
        return Ok(Removal::Changed);
    }
    Ok(Removal::Went(Went {
        bytes,
        aside: set_aside.map(|set_aside| set_aside.to),
    }))
}

/// What a sweep's branch deletions judge a landing by: main's commit, and the two proofs of a
/// landing in it, each read once per commit ([`landed`]).
struct Landings<'a> {
    main: &'a str,
    ancestry: &'a mut BTreeMap<String, bool>,
    delivered: &'a mut Delivered,
    /// The branches found squashed into main, with how their receipts say so.
    squashed: BTreeMap<String, String>,
}

impl Landings<'_> {
    /// Whether the branch `name` at `sha` landed in main, noting one a pull request squashed.
    fn holds(&mut self, root: &Path, name: &str, sha: &str) -> bool {
        if merged(root, sha, self.main, self.ancestry) {
            return true;
        }
        match self.delivered.proof(sha, self.main) {
            Some(said) => {
                self.squashed.insert(name.to_string(), said);
                true
            }
            None => false,
        }
    }
}

/// The tasks a land's record of its merge names, in the lands' folder of `root`'s repository
/// ([`crate::land::Record`]): each a land interrupted after it moved the default branch, which `just
/// land SLUG` run again finishes from the record and the task's branch. A record counts under the
/// name a land keeps it by, or the dashed one a land before kept it by, which `just land` still
/// finishes from ([`crate::land::Record::read`]), each for the task it names.
fn recorded_lands(root: &Path) -> BTreeSet<String> {
    let Ok(common) = crate::git::common_dir(root) else {
        return BTreeSet::new();
    };
    let folder = trigger::folder(&common).join(crate::land::FOLDER);
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return BTreeSet::new();
    };
    let mut slugs = BTreeSet::new();
    slugs.extend(entries.flatten().filter_map(|entry| {
        let text = std::fs::read_to_string(entry.path()).ok()?;
        let record: crate::land::Record = crate::objects::parse(&text)?;
        let at = entry.path();
        (at == crate::land::Record::path(&folder, &record.slug)
            || at == crate::land::Record::dashed(&folder, &record.slug))
        .then_some(record.slug)
    }));
    slugs
}

/// Why branch `name`, landed in main, stays though its worktree went, if it does: a land's
/// record of its merge names it (`recorded`), which a land run again finishes from it; a delivery
/// of it by pull request not yet asked to merge reads its tip ([`Delivered::under_way`]); or its
/// task is done by an approve no landing has closed, which may still go back to review for a
/// fresh approve, and the guard against sending back what landed reads the branch's tip
/// (`crate::land::reopenable`).
fn branch_kept(
    store: &Store,
    name: &str,
    recorded: &BTreeSet<String>,
    delivered: &Delivered,
) -> Option<String> {
    if recorded.contains(name) {
        return Some(format!(
            "a land's record of its merge names it, and `just land {name}` run again finishes that \
             land from it"
        ));
    }
    if let Some(why) = delivered.under_way(name) {
        return Some(why);
    }
    let task = store.get(name).ok()?;
    if task.status != "done" || task.parent.is_some() || task.instruction {
        return None;
    }
    // A thread that cannot be read keeps it, as a standing approve would.
    let approve = store
        .thread(name)
        .map(|thread| crate::approved(&thread).is_some())
        .unwrap_or(true);
    approve.then(|| {
        format!(
            "task {name} is done by an approve no landing has closed, and what keeps it from going \
             back to review once its tip is on main is that tip"
        )
    })
}

/// Delete the branch each removed worktree had checked out (`refs`: its folder, the branch, the
/// commit it was at), once landed in main ([`landed`]), if it still points there and no worktree
/// but the one removed holds it, unless the land or the board still needs it ([`branch_kept`]):
/// those deleted, and those kept with why. The `worktree-agent-*` branches are [`agent_branches`]'.
fn left_branches(
    root: &Path,
    landings: &mut Landings<'_>,
    refs: &[(PathBuf, String, String)],
    store: &Store,
    o: &Options,
) -> Result<(Vec<String>, Vec<Kept>), String> {
    if refs.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let current = git::worktrees(root)?;
    let recorded = recorded_lands(root);
    let (mut deleted, mut kept) = (Vec::new(), Vec::new());
    for (removed, reference, sha) in refs {
        let Some(name) = reference.strip_prefix("refs/heads/") else {
            continue;
        };
        if name == "main" || reference.starts_with("refs/heads/worktree-agent-") {
            continue;
        }
        let held = current.iter().any(|t| {
            t.branch.as_deref() == Some(reference.as_str()) && canonical(&t.path) != *removed
        });
        if held
            || git::rev(root, reference).as_deref() != Some(sha.as_str())
            || !landings.holds(root, name, sha)
        {
            continue;
        }
        if let Some(why) = branch_kept(store, name, &recorded, landings.delivered) {
            kept.push((name.to_string(), why));
            continue;
        }
        if !o.dry_run {
            (o.pause)("delete", name);
        }
        if o.dry_run || git::succeeds(root, &["update-ref", "-d", reference, sha]) {
            deleted.push(name.to_string());
        }
    }
    Ok((deleted, kept))
}

/// Delete each `worktree-agent-*` branch landed in main ([`landed`]) that no worktree left holds,
/// only if it still points where it did when listed: a branch that moved since is not what was
/// judged landed.
fn agent_branches(
    root: &Path,
    landings: &mut Landings<'_>,
    trees: &[git::Listed],
    paths: &[PathBuf],
    gone: &BTreeSet<PathBuf>,
    store: &Store,
    o: &Options,
) -> Result<Vec<String>, String> {
    let mut held: BTreeSet<&str> = BTreeSet::new();
    held.extend(
        trees
            .iter()
            .zip(paths)
            .filter(|(_, path)| !gone.contains(*path))
            .filter_map(|(entry, _)| entry.branch.as_deref()),
    );
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
        let name = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        if held.contains(reference) || !landings.holds(root, name, sha) {
            continue;
        }
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

/// The window of recent activity: the flag, else `SYMPHONY_SWEEP_RECENT_HOURS`, else 2 h; refused
/// when it is not a number of hours, 0 or more. A blank value is unset.
pub fn recent_hours(vars: &BTreeMap<String, String>, flag: Option<f64>) -> Result<f64, String> {
    let named = || {
        symphony_env::SWEEP_RECENT_HOURS
            .get(|name| vars.get(name).map(|value| value.trim()))
            .filter(|value| !value.is_empty())
    };
    let hours = match flag {
        Some(hours) => hours,
        None => match named() {
            Some(text) => text
                .parse::<f64>()
                .map_err(|e| format!("{RECENT_ENV}={text:?} is not a number of hours ({e})"))?,
            None => RECENT_HOURS,
        },
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
    use crate::children::Child as _;

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
            said.starts_with("SYMPHONY_SWEEP_RECENT_HOURS=\"soon\" is not a number of hours"),
            "{said}"
        );
        for wrong in ["-0.5", "NaN"] {
            assert!(recent_hours(&vars(Some(wrong)), None).is_err(), "{wrong}");
        }
    }

    #[test]
    fn documents_that_cannot_be_searched_say_why() {
        let gone =
            std::env::temp_dir().join(format!("symphony-sweep-docs-gone-{}", std::process::id()));
        assert_eq!(
            docs_naming(&gone, "main", "artifacts/"),
            Err("git did not start".to_string())
        );
        let bare =
            std::env::temp_dir().join(format!("symphony-sweep-docs-none-{}", std::process::id()));
        std::fs::create_dir_all(&bare).unwrap();
        let why = docs_naming(&bare, "main", "artifacts/").unwrap_err();
        assert!(why.contains("not a git repository"), "{why}");
        std::fs::remove_dir_all(&bare).unwrap();
    }

    #[test]
    fn a_lands_record_counts_by_its_name_or_the_dashed_name_a_land_before_kept_it_by() {
        let root =
            std::env::temp_dir().join(format!("symphony-sweep-records-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let init = std::process::Command::of("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(init.status.success(), "{init:?}");
        assert!(recorded_lands(&root).is_empty(), "no lands' folder yet");
        let folder = trigger::folder(&root.join(".git")).join(crate::land::FOLDER);
        std::fs::create_dir_all(&folder).unwrap();
        let write = |path: PathBuf, slug: &str| {
            let record = crate::land::Record {
                slug: slug.into(),
                tip: "a".repeat(40),
                merge: "b".repeat(40),
                checked: "b".repeat(40),
                regenerated: Vec::new(),
            };
            std::fs::write(path, serde_json::to_string(&record).unwrap()).unwrap();
        };
        write(
            crate::land::Record::path(&folder, "tools/a-b/c"),
            "tools/a-b/c",
        );
        // A land before kept it by the dashed name, which `just land` still finishes from.
        write(crate::land::Record::dashed(&folder, "sim/jump"), "sim/jump");
        // None of these is a task's own record: one under another task's name, one under a dashed
        // name its slug does not give, and what is no record.
        write(folder.join("tools-other.merged"), "tools/stray");
        write(folder.join("sim-jump-2.merged"), "sim/jump/3");
        std::fs::write(folder.join("running"), "{\"id\": 1}").unwrap();
        std::fs::write(folder.join("tools-x.merged"), "not a record").unwrap();
        assert_eq!(
            recorded_lands(&root),
            BTreeSet::from(["sim/jump".to_string(), "tools/a-b/c".to_string()])
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_task_a_workflow_holds_is_finished_by_its_terminal_position_never_its_projected_status() {
        use crate::workflow::operate::tests::{held, legacy, reviewed};
        let launched = |slug: &str| TaskSessionRow {
            slug: slug.into(),
            handle: "amber".into(),
            worktree: format!("/nowhere/{slug}"),
            ..TaskSessionRow::default()
        };
        let tasks = vec![
            // Approved: it projects to done, yet its position is no terminal one.
            held("sim/approved", "verify", "approved"),
            held("sim/accepted", "verify", "accepted"),
            held("sim/cancelled", "build", "cancelled"),
            held("sim/awaiting", "verify", "awaiting"),
            legacy("sim/done", "done"),
            legacy("sim/review", "review"),
        ];
        let facts = Facts {
            launches: tasks.iter().map(|t| launched(&t.slug)).collect(),
            tasks,
            sessions: Vec::new(),
            operating: Some(reviewed()),
        };
        let index = Index::of(&facts, &[]);
        for (slug, finished) in [
            ("sim/approved", false),
            ("sim/accepted", true),
            ("sim/cancelled", true),
            ("sim/awaiting", false),
            ("sim/done", true),
            ("sim/review", false),
        ] {
            let at = canonical(Path::new(&format!("/nowhere/{slug}")));
            assert_eq!(index.finished(&at, None), finished, "{slug}");
            assert_eq!(index.live(&at, None).is_some(), !finished, "{slug}");
        }
        // With no workflow read, a held task is never taken as finished by its status.
        let unread = Facts {
            operating: None,
            ..facts.clone()
        };
        let index = Index::of(&unread, &[]);
        let at = canonical(Path::new("/nowhere/sim/approved"));
        assert!(!index.finished(&at, None));
    }

    fn listed(path: &str, head: &str, branch: Option<&str>) -> git::Listed {
        git::Listed {
            path: path.into(),
            head: head.into(),
            branch: branch.map(|b| format!("refs/heads/{b}")),
            ..git::Listed::default()
        }
    }

    #[test]
    fn a_worktree_is_as_judged_until_its_head_branch_folder_or_what_it_holds_changes() {
        let at = Path::new("/nowhere/wt/a");
        let judged = listed("/nowhere/wt/a", "aaa", Some("sim/a"));
        let main = listed("/nowhere/main", "mmm", Some("main"));
        let beside = listed("/nowhere/wt/ab", "bbb", None);
        let trees = vec![main.clone(), judged.clone(), beside.clone()];
        // Unchanged: its entry, and every worktree's path in git's order. A path that only starts
        // with its name is beside it, not inside it, and its own path is not inside itself.
        assert_eq!(
            as_judged(trees.clone(), at, &judged, at),
            Some(Relisted {
                entry: judged.clone(),
                paths: ["/nowhere/main", "/nowhere/wt/a", "/nowhere/wt/ab"]
                    .map(PathBuf::from)
                    .to_vec(),
            })
        );
        // A lock is the caller's to judge: it comes back on the entry.
        let locked = git::Listed {
            locked: Some("claude agent agent-a (pid 1)".into()),
            ..judged.clone()
        };
        let found = as_judged(vec![main.clone(), locked.clone()], at, &judged, at);
        assert_eq!(found.map(|found| found.entry), Some(locked));
        let changed = [
            ("no longer listed", vec![main.clone(), beside.clone()]),
            ("nothing listed", Vec::new()),
            (
                "another head",
                vec![main.clone(), listed("/nowhere/wt/a", "ccc", Some("sim/a"))],
            ),
            (
                "another branch",
                vec![main.clone(), listed("/nowhere/wt/a", "aaa", Some("sim/b"))],
            ),
            (
                "detached",
                vec![main.clone(), listed("/nowhere/wt/a", "aaa", None)],
            ),
            (
                "prunable",
                vec![
                    main.clone(),
                    git::Listed {
                        prunable: true,
                        ..judged.clone()
                    },
                ],
            ),
            (
                "a worktree inside it",
                vec![
                    main.clone(),
                    judged.clone(),
                    listed("/nowhere/wt/a/.claude/worktrees/agent-x", "ddd", None),
                ],
            ),
        ];
        for (what, trees) in changed {
            assert_eq!(as_judged(trees, at, &judged, at), None, "{what}");
        }
        // The worktree inside is looked for under the path the caller names, which need not be
        // the one it is listed by.
        let inside_other = vec![main, judged.clone(), listed("/elsewhere/a/in", "ddd", None)];
        assert!(as_judged(inside_other.clone(), at, &judged, at).is_some());
        let elsewhere = Path::new("/elsewhere/a");
        assert_eq!(as_judged(inside_other, at, &judged, elsewhere), None);
        assert!(holds_another(&[PathBuf::from("/x/y/z")], Path::new("/x")));
        assert!(!holds_another(&[PathBuf::from("/x")], Path::new("/x")));
        assert!(!holds_another(&[], Path::new("/x")));
    }

    #[test]
    fn relisting_reads_git_and_says_why_it_could_not() {
        let gone =
            std::env::temp_dir().join(format!("symphony-sweep-relist-gone-{}", std::process::id()));
        let judged = listed("/nowhere/wt/a", "aaa", None);
        assert_eq!(
            relisted(&gone, &gone, &judged, &gone),
            Err("git worktree list --porcelain failed".to_string())
        );
    }

    #[test]
    fn a_touch_keeps_a_worktree_only_within_the_window() {
        let dir =
            std::env::temp_dir().join(format!("symphony-sweep-touched-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let touched = git::last_touched(&dir, 0.0);
        assert!(touched > 0.0, "the folder's own time is read");
        let hour = touched + 3600.0;
        let ago = touched_within(&dir, hour, 2.0).expect("an hour ago is within two");
        assert!((ago - 3600.0).abs() < 1.0, "{ago}");
        assert_eq!(touched_within(&dir, hour, 0.5), None);
        // A window of no hours keeps no touch at or before the clock, not even one this instant.
        assert_eq!(touched_within(&dir, touched, 0.0), None);
        // A touch read as ahead of the clock is within any window, that one too, said as read.
        assert_eq!(touched_within(&dir, touched - 60.0, 0.0), Some(-60.0));
        assert_eq!(touched_within(&dir, touched - 60.0, 2.0), Some(-60.0));
        std::fs::remove_dir_all(&dir).unwrap();
        // Nothing readable is busy now: kept by any window, let go by none.
        assert_eq!(touched_within(&dir, 500.0, 2.0), Some(0.0));
        assert_eq!(touched_within(&dir, 500.0, 0.0), None);
    }

    #[test]
    fn the_board_frees_a_worktree_no_session_is_bound_to_and_no_live_task_keeps() {
        use crate::health::Health;
        use crate::rows::SessionRow;
        use crate::workflow::operate::tests::legacy;
        let session = |handle: &str, at: &str, health: Health| Session {
            row: SessionRow {
                handle: handle.into(),
                worktree: at.into(),
                ..SessionRow::default()
            },
            health,
        };
        let launched = |slug: &str, at: &str| TaskSessionRow {
            slug: slug.into(),
            handle: "amber".into(),
            worktree: at.into(),
            ..TaskSessionRow::default()
        };
        let facts = Facts {
            sessions: vec![
                session("live", "/nowhere/bound", Health::Live),
                session("stale", "/nowhere/stale", Health::Stale),
                session("unknown", "/nowhere/unknown", Health::Unknown),
                session("lost", "/nowhere/lost", Health::Lost),
                session("left", "/nowhere/left", Health::Left),
                session("closing", "/nowhere/done", Health::Live),
            ],
            tasks: vec![
                legacy("sim/done", "done"),
                legacy("sim/cancelled", "cancelled"),
                legacy("sim/review", "review"),
            ],
            launches: vec![
                launched("sim/done", "/nowhere/done"),
                launched("sim/review", "/nowhere/review"),
                launched("sim/done", "/nowhere/both"),
                launched("sim/review", "/nowhere/both"),
            ],
            operating: None,
        };
        let index = Index::of(&facts, &[]);
        let nobody = BTreeSet::new();
        let closing = BTreeSet::from(["closing".to_string()]);
        let frees = |at: &str, branch: Option<&str>, unbound: &BTreeSet<String>| {
            index.frees(&canonical(Path::new(at)), branch, unbound)
        };
        // A session that holds its work keeps its worktree, whatever its tasks.
        for held in ["/nowhere/bound", "/nowhere/stale", "/nowhere/unknown"] {
            assert_eq!(frees(held, None, &nobody), None, "{held}");
        }
        // One lost or gone does not, and a worktree the board knows nothing of is tied to no
        // task: every one of none is finished.
        for free in ["/nowhere/lost", "/nowhere/left", "/nowhere/unheard"] {
            assert_eq!(frees(free, None, &nobody), Some(true), "{free}");
        }
        // The sessions of a task just finished do not count as bound.
        assert_eq!(frees("/nowhere/done", None, &nobody), None);
        assert_eq!(frees("/nowhere/done", None, &closing), Some(true));
        assert_eq!(frees("/nowhere/bound", None, &closing), None);
        // A live task keeps it by a launch there or by its branch, alone or beside a finished one.
        assert_eq!(frees("/nowhere/review", None, &nobody), None);
        assert_eq!(frees("/nowhere/both", None, &nobody), None);
        assert_eq!(frees("/nowhere/unheard", Some("sim/review"), &nobody), None);
        assert_eq!(frees("/nowhere/done", Some("sim/review"), &closing), None);
        assert_eq!(
            frees("/nowhere/unheard", Some("sim/cancelled"), &nobody),
            Some(true)
        );
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
        done.rows.push(Row {
            bytes: Some(2_000_000_000),
            ..Row::default()
        });
        assert_eq!(
            done.summary(),
            "worktree sweep: would remove 0 of 3 worktrees, would delete 1 merged worktree-agent-* branch; kept 1 merged for files that did not land; kept 1 in use; would free 0 B of the 2.0 GB its 1 worktree measured hold"
        );
    }

    #[test]
    fn the_summary_names_what_was_freed_and_the_branches_of_removed_worktrees() {
        let done = Sweep {
            total: 4,
            removed: vec![(PathBuf::from("/wt/a"), "merged".into())],
            left_branches: vec!["sim/a".into(), "sim/b".into()],
            cleared: vec![Cleared {
                folder: PathBuf::from("/wt/c/.target"),
                why: "task sim/c is review; nothing ran there for an hour".into(),
                bytes: 9_800_000_000,
            }],
            freed: 10_100_000_000,
            ..Sweep::default()
        };
        assert_eq!(
            done.summary(),
            "worktree sweep: removed 1 of 4 worktrees, deleted 0 merged worktree-agent-* branches; deleted 2 merged branches of removed worktrees; cleared 1 build folder of idle work; freed 10.1 GB"
        );
        assert!(done.lines().contains(
            &"cleared build output /wt/c/.target (task sim/c is review; nothing ran there for an hour; 9.8 GB)".to_string()
        ));
        assert!(done.lines().contains(
            &"deleted branch sim/a (merged into main, its worktree removed)".to_string()
        ));
        // Nothing removed or cleared: no bytes said.
        let idle = Sweep {
            total: 2,
            ..Sweep::default()
        };
        assert!(!idle.summary().contains("freed"), "{}", idle.summary());
    }

    #[test]
    fn the_summary_says_why_no_build_output_was_cleared() {
        let done = Sweep {
            total: 2,
            removed: vec![(PathBuf::from("/wt/a"), "merged".into())],
            uncleared: Some("the board did not answer".into()),
            freed: 1_500_000,
            ..Sweep::default()
        };
        assert_eq!(
            done.summary(),
            "worktree sweep: removed 1 of 2 worktrees, deleted 0 merged worktree-agent-* branches; cleared no build output: the board did not answer; freed 1.5 MB"
        );
    }

    #[test]
    fn a_move_the_board_refused_is_said_beside_the_removal_it_followed() {
        let done = Sweep {
            total: 2,
            removed: vec![(PathBuf::from("/wt/a"), "merged".into())],
            unmoved: vec![("sim/a".into(), "the board did not answer".into())],
            freed: 1_500_000,
            ..Sweep::default()
        };
        assert_eq!(
            done.lines(),
            [
                "removed /wt/a (merged)",
                "the comments passed on into sim/a stay under its name, not main's: the board did \
                 not answer",
                "worktree sweep: removed 1 of 2 worktrees, deleted 0 merged worktree-agent-* \
                 branches; freed 1.5 MB",
            ]
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

    #[test]
    fn a_dry_run_says_one_line_per_worktree_before_its_summary() {
        let done = Sweep {
            dry_run: true,
            total: 2,
            rows: vec![Row {
                path: PathBuf::from("/wt/a"),
                bytes: Some(3_000),
                verdict: "kept: a land runs there (pid 7)".into(),
                ..Row::default()
            }],
            ..Sweep::default()
        };
        assert_eq!(
            done.lines(),
            [
                "worktree /wt/a: 3.0 KB; no task; kept: a land runs there (pid 7)",
                "worktree sweep: would remove 0 of 2 worktrees, would delete 0 merged worktree-agent-* branches; would free 0 B of the 3.0 KB its 1 worktree measured hold",
            ]
        );
        // A pass that is not a dry run says no row.
        let real = Sweep {
            dry_run: false,
            ..done
        };
        assert_eq!(real.lines().len(), 1);
    }
}
