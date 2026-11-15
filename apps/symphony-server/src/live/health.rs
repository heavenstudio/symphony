//! What the pages and the project picker say of the board's health: while its callback dispatch or
//! its idle sweep is failing ([`symphony_board::board_health::Recorder::failing`]), every page of
//! the project says so on the server's notice, beside whatever else that notice says, and the
//! picker says so under the project's name. A board that went back below the schema it was seen at
//! ([`symphony_board::board_health::Recorder::went_back`]) is said unwell in those places with that
//! line alone, which names what not to run and where the recovery is. A board that cannot be reached
//! while a start replays its store's log ([`symphony_board::server::replaying_in`]: this server's
//! own, run apart from its reads, or another's) is said to be starting there, with the log's size,
//! in place of the dispatch's words, until a reach finds no replay. While the disk holding the
//! project has less free than `SYMPHONY_LOW_DISK_GB` ([`symphony_board::board_health::disk`]),
//! every page says that too. None of it reads the board. While the project's settings do not load,
//! every page says so and why, as the project is served with the defaults ([`Hub::tell_project`]).

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
    /// The project's settings that did not load ([`project`]).
    project: String,
    /// The board's health ([`notice`]).
    health: String,
    /// The disk running low ([`Low::notice`]).
    disk: String,
    /// The project file as the hub last found it ([`stamp`]): an edit to it renders main's
    /// documents again, whether the settings still load or not ([`Hub::tell_project`]).
    project_file: Stamp,
}

/// A file's inode, size, modification and change times, or none while there is no file: a write,
/// a move or a copy over it changes one of them.
type Stamp = Option<(u64, u64, i64, i64, i64, i64)>;

/// What [`Stamp`] says of the file at `path` now.
fn stamp(path: &Path) -> Stamp {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|file| {
        (
            file.ino(),
            file.size(),
            file.mtime(),
            file.mtime_nsec(),
            file.ctime(),
            file.ctime_nsec(),
        )
    })
}

impl Notices {
    /// Nothing said yet, the project file at `root` as the hub finds it at its start.
    pub(crate) fn of(root: &Path) -> Self {
        Self {
            project_file: stamp(&root.join(symphony::project::FILE)),
            ..Self::default()
        }
    }

    /// The notice the pages show: each part that says something, the update's first.
    fn said(&self) -> String {
        [
            self.update.as_str(),
            self.project.as_str(),
            self.health.as_str(),
            self.disk.as_str(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
    }
}

/// The disk holding `root`, read at each call against `SYMPHONY_LOW_DISK_GB` as this process
/// started with it (a value that is not a number of gigabytes is said once, and the
/// default taken). The library's own tests read a disk that is never low, so no page they check
/// says the machine's.
pub(crate) fn low_disk(root: &Path) -> Box<dyn Fn() -> Option<Low> + Send + Sync> {
    if cfg!(test) {
        return Box::new(|| None);
    }
    let var = symphony_env::LOW_DISK_GB;
    reading(
        root,
        &std::env::vars()
            .filter(|(name, _)| name == var.name)
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

    /// Tell every page, on the server's notice, whether the project's settings load, when that
    /// changes: while they do not, the project is served with the defaults
    /// ([`symphony::project::Project::problem`]). A change asks the loop to read the documents
    /// again, as one a plugin's process makes comes with no file event; once they load, what the
    /// defaults' read left of the plugins' watches is forgotten, so that read warms them as it
    /// would a project's first, with no file they watch changed. An edit to the project file that
    /// still loads (an entrypoint added, a plugin chosen) asks for that read too.
    pub(crate) fn tell_project(&self) {
        let file = stamp(&self.root.join(symphony::project::FILE));
        let said = symphony::project::Project::problem(&self.root)
            .map(|why| project(&why))
            .unwrap_or_default();
        let mut notices = self.notices.lock().unwrap_or_else(PoisonError::into_inner);
        let edited = std::mem::replace(&mut notices.project_file, file) != file;
        if notices.project != said {
            let loads = said.is_empty();
            notices.project = said;
            self.server_notice.send_replace(notices.said());
            drop(notices);
            if loads {
                self.forget_watched(&self.root);
            }
        } else if !edited {
            return;
        } else {
            drop(notices);
        }
        // Its settings render its documents: the read renders them again, as after a lost event.
        self.dirty
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(self.root.clone());
        self.rescan.store(true, std::sync::atomic::Ordering::SeqCst);
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
    /// is read; else that it is starting, while a replay of its store's log keeps it from being
    /// reached ([`Hub::reconnect`]); else whether its dispatch or sweep is failing ([`notice`]).
    fn board_notice(&self, now: f64) -> String {
        if let Some(line) = self.health.rollback() {
            return went_back(&line);
        }
        let replaying = self
            .replaying
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        match replaying {
            Some(line) => starting(&line),
            None => notice(self.health.failing(now)),
        }
    }
}

/// What a page says of a project whose settings do not load, `why` the load's words.
pub(crate) fn project(why: &str) -> String {
    format!("This project's settings do not load, so it is served with the defaults: {why}.")
}

/// What a page says of a board that cannot be reached while a start replays its store's log, `line`
/// the board's words for it ([`symphony_board::server::replaying_in`]).
pub(crate) fn starting(line: &str) -> String {
    format!("The task board is starting: {line}.")
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
