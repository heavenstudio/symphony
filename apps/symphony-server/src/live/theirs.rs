//! What a reader the board names is shown of what waits, and what the feed sends them of it
//! (docs/product/analysis/users-and-identity.md, step 4 of "The way from one owner"): the queue's
//! cards and spot-checks, the board's first cards and counts, and the pending list, each of the
//! cards they decide alone ([`symphony::owners::theirs`]). The person at the machine, and every
//! reader of a board that names nobody, is shown the hub's page whole, as before.
//!
//! A page holds who decides each card it holds as waiting ([`Page::deciders`]), read with the
//! board's people and map as the board's last look read them: as its documents are read again, and
//! as the board's people or map move ([`Hub::people_read`]). The feed's history is one JSON string
//! an event for every subscriber, which says no reader's own; so while a page has deciders, each
//! event that changed what waits carries its waits before and after it ([`Waits`],
//! [`Event::waits`]), and a named reader's feed
//! sends, in place of the event's own regions of what waits ([`REGIONS`]), their diff of the two
//! as they are shown each ([`sent_to`]), made by the same comparisons the shared events are. A
//! change of the people or the map moves no card, so it is an event of its own, whose shared
//! changes are none: a bare version on every other stream, which keeps each one's cursor true.
//!
//! [`Event::waits`]: super::Event::waits

use super::{Hub, Page, board_changes, diff_items, region_event};
use serde_json::{Value, json};
use std::sync::Arc;
use symphony::owners::theirs::{self, Deciders};
use symphony_api::{Board, Pending, Queue};
use symphony_board::lock::Recovered as _;

/// The regions of a feed event that say what waits: the board's first cards, one of them, its
/// counts and progress, and the lists of the queue's cards, its spot-checks and the pending
/// cards, with their orders and the pending list's documents.
pub(crate) const REGIONS: [&str; 11] = [
    "queue",
    "card",
    "counts",
    "progress",
    "queue-item",
    "queue-item-order",
    "spot-check",
    "spot-check-order",
    "pending",
    "pending-order",
    "docs",
];

/// What of a page waits on someone, as one event left it, with who decides each card.
#[derive(Debug, PartialEq)]
pub(crate) struct Waits {
    /// The board, its version, build and ping left out: they say nothing of what waits.
    board: Board,
    /// The queue's waiting cards and spot-checks; what waits on an agent is read by everyone.
    queue: Queue,
    pending: Pending,
    deciders: Option<Arc<Deciders>>,
}

impl Waits {
    /// What `page` holds of what waits now.
    fn of(page: &Page) -> Waits {
        Waits {
            board: Board {
                version: 0,
                build: String::new(),
                ping_ms: 0,
                ..page.board.clone()
            },
            queue: Queue {
                waiting: page.queue.waiting.clone(),
                spot_check: page.queue.spot_check.clone(),
                ..Queue::default()
            },
            pending: page.pending.clone(),
            deciders: page.deciders.clone(),
        }
    }

    /// Whether `page` holds what these say.
    fn holds(&self, page: &Page) -> bool {
        self.board.next == page.board.next
            && self.board.counts == page.board.counts
            && self.board.progress == page.board.progress
            && self.queue.waiting == page.queue.waiting
            && self.queue.spot_check == page.queue.spot_check
            && self.pending == page.pending
            && self.deciders == page.deciders
    }
}

/// What the reader `name` is shown of `board`, `queue` and `pending` by `deciders`.
pub(crate) struct Shown {
    pub board: Board,
    pub queue: Queue,
    pub pending: Pending,
}

/// [`Shown`], as the reader `name` of `deciders` is shown `board`, `queue` and `pending`.
pub(crate) fn shown(
    board: &Board,
    queue: &Queue,
    pending: &Pending,
    deciders: Option<&Deciders>,
    name: &str,
) -> Shown {
    let queue = theirs::queue(queue, deciders, name);
    Shown {
        board: theirs::board(board, &queue),
        pending: theirs::pending(pending, deciders, name),
        queue,
    }
}

impl Shown {
    /// What `waits` shows `name`.
    fn of(waits: &Waits, name: &str) -> Shown {
        let deciders = waits.deciders.as_deref();
        shown(&waits.board, &waits.queue, &waits.pending, deciders, name)
    }
}

/// What the feed tells `name` moved between `before` and `after`: the changes of [`REGIONS`] a
/// shared event makes of two pages, made of the two as `name` is shown each.
fn changes(before: &Waits, after: &Waits, name: &str) -> Vec<Value> {
    let (before, after) = (Shown::of(before, name), Shown::of(after, name));
    let mut changes = board_changes(&before.board, &after.board);
    diff_items(
        &mut changes,
        "queue-item",
        &before.queue.waiting,
        &after.queue.waiting,
        |card| format!("{}#{}", card.doc, card.slug),
    );
    diff_items(
        &mut changes,
        "spot-check",
        &before.queue.spot_check,
        &after.queue.spot_check,
        |item| item.href.clone(),
    );
    diff_items(
        &mut changes,
        "pending",
        &before.pending.items,
        &after.pending.items,
        |item| item.href.clone(),
    );
    if before.pending.docs != after.pending.docs {
        changes.push(region_event("docs", json!(after.pending.docs)));
    }
    changes
}

/// What of `page` waits moved since its last event: its waits then and now, now's kept as the
/// next event's before. None where nothing moved, and on a page whose board names nobody and
/// named nobody at its last event, so no event of such a page carries any.
pub(super) fn moved(page: &mut Page) -> Option<(Arc<Waits>, Arc<Waits>)> {
    if page.waits.as_ref().is_some_and(|waits| waits.holds(page)) {
        return None;
    }
    if page.waits.is_none() && page.deciders.is_none() {
        return None;
    }
    let after = Arc::new(Waits::of(page));
    // A board that named nobody showed a reader it names nothing.
    let before = page.waits.take().unwrap_or_else(|| {
        Arc::new(Waits {
            deciders: None,
            ..Waits::of(page)
        })
    });
    page.waits = page.deciders.is_some().then(|| Arc::clone(&after));
    Some((before, after))
}

/// Follow what of the new `page` waits, while its board names someone: its first event's before
/// is what it holds now.
pub(super) fn begin(page: &mut Page) {
    page.waits = page.deciders.is_some().then(|| Arc::new(Waits::of(page)));
}

/// The feed event `json` as it is sent to `name`, a reader the board names: its regions of what
/// waits left out, and where it carries `waits`, their diff as `name` is shown each, after its
/// other changes. An event that does not read as one is sent as it is.
pub(crate) fn sent_to(json: &str, waits: Option<&(Arc<Waits>, Arc<Waits>)>, name: &str) -> String {
    let Ok(mut event) = serde_json::from_str::<Value>(json) else {
        return json.into();
    };
    let Some(sent) = event["changes"].as_array_mut() else {
        return json.into();
    };
    sent.retain(|change| {
        !(change["region"].as_str()).is_some_and(|region| REGIONS.contains(&region))
    });
    if let Some((before, after)) = waits {
        sent.extend(changes(before, after, name));
    }
    event.to_string()
}

impl Hub {
    /// Who decides each card `queue` holds as waiting, on the board's people and map as its last
    /// look read them ([`Hub::people_read`]): none on a board that names nobody; `Err` before its
    /// first look, when a page keeps the ones it had. Reads no board, only the documents.
    pub(super) fn deciders_of(&self, queue: &Queue) -> Result<Option<Arc<Deciders>>, ()> {
        let read = self.people.lock().recover().clone().ok_or(())?;
        let (people, map) = &*read;
        if people.members.is_empty() {
            return Ok(None);
        }
        let cards = theirs::cards(queue);
        Ok(Some(Arc::new(Deciders::read(
            people, map, &self.root, &cards,
        ))))
    }

    /// Main's page's board, at its version, as `reader`, a person a board named, is shown it, and
    /// whole to anyone else: none before main's page is read.
    pub(crate) fn main_board(&self, reader: Option<&str>) -> Option<Board> {
        let pages = self.pages.read().recover();
        let main = pages.get("")?;
        Some(match reader {
            Some(name) => {
                let queue = theirs::queue(&main.queue, main.deciders.as_deref(), name);
                theirs::board(&main.board, &queue)
            }
            None => main.board.clone(),
        })
    }

    /// A board read's look at its people and map on `store`, kept for the pages' reads
    /// ([`Hub::deciders_of`]): where either moved since main's page read who decides its cards,
    /// they are read again, put on every page, and said to the feed by an event of each page
    /// whose shared changes are none, its named readers' diffs its waits'. A board whose people
    /// cannot be read keeps the pages' as they are.
    pub(crate) fn people_read(&self, store: &symphony_board::Store) {
        let Ok(people) = store.people() else {
            return;
        };
        let map = if people.members.is_empty() {
            Default::default()
        } else {
            match store.ownership_map() {
                Ok(map) => map,
                Err(_) => return,
            }
        };
        let revision = (!people.members.is_empty()).then_some((people.revision, map.revision));
        let read = Arc::new((people, map));
        *self.people.lock().recover() = Some(Arc::clone(&read));
        let cards = {
            let pages = self.pages.read().recover();
            let Some(main) = pages.get("") else {
                return;
            };
            if main.deciders.as_ref().map(|held| held.revision) == revision {
                return;
            }
            theirs::cards(&main.queue)
        };
        let (people, map) = &*read;
        let deciders = revision.map(|_| Arc::new(Deciders::read(people, map, &self.root, &cards)));
        let mut pages = self.pages.write().recover();
        // Main's cards moved meanwhile: the refresh that moved them read their deciders.
        if pages
            .get("")
            .is_none_or(|main| theirs::cards(&main.queue) != cards)
        {
            return;
        }
        for page in pages.values_mut() {
            page.deciders = deciders.clone();
            self.publish(page, Vec::new());
        }
    }
}

#[cfg(test)]
#[path = "theirs_tests.rs"]
mod tests;
