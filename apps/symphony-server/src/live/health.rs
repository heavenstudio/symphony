//! What the pages and the project picker say of the board's health: while its callback dispatch or
//! its idle sweep is failing ([`symphony_board::board_health::Recorder::failing`]), every page of
//! the project says so on the server's notice, beside whatever else that notice says, and the
//! picker says so under the project's name. A board that went back below the schema it was seen at
//! ([`symphony_board::board_health::Recorder::went_back`]) is said unwell in those places with that
//! line alone, which names what not to run and where the recovery is. While the disk holding the
//! project has less free than `SF_LOW_DISK_GB` ([`symphony_board::board_health::disk`]), every page
//! says that too. None of it reads the board.

use super::Hub;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::PoisonError;
use symphony_board::board_health::disk::{self, Low};

/// What the server's notice says, by where it comes from.
#[derive(Debug, Default)]
pub(crate) struct Notices {
    /// A failed update of the server ([`crate::reload`]).
    update: String,
    /// The board's health ([`notice`]).
    health: String,
    /// The disk running low ([`Low::notice`]).
    disk: String,
}

impl Notices {
    /// The notice the pages show: each part that says something, the update's first.
    fn said(&self) -> String {
        [
            self.update.as_str(),
            self.health.as_str(),
            self.disk.as_str(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
    }
}

/// The disk holding `root`, read at each call against `SF_LOW_DISK_GB` as this process started with
/// it (a value that is not a number of gigabytes is said once, and the default taken). The
/// library's own tests read a disk that is never low, so no page they check says the machine's.
pub(crate) fn low_disk(root: &Path) -> Box<dyn Fn() -> Option<Low> + Send + Sync> {
    if cfg!(test) {
        return Box::new(|| None);
    }
    reading(
        root,
        &std::env::vars()
            .filter(|(name, _)| name == disk::VAR)
            .collect(),
    )
}

/// The disk holding `root`, read at each call against the threshold `vars` names.
pub(crate) fn reading(
    root: &Path,
    vars: &BTreeMap<String, String>,
) -> Box<dyn Fn() -> Option<Low> + Send + Sync> {
    let threshold = disk::threshold(vars).unwrap_or_else(|why| {
        eprintln!(
            "· {why}: the pages warn under {} GB free",
            disk::THRESHOLD_GB
        );
        disk::THRESHOLD_GB
    });
    let root = root.to_path_buf();
    Box::new(move || Low::check(&root, threshold))
}

impl Hub {
    /// The time on the hub's clock, in seconds since the epoch.
    pub(crate) fn now(&self) -> f64 {
        (self.clock)()
    }

    /// Tell every page, on the server's notice, whether the board went back, or its dispatch or
    /// sweep is failing at `now`, and whether the disk is low, when either changes.
    pub(crate) fn tell_health(&self, now: f64) {
        let health = self.board_notice(now);
        let disk = (self.low_disk)()
            .map(|low| low.notice())
            .unwrap_or_default();
        let mut notices = self.notices.lock().unwrap_or_else(PoisonError::into_inner);
        if notices.health != health || notices.disk != disk {
            notices.health = health;
            notices.disk = disk;
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

    /// What the project picker says of this project's board now: nothing while it is well.
    pub(crate) fn health_notice(&self) -> String {
        self.board_notice(self.now())
    }

    /// What a page says of the board at `now`: that it went back, which is why nothing else of it
    /// is read, else whether its dispatch or sweep is failing ([`notice`]).
    fn board_notice(&self, now: f64) -> String {
        match self.health.rollback() {
            Some(line) => went_back(&line),
            None => notice(self.health.failing(now)),
        }
    }
}

/// What a page says of a board that went back below the schema it was seen at, `line` the
/// board's health's words for it ([`symphony_board::live::seen::Rollback::line_in`]).
pub(crate) fn went_back(line: &str) -> String {
    format!("The task board is unwell: {line}.")
}

/// What a page says of a board whose dispatch, sweep, or both, are failing; nothing while
/// neither is. A board failing every other tick still fails some agents' callbacks.
pub(crate) fn notice([dispatch, sweep]: [bool; 2]) -> String {
    let failing = match (dispatch, sweep) {
        (true, true) => {
            "its callback dispatch and its idle sweep are failing, so agents may not hear from it"
        }
        (true, false) => "its callback dispatch is failing, so agents may not hear from it",
        (false, true) => "its idle sweep is failing",
        (false, false) => return String::new(),
    };
    format!("The task board is unwell: {failing}. Run “just tasks status” to see why.")
}
