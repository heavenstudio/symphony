//! The machine's schema updates as the Runs page reads them
//! ([`symphony_api::SCHEMA_UPDATES_PATH`]): the machine folder's ledger, which a release's refresh
//! writes ([`symphony::machine::updates`]), each update said as the page draws it. No board is
//! read for it, and every server answers it, for any project it serves, from the same ledger.

use symphony::machine::updates::{self, Ledger, Stamp, Task, Update};
use symphony_api::{SCHEMA_UPDATES_SHOWN, SchemaUpdate, SchemaUpdates, UpdateState, UpdateTime};

use crate::live::Hub;

/// What the route answers on `hub`'s server: the updates of the machine's ledger [`shown`], each
/// with its task's page where this server serves it ([`Hub::task_pages`]); none, and why, while the
/// ledger cannot be read or this server finds no machine folder. A machine with no ledger yet has
/// none, and nothing to say of it.
pub(crate) fn answer(hub: &Hub) -> SchemaUpdates {
    let Some(machine) = hub.machine.as_deref() else {
        return SchemaUpdates {
            updates: Vec::new(),
            unread: Some("this server finds no machine folder".into()),
        };
    };
    match Ledger::read(machine) {
        Ok(ledger) => {
            let page = hub.task_pages();
            SchemaUpdates {
                updates: shown(&ledger).map(|update| said(&page, update)).collect(),
                unread: None,
            }
        }
        Err(why) => SchemaUpdates {
            updates: Vec::new(),
            unread: Some(why),
        },
    }
}

/// The updates the answer holds, newest first (the ledger's order turned round): every open one,
/// and the [`SCHEMA_UPDATES_SHOWN`] newest whatever their state.
fn shown(ledger: &Ledger) -> impl Iterator<Item = &Update> {
    (ledger.updates.iter().rev().enumerate())
        .filter(|(newer, update)| *newer < SCHEMA_UPDATES_SHOWN || update.open())
        .map(|(_, update)| update)
}

/// `update` as the page draws it: its times with where each comes from ([`source`]), its waits in
/// the owner's words ([`updates::said`]), its last failure's words and its task's `page` on the
/// server that answers.
fn said(page: &impl Fn(&Task) -> Option<String>, update: &Update) -> SchemaUpdate {
    let time = |stamp: &Option<Stamp>| {
        stamp.as_ref().map(|stamp| UpdateTime {
            at: stamp.at,
            source: source(&stamp.source).to_string(),
            words: stamp.source.clone(),
        })
    };
    let state = if update.served.is_some() {
        UpdateState::Served
    } else if update.left.is_some() {
        UpdateState::Left
    } else {
        UpdateState::Open
    };
    SchemaUpdate {
        id: update.id.clone(),
        project: update.project.clone(),
        schema: update.schema,
        generation: update.generation,
        state,
        reached: time(&update.reached),
        available: time(&update.available),
        served: time(&update.served),
        total: update.total(),
        total_said: update.total().map(updates::said),
        after_available: update.after_available(),
        after_available_said: update.after_available().map(updates::said),
        failures: u32::try_from(update.failures.len()).unwrap_or(u32::MAX),
        failure: update.failures.last().map(|failure| failure.why.clone()),
        task: update.task.as_ref().and_then(page),
    }
}

/// A time's source as `release --status` names it: the record's words before their first colon,
/// trimmed; all of them when they have none.
fn source(words: &str) -> &str {
    words.split(':').next().unwrap_or(words).trim()
}

#[cfg(test)]
#[path = "schema_updates/tests.rs"]
mod tests;
