//! What the hub tells its pages when the worker's board read or reconnect fails: that the board
//! went, whether it is slow (it took the request and answered late, [`symphony_board::rpc::slow`])
//! or unreachable, and again when a gone board turns from one to the other.

use super::Hub;
use serde_json::json;
use std::path::Path;
use symphony_board::lock::Recovered as _;

impl Hub {
    /// One due worker attempt, including opening a missing store. A failed read drops that store
    /// but keeps this operation's streak across reopening; startup and recurring ticks share it.
    /// One refused while the board is held is due at the first tick after its release
    /// ([`super::retry`]).
    pub(super) fn board_tick(
        &self,
        board: &mut Option<symphony_board::Store>,
        connect: &mut impl FnMut(&Path) -> Result<symphony_board::Store, String>,
    ) {
        self.board_retry.lock().recover().run_unless_released(
            &self.retry_clock,
            &|| self.board_held(),
            || {
                if board.is_none() {
                    *board = self.reconnect(connect);
                }
                let store = board.as_ref().ok_or(())?;
                if let Err(error) = super::measured("board tick", || self.board_read(store)) {
                    self.board_error(error);
                    *board = None;
                    return Err(());
                }
                super::measured("people tick", || self.people_read(store));
                // The board moved, or may have, and its people are looked at: the notices look
                // again, by that look, so only after it ([`super::notices`]). A tick that fails
                // wakes none, and leaves the look failed ([`Hub::board_error`]).
                self.wake_notices();
                Ok(())
            },
        );
    }

    /// The store for the worker's next board read: `connect`'s, or none once the pages have heard
    /// why there is none. An opening that fails makes no dispatch pass: the board's health counts it
    /// failed, unless the project has no board ([`symphony_board::board_health::absent`]), which is
    /// not unwell. A board refused because it went back below the schema it was seen at
    /// ([`symphony_board::live::seen`]) is said so on the board's health, until a store opens it.
    pub(crate) fn reconnect(
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
                let changed = self
                    .health
                    .went_back(now, symphony_board::live::seen::Rollback::line_in(&error));
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

    /// Whether the board's database is held stopped now ([`symphony_board::server::Home::held`]),
    /// read from the hold's file in the repository this hub is held to, as the reach that refuses
    /// a held board reads it: no board is reached or started. Never for a folder no repository held.
    pub(super) fn board_held(&self) -> bool {
        self.repository.as_ref().is_some_and(|common| {
            symphony_board::server::Home::in_common(common)
                .held()
                .is_some()
        })
    }

    /// Whether the project has no board: the last reach of it said none is made yet, or that the
    /// project's folder is gone ([`symphony_board::board_health::absent`]). A board that is there
    /// and cannot be served, one still on SurrealDB among them, is a board, and unwell. Read as
    /// [`Hub::tasks_read`] answers, it never waits on a board read under way.
    pub(crate) fn boardless(&self) -> bool {
        self.tasks_read()
            .err()
            .as_deref()
            .is_some_and(symphony_board::board_health::absent)
    }

    /// Whether the project has a board to maintain, as the hub knows it: none until its first
    /// board read is done, then whether its last reach found one ([`Hub::boardless`]) and its
    /// folder still holds it ([`Hub::holds_its_project`]). A board that cannot be reached now is
    /// one, and unwell. The server's maintenance of the project (its idle sweep) waits on this, so it never makes a board where none is, nor runs in a folder
    /// whose board lookup would lead into another repository.
    pub(crate) fn has_board(&self) -> Option<bool> {
        (*self.initial_board.borrow()).then(|| !self.boardless() && self.holds_its_project())
    }

    /// Whether the folder this hub reads still holds its project: it is there, and the board's
    /// lookup from it, on the files alone ([`symphony_board::project::locate_quick`]), finds the
    /// repository it found when the hub was made, whose board is the project's, and that is a
    /// repository still ([`symphony_board::project::is_repository`]). A folder removed, emptied or
    /// left without its `.git` leads that lookup up to whatever checkout holds it, whose board is
    /// another repository's; a `.git` a board's start under way made again holds only the board's
    /// folders. A folder no repository held then is held while it is there. Every reach of its
    /// board from the folder in this process, the hub's and any route's after this check, is held to
    /// the same repository ([`taskview::serve`]), so a removal after this check leads none to
    /// another board.
    ///
    /// [`taskview::serve`]: symphony::taskview::serve
    pub(crate) fn holds_its_project(&self) -> bool {
        self.root.is_dir()
            && self.repository.as_ref().is_none_or(|repository| {
                symphony_board::project::locate_quick(&self.root)
                    .is_some_and(|now| &now.common == repository)
                    && symphony_board::project::is_repository(repository)
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
    /// once, when it goes. The notices tell by no look until a tick reads the board's people again
    /// ([`Hub::people_unread`]).
    pub(super) fn board_error(&self, error: String) {
        self.people_unread(error.clone());
        let slow = symphony_board::rpc::slow(&error);
        // A panic under these locks leaves at worst an older board observation, or some pages
        // without one event: each a whole value, and the next board read publishes what changed
        // since, so a poisoned lock is read as it is.
        let mut held = self.tasks.lock().recover();
        let was = held.error.replace(error.clone());
        self.publish_listing(Err(error.clone()));
        drop(held);
        let newly_unavailable = was.is_none();
        let turned = was.is_some_and(|was| symphony_board::rpc::slow(&was) != slow);
        if !newly_unavailable && !turned {
            return;
        }
        let status =
            json!({"region":"tasks-status", "data":{"available":false,"slow":slow,"error":error}});
        let mut pages = self.pages.write().recover();
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
