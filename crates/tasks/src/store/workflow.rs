//! A custom delivery workflow's records on the board (docs/WORKFLOW.md), from schema 23: the
//! active generation, the immutable definitions, activations, transitions and receipts in the
//! `workflow` table, and the position of each task the workflow holds in its
//! `fields._board.workflow`.
//!
//! Nothing but this module writes them, and the schema's fences hold every writer to that: a
//! task's position, or the status of a task that has one, changes only in the write that records
//! its transition, and a recorded definition, activation, transition or receipt is never changed
//! (`crate::schema`'s `workflow_position`, `workflow_history` and `workflow_active`, and their
//! PostgreSQL twins). `Store::raw` refuses the table, and `Store::set_field` the board's own
//! fields. A board with no custom workflow holds none of this, and every other write meets the
//! fences as it always ran.

use super::postgres::workflow as pg;
use super::statement::both;
use super::surreal::workflow as sql;
use super::{Error, Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cmp::Ordering;
use symphony_api::workflow::{Definition, Effect, Operation, Pair, Revision};
use symphony_plugin::execution::Principal;

/// Where a task a custom workflow holds stands (`fields._board.workflow`): its pair, the pair a
/// blocked task resumes to, the activation generation and digest it was last moved under, and its
/// workflow revision, which only its own transitions move (a post on its thread or an edit of its
/// title moves none).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskPosition {
    pub phase: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<Pair>,
    pub generation: u64,
    pub revision: u64,
    pub digest: String,
}

impl TaskPosition {
    /// Its pair.
    pub fn at(&self) -> Pair {
        Pair {
            phase: self.phase.clone(),
            state: self.state.clone(),
        }
    }
}

/// One move of a task's position, as recorded (`workflow:['event', slug, revision]`): an
/// admitted operation on an edge, or an activation moving the task to its mapped pair.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    /// Its stable identity, `SLUG@REVISION`.
    pub id: String,
    pub slug: String,
    /// The task's workflow revision it made.
    pub revision: u64,
    pub generation: u64,
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<Operation>,
    /// The activation that moved it, for a move no edge made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Pair>,
    pub to: Pair,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<Pair>,
    /// The board status the position projects to, which the task's row holds.
    pub status: String,
    pub principal: Principal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    /// The idempotency key of the request that made it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub at: f64,
}

impl Transition {
    /// The identity of the transition that made `revision` of `slug`.
    pub fn id_of(slug: &str, revision: u64) -> String {
        format!("{slug}@{revision}")
    }

    /// The position it puts its task at.
    pub fn position(&self) -> TaskPosition {
        TaskPosition {
            phase: self.to.phase.clone(),
            state: self.to.state.clone(),
            resume: self.resume.clone(),
            generation: self.generation,
            revision: self.revision,
            digest: self.digest.clone(),
        }
    }
}

/// The active custom workflow: its activation generation and its definition's digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Active {
    pub generation: u64,
    pub digest: String,
}

/// A definition as recorded under its digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recorded {
    pub definition: Definition,
    pub revision: Revision,
}

impl Store {
    /// The active custom workflow, or none on a board that never activated one.
    pub fn workflow_active(&self) -> Result<Option<Active>, Error> {
        let body: Value =
            self.retrying(|| self.read(both(sql::ACTIVE, pg::active()), json!({})))?;
        decoded(body, "active workflow")
    }

    /// The definition recorded under `digest`, or none.
    pub fn workflow_definition(&self, digest: &str) -> Result<Option<Recorded>, Error> {
        let body: Value = self.retrying(|| {
            self.read(
                both(sql::DEFINITION, pg::definition()),
                json!({"digest": digest}),
            )
        })?;
        decoded(body, "workflow definition")
    }

    /// `slug`'s workflow position, or none for a task the board's own writers keep; refused for
    /// a task that is not there.
    pub fn workflow_position(&self, slug: &str) -> Result<Option<TaskPosition>, Error> {
        let body: Value = self.retrying(|| {
            self.transact(both(sql::POSITION, pg::position()), json!({"slug": slug}))
        })?;
        decoded(body, "workflow position")
    }

    /// `transition` recorded and its task put where it goes, in one write, when the task is at
    /// workflow revision `expected` (0 for a task no workflow held yet); refused as `stale_task`
    /// at any other. The one write of a position: activation and admission call it, nothing else.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "activation and admission, the slices after storage, call it"
        )
    )]
    pub(crate) fn record_transition(
        &self,
        transition: &Transition,
        expected: u64,
    ) -> Result<(), Error> {
        let vars = json!({
            "slug": transition.slug,
            "expected": expected,
            "transition": transition,
            "position": transition.position(),
        });
        self.retrying(|| self.transact::<Value>(both(sql::RECORD, pg::record()), vars.clone()))
            .map(drop)
    }
}

/// `body` as `T`, none for null; a body in another shape is the database's error, named `what`.
fn decoded<T: serde::de::DeserializeOwned>(body: Value, what: &str) -> Result<Option<T>, Error> {
    if body.is_null() {
        return Ok(None);
    }
    serde_json::from_value(body)
        .map(Some)
        .map_err(|e| Error::Db(format!("a {what} not in the board's shape: {e}")))
}

/// The order of two workflow record keys, the parts of their ids, as SurrealDB orders compound
/// ids: part by part, a number by its value and a text by its bytes, a shorter key first where
/// one is the start of the other. PostgreSQL's jsonb orders arrays by length first, so the dump
/// sorts by this, and a board's export is one text in either database.
pub(crate) fn key_order(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Array(a), Value::Array(b)) => {
            for (x, y) in a.iter().zip(b) {
                match key_order(x, y) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            a.len().cmp(&b.len())
        }
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap_or(0.0), y.as_f64().unwrap_or(0.0));
            x.partial_cmp(&y).unwrap_or(Ordering::Equal)
        }
        (Value::String(x), Value::String(y)) => x.cmp(y),
        // Numbers before texts before anything else, as SurrealDB sorts kinds apart.
        (x, y) => rank(x).cmp(&rank(y)),
    }
}

fn rank(value: &Value) -> u8 {
    match value {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    }
}

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;
