//! What the hub tells its pages when the worker's board read or reconnect fails: that the board
//! went, whether it is slow (it took the request and answered late, [`symphony_board::rpc::slow`])
//! or unreachable, and again when a gone board turns from one to the other.

use super::Hub;
use serde_json::json;
use std::path::Path;

impl Hub {
    /// The store for the worker's next board read: `connect`'s, or none once the pages have heard
    /// why there is none. A tick with no store makes no dispatch pass: the board's health counts it
    /// failed. A board refused because it went back below the schema it was seen at
    /// ([`symphony_board::live::seen`]) is said so on the board's health, until a store opens it.
    pub(super) fn reconnect(
        &self,
        connect: &mut impl FnMut(&Path) -> Result<symphony_board::Store, String>,
    ) -> Option<symphony_board::Store> {
        match connect(&self.root) {
            Ok(store) => {
                let now = self.now();
                if self.health.went_back(now, None) {
                    self.tell_health(now);
                }
                Some(store)
            }
            Err(error) => {
                let now = self.now();
                self.health
                    .went_back(now, symphony_board::live::seen::Rollback::line_in(&error));
                self.health.dispatched(
                    now,
                    Err(&format!("no pass, the board could not be reached: {error}")),
                );
                self.tell_health(now);
                self.board_error(error);
                None
            }
        }
    }

    /// A failed board read or reconnect: pages hear that the board went, whether it is slow, and
    /// hear again only when a gone board turns from slow to unreachable or back. Its task counts go
    /// once, when it goes.
    pub(super) fn board_error(&self, error: String) {
        let slow = symphony_board::rpc::slow(&error);
        // A panic under these locks leaves at worst an older board observation, or some pages
        // without one event: each a whole value, and the next board read publishes what changed
        // since, so a poisoned lock is read as it is.
        let mut held = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        let was = held.error.replace(error.clone());
        drop(held);
        let newly_unavailable = was.is_none();
        let turned = was.is_some_and(|was| symphony_board::rpc::slow(&was) != slow);
        if !newly_unavailable && !turned {
            return;
        }
        let status =
            json!({"region":"tasks-status", "data":{"available":false,"slow":slow,"error":error}});
        let mut pages = self.pages.write().unwrap_or_else(|e| e.into_inner());
        for page in pages.values_mut() {
            let mut changes = vec![status.clone()];
            if newly_unavailable {
                page.board.counts.tasks = None;
                changes.push(json!({"region":"counts", "data":page.board.counts}));
            }
            self.publish(page, changes);
        }
    }
}
