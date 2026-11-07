//! The cap on an orchestrator's open tasks as a rule's write holds to it ([`crate::slots`]).
//!
//! A rule that may start work ([`super::Opens`]) reads, in the same transaction as its task, what
//! the cap decides on (`Which::Starting`): every task's place and status, the orchestrator and
//! executor roles, and each orchestrator's slot counter (`counter:['slots', HANDLE]`). The write
//! then holds each counter it read for an orchestrator it opens a slot for, and moves it on: two
//! starts at once under one orchestrator meet there, and the one that loses reads and decides
//! again, so the cap is never passed by two writes that each saw a slot free. A slot freed meanwhile
//! needs no counter: it only makes a refusal read too strict.

use super::{Failure, Opens, Store, View, refused};
use crate::slots::{self, Entry, Ledger};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;

/// What the cap decides on, a field of the read: the tasks whole (three fields of each), the roles
/// by their role, the counters by their range.
pub(super) const READ: &str = "{
    tasks: (SELECT VALUE [slug, parent, status] FROM task),
    orchestrators: (SELECT VALUE [slug, handle] FROM role WHERE role = 'orchestrator'),
    executors: (SELECT VALUE [slug, handle] FROM role WHERE role = 'executor'),
    counted: (SELECT VALUE [record::id(id)[1], last] FROM counter:['slots', NONE]..['slots', ..]),
}";

/// The write's hold on each slot counter it read (`$slots`: handle and `last`): stale if another
/// start moved it since, then moved on.
pub(super) const GUARD: &str = "
FOR $s IN $slots {
    IF ((SELECT VALUE last FROM ONLY type::record('counter', ['slots', $s.handle])) ?? 0) != $s.last { THROW 'stale' };
    UPSERT type::record('counter', ['slots', $s.handle]) SET last = $s.last + 1 RETURN NONE;
};";

/// [`READ`] as it comes back.
#[derive(Clone, Deserialize)]
pub(super) struct Read {
    tasks: Vec<(String, Option<String>, String)>,
    orchestrators: Vec<(String, String)>,
    executors: Vec<(String, String)>,
    counted: Vec<(String, i64)>,
}

impl Store {
    /// The slot counters starting `slug` as `opens` says takes under `cap`, each `{handle, last}`
    /// as read: one for each orchestrator it opens a new slot for. Refused, naming the holder's open
    /// tasks, when it would open one for an orchestrator already holding `cap`. Decided on what
    /// `view` read (`Which::Starting`), else on a read of its own.
    pub(super) fn slots_taken(
        &self,
        view: &View,
        slug: &str,
        opens: &Opens,
        cap: usize,
    ) -> Result<Vec<Value>, Failure> {
        let own;
        let read: &Read = match &view.snap.slots {
            Some(read) => read,
            None => {
                own = self.transact::<Read>(&format!("RETURN {READ};"), json!({}))?;
                &own
            }
        };
        let mut orchestrators: HashMap<&str, Vec<&str>> = HashMap::new();
        for (task, handle) in &read.orchestrators {
            if view.health(handle).holds() {
                orchestrators.entry(task).or_default().push(handle);
            }
        }
        let mut executors: HashMap<&str, Vec<&str>> = HashMap::new();
        for (task, handle) in &read.executors {
            executors.entry(task).or_default().push(handle);
        }
        // Each task once, its roles taken out as it is read.
        let tasks = read.tasks.iter().map(|(task, parent, status)| Entry {
            slug: task,
            parent: parent.as_deref(),
            status,
            orchestrators: orchestrators.remove(task.as_str()).unwrap_or_default(),
            executors: executors.remove(task.as_str()).unwrap_or_default(),
        });
        let parents = (view.snap.sessions.iter())
            .filter_map(|row| Some((row.handle.as_str(), row.parent.as_deref()?)));
        let ledger = Ledger::new(tasks, parents);
        let by: Vec<&str> = opens.by.iter().map(String::as_str).collect();
        let place = match &opens.orchestrator {
            Some(taker) => ledger.orchestrated_by(slug, taker).place_by(slug, &by),
            None => ledger.place_by(slug, &by),
        };
        let opened = ledger.take(&place, cap).map_err(|full| {
            let children: Vec<&str> = if place.by_parent {
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
        Ok(opened
            .into_iter()
            .map(|owner| json!({"handle": owner, "last": counted.get(owner).copied().unwrap_or(0)}))
            .collect())
    }
}
