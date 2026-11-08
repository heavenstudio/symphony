//! The cap on an orchestrator's open tasks as a rule's write holds to it ([`crate::slots`]).
//!
//! A rule that may start work ([`super::Opens`]) reads, in the same transaction as its task, what
//! the cap decides on (`Which::Starting`): every slot counter (`counter:['slots', HANDLE]`), the
//! open tasks through `task_status`, the holds ([`HOLDS`]) and their tasks by id, and their
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
//! under the red, and holds the counters a start would. A red's own counter (`counter:['red',
//! SLUG]`) is held by its close and by every start that takes no slot under it
//! ([`crate::slots::reds`]).
//!
//! The write holds the counter of every orchestrator whose open tasks it changes, a start into a
//! slot already open included, and moves it on: two starts at once under one orchestrator meet
//! there, and the one that loses reads and decides again. So no write passes the cap on what it read
//! before another start: one that joins a slot whose last task closed meanwhile, while a third start
//! took the slot that freed, finds the counter moved and decides again. The counters are read in the
//! transaction that reads the open tasks and the holds, so a start that wrote after that read is
//! caught by the write, and one that wrote before it is in what was read.

use super::{Change, Error, Failure, Opens, Store, View, Which, refused};
use crate::slots::{self, Entry, Ledger, OPEN};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};

/// Rounds of reads by id above the open tasks: deeper than any tree the board holds.
const DEPTH: usize = 32;

/// The open tasks, through `task_status`, each with its red mark ([`crate::red`]).
pub const OPEN_TASKS: &str = "SELECT slug, parent, status, instruction, fields._board.red AS red FROM task WHERE status IN $slot_statuses";
/// The tasks `$slot_wanted` names, by id.
pub const BY_ID: &str = "SELECT slug, parent, status, instruction, fields._board.red AS red FROM $slot_wanted.map(|$s| type::record('task', $s))";
/// Their roles, through `role_slug`: a task holds a handful, and naming the two the cap reads would
/// let the server take an index of every row holding one of them instead.
pub const ROLES_OF: &str = "SELECT slug, handle, role FROM role WHERE slug IN $slot_wanted";
/// Every slot counter, a range of record ids.
pub const COUNTERS: &str =
    "SELECT VALUE [record::id(id)[1], last] FROM counter:['slots', NONE]..['slots', ..]";
/// Every red's counter ([`crate::slots::reds`]), a range of record ids.
pub const RED_COUNTERS: &str =
    "SELECT VALUE [record::id(id)[1], last] FROM counter:['red', NONE]..['red', ..]";
/// Every hold, its task and the stamp it was made at, a range of record ids.
pub const HOLDS: &str =
    "SELECT VALUE [record::id(id)[1], ts] FROM counter:['hold', NONE]..['hold', ..]";
/// The tasks `$slot_holds` holds, by id, with the stamp each hold must match.
pub const HELD_TASKS: &str = "SELECT slug, parent, status, instruction, fields._board.red AS red, updated FROM $slot_holds.map(|$h| type::record('task', $h[0]))";

/// Every statement the cap sends, which `tests/store/slots/cost.rs` has the server plan.
pub const STATEMENTS: [&str; 7] = [
    OPEN_TASKS,
    BY_ID,
    ROLES_OF,
    COUNTERS,
    HOLDS,
    HELD_TASKS,
    RED_COUNTERS,
];

/// What `Which::Starting` sets before its `RETURN`: the open tasks, the holds and the tasks they
/// hold, and all of those with the task read.
pub(super) fn lets() -> String {
    format!(
        "LET $slot_open = ({OPEN_TASKS}); LET $slot_holds = ({HOLDS}); LET $slot_held = ({HELD_TASKS});
         LET $slot_wanted = array::union(array::union($slot_open.slug, $slot_held.slug), [$slug]);"
    )
}

/// The field of `Which::Starting`'s read the cap decides on.
pub(super) fn field() -> String {
    format!(
        "slots: {{ open: $slot_open, holds: $slot_holds, held: $slot_held, roles: ({ROLES_OF}), counted: ({COUNTERS}), reds: ({RED_COUNTERS}) }},"
    )
}

/// The variables [`lets`] and [`field`] read beside `$slug`.
pub(super) fn vars() -> Value {
    json!({"slot_statuses": OPEN})
}

/// The write's hold on each counter it read (`$slots`: its key, `['slots', HANDLE]` or `['red',
/// SLUG]`, and `last`): stale if another write moved it since, then moved on.
pub(super) const GUARD: &str = "
FOR $s IN $slots {
    IF ((SELECT VALUE last FROM ONLY type::record('counter', $s.key)) ?? 0) != $s.last { THROW 'stale' };
    UPSERT type::record('counter', $s.key) SET last = $s.last + 1 RETURN NONE;
};";

/// The end of `$slug`'s hold, in a write that moves it or changes its parent or dependencies.
pub(super) const UNHOLD: &str = "DELETE type::record('counter', ['hold', $slug]) RETURN NONE;";

/// A rule's write's part in a hold: a move (`$status`) ends one, and the hold's own move (`$hold`)
/// makes one, stamped with the task's `updated` as the write left it (`$updated`).
pub(super) const HOLDING: &str = "
IF $status { DELETE type::record('counter', ['hold', $slug]) RETURN NONE; };
IF $hold { UPSERT type::record('counter', ['hold', $slug]) SET last = 0, ts = $updated RETURN NONE; };";

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

/// The counters of `kind` a write holds ([`GUARD`]), one for each of `names`, at the `last` of
/// each as `read` gave it.
fn held(kind: &str, read: &[(String, i64)], names: &[&str]) -> Vec<Value> {
    let last: HashMap<&str, i64> = read.iter().map(|(n, l)| (n.as_str(), *l)).collect();
    let last = |name: &str| last.get(name).copied().unwrap_or(0);
    (names.iter())
        .map(|name| json!({"key": [kind, name], "last": last(name)}))
        .collect()
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
}

impl Read {
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
}

impl Store {
    /// A rule's read of `which` ([`Store::ruling`]) and what `decide` makes of it, with the counters
    /// a red's close holds under the cap: a close of an open red read without the cap's board is
    /// read and decided again with it ([`closing`]). No counters for any other change.
    pub(super) fn ruled(
        &self,
        which: &Which,
        slug: &str,
        decide: &impl Fn(&View) -> Result<Change, Error>,
    ) -> Result<(View, Change, Vec<Value>), Failure> {
        let mut view = self.view(which)?;
        let mut change = decide(&view).map_err(Failure::Stop)?;
        let closes = |view: &View, change: &Change| {
            (change.opens.is_none() && self.slots.is_some())
                .then(|| closing(view, slug, change.status.as_deref()))
                .flatten()
        };
        if !matches!(which, Which::Starting(_)) && closes(&view, &change).is_some() {
            view = self.view(&Which::Starting(slug))?;
            change = decide(&view).map_err(Failure::Stop)?;
        }
        let closed = match closes(&view, &change) {
            Some(opens) => self.holding(&view, slug, &opens, None)?,
            None => Vec::new(),
        };
        Ok((view, change, closed))
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

    /// The counters the change `opens` describes holds ([`GUARD`]): refused past `cap` as
    /// [`Store::slots_taken`] is, and never with none, a red's close ([`closing`]).
    fn holding(
        &self,
        view: &View,
        slug: &str,
        opens: &Opens,
        cap: Option<usize>,
    ) -> Result<Vec<Value>, Failure> {
        let Some(read) = &view.snap.slots else {
            return Err(Failure::Stop(super::Error::Db(
                "a start read nothing for the cap".into(),
            )));
        };
        let mut rows: HashMap<String, Row> = HashMap::new();
        for row in read.open.iter().cloned().chain(read.kept()) {
            rows.insert(row.slug.clone(), row);
        }
        for task in &view.snap.tasks {
            let row = &task.row;
            rows.entry(row.slug.clone()).or_insert_with(|| Row {
                slug: row.slug.clone(),
                parent: row.parent.clone(),
                status: row.status.clone(),
                instruction: Some(row.instruction),
                red: (crate::red::marked(&row.fields)).then_some(Value::Bool(true)),
                updated: None,
                held: false,
            });
        }
        let parent = opens.parent.as_ref().map(Option::as_deref);
        let mut roles = read.roles.clone();
        let mut looked: BTreeSet<String> = rows.keys().cloned().collect();
        for _ in 0..DEPTH {
            let above = (rows.values()).map(|r| (r.slug.as_str(), r.parent.as_deref()));
            let wanted: Vec<String> = (slots::wanted(above).into_iter())
                .chain(parent.flatten())
                .filter(|s| !looked.contains(*s))
                .map(String::from)
                .collect();
            if wanted.is_empty() {
                break;
            }
            looked.extend(wanted.iter().cloned());
            let level: Level = self.transact(
                &format!("RETURN {{ tasks: ({BY_ID}), roles: ({ROLES_OF}) }};"),
                json!({"slot_wanted": wanted}),
            )?;
            for row in level.tasks {
                rows.insert(row.slug.clone(), row);
            }
            roles.extend(level.roles);
        }
        let mut orchestrators: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut executors: HashMap<&str, Vec<&str>> = HashMap::new();
        for role in &roles {
            let (slug, handle) = (role.slug.as_str(), role.handle.as_str());
            match role.role.as_str() {
                "orchestrator" if view.health(handle).holds() => {
                    orchestrators.entry(slug).or_default().push(handle);
                }
                "executor" => executors.entry(slug).or_default().push(handle),
                _ => {}
            }
        }
        for held in orchestrators.values_mut().chain(executors.values_mut()) {
            held.sort_unstable();
            held.dedup();
        }
        let tasks = rows.values().map(|row| Entry {
            slug: &row.slug,
            parent: row.parent.as_deref(),
            status: &row.status,
            instruction: row.instruction.unwrap_or(false),
            held: row.held,
            orchestrators: orchestrators.remove(row.slug.as_str()).unwrap_or_default(),
            executors: executors.remove(row.slug.as_str()).unwrap_or_default(),
            red: row.red.as_ref().is_some_and(|what| !what.is_null()),
        });
        let parents = (view.snap.sessions.iter())
            .filter_map(|row| Some((row.handle.as_str(), row.parent.as_deref()?)));
        let before = Ledger::new(tasks, parents);
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
            Failure::Stop(refused(slots::refusal(&opens.what, &full, &children, cap)))
        };
        let changed = match cap {
            Some(cap) => slots::started(&before, &after, cap).map_err(|full| refusal(full, cap))?,
            None => slots::changed(&before, &after),
        };
        let mut counters = held("slots", &read.counted, &changed);
        counters.extend(held("red", &read.reds, &slots::reds(&before, &after)));
        Ok(counters)
    }

    /// The counters a close of `slug` into `status` holds under the cap, for a write that reads
    /// nothing the cap decides on, the owner's: none unless it closes an open red ([`closing`]).
    pub(super) fn closing_slots(&self, slug: &str, status: &str) -> Result<Vec<Value>, Failure> {
        if self.slots.is_none() {
            return Ok(Vec::new());
        }
        let view = self.view(&Which::Starting(slug))?;
        match closing(&view, slug, Some(status)) {
            Some(opens) => self.holding(&view, slug, &opens, None),
            None => Ok(Vec::new()),
        }
    }

    /// The slot counters `by`'s change of `slug`'s parent to `parent` holds under the cap: the open
    /// work at and below it changes orchestrator with its tree, decided as a start. None for the
    /// owner's change, or with no cap.
    pub(super) fn reparenting(
        &self,
        slug: &str,
        parent: Option<&str>,
        by: &str,
    ) -> Result<Vec<Value>, Failure> {
        let Some(cap) = self.slots.filter(|_| by != super::work::OWNER) else {
            return Ok(Vec::new());
        };
        let opens = Opens {
            parent: Some(parent.map(String::from)),
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
        };
        let kept: Vec<(String, bool)> = read.kept().map(|r| (r.slug, r.held)).collect();
        assert_eq!(kept, [("kept".to_string(), true)]);
    }
}
