//! What waits on one reader a board names (docs/product/analysis/users-and-identity.md, step 4 of
//! "The way from one owner": queues follow a thing's group): of the cards a page holds as waiting,
//! those the reader is among the deciders of ([`ownership::deciders`]), each card owned as the
//! notices read it ([`super::waiting`]), so a card's queues and its notices name the same people.
//! The queue's cards and its spot-checks, the board's first cards and its counts, and the pending
//! list are each said to them filtered; the agent queue, the activity and the documents are read
//! by everyone, and stay whole. Of the comments held on documents the queue lists, a reader is
//! shown those that wait on them to pass on, as the notices tell them ([`Deciders::passing`]).
//!
//! The person at the machine decides every thing, and is shown every card as before: nothing here
//! is asked for them. A reader the board names is shown no card where its deciders were never read
//! (a board that names nobody, or one whose people could not be read).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use symphony_api::{Board, Pending, Queue};
use symphony_board::people::People;
use symphony_board::people::ownership::{self, Map};

use crate::dashboard::{count, document_path};

/// Who decides each card that waits on someone, by the queue's document and the card's slug.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Deciders {
    /// The board's people revision and its map's, as they were read.
    pub revision: (i64, i64),
    cards: BTreeMap<(String, String), Vec<String>>,
    /// Who each comment held on a document waits on to pass it on, by its number.
    held: BTreeMap<i64, Vec<String>>,
}

impl Deciders {
    /// Of the cards `cards`, each the queue's document (`docs/…/X.md`, or `@scope/docs/…/X.md` for
    /// a branch's proposal) and its slug, on `people` and `map`: each owned as main's file of its
    /// document holds it in the checkout at `root` ([`super::waiting`]), and decided by those of
    /// its owner who hold decide ([`ownership::deciders`]).
    pub fn read(people: &People, map: &Map, root: &Path, cards: &[(String, String)]) -> Deciders {
        let owners = super::waiting(people, map, root, cards);
        let mut deciders = BTreeMap::new();
        deciders.extend((cards.iter().zip(&owners)).map(|((doc, slug), owning)| {
            let key = (doc.trim_start_matches('/').to_string(), slug.clone());
            (key, ownership::deciders(people, owning))
        }));
        let cards = deciders;
        Deciders {
            revision: (people.revision, map.revision),
            cards,
            held: BTreeMap::new(),
        }
    }

    /// These deciders, with `held` saying who each comment held on a document waits on to pass it
    /// on, by its number, in place of what they said: the people its notice tells.
    #[must_use]
    pub fn passing(self, held: impl IntoIterator<Item = (i64, Vec<String>)>) -> Deciders {
        let mut passing = BTreeMap::new();
        passing.extend(held);
        Deciders {
            held: passing,
            ..self
        }
    }

    /// Whether the comment held on a document `id` waits on `name` to pass it on: not for one
    /// these deciders were not told of.
    pub fn passes(&self, id: i64, name: &str) -> bool {
        (self.held.get(&id)).is_some_and(|names| names.iter().any(|passer| passer == name))
    }

    /// Whether `name` decides the card `slug` of the queue's document `doc`: not for a card these
    /// deciders were not read for.
    pub fn decides(&self, doc: &str, slug: &str, name: &str) -> bool {
        let key = (doc.trim_start_matches('/').to_string(), slug.to_string());
        self.cards
            .get(&key)
            .is_some_and(|names| names.iter().any(|decider| decider == name))
    }
}

/// The cards `queue` holds as waiting on someone, each its document and its slug: its waiting
/// cards, then its spot-checks, each once. What [`Deciders::read`] is asked of.
pub fn cards(queue: &Queue) -> Vec<(String, String)> {
    let waiting = (queue.waiting.iter()).map(|card| (card.doc.as_str(), card.slug.as_str()));
    let spot = (queue.spot_check.iter()).map(|item| (item.path.as_str(), item.slug.as_str()));
    let mut seen = BTreeSet::new();
    (waiting.chain(spot))
        .map(|(doc, slug)| (doc.trim_start_matches('/'), slug))
        .filter(|card| seen.insert(*card))
        .map(|(doc, slug)| (doc.to_string(), slug.to_string()))
        .collect()
}

/// Whether `name` decides the card `slug` of `doc` by `deciders`: never where none were read.
fn decides(deciders: Option<&Deciders>, doc: &str, slug: &str, name: &str) -> bool {
    deciders.is_some_and(|deciders| deciders.decides(doc, slug, name))
}

/// `queue` as `name` is shown it: its waiting cards and its spot-checks theirs alone, in its
/// order, and the comments held on documents that wait on them to pass on; the cards waiting on
/// an agent whole.
pub fn queue(queue: &Queue, deciders: Option<&Deciders>, name: &str) -> Queue {
    Queue {
        held: (queue.held.iter())
            .filter(|held| deciders.is_some_and(|deciders| deciders.passes(held.id, name)))
            .cloned()
            .collect(),
        waiting: (queue.waiting.iter())
            .filter(|card| decides(deciders, &card.doc, &card.slug, name))
            .cloned()
            .collect(),
        spot_check: (queue.spot_check.iter())
            .filter(|item| decides(deciders, &item.path, &item.slug, name))
            .cloned()
            .collect(),
        agent: queue.agent.clone(),
        branches: queue.branches.clone(),
    }
}

/// `board` as the reader shown `theirs` (their [`queue`]) is shown it: its first cards
/// [`symphony_api::BOARD_CARDS`] of theirs, the cards waiting on them and their documents, their
/// spot-checks, the held comments they may pass on and the decisions waiting counted from theirs;
/// the rest whole.
pub fn board(board: &Board, theirs: &Queue) -> Board {
    let mut shown = board.clone();
    shown.next = (theirs.waiting.iter())
        .take(symphony_api::BOARD_CARDS)
        .cloned()
        .collect();
    shown.counts.waiting = count(theirs.waiting.len());
    let mut docs = BTreeSet::new();
    docs.extend((theirs.waiting.iter()).map(|card| document_path(&card.doc)));
    shown.counts.docs_waiting = count(docs.len());
    shown.counts.spot_check = count(theirs.spot_check.len());
    shown.counts.held = count(theirs.held.len());
    shown.progress.decisions_waiting = shown.counts.waiting;
    shown
}

/// `pending` as `name` is shown it: its cards theirs alone, in its order, and its documents but a
/// branch's proposal's where none of theirs is (a branch's own document, `@scope/…`, is there only
/// for the cards it proposes).
pub fn pending(pending: &Pending, deciders: Option<&Deciders>, name: &str) -> Pending {
    let items: Vec<_> = (pending.items.iter())
        .filter(|item| decides(deciders, &item.path, &item.slug, name))
        .cloned()
        .collect();
    let docs = (pending.docs.iter())
        .filter(|doc| !doc.starts_with('@') || items.iter().any(|item| item.path == **doc))
        .cloned()
        .collect();
    Pending { items, docs }
}

#[cfg(test)]
#[path = "theirs_tests.rs"]
mod tests;
