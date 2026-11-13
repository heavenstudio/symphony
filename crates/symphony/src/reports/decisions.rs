//! The decision cards of a day file: how many waited on the owner at each hour's end, which ones
//! the owner answered, and which an agent's line answered without asking, read back from the
//! dated lines under each card in the project's documents ([`crate::board::documents`]).
//!
//! A card's history is replayed one dated line at a time: after each, the card is regrouped as the
//! register groups it ([`Decision::thread`]), over the lines up to it and the mark they leave (the
//! last verdict a line gives, the owner's or an agent's `resolved`, `auto-approved` or `dismissed`;
//! the card's own mark after its last line). It waits on the owner while it sits in *Needs your
//! decision* or *Answered: approve again or comment*, as the owner's queue counts it
//! ([`crate::triage::queue`]). The line that takes it out of those groups answered it: the owner's,
//! or an agent's, which answered it without asking; an agent's line that posts a card already out
//! of them answered it so too. What the history cannot tell: a line's minute is when it was written,
//! in the machine's time zone; a card with no dated line has no history, and counts as it stands now
//! at every hour; an `auto-approved` line has no fixed word for a basis that names an earlier
//! verdict of the owner's that decides the card, so it counts as the triage's own judgement does.

use crate::board::Doc;
use crate::clock::stamp_seconds;
use crate::plugins::Plugins;
use crate::register::{Decision, Group, QUESTION, State, Who};
use symphony_api::reports::{Answer, DecisionCounts, Decisions, OpenCard, ReportDay};
use symphony_docs::syntax::{entries, entry_line, is_continuation, unseen_owner};

/// The mark the lines `raw` leave a card with: the last verdict a line gives that sets one, an
/// agent's question setting none; pending with none.
fn mark(raw: &[String]) -> State {
    entries(raw)
        .iter()
        .rev()
        .find_map(|entry| {
            let verdict = entry.verdict.as_deref()?;
            if entry.who == "agent" && verdict == QUESTION {
                return None;
            }
            State::parse(verdict)
        })
        .unwrap_or(State::Pending)
}

/// One dated line under a card and where it left the card.
#[derive(Debug, Clone, PartialEq)]
struct Step {
    /// The line's minute, never before the line above it.
    at: f64,
    /// Whether the card waited on the owner after it.
    waiting: bool,
    who: Who,
    verdict: Option<String>,
}

fn waits_on_owner(group: Group) -> bool {
    matches!(group, Group::Decide | Group::Confirm)
}

/// Where each dated line of `raw` starts and the second it names, read as [`entries`] reads them:
/// a deeper-indented line under a dated one continues it, a hidden line is none, and a line whose
/// stamp names no minute is left out. A line dated before the one above it is read at that one's.
fn starts(raw: &[String]) -> Vec<(usize, f64)> {
    let mut found: Vec<(usize, f64)> = Vec::new();
    let mut dated = false;
    for (index, line) in raw.iter().enumerate() {
        if unseen_owner(line).is_some() || line.trim().is_empty() {
            continue;
        }
        if dated && is_continuation(line) {
            continue;
        }
        let entry = entry_line(line);
        dated = entry.is_some();
        if let Some(at) = entry
            .and_then(|entry| entry.ts)
            .as_deref()
            .and_then(stamp_seconds)
        {
            let at = found.last().map_or(at, |(_, before)| at.max(*before));
            found.push((index, at));
        }
    }
    found
}

/// Each dated line of `card`, in the order written, and where it left the card.
fn steps(card: &Decision, plugins: &Plugins) -> Vec<Step> {
    let starts = starts(&card.raw);
    let mut steps = Vec::new();
    for (k, (index, at)) in starts.iter().enumerate() {
        let end = starts.get(k + 1).map_or(card.raw.len(), |(next, _)| *next);
        let raw = card.raw[..end].to_vec();
        let state = if end == card.raw.len() {
            card.state
        } else {
            mark(&raw)
        };
        let Some(line) = entry_line(&card.raw[*index]) else {
            continue;
        };
        let then = Decision {
            state,
            raw,
            note: Vec::new(),
            ..card.clone()
        };
        steps.push(Step {
            at: *at,
            waiting: waits_on_owner(then.thread(plugins).group),
            who: if line.who == "owner" {
                Who::Owner
            } else {
                Who::Agent
            },
            verdict: line.verdict,
        });
    }
    steps
}

/// One card's history, as the day file counts it.
struct History {
    doc: String,
    slug: String,
    steps: Vec<Step>,
    /// Whether it waits on the owner now: what a card with no dated line counts as at every hour.
    waiting_now: bool,
}

impl History {
    /// Whether it waited on the owner just before `at`; a card posted at or after `at` did not.
    fn waiting(&self, at: f64) -> bool {
        if self.steps.is_empty() {
            return self.waiting_now;
        }
        self.steps
            .iter()
            .rev()
            .find(|step| step.at < at)
            .is_some_and(|step| step.waiting)
    }

    /// Since when it had waited just before `at`, when it did: the line that last put it in the
    /// owner's queue; none for a card with no dated line.
    fn since(&self, at: f64) -> Option<f64> {
        let before: Vec<&Step> = self.steps.iter().filter(|step| step.at < at).collect();
        let last = before.iter().rposition(|step| !step.waiting);
        before.get(last.map_or(0, |i| i + 1)).map(|step| step.at)
    }

    /// Each line that took it out of the owner's queue, or posted it out of it, by an agent
    /// (`false`) or the owner (`true`).
    fn answers(&self) -> Vec<(bool, Answer)> {
        let mut found = Vec::new();
        for (k, step) in self.steps.iter().enumerate() {
            let before = k.checked_sub(1).map(|k| self.steps[k].waiting);
            let answered = match before {
                Some(waiting) => waiting && !step.waiting,
                None => !step.waiting,
            };
            if !answered {
                continue;
            }
            let owner = step.who == Who::Owner;
            let how = match (owner, step.verdict.as_deref()) {
                (true, Some(verdict)) => verdict.to_string(),
                (true, None) => "commented".to_string(),
                (false, Some("auto-approved")) => "auto-approved".to_string(),
                (false, _) => "agent".to_string(),
            };
            found.push((
                owner,
                Answer {
                    doc: self.doc.clone(),
                    slug: self.slug.clone(),
                    at: step.at,
                    how,
                },
            ));
        }
        found
    }
}

/// Fill `day`'s decisions, and each hour's, from the cards of `docs`.
pub fn fill(day: &mut ReportDay, docs: &[Doc], plugins: &Plugins) {
    let histories: Vec<History> = docs
        .iter()
        .flat_map(|doc| {
            doc.decisions.iter().map(|card| History {
                doc: doc.path.clone(),
                slug: card.slug.clone(),
                steps: steps(card, plugins),
                waiting_now: waits_on_owner(card.group(plugins)),
            })
        })
        .collect();
    let answers: Vec<(bool, Answer)> = histories.iter().flat_map(History::answers).collect();
    let until = day.until;
    for hour in &mut day.hours {
        let to = (hour.start + crate::reports::HOUR).min(until);
        let within = |at: f64| hour.start <= at && at < to;
        let count = |owner: bool| {
            let n = answers
                .iter()
                .filter(|(by, a)| *by == owner && within(a.at))
                .count();
            u32::try_from(n).unwrap_or(u32::MAX)
        };
        hour.decisions = Some(DecisionCounts {
            open: u32::try_from(histories.iter().filter(|h| h.waiting(to)).count())
                .unwrap_or(u32::MAX),
            answered: count(true),
            unasked: count(false),
        });
    }
    let mut open: Vec<OpenCard> = histories
        .iter()
        .filter(|history| history.waiting(until))
        .map(|history| OpenCard {
            doc: history.doc.clone(),
            slug: history.slug.clone(),
            since: history.since(until),
        })
        .collect();
    open.sort_by(|a, b| {
        let since = |card: &OpenCard| card.since.unwrap_or(f64::NEG_INFINITY);
        since(a)
            .total_cmp(&since(b))
            .then_with(|| a.slug.cmp(&b.slug))
    });
    let in_day = |owner: bool| {
        let mut picked: Vec<Answer> = answers
            .iter()
            .filter(|(by, a)| *by == owner && day.start <= a.at && a.at < until)
            .map(|(_, a)| a.clone())
            .collect();
        picked.sort_by(|a, b| a.at.total_cmp(&b.at).then_with(|| a.slug.cmp(&b.slug)));
        picked
    };
    day.decisions = Some(Decisions {
        open,
        answered: in_day(true),
        unasked: in_day(false),
    });
}

#[cfg(test)]
#[path = "decisions_tests.rs"]
mod tests;
