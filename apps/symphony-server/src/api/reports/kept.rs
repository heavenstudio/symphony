//! The Reports page's answers kept between reads, each with what it was read from ([`Seen`]): the
//! hour of the clock, which sets the window; each day file of the window in the three folders, by
//! what a stat says of it; the oldest roll-up file by its name, the answer's `first`; and the tip
//! of the project's default branch, which the marks are read from.
//!
//! Every read looks at those and at nothing else: a stat of each file, the names in the roll-up's
//! folder and one read of the branch's ref in this process, no file opened and no git started. A window
//! whose kept answer saw the same is answered with its bytes and their validator, so the page's
//! check of an answer it holds is a `304` that cost neither. What moves within an hour is seen the
//! same way:
//!
//! - a commit reaching the default branch moves its tip, and the answer is read again, its marks
//!   by the one `git log` an answer makes ([`Kept::gits`]);
//! - the agents' file of the day is appended to at each tick that has a change to say, and the
//!   roll-up's and the health's are written at the hour: a file that moved has the window's files
//!   read again ([`Kept::passes`]), with no git, the marks being kept by the tip ([`Marks`]). The
//!   answer reads an agents' file for its hour lines alone, so most appends leave its bytes, and
//!   so its validator, as they were;
//! - the hour itself moves the window: its files are read again, its marks taken from the kept
//!   ones, which reach back at least as far.
//!
//! The look comes before the read, so a file or a tip that moves between the two is kept under
//! what it was before, and the next read finds it moved. An answer whose marks could not be read
//! is not kept, nor one whose branch tip could not be read: the next read asks git again.

use super::{Folders, Grain, ReportMark, day_files, days_read, first, page, periods};
use crate::api::validated::checksum;
use axum::body::Bytes;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use symphony_api::run_day;
use symphony_board::lock::Recovered as _;

/// The windows a hub keeps the answers of, the one read longest ago going first: a page asks for
/// one at a time, of the few its controls offer, while `days` is the asker's to name.
const WINDOWS: usize = 8;

/// An answer as it is sent: the page's JSON and the validator of those bytes
/// ([`crate::api::validated::checksum`]), the one the route would give them.
#[derive(Clone, Debug)]
pub(crate) struct Answer {
    pub(crate) body: Bytes,
    pub(crate) tag: String,
}

/// What a stat says of a file: its inode, size, time written (seconds and nanoseconds) and mode. A
/// link is its own, never what it leads to, as a page reads through none.
type Stamp = (u64, u64, i64, i64, u32);

/// What an answer was read from.
#[derive(Debug, PartialEq)]
struct Seen {
    /// Where the window ends: the end of the clock's hour.
    to: f64,
    /// The default branch's tip; none where the project has no such branch.
    tip: Option<String>,
    /// The first second of the oldest roll-up file, by its name ([`first`]).
    first: Option<f64>,
    /// Each day file of the window, newest day first, in each folder there is; none for one that
    /// is not there.
    files: Vec<Option<Stamp>>,
}

/// A window's kept answer.
struct Window {
    grain: Grain,
    days: u32,
    /// Held while this window is read, so its readers share one read without holding up a kept
    /// answer of another window.
    answer: Mutex<Option<(Seen, Answer)>>,
}

/// The marks one `git log` read: those of `tip` since `since`, oldest first.
struct Marks {
    tip: String,
    since: f64,
    marks: Vec<ReportMark>,
}

/// A hub's kept answers and marks, and how often it read for them.
#[derive(Default)]
pub(crate) struct Kept {
    /// Held only to find a window and move it to the end, never for its files or git.
    windows: Mutex<Vec<Arc<Window>>>,
    marks: Mutex<Option<Marks>>,
    passes: AtomicU64,
    gits: AtomicU64,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().recover()
}

fn stamp(path: &Path) -> Option<Stamp> {
    let said = std::fs::symlink_metadata(path).ok()?;
    Some((
        said.ino(),
        said.size(),
        said.mtime(),
        said.mtime_nsec(),
        said.mode(),
    ))
}

impl Seen {
    /// What a window from `from` to `to` would be read from now, with the observed branch tip.
    fn now(folders: &Folders, tip: Option<String>, from: f64, to: f64) -> Seen {
        Seen {
            to,
            tip,
            first: folders.reports.as_ref().ok().and_then(|path| first(path)),
            files: days_read(from, to)
                .flat_map(|day| day_files(folders, &run_day(day)))
                .flatten()
                .map(|path| stamp(&path))
                .collect(),
        }
    }
}

impl Kept {
    /// How many times the day files of a window were read through.
    #[cfg(test)]
    pub(crate) fn passes(&self) -> u64 {
        self.passes.load(Ordering::Relaxed)
    }

    /// How many times git was started for the marks.
    #[cfg(test)]
    pub(crate) fn gits(&self) -> u64 {
        self.gits.load(Ordering::Relaxed)
    }

    /// The answer of a window of `days` by `grain` at `now`, from `folders` and the default branch
    /// of the project at `root`: the kept one where what it was read from is what is there now,
    /// else read, and kept in its place.
    pub(crate) fn answer(
        &self,
        folders: &Folders,
        root: &Path,
        grain: Grain,
        days: u32,
        now: f64,
    ) -> Answer {
        self.answer_with_tip(folders, root, grain, days, now, symphony::gates::tip)
    }

    /// Read the branch tip separately from git's marks read, so a missing observation can be
    /// exercised even when git can read the branch.
    fn answer_with_tip(
        &self,
        folders: &Folders,
        root: &Path,
        grain: Grain,
        days: u32,
        now: f64,
        tip: impl FnOnce(&Path, &str) -> Option<String>,
    ) -> Answer {
        let branch = symphony::project::Project::of(root).default_branch.clone();
        let spans = periods(grain, days, now);
        let from = spans.first().map_or(now, |span| span.0);
        let to = spans.last().map_or(now, |span| span.1);
        let seen = Seen::now(folders, tip(root, &branch), from, to);
        let window = self.window(grain, days);
        let mut kept = lock(&window.answer);
        if let Some((before, answer)) = kept.as_ref()
            && *before == seen
        {
            return answer.clone();
        }
        *kept = None;
        let mut failed = false;
        self.passes.fetch_add(1, Ordering::Relaxed);
        let page = page(folders, grain, days, now, |since| {
            self.marks(root, &branch, seen.tip.as_deref(), since)
                .inspect_err(|_| failed = true)
        });
        let body = serde_json::to_vec(&page).expect("a reports page serializes");
        let answer = Answer {
            tag: checksum(&body),
            body: Bytes::from(body),
        };
        if !failed && seen.tip.is_some() {
            *kept = Some((seen, answer.clone()));
        }
        answer
    }

    /// Find this window and mark it as the most recently asked for, making room for a new one
    /// when needed. Its answer has its own lock, which stays alive for any readers after eviction.
    fn window(&self, grain: Grain, days: u32) -> Arc<Window> {
        let mut windows = lock(&self.windows);
        let window = match windows
            .iter()
            .position(|window| window.grain == grain && window.days == days)
        {
            Some(at) => windows.remove(at),
            None => {
                if windows.len() >= WINDOWS {
                    windows.remove(0);
                }
                Arc::new(Window {
                    grain,
                    days,
                    answer: Mutex::default(),
                })
            }
        };
        windows.push(Arc::clone(&window));
        window
    }

    /// The marks of `branch` since `since`, oldest first: the kept ones where they were
    /// read from `tip` and reach back as far, else one `git log`, kept where the branch has a tip.
    fn marks(
        &self,
        root: &Path,
        branch: &str,
        tip: Option<&str>,
        since: f64,
    ) -> Result<Vec<ReportMark>, String> {
        let mut kept = lock(&self.marks);
        if let (Some(marks), Some(tip)) = (kept.as_ref(), tip)
            && marks.tip == tip
            && marks.since <= since
        {
            return Ok(marks.marks.clone());
        }
        self.gits.fetch_add(1, Ordering::Relaxed);
        let marks = super::marks(root, branch, since)?;
        if let Some(tip) = tip {
            *kept = Some(Marks {
                tip: tip.to_string(),
                since,
                marks: marks.clone(),
            });
        }
        Ok(marks)
    }
}

#[cfg(test)]
#[path = "kept_tests.rs"]
mod tests;
