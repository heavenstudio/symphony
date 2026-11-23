//! The cap on an orchestrator's open tasks as a rule's write holds to it ([`crate::slots`]).
//!
//! A rule that may start work ([`super::Opens`]) reads, in the same transaction as its task, what
//! the cap decides on (`Which::Starting`): every slot counter (`counter:['slots', HANDLE]`), the
//! open tasks through `task_status`, the holds ([`sql::HOLDS`]) and their tasks by id, and their
//! orchestrator and executor roles through `role_slug`. Then, by id, the tasks above them and the
//! tasks they review, a level a round trip, with their roles. Each task comes with its red mark
//! ([`crate::red`]), so red work is told apart where the tree is read. Nothing reads a table whole.
//!
//! A hold (`Store::hold_blocked`, a landing's while it waits on stamps) moves open work to blocked and
//! keeps its slot: `counter:['hold', SLUG]`, its `ts` the task's `updated` as the hold left it. It
//! lasts while the task is blocked and that stamp is the task's: every move, change of parent or
//! of dependencies stamps the task anew, and the board's own writes delete the record then too.
//! A record and not a field of the task: a reader built before it would refuse a task row holding
//! a field it does not know.
//!
//! A red's close ([`closing`]) is read so too: never refused, it raises the counts of the open work
//! under the red, and holds the counters a start would, whether the closing store caps starts or
//! not (the release run's does not), since a store that does may have decided on the board before it. A
//! red's own counter (`counter:['red', SLUG]`) is held by its close and by every start that takes
//! no slot under it ([`crate::slots::reds`]).
//!
//! The write holds the counter of every orchestrator whose open tasks it changes, a start into a
//! slot already open included, and moves it on: two starts at once under one orchestrator meet
//! there, and the one that loses reads and decides again. So no write passes the cap on what it read
//! before another start: one that joins a slot whose last task closed meanwhile, while a third start
//! took the slot that freed, finds the counter moved and decides again. The counters are read in the
//! transaction that reads the open tasks and the holds, so a start that wrote after that read is
//! caught by the write, and one that wrote before it is in what was read.

use super::branches::{self, Recorded};
use super::surreal::cap as sql;
use super::{Change, Error, Failure, Opens, Store, View, Which, refused};
use crate::maps::Collect as _;
use crate::slots::{self, OPEN};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

mod ledger;
use ledger::Observation;

pub use super::surreal::cap::STATEMENTS;

/// Rounds of reads by id above the open tasks: deeper than any tree the board holds.
const DEPTH: usize = 32;

/// The variables [`sql::lets`] and [`sql::field`] read beside the wanted tasks.
pub(super) fn vars() -> Value {
    json!({"slot_statuses": OPEN})
}

/// The change a write moving `slug` into `status` makes under the cap, read in `view`, when it
/// closes an open red ([`crate::red`]): never refused, it holds the counters it changes
/// ([`slots::changed`], [`slots::reds`]), so a start decided on what it read before the close is
/// decided again. None for any other write.
pub(super) fn closing(view: &View, slug: &str, status: Option<&str>) -> Option<Opens> {
    let task = view.task(slug).ok()?;
    let status = status.filter(|to| !crate::red::open(to))?;
    let red = crate::red::marked(&task.fields) && crate::red::open(&task.status);
    red.then(|| Opens {
        status: Some(status.into()),
        ..Opens::default()
    })
}

/// The counters of `kind` a write holds ([`sql::GUARD`]), one for each of `names`, at the `last` of
/// each as `read` gave it.
fn held(kind: &str, read: &[(String, i64)], names: &[&str]) -> Vec<Value> {
    let last: HashMap<&str, i64> = read.iter().map(|(n, l)| (n.as_str(), *l)).collect();
    let last = |name: &str| last.get(name).copied().unwrap_or(0);
    (names.iter())
        .map(|name| json!({"key": [kind, name], "last": last(name)}))
        .collect()
}

/// A rule's read and decision ([`Store::ruled`]).
pub(super) struct Ruled {
    /// The board as the decision read it: the cap's read too for a red's close.
    pub(super) view: View,
    /// What the rule decided on `view`.
    pub(super) change: Change,
    /// The counters its write holds ([`sql::GUARD`]): a start's under the cap, refused past it
    /// ([`Store::slots_taken`]), and a red's close's ([`closing`]); none for any other change.
    pub(super) slots: Vec<Value>,
}

/// An owner's close as the cap judged it ([`Store::closing_slots`]).
#[derive(Default)]
pub(super) struct Closed {
    /// The task's version as read, which the write must still find ([`sql::FENCE`]); none for a
    /// move that reads nothing, into an open status.
    pub(super) version: Option<i64>,
    /// The counters its write holds ([`sql::GUARD`]).
    pub(super) slots: Vec<Value>,
}

impl Closed {
    /// Whether this close's write is fenced on the version read ([`sql::FENCE`]).
    pub(super) fn fenced(&self) -> bool {
        self.version.is_some()
    }

    /// Whether this close's write holds counters ([`sql::GUARD`]).
    pub(super) fn guarded(&self) -> bool {
        !self.slots.is_empty()
    }
}

/// The version of `slug` as `view` read it, for a write to fence on ([`sql::FENCE`]). A task not on
/// the board is refused at the read: no write goes on unfenced to find one filed since, a red with
/// work started under it that its counters, judged on nothing, would let count past the cap.
fn version_read(view: &View, slug: &str) -> Result<i64, Failure> {
    let read = view.snap.tasks.iter().find(|task| task.row.slug == slug);
    read.map(|task| task.version)
        .ok_or_else(|| Failure::Stop(super::work::no_task(slug)))
}

/// A task as the cap reads it.
#[derive(Clone, Deserialize)]
struct Row {
    slug: String,
    parent: Option<String>,
    status: String,
    #[serde(default)]
    instruction: Option<bool>,
    /// Its red mark, what is red, when it was filed for one ([`crate::red`]).
    #[serde(default)]
    red: Option<Value>,
    /// Read for a held task alone: the stamp its hold must match.
    #[serde(default)]
    updated: Option<f64>,
    /// Where a custom workflow holds it, its position (`fields._board.workflow`), by whose slot
    /// policy it holds a slot ([`crate::slots::Entry::slot`]).
    /// Null for a task no workflow holds: a plain value, as the positions are below.
    #[serde(default)]
    position: Value,
    /// Blocked under a hold that keeps its slot.
    #[serde(skip)]
    held: bool,
}

/// A role as the cap reads it.
#[derive(Clone, Deserialize)]
struct RoleRead {
    slug: String,
    handle: String,
    role: String,
}

/// [`field`] as it comes back.
#[derive(Clone, Deserialize)]
pub(super) struct Read {
    open: Vec<Row>,
    /// Each hold's task and stamp.
    holds: Vec<(String, Option<f64>)>,
    /// The tasks the holds name.
    held: Vec<Row>,
    roles: Vec<RoleRead>,
    counted: Vec<(String, i64)>,
    /// Each red's counter, by slug.
    reds: Vec<(String, i64)>,
    /// The active custom workflow's positions, each its pair and slot policy (`phase`, `state`,
    /// `slot`); none while no workflow is active. One plain value read by field, as `Row`'s
    /// position is: no reader of their own, nor of a list or an option of values, is inlined
    /// into the cap's (tools/size-budget.toml).
    #[serde(default)]
    positions: Value,
}

impl Read {
    fn of(view: &View) -> Result<&Self, Failure> {
        view.snap.slots.as_ref().ok_or_else(|| {
            Failure::Stop(Error::Db("slot accounting read no admission ledger".into()))
        })
    }

    /// The tasks a hold keeps open: blocked, at the stamp the hold was made at.
    fn kept(&self) -> impl Iterator<Item = Row> + '_ {
        let stamps: HashMap<&str, Option<f64>> = (self.holds.iter())
            .map(|(slug, ts)| (slug.as_str(), *ts))
            .collect();
        self.held.iter().filter_map(move |row| {
            let stamp = stamps.get(row.slug.as_str()).copied().flatten();
            (row.status == "blocked" && stamp.is_some() && stamp == row.updated).then(|| Row {
                held: true,
                ..row.clone()
            })
        })
    }
}

/// A level of tasks above, read by id, and their roles.
#[derive(Deserialize)]
struct Level {
    tasks: Vec<Row>,
    roles: Vec<RoleRead>,
    branches: Vec<Recorded>,
}

impl Store {
    /// Counted parent-feature keys by owner, from one read-only whole-board observation.
    /// Uses the admission ledger's hold-stamp and presence rules, including Unknown owners;
    /// red work and work in review are excluded. No cap need be configured. Owners without a counted slot are absent.
    /// This reads one snapshot (tasks, roles, branches, holds and session health), with no
    /// per-parent follow-up reads. A failed observation returns an error, never empty holdings.
    pub fn slot_holdings(&self) -> Result<BTreeMap<String, BTreeSet<String>>, Error> {
        self.retrying(|| {
            let view = self.view(&Which::SlotHoldings)?;
            let observation = Observation::new(&view, Read::of(&view)?);
            Ok(observation
                .ledger(&view)
                .holdings()
                .into_iter()
                .map(|(owner, slots)| {
                    (
                        owner.to_string(),
                        slots.into_keys().map(String::from).collect(),
                    )
                })
                .collect_map())
        })
    }

    /// A rule's read of `which` ([`Store::ruling`]) and what `decide` makes of it, with the counters
    /// its write holds: a start's under `cap`, the store's as the rule read it once, and a red's
    /// close's whether this store caps starts or not, a close of an open red read without the
    /// cap's board read and decided again with it ([`closing`]). No counters for any other change.
    pub(super) fn ruled(
        &self,
        which: &Which,
        slug: &str,
        cap: Option<usize>,
        decide: &impl Fn(&View) -> Result<Change, Error>,
    ) -> Result<Ruled, Failure> {
        let mut view = self.view(which)?;
        let mut change = decide(&view).map_err(Failure::Stop)?;
        let closes = |view: &View, change: &Change| {
            let start = change.opens.is_some();
            closing(view, slug, change.status.as_deref()).filter(|_| !start)
        };
        if !matches!(which, Which::Starting(_)) && closes(&view, &change).is_some() {
            view = self.view(&Which::Starting(slug))?;
            change = decide(&view).map_err(Failure::Stop)?;
        }
        let slots = match (&change.opens, cap, closes(&view, &change)) {
            (Some(opens), Some(cap), _) => self.slots_taken(&view, slug, opens, cap)?,
            (_, _, Some(closing)) => self.holding(&view, slug, &closing, None)?,
            _ => Vec::new(),
        };
        Ok(Ruled {
            view,
            change,
            slots,
        })
    }

    /// The slot counters the change `opens` describes holds under `cap`, each `{handle, last}` as
    /// read: one for each orchestrator whose open tasks it changes. Refused, naming the holder's
    /// open tasks, when it would raise one's past `cap`. Decided on what `view` read
    /// (`Which::Starting`) and the tasks above the open ones, and above a new parent, read by id.
    pub(super) fn slots_taken(
        &self,
        view: &View,
        slug: &str,
        opens: &Opens,
        cap: usize,
    ) -> Result<Vec<Value>, Failure> {
        self.holding(view, slug, opens, Some(cap))
    }

    /// The counters the change `opens` describes holds ([`sql::GUARD`]): refused past `cap` as
    /// [`Store::slots_taken`] is, and never with none, a red's close ([`closing`]).
    fn holding(
        &self,
        view: &View,
        slug: &str,
        opens: &Opens,
        cap: Option<usize>,
    ) -> Result<Vec<Value>, Failure> {
        let read = Read::of(view)?;
        let parent = opens.parent.as_ref().map(Option::as_deref);
        let mut observation = Observation::new(view, read);
        observation.read_above(self, parent.flatten())?;
        let before = observation.ledger(view);
        let by: Vec<&str> = opens.by.iter().map(String::as_str).collect();
        let mut after = before.changed(
            slug,
            opens.status.as_deref(),
            opens.orchestrator.as_deref(),
            &by,
        );
        if let Some(parent) = parent {
            after = after.reparented(slug, parent);
        }
        if opens.unmarks {
            after = after.unmarked(slug);
        }
        if let Some(holds) = opens.slot {
            after = after.slotted(slug, holds);
        }
        let refusal = |full: slots::Full<'_>, cap: usize| {
            let place = after.place(slug);
            let children: Vec<&str> = if place.by_parent && place.owners.contains(&full.owner) {
                (by.iter().copied())
                    .filter(|s| {
                        view.session(s).and_then(|r| r.parent.as_deref()) == Some(full.owner)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let said = slots::refusal(&opens.what, &full, &children, cap);
            let hint = slots::claim_hint(slug, &opens.what);
            Failure::Stop(refused(format!("{said}{hint}")))
        };
        let changed = match cap {
            Some(cap) => slots::started(&before, &after, cap).map_err(|full| refusal(full, cap))?,
            None => slots::changed(&before, &after),
        };
        let mut counters = held("slots", &read.counted, &changed);
        counters.extend(held("red", &read.reds, &slots::reds(&before, &after)));
        Ok(counters)
    }

    /// What a move of `slug` into `status` holds, for a write that reads nothing the cap decides
    /// on, the owner's: no counters unless it closes an open red ([`closing`]), whether this store
    /// caps starts or not, since a store that does may have read the board before it; and, for a
    /// close, the task's version they were judged at, which the write fences on ([`sql::FENCE`]), a
    /// task not on the board refused at that read ([`version_read`]). A move into an open status
    /// reads nothing.
    pub(super) fn closing_slots(&self, slug: &str, status: &str) -> Result<Closed, Failure> {
        if crate::red::open(status) {
            return Ok(Closed::default());
        }
        let view = self.view(&Which::Starting(slug))?;
        let version = Some(version_read(&view, slug)?);
        let slots = match closing(&view, slug, Some(status)) {
            Some(opens) => self.holding(&view, slug, &opens, None)?,
            None => Vec::new(),
        };
        Ok(Closed { version, slots })
    }

    /// What lifting `slug`'s red mark holds ([`Store::unmark_red`]), as a red's close does: the
    /// counters of the counts it raises, at any count, capped store or not; none unless `slug` is
    /// an open red. And the task's version they were judged at, which the write fences on.
    pub(super) fn unmarking_slots(&self, slug: &str) -> Result<Closed, Failure> {
        let view = self.view(&Which::Starting(slug))?;
        let version = Some(version_read(&view, slug)?);
        let task = view.task(slug).map_err(Failure::Stop)?;
        let slots = if crate::red::marked(&task.fields) && crate::red::open(&task.status) {
            let opens = Opens {
                unmarks: true,
                ..Opens::default()
            };
            self.holding(&view, slug, &opens, None)?
        } else {
            Vec::new()
        };
        Ok(Closed { version, slots })
    }

    /// The slot counters `by`'s change of `slug`'s parent to `parent` holds under the cap: the open
    /// work at and below it changes orchestrator with its tree, decided as a start; with `kept`,
    /// the orchestrator the move gives `slug` to, the one it had, it stays that one's work. None for
    /// the owner's change, or with no cap.
    pub(super) fn reparenting(
        &self,
        slug: &str,
        parent: Option<&str>,
        by: &str,
        kept: Option<&str>,
    ) -> Result<Vec<Value>, Failure> {
        let Some(cap) = self.slots().filter(|_| !self.is_person(by)) else {
            return Ok(Vec::new());
        };
        let opens = Opens {
            parent: Some(parent.map(String::from)),
            orchestrator: kept.map(String::from),
            what: format!("{slug} cannot go under {}", parent.unwrap_or("nothing")),
            ..Opens::default()
        };
        self.slots_taken(&self.view(&Which::Starting(slug))?, slug, &opens, cap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(slug: &str, status: &str, updated: f64) -> Row {
        Row {
            slug: slug.into(),
            parent: None,
            status: status.into(),
            instruction: Some(false),
            red: None,
            updated: Some(updated),
            position: Value::Null,
            held: false,
        }
    }

    #[test]
    fn a_hold_keeps_blocked_work_open_only_at_the_stamp_it_was_made_at() {
        let read = Read {
            open: Vec::new(),
            holds: vec![
                ("kept".into(), Some(5.0)),
                ("moved".into(), Some(5.0)),
                ("unblocked".into(), Some(5.0)),
                ("unstamped".into(), None),
            ],
            held: vec![
                row("kept", "blocked", 5.0),
                row("moved", "blocked", 6.0),
                row("unblocked", "review", 5.0),
                row("unstamped", "blocked", 5.0),
                row("no-hold", "blocked", 5.0),
            ],
            roles: Vec::new(),
            counted: Vec::new(),
            reds: Vec::new(),
            positions: Value::Null,
        };
        let kept: Vec<(String, bool)> = read.kept().map(|r| (r.slug, r.held)).collect();
        assert_eq!(kept, [("kept".to_string(), true)]);
    }
}
