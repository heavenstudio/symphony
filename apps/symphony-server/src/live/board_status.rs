//! What the hub tells its pages when the worker's board read or reconnect fails: that the board
//! went, whether it is slow (it took the request and answered late, [`symphony_board::rpc::slow`])
//! or unreachable, and again when a gone board turns from one to the other.

use super::Hub;
use serde_json::json;
use std::path::Path;

impl Hub {
    /// The store for the worker's next board read: `connect`'s, or none once the pages have heard
    /// why there is none. A tick with no store makes no dispatch pass: the board's health counts it
    /// failed, unless the project has no board ([`symphony_board::board_health::absent`]), which is
    /// not unwell. A board refused because it went back below the schema it was seen at
    /// ([`symphony_board::live::seen`]) is said so on the board's health, until a store opens it;
    /// one refused while a start replays its store's log, this server's own run apart from this
    /// thread or another's ([`symphony_board::server::replaying_in`]), is said to be starting,
    /// until a reach finds no replay.
    pub(crate) fn reconnect(
        &self,
        connect: &mut impl FnMut(&Path) -> Result<symphony_board::Store, String>,
    ) -> Option<symphony_board::Store> {
        match connect(&self.root) {
            Ok(store) => {
                let now = self.now();
                let replayed = self.replaying_now(None);
                if self.health.went_back(now, None) || replayed {
                    self.tell_health(now);
                }
                Some(store)
            }
            Err(error) => {
                let now = self.now();
                let replaying = self.replaying_now(symphony_board::server::replaying_in(&error));
                let changed = self
                    .health
                    .went_back(now, symphony_board::live::seen::Rollback::line_in(&error))
                    || replaying;
                if !symphony_board::board_health::absent(&error) {
                    self.health.dispatched(
                        now,
                        Err(&format!("no pass, the board could not be reached: {error}")),
                    );
                    self.tell_health(now);
                } else if changed {
                    self.tell_health(now);
                }
                self.board_error(error);
                None
            }
        }
    }

    /// Keep `line`, what the pages say of a replay under way, or none: whether it changed.
    fn replaying_now(&self, line: Option<String>) -> bool {
        let mut held = self
            .replaying
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let changed = *held != line;
        *held = line;
        changed
    }

    /// Whether the project has no board: the last reach of it said none is made, or that it has no
    /// store and no SurrealDB here to serve one ([`symphony_board::board_health::absent`]). A store
    /// that is there and cannot be served is a board, and unwell.
    pub(crate) fn boardless(&self) -> bool {
        // A panic under this lock leaves at worst an older board observation, read as it is.
        let held = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        held.error
            .as_deref()
            .is_some_and(symphony_board::board_health::absent)
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
