//! The hold ([`Home::hold`]): the board's database stopped and kept stopped while someone works on
//! its data, a snapshot, a replay on a clone or a swap. While `sf-tasks/hold` is there nothing of
//! the board starts a server on that data: [`Home::ensure`], [`Home::stop`], [`Home::reclaim`] and
//! every start refuse with [`ServerError::Held`], which names who holds it, since when and why, and
//! the command that releases it ([`Home::release`]). Only builds that carry it honour it.

use super::{DATA, Home, LOG, Running, ServerError, Waited, io};
use crate::sys::local_minute;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// In the board's folder: the hold, while it is held.
pub(super) const HOLD: &str = "hold";

/// Who holds the board's database stopped, and why: what `sf-tasks/hold` says.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hold {
    /// The handle that set it.
    pub by: String,
    /// Why, in its setter's words.
    pub why: String,
    /// When it was set, in seconds since the epoch.
    pub at: i64,
    /// The process that set it.
    pub pid: i64,
}

impl fmt::Display for Hold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[allow(clippy::cast_precision_loss)] // a time in seconds is far inside f64's integers
        let since = local_minute(self.at as f64);
        write!(f, "held by {} since {since}: {}", self.by, self.why)
    }
}

/// What [`Home::hold`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct Holding {
    /// The hold, as `sf-tasks/hold` says it.
    pub hold: Hold,
    /// Each server it stopped: the recorded one, then one on the data no record named.
    pub stopped: Vec<Running>,
    /// The data, which nothing serves now and nothing starts a server on until the release.
    pub data: PathBuf,
}

impl Home {
    /// The hold on this board's database, if it is held. A hold file that cannot be read still
    /// holds: its setter is unknown and its reason is that it does not read.
    pub fn held(&self) -> Option<Hold> {
        let path = self.dir.join(HOLD);
        let unreadable = |problem: &dyn fmt::Display| Hold {
            by: "an unknown setter".into(),
            why: format!(
                "{} is there and does not read as a hold ({problem})",
                path.display()
            ),
            at: modified(&path),
            pid: 0,
        };
        match fs::read_to_string(&path) {
            Ok(text) => Some(serde_json::from_str(&text).unwrap_or_else(|e| unreadable(&e))),
            Err(problem) if problem.kind() == std::io::ErrorKind::NotFound => None,
            Err(problem) => Some(unreadable(&problem)),
        }
    }

    /// Hold this board's database stopped, as `by`, for `why`: under the starters' lock, write the
    /// hold, then stop cleanly every server on the data ([`Home::stop_serving`]) and forget its
    /// address. The hold is written first, so a stop left waiting (a server still replaying its
    /// log, or still closing its store) leaves no one starting another meanwhile; `by` holding it
    /// again finishes that stop. Refused while another holds it.
    pub fn hold(&self, by: &str, why: &str) -> Result<Holding, ServerError> {
        let _locked = self.lock()?;
        let hold = match self.held() {
            Some(hold) if hold.by == by => hold,
            Some(hold) => return Err(ServerError::Held(hold)),
            None => {
                let hold = Hold {
                    by: by.into(),
                    why: why.into(),
                    at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |since| {
                            i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
                        }),
                    pid: i64::from(std::process::id()),
                };
                self.write_hold(&hold)?;
                hold
            }
        };
        let stopped = self.stop_serving()?;
        Ok(Holding {
            hold,
            stopped,
            data: self.dir.join(DATA),
        })
    }

    /// Release the hold, if there is one: what it was. It starts nothing; the board's next command
    /// starts the database.
    pub fn release(&self) -> Result<Option<Hold>, ServerError> {
        let _locked = self.lock()?;
        let hold = self.held();
        let path = self.dir.join(HOLD);
        if hold.is_some() {
            fs::remove_file(&path).map_err(io("could not remove", &path))?;
        }
        Ok(hold)
    }

    /// The server that runs on this board's data, if one does: the recorded one, else one no
    /// record names. Asks it nothing; under a hold, one still closing its store, or still starting.
    pub fn serving(&self) -> Option<Running> {
        self.recorded()
            .filter(|running| self.serves(running.pid))
            .or_else(|| self.unrecorded())
    }

    /// Refused while held.
    pub(super) fn unheld(&self) -> Result<(), ServerError> {
        self.held()
            .map_or(Ok(()), |hold| Err(ServerError::Held(hold)))
    }

    /// Every server on this board's data stopped cleanly, the recorded one first, then one no
    /// record names, and the record of each removed once it has ended: those it stopped. A server
    /// that has not answered yet is still starting (replaying its log) and is waited for, never
    /// signalled: a signal before its handlers are set ends it at once, as a kill would, and a store
    /// cut off in its replay can lose what that replay had not yet written. One that does not answer
    /// within the patience is left running and the stop refused, naming it. Under the lock.
    pub(super) fn stop_serving(&self) -> Result<Vec<Running>, ServerError> {
        let mut stopped = Vec::new();
        if let Some(running) = self.recorded() {
            let serving = self.serves(running.pid);
            if serving {
                self.stop_one(&running)?;
            }
            self.forget()?;
            if serving {
                stopped.push(running);
            }
        }
        if let Some(running) = self.unrecorded() {
            self.stop_one(&running)?;
            stopped.push(running);
        }
        Ok(stopped)
    }

    /// `running`, a server on this board's data, stopped cleanly once it answers ([`Home::end`]);
    /// one that does not answer within the patience is left as it is, and said.
    pub(super) fn stop_one(&self, running: &Running) -> Result<(), ServerError> {
        match self.answers(running)? {
            Waited::Ended => Ok(()),
            Waited::Answers => self.end(running.pid),
            Waited::Silent => Err(ServerError::NotUp(format!(
                "the board's server (pid {}, {}) has not answered within {} s, so it is still \
                 starting (replaying its log) or stuck: it is left running, never signalled before \
                 it answers; its log is {}",
                running.pid,
                running.addr,
                self.patience.as_secs(),
                self.dir.join(LOG).display()
            ))),
        }
    }

    /// Write the hold whole, so a reader never sees half of it.
    fn write_hold(&self, hold: &Hold) -> Result<(), ServerError> {
        let path = self.dir.join(HOLD);
        let next = self.dir.join(format!("{HOLD}.{}", std::process::id()));
        let text = serde_json::to_string(hold).expect("a hold serialises");
        fs::write(&next, text).map_err(io("could not write", &next))?;
        fs::rename(&next, &path).map_err(io("could not write", &path))
    }
}

/// When `path` was last written, in seconds since the epoch; 0 when that cannot be read.
fn modified(path: &std::path::Path) -> i64 {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| {
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
        })
}
