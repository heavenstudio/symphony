//! The [`Reader`]: a board's attempts, read for a caller its grant lets read them, with no provider
//! at hand. A host that shows a task's history, its past attempts and the resources and
//! conversations bound to them, needs no [`super::Adapter`]: the [`super::Coordinator`] reads
//! through one of these, and so does a host that holds no provider of the attempts it shows.

use super::keys;
use super::record::Row;
use super::{Act, Attempt, Grant, Recorded, Refusal};
use crate::Error;
use crate::store::Store;
use serde_json::json;
use std::collections::HashMap;
use symphony_plugin::execution::Scope;

/// The attempts of one project's board, read for a caller its [`Grant`] lets [`Act::Read`] them:
/// an attempt of another project or board is refused as [`Refusal::Scope`], however its id came.
pub struct Reader<'a> {
    store: &'a Store,
    scope: Scope,
}

impl<'a> Reader<'a> {
    /// The attempts `store` holds of `scope`'s project and board.
    pub fn new(store: &'a Store, scope: Scope) -> Reader<'a> {
        Reader { store, scope }
    }

    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    /// Attempt `id`.
    pub fn attempt(&self, grant: &Grant, id: &str) -> Result<Attempt, Refusal> {
        let row = self.row(id)?;
        grant.allows(&self.scope, Some(&row.task), Act::Read)?;
        self.view_of(row)
    }

    /// Every attempt of `task`, oldest first: its history, each retry and resume linked to the
    /// attempt before it, its resources and conversations kept after its handle left.
    pub fn history(&self, grant: &Grant, task: &str) -> Result<Vec<Attempt>, Refusal> {
        grant.allows(&self.scope, Some(task), Act::Read)?;
        let mut lanes: HashMap<String, Option<String>> = HashMap::new();
        let mut attempts = Vec::new();
        for value in self.store.execution_of_task(task)? {
            let row = Row::of(&value)?;
            self.same_board(&row)?;
            let lane = &row.identity.lane;
            if !lanes.contains_key(lane) {
                let current = self.lane_holder(task, lane)?;
                lanes.insert(lane.clone(), current);
            }
            let current = lanes[lane].as_deref() == Some(row.attempt.as_str());
            attempts.push(attempt_of(row, current));
        }
        Ok(attempts)
    }

    /// Attempt `id`'s facts, in the order they were appended.
    pub fn facts(&self, grant: &Grant, id: &str) -> Result<Vec<Recorded>, Refusal> {
        let row = self.row(id)?;
        grant.allows(&self.scope, Some(&row.task), Act::Read)?;
        self.store
            .execution_facts(id)?
            .into_iter()
            .map(|value| {
                let fact = crate::objects::read(&value["fact"]).map_err(|e| {
                    Refusal::Board(Error::Db(format!(
                        "an execution fact not in the board's shape: {e}"
                    )))
                })?;
                Ok(Recorded {
                    seq: value["seq"].as_u64().unwrap_or(0),
                    at: value["at"].as_i64().unwrap_or(0),
                    fact,
                })
            })
            .collect()
    }

    /// Attempt `id`'s row, if it is this board's.
    pub(super) fn row(&self, id: &str) -> Result<Row, Refusal> {
        if keys::parse(id).is_none() {
            return Err(Refusal::Unknown(id.to_string()));
        }
        let value = self
            .store
            .execution_attempt(id)?
            .ok_or_else(|| Refusal::Unknown(id.to_string()))?;
        let row = Row::of(&value)?;
        self.same_board(&row)?;
        Ok(row)
    }

    pub(super) fn same_board(&self, row: &Row) -> Result<(), Refusal> {
        if row.identity.project != self.scope.project || row.identity.board != self.scope.board {
            return Err(Refusal::Scope);
        }
        Ok(())
    }

    /// The attempt that holds lane `lane` of `task`, if any.
    pub(super) fn lane_holder(&self, task: &str, lane: &str) -> Result<Option<String>, Refusal> {
        let fence = self.store.execution_fence(json!(["lane", task, lane]))?;
        Ok(fence.and_then(|fence| fence["attempt"].as_str().map(str::to_string)))
    }

    /// `row`'s attempt as a reader sees it, whether it holds its lane read now.
    pub(super) fn view_of(&self, row: Row) -> Result<Attempt, Refusal> {
        let current = self.lane_holder(&row.task, &row.identity.lane)?.as_deref()
            == Some(row.attempt.as_str());
        Ok(attempt_of(row, current))
    }
}

fn attempt_of(row: Row, current: bool) -> Attempt {
    Attempt {
        id: row.attempt,
        created: row.created,
        identity: row.identity,
        state: row.state,
        revision: row.revision,
        current,
    }
}
