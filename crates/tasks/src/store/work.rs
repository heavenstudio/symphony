//! Tasks: making them, the tree of subtasks, joining and releasing them, moving them along, landing
//! them.

use super::{
    ACT, Change, Error, Failure, REFUSED, Say, Store, Task, Which, acting, left_actor, logged,
    refused, repr, unknown_actor,
};
use crate::health::Health;
use serde::Deserialize;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

mod roles;

/// Who makes a task or posts to a thread without a session of its own.
pub(super) const OWNER: &str = "owner";

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
/// What a landing closes a task from ([`Store::landed`]).
pub const LANDS_FROM: [&str; 3] = ["review", "blocked", "in_progress"];

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

/// Whether `move` may take a task from `current` to `target`. `done` is only reached by a verdict, a
/// landing, or `done` on an instruction subtask.
pub fn can_transition(current: &str, target: &str) -> bool {
    TRANSITIONS
        .iter()
        .any(|(from, to)| *from == current && to.contains(&target))
}

/// `area/short-name`: lowercase, digits and dashes, each part starting with a letter or digit.
fn plain_slug(slug: &str) -> bool {
    let word = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    slug.split('/')
        .all(|part| part.starts_with(word) && part.chars().all(|c| word(c) || c == '-'))
}

pub(super) fn no_task(slug: &str) -> Error {
    refused(format!("no task {}", repr(slug)))
}

/// The task, its dependencies and parent there, its assignee a session, and a slug nobody took.
const CREATE: &str = "
IF record::exists(type::record('task', $slug)) { THROW $taken };
FOR $other IN $needed { IF !record::exists(type::record('task', $other.slug)) { THROW $other.missing }; };
IF $assignee != NONE AND !record::exists(type::record('session', $assignee)) { THROW $unassignable };
LET $sid = (UPSERT ONLY counter:task SET last += 1).last;
CREATE type::record('task', $slug) CONTENT {
    sid: $sid, slug: $slug, title: $title, body: $body, status: $status, priority: $priority, parent: $parent,
    created_by: $actor, created: $now, updated: $now, instruction: $instruction,
} RETURN NONE;
IF $assignee != NONE {
    CREATE type::record('role', [$slug, $assignee, 'reserved'])
        CONTENT { slug: $slug, handle: $assignee, role: 'reserved', since: $now } RETURN NONE;
};
FOR $link IN $links {
    UPSERT type::record('link', [$slug, $link.kind, $link.target])
        CONTENT { slug: $slug, kind: $link.kind, target: $link.target } RETURN NONE;
};";

/// Every task's slug and parent: the tree, read whole.
const TREE: &str = "SELECT VALUE [slug, parent] FROM task";

/// The tree as `set_parent` decides on it, and the count of moves it was read at.
const TREE_READ: &str = "RETURN {
    tree: (SELECT VALUE [slug, parent] FROM task),
    moves: (SELECT VALUE last FROM ONLY counter:tree) ?? 0,
    me: (SELECT left_at FROM ONLY type::record('session', $actor)),
    known: record::exists(type::record('session', $actor)),
};";

/// No task moved under another since the tree was read.
const MOVE_UNDER: &str = "
IF ((SELECT VALUE last FROM ONLY counter:tree) ?? 0) != $moves { THROW 'stale' };
UPSERT counter:tree SET last += 1 RETURN NONE;
UPDATE type::record('task', $slug) SET parent = $parent, updated = $now, version += 1 RETURN NONE;";

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

/// What starts the line a cancelled prerequisite's block puts on its dependent's thread.
pub(super) const BLOCK_LINE: &str = "blocked: cancelled dependencies: ";

/// Cancellation does not satisfy a prerequisite. Block its unfinished dependents, or the task
/// being created/moved if one of its prerequisites was cancelled earlier, in the same transaction.
/// Already blocked work still needs each new cancellation's reason on its thread. The line ends with
/// the status the dependent held, `(was ready)`, which `depends` gives it back.
pub(super) fn block_cancelled_dependencies(cancelling: bool) -> String {
    // Ordinary moves/creates use link_slug's index; only cancellation needs the reverse lookup.
    let scope = if cancelling {
        "target = $slug"
    } else {
        "slug = $slug"
    };
    format!(
        "LET $blocked_links = SELECT slug, target FROM link WHERE kind = 'dep'
            AND {scope}
            AND type::record('task', target).status = 'cancelled'
            AND type::record('task', slug).status NOT IN ['done', 'cancelled']
            ORDER BY slug, target;
FOR $dependent IN array::distinct($blocked_links.slug) {{
    LET $cancelled = $blocked_links.filter(|$link| $link.slug = $dependent).target;
    LET $was = (SELECT VALUE status FROM ONLY type::record('task', $dependent));
    UPDATE type::record('task', $dependent) SET status = 'blocked', updated = $now, version += 1 RETURN NONE;
    LET $reason = '{BLOCK_LINE}' + array::join($cancelled, ', ') + ' (was ' + $was + ')';
    {}
}};",
        logged("$dependent", "$actor", "'system'", "$reason", "NONE")
    )
}

impl Store {
    /// A new task, on the record. `assignee`: a session the task is reserved for, with no lapse.
    pub fn create(&self, new: &NewTask) -> Result<Task, Error> {
        if !plain_slug(&new.slug) {
            return Err(refused(format!(
                "slug {} must look like area/short-name (lowercase, digits, dashes)",
                repr(&new.slug)
            )));
        }
        let title = new.title.trim();
        if title.is_empty() {
            return Err(refused("a task needs a title".into()));
        }
        if !BEFORE_WORK.contains(&new.status.as_str()) {
            return Err(refused(format!(
                "a new task starts as {}, not {}",
                BEFORE_WORK.join(", "),
                repr(&new.status)
            )));
        }
        if !(0..=4).contains(&new.priority) {
            return Err(refused("priority runs 0 (most urgent) to 4".into()));
        }
        let parent = new.parent.as_deref().filter(|p| !p.is_empty());
        if new.instruction && parent.is_none() {
            return Err(refused(
                "an instruction task is a subtask: it needs a parent".into(),
            ));
        }
        let needed: Vec<Value> = new
            .depends
            .iter()
            .map(String::as_str)
            .chain(parent)
            .map(|other| json!({"slug": other, "missing": format!("{REFUSED}{}", no_task(other))}))
            .collect();
        let links: Vec<Value> = (new.links.iter().map(|l| ("doc", l)))
            .chain(new.depends.iter().map(|d| ("dep", d)))
            .map(|(kind, target)| json!({"kind": kind, "target": target}))
            .collect();
        let mut said = format!("created by {}", new.by);
        if let Some(assignee) = &new.assignee {
            let _ = write!(said, " for {assignee}");
        }
        let act = if new.by == OWNER { "" } else { ACT };
        let body = format!(
            "{act}{CREATE}\n{}\n{}",
            logged("$slug", "$actor", "'system'", "$said", "NONE"),
            block_cancelled_dependencies(false)
        );
        let vars = acting(
            &new.by,
            (self.clock)(),
            json!({
                "slug": new.slug, "title": title, "body": new.body, "status": new.status,
                "priority": new.priority, "parent": parent, "instruction": new.instruction,
                "needed": needed, "links": links, "assignee": new.assignee, "said": said,
                "taken": format!("{REFUSED}task {} already exists", new.slug),
                "unassignable": new.assignee.as_ref()
                    .map(|a| format!("{REFUSED}no session {} to assign it to", repr(a))),
            }),
        );
        self.retrying(|| self.written(&body, vars.clone(), &new.slug))
    }

    /// Set a task's priority, recording the actor and both values without changing its other fields.
    pub fn set_priority(&self, slug: &str, priority: i64, handle: &str) -> Result<Task, Error> {
        if !(0..=4).contains(&priority) {
            return Err(refused("priority runs 0 (most urgent) to 4".into()));
        }
        // Read the old value and write its replacement and history in the same transaction. A
        // concurrent edit conflicts and retries, so history always names the value replaced.
        let act = if handle == OWNER { "" } else { ACT };
        let body = format!(
            "{act}
LET $task = (SELECT priority FROM ONLY type::record('task', $slug));
IF $task = NONE {{ THROW $missing }};
UPDATE type::record('task', $slug) SET priority = $priority, version += 1 RETURN NONE;
LET $said = $actor + ' changed priority p' + <string>$task.priority + ' → p' + <string>$priority;
{}",
            logged("$slug", "$actor", "'system'", "$said", "NONE")
        );
        self.retrying(|| {
            let vars = acting(
                handle,
                (self.clock)(),
                json!({
                    "slug": slug, "priority": priority,
                    "missing": format!("{REFUSED}{}", no_task(slug)),
                }),
            );
            self.written(&body, vars, slug)
        })
    }

    /// Move a task under `parent`, or to the top with none. A task is never its own ancestor.
    pub fn set_parent(&self, slug: &str, parent: Option<&str>, by: &str) -> Result<Task, Error> {
        #[derive(Deserialize)]
        struct Read {
            tree: Vec<(String, Option<String>)>,
            moves: i64,
            me: Option<Value>,
            known: bool,
        }
        let act = if by == OWNER { "" } else { ACT };
        let body = format!(
            "{act}{MOVE_UNDER}\n{}",
            logged("$slug", "$actor", "'system'", "$said", "NONE")
        );
        let said = format!("{by} put it under {}", parent.unwrap_or("nothing"));
        self.retrying(|| {
            let read: Read = self.transact(TREE_READ, json!({"actor": by}))?;
            if by != OWNER {
                if !read.known {
                    return Err(Failure::Stop(refused(unknown_actor(by))));
                }
                if read.me.as_ref().is_some_and(|me| !me["left_at"].is_null()) {
                    return Err(Failure::Stop(refused(left_actor(by))));
                }
            }
            let tree: Tree = read.tree.into_iter().collect();
            if !tree.contains_key(slug) {
                return Err(Failure::Stop(no_task(slug)));
            }
            if let Some(parent) = parent {
                let above = ancestors_in(&tree, parent).map_err(Failure::Stop)?;
                if parent == slug || above.iter().any(|a| a == slug) {
                    return Err(Failure::Stop(refused(format!(
                        "{slug} cannot go under {parent}: it would be its own ancestor"
                    ))));
                }
            }
            let vars = acting(
                by,
                (self.clock)(),
                json!({"slug": slug, "parent": parent, "moves": read.moves, "said": said}),
            );
            self.written(&body, vars, slug)
        })
    }

    /// The parent, its parent, … up to the root, nearest first.
    pub fn ancestors(&self, slug: &str) -> Result<Vec<String>, Error> {
        let read = format!("RETURN {TREE};");
        let tree: Vec<(String, Option<String>)> =
            self.retrying(|| self.transact(&read, json!({})))?;
        ancestors_in(&tree.into_iter().collect(), slug)
    }

    /// The tasks right under `slug`, in the order they were made.
    pub fn children(&self, slug: &str) -> Result<Vec<Task>, Error> {
        let view = self.retrying(|| self.view(&Which::Children(slug)))?;
        if !view.snap.known {
            return Err(no_task(slug));
        }
        Ok(view.tasks())
    }

    /// Every task below `slug`, at any depth, in the order they were made: from one read of the
    /// whole board, so the tree is as of one moment however deep it goes.
    pub fn descendants(&self, slug: &str) -> Result<Vec<Task>, Error> {
        let view = self.retrying(|| self.view(&Which::All))?;
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
        tasks.sort_by_key(|t| t.id);
        Ok(tasks)
    }

    /// Tasks linked to `doc` itself or to one of its headings (`doc#Heading`), by priority then slug.
    pub fn linked_to(&self, doc: &str) -> Result<Vec<Task>, Error> {
        Ok(self.retrying(|| self.view(&Which::Linked(doc)))?.tasks())
    }

    /// Move a task along (`can_transition`). Work started, or held by an executor, is moved only by a
    /// session holding a role on it, except that the owner may cancel any open task and return
    /// blocked work to review.
    pub fn move_to(&self, slug: &str, status: &str, handle: &str) -> Result<Task, Error> {
        if handle == OWNER && matches!(status, "cancelled" | "review") {
            return self.move_as_owner(slug, status);
        }
        self.rule(slug, handle, |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            let current = task.status.as_str();
            if !can_transition(current, status) {
                return Err(refused(format!(
                    "{slug} cannot go from {current} to {status}"
                )));
            }
            if task.instruction && (status == "review" || status == "done") {
                return Err(refused(format!(
                    "{slug} is an instruction subtask: its executor closes it with `done`, not {status}"
                )));
            }
            let held = !BEFORE_WORK.contains(&current) || view.holders("executor").next().is_some();
            if held && !view.snap.roles.iter().any(|r| r.handle == handle) {
                return Err(refused(format!(
                    "{handle} holds no role on {slug}, which is {current}"
                )));
            }
            Ok(Change {
                status: Some(status.into()),
                say: Say::system(format!("{handle} moved it {current} → {status}")),
                ..Change::default()
            })
        })
    }

    /// The owner's two moves: any open task to `cancelled`, a blocked one (never an instruction
    /// subtask) back to `review`.
    fn move_as_owner(&self, slug: &str, status: &str) -> Result<Task, Error> {
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
        let body = format!(
            "LET $task = (SELECT status, instruction FROM ONLY type::record('task', $slug));
IF $task = NONE {{ THROW $missing }};
IF $task.status NOT IN $from {{
    LET $refusal = $cannot + $task.status + ' to ' + $status;
    THROW $refusal;
}};
IF $status = 'review' AND $task.instruction {{ THROW $instruction }};
UPDATE type::record('task', $slug) SET status = $status, updated = $now, version += 1 RETURN NONE;
LET $said = $actor + ' moved it ' + $task.status + ' → ' + $status;
{}\n{}",
            logged("$slug", "$actor", "'system'", "$said", "NONE"),
            block_cancelled_dependencies(cancelling)
        );
        self.retrying(|| {
            let vars = acting(
                OWNER,
                (self.clock)(),
                json!({
                    "slug": slug, "status": status, "from": from,
                    "missing": format!("{REFUSED}{}", no_task(slug)),
                    "cannot": format!("{REFUSED}{slug} cannot go from "),
                    "instruction": format!("{REFUSED}{slug} is an instruction subtask: its executor closes it with `done`, not review"),
                }),
            );
            self.written(&body, vars, slug)
        })
    }

    /// A task in review whose branch `just land` put on main is done: the landing is its verdict. So is
    /// one blocked, which land held there while its stamps waited on nobody (`just land-stamps`) and
    /// could not move back, and one in progress: taken back (by its executor or a changes verdict) after
    /// land's last read of it, as main moved. Land refuses a task in progress before it moves main and
    /// names in progress in `from` only once the branch is on main, so that work landed; further
    /// changes go in a new task. No other command closes a task in progress.
    ///
    /// `from` is which of [`LANDS_FROM`] this landing closes, checked against the status the write
    /// reads, and the answer names the one it found. A task closed from in progress has the verdict
    /// addressed to its executor, saying it was taken back, in the same write.
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
                "a landing closes a task in review, blocked or in_progress, not {status}"
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
                status: Some("done".into()),
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
                ..Change::default()
            })
        })?;
        let from = found.into_inner();
        if from == "in_progress" {
            self.tell_if_unread(
                slug,
                None,
                &format!("the landing of {slug} from {handle} to @executor"),
            );
        }
        Ok(Landed { task, from })
    }

    /// The best work for `handle`: its own reservation, then priority, then its areas, then the order
    /// made.
    pub fn next_for(&self, handle: &str) -> Result<Option<Task>, Error> {
        let view = self.retrying(|| self.view(&Which::Pullable(handle)))?;
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
        let mut best: Option<((bool, i64, bool, i64), Task)> = None;
        for task in view.tasks() {
            if waiting.contains(task.slug.as_str()) {
                continue;
            }
            let reservers: Vec<&str> = (task.roles.iter())
                .filter(|r| {
                    r.role == "reserved" && !matches!(r.health, Health::Lost | Health::Left)
                })
                .map(|r| r.handle.as_str())
                .collect();
            let mine = reservers.contains(&handle);
            if !reservers.is_empty() && !mine {
                continue;
            }
            if (task.roles.iter()).any(|r| r.role == "executor" && r.health.holds()) {
                continue;
            }
            let key = (
                !mine,
                task.priority,
                !areas.contains(&area(&task.slug)),
                task.id,
            );
            if best.as_ref().is_none_or(|(least, _)| key < *least) {
                best = Some((key, task));
            }
        }
        Ok(best.map(|(_, task)| task))
    }

    /// Every session that has held executor on `slug`, now or before (from its claims on the record).
    pub fn ever_executed(&self, slug: &str) -> Result<BTreeSet<String>, Error> {
        #[derive(Deserialize)]
        struct Read {
            known: bool,
            claimed: Vec<String>,
            executors: Vec<String>,
        }
        let read = "RETURN {
            known: record::exists(type::record('task', $slug)),
            claimed: (SELECT VALUE sender FROM message:[$slug, NONE]..[$slug, ..]
                WHERE kind = 'system' AND string::starts_with(text, sender + ' claimed it')),
            executors: (SELECT VALUE handle FROM role WHERE slug = $slug AND role = 'executor'),
        };";
        let read: Read = self.retrying(|| self.transact(read, json!({"slug": slug})))?;
        if !read.known {
            return Err(no_task(slug));
        }
        Ok(read.claimed.into_iter().chain(read.executors).collect())
    }
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
}
