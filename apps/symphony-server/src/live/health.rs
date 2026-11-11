//! What the pages and the project picker say of the board's health: while its callback dispatch or
//! its idle sweep is failing ([`symphony_board::board_health::Recorder::failing`]), every page of
//! the project says so on the server's notice, beside whatever else that notice says, and the
//! picker says so under the project's name. None of it reads the board.

use super::Hub;
use std::sync::PoisonError;

/// What the server's notice says, by where it comes from.
#[derive(Debug, Default)]
pub(crate) struct Notices {
    /// A failed update of the server ([`crate::reload`]).
    update: String,
    /// The board's health ([`notice`]).
    health: String,
}

impl Notices {
    /// The notice the pages show: each part that says something, the update's first.
    fn said(&self) -> String {
        [self.update.as_str(), self.health.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

impl Hub {
    /// Tell every page, on the server's notice, whether the board's dispatch or sweep is failing
    /// at `now`, when that changes.
    pub(crate) fn tell_health(&self, now: f64) {
        let health = notice(self.health.failing(now));
        let mut notices = self.notices.lock().unwrap_or_else(PoisonError::into_inner);
        if notices.health != health {
            notices.health = health;
            self.server_notice.send_replace(notices.said());
        }
    }

    /// Tell every page, on the server's notice, that the server's update failed, `message`
    /// saying how, beside the board's health.
    pub(crate) fn tell_update(&self, message: String) {
        let mut notices = self.notices.lock().unwrap_or_else(PoisonError::into_inner);
        notices.update = message;
        self.server_notice.send_replace(notices.said());
    }

    /// What the project picker says of this project's board at `now`: nothing while it is well.
    pub(crate) fn health_notice(&self, now: f64) -> String {
        notice(self.health.failing(now))
    }
}

/// What a page says of a board whose dispatch, sweep, or both, are failing; nothing while
/// neither is.
pub(crate) fn notice([dispatch, sweep]: [bool; 2]) -> String {
    let failing = match (dispatch, sweep) {
        (true, true) => {
            "its callback dispatch and its idle sweep are failing, so no agent hears from it"
        }
        (true, false) => "its callback dispatch is failing, so no agent hears from it",
        (false, true) => "its idle sweep is failing",
        (false, false) => return String::new(),
    };
    format!("The task board is unwell: {failing}. `just tasks status` says why.")
}
