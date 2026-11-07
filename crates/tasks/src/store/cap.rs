//! The cap on an orchestrator's open tasks as a rule's write holds to it ([`crate::slots`]).
//!
//! A rule that may start work ([`super::Opens`]) reads, in the same transaction as its task, what
//! the cap decides on (`Which::Starting`): every slot counter (`counter:['slots', HANDLE]`), the
//! open tasks through `task_status`, their orchestrator and executor roles through `role_slug`, and
//! the lines that blocked the task. Then, by id, the tasks above them and the tasks they review,
//! a level a round trip, with their roles. Nothing reads a table whole.
//!
//! The write holds the counter of every orchestrator whose open tasks it changes, a start into a
//! slot already open included, and moves it on: two starts at once under one orchestrator meet
//! there, and the one that loses reads and decides again. So no write passes the cap on what it read
//! before another start: one that joins a slot whose last task closed meanwhile, while a third start
//! took the slot that freed, finds the counter moved and decides again. The counters are read before
//! anything they guard, so a start that wrote after that read is caught by the write, and one that
//! wrote before it is in what was read.

use super::{Failure, Opens, Store, View, refused};
use crate::slots::{self, Entry, Ledger, OPEN};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};

/// Rounds of reads by id above the open tasks: deeper than any tree the board holds.
const DEPTH: usize = 32;

/// The open tasks, through `task_status`.
pub const OPEN_TASKS: &str =
    "SELECT slug, parent, status, instruction FROM task WHERE status IN $slot_statuses";
/// The tasks `$slot_wanted` names, by id.
pub const BY_ID: &str =
    "SELECT slug, parent, status, instruction FROM $slot_wanted.map(|$s| type::record('task', $s))";
/// Their roles, through `role_slug`: a task holds a handful, and naming the two the cap reads would
/// let the server take an index of every row holding one of them instead.
pub const ROLES_OF: &str = "SELECT slug, handle, role FROM role WHERE slug IN $slot_wanted";
/// Every slot counter, a range of record ids.
pub const COUNTERS: &str =
    "SELECT VALUE [record::id(id)[1], last] FROM counter:['slots', NONE]..['slots', ..]";
/// The lines of the task's thread that blocked it, through its range of record ids.
pub const BLOCKS: &str = "SELECT VALUE text FROM message:[$slug, NONE]..[$slug, ..] WHERE kind = 'system' AND (string::ends_with(text, ' → blocked') OR string::starts_with(text, $slot_block))";

/// Every statement the cap sends, which `tests/store/slots/cost.rs` has the server plan.
pub const STATEMENTS: [&str; 5] = [OPEN_TASKS, BY_ID, ROLES_OF, COUNTERS, BLOCKS];

/// What `Which::Starting` sets before its `RETURN`: the open tasks, and them with the task read.
pub(super) fn lets() -> String {
    format!(
        "LET $slot_open = ({OPEN_TASKS}); LET $slot_wanted = array::union($slot_open.slug, [$slug]);"
    )
}

/// The field of `Which::Starting`'s read the cap decides on.
pub(super) fn field() -> String {
    format!(
        "slots: {{ open: $slot_open, roles: ({ROLES_OF}), counted: ({COUNTERS}), blocks: ({BLOCKS}) }},"
    )
}

/// The variables [`lets`] and [`field`] read beside `$slug`.
pub(super) fn vars() -> Value {
    json!({"slot_statuses": OPEN, "slot_block": super::work::BLOCK_LINE})
}

/// The write's hold on each slot counter it read (`$slots`: handle and `last`): stale if another
/// start moved it since, then moved on.
pub(super) const GUARD: &str = "
FOR $s IN $slots {
    IF ((SELECT VALUE last FROM ONLY type::record('counter', ['slots', $s.handle])) ?? 0) != $s.last { THROW 'stale' };
    UPSERT type::record('counter', ['slots', $s.handle]) SET last = $s.last + 1 RETURN NONE;
};";

/// The status a blocked task held when it was blocked, from the lines that blocked it ([`BLOCKS`],
/// oldest first): the newest of `… moved it STATUS → blocked` and a cancelled prerequisite's block
/// line recording `(was STATUS)`, where a block of work already blocked defers to the one before
/// it. None when no line records it.
pub(super) fn blocked_from(lines: &[String]) -> Option<&str> {
    for line in lines.iter().rev() {
        let was = match line.strip_prefix(super::work::BLOCK_LINE) {
            Some(rest) => (rest.strip_suffix(')'))
                .and_then(|rest| rest.rsplit_once(" (was "))
                .map(|(_, was)| was),
            None => (line.strip_suffix(" → blocked"))
                .and_then(|rest| rest.rsplit_once(" moved it "))
                .map(|(_, was)| was),
        };
        if was != Some("blocked") {
            return was;
        }
    }
    None
}

/// A task as the cap reads it.
#[derive(Clone, Deserialize)]
struct Row {
    slug: String,
    parent: Option<String>,
    status: String,
    #[serde(default)]
    instruction: Option<bool>,
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
    roles: Vec<RoleRead>,
    counted: Vec<(String, i64)>,
    /// The lines that moved the task into `blocked`, oldest first.
    pub(super) blocks: Vec<String>,
}

/// A level of tasks above, read by id, and their roles.
#[derive(Deserialize)]
struct Level {
    tasks: Vec<Row>,
    roles: Vec<RoleRead>,
}

impl Store {
    /// The slot counters the change `opens` describes holds under `cap`, each `{handle, last}` as
    /// read: one for each orchestrator whose open tasks it changes. Refused, naming the holder's
    /// open tasks, when it would raise one's past `cap`. Decided on what `view` read
    /// (`Which::Starting`) and the tasks above the open ones, read by id.
    pub(super) fn slots_taken(
        &self,
        view: &View,
        slug: &str,
        opens: &Opens,
        cap: usize,
    ) -> Result<Vec<Value>, Failure> {
        let Some(read) = &view.snap.slots else {
            return Err(Failure::Stop(super::Error::Db(
                "a start read nothing for the cap".into(),
            )));
        };
        let mut rows: HashMap<String, Row> = HashMap::new();
        for row in &read.open {
            rows.insert(row.slug.clone(), row.clone());
        }
        for task in &view.snap.tasks {
            let row = &task.row;
            rows.entry(row.slug.clone()).or_insert_with(|| Row {
                slug: row.slug.clone(),
                parent: row.parent.clone(),
                status: row.status.clone(),
                instruction: Some(row.instruction),
            });
        }
        let mut roles = read.roles.clone();
        let mut looked: BTreeSet<String> = rows.keys().cloned().collect();
        for _ in 0..DEPTH {
            let wanted: Vec<String> = (slots::wanted(
                rows.values()
                    .map(|r| (r.slug.as_str(), r.parent.as_deref())),
            )
            .into_iter())
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
            orchestrators: orchestrators.remove(row.slug.as_str()).unwrap_or_default(),
            executors: executors.remove(row.slug.as_str()).unwrap_or_default(),
        });
        let parents = (view.snap.sessions.iter())
            .filter_map(|row| Some((row.handle.as_str(), row.parent.as_deref()?)));
        let before = Ledger::new(tasks, parents);
        let by: Vec<&str> = opens.by.iter().map(String::as_str).collect();
        let after = before.changed(
            slug,
            opens.status.as_deref(),
            opens.orchestrator.as_deref(),
            &by,
        );
        let changed = slots::started(&before, &after, cap).map_err(|full| {
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
        })?;
        let counted: HashMap<&str, i64> = (read.counted.iter())
            .map(|(handle, last)| (handle.as_str(), *last))
            .collect();
        Ok(changed
            .into_iter()
            .map(|owner| json!({"handle": owner, "last": counted.get(owner).copied().unwrap_or(0)}))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|t| (*t).to_string()).collect()
    }

    #[test]
    fn a_block_records_the_status_the_task_held_the_newest_first() {
        let moved = lines(&[
            "kite moved it in_progress → blocked",
            "lander moved it review → blocked",
        ]);
        assert_eq!(blocked_from(&moved), Some("review"));
        let by_dependency = lines(&["blocked: cancelled dependencies: sim/a (was in_progress)"]);
        assert_eq!(blocked_from(&by_dependency), Some("in_progress"));
        let twice = lines(&[
            "kite moved it review → blocked",
            "blocked: cancelled dependencies: sim/b (was blocked)",
        ]);
        assert_eq!(
            blocked_from(&twice),
            Some("review"),
            "a block of blocked work defers"
        );
        let legacy = lines(&[
            "kite moved it review → blocked",
            "blocked: cancelled dependencies: sim/a",
        ]);
        assert_eq!(blocked_from(&legacy), None, "the newest records nothing");
        assert_eq!(blocked_from(&[]), None);
    }
}
