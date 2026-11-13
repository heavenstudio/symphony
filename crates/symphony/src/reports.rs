//! The hourly roll-up of a project into its day files ([`symphony_api::reports`];
//! docs/analysis/reports-and-monitoring.md, *The day files*):
//! today's file written again with what the day held up to now, and yesterday's made final once,
//! from one read of the board ([`symphony_board::reports`] counts it), two cheap reads of the
//! default branch's history ([`git`]) and the decision cards of the project's documents with a
//! live branch's posted cards ([`decisions`]).
//!
//! Only the machine's one server rolls a project up, once an hour (apps/symphony-server,
//! `live/reports.rs`): it reads the board for the days [`due`] names and hands that read here, off
//! its tick; nothing here reads a clock, the board or starts on its own. A board read that fails
//! writes no file, so each day file stays what the last roll-up made of it. A day file whose day
//! had ended when it was read is final: it is never written again. The documents or git failing
//! leaves only their own figures out ([`symphony_api::ReportDay::decisions`],
//! [`symphony_api::ReportDay::git`]), and the roll-up says why ([`Rolled::problems`]).

pub mod decisions;
pub mod git;

use crate::branches::BranchCard;
use std::path::Path;
use symphony_api::reports::GitRead;
use symphony_board::ReportRead;
pub use symphony_board::reports::{DAY, HOUR, day_start};

/// What one roll-up wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rolled {
    /// The days whose file it wrote (`YYYY-MM-DD`), oldest first.
    pub written: Vec<String>,
    /// What could not be read besides the board, its figures left out of the files.
    pub problems: Vec<String>,
}

/// The first second of each day whose file is due at `now` in `folder`, oldest first: yesterday's,
/// while its file is missing, unreadable or not final, then today's. The board read a roll-up
/// hands [`roll_up`] starts at the first.
pub fn due(folder: &Path, now: f64) -> Vec<f64> {
    let today = day_start(now);
    let yesterday = today - DAY;
    if symphony_board::reports::closed(folder, &symphony_api::run_day(yesterday)) {
        vec![today]
    } else {
        vec![yesterday, today]
    }
}

/// Roll the project at `root`, its id `project`, up into `folder` at `now`: the day files of
/// `days` ([`due`]), their board figures from `read`, the board's one read of everything since the
/// first of them began, their cards from the documents with `branch_cards` (the server's kept scan
/// of the live branches' cards). A file that cannot be written is the error.
pub fn roll_up(
    root: &Path,
    project: &str,
    now: f64,
    folder: &Path,
    days: &[f64],
    read: &ReportRead,
    branch_cards: &[BranchCard],
) -> Result<Rolled, String> {
    let mut rolled = Rolled::default();
    let Some(since) = days.first().copied() else {
        return Ok(rolled);
    };
    let settings = crate::project::Project::of(root);
    let excluded = git::excluded(&settings);
    let commits = git::read(root, &settings.default_branch, since, &excluded);
    if let Err(why) = &commits {
        rolled.problems.push(format!("git: {why}"));
    }
    let plugins = crate::plugins::Plugins::of(root);
    let docs = crate::board::documents(root).map(|mut docs| {
        crate::dashboard::merge_decisions(&mut docs, branch_cards, &plugins);
        docs
    });
    if let Err(why) = &docs {
        rolled.problems.push(format!("documents: {why}"));
    }
    for start in days {
        let until = now.min(start + DAY);
        let mut day = symphony_board::reports::day(project, *start, until, now, read);
        if let Ok(docs) = &docs {
            decisions::fill(&mut day, docs, &plugins);
        }
        if let Ok(commits) = &commits {
            git::fill(
                &mut day,
                commits,
                GitRead {
                    branch: settings.default_branch.clone(),
                    excluded: excluded.clone(),
                },
            );
        }
        symphony_board::reports::write(folder, &day)?;
        rolled.written.push(day.day);
    }
    Ok(rolled)
}

#[cfg(test)]
mod tests;
