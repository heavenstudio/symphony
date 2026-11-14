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

    /// Whether the project has a board to maintain, as the hub knows it: none until its first
    /// board read is done, then whether its last reach found one ([`Hub::boardless`]) and its
    /// folder still holds it ([`Hub::holds_its_project`]). A board that cannot be reached now is
    /// one, and unwell. The server's maintenance of the project (its idle sweep, its embedding
    /// catch-up) waits on this, so it never makes a board where none is, nor runs in a folder
    /// whose board lookup would lead into another repository.
    pub(crate) fn has_board(&self) -> Option<bool> {
        (*self.initial_board.borrow()).then(|| !self.boardless() && self.holds_its_project())
    }

    /// Whether the folder this hub reads still holds its project: it is there, and the board's
    /// lookup from it, on the files alone ([`symphony_board::project::locate_quick`]), stops where
    /// it stopped when the hub was made, in the same repository. A folder removed, emptied or left
    /// without its `.git` leads that lookup up to whatever checkout holds it, whose board is
    /// another repository's. A folder no repository held then is held while it is there.
    pub(crate) fn holds_its_project(&self) -> bool {
        self.root.is_dir()
            && self.place.as_ref().is_none_or(|(root, common)| {
                symphony_board::project::locate_quick(&self.root)
                    .is_some_and(|now| &now.root == root && &now.common == common)
            })
    }

    /// Why the hub reads no board once its folder no longer holds its project, in words that count
    /// as a project with no board ([`symphony_board::board_health::absent`]).
    pub(crate) fn gone(&self) -> String {
        format!(
            "the project's folder {} is gone, or no longer holds the repository it was served \
             from: {}",
            self.root.display(),
            symphony_board::project::GONE
        )
    }

    /// The hub's folder no longer holds its project ([`Hub::holds_its_project`]): said once, in the
    /// log and to its pages as a project with no board, so its maintenance waits. Its worker then
    /// ends, and reads and records nothing more.
    pub(super) fn lose(&self) {
        let why = self.gone();
        eprintln!("· Symphony: {why}; nothing more is read or recorded for it");
        self.board_error(why);
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
