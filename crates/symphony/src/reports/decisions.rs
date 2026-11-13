//! The decision cards of a day file: how many waited on the owner at each hour's end, which ones
//! the owner answered, which an agent auto-approved and which an agent withdrew, read back from the
//! dated lines under each card of the project's documents, a live branch's posted cards laid on
//! main's as the owner's queue lays them ([`crate::dashboard::merge_decisions`]).
//!
//! A card's history is replayed one dated line at a time: after each, the card is regrouped as the
//! register groups it ([`Decision::thread`]), over the lines up to it and the mark they leave (the
//! last verdict a line gives, the owner's or an agent's `resolved`, `auto-approved` or `dismissed`;
//! the card's own mark after its last line). It waits on the owner while it sits in *Needs your
//! decision* or *Answered: approve again or comment*, as the owner's queue counts it
//! ([`crate::triage::queue`]). The line that takes it out of those groups is counted once, by whose
//! it is and what it says: the owner's answered it, an agent's `auto-approved` decided it without
//! asking, and any other agent's (a supersession's `dismissed`, a question withdrawn) withdrew it.
//! What the history cannot tell: a line's minute is when it was written, in the machine's time
//! zone; a card with no dated line has no history, and counts as it stands now at every hour; an
//! `auto-approved` line has no fixed word for a basis that names an earlier verdict of the owner's
//! that decides the card, so it counts as the triage's own judgement does; and a decision recorded
//! as answered by a plain agent line on the card the owner approved leaves that card where it was,
//! out of the queue, so it is counted nowhere.

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

    /// Each line that took it out of the owner's queue, and how. Its first dated line (its posting,
    /// or the first since lines were dated) has no queue before it: the owner's line that leaves it
    /// out of the queue answered it, and an agent's posting of a card already auto-approved
    /// auto-approved it; any other agent's posting out of the queue never asked the owner.
    fn answers(&self) -> Vec<(Out, Answer)> {
        let mut found = Vec::new();
        for (k, step) in self.steps.iter().enumerate() {
            let before = k.checked_sub(1).map(|k| self.steps[k].waiting);
            if step.waiting || before == Some(false) {
                continue;
            }
            let out = match (step.who, step.verdict.as_deref()) {
                (Who::Owner, _) => Out::Answered,
                (Who::Agent, Some(AUTO_APPROVED)) => Out::AutoApproved,
                (Who::Agent, _) if before.is_some() => Out::Withdrawn,
                (Who::Agent, _) => continue,
            };
            let how = match (step.who, step.verdict.as_deref()) {
                (_, Some(verdict)) => verdict,
                (Who::Owner, None) => "commented",
                (Who::Agent, None) => "replied",
            };
            found.push((
                out,
                Answer {
                    doc: self.doc.clone(),
                    slug: self.slug.clone(),
                    at: step.at,
                    how: how.to_string(),
                },
            ));
        }
        found
    }
}

/// The word of an agent's line that auto-approves a card.
const AUTO_APPROVED: &str = "auto-approved";

/// How a card left the owner's queue ([`symphony_api::reports::DecisionCounts`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Out {
    /// By the owner's own line.
    Answered,
    /// By an agent's `auto-approved` line: decided without asking.
    AutoApproved,
    /// By any other agent's line, deciding nothing.
    Withdrawn,
}

/// Fill `day`'s decisions, and each hour's, from the cards of `docs`. Each list is kept in order as
/// it is made, by time and then slug, rather than sorted: a sort costs the server's binary a sort of
/// its own for each kind of list (docs/CHECKS.md, size budgets).
pub fn fill(day: &mut ReportDay, docs: &[Doc], plugins: &Plugins) {
    let mut histories: Vec<History> = Vec::new();
    for doc in docs {
        for card in &doc.decisions {
            histories.push(History {
                doc: doc.path.clone(),
                slug: card.slug.clone(),
                steps: steps(card, plugins),
                waiting_now: waits_on_owner(card.group(plugins)),
            });
        }
    }
    let mut answers: Vec<(Out, Answer)> = Vec::new();
    for answer in histories.iter().flat_map(History::answers) {
        let key = (answer.1.at, &answer.1.slug);
        let at = answers.partition_point(|(_, kept)| (kept.at, &kept.slug) <= key);
        answers.insert(at, answer);
    }
    let until = day.until;
    for hour in &mut day.hours {
        let to = (hour.start + crate::reports::HOUR).min(until);
        hour.decisions = Some(DecisionCounts {
            open: u32::try_from(histories.iter().filter(|h| h.waiting(to)).count())
                .unwrap_or(u32::MAX),
            ..DecisionCounts::default()
        });
    }
    // Each answer in the day, in order, to its hour's count and the day's list of its kind.
    let mut lists = Decisions::default();
    for (out, answer) in answers {
        if answer.at < day.start || answer.at >= until {
            continue;
        }
        let counts = symphony_board::reports::hour_at(&mut day.hours, until, answer.at)
            .and_then(|hour| hour.decisions.as_mut());
        let (count, list) = match out {
            Out::Answered => (counts.map(|c| &mut c.answered), &mut lists.answered),
            Out::AutoApproved => (counts.map(|c| &mut c.auto_approved), &mut lists.auto_approved),
            Out::Withdrawn => (counts.map(|c| &mut c.withdrawn), &mut lists.withdrawn),
        };
        if let Some(count) = count {
            *count += 1;
        }
        list.push(answer);
    }
    let mut open: Vec<OpenCard> = Vec::new();
    for history in histories.iter().filter(|history| history.waiting(until)) {
        let card = OpenCard {
            doc: history.doc.clone(),
            slug: history.slug.clone(),
            since: history.since(until),
        };
        // A card waiting since no line it has comes first.
        let key = (card.since.unwrap_or(f64::NEG_INFINITY), &card.slug);
        let at = open
            .partition_point(|kept| (kept.since.unwrap_or(f64::NEG_INFINITY), &kept.slug) <= key);
        open.insert(at, card);
    }
    lists.open = open;
    day.decisions = Some(lists);
}

#[cfg(test)]
#[path = "decisions_tests.rs"]
mod tests;
