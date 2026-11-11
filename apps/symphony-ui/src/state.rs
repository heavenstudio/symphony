//! The pure half of following the feed: one event applied to the board in place, how long to
//! wait before trying a lost feed again, and when another client build loads the page again.

use symphony_api::{ActivityItem, BOARD_CARDS, Board, Change, Delta};

/// The first wait before a lost feed is tried again, in ms.
const FIRST_WAIT_MS: i32 = 1_000;
/// The longest wait between tries, in ms; a stream that held this long starts the waits again.
const LAST_WAIT_MS: i32 = 30_000;

/// Coalesce the initial metadata read with the first feed handshake. A reconnect still
/// requires a fresh snapshot; a failed initial read may retry once after that handshake.
#[derive(Clone, Copy, Default)]
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) struct MetadataReads {
    connections: u64,
    active: bool,
    ready: bool,
}

#[cfg(any(target_arch = "wasm32", test))]
impl MetadataReads {
    pub(crate) fn connected(&mut self) -> bool {
        self.connections += 1;
        self.connections > 1 || (!self.active && !self.ready)
    }

    pub(crate) fn start(&mut self) -> u64 {
        self.active = true;
        self.connections
    }

    /// Only the current request completes this state; callers discard superseded responses.
    pub(crate) fn finished(&mut self, started: u64, success: bool) -> bool {
        self.active = false;
        self.ready = success;
        !success && started < self.connections
    }
}

#[cfg(test)]
mod metadata_tests {
    use super::MetadataReads;

    #[test]
    fn the_first_connection_reuses_an_active_or_successful_initial_read() {
        let mut reads = MetadataReads::default();
        let started = reads.start();
        assert!(!reads.connected());
        assert!(!reads.finished(started, true));
        assert!(reads.connected(), "a reconnect needs a fresh snapshot");

        let mut reads = MetadataReads::default();
        let started = reads.start();
        assert!(!reads.finished(started, true));
        assert!(!reads.connected());
    }

    #[test]
    fn a_failed_initial_read_retries_once_on_either_side_of_the_handshake() {
        let mut reads = MetadataReads::default();
        let started = reads.start();
        assert!(!reads.finished(started, false));
        assert!(reads.connected());

        let mut reads = MetadataReads::default();
        let started = reads.start();
        assert!(!reads.connected());
        assert!(reads.finished(started, false));
        let retried = reads.start();
        assert!(
            !reads.finished(retried, false),
            "do not loop on a failing API"
        );
        assert!(reads.connected(), "the next reconnect can retry again");
    }

    #[test]
    fn a_reconnect_refreshes_even_while_an_older_read_is_active() {
        let mut reads = MetadataReads::default();
        assert!(reads.connected());
        reads.start();
        assert!(reads.connected());
        let replacement = reads.start();
        assert!(!reads.finished(replacement, true));
    }
}

/// The waits between tries of a lost feed: 1 s, doubling to 30 s, until a stream holds 30 s. A
/// stream that ends as soon as it opens does not start them again, so a server that keeps
/// closing it is asked ever less often.
#[derive(Debug)]
pub struct Backoff {
    next: i32,
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff {
            next: FIRST_WAIT_MS,
        }
    }
}

impl Backoff {
    /// The wait, in ms, after a stream that held `held_ms` (0 when it never opened).
    pub fn after(&mut self, held_ms: f64) -> i32 {
        if held_ms >= f64::from(LAST_WAIT_MS) {
            self.next = FIRST_WAIT_MS;
        }
        let wait = self.next;
        self.next = (wait * 2).min(LAST_WAIT_MS);
        wait
    }
}

/// Whether a client whose own build is `own` loads the page again for the board naming `served`:
/// only for another build, and once per build (`tried` is the one it last loaded again for), so a
/// shell the client cannot match never loads it in a loop. A client that cannot name its own
/// build never does.
pub fn reload_for(own: Option<&str>, served: &str, tried: Option<&str>) -> bool {
    own.is_some_and(|own| own != served) && tried != Some(served)
}

/// Whether a read of `/api/activity` leaves the persistent panel's baseline where it is instead
/// of moving it to `known`, the ids the page knew before this read. The panel compares its
/// first activity snapshot with the baseline when it mounts, and toasts what the baseline lacks.
/// Until the panel is ready, `known` holding an id the baseline lacks means an earlier read
/// already announced that arrival and is loading the panel for it; moving the baseline then
/// (another read while the panel's module loads, such as a reconnect's) would make the arrival
/// look old, and it would never show.
pub fn keeps_panel_baseline<T: PartialEq>(
    baseline: Option<&[T]>,
    known: Option<&[T]>,
    panel_ready: bool,
) -> bool {
    !panel_ready
        && matches!((baseline, known), (Some(baseline), Some(known))
            if known.iter().any(|id| !baseline.contains(id)))
}

/// What a page's feed heard of the asks (the server's `asks` region), for the readers of ask
/// threads in any module: the open chat and a document's questions read again on it, never on a
/// timer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AsksNews {
    /// The stream opened again: anything may have moved while it was away.
    pub everything: bool,
    /// The board names another chat recipient, which every thread's reply shows.
    pub recipient: bool,
    /// Each thread with a new message: its id and its document's path.
    pub threads: Vec<(i64, String)>,
}

impl AsksNews {
    /// Whether a reader of thread `id`, or of document `path`'s threads, reads again.
    pub fn concerns(&self, id: Option<i64>, path: Option<&str>) -> bool {
        self.everything
            || self.recipient
            || self
                .threads
                .iter()
                .any(|(moved, on)| id == Some(*moved) || path == Some(on.as_str()))
    }
}

/// How long a batch of activity toasts stays up unless dismissed, in ms.
pub const TOAST_MS: i32 = 7_000;
/// The most activity toasts up at once; a newer one takes the oldest down.
pub const TOASTS: usize = 3;

/// The activity toasts on screen, oldest first. The toasts that arrive together are a batch and
/// leave together, `TOAST_MS` after they arrived: a batch's expiry takes down only its own
/// toasts, never a later batch's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Toasts {
    shown: Vec<(u64, ActivityItem)>,
    batches: u64,
}

impl Toasts {
    /// Put `items` up as one new batch, keeping the newest `TOASTS`, and name the batch for its
    /// expiry.
    pub fn arrive(&mut self, items: Vec<ActivityItem>) -> u64 {
        self.batches += 1;
        let batch = self.batches;
        self.shown
            .extend(items.into_iter().map(|item| (batch, item)));
        let over = self.shown.len().saturating_sub(TOASTS);
        self.shown.drain(..over);
        batch
    }

    /// Take down what is left of `batch`, its time up.
    pub fn expire(&mut self, batch: u64) {
        self.shown.retain(|(shown, _)| *shown != batch);
    }

    /// Take down the toast for `id`, as its dismiss button does.
    pub fn dismiss(&mut self, id: &str) {
        self.shown.retain(|(_, item)| item.id != id);
    }

    /// The toasts up, oldest first.
    pub fn items(&self) -> impl Iterator<Item = &ActivityItem> {
        self.shown.iter().map(|(_, item)| item)
    }
}

/// A region of the board that moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    /// The card at this index of `Board::next`.
    Card(usize),
    Queue,
    Counts,
    Progress,
}

/// What the owner is told when more cards wait on them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
}

/// What an event did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The regions that moved, in the event's order.
    pub moved: Vec<Region>,
    /// Set when more cards wait on the owner after the event than before it.
    pub notice: Option<Notice>,
}

/// Apply `delta` to `board`. An event at or before the board's version was already applied
/// (the feed replays from `since` on a reconnect), so it changes nothing.
pub fn apply(board: &mut Board, delta: &Delta) -> Outcome {
    let mut outcome = Outcome::default();
    if delta.version <= board.version {
        return outcome;
    }
    let waiting = board.counts.waiting;
    for change in &delta.changes {
        match change {
            Change::Card(card) => {
                if let Some(index) = board
                    .next
                    .iter()
                    .position(|shown| shown.slug == card.slug && shown.doc == card.doc)
                {
                    board.next[index] = card.clone();
                    outcome.moved.push(Region::Card(index));
                }
            }
            Change::Queue(cards) => {
                board.next = cards.iter().take(BOARD_CARDS).cloned().collect();
                outcome.moved.push(Region::Queue);
            }
            Change::Counts(counts) => {
                board.counts = counts.clone();
                outcome.moved.push(Region::Counts);
            }
            Change::Progress(progress) => {
                board.progress = progress.clone();
                outcome.moved.push(Region::Progress);
            }
            Change::Client { build } => board.build.clone_from(build),
            Change::Other => {}
        }
    }
    board.version = delta.version;
    if board.counts.waiting > waiting {
        let count = board.counts.waiting;
        outcome.notice = Some(Notice {
            title: format!(
                "{count} {} on you",
                if count == 1 {
                    "card waits"
                } else {
                    "cards wait"
                }
            ),
            body: board
                .next
                .first()
                .map(|card| card.title.clone())
                .unwrap_or_default(),
        });
    }
    outcome
}
