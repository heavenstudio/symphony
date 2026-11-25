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
use symphony_board::people::People;
use symphony_board::people::ownership::Map;

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

/// What of a page waits on someone, as one event left it, with who decides each card. Each part
/// is the one the event before holds where this event did not change it ([`Waits::after`]), so a
/// part that is the same allocation before and after an event moved for no reader.
#[derive(Debug, PartialEq)]
pub(crate) struct Waits {
    /// The board's first cards, counts and progress: its version, build and ping left out, as
    /// they say nothing of what waits.
    board: Arc<Board>,
    /// The queue's waiting cards and spot-checks; what waits on an agent is read by everyone.
    queue: Arc<Queue>,
    pending: Arc<Pending>,
    deciders: Option<Arc<Deciders>>,
}

impl Waits {
    /// What `page` holds of what waits now, every part made anew.
    fn of(page: &Page) -> Waits {
        Waits {
            board: board_part(page),
            queue: queue_part(page),
            pending: pending_part(page),
            deciders: page.deciders.clone(),
        }
    }

    /// What `page` holds of what waits now, each part these hold where `page` holds it unchanged
    /// and made anew where it moved: none where no part and no decider moved.
    fn after(&self, page: &Page) -> Option<Waits> {
        let board = (self.board.next == page.board.next
            && self.board.counts == page.board.counts
            && self.board.progress == page.board.progress)
            .then(|| Arc::clone(&self.board));
        let queue = (self.queue.waiting == page.queue.waiting
            && self.queue.spot_check == page.queue.spot_check)
            .then(|| Arc::clone(&self.queue));
        let pending = (*self.pending == page.pending).then(|| Arc::clone(&self.pending));
        if board.is_some() && queue.is_some() && pending.is_some() && self.deciders == page.deciders
        {
            return None;
        }
        Some(Waits {
            board: board.unwrap_or_else(|| board_part(page)),
            queue: queue.unwrap_or_else(|| queue_part(page)),
            pending: pending.unwrap_or_else(|| pending_part(page)),
            deciders: page.deciders.clone(),
        })
    }
}

/// `page`'s board as [`Waits`] holds it, made anew.
fn board_part(page: &Page) -> Arc<Board> {
    #[cfg(test)]
    counted::part();
    Arc::new(Board {
        version: 0,
        build: String::new(),
        ping_ms: 0,
        ..page.board.clone()
    })
}

/// `page`'s waiting cards and spot-checks as [`Waits`] holds them, made anew.
fn queue_part(page: &Page) -> Arc<Queue> {
    #[cfg(test)]
    counted::part();
    Arc::new(Queue {
        waiting: page.queue.waiting.clone(),
        spot_check: page.queue.spot_check.clone(),
        ..Queue::default()
    })
}

/// `page`'s pending list as [`Waits`] holds it, made anew.
fn pending_part(page: &Page) -> Arc<Pending> {
    #[cfg(test)]
    counted::part();
    Arc::new(page.pending.clone())
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

/// What the feed tells `name` moved between `before` and `after`: the changes of [`REGIONS`] a
/// shared event makes of two pages, made of the two as `name` is shown each. A part the event did
/// not change, read by the same deciders, is shown every reader as it was: it moved for none of
/// them, so it is not compared, and a queue it holds is filtered once for both boards.
fn changes(before: &Waits, after: &Waits, name: &str) -> Vec<Value> {
    let decided_alike = before.deciders == after.deciders;
    let queue_alike = decided_alike && Arc::ptr_eq(&before.queue, &after.queue);
    let pending_alike = decided_alike && Arc::ptr_eq(&before.pending, &after.pending);
    let queue = |waits: &Waits| {
        #[cfg(test)]
        counted::filter();
        theirs::queue(&waits.queue, waits.deciders.as_deref(), name)
    };
    let mut changes = Vec::new();
    if !queue_alike {
        let (shown_before, shown_after) = (queue(before), queue(after));
        changes = board_changes(
            &theirs::board(&before.board, &shown_before),
            &theirs::board(&after.board, &shown_after),
        );
        diff_items(
            &mut changes,
            "queue-item",
            &shown_before.waiting,
            &shown_after.waiting,
            |card| format!("{}#{}", card.doc, card.slug),
        );
        diff_items(
            &mut changes,
            "spot-check",
            &shown_before.spot_check,
            &shown_after.spot_check,
            |item| item.href.clone(),
        );
    } else if !Arc::ptr_eq(&before.board, &after.board) {
        // Their queue is the same before and after: filtered once, for both boards.
        let shown = queue(after);
        changes = board_changes(
            &theirs::board(&before.board, &shown),
            &theirs::board(&after.board, &shown),
        );
    }
    if !pending_alike {
        let pending = |waits: &Waits| {
            #[cfg(test)]
            counted::filter();
            theirs::pending(&waits.pending, waits.deciders.as_deref(), name)
        };
        let (before, after) = (pending(before), pending(after));
        diff_items(
            &mut changes,
            "pending",
            &before.items,
            &after.items,
            |item| item.href.clone(),
        );
        if before.docs != after.docs {
            changes.push(region_event("docs", json!(after.docs)));
        }
    }
    changes
}

/// What of `page` waits moved since its last event: its waits then and now, now's kept as the
/// next event's before, sharing each part then's holds unchanged. None where nothing moved, and on
/// a page whose board names nobody and named nobody at its last event, so no event of such a page
/// carries any.
pub(super) fn moved(page: &mut Page) -> Option<(Arc<Waits>, Arc<Waits>)> {
    let before = match page.waits.take() {
        Some(before) => before,
        None if page.deciders.is_none() => return None,
        // A board that named nobody showed a reader it names nothing.
        None => Arc::new(Waits {
            deciders: None,
            ..Waits::of(page)
        }),
    };
    let Some(after) = before.after(page) else {
        page.waits = Some(before);
        return None;
    };
    let after = Arc::new(after);
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

/// The board's people and map as its last look read them, and why a look since could not.
#[derive(Default)]
pub(crate) struct Looked {
    /// The last look that read them: none before the first.
    read: Option<Arc<(People, Map)>>,
    /// Why the board or its people could not be read since that look, in the first failure's
    /// words: none once a look reads them.
    failed: Option<String>,
}

impl Hub {
    /// The board's people and map as its last look read them ([`Hub::people_read`]), which the
    /// notices tell by: none before its first look; `Err` where the board or its people did not
    /// read since ([`Hub::people_unread`]), why.
    pub(super) fn people_looked(&self) -> Result<Option<Arc<(People, Map)>>, String> {
        let looked = self.people.lock().recover();
        match &looked.failed {
            Some(why) => Err(why.clone()),
            None => Ok(looked.read.clone()),
        }
    }

    /// Who decides each card `queue` holds as waiting, on the board's people and map as its last
    /// look read them ([`Hub::people_read`]): none on a board that names nobody; `Err` before its
    /// first look, when a page keeps the ones it had. Reads no board, only the documents.
    pub(super) fn deciders_of(&self, queue: &Queue) -> Result<Option<Arc<Deciders>>, ()> {
        let read = self.people.lock().recover().read.clone().ok_or(())?;
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

    /// A board read's look at its people and map on `store`, the one read of them for its tick,
    /// kept for the pages' reads ([`Hub::deciders_of`]) and the notices' pass the tick wakes
    /// ([`Hub::people_looked`]): where either moved since main's page read who decides its
    /// cards, they are read again, put on every page, and said to the feed by an event of each
    /// page whose shared changes are none, its named readers' diffs its waits'. A board whose
    /// people cannot be read keeps the pages' as they are, and the look says why.
    pub(crate) fn people_read(&self, store: &symphony_board::Store) {
        let read = store.people().and_then(|people| {
            // On a board that names nobody, every thing is the person at the machine's.
            let map = if people.members.is_empty() {
                Map::default()
            } else {
                store.ownership_map()?
            };
            Ok((people, map))
        });
        let read = match read {
            Ok(read) => Arc::new(read),
            Err(error) => return self.people_unread(error.to_string()),
        };
        *self.people.lock().recover() = Looked {
            read: Some(Arc::clone(&read)),
            failed: None,
        };
        let (people, map) = &*read;
        let revision = (!people.members.is_empty()).then_some((people.revision, map.revision));
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

    /// The board or its people could not be read, `why`: the notices tell by no look until a tick
    /// reads them again ([`Hub::people_looked`]), so nobody the board no longer names is told by
    /// an older look, while the pages keep that look's deciders. The first failure since the
    /// last look that read them is kept, so the notices say it once, whatever the next ones say.
    pub(super) fn people_unread(&self, why: String) {
        self.people.lock().recover().failed.get_or_insert(why);
    }
}

/// What this thread made of what waits and filtered of it for a reader, counted for a test's
/// measure of an event.
#[cfg(test)]
pub(crate) mod counted {
    use std::cell::Cell;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct Counted {
        /// Parts of a [`super::Waits`] made anew: a board, a queue, a pending list.
        pub parts: usize,
        /// Queues and pending lists filtered for one reader.
        pub filters: usize,
    }

    thread_local! {
        static COUNTED: Cell<Counted> = const { Cell::new(Counted { parts: 0, filters: 0 }) };
    }

    pub(crate) fn part() {
        COUNTED.with(|counted| {
            counted.set(Counted {
                parts: counted.get().parts + 1,
                ..counted.get()
            })
        });
    }

    pub(crate) fn filter() {
        COUNTED.with(|counted| {
            counted.set(Counted {
                filters: counted.get().filters + 1,
                ..counted.get()
            })
        });
    }

    /// What this thread counted since the last take, counted from none again.
    pub(crate) fn take() -> Counted {
        COUNTED.with(|counted| counted.replace(Counted::default()))
    }
}

#[cfg(test)]
#[path = "theirs_tests.rs"]
mod tests;
