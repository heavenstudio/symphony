//! A task's recorded workflow transitions, oldest first: what its page's timeline draws the phases
//! and states it went through from (docs/WORKFLOW.md, "The app"). A read alone, of the records
//! `workflow:['event', SLUG, REVISION]` that every move of a task's position writes
//! ([`super::workflow::Transition`]), over the range of its slug's keys: no other task's are read.

use super::postgres::Pg;
use super::statement::both;
use super::workflow::Transition;
use super::{Error, Store};
use serde_json::{Value, json};

/// The transitions of the task `$slug`, by revision, each its recorded body: an empty list for a
/// task no workflow ever held, or no task at all.
const TRANSITIONS: &str =
    "RETURN (SELECT VALUE body FROM workflow:['event', $slug, NONE]..['event', $slug, ..]);";

/// Twin of [`TRANSITIONS`]. A key is a jsonb array, which PostgreSQL orders by its length, then
/// part by part: a null below every number and `true` above, so the range holds every revision of
/// the slug's events, in order, and reads them from the key's index.
fn transitions() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(body ORDER BY id) FROM workflow \
         WHERE id > jsonb_build_array('event', v ->> 'slug', null) \
         AND id < jsonb_build_array('event', v ->> 'slug', true)), '[]'::jsonb)",
    )
}

impl Store {
    /// The workflow transitions recorded for `slug`, oldest first: none for a task the board's own
    /// writers have always kept. A record not in this build's shape (a newer board's) refuses the
    /// read whole, so a reader never draws part of a task's history as all of it.
    pub fn workflow_transitions(&self, slug: &str) -> Result<Vec<Transition>, Error> {
        let body: Value =
            self.retrying(|| self.read(both(TRANSITIONS, transitions()), json!({"slug": slug})))?;
        serde_json::from_value(body).map_err(|e| {
            Error::Db(format!(
                "a workflow transition not in the board's shape: {e}"
            ))
        })
    }
}

#[cfg(test)]
#[path = "workflow_events_tests.rs"]
mod tests;
