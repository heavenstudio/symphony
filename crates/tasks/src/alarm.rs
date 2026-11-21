//! The pipeline alarm: review holding more than [`Thresholds::ratio`] times the work in progress for
//! [`Thresholds::raise_hours`] raises it, and it stands until review holds no more than that for
//! [`Thresholds::clear_hours`], so a short dip does not lower it. Review that holds many times the
//! work in progress is alarming: 31 tasks in review against 6 in progress (and 28 done in the last
//! 24 hours), for one.
//! The PULL digest says it ([`crate::cli::pull`]).
//!
//! The board keeps no history of its counts, but every change of a task's status writes a line on
//! its thread: a claim, a move, a verdict, a landing, a block by a cancelled dependency and its end
//! ([`step`]). [`timeline`] walks a task's lines of the last [`LOOKBACK_SECONDS`] (or twice the
//! raise and clear spans, if longer) back from its status now, [`counted`] adds the work up at every
//! change, and [`replay`] runs the alarm over those counts. So the alarm is what the board's record
//! says, the same for every reader at the same moment, and needs no writer.
//!
//! What held when the window opened is read, never assumed. Where a task's status then, or whether
//! it was rework, rests on its thread before the window ([`rests_on_before`]), its newest lines
//! before it say ([`opening`]). The alarm's own state then is what a replay of a window twice as
//! long says, when this one holds neither a raise span of review above the ratio nor a clear span
//! below it (the replays from raised and from lowered disagree), back to the board's first task,
//! before which nothing was in review and the alarm was lowered.
//!
//! Only work counts ([`crate::slots::Ledger::work`]): a review mirror counts with the task it reviews
//! and an instruction subtask with the task it serves, neither on its own, as the tree stood at each
//! moment: a task made or moved in it changes the counts from then on ([`own_over`],
//! `alarm/tree.rs`). A task in review whose
//! tip a reviewer asked changes of is rework, counted in progress (`alarm/rework.rs`, [`reworked`]).
//!
//! The thresholds are the project's: `[board]` [`KEYS`] in the main checkout's `symphony.toml`,
//! read as the limits beside them are ([`crate::limits`]); a key not written takes the default.

mod rework;
mod tree;

pub(crate) use rework::waits_set;
pub use rework::{
    ASSESSED, Assessed, CHANGES, FIXED, Recorded, Tip, Tips, WAITS, WAITS_SET, Waits, Written,
    as_worked, kept_fix, reworked, written_key,
};
pub use tree::{Moved, NOTHING, UNDER, WAS, moved, own_only, own_over};

use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};

/// The `[board]` keys of the project file: the ratio, the hours above it that raise the alarm, the
/// hours at or below it that clear it.
pub const KEYS: [&str; 3] = [
    "review_alarm_ratio",
    "review_alarm_hours",
    "review_alarm_clear_hours",
];

/// How far back the replay reads the board's record: a day.
pub const LOOKBACK_SECONDS: f64 = 24.0 * 3600.0;

/// When the alarm raises and clears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    /// Review holding more than this many times the work in progress is too much.
    pub ratio: u32,
    /// Too much for this many hours raises the alarm.
    pub raise_hours: u32,
    /// No more than the ratio for this many hours clears it.
    pub clear_hours: u32,
}

impl Default for Thresholds {
    /// Twice in progress, for 4 hours, cleared after an hour.
    fn default() -> Self {
        Thresholds {
            ratio: 2,
            raise_hours: 4,
            clear_hours: 1,
        }
    }
}

impl Thresholds {
    pub fn raise_seconds(&self) -> f64 {
        f64::from(self.raise_hours) * 3600.0
    }

    pub fn clear_seconds(&self) -> f64 {
        f64::from(self.clear_hours) * 3600.0
    }

    /// How far back the replay reads: [`LOOKBACK_SECONDS`], or twice a raise and a clear if longer.
    pub fn lookback(&self) -> f64 {
        LOOKBACK_SECONDS.max(2.0 * (self.raise_seconds() + self.clear_seconds()))
    }

    /// Whether `counts` hold more review than the ratio allows.
    pub fn over(&self, counts: Counts) -> bool {
        counts.review > counts.in_progress.saturating_mul(self.ratio as usize)
    }
}

/// A line on a task's thread that may say its status changed (`Store::status_lines`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Line {
    pub slug: String,
    pub sid: i64,
    pub ts: f64,
    pub kind: String,
    pub text: String,
}

/// What a line of the board's says of a task's status.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step<'a> {
    /// Nothing: no change of status.
    Unchanged,
    /// From one status into another, both named: a move, a verdict, a block and its end.
    Between(&'a str, &'a str),
    /// A claim: ready work goes into progress, any other stays as it was.
    Claimed,
    /// A landing, which closes a task in review, blocked or (taken back) in progress.
    Landed { taken_back: bool },
    /// A close as covered by another task's landing (`crate::COVERED`), which closes a task in any
    /// open status: the one the line before it left.
    Covered,
}

/// The line a move writes: `HANDLE moved it FROM → TO`.
pub const MOVED: &str = " moved it ";
pub const ARROW: &str = " → ";
/// The line a claim writes: `HANDLE claimed it`, perhaps `, handed over from …`.
pub const CLAIMED: &str = " claimed it";
/// What a landing that closes work taken back to in progress says.
pub const TAKEN_BACK: &str = " was taken back to in_progress";

/// The status named at the start of `text`, if it is one.
fn status_word(text: &str) -> Option<&str> {
    let word = text.split([' ', '(', ')', ',', ';']).next()?;
    crate::store::STATUSES.contains(&word).then_some(word)
}

/// The moves a task a custom workflow holds made, as the lines the alarm reads ([`step`]): its
/// recorded transitions (`transitions`, each task's oldest first), each that changed the status
/// its task projects to said as a move from its predecessor's status into its own, written when it
/// was, for those made at `since` or later. A task's first transition, which places it, and one
/// that keeps its status say nothing. So the alarm reads a held task's record, never its line on
/// the thread or a state's name.
pub fn transition_lines(transitions: &[crate::workflow::Moved], since: f64) -> Vec<Line> {
    (transitions.windows(2))
        .filter(|pair| pair[0].slug == pair[1].slug && pair[1].at >= since)
        .filter(|pair| pair[0].status != pair[1].status)
        .map(|pair| Line {
            slug: pair[1].slug.clone(),
            sid: 0,
            ts: pair[1].at,
            kind: "system".into(),
            text: format!(
                "workflow{MOVED}{}{ARROW}{} ({})",
                pair[0].status,
                pair[1].status,
                crate::workflow::Transition::id_of(&pair[1].slug, pair[1].revision)
            ),
        })
        .collect()
}

/// What `line` says of its task's status ([`Step`]).
pub fn step(line: &Line) -> Step<'_> {
    let text = line.text.as_str();
    match line.kind.as_str() {
        "system" => {
            if let Some((by, moved)) = text.split_once(MOVED)
                && !by.contains(' ')
                && let Some((from, to)) = moved.split_once(ARROW)
                && let (Some(from), Some(to)) = (status_word(from), status_word(to))
            {
                return Step::Between(from, to);
            }
            if let Some(rest) = text.strip_prefix(crate::store::BLOCK_LINE)
                && let Some(was) = rest
                    .strip_suffix(')')
                    .and_then(|rest| rest.rsplit_once(" (was "))
                    .and_then(|(_, was)| status_word(was))
            {
                return Step::Between(was, "blocked");
            }
            if let Some(to) = crate::stages::unblocked_to(text).and_then(status_word) {
                return Step::Between("blocked", to);
            }
            let claimed = text.split_once(CLAIMED).is_some_and(|(by, rest)| {
                !by.contains(' ') && (rest.is_empty() || rest.starts_with(','))
            });
            if claimed {
                return Step::Claimed;
            }
            Step::Unchanged
        }
        "verdict" => {
            let said = |word: &str| text == word || text.starts_with(&format!("{word}: "));
            if said("approve") {
                Step::Between("review", "done")
            } else if said("changes") {
                Step::Between("review", "in_progress")
            } else if said("done") {
                Step::Between("in_progress", "done")
            } else if text.starts_with(crate::COVERED) {
                Step::Covered
            } else {
                Step::Landed {
                    taken_back: text.contains(TAKEN_BACK),
                }
            }
        }
        _ => Step::Unchanged,
    }
}

impl<'a> Step<'a> {
    /// The status the line leaves its task in, when it says.
    fn after(&self) -> Option<&'a str> {
        match self {
            Step::Unchanged => None,
            Step::Between(_, to) => Some(to),
            Step::Claimed => Some("in_progress"),
            Step::Landed { .. } | Step::Covered => Some("done"),
        }
    }

    /// The status before the line, its task being in `after` once it was written, and `older` the
    /// status the line before it on the thread left, where it says: with none, a claim that put
    /// work in progress took it from ready, as a task's first claim does.
    fn before(&self, after: &'a str, older: Option<&'a str>) -> &'a str {
        match self {
            Step::Unchanged => after,
            Step::Between(from, _) => from,
            Step::Claimed if after == "in_progress" => match older {
                Some("in_progress") => "in_progress",
                _ => "ready",
            },
            Step::Claimed => after,
            Step::Landed { taken_back: true } => "in_progress",
            Step::Landed { taken_back: false } => match older {
                Some("blocked") => "blocked",
                _ => "review",
            },
            // Whatever open status it closed; with none said, the status work is filed in.
            Step::Covered => older.unwrap_or("ready"),
        }
    }
}

/// What a task's thread said before the window opened ([`opening`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Opening<'a> {
    /// The status its newest change of status left it in; none when its thread never said.
    pub status: Option<&'a str>,
    /// Its newest record of [`WAITS`], with when it was written, which stands into the window
    /// ([`reworked`]); none when the newest says nothing the record reads, or its thread never
    /// recorded one.
    pub record: Option<Recorded<'a>>,
}

/// What `lines`, a task's newest lines before the window (`Store::lines_before`), oldest first, say
/// held when it opened: the newest change of status, and the newest record of [`WAITS`].
pub fn opening<'a>(lines: &[&'a Line]) -> Opening<'a> {
    let newest_first = || lines.iter().rev();
    Opening {
        status: newest_first().find_map(|line| step(line).after()),
        record: newest_first()
            .find(|line| waits_set(line).is_some())
            .and_then(|line| Recorded::of(line)),
    }
}

/// Whether what a task held when the window opened rests on its thread before it ([`opening`]),
/// `status` being its status now and `lines` its own lines in the window, oldest first: the oldest
/// change of status in the window is a claim or a landing, whose status before it the line before
/// tells ([`Step::before`]), or it was in review some time in the window, where a record of
/// [`CHANGES`] written before it may stand ([`reworked`]).
pub fn rests_on_before(status: &str, lines: &[&Line]) -> bool {
    let steps: Vec<Step<'_>> = (lines.iter().map(|line| step(line)))
        .filter(|s| *s != Step::Unchanged)
        .collect();
    let in_review = |s: &Step<'_>| match s {
        Step::Between(from, to) => *from == "review" || *to == "review",
        Step::Landed { taken_back } => !taken_back,
        Step::Claimed | Step::Covered | Step::Unchanged => false,
    };
    let from_before = matches!(
        steps.first(),
        Some(Step::Claimed | Step::Landed { taken_back: false } | Step::Covered)
    );
    from_before || status == "review" || steps.iter().any(in_review)
}

/// The status a task held from each time on, oldest first, over the window from `since`: walked back
/// from `status`, its status now, through `lines`, its own lines since then, oldest first, to
/// `opened`, the status its newest change of status before the window left ([`Opening::status`]).
/// Before `created` it held none (`None`).
pub fn timeline<'a>(
    status: &'a str,
    created: f64,
    lines: &[&'a Line],
    since: f64,
    opened: Option<&'a str>,
) -> Vec<(f64, Option<&'a str>)> {
    let steps: Vec<Step<'a>> = lines.iter().map(|line| step(line)).collect();
    let mut pieces = Vec::new();
    let mut after = status;
    let newest_first = lines.iter().enumerate().rev();
    for (i, line) in newest_first {
        if steps[i] == Step::Unchanged {
            continue;
        }
        pieces.push((line.ts, Some(after)));
        let older = steps[..i].iter().rev().find_map(Step::after).or(opened);
        after = steps[i].before(after, older);
    }
    let start = since.max(created);
    pieces.push((start, Some(after)));
    if created > since {
        pieces.push((since, None));
    }
    pieces.reverse();
    pieces
}

/// Work in review and in progress at one moment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub review: usize,
    pub in_progress: usize,
}

impl Counts {
    fn add(&mut self, status: Option<&str>, by: isize) {
        let count = match status {
            Some("review") => &mut self.review,
            Some("in_progress") => &mut self.in_progress,
            _ => return,
        };
        *count = count.saturating_add_signed(by);
    }
}

/// A task's move from one status to another, either none (before it was made, after it was gone).
type Move<'a> = (Option<&'a str>, Option<&'a str>);

/// The counts at the window's start, then the counts from each moment one changed, oldest first:
/// `timelines` added up ([`timeline`]). Changes at one moment are taken together.
pub fn counted(timelines: &[&[(f64, Option<&str>)]]) -> (Counts, Vec<(f64, Counts)>) {
    let mut start = Counts::default();
    let mut changes: BTreeMap<OrderedTime, Vec<Move<'_>>> = BTreeMap::new();
    for pieces in timelines {
        let Some(((_, first), rest)) = pieces.split_first() else {
            continue;
        };
        start.add(*first, 1);
        let mut held = *first;
        for (at, status) in rest {
            changes
                .entry(OrderedTime(*at))
                .or_default()
                .push((held, *status));
            held = *status;
        }
    }
    let mut counts = start;
    let mut out = Vec::new();
    for (at, moves) in changes {
        let before = counts;
        for (from, to) in moves {
            counts.add(from, -1);
            counts.add(to, 1);
        }
        if counts != before {
            out.push((at.0, counts));
        }
    }
    (start, out)
}

/// A time a map orders by: a board's times are never NaN.
#[derive(Debug, Clone, Copy, PartialEq)]
struct OrderedTime(f64);

impl Eq for OrderedTime {}

impl PartialOrd for OrderedTime {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedTime {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

/// The alarm as the board's record has it now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Alarm {
    /// Whether it stands.
    pub raised: bool,
    /// The work now.
    pub counts: Counts,
    /// Whether review holds more than the ratio allows now.
    pub over: bool,
    /// Since when it has (`over`), or has not: the window's start at the earliest.
    pub since: f64,
}

/// Whether the alarm stood when a replay's window opened ([`replay`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Lowered,
    Raised,
}

/// The alarm at `now`, run over the counts from `since` ([`counted`]): `opened` as it stood at
/// `since`, raised once review held more than the ratio for the raise span, cleared once it held no
/// more for the clear span. Run from each, two replays that agree held one of those spans: the state
/// at `since` did not decide.
pub fn replay(
    thresholds: &Thresholds,
    opened: Standing,
    start: Counts,
    changes: &[(f64, Counts)],
    since: f64,
    now: f64,
) -> Alarm {
    let (raise, clear) = (thresholds.raise_seconds(), thresholds.clear_seconds());
    let mut raised = opened == Standing::Raised;
    let mut counts = start;
    let mut over = thresholds.over(counts);
    let mut run = since;
    let decide = |raised: bool, over: bool, run: f64, at: f64| {
        if over && at - run >= raise {
            true
        } else if !over && at - run >= clear {
            false
        } else {
            raised
        }
    };
    let until_now = changes.iter().filter(|(at, _)| *at <= now);
    for (at, next) in until_now {
        raised = decide(raised, over, run, *at);
        counts = *next;
        let now_over = thresholds.over(counts);
        if now_over != over {
            over = now_over;
            run = *at;
        }
    }
    Alarm {
        raised: decide(raised, over, run, now),
        counts,
        over,
        since: run,
    }
}

/// Since when each task of `timelines` that holds `status` at the end has held it without a break
/// ([`timeline`]): the window's start for one that held it all through.
pub fn held_since<'a>(
    timelines: &HashMap<&'a str, Vec<(f64, Option<&str>)>>,
    status: &str,
) -> HashMap<&'a str, f64> {
    let mut found = HashMap::new();
    for (slug, pieces) in timelines {
        let mut since = None;
        for (at, held) in pieces {
            since = match (since, *held == Some(status)) {
                (None, true) => Some(*at),
                (_, false) => None,
                (kept, true) => kept,
            };
        }
        if let Some(since) = since {
            found.insert(*slug, since);
        }
    }
    found
}

#[cfg(test)]
mod tests;
