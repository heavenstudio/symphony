//! What an open task waits on, why in a few words and since when: the one line a card of the board
//! ends in (docs/workflow/TASKS.md, "What a task waits on"), read from the board alone: its roles' health,
//! its dependencies and the tasks under it, what its block names ([`Blocked`]) and what its
//! landing lacks ([`crate::alarm::WAITS`]). Who is there is the PULL digest's rule ([`Look`]),
//! which the digest calls, so the board and PULL never disagree on it.
//!
//! A block names a thing, never a person ([`Thing`]): `just tasks move SLUG blocked --on THING`
//! refuses one that names none. A block made without one (by an older build, or a caller of the
//! library or the api that gives none) reads `nobody · no reason recorded`, and is not blocked on
//! the wire: the listings send it in the stage it stopped in ([`Line::unnamed`]).

use crate::alarm::{self, Assessed, Waits, Written};
use crate::health::{EXECUTING, FINISHED};
use crate::land::needs::{OWNER, REVIEW};
use crate::{Health, Task};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

/// Someone there holds the role the stage needs: its executor, or its reviewer in review.
pub const WORKING: &str = "working";
/// A decision the owner has to make: a card, the owner's sign-off, or his approve of the tip (the
/// `owner` stamp, alone what a block or a landing waits on: [`OWNER_APPROVE`]).
pub const DECISION: &str = "decision";
/// A check or stamp someone other than the owner gives: one a block names, or what its landing
/// lacks.
pub const CHECK: &str = "check";
/// Another task still open: a dependency, or one a block names.
pub const TASK: &str = "task";
/// Work not started, with nothing before it: it waits to be staffed.
pub const QUEUED: &str = "queued";
/// Blocked on purpose: a recurrence, an exemption that stays, a quiet machine.
pub const PARKED: &str = "parked";
/// Nobody: a role's holder lost or left, a role nobody holds, a dependency gone, a block that names
/// nothing.
pub const NOBODY: &str = "nobody";

/// The board field (beside [`crate::alarm::WAITS`] in [`crate::red::FIELDS`]) in which a block keeps
/// what it waits on ([`Blocked`]), written in the move's own write and removed by every write that
/// takes the task out of blocked.
pub const BLOCKED: &str = "blocked";
/// The most characters a block's why takes: a few words, never a brief.
pub const WHY_LIMIT: usize = 120;
/// What a block that names nothing waits on.
pub const NO_REASON: &str = "no reason recorded";
/// The decision a task waits on when the owner's stamp is all it lacks (`check:owner`, the
/// default landing's hold, or a landing's record naming `owner` alone): his approve, never him.
pub const OWNER_APPROVE: &str = "the owner's approve of its tip";
/// The things a block names, as a refusal says them.
pub const THINGS: &str = "decision:NAME, check:NAME, task:SLUG or parked";

/// What a block waits on: an owner's decision (a card, by its name), a check (a stamp, by its
/// name), another task (by its slug), or nothing but time (`parked`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thing<'a> {
    Decision(&'a str),
    Check(&'a str),
    Task(&'a str),
    Parked,
}

impl<'a> Thing<'a> {
    /// The thing `text` names, `decision:NAME`, `check:NAME`, `task:SLUG` or `parked`; a refusal
    /// naming the four for anything else. A name is one word: no space, no comma.
    pub fn parse(text: &'a str) -> Result<Thing<'a>, String> {
        let text = text.trim();
        if text == PARKED {
            return Ok(Thing::Parked);
        }
        let refusal = || format!("a block names what it waits on, one of {THINGS}: not {text:?}");
        let (kind, name) = text.split_once(':').ok_or_else(refusal)?;
        let one_word = !name.is_empty() && !name.chars().any(|c| c.is_whitespace() || c == ',');
        if !one_word {
            return Err(refusal());
        }
        match kind {
            DECISION => Ok(Thing::Decision(name)),
            CHECK => Ok(Thing::Check(name)),
            TASK => Ok(Thing::Task(name)),
            _ => Err(refusal()),
        }
    }

    /// The name it holds; none for `parked`.
    pub fn name(self) -> &'a str {
        match self {
            Thing::Decision(name) | Thing::Check(name) | Thing::Task(name) => name,
            Thing::Parked => "",
        }
    }
}

impl fmt::Display for Thing<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Thing::Decision(name) => write!(f, "{DECISION}:{name}"),
            Thing::Check(name) => write!(f, "{CHECK}:{name}"),
            Thing::Task(name) => write!(f, "{TASK}:{name}"),
            Thing::Parked => f.write_str(PARKED),
        }
    }
}

/// A block's why as given: trimmed, in words, and a few of them ([`WHY_LIMIT`]); a refusal saying
/// so otherwise.
pub fn why(text: &str) -> Result<&str, String> {
    let text = text.trim();
    let count = text.chars().count();
    if text.is_empty() {
        Err("a block's why says it in words".into())
    } else if count > WHY_LIMIT {
        Err(format!(
            "a block's why is a few words, at most {WHY_LIMIT} characters: this one is {count}; \
             the thread takes the rest"
        ))
    } else {
        Ok(text)
    }
}

/// What a block waits on, as the board keeps it ([`BLOCKED`]): the thing (`on`, as [`Thing`] says
/// it), why in a few words, who blocked it, when, and the status it was blocked from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Blocked {
    pub on: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    pub by: String,
    pub at: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

impl Blocked {
    /// The record on `task` while it is blocked, if one is: none for a task in any other status,
    /// whatever its fields hold.
    pub fn of(task: &Task) -> Option<Blocked> {
        Blocked::kept(&task.status, &task.fields)
    }

    /// The record `fields` keep, of a task in `status`, as [`Blocked::of`] reads it.
    pub fn kept(
        status: &str,
        fields: &BTreeMap<String, BTreeMap<String, Value>>,
    ) -> Option<Blocked> {
        if status != "blocked" {
            return None;
        }
        let kept = fields.get(crate::red::FIELDS)?.get(BLOCKED)?;
        crate::objects::read(kept).ok()
    }

    /// The record as the board writes it.
    pub fn value(&self) -> Value {
        serde_json::to_value(self).expect("a block's record serializes")
    }

    /// What it names; none for a record no build of the board wrote.
    pub fn thing(&self) -> Option<Thing<'_>> {
        Thing::parse(&self.on).ok()
    }
}

/// The board as the PULL digest reads who is there: each task by slug, the tasks under each, and
/// the sessions running a test (`busy`), which count as there whatever their beat says.
pub struct Look<'a> {
    by_slug: HashMap<&'a str, &'a Task>,
    kids: HashMap<&'a str, Vec<&'a Task>>,
    busy: HashSet<&'a str>,
}

impl<'a> Look<'a> {
    pub fn new(tasks: &'a [Task], busy: &'a [String]) -> Look<'a> {
        let by_slug: HashMap<&str, &Task> = tasks.iter().map(|t| (t.slug.as_str(), t)).collect();
        let mut kids: HashMap<&str, Vec<&Task>> = HashMap::new();
        for t in tasks {
            if let Some(parent) = t.parent.as_deref().filter(|p| by_slug.contains_key(p)) {
                kids.entry(parent).or_default().push(t);
            }
        }
        Look {
            by_slug,
            kids,
            busy: busy.iter().map(String::as_str).collect(),
        }
    }

    /// The task the board holds by `slug`.
    pub fn task(&self, slug: &str) -> Option<&'a Task> {
        self.by_slug.get(slug).copied()
    }

    /// The tasks under `task` still to be executed, at any depth: a subtask in review, done or
    /// cancelled has an executor that is waiting or finished, not on this work.
    pub fn below(&self, task: &Task) -> Vec<&'a Task> {
        let children = |slug: &str| self.kids.get(slug).cloned().unwrap_or_default();
        let (mut found, mut todo) = (Vec::new(), children(&task.slug));
        while let Some(child) = todo.pop() {
            if EXECUTING.contains(&child.status.as_str()) {
                found.push(child);
            }
            todo.extend(children(&child.slug));
        }
        found
    }

    /// Whether a session that is there holds `role` on `task`: live, stale or awaiting a sweep
    /// ([`Health::holds`]), or running a test.
    pub fn present(&self, task: &Task, role: &str) -> bool {
        task.roles.iter().any(|r| r.role == role && self.there(r))
    }

    /// Whether its executor is there, on it or on a task under it still to be executed.
    pub fn staffed(&self, task: &Task) -> bool {
        self.present(task, "executor")
            || self.below(task).iter().any(|c| self.present(c, "executor"))
    }

    /// Its dependencies the board holds cancelled, in the order it names them.
    pub fn cancelled<'t>(&self, task: &'t Task) -> Vec<&'t str> {
        self.dependencies(task, |d| d.status == "cancelled")
    }

    /// Its dependencies the board holds open: neither done nor cancelled.
    pub fn open<'t>(&self, task: &'t Task) -> Vec<&'t str> {
        self.dependencies(task, |d| !FINISHED.contains(&d.status.as_str()))
    }

    /// Whether every dependency the board holds is done: nothing open before it, nothing cancelled.
    pub fn free(&self, task: &Task) -> bool {
        (task.depends.iter())
            .filter_map(|d| self.task(d))
            .all(|d| d.status == "done")
    }

    /// The ready tasks of `tasks` no executor that is there holds, with every dependency done:
    /// what waits to start, the digest's `ready_unstaffed` and a ready red's alike.
    pub fn unstarted(&self, tasks: &[Task]) -> usize {
        (tasks.iter())
            .filter(|t| t.status == "ready" && self.free(t) && !self.staffed(t))
            .count()
    }

    fn there(&self, role: &crate::Role) -> bool {
        role.health.holds() || self.busy.contains(role.handle.as_str())
    }

    fn dependencies<'t>(&self, task: &'t Task, keep: impl Fn(&Task) -> bool) -> Vec<&'t str> {
        (task.depends.iter())
            .filter(|slug| self.task(slug).is_some_and(&keep))
            .map(String::as_str)
            .collect()
    }

    /// The first holder of `role` on `task` that is there, as the line names it, and the role it
    /// holds; then the first under it ([`Look::under_work`]).
    fn at_work(&self, task: &Task, role: &str) -> Option<(String, String)> {
        (self.held(task, role))
            .map(|who| (who, role.to_string()))
            .or_else(|| self.under_work(task, role))
    }

    /// The first holder of `role` that is there on a task under `task` still to be executed, the
    /// oldest made first, and the role it holds there: `executor of SLUG`.
    fn under_work(&self, task: &Task, role: &str) -> Option<(String, String)> {
        let mut under = self.below(task);
        #[expect(
            clippy::disallowed_methods,
            reason = "a busy board path keeps std's in-place sort (crate::order): PULL's waits"
        )]
        under.sort_unstable_by_key(|t| t.id);
        (under.iter()).find_map(|child| {
            (self.held(child, role)).map(|who| (who, format!("{role} of {}", child.slug)))
        })
    }

    fn held(&self, task: &Task, role: &str) -> Option<String> {
        (task.roles.iter())
            .find(|r| r.role == role && self.there(r))
            .map(holder)
    }

    /// A holder of `role` on `task` that is gone, as the line says it: `executor amber-heron lost`.
    fn gone(&self, task: &Task, role: &str) -> Option<(String, String)> {
        (task.roles.iter())
            .find(|r| r.role == role && !self.there(r))
            .map(|r| {
                (
                    r.handle.clone(),
                    format!("{role} {} {}", r.handle, r.health.as_str()),
                )
            })
    }
}

/// A holder as the line names it: its handle, `(stale)` after a stale one's.
fn holder(role: &crate::Role) -> String {
    if role.health == Health::Stale {
        format!("{} (stale)", role.handle)
    } else {
        role.handle.clone()
    }
}

/// What a task's landing waits on, as the tool that lands it records it ([`alarm::WAITS`]), read
/// by the line: the names it lacks but [`alarm::CHANGES`], whether the record judged the tip, and
/// whether it asks for changes the branch has not answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Landing<'a> {
    pub names: Vec<&'a str>,
    pub current: bool,
    pub reworking: bool,
}

impl<'a> Landing<'a> {
    /// The record on `task`, judged at `tip`, the branch's tip now; with none known (no repository
    /// read, as the server's listing reads none), taken to be at the tip it judged, as PULL takes
    /// it without a repository. Changes the board keeps the branch's answer to ([`alarm::FIXED`])
    /// are answered; with nothing kept, they stand.
    pub fn of(task: &'a Task, tip: Option<&str>) -> Option<Landing<'a>> {
        let waits = Waits::of(task)?;
        let current = waits.judged(tip);
        let answered = (Assessed::of(task))
            .and_then(|assessed| {
                assessed.sid.map(|sid| Written {
                    tip: waits.tip,
                    at: assessed.at,
                    sid,
                })
            })
            .is_some_and(|record| alarm::kept_fix(task, record).is_some());
        Some(Landing {
            reworking: current && waits.changes() && !answered,
            names: (waits.on.iter().copied())
                .filter(|name| *name != alarm::CHANGES)
                .collect(),
            current,
        })
    }
}

/// What a task waits on, but since when, which the replay of its thread says
/// ([`crate::stages::in_status`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// One of the kinds above: [`symphony_api::WAITS_KINDS`].
    pub kind: &'static str,
    /// Who or what: the holder at work, the decision, the checks, the tasks; empty where none is.
    pub who: String,
    pub why: String,
    /// A block that names nothing it waits on: on the wire, not blocked, but in the stage it
    /// stopped in.
    pub unnamed: bool,
}

impl Line {
    fn new(kind: &'static str, who: impl Into<String>, why: impl Into<String>) -> Line {
        Line {
            kind,
            who: who.into(),
            why: why.into(),
            unnamed: false,
        }
    }
}

/// What `task` waits on, read on the board `look` sees, its landing's record judged by `landing`
/// ([`Landing::of`]); none for a finished task.
pub fn line(task: &Task, look: &Look<'_>, landing: Option<&Landing<'_>>) -> Option<Line> {
    let status = task.status.as_str();
    if FINISHED.contains(&status) {
        return None;
    }
    if status == "blocked" {
        return Some(blocked(task, look, landing));
    }
    let cancelled = look.cancelled(task);
    if !cancelled.is_empty() {
        return Some(cancelled_line(&cancelled));
    }
    let reworking = status == "review" && landing.is_some_and(|l| l.reworking);
    Some(match status {
        "review" if !reworking => in_review(task, look, landing),
        "review" | "in_progress" => in_progress(task, look, reworking),
        _ => before_work(task, look),
    })
}

/// Blocked: what its record names; with none, the board's own block (a cancelled dependency), then
/// what its landing lacks (a consumer's landing holds it so); with none of them, nothing.
fn blocked(task: &Task, look: &Look<'_>, landing: Option<&Landing<'_>>) -> Line {
    let record = Blocked::of(task);
    if let Some(record) = &record
        && let Some(thing) = record.thing()
    {
        let said = |or: &str| record.why.clone().unwrap_or_else(|| or.into());
        return match thing {
            Thing::Decision(name) => Line::new(DECISION, name, said("the owner's decision")),
            // The owner's stamp is the owner's to give: what waits on him is a decision, named by
            // his approve, never by him. A why describes the block, not the decision's name;
            // a block made by hand need not have a landing behind it.
            Thing::Check(OWNER) => Line::new(DECISION, OWNER_APPROVE, said("the owner's approval")),
            Thing::Check(name) => Line::new(CHECK, name, said("a check to give")),
            Thing::Parked => Line::new(PARKED, "", said("parked on purpose")),
            Thing::Task(slug) => match look.task(slug).map(|t| t.status.as_str()) {
                None => Line::new(
                    NOBODY,
                    slug,
                    format!("its dependency {slug} is not on the board"),
                ),
                Some("done") => Line::new(NOBODY, slug, format!("its dependency {slug} is done")),
                Some("cancelled") => cancelled_line(&[slug]),
                Some(_) => Line::new(TASK, slug, said("waits on it")),
            },
        };
    }
    let cancelled = look.cancelled(task);
    if !cancelled.is_empty() {
        return cancelled_line(&cancelled);
    }
    if let Some(landing) = landing.filter(|l| !l.names.is_empty()) {
        return lacking(&landing.names);
    }
    Line {
        unnamed: true,
        ..Line::new(NOBODY, "", NO_REASON)
    }
}

/// What a landing that lacks the stamps `names` waits on: the owner's approve alone is his
/// decision ([`OWNER_APPROVE`]); any other stamp is a check someone gives first.
fn lacking(names: &[&str]) -> Line {
    match names {
        [OWNER] => Line::new(DECISION, OWNER_APPROVE, "its landing lacks it"),
        _ => Line::new(CHECK, names.join(", "), "its landing lacks them"),
    }
}

fn cancelled_line(cancelled: &[&str]) -> Line {
    let why = match cancelled {
        [one] => format!("its dependency {one} was cancelled"),
        many => format!("its dependencies {} were cancelled", many.join(", ")),
    };
    Line::new(NOBODY, cancelled.join(", "), why)
}

/// In review, its tip not judged to need changes: its reviewer at work, or an executor still at
/// work under it; else what its landing lacks; with nothing, it waits to land.
fn in_review(task: &Task, look: &Look<'_>, landing: Option<&Landing<'_>>) -> Line {
    if let Some((who, why)) = look.at_work(task, "reviewer") {
        return Line::new(WORKING, who, why);
    }
    if let Some((who, why)) = look.under_work(task, "executor") {
        return Line::new(WORKING, who, why);
    }
    let reviewer = || {
        (look.gone(task, "reviewer")).map_or_else(
            || Line::new(NOBODY, "", "no reviewer"),
            |(who, why)| Line::new(NOBODY, who, why),
        )
    };
    match landing {
        Some(landing) if !landing.current => Line::new(
            CHECK,
            landing.names.join(", "),
            "its tip is newer than its landing's last look",
        ),
        Some(landing) if landing.names.contains(&REVIEW) => reviewer(),
        Some(landing) if !landing.names.is_empty() => lacking(&landing.names),
        Some(_) => Line::new(QUEUED, "", "nothing missing: it waits to land"),
        None => reviewer(),
    }
}

/// In progress, or in review with changes to make: its executor at work, on it or under it.
fn in_progress(task: &Task, look: &Look<'_>, reworking: bool) -> Line {
    if let Some((who, why)) = look.at_work(task, "executor") {
        let why = if reworking {
            format!("{why}, making the changes asked")
        } else {
            why
        };
        return Line::new(WORKING, who, why);
    }
    if let Some((who, why)) = look.gone(task, "executor") {
        return Line::new(NOBODY, who, why);
    }
    Line::new(
        NOBODY,
        "",
        if reworking {
            "no executor to make the changes asked"
        } else {
            "no executor"
        },
    )
}

/// Not started (backlog, concept, design or ready): its executor at work, the tasks before it still
/// open, a holder gone; else it waits to be staffed.
fn before_work(task: &Task, look: &Look<'_>) -> Line {
    if let Some((who, why)) = look.at_work(task, "executor") {
        return Line::new(WORKING, who, why);
    }
    let open = look.open(task);
    if !open.is_empty() {
        let why = if open.len() == 1 {
            "its dependency"
        } else {
            "its dependencies"
        };
        return Line::new(TASK, open.join(", "), why);
    }
    if let Some((who, why)) = look.gone(task, "executor") {
        return Line::new(NOBODY, who, why);
    }
    let reserved = (task.roles.iter()).find(|r| r.role == "reserved" && r.health.holds());
    match (task.status.as_str(), reserved) {
        (_, Some(r)) => Line::new(
            QUEUED,
            r.handle.clone(),
            format!("reserved for {}", r.handle),
        ),
        ("backlog", None) => Line::new(QUEUED, "", "in the backlog"),
        _ => Line::new(QUEUED, "", "waits to be staffed"),
    }
}

/// Each session holding the orchestrator role on an open task of `tasks`, by handle, with its
/// health: the board says once when none is live.
pub fn orchestrators(tasks: &[Task]) -> Vec<(String, Health)> {
    let mut found: std::collections::BTreeMap<&str, Health> = std::collections::BTreeMap::new();
    for task in tasks
        .iter()
        .filter(|t| !FINISHED.contains(&t.status.as_str()))
    {
        for role in task.roles.iter().filter(|r| r.role == "orchestrator") {
            found.insert(&role.handle, role.health);
        }
    }
    found
        .into_iter()
        .map(|(handle, health)| (handle.to_string(), health))
        .collect()
}

/// How long `seconds` is, as the line says it: `42 s`, `13 min`, `5 h`, `2 d`.
pub fn age(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    if seconds < 90.0 {
        format!("{seconds:.0} s")
    } else if seconds < 90.0 * 60.0 {
        format!("{:.0} min", seconds / 60.0)
    } else if seconds < 36.0 * 3600.0 {
        format!("{:.0} h", seconds / 3600.0)
    } else {
        format!("{:.0} d", seconds / 86_400.0)
    }
}

/// The line as `tasks show` prints it: `Waits: on KIND[: WHO] · WHY · AGE`, its who left out where
/// its why says it already.
pub fn said(line: &Line, seconds: f64) -> String {
    let names_who = !line.who.is_empty()
        && line.why.match_indices(&line.who).any(|(at, who)| {
            let word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '/' | '.');
            !line.why[..at].ends_with(word) && !line.why[at + who.len()..].starts_with(word)
        });
    let who = if line.who.is_empty() || names_who {
        String::new()
    } else {
        format!(": {}", line.who)
    };
    format!(
        "Waits: on {}{who} · {} · {}",
        line.kind,
        line.why,
        age(seconds)
    )
}

#[cfg(test)]
mod tests;
