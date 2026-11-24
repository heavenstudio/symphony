//! The hold: the board's database stopped and kept stopped while someone works on its data, a
//! snapshot, a restore or a swap ([`super::Postgres::hold`]). While `symphony/tasks/hold` is there
//! nothing of the board starts a server on that data: every start, stop, snapshot and reclaim
//! refuses with [`ServerError::Held`], which names who holds it, since when and why, and the
//! command that releases it ([`Home::release`]). Only builds that carry it honour it.
//!
//! Its holder releases it, or another handle that names the holder: a release by mistake in the
//! middle of someone's work on the data (a restart between a stop and a rename) is what the hold
//! is there to stop. A hold that does not read names no holder, and any handle releases it. No
//! role decides it (an orchestrator's, say): the board, which holds the roles, is not read while it
//! is held. Each hold and release is a line of `symphony/tasks/hold.log`, which nothing removes.

use super::{Doing, Home, ServerError, io};
use crate::sys::local_minute;
use std::fmt;
use std::fs;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

/// In the board's folder: the hold, while it is held.
pub(super) const HOLD: &str = "hold";
/// In the board's folder: every hold and release, a JSON line each.
pub(super) const HISTORY: &str = "hold.log";
/// The holder a hold that does not read names.
const UNKNOWN: &str = "an unknown setter";

/// Who holds the board's database stopped, and why: what `symphony/tasks/hold` says.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hold {
    /// The handle that set it.
    pub by: String,
    /// Why, in its setter's words.
    pub why: String,
    /// When it was set, in seconds since the epoch.
    pub at: i64,
    /// The process that set it: the command, which ends once its stop is done.
    pub pid: i64,
}

impl fmt::Display for Hold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        #[allow(clippy::cast_precision_loss)] // a time in seconds is far inside f64's integers
        let since = local_minute(self.at as f64);
        write!(f, "held by {} since {since}: {}", self.by, self.why)
    }
}

impl Home {
    /// The hold on this board's database, if it is held. A hold file that cannot be read still
    /// holds: its setter is unknown and its reason is that it does not read.
    pub fn held(&self) -> Option<Hold> {
        let path = self.dir.join(HOLD);
        let unreadable = |problem: &dyn fmt::Display| Hold {
            by: UNKNOWN.into(),
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

    /// Record a hold, as `by`, for `why`, under the caller's starters' lock, before its server is
    /// stopped, so a stop left waiting leaves no one starting another meanwhile: one `by` set
    /// already is kept as it is, and another's refuses. The hold is a line of the history before
    /// it is written.
    pub(super) fn hold_as(&self, by: &str, why: &str) -> Result<Hold, ServerError> {
        let hold = match self.held() {
            Some(hold) if hold.by == by => hold,
            Some(hold) => return Err(ServerError::Held(hold)),
            None => {
                let hold = Hold {
                    by: by.into(),
                    why: why.into(),
                    at: seconds_now(),
                    pid: i64::from(std::process::id()),
                };
                self.history(&serde_json::json!({
                    "did": "hold", "by": by, "why": why, "at": hold.at,
                }))?;
                self.write_hold(&hold)?;
                hold
            }
        };
        Ok(hold)
    }

    /// Release the hold, if there is one, as `by`: what it was. Only its holder releases it, or
    /// another handle whose `of` names the holder; any handle a hold that does not read, which
    /// names none. The release is a line of the history before the hold goes. It starts nothing;
    /// the board's next command starts the database.
    pub fn release(&self, by: &str, of: Option<&str>) -> Result<Option<Hold>, ServerError> {
        let _locked = self.lock(Doing::Releasing)?;
        self.release_as(by, of)
    }

    /// [`Home::release`] under the caller's starters' lock.
    pub(super) fn release_as(
        &self,
        by: &str,
        of: Option<&str>,
    ) -> Result<Option<Hold>, ServerError> {
        // A switch that stopped part way keeps its hold until its holder settles it.
        let journal = self.dir.join(super::moving::JOURNAL);
        if journal.exists() {
            return Err(ServerError::Io(super::moving::unsettled(&journal)));
        }
        let Some(hold) = self.held() else {
            return Ok(None);
        };
        if hold.by != by && hold.by != UNKNOWN && of != Some(hold.by.as_str()) {
            return Err(ServerError::NotHolder(hold));
        }
        self.history(&serde_json::json!({
            "did": "release", "by": by, "of": hold.by, "why": hold.why, "held_at": hold.at,
            "at": seconds_now(),
        }))?;
        let path = self.dir.join(HOLD);
        fs::remove_file(&path).map_err(io("could not remove", &path))?;
        Ok(Some(hold))
    }

    /// `line` appended to the hold's history, whole, as one line.
    fn history(&self, line: &serde_json::Value) -> Result<(), ServerError> {
        let path = self.dir.join(HISTORY);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(io("could not open", &path))?;
        file.write_all(format!("{line}\n").as_bytes())
            .map_err(io("could not write", &path))
    }

    /// Refused while held.
    pub(super) fn unheld(&self) -> Result<(), ServerError> {
        self.held()
            .map_or(Ok(()), |hold| Err(ServerError::Held(hold)))
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

/// Now, in whole seconds since the epoch.
fn seconds_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
        })
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
