//! Rework: what a task's landing waits on, as the tool that lands it records it ([`WAITS`]), and a
//! task in review whose tip now stands judged to need [`CHANGES`] counted as work in progress over
//! the alarm's window ([`reworked`], [`as_worked`]).

use super::Line;

/// The board field in which the tool that lands a task records what its landing waits on
/// ([`crate::red::FIELDS`]`.`[`WAITS`], text, set through `Store::record_waits` with the assessment
/// it states, [`ASSESSED`]): the tip it judged, `: `, then each name, `, ` between them
/// (`0123…: certificate, review`); none after the colon waits on nothing. The board shows the names
/// as they are written, but one ([`CHANGES`]).
pub const WAITS: &str = "waits";
/// A name in [`WAITS`]: a reviewer's verdict on that tip asks for changes, so its executor owes a
/// new one, and a task in review so judged counts as work in progress (its executor at work), not
/// as waiting on review.
pub const CHANGES: &str = "changes";
/// What the line a change of [`WAITS`] writes holds before its text.
pub const WAITS_SET: &str = " set _board.waits to ";
/// The board field beside [`WAITS`] naming the assessment its record states, and when that record
/// was written ([`Assessed`], `Store::record_waits`).
pub const ASSESSED: &str = "assessed";

/// The assessment a record of [`WAITS`] states, as the tool that lands the task names it, and when
/// the record was written (its line's time on the board): a fresh assessment (a new verdict) names
/// another, so its record is one of its own though its text is the same; a retry of one write names
/// the same, and is no new record.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Assessed {
    pub id: String,
    pub at: f64,
    /// The occurrence's unique thread sequence; absent on historical records.
    #[serde(default)]
    pub sid: Option<i64>,
}

impl Assessed {
    /// The one beside the record on `task`, if one is.
    pub fn of(task: &crate::Task) -> Option<Assessed> {
        let board = task.fields.get(crate::red::FIELDS)?;
        let assessed: Self = crate::objects::read(board.get(ASSESSED)?).ok()?;
        (assessed.at.is_finite()
            && !assessed.id.trim().is_empty()
            && assessed.sid.is_none_or(|sid| sid > 0))
        .then_some(assessed)
    }
}

/// A record of [`WAITS`] as one write made it: the tip it judged, when, and its unique thread line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Written<'a> {
    pub tip: &'a str,
    pub at: f64,
    /// The thread sequence identifies this occurrence even when timestamps tie.
    pub sid: i64,
}

impl Written<'_> {
    /// The saved-end key, separate from historical timestamp keys.
    pub(crate) fn key(self) -> String {
        format!("line:{}", self.sid)
    }
}

/// A parsed occurrence, retaining its thread identity across a replay boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct Recorded<'line> {
    /// The unique thread sequence, judged tip and time used for end lookups.
    pub written: Written<'line>,
    /// The names this occurrence recorded.
    pub waits: Waits<'line>,
}

impl<'line> Recorded<'line> {
    /// A genuine waits line with a finite time and positive sequence number.
    pub fn of(line: &'line Line) -> Option<Self> {
        if !line.ts.is_finite() || line.sid <= 0 {
            return None;
        }
        let waits = Waits::parse(waits_set(line)?)?;
        let written = Written {
            tip: waits.tip,
            at: line.ts,
            sid: line.sid,
        };
        Some(Self { written, waits })
    }
}

/// What a task's landing waits on, as [`WAITS`] records it.
#[derive(Debug, Clone, PartialEq)]
pub struct Waits<'a> {
    /// The tip it was judged on.
    pub tip: &'a str,
    /// The names, in the order written.
    pub on: Vec<&'a str>,
}

impl<'a> Waits<'a> {
    /// The record `text` holds: none without a tip before its colon.
    pub fn parse(text: &'a str) -> Option<Waits<'a>> {
        let (tip, on) = text.split_once(':')?;
        let tip = tip.trim();
        if tip.is_empty() || tip.contains(' ') {
            return None;
        }
        let on = (on.split(',').map(str::trim))
            .filter(|name| !name.is_empty())
            .collect();
        Some(Waits { tip, on })
    }

    /// The record on `task`, if one is.
    pub fn of(task: &'a crate::Task) -> Option<Waits<'a>> {
        let board = task.fields.get(crate::red::FIELDS)?;
        Waits::parse(board.get(WAITS)?.as_str()?)
    }

    /// Whether it judged `tip`, the branch's tip now; with none known, it is taken to.
    pub fn judged(&self, tip: Option<&str>) -> bool {
        tip.is_none_or(|tip| tip == self.tip)
    }

    /// Whether it names [`CHANGES`].
    pub fn changes(&self) -> bool {
        self.on.contains(&CHANGES)
    }
}

/// The text a line recording a change of [`WAITS`] set, if it is one: a system line, its writer's
/// handle before [`WAITS_SET`].
pub(crate) fn waits_set(line: &Line) -> Option<&str> {
    let (by, text) = line.text.split_once(WAITS_SET)?;
    (line.kind == "system" && !by.contains(' ')).then_some(text)
}

/// A commit on a branch, its tip or one before: its id, and when it was made (its committer's
/// time, in seconds) or the branch moved to it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Tip {
    pub commit: String,
    pub at: f64,
}

/// The board field in which the board keeps, of each record of [`CHANGES`], the first commit its
/// branch moved to from the tip it judged after it was written, as first learnt
/// (`fields._board.fixed`, `{TIP: {"line:SID": {commit, at}}}`,
/// `Store::record_fixed`): the end of the rework that record asked for, which nothing moves once
/// kept, whatever the branch does after (another commit, an amend, a rebase, a return to that tip
/// and a new record of changes on it, which is a request of its own) and whatever the repository
/// then tells.
pub const FIXED: &str = "fixed";

/// A historical timestamp key in [`FIXED`], adopted only when the thread proves it unambiguous.
pub fn written_key(written: f64) -> String {
    written.to_string()
}

/// The commit `task`'s branch first moved to from the tip `record` judged after it was written, as
/// the board keeps it ([`FIXED`]).
pub fn kept_fix(task: &crate::Task, record: Written<'_>) -> Option<Tip> {
    if record.sid <= 0 || !record.at.is_finite() {
        return None;
    }
    let kept = task
        .fields
        .get(crate::red::FIELDS)?
        .get(FIXED)?
        .get(record.tip)?
        .get(record.key())?;
    let tip: Tip = crate::objects::read(kept).ok()?;
    (tip.at.is_finite() && !tip.commit.trim().is_empty()).then_some(tip)
}

/// What a reader asks of the repository of a task's branch, none when it has none: with no record,
/// its tip now; with a record, the first commit the branch moved to from the tip it judged after it
/// was written (the next made on it, or what replaced it, amended or rebased away), none while the
/// branch has not moved from it since, and the tip now when the repository cannot place it
/// (`cli::probes::tip`). The board keeps the first answer of each record ([`FIXED`]).
pub type Tips<'a> = &'a dyn Fn(&crate::Task, Option<Written<'_>>) -> Option<Tip>;

/// Whether a task's tip stood judged to need [`CHANGES`] from each time on, oldest first, over the
/// window from `since`: each record of [`WAITS`] says so from when it was written until the next
/// (`opened`, the newest written before the window and when, from its start; then each of `lines`,
/// its own in the window). A change of status ends none: a record judged a tip, which a move into
/// progress and back keeps. A record of changes ends earlier when the branch first moved past the
/// tip it judged after the record was written (`fixed`, of that record: when the commit it moved
/// to was made or it moved, which no later commit moves, so two records of one tip end apart),
/// never before the record; where the repository does not tell, the next record ends it.
pub fn reworked(
    lines: &[&Line],
    opened: Option<&Recorded<'_>>,
    fixed: &dyn Fn(Written<'_>) -> Option<f64>,
    since: f64,
) -> Vec<(f64, bool)> {
    // Parsed records borrow their lines. The opening record's names are copied into the combined
    // sequence, whose other records are parsed here, leaving the caller's opening evidence intact.
    let in_window = (lines.iter()).filter_map(|line| {
        waits_set(line)?;
        line.ts.is_finite().then(|| (line.ts, Recorded::of(line)))
    });
    let opening = opened.cloned();
    let records: Vec<(f64, Option<Recorded<'_>>)> =
        std::iter::once((since, opening)).chain(in_window).collect();
    let mut flags = Vec::new();
    let numbered = records.iter().enumerate();
    for (n, (from, record)) in numbered {
        let changes = record.as_ref().filter(|record| record.waits.changes());
        flags.push((*from, changes.is_some()));
        let next = records.get(n + 1).map_or(f64::INFINITY, |(at, _)| *at);
        let ended = changes
            .and_then(|record| fixed(record.written))
            .map(|at| at.max(*from));
        if let Some(ended) = ended.filter(|at| *at < next) {
            flags.push((ended, false));
        }
    }
    flags
}

/// `status`'s timeline as the work counts it: in review while its tip stood judged to need
/// [`CHANGES`] (`reworked`) is in progress. Of two changes at one time, the later written stands.
pub fn as_worked<'a>(
    status: &[(f64, Option<&'a str>)],
    reworked: &[(f64, bool)],
) -> Vec<(f64, Option<&'a str>)> {
    merged(status, reworked, |status, reworked| match status {
        Some("review") if reworked => Some("in_progress"),
        status => status,
    })
}

/// `status`'s timeline read through `flags` from each time on: what `read` makes of the status and
/// the flag that held then. Of two changes at one time, the later written stands.
pub(super) fn merged<'a>(
    status: &[(f64, Option<&'a str>)],
    flags: &[(f64, bool)],
    read: fn(Option<&'a str>, bool) -> Option<&'a str>,
) -> Vec<(f64, Option<&'a str>)> {
    let mut times: Vec<f64> = (status.iter().map(|(at, _)| *at))
        .chain(flags.iter().map(|(at, _)| *at))
        .collect();
    #[expect(
        clippy::disallowed_methods,
        reason = "a busy board path keeps std's in-place sort (crate::order): PULL's alarms"
    )]
    times.sort_unstable_by(f64::total_cmp);
    times.dedup();
    let held = |at: f64| {
        let status = (status.iter().rev()).find(|(from, _)| *from <= at);
        let flag = (flags.iter().rev()).find(|(from, _)| *from <= at);
        read(status.and_then(|(_, s)| *s), flag.is_some_and(|(_, f)| *f))
    };
    let mut pieces: Vec<(f64, Option<&'a str>)> = Vec::new();
    for at in times {
        let now = held(at);
        if pieces.last().is_none_or(|(_, last)| *last != now) {
            pieces.push((at, now));
        }
    }
    pieces
}

#[cfg(test)]
mod tests;
