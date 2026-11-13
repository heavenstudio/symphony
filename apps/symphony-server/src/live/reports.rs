//! The project's hourly roll-up into its day files ([`symphony::reports`]): the
//! machine's one server's alone, as the board's health record is, since a private server's files
//! would write over its. The worker's board tick starts it once in each hour of the hub's clock,
//! the first tick after the hub starts included: the tick makes the roll-up's one board read, with
//! the store it read, and hands it, with the kept scan of the live branches' cards, to a thread of
//! its own, which reads git and the documents and writes the files. That thread is never joined: a
//! stop or a reload waits on none of it, and a roll-up a stop cut short leaves each file whole, as
//! each is written aside and renamed. One roll-up runs at a time; an hour that comes while one runs
//! waits for it. A roll-up that fails, the board unreachable or its read refused, writes nothing
//! and is said once; the files stay as the last one left them, and the next hour tries again.

use super::Hub;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use symphony::reports::HOUR;

/// Where the shared server keeps the project's day files, the hour it last rolled them up in and
/// the roll-up under way.
#[derive(Debug)]
pub(crate) struct Rollup {
    pub(crate) folder: PathBuf,
    /// The hour of the hub's clock (its seconds over an hour, floored) last rolled up or tried.
    tried: Option<i64>,
    /// What went wrong last, so a failure that goes on is said once.
    said: Arc<Mutex<Option<String>>>,
    /// The thread of the last roll-up, never joined but by a test.
    running: Option<JoinHandle<()>>,
}

impl Rollup {
    pub(crate) fn new(folder: PathBuf) -> Rollup {
        Rollup {
            folder,
            tried: None,
            said: Arc::default(),
            running: None,
        }
    }

    /// The hour `now` falls in, when no roll-up has been tried in it.
    fn due(&self, now: f64) -> Option<i64> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "an hour since the epoch is far inside an i64"
        )]
        let hour = (now / HOUR).floor() as i64;
        (self.tried != Some(hour)).then_some(hour)
    }

    /// Wait for the roll-up under way: a test's, which reads the files it wrote.
    #[cfg(test)]
    pub(crate) fn settle(&mut self) {
        if let Some(running) = self.running.take() {
            running.join().unwrap();
        }
    }

    /// Take `running` for the roll-up under way: a test's, which holds one that has not ended.
    #[cfg(test)]
    pub(crate) fn hold(&mut self, running: JoinHandle<()>) {
        self.running = Some(running);
    }

    /// What went wrong last and was said; none once it is right again: a test's.
    #[cfg(test)]
    pub(crate) fn said(&self) -> Option<String> {
        self.said.lock().unwrap().clone()
    }
}

/// Say `what` went wrong, once while it stays the same, on the server's log; `None` once it is
/// right again. Whether it said anything.
pub(super) fn say(said: &Mutex<Option<String>>, what: Option<String>) -> bool {
    let mut said = said.lock().unwrap_or_else(|e| e.into_inner());
    let new = what
        .as_ref()
        .is_some_and(|what| said.as_ref() != Some(what));
    if let Some(what) = what.as_deref().filter(|_| new) {
        crate::front::tell(format_args!("· reports: {what}"));
    }
    *said = what;
    new
}

impl Hub {
    /// The roll-up of this hour, when one is due and this is the shared server, from the board
    /// `board` (none when the tick could not reach it): its read here, the rest on a thread.
    pub(super) fn reports_tick(&self, board: Option<&symphony_board::Store>) {
        let mut rollup = self.reports.lock().unwrap_or_else(|e| e.into_inner());
        let Some(rollup) = rollup.as_mut() else {
            return;
        };
        let now = self.now();
        let Some(hour) = rollup.due(now) else {
            return;
        };
        if rollup.running.as_ref().is_some_and(|r| !r.is_finished()) {
            say(
                &rollup.said,
                Some("the last roll-up has not ended; this hour's waits for it".into()),
            );
            return;
        }
        rollup.tried = Some(hour);
        let days = symphony::reports::due(&rollup.folder, now);
        let read = match board {
            Some(store) => store.report_records(days[0]).map_err(|e| e.to_string()),
            None => Err("the board could not be reached".into()),
        };
        let read = match read {
            Ok(read) => read,
            Err(why) => {
                say(
                    &rollup.said,
                    Some(format!(
                        "no day file written ({why}); the last ones stay, and the next hour \
                         tries again"
                    )),
                );
                return;
            }
        };
        let cards = Arc::clone(&self.branches.lock().unwrap_or_else(|e| e.into_inner()).0);
        let root = self.root.clone();
        let folder = rollup.folder.clone();
        let said = Arc::clone(&rollup.said);
        let started = std::thread::Builder::new()
            .name("reports".into())
            .spawn(move || {
                let project = symphony::project::Project::of(&root).id.clone();
                let rolled =
                    symphony::reports::roll_up(&root, &project, now, &folder, &days, &read, &cards);
                say(
                    &said,
                    match rolled {
                        Ok(rolled) if rolled.problems.is_empty() => None,
                        Ok(rolled) => Some(format!(
                            "{} written without {}",
                            rolled.written.join(" and "),
                            rolled.problems.join("; ")
                        )),
                        Err(why) => Some(format!(
                            "a day file could not be written ({why}); the next hour tries again"
                        )),
                    },
                );
            });
        match started {
            Ok(running) => rollup.running = Some(running),
            Err(e) => {
                say(
                    &rollup.said,
                    Some(format!("the roll-up could not start: {e}")),
                );
            }
        }
    }
}
