//! The pure half of what the chat's panel makes of what the page heard: its activity toasts, and
//! what the asks' readers read again on. Every page follows the feed in the web client
//! (apps/symphony-web), which tells the panel of the asks as it hears them.

use symphony_api::ActivityItem;

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
