//! Tasks: making them, the tree of subtasks, joining and releasing them, moving them along, landing
//! them.

use super::messages::BOARD;
use super::postgres::{Pg, work as pg};
use super::statement::Statement;
use super::{
    Change, Error, Failure, Message, Opens, REFUSED, Say, Store, Task, Which, executors_or, lapsed,
    left_actor, refused, repr, unknown_actor,
};
use crate::alarm::{NOTHING, UNDER, WAS};
use crate::health::Health;
use crate::slots::OPEN;
use crate::stages;
use serde::Deserialize;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};

mod covered;

mod gates;
pub(crate) use covered::covered_command;
pub use covered::{COVERED, CoveredLine, covered_by, covered_line, landing};

mod given;
pub(super) use given::Pass;
pub use given::{Filed, Given};

mod kind;

mod moves;
use moves::Moving;

mod needs;

mod owner_task;

mod red;

mod roles;

/// Who makes a task or posts to a thread without a session of its own.
pub(super) const OWNER: &str = "owner";

/// What a block waits on, as a move to blocked names it ([`Store::move_on`],
/// [`Store::hold_blocked_on`]): a thing of [`crate::waits_on::THINGS`], and why in a few words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct On<'a> {
    pub thing: &'a str,
    pub why: Option<&'a str>,
}

/// Every status, the pipeline's in order and then the two off it.
pub const STATUSES: [&str; 9] = [
    "backlog",
    "concept",
    "design",
    "ready",
    "in_progress",
    "review",
    "done",
    "blocked",
    "cancelled",
];
/// The way a task goes when nothing stops it.
pub const PIPELINE: [&str; 7] = [
    "backlog",
    "concept",
    "design",
    "ready",
    "in_progress",
    "review",
    "done",
];
/// Where a task may start.
pub const BEFORE_WORK: [&str; 4] = ["backlog", "concept", "design", "ready"];
/// What `next` offers and `claim` accepts, with in_progress.
pub const PULLABLE: [&str; 3] = ["concept", "design", "ready"];
pub const ROLES: [&str; 4] = ["orchestrator", "executor", "reviewer", "reserved"];
/// What a landing closes a task from ([`Store::landed`]): done is a task its reviewer approved.
pub const LANDS_FROM: [&str; 4] = ["review", "blocked", "in_progress", "done"];

/// A task a landing closed: as the write left it, and the status the write found it in.
#[derive(Debug, Clone, PartialEq)]
pub struct Landed {
    pub task: Task,
    pub from: String,
}

/// A task as it is made (`Store.create`'s arguments).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NewTask {
    pub slug: String,
    pub title: String,
    /// A session's handle, or `owner`.
    pub by: String,
    pub body: String,
    pub priority: i64,
    pub status: String,
    pub parent: Option<String>,
    /// The documents it implements.
    pub links: Vec<String>,
    /// The tasks it waits on.
    pub depends: Vec<String>,
    /// A session it is reserved for, with no lapse, so `next` offers it to no one else.
    pub assignee: Option<String>,
    /// A subtask (it needs a `parent`) whose executor closes it with `done`.
    pub instruction: bool,
    /// What is red, for a task filed to fix a red that stops every landing ([`crate::red`]): it
    /// and the work under it take no orchestrator slot.
    pub red: Option<String>,
    /// What its work is, one of [`crate::kind::KINDS`]; none leaves it without one, or `red` for a
    /// task filed with a red mark ([`crate::kind`]).
    pub kind: Option<String>,
    /// The group or the person it is filed for, written on it ([`crate::people::ownership`]): none
    /// leaves it to the group its area maps to.
    pub group: Option<String>,
}

impl Default for NewTask {
    fn default() -> NewTask {
        NewTask {
            slug: String::new(),
            title: String::new(),
            by: String::new(),
            body: String::new(),
            priority: 2,
            status: "ready".into(),
            parent: None,
            links: Vec::new(),
            depends: Vec::new(),
            assignee: None,
            instruction: false,
            red: None,
            kind: None,
            group: None,
        }
    }
}

impl NewTask {
    pub fn new(slug: &str, title: &str, by: &str) -> NewTask {
        NewTask {
            slug: slug.into(),
            title: title.into(),
            by: by.into(),
            ..NewTask::default()
        }
    }
}

/// Where `move` may take a task from each status; nowhere from `done` and `cancelled`.
const TRANSITIONS: [(&str, &[&str]); 7] = [
    ("backlog", &["concept", "design", "ready", "cancelled"]),
    (
        "concept",
        &["backlog", "design", "ready", "blocked", "cancelled"],
    ),
    (
        "design",
        &["backlog", "concept", "ready", "blocked", "cancelled"],
    ),
    (
        "ready",
        &[
            "backlog",
            "concept",
            "design",
            "in_progress",
            "blocked",
            "cancelled",
        ],
    ),
    ("in_progress", &["ready", "review", "blocked", "cancelled"]),
    ("review", &["in_progress", "blocked", "cancelled"]),
    // Back to review: work held only for stamps nobody was producing (`just land-stamps`).
    (
        "blocked",
        &[
            "concept",
            "design",
            "ready",
            "in_progress",
            "review",
            "cancelled",
        ],
    ),
];

/// The approve standing on a task whose `thread` this is: its last verdict, when that is an
/// `approve` (a reviewer's, or a fast fix's orchestrator's). A landing's line, a `done` or a
/// `changes` after it is its last verdict instead, so a task an approve left done and no landing
/// has closed is one this names.
pub fn approved(thread: &[Message]) -> Option<&Message> {
    let last = thread.iter().rev().find(|m| m.kind == "verdict")?;
    stages::verdict_is(&last.text, "approve").then_some(last)
}

/// Who sends an approved task no landing has closed back to review ([`Store::move_to`]).
const REOPENERS: [&str; 3] = ["executor", "reviewer", "orchestrator"];

/// The post a move's reason leaves on the thread ([`Store::move_with`]): the status it moved to,
/// then why, as a post of the mover's beside the move's own line, which stays the board's record of
/// the move as every reader of a thread's status changes reads it.
pub(super) fn reason_line(status: &str, reason: &str) -> String {
    format!("{status}: {}", reason.trim())
}

/// Whether `move` may take a task from `current` to `target`. `done` is only reached by a verdict, a
/// landing, `done` on a subtask or a close as covered by another's landing ([`Store::close_covered`]);
/// it is left only by an approved task no landing has closed, back to review ([`approved`], [`Store::move_to`]).
pub fn can_transition(current: &str, target: &str) -> bool {
    TRANSITIONS
        .iter()
        .any(|(from, to)| *from == current && to.contains(&target))
}

/// What a move [`can_transition`] refuses says: that `slug` cannot go from `current` to `status`,
/// then what takes it on, as `board` ([`Store::board_command`]) reaches it. Its reviewer's verdict
/// closes work in review, work in progress goes to review first, and either, when its work landed
/// with another task's, closes as covered by that landing ([`Store::close_covered`]); its executor
/// closes a subtask with `done`, a done task goes only back to review while the approve that left
/// it done stands (`approve_stands`, [`approved`]), else it moves no further and what it needs is a
/// new task, and any other move is told where `current` goes, or that it goes nowhere.
pub(crate) fn cannot_move(
    board: &str,
    slug: &str,
    current: &str,
    status: &str,
    subtask: bool,
    approve_stands: bool,
) -> String {
    let next = match (current, status) {
        ("in_progress", "done") if subtask => {
            format!("its executor closes a subtask with: {board} done {slug} NOTE")
        }
        ("review", "done") => format!(
            "its reviewer closes it with: {board} verdict {slug} approve; or, when its work \
             landed with another task's, {}",
            covered_command(board, slug)
        ),
        ("in_progress", "done") => format!(
            "it goes to review first: {board} move {slug} review; or, when its work landed with \
             another task's, {}",
            covered_command(board, slug)
        ),
        // A subtask never goes to review: a done one moves no further.
        ("done", _) if !subtask && approve_stands => format!(
            "a done task goes only back to review, and only one approved that no landing has \
             closed: {board} move {slug} review"
        ),
        // A landing closed it, or no approve left it done: back to review is refused too.
        ("done", _) if !subtask => format!(
            "a done task no approve stands on, as a landing leaves it, moves no further: what it \
             still needs goes in a new task, {board} create SLUG TITLE"
        ),
        _ => match TRANSITIONS.iter().find(|(from, _)| *from == current) {
            Some((_, [rest @ .., last])) => {
                format!("from {current} it goes to {} or {last}", rest.join(", "))
            }
            _ => format!("a {current} task moves no further"),
        },
    };
    format!("{slug} cannot go from {current} to {status}; {next}")
}

/// `area/short-name`: lowercase, digits and dashes, each part starting with a letter or digit.
fn plain_slug(slug: &str) -> bool {
    let word = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    slug.split('/')
        .all(|part| part.starts_with(word) && part.chars().all(|c| word(c) || c == '-'))
}

pub(super) fn no_task(slug: &str) -> Error {
    refused(format!("{}{}", Error::NO_TASK, repr(slug)))
}

impl Error {
    /// How the board refuses a slug no task has, before the slug as it quotes it (`no task 'x'`):
    /// what a caller that goes on without the task, or answers that there is none, reads the
    /// refusal by ([`Error::is_no_task`]).
    pub const NO_TASK: &str = "no task ";

    /// Whether this is the board's refusal of a slug no task has.
    pub fn is_no_task(&self) -> bool {
        matches!(self, Error::Refused(why) if why.starts_with(Error::NO_TASK))
    }
}

/// slug -> parent, for every task.
pub(super) type Tree = HashMap<String, Option<String>>;

/// `slug`'s parent, its parent, and so on as far as the tree goes: up to a task it does not hold,
/// and once round a loop the board should never hold.
pub(super) fn above<'t>(tree: &'t Tree, slug: &str) -> impl Iterator<Item = &'t str> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut at = tree.get(slug).and_then(Option::as_deref);
    std::iter::from_fn(move || {
        let here = at.filter(|here| seen.insert(here))?;
        at = tree.get(here).and_then(Option::as_deref);
        Some(here)
    })
}

/// Where and as whom a run started, as its recorder knows before it asks the board.
#[derive(Debug, Default)]
pub struct RunPlace<'a> {
    /// The handle it was given (`--as`, `TASKS_SESSION`).
    pub handle: Option<&'a str>,
    /// Its linked worktree's top folder, as the beat hook records where an in-process agent's calls
    /// run; none in the main checkout, where every agent's board commands run.
    pub checkout: Option<&'a str>,
    /// The Claude Code session it runs in (`CLAUDE_CODE_SESSION_ID`): a subagent's is its parent's.
    pub claude: Option<&'a str>,
    /// Its checkout's branch.
    pub branch: Option<&'a str>,
}

/// A session [`Store::run_for`] read, with what its health is judged on.
#[derive(Deserialize)]
struct Seen {
    handle: String,
    last_seen: f64,
    busy_until: Option<f64>,
    pid: Option<i64>,
    host: Option<String>,
}

/// The one of `slugs` every other one is under in `tree`, if there is one.
fn top_in<'s>(tree: &Tree, slugs: &'s [String]) -> Option<&'s String> {
    slugs.iter().find(|top| {
        slugs
            .iter()
            .all(|slug| slug == *top || above(tree, slug).any(|up| up == top.as_str()))
    })
}

/// `slug`'s parent, its parent, and so on up to the root, nearest first.
fn ancestors_in(tree: &Tree, slug: &str) -> Result<Vec<String>, Error> {
    let mut found: Vec<String> = Vec::new();
    let mut at = tree.get(slug).ok_or_else(|| no_task(slug))?;
    while let Some(parent) = at {
        if parent == slug || found.contains(parent) {
            break; // a loop the board should never hold
        }
        found.push(parent.clone());
        at = tree.get(parent).ok_or_else(|| no_task(parent))?;
    }
    Ok(found)
}

pub(super) use crate::stages::BLOCK_LINE;

impl Store {
    /// Set a task's priority, recording the actor and both values without changing its other fields.
    pub fn set_priority(&self, slug: &str, priority: i64, handle: &str) -> Result<Task, Error> {
        if !(0..=4).contains(&priority) {
            return Err(refused("priority runs 0 (most urgent) to 4".into()));
        }
        // Read the old value and write its replacement and history in the same transaction. A
        // concurrent edit conflicts and retries, so history always names the value replaced.
        let acts = !self.is_person(handle);
        let body = || Statement::from(Pg::script(pg::set_priority(acts))).posting();
        self.retrying(|| {
            let vars = self.acting(
                handle,
                (self.clock)(),
                json!({
                    "slug": slug, "priority": priority,
                    "missing": format!("{REFUSED}{}", no_task(slug)),
                }),
            );
            self.written(body(), vars, slug)
        })
    }

    /// Retitle a task and link it to more documents, in one write: how the Symphony's verdict roll-up
    /// keeps its title counting every card it lists and links each card's page. A changed title is
    /// recorded with its actor and both titles; a link it already has stays one link.
    pub fn amend(
        &self,
        slug: &str,
        title: &str,
        links: &[String],
        handle: &str,
    ) -> Result<Task, Error> {
        let title = title.trim();
        if title.is_empty() {
            return Err(refused("a task needs a title".into()));
        }
        let acts = !self.is_person(handle);
        let body = || Statement::from(pg::amend(acts)).posting();
        self.retrying(|| {
            let vars = self.acting(
                handle,
                (self.clock)(),
                json!({
                    "slug": slug, "title": title, "links": links,
                    "missing": format!("{REFUSED}{}", no_task(slug)),
                }),
            );
            self.written(body(), vars, slug)
        })
    }

    /// Move a task under `parent`, or to the top with none. A task is never its own ancestor. Under a
    /// cap, a session's change that would raise an orchestrator's open tasks past it, the open work at
    /// and below the task coming under its tree, is refused ([`crate::slots`]); a hold on the task
    /// ends. Open work the move would leave with no orchestrator stays with the one it had: the
    /// move gives it the task, in the same write, and its cap read counts the work as that one's
    /// ([`Store::kept_at_move`]). Only a move out from under a task above it reads more than the
    /// tree for that.
    pub fn set_parent(&self, slug: &str, parent: Option<&str>, by: &str) -> Result<Task, Error> {
        #[derive(Deserialize)]
        struct Read {
            tree: Vec<(String, Option<String>)>,
            moves: i64,
            me: Option<Value>,
            known: bool,
        }
        let acts = !self.is_person(by);
        self.retrying(|| {
            let read: Read =
                self.read_together(Statement::from(pg::tree_read()), json!({"actor": by}))?;
            if !self.is_person(by) {
                if !read.known {
                    return Err(Failure::Stop(refused(unknown_actor(&self.board_command(), by))));
                }
                if read.me.as_ref().is_some_and(|me| !me["left_at"].is_null()) {
                    return Err(Failure::Stop(refused(left_actor(by))));
                }
            }
            let tree: Tree = read.tree.into_iter().collect();
            let Some(was) = tree.get(slug) else { return Err(Failure::Stop(no_task(slug))) };
            let (to, was) = (parent.unwrap_or(NOTHING), was.as_deref().unwrap_or(NOTHING));
            let said = format!("{by}{UNDER}{to}{WAS}{was})"); // the alarm reads both (alarm::moved)
            if let Some(parent) = parent {
                let above = ancestors_in(&tree, parent).map_err(Failure::Stop)?;
                if parent == slug || above.iter().any(|a| a == slug) {
                    return Err(Failure::Stop(refused(format!(
                        "{slug} cannot go under {parent}: it would be its own ancestor"
                    ))));
                }
            }
            let kept = self.kept_at_move(&tree, slug, parent)?;
            let to = kept.as_ref().and_then(|pass| pass.given.gave.first()).map(|(_, to)| to);
            let slots = self.reparenting(slug, parent, by, to.map(String::as_str))?;
            let guarded = !slots.is_empty();
            let vars = self.acting(
                by,
                (self.clock)(),
                json!({"slug": slug, "parent": parent, "moves": read.moves, "said": said, "slots": slots}),
            );
            let Some(pass) = kept else {
                let body = Statement::from(pg::set_parent(acts, guarded, false))
                .posting();
                return self.written(body, vars, slug);
            };
            // The give's fences go before the move moves the task's version.
            let sender = if self.is_person(by) { BOARD } else { by };
            let body = Statement::from(pg::set_parent(acts, guarded, true))
            .posting();
            self.written(body, pass.vars(sender, vars), slug)
        })
    }

    /// The parent, its parent, … up to the root, nearest first.
    pub fn ancestors(&self, slug: &str) -> Result<Vec<String>, Error> {
        let tree: Vec<(String, Option<String>)> =
            self.retrying(|| self.read_together(Statement::from(pg::TREE_ONLY), json!({})))?;
        ancestors_in(&tree.into_iter().collect(), slug)
    }

    /// The session and the task a run started at `place` is for, as a recorder takes them, in one
    /// read of the board that writes nothing (docs/workflow/MACHINE_WORK.md, "For which task"):
    ///
    /// 1. the handle `place` names, and the task it holds, else its branch's;
    /// 2. else the one live session whose in-process binding (the beat hook's) places its last tool
    ///    call in the run's linked worktree, and the task it holds, else the branch's;
    /// 3. else, for the task, the checkout's branch when it is a task's slug, whatever its status:
    ///    an approved task's executor holds none, and what it runs on the branch after a land's
    ///    conflict (main merged, its tests run at the merged tip) is the task's;
    /// 4. else, with no session placed there either, the live full session bound by the Claude Code
    ///    session the run is in, and the task it holds: a subagent shares its parent's id, so a task
    ///    found only through it never beats the branch's.
    ///
    /// A session counts only while it is live as the board judges it ([`crate::health::health`]):
    /// one that finished and never left, or went stale or lost, is passed over. A session's task is
    /// the one it holds ([`Store::claimable`]), or of several the one all the others are under (a
    /// feature's executor holds its `/work` instruction too).
    pub fn run_for(&self, place: &RunPlace) -> Result<(Option<String>, Option<String>), Error> {
        /// A role a candidate session holds on work another could take over.
        #[derive(Deserialize)]
        struct Holding {
            handle: String,
            slug: String,
        }
        #[derive(Deserialize)]
        struct Found {
            here: Vec<Seen>,
            called: Vec<Seen>,
            held: Vec<Holding>,
            tree: Vec<(String, Option<String>)>,
            branch: Option<String>,
        }
        let vars = json!({"asked": place.handle, "checkout": place.checkout,
            "claude": place.claude, "branch": place.branch,
            "executing": crate::health::EXECUTING});
        let found: Found = self
            .retrying(|| self.read_together(Statement::from(pg::run_for_read()), vars.clone()))?;
        let now = self.now();
        let live = |seen: &&Seen| {
            let row = crate::rows::SessionRow {
                handle: seen.handle.clone(),
                last_seen: seen.last_seen,
                busy_until: seen.busy_until,
                pid: seen.pid,
                host: seen.host.clone().unwrap_or_default(),
                ..Default::default()
            };
            crate::health::health(&row, now, &self.host, &*self.pid_alive, false) == Health::Live
        };
        let tree: Tree = found.tree.into_iter().collect();
        let task_of = |handle: &str| {
            let mut slugs: Vec<String> = (found.held.iter())
                .filter(|held| held.handle == handle)
                .map(|held| held.slug.clone())
                .collect();
            crate::order::sort_by(&mut slugs, &Ord::cmp);
            slugs.dedup();
            top_in(&tree, &slugs).cloned()
        };
        let branch = (place.branch)
            .filter(|_| found.branch.is_some())
            .map(str::to_owned);
        let here: Vec<&Seen> = found.here.iter().filter(live).collect();
        let called: Vec<&Seen> = found.called.iter().filter(live).collect();
        Ok(match (place.handle, here.as_slice(), called.as_slice()) {
            (Some(asked), _, _) => (Some(asked.to_owned()), task_of(asked).or(branch)),
            (None, [one], _) => (Some(one.handle.clone()), task_of(&one.handle).or(branch)),
            (None, [], [one]) if branch.is_none() => {
                (Some(one.handle.clone()), task_of(&one.handle))
            }
            (None, _, _) => (None, branch),
        })
    }

    /// The tasks right under `slug`, in the order they were made.
    pub fn children(&self, slug: &str) -> Result<Vec<Task>, Error> {
        let view = self.retrying(|| self.shown(&Which::Children(slug)))?;
        if !view.snap.known {
            return Err(no_task(slug));
        }
        Ok(view.tasks())
    }

    /// Every task below `slug`, at any depth, in the order they were made: from one read of the
    /// whole board, so the tree is as of one moment however deep it goes.
    pub fn descendants(&self, slug: &str) -> Result<Vec<Task>, Error> {
        let view = self.retrying(|| self.shown(&Which::All))?;
        let mut below: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut known = false;
        for task in &view.snap.tasks {
            known |= task.row.slug == slug;
            if let Some(parent) = &task.row.parent {
                below.entry(parent).or_default().push(&task.row.slug);
            }
        }
        if !known {
            return Err(no_task(slug));
        }
        let mut found: HashSet<&str> = HashSet::new();
        let mut next = vec![slug];
        while let Some(at) = next.pop() {
            for child in below.get(at).into_iter().flatten() {
                if found.insert(child) {
                    next.push(child);
                }
            }
        }
        let mut tasks: Vec<Task> = view
            .tasks()
            .into_iter()
            .filter(|t| found.contains(t.slug.as_str()))
            .collect();
        #[expect(
            clippy::disallowed_methods,
            reason = "a busy board path keeps std's in-place sort (crate::order): the sweep's Store::descendants"
        )]
        tasks.sort_by_key(|t| t.id);
        Ok(tasks)
    }

    /// Tasks linked to `doc` itself or to one of its headings (`doc#Heading`), by priority then slug.
    pub fn linked_to(&self, doc: &str) -> Result<Vec<Task>, Error> {
        Ok(self.retrying(|| self.shown(&Which::Linked(doc)))?.tasks())
    }

    /// Move a task along (`can_transition`). Work started, or held by an executor, is moved only by a
    /// session holding a role on it, except that the owner may cancel any open task and return
    /// blocked work to review. A move into in_progress or review from a status that holds no slot
    /// starts work, refused when it would raise an orchestrator's open tasks past the store's cap
    /// ([`crate::slots`]): blocked work a hold kept open ([`Store::hold_blocked`]) holds its slot still, and
    /// any other takes one. Any move ends a hold. A task an approve left done that no landing has
    /// closed ([`approved`]) goes back to review, by its executor, reviewer or orchestrator, for a
    /// fresh approve of a tip the approve does not cover: a fix after the landing refused a
    /// conflict, say. The board cannot see the repository: the command line and the API refuse
    /// first a task whose tip the default branch holds (`crate::land::reopenable`). Its executor
    /// and reviewer are told on the thread, where their inbox and PULL digest find it with their
    /// panes closed; with no executor to read it, its orchestrator hears that.
    pub fn move_to(&self, slug: &str, status: &str, handle: &str) -> Result<Task, Error> {
        self.move_with(slug, status, handle, None)
    }

    /// [`Store::move_to`], with `reason` (why it moves: a cancel's, say) on the thread when given,
    /// a post of the mover's right after the move's own line, in the move's one write
    /// ([`reason_line`]); a reason with no words is refused.
    pub fn move_with(
        &self,
        slug: &str,
        status: &str,
        handle: &str,
        reason: Option<&str>,
    ) -> Result<Task, Error> {
        self.move_on(slug, status, handle, reason, None)
    }

    /// [`Store::move_with`], a move to blocked naming what the block waits on (`on`), kept on the
    /// task ([`crate::waits_on::Blocked`]) in the move's own write; with no reason given, its
    /// thread says what it waits on in the reason's place. A task blocked already is given what
    /// it waits on, in place of what it named, and stays blocked. A block that names nothing is
    /// still taken here, as before (the command line alone refuses it): it waits on nothing the
    /// board can name ([`crate::waits_on::NO_REASON`]).
    pub fn move_on(
        &self,
        slug: &str,
        status: &str,
        handle: &str,
        reason: Option<&str>,
        on: Option<On<'_>>,
    ) -> Result<Task, Error> {
        let on = on.map(|on| self.blocking(slug, status, on)).transpose()?;
        let reason = match reason.map(str::trim) {
            Some("") => {
                return Err(refused(
                    "a move's reason says why it moves, in words".into(),
                ));
            }
            Some(why) => Some(reason_line(status, why)),
            None => on.as_ref().map(|(thing, why)| {
                let why = why
                    .as_ref()
                    .map_or_else(String::new, |why| format!("; {why}"));
                reason_line(status, &format!("on {thing}{why}"))
            }),
        };
        if self.is_person(handle) && matches!(status, "cancelled" | "review") {
            return self.move_as_owner(slug, status, reason.as_deref(), handle);
        }
        let (task, from) = self.moving_from(slug, status, handle, Moving::Any, reason, on)?;
        if from == "done" {
            let text = format!(
                "{slug} is back in review from done, by {handle}: the approve standing does not \
                 cover what is to land (the thread says why), so its reviewer approves the tip \
                 again, naming it"
            );
            for to in ["@executor", "@reviewer"] {
                self.post(slug, handle, BOARD, &text, Some(to), None, false)?;
            }
            // Reopened after its orchestrator went: open work again, so somebody's again.
            let why = "reopened with no orchestrator";
            if self.keep_owned(slug, handle, why)? {
                return self.get(slug);
            }
        }
        Ok(task)
    }

    /// Hold open work blocked, keeping its slot under the cap, as a landing does while every stamp
    /// its branch lacks is one nobody is producing: a move to blocked, as [`Store::move_to`] makes
    /// it and with the same line on the thread, after which the task counts as open until its next
    /// move, change of parent or of dependencies. So the move that returns it starts nothing, and
    /// the slot it held is no one else's meanwhile.
    pub fn hold_blocked(&self, slug: &str, handle: &str) -> Result<Task, Error> {
        self.hold_blocked_on(slug, handle, None)
    }

    /// [`Store::hold_blocked`], naming what the hold waits on (`on`), kept on the task as a move's
    /// block keeps it ([`Store::move_on`]): the default landing names the owner's approve
    /// (`check:owner`). With none, as before, it names nothing.
    pub fn hold_blocked_on(
        &self,
        slug: &str,
        handle: &str,
        on: Option<On<'_>>,
    ) -> Result<Task, Error> {
        let on = on
            .map(|on| self.blocking(slug, "blocked", on))
            .transpose()?;
        Ok(self
            .moving_from(slug, "blocked", handle, Moving::Hold, None, on)?
            .0)
    }

    /// What `on` names for a move of `slug` to `status`, and its why, each as given and checked: a
    /// thing of [`crate::waits_on::THINGS`], a task the board holds that is not `slug` itself,
    /// and a few words; refused for a move to any status but blocked.
    fn blocking(
        &self,
        slug: &str,
        status: &str,
        on: On<'_>,
    ) -> Result<(String, Option<String>), Error> {
        if status != "blocked" {
            return Err(refused(format!(
                "what a block waits on is said by a move to blocked, not to {status}"
            )));
        }
        let thing = crate::waits_on::Thing::parse(on.thing).map_err(refused)?;
        if let crate::waits_on::Thing::Task(target) = thing {
            if target == slug {
                return Err(refused(format!("{slug} cannot wait on itself")));
            }
            if self.get(target).is_err() {
                return Err(refused(format!(
                    "no task {}: a block waits on a task the board holds",
                    repr(target)
                )));
            }
        }
        let why = (on.why)
            .map(crate::waits_on::why)
            .transpose()
            .map_err(refused)?;
        Ok((thing.to_string(), why.map(str::to_string)))
    }

    /// [`Store::move_to`] by a session, from the statuses `how` allows ([`Moving`]).
    fn moving(&self, slug: &str, status: &str, handle: &str, how: Moving) -> Result<Task, Error> {
        Ok(self.moving_from(slug, status, handle, how, None, None)?.0)
    }

    /// [`Store::moving`], with `reason`'s line ([`reason_line`]) posted in its write when given,
    /// what a block waits on (`on`, [`Store::blocking`]) kept on the task, and the status it moved
    /// the task from. A task blocked already that is given what it waits on keeps its status and
    /// takes the record alone, the status it was blocked from kept, or read from its thread.
    fn moving_from(
        &self,
        slug: &str,
        status: &str,
        handle: &str,
        how: Moving,
        reason: Option<String>,
        on: Option<(String, Option<String>)>,
    ) -> Result<(Task, String), Error> {
        let starts = OPEN.contains(&status);
        // The board the commands a refusal names reach: this one.
        let board = self.board_command();
        let from = RefCell::new(String::new());
        let task = self.ruling(slug, handle, starts, |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            let current = task.status.as_str();
            from.replace(current.to_string());
            how.allows(slug, current)?;
            let record = |was: Option<String>| {
                on.as_ref().map(|(thing, why)| {
                    crate::waits_on::Blocked {
                        on: thing.clone(),
                        why: why.clone(),
                        by: handle.into(),
                        at: view.now,
                        from: was,
                    }
                    .value()
                })
            };
            if current == "blocked" && status == "blocked" && on.is_some() {
                if !view.snap.roles.iter().any(|r| r.handle == handle) {
                    return Err(refused(format!(
                        "{handle} holds no role on {slug}, which is blocked"
                    )));
                }
                let was = match crate::waits_on::Blocked::kept(current, &task.fields) {
                    Some(kept) => kept.from,
                    None => self.blocked_from(slug, current, task.created, task.updated)?,
                };
                return Ok(Change {
                    reason: reason.clone(),
                    blocked: record(was),
                    ..Change::default()
                });
            }
            if current == "done" && status == "review" {
                // Read after the task, whose version the write holds: a landing since is a stale
                // write, read and decided again.
                let thread = self.thread(slug)?;
                if approved(&thread).is_none() {
                    return Err(refused(format!(
                        "{slug} is done: only an approved task no landing has closed goes back to \
                         review, for a fresh approve of its new tip"
                    )));
                }
                if !REOPENERS.iter().any(|role| view.holds(handle, role)) {
                    return Err(refused(format!(
                        "{handle} is not {slug}'s executor, reviewer or orchestrator, which alone \
                         send it back to review"
                    )));
                }
            } else if !can_transition(current, status) {
                let subtask = task.parent.is_some();
                // What a done task's refusal names next: back to review only while its approve
                // stands, read as that move reads it.
                let approve_stands =
                    current == "done" && !subtask && approved(&self.thread(slug)?).is_some();
                return Err(refused(cannot_move(
                    &board,
                    slug,
                    current,
                    status,
                    subtask,
                    approve_stands,
                )));
            }
            if (task.parent.is_some() || task.instruction)
                && (status == "review" || status == "done")
            {
                return Err(refused(format!(
                    "{slug} is a subtask: integrate into its parent and close it with `done`, not {status}"
                )));
            }
            let held = !BEFORE_WORK.contains(&current) || view.holders("executor").next().is_some();
            if held && !view.snap.roles.iter().any(|r| r.handle == handle) {
                return Err(refused(format!(
                    "{handle} holds no role on {slug}, which is {current}"
                )));
            }
            // Work started again, or started by a move: a slot of its orchestrator's, or of its
            // executor's parent (the mover's when it has none) where no orchestrator holds it.
            // Blocked work a hold kept open holds its slot already, which the cap reads.
            let starting = OPEN.contains(&status) && !OPEN.contains(&current);
            let opens = starting.then(|| Opens {
                status: Some(status.into()),
                by: executors_or(view.executors(), Some(handle)),
                what: format!("{slug} cannot go from {current} to {status}"),
                ..Opens::default()
            });
            Ok(Change {
                status: Some(status.into()),
                hold: how.holds(),
                say: Say::system(stages::moved_line(handle, current, status)),
                reason: reason.clone(),
                opens,
                blocked: record(Some(current.into())),
                // Back to review from done: open work again, its approve standing no more.
                unlanded: (current == "done").then_some(Value::Null),
                ..Change::default()
            })
        })?;
        Ok((task, from.into_inner()))
    }

    /// The status `slug`, in `status` (blocked with no record of what it waits on) since its
    /// write at `updated`, was blocked from, as its thread's replay reads it
    /// ([`stages::in_status`]).
    fn blocked_from(
        &self,
        slug: &str,
        status: &str,
        created: f64,
        updated: f64,
    ) -> Result<Option<String>, Error> {
        let thread = self.thread(slug)?;
        let lines: Vec<stages::Line> = thread.iter().map(stages::Line::from).collect();
        let facts = stages::Facts {
            status,
            created,
            updated,
        };
        Ok(stages::in_status(facts, &lines).from)
    }

    /// The owner's two moves, and a person's (`by`, [`Store::is_person`]): any open task to
    /// `cancelled`, a blocked one (never an instruction subtask) back to `review`; `reason`'s line,
    /// when given, posted after the move's own, which names `by` (`ana moved it …`).
    fn move_as_owner(
        &self,
        slug: &str,
        status: &str,
        reason: Option<&str>,
        by: &str,
    ) -> Result<Task, Error> {
        let cancelling = status == "cancelled";
        let from: Vec<_> = if cancelling {
            TRANSITIONS
                .iter()
                .filter(|(_, targets)| targets.contains(&"cancelled"))
                .map(|(source, _)| *source)
                .collect()
        } else {
            vec!["blocked"]
        };
        // As in set_priority, status validation, the update and history share one transaction.
        // A concurrent edit conflicts and retries without extending the shared agent write path.
        self.retrying(|| {
            // A red's close holds the counters it changes as a start does, capped store or not,
            // judged on the task as read: a red marked since is read and decided again.
            let closed = self.closing_slots(slug, status)?;
            let (reasoned, fenced, guarded) = (reason.is_some(), closed.fenced(), closed.guarded());
            let body = Statement::from(pg::move_as_owner(reasoned, cancelling, fenced, guarded))
            .posting();
            let vars = self.acting(
                by,
                (self.clock)(),
                json!({
                    "slug": slug, "status": status, "from": from, "slots": closed.slots, "version": closed.version,
                    "move_reason": reason,
                    "missing": format!("{REFUSED}{}", no_task(slug)),
                    "cannot": format!("{REFUSED}{slug} cannot go from "),
                    "instruction": format!("{REFUSED}{slug} is a subtask: integrate into its parent and close it with `done`, not review"),
                }),
            );
            self.written(body, vars, slug)
        })
    }

    /// A task in review whose branch `just land` put on main is done: the landing is its verdict. So is
    /// one blocked, which land held there while its stamps waited on nobody (`just land-stamps`) and
    /// could not move back, and one in progress: taken back (by its executor or a changes verdict) after
    /// land's last read of it, as main moved. Land refuses a task in progress before it moves main and
    /// names in progress in `from` only once the branch is on main, so that work landed; further
    /// changes go in a new task. No other command closes a task in progress but a subtask's `done`
    /// and a close as covered by another's landing ([`Store::close_covered`]).
    ///
    /// `from` is which of [`LANDS_FROM`] this landing closes, checked against the status the write
    /// reads, and the answer names the one it found. A task closed from in progress has the verdict
    /// addressed to its executor, saying it was taken back, in the same write. One done (its
    /// reviewer's `approve`, which the Symphony's default `just land` lands) stays done, the line
    /// written.
    ///
    /// Taken by a session holding a role on the task, or one that asked to land its branch
    /// (`requested`: `land-request` keeps that record, off the board), so the approval recorded with
    /// `land-approve` is the one verdict, never a second one on the board.
    pub fn landed(
        &self,
        slug: &str,
        handle: &str,
        text: &str,
        requested: bool,
        from: &[&str],
    ) -> Result<Landed, Error> {
        if let Some(status) = from.iter().find(|s| !LANDS_FROM.contains(s)) {
            return Err(refused(format!(
                "a landing closes a task in review, blocked, in_progress or done, not {status}"
            )));
        }
        let found = RefCell::new(String::new());
        let task = self.rule(slug, handle, |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            if !from.contains(&task.status.as_str()) {
                return Err(refused(format!(
                    "{slug} is {}, which this landing does not close",
                    task.status
                )));
            }
            if !requested && !view.snap.roles.iter().any(|r| r.handle == handle) {
                return Err(refused(format!("{handle} holds no role on {slug}")));
            }
            let taken_back = task.status == "in_progress";
            found.replace(task.status.clone());
            Ok(Change {
                status: (task.status != "done").then(|| "done".into()),
                say: Some(Say {
                    kind: "verdict",
                    text: if taken_back {
                        format!(
                            "{}; {slug} was taken back to in_progress, so the landing closes it: further changes go in a new task",
                            text.trim()
                        )
                    } else {
                        text.trim().into()
                    },
                    recipient: taken_back.then(|| "@executor".into()),
                }),
                // Landed: no longer approved and waiting for its land.
                unlanded: Some(Value::Null),
                ..Change::default()
            })
        })?;
        let from = found.into_inner();
        if from == "in_progress" {
            let what = format!("the landing of {slug} from {handle} to @executor");
            self.tell_if_unread(slug, None, &what, None);
        }
        Ok(Landed { task, from })
    }

    /// The best work for `handle`: its own reservation, then priority, then a red ([`crate::red`])
    /// before other work, then its areas, then the order made. A task a custom workflow holds is
    /// offered where an edge leaving its position gives the executor's role, nobody there holds it,
    /// and admission's preview shows nothing stopping `handle` taking that edge now
    /// ([`Store::workflow_actions`]): its reservations, dependencies and authority are the edge's,
    /// never the board's own claim's (docs/workflow/WORKFLOW.md, "Operations").
    pub fn next_for(&self, handle: &str) -> Result<Option<Task>, Error> {
        let view = self.retrying(|| self.shown(&Which::Pullable(handle)))?;
        let me = view
            .session(handle)
            .ok_or_else(|| refused(format!("no session {}", repr(handle))))?;
        let area = |slug: &str| slug.split('/').next().unwrap_or_default().to_string();
        let areas: HashSet<String> = (me
            .affinity
            .split(',')
            .filter(|a| !a.is_empty())
            .map(String::from))
        .chain(view.snap.mine.iter().map(|slug| area(slug)))
        .collect();
        let waiting: HashSet<&str> = view
            .snap
            .open_deps
            .iter()
            .map(|l| l.slug.as_str())
            .collect();
        let tasks = view.tasks();
        let operating = self.operating_for(&tasks)?;
        /// A task's rank, least first: not reserved for the session, priority, not red, not in
        /// its areas, the order made.
        type Rank = (bool, i64, bool, bool, i64);
        // Each candidate with its rank, and the edge to preview for one a workflow holds.
        let mut candidates: Vec<(Rank, Task, Option<String>)> = Vec::new();
        for task in tasks {
            // The reservations that still hold it, by the rule a write lapses one by
            // ([`super::lapsed`]): the view's tasks already leave out one past its time, and give
            // a holder with no session `Lost`, which lapses one as no session does.
            let reservers: Vec<&str> = (task.roles.iter())
                .filter(|r| r.role == "reserved" && !lapsed(r.expires, Some(r.health), view.now).0)
                .map(|r| r.handle.as_str())
                .collect();
            let mine = reservers.contains(&handle);
            if !reservers.is_empty() && !mine {
                continue;
            }
            if (task.roles.iter()).any(|r| r.role == "executor" && r.health.holds()) {
                continue;
            }
            let edge = if crate::store::workflow::position_held(&task.fields) {
                let claim = (operating.as_ref())
                    .and_then(|o| o.placed(&task))
                    .and_then(|placed| {
                        (placed.claims().into_iter())
                            .find(|c| c.role == symphony_api::workflow::Role::Executor)
                            .map(|c| c.edge.id.clone())
                    });
                let Some(edge) = claim else {
                    continue;
                };
                Some(edge)
            } else if waiting.contains(task.slug.as_str())
                // The read takes a held task's claim statuses too: one the board's writers keep
                // is offered only in a pullable status.
                || !PULLABLE.contains(&task.status.as_str())
            {
                continue;
            } else {
                None
            };
            let key = (
                !mine,
                task.priority,
                !task.red(),
                !areas.contains(&area(&task.slug)),
                task.id,
            );
            candidates.push((key, task, edge));
        }
        let principal = crate::workflow::agent_session(handle);
        let host = crate::workflow::Host::local();
        // The least rank first, each taken in turn rather than all sorted: a rank names its
        // task's id, so no two are equal and the order is the sort's.
        while let Some(least) = (0..candidates.len()).min_by_key(|&i| candidates[i].0) {
            let (_, task, edge) = candidates.swap_remove(least);
            let Some(edge) = edge else {
                return Ok(Some(task));
            };
            // Admission's own preview decides a held task, read only for the best of them.
            let next = match self.workflow_actions(&task.slug, &principal, &host) {
                Ok(next) => next,
                Err(Error::Refused(_)) => continue,
                Err(error) => return Err(error),
            };
            if (next.actions.iter()).any(|a| a.edge == edge && a.unavailable.is_empty()) {
                return Ok(Some(task));
            }
        }
        Ok(None)
    }

    /// Every session that has held executor on `slug` or on a work subtask under it, now or before:
    /// from their claims and joins as executor on the record, so one that released the role and
    /// took another is still named, and one that executed a slice, whose work is on the task's
    /// branch, too; not one that executed only an instruction subtask, a review say
    /// ([`Store::ever_held`]).
    pub fn ever_executed(&self, slug: &str) -> Result<BTreeSet<String>, Error> {
        Ok(self.ever_held(slug)?.executed)
    }

    /// Who has held the roles that make a session no fresh reviewer of `slug` (RULES.md
    /// `workflow/reviewers-not-the-owner`): executor on it or on any work subtask under it, at any
    /// depth, and its orchestrator, now or before (joined, took over, handed over or handed it),
    /// each read from the threads' system lines and the roles held now. Each task is read on its
    /// own, the tasks under one found by their parent. An instruction subtask under it (its
    /// `instruction` flag: a review, a question, an errand) puts nothing on its branch, so its
    /// executor is not one of the task's, as the reviewer that claimed `SLUG/review` is not; the
    /// tasks under an instruction are read all the same, each by its own flag. `slug`'s own
    /// executors count whatever it is.
    pub fn ever_held(&self, slug: &str) -> Result<Held, Error> {
        #[derive(Deserialize)]
        struct Line {
            sender: String,
            text: String,
        }
        #[derive(Deserialize)]
        struct Holder {
            handle: String,
            role: String,
        }
        #[derive(Deserialize)]
        struct Below {
            slug: String,
            #[serde(default)]
            instruction: Option<bool>,
        }
        #[derive(Deserialize)]
        struct Read {
            known: bool,
            below: Vec<Below>,
            lines: Vec<Line>,
            holders: Vec<Holder>,
        }
        let mut held = Held::default();
        let mut seen: HashSet<String> = HashSet::new();
        // Each task to read, and whether its executors are the task's: all but an instruction's.
        let mut next = vec![(slug.to_string(), true)];
        while let Some((at, work)) = next.pop() {
            if !seen.insert(at.clone()) {
                continue;
            }
            let read: Read = self.retrying(|| {
                self.read_together(Statement::from(pg::EVER_HELD), json!({"slug": at}))
            })?;
            let root = at == slug;
            if root && !read.known {
                return Err(no_task(slug));
            }
            for line in &read.lines {
                let sender = line.sender.as_str();
                if line.text.starts_with(&format!("{sender} claimed it"))
                    || line.text == format!("{sender} joined as executor")
                {
                    if work {
                        held.executed.insert(sender.into());
                    }
                } else if root && line.text == format!("{sender} joined as orchestrator") {
                    held.orchestrated.insert(sender.into());
                } else if let Some((from, to)) = crate::orchestration::moved(&line.text)
                    && root
                {
                    held.orchestrated.extend(from.into_iter().map(String::from));
                    held.orchestrated.insert(to.into());
                }
            }
            for holder in read.holders {
                match holder.role.as_str() {
                    "executor" => {
                        if work {
                            held.executed.insert(holder.handle);
                        }
                    }
                    _ if root => {
                        held.orchestrated.insert(holder.handle);
                    }
                    _ => {}
                }
            }
            next.extend(
                (read.below.into_iter()).map(|below| (below.slug, below.instruction != Some(true))),
            );
        }
        Ok(held)
    }
}

/// Who has held a task's roles that a fresh reviewer never held ([`Store::ever_held`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Held {
    /// Every session that executed the task or a work subtask under it, never counting an
    /// instruction subtask's executor.
    pub executed: BTreeSet<String>,
    /// Every session that orchestrated the task.
    pub orchestrated: BTreeSet<String>,
}

/// What makes a session no fresh reviewer of a task (RULES.md `workflow/reviewers-not-the-owner`):
/// it wrote, executed or requested the work ([`Held::unfresh`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unfresh {
    /// It executed the task, or a work subtask under it.
    Executed,
    /// It orchestrates the task, or did.
    Orchestrated,
    /// It filed the task.
    Filed,
}

impl Held {
    /// Why `handle`'s verdict on the task these are the roles of is none a landing goes by, when it
    /// is none: it executed the task or work under it, or it requested the work
    /// ([`Held::requested`]). The board refuses such a verdict ([`Store::verdict`]) and the land an
    /// approve an older board took from one (`land::independent`), by this one rule.
    pub fn unfresh(&self, handle: &str, filer: &str, orchestrates: bool) -> Option<Unfresh> {
        if self.executed.contains(handle) {
            return Some(Unfresh::Executed);
        }
        self.requested(handle, filer, orchestrates)
    }

    /// Whether `handle` requested the work, and how, when its verdict is no fresh reviewer's for
    /// that: it orchestrates the task (`orchestrates`, the role as read now, which an older board's
    /// record may not show) or did, or it filed it (`filer`). The board refuses
    /// such a session the reviewer's role too ([`Store::take_role`]).
    pub fn requested(&self, handle: &str, filer: &str, orchestrates: bool) -> Option<Unfresh> {
        if orchestrates || self.orchestrated.contains(handle) {
            Some(Unfresh::Orchestrated)
        } else if filer == handle {
            Some(Unfresh::Filed)
        } else {
            None
        }
    }
}

/// The refusal of `handle`'s verdict on `slug`, or of the reviewer's role there when `role`, for
/// what makes it no fresh reviewer (`why`, [`Held::unfresh`]): what it did, and, when it requested
/// the work, who gives the verdict instead, on `board` ([`Store::board_command`]).
pub(crate) fn unfresh_refusal(
    board: &str,
    slug: &str,
    handle: &str,
    why: Unfresh,
    role: bool,
) -> String {
    let did = match why {
        Unfresh::Executed => {
            return format!(
                "{handle} executed {slug}: its verdict is a fresh reviewer's, one that did not \
                 write or execute it"
            );
        }
        Unfresh::Orchestrated => "orchestrated",
        Unfresh::Filed => "filed",
    };
    let refused = if role {
        " and cannot review it:"
    } else {
        ", and"
    };
    format!(
        "{handle} {did} {slug}{refused} the verdict is a fresh reviewer's, one that did not; ask a \
         fresh reviewer, which takes the role from a session of its own: {board} role {slug} \
         reviewer"
    )
}

#[cfg(test)]
mod tests {
    //! The moves a task's status may make.
    use super::*;

    #[test]
    fn the_forward_path_is_allowed() {
        for (a, b) in [
            ("backlog", "ready"),
            ("ready", "in_progress"),
            ("in_progress", "review"),
            ("review", "in_progress"),
            ("blocked", "ready"),
            ("in_progress", "blocked"),
            ("review", "blocked"),
            ("blocked", "review"),
        ] {
            assert!(can_transition(a, b), "{a} -> {b}");
        }
    }

    #[test]
    fn a_refused_move_names_what_takes_the_task_on_from_where_it_is() {
        let said = |current, status, subtask, approve_stands| {
            cannot_move("B", "a/b", current, status, subtask, approve_stands)
        };
        // Back to review is named for a done task only while its approve stands.
        let back = "a done task goes only back to review, and only one approved that no landing \
                    has closed: B move a/b review";
        let closed = "a done task no approve stands on, as a landing leaves it, moves no further: \
                      what it still needs goes in a new task, B create SLUG TITLE";
        for (current, status, subtask, approve_stands, next) in [
            (
                "review",
                "done",
                false,
                false,
                "its reviewer closes it with: B verdict a/b approve; or, when its work landed \
                 with another task's, B done a/b NOTE --covered-by OTHER",
            ),
            (
                "in_progress",
                "done",
                false,
                false,
                "it goes to review first: B move a/b review; or, when its work landed with \
                 another task's, B done a/b NOTE --covered-by OTHER",
            ),
            (
                "in_progress",
                "done",
                true,
                false,
                "its executor closes a subtask with: B done a/b NOTE",
            ),
            ("done", "ready", false, true, back),
            ("done", "in_progress", false, true, back),
            ("done", "ready", false, false, closed),
            ("done", "cancelled", false, false, closed),
            ("done", "ready", true, false, "a done task moves no further"),
            ("done", "ready", true, true, "a done task moves no further"),
            (
                "cancelled",
                "ready",
                false,
                false,
                "a cancelled task moves no further",
            ),
            (
                "backlog",
                "review",
                false,
                false,
                "from backlog it goes to concept, design, ready or cancelled",
            ),
        ] {
            assert_eq!(
                said(current, status, subtask, approve_stands),
                format!("a/b cannot go from {current} to {status}; {next}"),
                "approve stands: {approve_stands}"
            );
        }
    }

    /// Whose verdict is no fresh reviewer's, and why: who executed the task first, whatever else
    /// it did; then who orchestrates it now or did, then who filed it.
    #[test]
    fn a_fresh_reviewer_wrote_executed_and_requested_none_of_the_work() {
        let held = Held {
            executed: BTreeSet::from(["amber".to_string(), "both".to_string()]),
            orchestrated: BTreeSet::from(["taupe".to_string(), "both".to_string()]),
        };
        let (executed, orchestrated, filed) = (
            Some(Unfresh::Executed),
            Some(Unfresh::Orchestrated),
            Some(Unfresh::Filed),
        );
        // handle, filer, orchestrates now, unfresh, requested.
        for (handle, filer, orchestrates, unfresh, requested) in [
            ("slate", "owner", false, None, None),
            ("amber", "owner", false, executed, None),
            ("both", "owner", false, executed, orchestrated),
            ("both", "both", true, executed, orchestrated),
            ("taupe", "owner", false, orchestrated, orchestrated),
            ("ochre", "owner", true, orchestrated, orchestrated),
            ("ochre", "ochre", true, orchestrated, orchestrated),
            ("owner", "owner", false, filed, filed),
            ("coral", "coral", false, filed, filed),
            ("amber", "amber", false, executed, filed),
        ] {
            let case = format!("{handle} filed by {filer}, orchestrates {orchestrates}");
            assert_eq!(held.unfresh(handle, filer, orchestrates), unfresh, "{case}");
            assert_eq!(
                held.requested(handle, filer, orchestrates),
                requested,
                "{case}"
            );
        }
    }

    #[test]
    fn a_refusal_for_no_fresh_reviewer_says_what_it_did_and_who_gives_the_verdict() {
        let said = |why, role| unfresh_refusal("B", "a/b", "ochre", why, role);
        let ask = "the verdict is a fresh reviewer's, one that did not; ask a fresh reviewer, which \
                   takes the role from a session of its own: B role a/b reviewer";
        assert_eq!(
            said(Unfresh::Orchestrated, false),
            format!("ochre orchestrated a/b, and {ask}")
        );
        assert_eq!(
            said(Unfresh::Filed, false),
            format!("ochre filed a/b, and {ask}")
        );
        assert_eq!(
            said(Unfresh::Orchestrated, true),
            format!("ochre orchestrated a/b and cannot review it: {ask}")
        );
        assert_eq!(
            said(Unfresh::Filed, true),
            format!("ochre filed a/b and cannot review it: {ask}")
        );
        for role in [false, true] {
            assert_eq!(
                said(Unfresh::Executed, role),
                "ochre executed a/b: its verdict is a fresh reviewer's, one that did not write or \
                 execute it"
            );
        }
    }

    #[test]
    fn concept_and_design_sit_between_backlog_and_ready() {
        assert_eq!(
            PIPELINE,
            [
                "backlog",
                "concept",
                "design",
                "ready",
                "in_progress",
                "review",
                "done"
            ]
        );
        for (a, b) in [
            ("backlog", "concept"),
            ("concept", "design"),
            ("design", "ready"),
            ("backlog", "design"),
            ("concept", "ready"),
            ("design", "concept"),
            ("blocked", "design"),
            ("design", "blocked"),
        ] {
            assert!(can_transition(a, b), "{a} -> {b}");
        }
        assert!(!can_transition("concept", "in_progress"));
        assert!(!can_transition("design", "review"));
    }

    #[test]
    fn done_is_reached_only_by_a_verdict() {
        for a in STATUSES {
            assert!(!can_transition(a, "done"), "{a}");
        }
    }

    #[test]
    fn finished_tasks_do_not_move() {
        for b in STATUSES {
            assert!(!can_transition("done", b), "{b}");
            assert!(!can_transition("cancelled", b), "{b}");
        }
    }

    #[test]
    fn skipping_review_is_refused() {
        assert!(!can_transition("ready", "review"));
        assert!(!can_transition("backlog", "in_progress"));
    }

    #[test]
    fn an_unknown_status_is_refused() {
        assert!(!can_transition("ready", "doing"));
        assert!(!can_transition("doing", "ready"));
    }

    #[test]
    fn every_move_the_table_allows() {
        let allowed: Vec<(&str, &str)> = STATUSES
            .iter()
            .flat_map(|a| STATUSES.iter().map(move |b| (*a, *b)))
            .filter(|(a, b)| can_transition(a, b))
            .collect();
        assert_eq!(allowed.len(), 33, "{allowed:?}");
    }

    /// What a recorder takes a session holding several tasks to work on.
    #[test]
    fn the_top_of_held_tasks_is_the_one_every_other_is_under() {
        let tree: Tree = [
            ("feature", None),
            ("feature/work", Some("feature")),
            ("feature/slice", Some("feature")),
            ("feature/slice/work", Some("feature/slice")),
            ("other", None),
        ]
        .into_iter()
        .map(|(slug, parent)| (slug.to_owned(), parent.map(str::to_owned)))
        .collect();
        let top = |held: &[&str]| {
            let held: Vec<String> = held.iter().map(|slug| (*slug).to_owned()).collect();
            top_in(&tree, &held).cloned()
        };
        assert_eq!(
            top(&["feature", "feature/work"]).as_deref(),
            Some("feature")
        );
        assert_eq!(
            top(&["feature/work", "feature"]).as_deref(),
            Some("feature")
        );
        assert_eq!(
            top(&["feature/slice/work", "feature", "feature/slice"]).as_deref(),
            Some("feature")
        );
        assert_eq!(top(&["feature"]).as_deref(), Some("feature"));
        // Siblings, unrelated tasks, or a task the board does not hold are no one tree.
        assert_eq!(top(&["feature/work", "feature/slice"]), None);
        assert_eq!(top(&["feature", "other"]), None);
        assert_eq!(top(&["feature", "gone/work"]), None);
        assert_eq!(top(&[]), None);
    }

    /// The refusal of a slug no task has is read by how the board words it, whatever the slug's
    /// quoting; another refusal that only names a task, and a database's error in the same words,
    /// are not it.
    #[test]
    fn a_refusal_of_a_slug_no_task_has_is_read_as_one_and_no_other_is() {
        assert!(no_task("tools/x").is_no_task());
        assert!(no_task("it's").is_no_task());
        assert!(no_task("").is_no_task());
        assert_eq!(no_task("tools/x").to_string(), "no task 'tools/x'");
        assert!(!refused("there is no task tools/x".into()).is_no_task());
        assert!(!refused("no tasks wait".into()).is_no_task());
        assert!(!refused(String::new()).is_no_task());
        assert!(!Error::Db("no task 'tools/x'".into()).is_no_task());
        // The approve standing on a thread is its last verdict, when that is an approve.
        let verdict = |text: &str| Message {
            id: 1,
            slug: "tools/x".into(),
            sender: "plum".into(),
            recipient: None,
            kind: "verdict".into(),
            text: text.into(),
            ts: 0.0,
        };
        let thread = [verdict("changes: one"), verdict("approve: at 1a2b3c4")];
        assert_eq!(
            approved(&thread).map(|m| m.text.as_str()),
            Some("approve: at 1a2b3c4")
        );
        assert!(approved(&[verdict("approve"), verdict("approved by nobody")]).is_none());
        assert!(approved(&[verdict("approve"), verdict("done: merged")]).is_none());
        assert!(approved(&[]).is_none());
    }
}
