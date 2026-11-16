//! One canonical admission ledger builder, shared with the read-only holdings producer.

use super::{
    DEPTH, Failure, Level, Read, Recorded, RoleRead, Row, Store, View, branches, slots, sql,
};
use crate::slots::{Entry, Ledger};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};

pub(super) struct Observation {
    rows: HashMap<String, Row>,
    roles: Vec<RoleRead>,
    branches: Vec<Recorded>,
}

impl Observation {
    pub(super) fn new(view: &View, read: &Read) -> Self {
        let mut rows = HashMap::new();
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
                red: crate::red::marked(&row.fields).then_some(Value::Bool(true)),
                updated: None,
                held: false,
            });
        }
        Self {
            rows,
            roles: read.roles.clone(),
            branches: view.snap.branches.clone(),
        }
    }

    /// Admission's scoped observation needs the ancestors and paired tasks absent from its
    /// initial read. A whole-board observation already contains them and uses no later reads.
    pub(super) fn read_above(
        &mut self,
        store: &Store,
        parent: Option<&str>,
    ) -> Result<(), Failure> {
        let mut looked: BTreeSet<String> = self.rows.keys().cloned().collect();
        for _ in 0..DEPTH {
            let above = self
                .rows
                .values()
                .map(|r| (r.slug.as_str(), r.parent.as_deref()));
            let wanted: Vec<String> = slots::wanted(above)
                .into_iter()
                .chain(parent)
                .filter(|s| !looked.contains(*s))
                .map(String::from)
                .collect();
            if wanted.is_empty() {
                break;
            }
            looked.extend(wanted.iter().cloned());
            let level: Level = store.transact(&sql::level(), json!({"slot_wanted": wanted}))?;
            for row in level.tasks {
                self.rows.entry(row.slug.clone()).or_insert(row);
            }
            self.roles.extend(level.roles);
            self.branches.extend(level.branches);
        }
        Ok(())
    }

    pub(super) fn ledger<'a>(&'a self, view: &'a View) -> Ledger<'a> {
        let mut orchestrators: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut executors: HashMap<&str, Vec<&str>> = HashMap::new();
        for role in &self.roles {
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
        let tasks = self.rows.values().map(|row| Entry {
            slug: &row.slug,
            branch: branches::latest(&self.branches, &row.slug),
            parent: row.parent.as_deref(),
            status: &row.status,
            instruction: row.instruction.unwrap_or(false),
            held: row.held,
            orchestrators: orchestrators.remove(row.slug.as_str()).unwrap_or_default(),
            executors: executors.remove(row.slug.as_str()).unwrap_or_default(),
            red: row.red.as_ref().is_some_and(|what| !what.is_null()),
        });
        let parents = view
            .snap
            .sessions
            .iter()
            .filter_map(|row| Some((row.handle.as_str(), row.parent.as_deref()?)));
        Ledger::new(tasks, parents)
    }
}
