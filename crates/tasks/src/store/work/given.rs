//! The orchestrator role the board gives, so every open task has an orchestrator (docs/workflow/TASKS.md,
//! "Every open task has an orchestrator"): to a task filed with none ([`Store::give_filed`]), to
//! the work a leave would leave with none (`Store::leave`), to a task moved out from under its
//! orchestrator's task, to that one ([`Store::kept_at_move`]), or reopened with none
//! ([`Store::keep_owned`]), and to every open task nobody orchestrates
//! ([`Store::give_unowned`]), which every reader of the whole board passes on when its read shows
//! one ([`Store::give_if_due`]): the server's clock, a detached dispatcher, an orchestrator's
//! `tasks wait`, monitor and `tasks pull`, and the idle sweep. A task its reviewer approved that no
//! landing has closed is given as open work is, though it reads done ([`orchestration::owed`]).
//!
//! Each give is decided on one read and written in one transaction that throws `stale` if what it
//! decided on moved: each task's `version`, the `last_seen` of each lost or left holder it drops,
//! and the receiver still there (fenced as a recipient is, so its leave meets the give). It writes
//! the role, the line on the thread ([`orchestration::given_line`]) by whoever made the give (the
//! board's own as [`BOARD`]), and moves the receiver's slot counter on, so a start decided on what
//! it read before is decided again ([`crate::store::cap`]): a give is never refused for the cap,
//! as the owner's hand-over is not, but a move's, which the move's own cap read decides. Only the
//! topmost task of each tree with no orchestrator is given ([`orchestration::unowned_tops`]); the
//! tree gives the rest. The board's own choice is an orchestrator that stays ([`Pool::of`]) and
//! holds fewer open tasks than the limit (`[limits] orchestrator_tasks`, [`Pool::within`]), but
//! for a red, which goes to one that stays whatever it holds; the orchestrator a filing's own story
//! names is passed over at the limit, and the filer told why ([`Store::give_filed_saying`]). With
//! none, that task is marked waiting ([`orchestration::WAITING`]), once, and the mark goes when it
//! is given or somebody's again. The limit is fenced as the cap is: a pass holds each receiver's
//! slot counter as it read it before the board, and a take or hand-over of the role that the limit
//! holds reads the taker's open tasks and its counter together (`postgres::work::GAINING`, `Store::gaining`), so
//! two writes that each saw a place cannot both fill it.

use crate::health::FINISHED;
use crate::orchestration::{self, Owners, Pool};
use crate::store::messages::BOARD;
use crate::store::postgres::{cap as pg_cap, work as pg};
use crate::store::statement::Statement;
use crate::store::{Board, Error, Failure, Store, Task, Which};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};

use super::OWNER;

/// What a pass of giving did: each task given and to whom, and each task marked waiting for an
/// orchestrator, none that stays being there.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Given {
    /// The task, then the session given its orchestrator role.
    pub gave: Vec<(String, String)>,
    /// The line each of those gives wrote on its task's thread, in the same order.
    pub lines: Vec<String>,
    /// The tasks that wait for an orchestrator.
    pub waiting: Vec<String>,
    /// Of those, the ones that began waiting with this pass, marked by it.
    pub marked: Vec<String>,
}

/// Who orchestrates a task just filed ([`Store::give_filed_saying`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Filed {
    /// Its orchestrator; none while it waits for one.
    pub orchestrator: Option<String>,
    /// Why the orchestrator its story named first was passed over: the filer refused the role, or
    /// one at the limit on the open tasks it holds the role on ([`orchestration::at_limit`]).
    pub passed: Option<String>,
}

/// Whose a task is once a give was looked for ([`Store::give_task`]).
enum Kept {
    /// The orchestrator the tree makes it: nothing written.
    Held(String),
    /// The orchestrator it was given.
    Given(String),
    /// None that stays: it waits, marked.
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
    /// Each receiver's slot counter as read before the board the pass decided on ([`Pass::guard`]).
    counters: Vec<Value>,
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
        self.given.lines.push(line.clone());
        self.gives
            .push(json!({"slug": top.slug, "to": to, "drop": dropped, "line": line}));
        if marked {
            self.unmarks.push(top.slug.clone());
        }
        self.receivers.insert(to.clone());
        self.given.gave.push((top.slug.clone(), to));
    }

    /// Hold each receiver's slot counter at `last`, as read before the board this pass decided on
    /// (`Store::slot_counters`): a take of the role or a hand-over to it written since moved it, so
    /// the pass, which gave it work by a count that write raised, reads and decides again, and the
    /// limit on its open tasks holds.
    pub(in crate::store) fn guard(&mut self, last: &[(String, i64)]) {
        let read: HashMap<&str, i64> = last.iter().map(|(h, l)| (h.as_str(), *l)).collect();
        self.counters = (self.receivers.iter())
            .map(|to| {
                let last = read.get(to.as_str()).copied().unwrap_or(0);
                json!({"key": ["slots", to], "last": last})
            })
            .collect();
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
            "counters": self.counters,
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

impl Store {
    /// The whole board as one read gives it, with each task's version.
    pub(in crate::store) fn board_versions(&self) -> Result<(Board, Versions), Failure> {
        self.versions_of(&Which::All)
    }

    /// The board as one read of `which` gives it, with each task's version.
    fn versions_of(&self, which: &Which) -> Result<(Board, Versions), Failure> {
        let view = self.view(which)?;
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
        let acts = session.is_some();
        let body = Statement::from(pg::give_pass(acts)).posting();
        let now = (self.clock)();
        let sender = by.unwrap_or(BOARD);
        let vars = match session {
            Some(actor) => self.acting(actor, now, json!({})),
            None => json!({"now": now}),
        };
        self.transact::<Value>(body, pass.vars(sender, vars))
            .map(|_| ())
    }

    /// Refused, saying `what` cannot happen and why, when `to` holds the orchestrator role on
    /// `limit` open tasks or more ([`orchestration::counted`]): a rule giving it the role on another
    /// (`role`, `handover`, a filer's own take). Else `to`'s slot counter, read with its tasks,
    /// joins `slots`, the counters the rule's write holds (`cap::GUARD`), so a give or another take
    /// written since, which moved it, has the rule read and decide again. A counter the rule's cap
    /// read already holds at another value is the board moving under it: decided again too.
    pub(in crate::store) fn gaining(
        &self,
        to: &str,
        what: &str,
        limit: usize,
        slots: &mut Vec<Value>,
    ) -> Result<(), Failure> {
        #[derive(serde::Deserialize)]
        struct Mine {
            last: i64,
            tasks: Vec<Held>,
        }
        #[derive(serde::Deserialize)]
        struct Held {
            status: String,
            #[serde(default)]
            red: Option<Value>,
        }
        let read = Statement::from(pg::GAINING);
        let mine: Mine = self.read(read, json!({"handle": to}))?;
        let held = (mine.tasks.iter())
            .filter(|t| !FINISHED.contains(&t.status.as_str()))
            .filter(|t| t.red.as_ref().is_none_or(Value::is_null))
            .count();
        if let Some(why) = orchestration::at_limit(to, held, limit) {
            return Err(Failure::Stop(crate::store::refused(format!(
                "{what}: {why}"
            ))));
        }
        let key = json!(["slots", to]);
        match slots.iter().find(|s| s["key"] == key) {
            Some(read) if read["last"] != json!(mine.last) => return Err(Failure::Again),
            Some(_) => {}
            None => slots.push(json!({"key": key, "last": mine.last})),
        }
        Ok(())
    }

    /// Every orchestrator's slot counter, `counter` `['slots', HANDLE]`, which every give and every
    /// take of the role or hand-over to it moves: what a pass reads before the board it decides on,
    /// to hold its receivers' where it read them ([`Pass::guard`]).
    pub(in crate::store) fn slot_counters(&self) -> Result<Vec<(String, i64)>, Failure> {
        self.read(Statement::from(pg_cap::counted()), json!({}))
    }

    /// The orchestrators of `board` that stay, but `except` ([`Pool::of`]), none given a task past
    /// the limit on its open tasks but a red ([`Pool::within`], `[limits] orchestrator_tasks`): its
    /// live ones past their first live line are read for whether each listens, a channel registered
    /// (`callback_state` with an address) or a reader of `tasks wait` (on the chat seat's waiting
    /// list), by their records alone; with none, nothing is read.
    pub(in crate::store) fn pool(&self, board: &Board, except: &[&str]) -> Result<Pool, Failure> {
        let now = self.now();
        let candidates = orchestration::candidates(board, except, now);
        let listening: BTreeSet<String> = match candidates.is_empty() {
            true => BTreeSet::new(),
            false => {
                let read = Statement::from(pg::LISTENING);
                self.read(read, json!({"candidates": candidates}))?
            }
        };
        Ok(Pool::of(board, except, &listening, now).within(self.limits().orchestrator_tasks))
    }

    /// What a reader of the whole board does with what it read, `board`: when it shows the board's
    /// give work to do ([`orchestration::give_due`]), a pass of [`Store::give_unowned`] as the
    /// board. The server's clock, a detached dispatcher's pass ([`Store::giving`]), each look of
    /// `tasks wait` and of a monitor, and `tasks pull` call it on the board they read anyway, so it
    /// adds no read while every open task has an orchestrator, and one record's read while what
    /// waits has nobody that stays to go to. Whether it made a pass, after which the
    /// caller reads the board again. Never fails the reader: what went wrong goes to stderr, and the
    /// next read tries again. A pass's own write, delivered, makes no pass of its own.
    pub fn give_if_due(&self, board: &Board) -> bool {
        self.try_give_if_due(board).unwrap_or_else(|error| {
            eprintln!("giving open tasks an orchestrator deferred: {error}");
            false
        })
    }

    /// [`Store::give_if_due`], returning a failed pass to a caller that schedules its own retries.
    /// A successful `false` means no give was needed; an error must never clear a retry streak.
    pub fn try_give_if_due(&self, board: &Board) -> Result<bool, Error> {
        thread_local! {
            static GIVING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        }
        let Some(tops) = orchestration::may_give(&board.tasks) else {
            return Ok(false);
        };
        if GIVING.with(|giving| giving.replace(true)) {
            return Ok(false);
        }
        let due = self
            .retrying(|| self.pool(board, &[]))
            .map(|pool| orchestration::give_due(&board.tasks, &tops, &pool));
        let made = match due {
            Ok(true) => self.give_unowned(None).map(|_| true),
            Ok(false) => Ok(false),
            Err(error) => Err(error),
        };
        GIVING.with(|giving| giving.set(false));
        made
    }

    /// Give every open task nobody orchestrates to an orchestrator that stays, the topmost of each
    /// tree ([`orchestration::unowned_tops`]) by rule 4 ([`Pool`]), one line on each thread; with
    /// none, mark each waiting, and lift the mark from those no longer waiting. `by`: the session
    /// whose write it is, or none for the board's own (the idle sweep, [`Store::give_if_due`]).
    /// What it gave, and what waits.
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
            let counted = self.slot_counters()?;
            let (board, versions) = self.board_versions()?;
            let mut pool = self.pool(&board, &[])?;
            let mut pass = Pass::default();
            for top in orchestration::unowned_tops(&board.tasks) {
                let why = if orchestration::waiting(&top.fields).is_some() {
                    "it waited for an orchestrator"
                } else {
                    "nobody orchestrated it"
                };
                pass.give(&board, &versions, top, pool.pick_for(top), why, &[]);
            }
            pass.unmark_settled(&board, &versions);
            pass.guard(&counted);
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
    /// orchestrator of the task `by` works on, and `by`'s parent session; else by rule 4 among the
    /// orchestrators that stay ([`Pool`]). Some orchestrator, or none when none stays and the task
    /// waits.
    pub fn give_filed(
        &self,
        slug: &str,
        by: &str,
        to: Option<&str>,
    ) -> Result<Option<String>, Error> {
        Ok(self.give_filed_saying(slug, by, to)?.orchestrator)
    }

    /// [`Store::give_filed`], with why the orchestrator the task's story names first was passed
    /// over: `by`, an orchestrator, refused the role (at the limit on the open tasks it holds the
    /// role on, `[limits] orchestrator_tasks`, or past its slots), or the first live one of the
    /// rest at that limit. What `tasks create` says.
    pub fn give_filed_saying(
        &self,
        slug: &str,
        by: &str,
        to: Option<&str>,
    ) -> Result<Filed, Error> {
        let mut refused = None;
        if to.is_none() && by != OWNER {
            match self.orchestrate_filed(slug, by) {
                Ok(true) => {
                    return Ok(Filed {
                        orchestrator: Some(by.to_owned()),
                        passed: None,
                    });
                }
                Ok(false) => {}
                Err(Error::Refused(why)) => refused = Some(why),
                Err(error) => return Err(error),
            }
        }
        let why = format!("filed by {by}");
        let (kept, passed) = self.give_task(slug, by, &why, &|board: &Board| {
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
        let orchestrator = match kept {
            Kept::Held(owner) | Kept::Given(owner) => Some(owner),
            Kept::Waiting | Kept::Finished => None,
        };
        // The filer's own refusal says nothing once the board gave it the task all the same (a
        // place of its freed between its take and the give); the orchestrator `--for` or the
        // story named, passed over at the limit, is said whoever took the task.
        let refused = refused.filter(|_| orchestrator.as_deref() != Some(by));
        Ok(Filed {
            passed: refused.or(passed),
            orchestrator,
        })
    }

    /// Keep `slug` somebody's after `by` reopened it: when nobody orchestrates it now, give it by
    /// rule 4, `why` on the line. Whether that wrote anything: a give, or the task marked waiting.
    pub(crate) fn keep_owned(&self, slug: &str, by: &str, why: &str) -> Result<bool, Error> {
        let (kept, _) = self.give_task(slug, by, why, &|_: &Board| Vec::new())?;
        Ok(matches!(kept, Kept::Given(_) | Kept::Waiting))
    }

    /// The give a move of `slug` under `parent` (none: to the top) makes in its own write, decided
    /// on `tree`, the tree the move read: when the move leaves open work at or below `slug` with no
    /// orchestrator, the role on `slug` to the orchestrator it had, the nearest holding one above it
    /// ([`Owners::of`]), so an orchestrator restructuring its own tree keeps its work. None when
    /// the move keeps every task somebody's, or it had nobody. Only a move out from under a task
    /// above it reads more than the tree, and then only the tasks of both lines of parents and of
    /// the moved tree.
    pub(in crate::store) fn kept_at_move(
        &self,
        tree: &super::Tree,
        slug: &str,
        parent: Option<&str>,
    ) -> Result<Option<Pass>, Failure> {
        let was: Vec<&str> = super::above(tree, slug).collect();
        let mut under: Vec<&str> = parent.into_iter().collect();
        under.extend(parent.into_iter().flat_map(|p| super::above(tree, p)));
        // Every task above it before stays above it: whoever held it still does.
        if was.iter().all(|a| under.contains(a)) {
            return Ok(None);
        }
        let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
        for (child, of) in tree {
            if let Some(of) = of {
                children
                    .entry(of.as_str())
                    .or_default()
                    .push(child.as_str());
            }
        }
        // The moved tree, `slug` first, each task once however the tree loops.
        let mut moved = vec![slug];
        let mut at = 0;
        while let Some(&here) = moved.get(at) {
            for child in children.get(here).into_iter().flatten() {
                if !moved.contains(child) {
                    moved.push(child);
                }
            }
            at += 1;
        }
        let wanted: BTreeSet<String> = (was.iter().chain(&under).chain(&moved))
            .map(|s| (*s).to_owned())
            .collect();
        let wanted: Vec<String> = wanted.into_iter().collect();
        let (board, versions) = self.versions_of(&Which::Slugs(&wanted))?;
        let Some(to) = Owners::new(&board.tasks)
            .of(slug)
            .first()
            .map(|h| (*h).to_owned())
        else {
            return Ok(None);
        };
        let mut after = board.tasks.clone();
        if let Some(task) = after.iter_mut().find(|t| t.slug == slug) {
            task.parent = parent.map(str::to_owned);
        }
        let owners = Owners::new(&after);
        let open = (after.iter())
            .filter(|t| moved.contains(&t.slug.as_str()))
            .filter(|t| !FINISHED.contains(&t.status.as_str()));
        let left = open.into_iter().any(|t| owners.of(&t.slug).is_empty());
        let Some(task) = board.tasks.iter().find(|t| t.slug == slug).filter(|_| left) else {
            return Ok(None);
        };
        let from = task.parent.as_deref().unwrap_or("nothing");
        let mut pass = Pass::default();
        let why = format!("{slug} moved out from under {from}");
        pass.give(&board, &versions, task, Some(to), &why, &[]);
        Ok(Some(pass))
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
    /// `first` names on the board read that holds fewer open tasks than the limit (any, for a red),
    /// else by rule 4 among those that stay, as `by` with `why` on its line. With it, why the first
    /// live one `first` names was passed over, at the limit ([`orchestration::at_limit`]).
    fn give_task(
        &self,
        slug: &str,
        by: &str,
        why: &str,
        first: &dyn Fn(&Board) -> Vec<String>,
    ) -> Result<(Kept, Option<String>), Error> {
        // Most tasks are somebody's by the tree: read the line of parents before the whole board.
        match self.orchestrators_of(slug) {
            Ok(owners) if !owners.is_empty() => return Ok((Kept::Held(owners[0].clone()), None)),
            Ok(_) | Err(Error::Refused(_)) => {}
            Err(error) => return Err(error),
        }
        let actor = self.give_actor(by)?;
        let limit = self.limits().orchestrator_tasks;
        self.retrying(|| {
            let counted = self.slot_counters()?;
            let (board, versions) = self.board_versions()?;
            let owners = Owners::new(&board.tasks);
            let task = board.tasks.iter().find(|t| t.slug == slug);
            let Some(task) = task.filter(|t| !FINISHED.contains(&t.status.as_str())) else {
                return Ok((Kept::Finished, None));
            };
            if let Some(owner) = owners.of(&task.slug).first() {
                return Ok((Kept::Held((*owner).to_owned()), None));
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
                return Ok((Kept::Finished, None));
            };
            // The task's own story names a live orchestrator under the limit on its open tasks (at
            // any count for a red), whoever it is; the board's own choice goes to one that stays
            // ([`Pool::of`]).
            let live = |handle: &str| {
                (board.sessions.iter()).any(|s| {
                    s.row.handle == handle
                        && s.row.kind == "orchestrator"
                        && s.health == crate::Health::Live
                })
            };
            let (mut chosen, mut passed) = (None, None);
            for handle in first(&board).into_iter().filter(|h| live(h)) {
                let held = orchestration::orchestrated(&board.tasks, &handle);
                if top.red() || held < limit {
                    chosen = Some(handle);
                    break;
                }
                passed = passed.or_else(|| orchestration::at_limit(&handle, held, limit));
            }
            let to = match chosen {
                Some(to) => Some(to),
                None => self.pool(&board, &[])?.pick_for(top),
            };
            let mut pass = Pass::default();
            pass.give(&board, &versions, top, to.clone(), why, &[]);
            pass.guard(&counted);
            self.write_pass(&pass, actor.as_deref())?;
            Ok((to.map_or(Kept::Waiting, Kept::Given), passed))
        })
    }

    /// Who a give `by` writes as: `by` while it is a session still there, else the board (the
    /// owner's task, whose give is the board's and no word of the owner's; a filer that left; a
    /// name no session has).
    fn give_actor(&self, by: &str) -> Result<Option<String>, Error> {
        if by == OWNER {
            return Ok(None);
        }
        match self.session(by) {
            Ok(session) if session.row.left_at.is_none() => Ok(Some(by.to_owned())),
            Ok(_) | Err(Error::Refused(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }
}
