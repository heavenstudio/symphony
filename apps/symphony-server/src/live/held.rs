//! What the hub tells its pages of the comments held on documents (docs/product/analysis/
//! users-and-identity.md, "Words without agent reach"): the `held` feed region, with no data, so a
//! document's page and the review queue read them again only when one was held or passed on, never
//! on a timer, as a hold writes no file for the watcher to tell (`symphony_board::held::OnDocument`);
//! and the queue's list of them, which every page's queue holds whole and its counts count
//! ([`symphony_api::Queue::held`]).
//!
//! The list carries no words: each comment's document, its number and its author, the link the
//! notice of it gives. Who it waits on is who its notice tells ([`super::notices`]'s
//! `held_waiting`), and a reader the board names is shown those that wait on them alone
//! ([`symphony::owners::theirs::Deciders::passing`]); the person at the machine, who may pass every
//! one on, is shown them all, and the server's reads keep them from a hosted visitor, who may pass
//! none on (`crate::api`). The agents' queues, built from the documents alone, never list them.

use super::Hub;
use serde_json::{Value, json};
use std::sync::Arc;
use symphony_api::QueueHeld;
use symphony_board::held::OnDocument;
use symphony_board::lock::Recovered as _;
use symphony_board::people::People;
use symphony_board::people::ownership::Map;

/// The feed's change that says a comment held on a document moved, with nothing of it.
fn moved() -> Value {
    json!({"region": "held", "data": {}})
}

impl Hub {
    /// Every page hears that a comment held on a document moved: held, or passed on. The board's
    /// comments held on documents are read again first, so a queue read once its page hears lists
    /// them as they now stand; a read that fails leaves the pages' list as it was, read again at
    /// the next tick ([`Hub::held_read`]).
    pub(crate) fn held_moved(&self) {
        let read = (self.reader()).and_then(|store| {
            (store.held_on_documents()).map_err(|error| format!("the held comments: {error}"))
        });
        if let Err(why) = &read {
            eprintln!("· {why}: the queue lists them as it did");
        }
        self.held_put(read.ok().map(Arc::new), true);
    }

    /// The comments held on documents read on `store`, the board a tick read, where they are
    /// unread: before the first, and after a read that failed. A read that fails again is tried at
    /// the next tick. Each page hears them only where its list moved.
    pub(super) fn held_read(&self, store: &symphony_board::Store) {
        if self.held.lock().recover().is_some() {
            return;
        }
        if let Ok(held) = store.held_on_documents() {
            self.held_put(Some(Arc::new(held)), false);
        }
    }

    /// The comments `held` on documents kept (none: unread, the pages' list left as it was), and
    /// every page's queue listing them and its counts counting them, who each waits on told to the
    /// pages' deciders on the board's people as its last look read them. A page hears what moved
    /// of its counts and, where `told` or its list moved, the `held` region.
    fn held_put(&self, held: Option<Arc<Vec<OnDocument>>>, told: bool) {
        (*self.held.lock().recover()).clone_from(&held);
        let rows = held.as_deref().map(|held| self.held_rows(held));
        let passing = (held.as_deref()).and_then(|held| self.held_passing(held));
        let mut pages = self.pages.write().recover();
        // Every page holds main's deciders: told once, for all of them.
        let deciders = (pages.get("").and_then(|main| main.deciders.clone()))
            .zip(passing)
            .map(|(deciders, passing)| Arc::new((*deciders).clone().passing(passing)));
        for page in pages.values_mut() {
            let mut changes = Vec::new();
            if let Some(rows) = &rows
                && page.queue.held != *rows
            {
                page.queue.held.clone_from(rows);
                changes.push(moved());
                let counted = u32::try_from(rows.len()).unwrap_or(u32::MAX);
                if page.board.counts.held != counted {
                    page.board.counts.held = counted;
                    changes.push(super::region_event("counts", json!(page.board.counts)));
                }
            }
            if told && changes.is_empty() {
                changes.push(moved());
            }
            if let Some(deciders) = &deciders {
                page.deciders = Some(Arc::clone(deciders));
            }
            self.publish(page, changes);
        }
    }

    /// The comments `held` on documents as the queue lists them, in their order, each document's
    /// title read from main's checkout, a branch's as main's file names it.
    fn held_rows(&self, held: &[OnDocument]) -> Vec<QueueHeld> {
        (held.iter())
            .map(|held| {
                let place = &held.document;
                QueueHeld {
                    id: held.id,
                    href: page(held),
                    doc_title: symphony::tree::title(&self.root.join(&place.path)),
                    author: held.author.clone(),
                }
            })
            .collect()
    }

    /// The queue's list of the comments held on documents as the hub last read them: none where
    /// they are unread.
    pub(super) fn held_listed(&self) -> Option<Vec<QueueHeld>> {
        let held = self.held.lock().recover().clone()?;
        Some(self.held_rows(&held))
    }

    /// Who each of `held` waits on to pass it on, on the board's people and map as its last look
    /// read them ([`passing`]): none before its first look, and on a board that names nobody, whose
    /// pages name no reader.
    fn held_passing(&self, held: &[OnDocument]) -> Option<Vec<(i64, Vec<String>)>> {
        let looked = self.people_looked().ok().flatten()?;
        let (people, map) = &*looked;
        (!people.members.is_empty()).then(|| passing(people, map, &self.root, held))
    }

    /// Who each comment held on a document that the hub last read waits on to pass it on, on
    /// `people` and `map` ([`passing`]): none where they are unread.
    pub(super) fn held_passing_on(&self, people: &People, map: &Map) -> Vec<(i64, Vec<String>)> {
        let held = self.held.lock().recover().clone();
        (held.as_deref()).map_or_else(Vec::new, |held| passing(people, map, &self.root, held))
    }

    /// Whether the hub holds a comment held on a document: a change of the board's people or map
    /// may move who it waits on.
    pub(super) fn holds_held(&self) -> bool {
        (self.held.lock().recover().as_deref()).is_some_and(|held| !held.is_empty())
    }
}

/// The page `held` is drawn on, as the queue's documents are named: `docs/…/X.md`, or
/// `@scope/docs/…/X.md` on a branch's page.
pub(super) fn document(held: &OnDocument) -> String {
    let place = &held.document;
    match &place.scope {
        Some(scope) => format!("@{scope}/{}", place.path),
        None => place.path.clone(),
    }
}

/// Where `held` is drawn, its box on its document's page: what its notice links and the queue's
/// row leads to.
pub(super) fn page(held: &OnDocument) -> String {
    format!("/{}#held-{}", document(held), held.id)
}

/// Who each of `held` waits on to pass it on, on `people` and `map` (`root` the project's
/// checkout): those its notice tells, by the notices' own reading of it ([`super::notices`]).
pub(super) fn passing(
    people: &People,
    map: &Map,
    root: &std::path::Path,
    held: &[OnDocument],
) -> Vec<(i64, Vec<String>)> {
    let waiting = super::notices::held_waiting(people, map, root, held);
    (held.iter().zip(waiting))
        .map(|(held, waiting)| (held.id, waiting.told.unwrap_or_default()))
        .collect()
}

#[cfg(test)]
#[path = "held_tests.rs"]
mod tests;
