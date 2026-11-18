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
//! While the board is at a schema ahead of this build's, which it reads and never writes, every
//! page says so ([`Hub::read_ahead`]); while this project holds the machine's server on a
//! project's build, the one server tells its pages so ([`Hub::tell_machine`]).
//!
//! The project picker and `/api/projects` read one judgement of the board ([`Hub::standing`]): red
//! while the pages say it is unwell, amber while it is slow over the hour or work waits to start
//! that no live orchestrator staffs, green otherwise, with the line saying why ([`Standing`]).

use super::Hub;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::PoisonError;
use symphony_board::board_health::disk::{self, Low};
use symphony_board::board_health::{Report, SLOW_MS};

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
    /// What the machine's server is to this project ([`Hub::tell_machine`]).
    machine: String,
    /// The board's schema while it is ahead of this build's, and what the pages say of it
    /// ([`Hub::read_ahead`]).
    ahead: (Option<i64>, String),
    /// The project file and its local settings file as the hub last found them ([`stamps`]): an
    /// edit to either renders main's documents again, whether the settings still load or not
    /// ([`Hub::tell_project`]).
    project_file: [Stamp; 2],
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

/// The project file at `root` and its local settings file, each as [`stamp`] finds it now.
fn stamps(root: &Path) -> [Stamp; 2] {
    [
        stamp(&root.join(symphony::project::FILE)),
        stamp(&symphony::project::Project::local_file(root)),
    ]
}

impl Notices {
    /// Nothing said yet, the project file at `root` as the hub finds it at its start.
    pub(crate) fn of(root: &Path) -> Self {
        Self {
            project_file: stamps(root),
            ..Self::default()
        }
    }

    /// The notice the pages show: each part that says something, the update's first.
    fn said(&self) -> String {
        [
            self.update.as_str(),
            self.project.as_str(),
            self.machine.as_str(),
            self.ahead.1.as_str(),
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
    let mut vars = BTreeMap::new();
    for (name, value) in std::env::vars().filter(|(name, _)| name == var.name) {
        vars.insert(name, value);
    }
    reading(root, &vars)
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
        let file = stamps(&self.root);
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
        self.ask_rescan();
    }

    /// Tell every page, on the server's notice, that the server's update failed, `message`
    /// saying how, beside the board's health.
    pub(crate) fn tell_update(&self, message: String) {
        let mut notices = self.notices.lock().unwrap_or_else(PoisonError::into_inner);
        notices.update = message;
        self.server_notice.send_replace(notices.said());
    }

    /// Tell every page, on the server's notice, what the machine's server is to this project when
    /// that changes: `said`, nothing while there is nothing to say ([`crate::projects`]).
    pub(crate) fn tell_machine(&self, said: String) {
        let mut notices = self.notices.lock().unwrap_or_else(PoisonError::into_inner);
        if notices.machine != said {
            notices.machine = said;
            self.server_notice.send_replace(notices.said());
        }
    }

    /// The board's schema when it is ahead of this build's, read on `store` once a tick: the hub
    /// then reads it and writes nothing to it, as the schema rule refuses a reader's every write
    /// on it ([`symphony_board::Store`]), and every page says so, until it is not. A read that
    /// fails keeps what the last one found.
    pub(crate) fn read_ahead(&self, store: &symphony_board::Store) -> Option<i64> {
        let found = store.board_version();
        let mut notices = self.notices.lock().unwrap_or_else(PoisonError::into_inner);
        let Ok(found) = found else {
            return notices.ahead.0;
        };
        let ahead = found.filter(|found| *found > symphony_board::SCHEMA);
        if notices.ahead.0 != ahead {
            notices.ahead = (ahead, ahead.map(read_only).unwrap_or_default());
            self.server_notice.send_replace(notices.said());
        }
        ahead
    }

    /// What the pages say of this project's board now: nothing while it is well.
    pub(crate) fn health_notice(&self) -> String {
        self.board_notice(self.now())
    }

    /// How this project's board stands now, as the project picker draws it and `/api/projects`
    /// says it: red, in the pages' words, while they say it is unwell ([`Hub::health_notice`]);
    /// else amber or green by the hour the recorder counted, as `/api/health` answers it, and the
    /// last board read ([`judged`]). It reads no board.
    pub(crate) fn standing(&self) -> Standing {
        let unwell = self.health_notice();
        if !unwell.is_empty() {
            return Standing {
                signal: Signal::Red,
                line: unwell,
            };
        }
        let tasks = self.tasks_read().ok();
        judged(&self.health.report(self.now()), tasks.as_deref())
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

/// How a project's board stands ([`Hub::standing`]): its worst signal, the dot before its name on
/// the project picker, and the line under its counts saying why, empty while it is well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Standing {
    pub signal: Signal,
    pub line: String,
}

impl Standing {
    /// Its line while the board is not well; none while it is.
    pub(crate) fn unwell(&self) -> Option<String> {
        (self.signal != Signal::Green).then(|| self.line.clone())
    }
}

/// A board's worst signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Signal {
    /// Well.
    Green,
    /// Slow over the hour ([`slow`]), or work waiting to start that no live orchestrator staffs
    /// ([`unstaffed`]).
    Amber,
    /// Unwell, as its pages say: its dispatch or sweep failing, gone back, or starting.
    Red,
}

impl Signal {
    /// Its name, the class that gives its colour on the picker.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Signal::Green => "green",
            Signal::Amber => "amber",
            Signal::Red => "red",
        }
    }

    /// What its colour says, for a reader who does not see it.
    pub(crate) fn said(self) -> &'static str {
        match self {
            Signal::Green => "well",
            Signal::Amber => "slow or unstaffed",
            Signal::Red => "failing",
        }
    }
}

/// How a board its pages do not call unwell stands, by `report`, the hour its recorder counted,
/// and `tasks`, its last read, if any: amber while it is slow ([`slow`]) or unstaffed
/// ([`unstaffed`]), the parts that say so joined by ` · `; green with neither.
pub(crate) fn judged(report: &Report, tasks: Option<&symphony_api::Tasks>) -> Standing {
    let parts: Vec<String> = [slow(report), tasks.and_then(unstaffed).map(String::from)]
        .into_iter()
        .flatten()
        .collect();
    Standing {
        signal: if parts.is_empty() {
            Signal::Green
        } else {
            Signal::Amber
        },
        line: parts.join(" · "),
    }
}

/// That the board was slow over the hour `report` counts, with the figures that say it: its writes'
/// p90 at [`SLOW_MS`] (a second) or more, in whole seconds, and the requests that went unanswered,
/// when any did: `Board slow (writes 3 s, 48 unanswered this hour)`. None while neither.
pub(crate) fn slow(report: &Report) -> Option<String> {
    let writes = (report.writes.p90_ms)
        .filter(|p90| *p90 >= SLOW_MS)
        .map(|p90| format!("writes {:.0} s", (p90 / 1000.0).round()));
    let failed = report.queries.failed;
    let unanswered = (failed > 0).then(|| format!("{failed} unanswered"));
    let figures: Vec<String> = [writes, unanswered].into_iter().flatten().collect();
    (!figures.is_empty()).then(|| format!("Board slow ({} this hour)", figures.join(", ")))
}

/// What staffing lacks while work waits to start (`tasks.unstarted`) and no orchestrator of an
/// open task is live, by the board's one rule for a session that is there ([`Health::holds`]):
/// `orchestrator stale` while one is still there, stale or its presence not yet swept,
/// `no orchestrator` when none is. None while nothing waits to start or one is live.
///
/// [`Health::holds`]: symphony_board::Health::holds
pub(crate) fn unstaffed(tasks: &symphony_api::Tasks) -> Option<&'static str> {
    use symphony_board::Health;
    let any = |healths: &[Health]| {
        (tasks.orchestrators.iter())
            .any(|one| healths.iter().any(|health| one.health == health.as_str()))
    };
    if tasks.unstarted == 0 || any(&[Health::Live]) {
        return None;
    }
    Some(if any(&[Health::Stale, Health::Unknown]) {
        "orchestrator stale"
    } else {
        "no orchestrator"
    })
}

/// What a page says of a board at `schema`, ahead of this build's, which it reads and never writes:
/// and what that does to the owner's words, which are saved in the documents while the asks and the
/// chat they open wait in the journal ([`symphony::callbacks`]).
pub(crate) fn read_only(schema: i64) -> String {
    format!(
        "The task board is at schema {schema}, ahead of this Symphony's {}: it is read, and nothing \
         is written to it from here, until a Symphony at schema {schema} serves it. A verdict or a \
         message saved now is kept, and reaches no agent until then.",
        symphony_board::SCHEMA
    )
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
