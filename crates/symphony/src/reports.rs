//! The hourly roll-up of a project into its day files ([`symphony_api::reports`];
//! docs/product/analysis/reports-and-monitoring.md, *The day files*):
//! today's file written again with what the day held up to now, and yesterday's made final once,
//! from one read of the board ([`symphony_board::reports`] counts it), two cheap reads of the
//! default branch's history ([`git`]), which give each landed task its landing's lines by its
//! land's receipt or its merge ([`git::landings`]), one more for each red that names the commits
//! it came between ([`git::trace`]), and the decision cards of the project's documents with a live
//! branch's posted cards ([`decisions`]). The tokens the board recorded are told by difference
//! ([`symphony_board::reports::spend`]): what its `usage` table holds at this read less what the
//! last day file written kept of it, today's or yesterday's, shared among the hours between the
//! two; with neither file, or none that kept it, the hours stay unknown and this roll-up's files
//! keep the sums for the next.
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

use crate::board::Doc;
use crate::branches::BranchCard;
use crate::plugins::Plugins;
use std::path::Path;
use symphony_api::reports::GitRead;
use symphony_board::ReportRead;
use symphony_board::recording::{Gate, State};
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
/// of the live branches' cards). Each file is written while `gate`, its server generation's
/// ([`symphony_board::recording`]), stands open, and held open meanwhile: once a reload's hand-off
/// closes it, nothing more is written, and the generation that replaces it writes them. A file that
/// cannot be written is the error.
#[expect(
    clippy::too_many_arguments,
    reason = "each is one of the roll-up's inputs, handed over from the server's tick as they are"
)]
pub fn roll_up(
    root: &Path,
    project: &str,
    now: f64,
    folder: &Path,
    days: &[f64],
    read: &ReportRead,
    branch_cards: &[BranchCard],
    gate: &Gate,
) -> Result<Rolled, String> {
    let mut rolled = Rolled::default();
    let Some(since) = days.first().copied() else {
        return Ok(rolled);
    };
    let settings = crate::project::Project::of(root);
    let excluded = git::excluded(&settings);
    let commits = git::read(root, &settings.default_branch, since, &excluded);
    let lands = git::lands(root);
    if let Err(why) = &commits {
        rolled.problems.push(format!("git: {why}"));
    }
    let plugins = Plugins::of(root);
    let docs = crate::board::documents(root).map(|docs| cards(docs, branch_cards, &plugins));
    if let Err(why) = &docs {
        rolled.problems.push(format!("documents: {why}"));
    }
    // The files as the last roll-up left them, read before any is written again: today's and
    // yesterday's, whichever of `days` is due.
    let today = day_start(now);
    let kept: Vec<symphony_api::ReportDay> = [today - DAY, today]
        .iter()
        .filter_map(|start| {
            symphony_board::reports::read(folder, &symphony_api::run_day(*start)).ok()?
        })
        .collect();
    let gained = kept
        .iter()
        .filter(|file| file.usage.is_some())
        .max_by(|a, b| a.read.total_cmp(&b.read))
        .map(|last| {
            let base = last.usage.as_deref().unwrap_or_default();
            symphony_board::reports::spend::gained(base, last.read, &read.usage, now)
        })
        .unwrap_or_default();
    for start in days {
        let until = now.min(start + DAY);
        let mut day = symphony_board::reports::day(project, *start, until, now, read);
        symphony_board::reports::spend::fill(
            &mut day,
            &read.usage,
            kept.iter().find(|file| file.start == *start),
            &gained,
        );
        git::trace(
            &mut day,
            root,
            &settings.default_branch,
            &mut rolled.problems,
        );
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
            git::landings(
                &mut day,
                commits,
                &settings.default_branch,
                lands.as_deref(),
            );
        }
        let open = gate.with(|state| match state {
            State::Open { .. } => symphony_board::reports::write(folder, &day).map(|()| true),
            State::Held | State::Closed(_) => Ok(false),
        })?;
        if !open {
            break;
        }
        rolled.written.push(day.day);
    }
    Ok(rolled)
}

/// The cards of `docs` with the live branches' (`branch_cards`) laid on them as the owner's queue
/// lays them ([`crate::dashboard::merge_decisions`]), each under its own page: a branch's copy of
/// one of main's cards goes back under main's page, so the day file names it by the card's page,
/// and only a card main has not got is named by the branch's.
fn cards(mut docs: Vec<Doc>, branch_cards: &[BranchCard], plugins: &Plugins) -> Vec<Doc> {
    // Where the card `slug` is in the page `path`: the page's place and the card's in it.
    let place = |docs: &[Doc], path: &str, slug: &str| {
        let page = docs.iter().position(|doc| doc.path == path)?;
        let card = docs[page].decisions.iter().position(|c| c.slug == slug)?;
        Some((page, card))
    };
    // Each branch card main has too, with main's page's place, which stands through the merge: it
    // takes cards out of pages and adds pages after them, never taking a page out.
    let mains: Vec<(&BranchCard, usize)> = branch_cards
        .iter()
        .filter(|b| !b.on_main)
        .filter_map(|b| Some((b, place(&docs, &b.doc, &b.card.slug)?.0)))
        .collect();
    crate::dashboard::merge_decisions(&mut docs, branch_cards, plugins);
    for (branch, main) in mains {
        let Some((page, card)) = place(&docs, &branch.path(), &branch.card.slug) else {
            continue; // no proposal: main's copy says it all, or closed the card after it
        };
        let card = docs[page].decisions.remove(card);
        docs[main].decisions.push(card);
    }
    docs
}

#[cfg(test)]
mod tests;
