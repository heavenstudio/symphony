//! The project's hourly roll-up into its day files ([`symphony::reports`]), from one read of the
//! board, two cheap reads of git and the documents: the machine's one server's alone, as the
//! board's health record is, since a private server's files would write over its. The worker's
//! board tick starts it once in each hour of the hub's clock, the first tick after the hub starts
//! included, with the store that tick read. A roll-up that fails, the board
//! unreachable or its read refused, writes nothing and is said once; the files stay as the last one
//! left them, and the next hour tries again.

use super::Hub;
use std::path::PathBuf;
use symphony::reports::HOUR;

/// Where the shared server keeps the project's day files, and the hour it last rolled them up in.
#[derive(Debug)]
pub(crate) struct Rollup {
    pub(crate) folder: PathBuf,
    /// The hour of the hub's clock (its seconds over an hour, floored) last rolled up or tried.
    tried: Option<i64>,
    /// What the last roll-up said went wrong, so a failure that goes on is said once.
    said: Option<String>,
}

impl Rollup {
    pub(crate) fn new(folder: PathBuf) -> Rollup {
        Rollup {
            folder,
            tried: None,
            said: None,
        }
    }

    /// Whether a roll-up is due at `now`: none has been tried in its hour.
    fn due(&mut self, now: f64) -> bool {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "an hour since the epoch is far inside an i64"
        )]
        let hour = (now / HOUR).floor() as i64;
        let due = self.tried != Some(hour);
        self.tried = Some(hour);
        due
    }

    /// Say `what` went wrong, once while it stays the same.
    fn say(&mut self, what: Option<String>) {
        if let Some(what) = &what
            && self.said.as_ref() != Some(what)
        {
            eprintln!("· reports: {what}");
        }
        self.said = what;
    }
}

impl Hub {
    /// The roll-up of this hour, when one is due and this is the shared server, from the board
    /// `board` (none when the tick could not reach it).
    pub(super) fn reports_tick(&self, board: Option<&symphony_board::Store>) {
        let mut rollup = self.reports.lock().unwrap_or_else(|e| e.into_inner());
        let Some(rollup) = rollup.as_mut() else {
            return;
        };
        let now = self.now();
        if !rollup.due(now) {
            return;
        }
        let project = symphony::project::Project::of(&self.root).id.clone();
        let rolled = match board {
            Some(store) => {
                symphony::reports::roll_up(&self.root, &project, now, &rollup.folder, |since| {
                    store.report_records(since).map_err(|e| e.to_string())
                })
            }
            None => Err("the board could not be reached".into()),
        };
        match rolled {
            Ok(rolled) if rolled.problems.is_empty() => rollup.say(None),
            Ok(rolled) => rollup.say(Some(format!(
                "{} written without {}",
                rolled.written.join(" and "),
                rolled.problems.join("; ")
            ))),
            Err(why) => rollup.say(Some(format!(
                "no day file written ({why}); the last ones stay, and the next hour tries again"
            ))),
        }
    }
}
