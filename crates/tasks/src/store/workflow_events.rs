//! A task's recorded workflow transitions, oldest first: what its page's timeline draws the phases
//! and states it went through from (docs/WORKFLOW.md, "The app"). A read alone, of the records
//! `workflow:['event', SLUG, REVISION]` that every move of a task's position writes
//! ([`super::workflow::Transition`]), over the range of its slug's keys: no other task's are read.

use super::postgres::workflow_events as pg;
use super::statement::both;
use super::surreal::workflow_events as sql;
use super::workflow::Transition;
use super::{Error, Store};
use serde_json::{Value, json};

impl Store {
    /// The workflow transitions recorded for `slug`, oldest first: none for a task the board's own
    /// writers have always kept. A record not in this build's shape (a newer board's) refuses the
    /// read whole, so a reader never draws part of a task's history as all of it.
    pub fn workflow_transitions(&self, slug: &str) -> Result<Vec<Transition>, Error> {
        let body: Value = self.retrying(|| {
            self.read(
                both(sql::TRANSITIONS, pg::transitions()),
                json!({"slug": slug}),
            )
        })?;
        crate::objects::read(&body).map_err(|e| {
            Error::Db(format!(
                "a workflow transition not in the board's shape: {e}"
            ))
        })
    }
}

#[cfg(test)]
#[path = "workflow_events_tests.rs"]
mod tests;
