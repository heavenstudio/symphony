//! The orchestrator role the board gives, so every open task has an orchestrator (docs/TASKS.md,
//! "Every open task has an orchestrator"): to a task filed with none ([`Store::give_filed`]), to
//! the work a leave would leave with none (`Store::leave`), to a task moved or reopened out from
//! under one ([`Store::keep_owned`]), and to every open task nobody orchestrates, by the idle sweep
//! and as an orchestrator arrives ([`Store::give_unowned`]).
//!
//! Each give is decided on one read of the whole board and written in one transaction that throws
//! `stale` if what it decided on moved: each task's `version`, the `last_seen` of each lost or left
//! holder it drops, and the receiver still there (fenced as a recipient is, so its leave meets the
//! give). It writes the role, the line on the thread ([`orchestration::given_line`]) by whoever
//! made the give (the board's sweep as [`BOARD`]), and moves the receiver's slot counter on, so a
//! start decided on what it read before is decided again ([`crate::store::cap`]): a give is never
//! refused for the cap, as the owner's hand-over is not. Only the topmost task of each tree with no
//! orchestrator is given ([`orchestration::unowned_tops`]); the tree gives the rest. With no live
//! orchestrator, that task is marked waiting ([`orchestration::WAITING`]), once, and the mark goes
//! when it is given or somebody's again.

use crate::health::FINISHED;
use crate::orchestration::{self, Owners, Pool, WAITING};
use crate::red::FIELDS;
use crate::store::messages::BOARD;
use crate::store::write::FENCE_RECIPIENT;
use crate::store::{ACT, Board, Error, Failure, Store, Task, Which, logged};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};

use super::OWNER;

/// What a pass of giving did: each task given and to whom, and each task marked waiting for an
/// orchestrator, none being live.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Given {
    /// The task, then the session given its orchestrator role.
    pub gave: Vec<(String, String)>,
    /// The tasks that wait for an orchestrator.
    pub waiting: Vec<String>,
    /// Of those, the ones that began waiting with this pass, marked by it.
    pub marked: Vec<String>,
}

/// Whose a task is once a give was looked for ([`Store::give_task`]).
enum Kept {
    /// The orchestrator the tree makes it: nothing written.
    Held(String),
    /// The orchestrator it was given.
    Given(String),
    /// None live: it waits, marked.
    Waiting,
    /// Done, cancelled or not on the board: nothing to give.
    Finished,
}

impl Given {
    /// The one line `to` is told of what it was given: `GIVEN 2 open tasks are yours to
    /// orchestrate: tools/a, tools/b`; none when it was given nothing.
    pub fn told(&self, to: &str) -> Option<String> {
        let slugs: Vec<&str> = (self.gave.iter())
            .filter(|(_, who)| who == to)
            .map(|(slug, _)| slug.as_str())
            .collect();
        orchestration::given_event(&slugs)
    }

    /// Whether it gave nothing and marked nothing.
    pub fn is_empty(&self) -> bool {
        self.gave.is_empty() && self.waiting.is_empty()
    }
}

/// The writes of one pass, decided on one read of the board.
#[derive(Default)]
pub(in crate::store) struct Pass {
    /// Each give: its task, receiver, the holders it drops and its line.
    gives: Vec<Value>,
    /// Each task decided on, with its version as read.
    fenced: Vec<Value>,
    /// Each holder dropped, with its `last_seen` as read.
    seen: Vec<Value>,
    receivers: BTreeSet<String>,
    marks: Vec<String>,
    unmarks: Vec<String>,
    pub(in crate::store) given: Given,
}

/// The tasks' versions as one read gave them, by slug.
pub(in crate::store) type Versions = HashMap<String, i64>;

impl Pass {
    pub(in crate::store) fn is_empty(&self) -> bool {
        self.gives.is_empty() && self.marks.is_empty() && self.unmarks.is_empty()
    }

    /// Give `top` to `to`, or with none mark it waiting: its line names the holders on it, each as
    /// `health` says ([`orchestration::given_line`]), or `why` when it holds none. `kept`: holders
    /// whose rows the pass leaves alone and whose `last_seen` it does not fence (a leaver's, which
    /// its own write drops and moves).
    pub(in crate::store) fn give(
        &mut self,
        board: &Board,
        versions: &Versions,
        top: &Task,
        to: Option<String>,
        why: &str,
        kept: &[&str],
    ) {
        let version = versions.get(&top.slug).copied().unwrap_or(0);
        self.fenced
            .push(json!({"slug": top.slug, "version": version}));
        let marked = orchestration::waiting(&top.fields).is_some();
        let Some(to) = to else {
            if !marked {
                self.marks.push(top.slug.clone());
                self.given.marked.push(top.slug.clone());
            }
            self.given.waiting.push(top.slug.clone());
            return;
        };
        let holders: Vec<&crate::Role> = (top.roles.iter())
            .filter(|r| r.role == "orchestrator" && r.handle != to)
            .collect();
        let from: Vec<(String, crate::Health)> = (holders.iter())
            .map(|r| {
                let health = if kept.contains(&r.handle.as_str()) {
                    crate::Health::Left
                } else {
                    r.health
                };
                (r.handle.clone(), health)
            })
            .collect();
        let dropped: Vec<&str> = (holders.iter())
            .map(|r| r.handle.as_str())
            .filter(|h| !kept.contains(h))
            .collect();
        for handle in &dropped {
            if let Some(s) = board.sessions.iter().find(|s| s.row.handle == *handle) {
                self.seen
                    .push(json!({"handle": handle, "last_seen": s.row.last_seen}));
            }
        }
        let line = orchestration::given_line(&to, &from, why);
        self.gives
            .push(json!({"slug": top.slug, "to": to, "drop": dropped, "line": line}));
        if marked {
            self.unmarks.push(top.slug.clone());
        }
        self.receivers.insert(to.clone());
        self.given.gave.push((top.slug.clone(), to));
    }

    /// Lift the waiting mark from each task of `board` that carries it and is somebody's now,
    /// finished, or no longer the top of what waits.
    fn unmark_settled(&mut self, board: &Board, versions: &Versions) {
        let decided: BTreeSet<&str> = (self.fenced.iter())
            .filter_map(|f| f["slug"].as_str())
            .collect();
        let settled: Vec<&Task> = (board.tasks.iter())
            .filter(|t| orchestration::waiting(&t.fields).is_some())
            .filter(|t| !decided.contains(t.slug.as_str()))
            .collect();
        for task in settled {
            let version = versions.get(&task.slug).copied().unwrap_or(0);
            self.fenced
                .push(json!({"slug": task.slug, "version": version}));
            self.unmarks.push(task.slug.clone());
        }
    }

    /// The pass's variables, beside `more`'s.
    pub(in crate::store) fn vars(&self, sender: &str, mut more: Value) -> Value {
        let own = json!({
            "gives": self.gives,
            "fenced": self.fenced,
            "seen": self.seen,
            "receivers": self.receivers,
            "marks": self.marks,
            "unmarks": self.unmarks,
            "sender": sender,
        });
        if let (Some(vars), Value::Object(own)) = (more.as_object_mut(), own) {
            vars.extend(own);
        }
        more
    }
}

/// The checks a pass's write makes first, before any statement of its own moves a version or a
/// `last_seen`: each task as read, each dropped holder as read. Needs `$fenced` and `$seen`.
pub(in crate::store) const FENCES: &str = "
FOR $g IN $fenced {
    IF (SELECT VALUE version FROM ONLY type::record('task', $g.slug)) != $g.version { THROW 'stale' };
};
FOR $s IN $seen {
    IF (SELECT VALUE last_seen FROM ONLY type::record('session', $s.handle)) != $s.last_seen { THROW 'stale' };
};";

/// A pass's writes, after its [`FENCES`]: each receiver still there, fenced as a recipient is; each
/// give's role, version, slot counter and line, sent by `$sender`; each mark set or lifted. Needs
/// `$receivers`, `$gives`, `$marks`, `$unmarks`, `$sender` and `$now`.
pub(in crate::store) fn gives() -> String {
    format!(
        "
FOR $handle IN $receivers {{
    IF !record::exists(type::record('session', $handle)) OR type::record('session', $handle).left_at != NONE {{ THROW 'stale' }};
    {FENCE_RECIPIENT}
}};
FOR $g IN $gives {{
    FOR $h IN $g.drop {{ DELETE type::record('role', [$g.slug, $h, 'orchestrator']) RETURN NONE; }};
    UPSERT type::record('role', [$g.slug, $g.to, 'orchestrator'])
        CONTENT {{ slug: $g.slug, handle: $g.to, role: 'orchestrator', since: $now }} RETURN NONE;
    UPDATE type::record('task', $g.slug) SET version += 1 RETURN NONE;
    UPSERT type::record('counter', ['slots', $g.to]) SET last += 1 RETURN NONE;
    {}
}};
FOR $slug IN $marks {{
    UPDATE type::record('task', $slug) SET version += 1, fields.{FIELDS}.{WAITING} = $now RETURN NONE;
}};
FOR $slug IN $unmarks {{
    UPDATE type::record('task', $slug) SET version += 1, fields.{FIELDS}.{WAITING} = NONE RETURN NONE;
}};",
        logged("$g.slug", "$sender", "'system'", "$g.line", "NONE")
    )
}

impl Store {
    /// The whole board as one read gives it, with each task's version.
    pub(in crate::store) fn board_versions(&self) -> Result<(Board, Versions), Failure> {
        let view = self.view(&Which::All)?;
        let versions = (view.snap.tasks.iter())
            .map(|t| (t.row.slug.clone(), t.version))
            .collect();
        Ok((Store::board_view(view), versions))
    }

    /// Write `pass` as `by`: a session's (checked and seen as any write's actor), the owner's, or,
    /// with none, the board's sweep's ([`BOARD`]).
    fn write_pass(&self, pass: &Pass, by: Option<&str>) -> Result<(), Failure> {
        if pass.is_empty() {
            return Ok(());
        }
        let session = by.filter(|by| *by != OWNER);
        let act = if session.is_some() { ACT } else { "" };
        let body = format!("{act}{FENCES}{}\nRETURN NONE;", gives());
        let now = (self.clock)();
        let sender = by.unwrap_or(BOARD);
        let vars = match session {
            Some(actor) => self.acting(actor, now, json!({})),
            None => json!({"now": now}),
        };
        self.transact::<Value>(&body, pass.vars(sender, vars))
            .map(|_| ())
    }

    /// Give every open task nobody orchestrates to a live orchestrator, the topmost of each tree
    /// ([`orchestration::unowned_tops`]) by rule 4 ([`Pool`]), one line on each thread; with none
    /// live, mark each waiting, and lift the mark from those no longer waiting. `by`: the
    /// orchestrator arriving (registering, or beating after it was not live), whose own write it
    /// is, or none for the idle sweep (`tasks reap`), whose lines are the board's. What it gave,
    /// and what waits.
    pub fn give_unowned(&self, by: Option<&str>) -> Result<Given, Error> {
        self.unowned_pass(by, true)
    }

    /// What [`Store::give_unowned`] would give and mark now, writing nothing (`tasks reap
    /// --dry-run`).
    pub fn would_give_unowned(&self) -> Result<Given, Error> {
        self.unowned_pass(None, false)
    }

    fn unowned_pass(&self, by: Option<&str>, write: bool) -> Result<Given, Error> {
        self.retrying(|| {
            let (board, versions) = self.board_versions()?;
            let mut pool = Pool::of(&board, &[]);
            let mut pass = Pass::default();
            for top in orchestration::unowned_tops(&board.tasks) {
                let why = if orchestration::waiting(&top.fields).is_some() {
                    "it waited for an orchestrator"
                } else {
                    "nobody orchestrated it"
                };
                pass.give(&board, &versions, top, pool.pick(), why, &[]);
            }
            pass.unmark_settled(&board, &versions);
            if write {
                self.write_pass(&pass, by)?;
            }
            Ok(pass.given)
        })
    }

    /// The orchestrator of `slug`, a task `by` just filed (`tasks create`, `tasks api create`, the
    /// owner's quick task, the board's own filings): the one holding a task above it, as the tree
    /// makes it; else `by` itself when it is an orchestrator ([`Store::orchestrate_filed`]); else,
    /// given to the first live orchestrator of `to` (`tasks create --no-role --for`), the
    /// orchestrator of the task `by` works on, and `by`'s parent session; else by rule 4
    /// ([`Pool`]). Some orchestrator, or none when none is live and the task waits.
    pub fn give_filed(
        &self,
        slug: &str,
        by: &str,
        to: Option<&str>,
    ) -> Result<Option<String>, Error> {
        if to.is_none() && by != OWNER {
            match self.orchestrate_filed(slug, by) {
                Ok(true) => return Ok(Some(by.to_owned())),
                Ok(false) | Err(Error::Refused(_)) => {}
                Err(error) => return Err(error),
            }
        }
        let why = format!("filed by {by}");
        let kept = self.give_task(slug, by, &why, &|board: &Board| {
            let mut first: Vec<String> = to.map(str::to_owned).into_iter().collect();
            // The work `by` holds a role on, the latest taken first: its orchestrators.
            let mut working: Vec<(f64, &str)> = Vec::new();
            let open = |t: &&Task| !FINISHED.contains(&t.status.as_str()) && t.slug != slug;
            for task in board.tasks.iter().filter(open) {
                let roles = task.roles.iter();
                let mine = roles.filter(|r| r.handle == by && r.role != "orchestrator");
                working.extend(mine.map(|r| (r.since, task.slug.as_str())));
            }
            working.sort_by(|a, b| b.0.total_cmp(&a.0));
            let owners = Owners::new(&board.tasks);
            for (_, task) in working {
                first.extend(owners.of(task).into_iter().map(str::to_owned));
            }
            let parent = (board.sessions.iter())
                .find(|s| s.row.handle == by)
                .and_then(|s| s.row.parent.clone());
            first.extend(parent);
            first
        })?;
        Ok(match kept {
            Kept::Held(owner) | Kept::Given(owner) => Some(owner),
            Kept::Waiting | Kept::Finished => None,
        })
    }

    /// Keep `slug` somebody's after `by` reopened it: when nobody orchestrates it now, give it by
    /// rule 4, `why` on the line. Whether that wrote anything: a give, or the task marked waiting.
    pub(crate) fn keep_owned(&self, slug: &str, by: &str, why: &str) -> Result<bool, Error> {
        let kept = self.give_task(slug, by, why, &|_: &Board| Vec::new())?;
        Ok(matches!(kept, Kept::Given(_) | Kept::Waiting))
    }

    /// The orchestrators of `slug` as the tree makes them now, nearest first ([`Owners::of`]).
    pub(crate) fn orchestrators_of(&self, slug: &str) -> Result<Vec<String>, Error> {
        let mut chain = vec![slug.to_owned()];
        chain.extend(self.ancestors(slug)?);
        let tasks = self.tasks_by_slug(&chain)?;
        Ok(Owners::new(&tasks)
            .of(slug)
            .into_iter()
            .map(str::to_owned)
            .collect())
    }

    /// Give the topmost task nobody orchestrates at or above `slug` to the first live orchestrator
    /// `first` names on the board read, else by rule 4, as `by` with `why` on its line.
    fn give_task(
        &self,
        slug: &str,
        by: &str,
        why: &str,
        first: &dyn Fn(&Board) -> Vec<String>,
    ) -> Result<Kept, Error> {
        // Most tasks are somebody's by the tree: read the line of parents before the whole board.
        match self.orchestrators_of(slug) {
            Ok(owners) if !owners.is_empty() => return Ok(Kept::Held(owners[0].clone())),
            Ok(_) | Err(Error::Refused(_)) => {}
            Err(error) => return Err(error),
        }
        let actor = self.give_actor(by)?;
        self.retrying(|| {
            let (board, versions) = self.board_versions()?;
            let owners = Owners::new(&board.tasks);
            let task = board.tasks.iter().find(|t| t.slug == slug);
            let Some(task) = task.filter(|t| !FINISHED.contains(&t.status.as_str())) else {
                return Ok(Kept::Finished);
            };
            if let Some(owner) = owners.of(&task.slug).first() {
                return Ok(Kept::Held((*owner).to_owned()));
            }
            let mut chain = vec![task.slug.as_str()];
            let mut at = task.parent.as_deref();
            while let Some(parent) = at.filter(|p| !chain.contains(p)) {
                chain.push(parent);
                at = (board.tasks.iter())
                    .find(|t| t.slug == parent)
                    .and_then(|t| t.parent.as_deref());
            }
            let tops = orchestration::unowned_tops(&board.tasks);
            let Some(top) = tops.into_iter().find(|t| chain.contains(&t.slug.as_str())) else {
                return Ok(Kept::Finished);
            };
            let mut pool = Pool::of(&board, &[]);
            let to = match first(&board).into_iter().find(|h| pool.holds(h)) {
                Some(to) => {
                    pool.give(&to);
                    Some(to)
                }
                None => pool.pick(),
            };
            let mut pass = Pass::default();
            pass.give(&board, &versions, top, to.clone(), why, &[]);
            self.write_pass(&pass, actor.as_deref())?;
            Ok(to.map_or(Kept::Waiting, Kept::Given))
        })
    }

    /// Who a give `by` writes as: `by` while it is a session still there or the owner, else the
    /// board (a filer that left, a name no session has).
    fn give_actor(&self, by: &str) -> Result<Option<String>, Error> {
        if by == OWNER {
            return Ok(Some(OWNER.to_owned()));
        }
        match self.session(by) {
            Ok(session) if session.row.left_at.is_none() => Ok(Some(by.to_owned())),
            Ok(_) | Err(Error::Refused(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }
}
