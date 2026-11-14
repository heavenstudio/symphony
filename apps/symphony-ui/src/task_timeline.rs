//! A task's timeline: what it went through, read from `/api/task-timeline`
//! ([`symphony_api::timeline`]). It is a page kind of its own (`page-timeline`), which the task
//! page never compiles: the page draws the box it goes in ([`crate::timeline_slot`]) and asks the
//! shell for it, and the shell loads this module once the page has its data and is idle, then
//! moves its root into the box. A task page and `/agents` pay for none of it on a first visit.
//!
//! - On a phone, a list ([`rows`]): a row per step with its start, its label, how long it took and
//!   a bar as long, its wait drawn lighter; the builds, tests and gate changes indented under the
//!   stage they happened in; the verdict after a review's bar, and a striped bar for what runs now.
//! - Wide, lanes over the same hours ([`lanes`]): the stage, the executor, the reviewer, builds and
//!   tests (a row each for runs that overlap), and a thin row per gate, its state drawn by a
//!   pattern of its own as well as a colour, with a line at now. Each segment is a stop of one
//!   arrow-key group holding its words, and the list stays for assistive technology.
//!
//! The box keeps the height the page gave it: the latest steps that fit show, a read that adds a
//! step moves nothing outside it, and "Show all" lets it grow. The module reads the timeline when
//! its task is asked for, again when the task changes, and each `every` seconds while the task is
//! open and its page shown, asking with the validator of the last answer so an unchanged timeline
//! is a `304`.

use crate::words::plural;
use dioxus::prelude::*;
use std::rc::Rc;
use symphony_api::gates::REVIEW;
use symphony_api::timeline::{Run, RunResult, Stage, Verdict};
use symphony_api::{GateState, TaskTimeline};

/// The timeline's style sheet, the module's own: the shell loads it with the module.
#[cfg(feature = "page-timeline")]
const STYLE: Asset = asset!(
    "/assets/task-timeline.css",
    AssetOptions::css().with_static_head(true)
);

/// How much of a row the longest step's bar takes on a phone, in percent: the rest is for the word
/// after it.
const BAR_SPAN: f64 = 72.0;

/// A character of the lanes' small type, about, in pixels; then the padding a segment's words take
/// beside them, and the gap a label keeps from the next. A word the estimate lets through that is
/// wider still is cut short with an ellipsis at its room's end, never drawn over the next.
const CHAR_PX: f64 = 6.0;
const PAD_PX: f64 = 12.0;
const GAP_PX: f64 = 4.0;
/// The lanes' margin after the track's end (`--end`, 5.5rem), which the last label may take.
const END_PX: f64 = 88.0;

/// The track widths a word's room is weighed at, in pixels, every 50 from 400 to 1600: a word
/// shows from the first of them its room holds it at (`tl-from-W`, `tl-to-W` in the style sheet),
/// at every width when 300 does, and never when 1600 does not.
const WIDTHS: [u32; 25] = [
    400, 450, 500, 550, 600, 650, 700, 750, 800, 850, 900, 950, 1000, 1050, 1100, 1150, 1200, 1250,
    1300, 1350, 1400, 1450, 1500, 1550, 1600,
];

/// How many rows the builds and tests lane takes at most: a run that overlaps runs in all of them
/// shares the row that frees first, its label left out.
const RUN_ROWS: usize = 3;

/// The hours between the wide view's ticks: the first that draws no more than eight.
const TICK_HOURS: [f64; 10] = [1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 24.0, 72.0, 168.0];

/// Statuses a task ends in: an instant on the timeline, with no time of its own.
const CLOSED: [&str; 2] = ["done", "cancelled"];

/// How long `seconds` is, to the minute: "41 m", "1 h 52 m", "2 d 3 h".
pub fn took(seconds: f64) -> String {
    let minutes = (seconds.max(0.0) / 60.0).round() as i64;
    let (days, hours, rest) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    match (days, hours, rest) {
        (0, 0, _) => format!("{rest} m"),
        (0, _, 0) => format!("{hours} h"),
        (0, _, _) => format!("{hours} h {rest} m"),
        (_, 0, _) => format!("{days} d"),
        _ => format!("{days} d {hours} h"),
    }
}

/// Minutes since 1970 on the server's local clock, `offset` seconds ahead of `at`.
fn local(at: f64, offset: i64) -> i64 {
    ((at + offset as f64) / 60.0).floor() as i64
}

/// `at` on the server's local clock: "09:02".
pub fn clock(at: f64, offset: i64) -> String {
    let minutes = local(at, offset).rem_euclid(1440);
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// The local day of `at`, month and day: "11-10".
pub fn date(at: f64, offset: i64) -> String {
    let (_, month, day) = civil(local(at, offset).div_euclid(1440));
    format!("{month:02}-{day:02}")
}

/// `at` on the server's clock as it read then, its offset that instant's (a change of summer time
/// between two steps reads each at its own).
fn clock_of(timeline: &TaskTimeline, at: f64) -> String {
    clock(at, timeline.offset_at(at))
}

fn date_of(timeline: &TaskTimeline, at: f64) -> String {
    date(at, timeline.offset_at(at))
}

/// The date `days` after 1970-01-01 in the proleptic Gregorian calendar (Howard Hinnant's
/// `civil_from_days`), the inverse of `words::Moment::from_parts`.
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `word` with a capital first letter and spaces for underscores: "In progress".
fn capitalised(word: &str) -> String {
    let word = word.replace('_', " ");
    let mut letters = word.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

/// The theme token a board status is drawn in.
fn status_tone(status: &str) -> &'static str {
    match status {
        "ready" => "auto",
        "in_progress" => "primary",
        "review" => "question",
        "done" => "like",
        "blocked" => "dislike",
        "cancelled" => "resolved",
        "concept" | "design" => "decision",
        _ => "muted-foreground",
    }
}

fn result_tone(result: RunResult) -> &'static str {
    match result {
        RunResult::Green => "like",
        RunResult::Red => "dislike",
        RunResult::Queued | RunResult::Running => "primary",
        RunResult::Unknown => "resolved",
    }
}

fn gate_tone(state: GateState) -> &'static str {
    match state {
        GateState::Met => "like",
        GateState::Missing => "question",
        GateState::Waived => "resolved",
        GateState::Held => "dislike",
        GateState::Unknown => "muted-foreground",
    }
}

fn verdict_tone(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Approved => "like",
        Verdict::Changes => "dislike",
        Verdict::Other => "muted-foreground",
    }
}

fn closed(stage: &Stage) -> bool {
    CLOSED.contains(&stage.status.as_str())
}

/// `label`, then " · `by`" when someone is named.
fn with(label: impl Into<String>, by: Option<&str>) -> String {
    let label = label.into();
    match by {
        Some(by) if !by.is_empty() => format!("{label} · {by}"),
        _ => label,
    }
}

/// A commit as a label names it: its first seven characters.
fn short_commit(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

/// The heading's totals: how long the task has taken, its rounds and bounces; then, once a run is
/// kept for it, the machine's time on its runs and their wait to start.
pub fn totals(timeline: &TaskTimeline) -> (String, Option<String>) {
    let length = took(timeline.end() - timeline.created);
    let length = if timeline.closed.is_some() {
        format!("{length} in all")
    } else {
        format!("{length} so far")
    };
    let rounds = u32::try_from(timeline.rounds.len()).unwrap_or(u32::MAX);
    let first = format!(
        "{length} · {rounds} {} · {} {}",
        plural(rounds, "review round"),
        timeline.bounces,
        plural(timeline.bounces, "bounce")
    );
    let machine = (!timeline.runs.is_empty()).then(|| {
        format!(
            "{}, queued {}",
            took(timeline.machine_seconds()),
            took(timeline.queued_seconds())
        )
    });
    (first, machine)
}

/// What the timeline cannot show: the runs the recorder's rotated history let go, before
/// `runs_from`.
pub fn kept_note(timeline: &TaskTimeline) -> Option<String> {
    timeline.runs_from.map(|from| {
        format!(
            "Builds and tests before {} {} are no longer kept.",
            date_of(timeline, from),
            clock_of(timeline, from)
        )
    })
}

/// One row of the phone's list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// When it began, server seconds.
    pub start: f64,
    /// The local day, "11-10", on the first row of each day of a timeline that spans more than one.
    pub day: Option<String>,
    /// When it began on the server's local clock, "09:02".
    pub at: String,
    /// A build, a test or a gate change, under the stage it happened in.
    pub sub: bool,
    /// A gate change: an instant, a diamond with no bar.
    pub event: bool,
    pub tone: &'static str,
    pub label: String,
    /// How long it took; none for an instant.
    pub took: Option<String>,
    /// Its bar; none for an instant.
    pub bar: Option<Bar>,
    /// The word after the bar (a verdict, a run's result, "now") and its tone.
    pub word: Option<(&'static str, &'static str)>,
    /// The whole of it, for a pointer resting on the row.
    pub title: String,
}

/// A row's bar: its wait, drawn lighter in `wait_tone`, then its work, each a percentage of the row
/// as long as its share of the longest step's.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub wait: f64,
    pub wait_tone: &'static str,
    pub work: f64,
    /// The work goes on now: striped.
    pub running: bool,
}

/// Who held the executor role while `stage` went on: the first claim over it.
fn claimer<'a>(timeline: &'a TaskTimeline, stage: &Stage) -> Option<&'a str> {
    let end = stage.to.unwrap_or(timeline.now);
    timeline
        .claims
        .iter()
        .find(|claim| claim.from < end && claim.to.unwrap_or(timeline.now) > stage.from)
        .map(|claim| claim.by.as_str())
}

/// What the list says of the stages in progress before this one: whether there was one, and who
/// held it last.
#[derive(Default)]
struct Worked {
    before: bool,
    by: Option<String>,
}

fn stage_row(
    timeline: &TaskTimeline,
    stage: &Stage,
    worked: &mut Worked,
    share: &dyn Fn(f64) -> f64,
) -> Row {
    let now = timeline.now;
    let end = stage.to.unwrap_or(now);
    let ended = closed(stage);
    let tone = status_tone(&stage.status);
    let round = timeline
        .rounds
        .iter()
        .find(|round| stage.round > 0 && round.number == stage.round);
    let label = match stage.status.as_str() {
        "ready" if timeline.first_claimed.is_none_or(|claimed| end <= claimed) => {
            "Ready, unclaimed".to_string()
        }
        "in_progress" => {
            let by = claimer(timeline, stage);
            let label = if worked.before {
                with(
                    "In progress again",
                    by.filter(|by| worked.by.as_deref() != Some(*by)),
                )
            } else {
                with("In progress", by)
            };
            worked.before = true;
            worked.by = by.map(str::to_string);
            label
        }
        "review" if stage.approved => "Approved, waiting to land".to_string(),
        "review" if stage.round > 0 => with(
            format!("Review, round {}", stage.round),
            round.and_then(|round| round.by.as_deref()),
        ),
        status => with(
            capitalised(status),
            (ended || status == "blocked")
                .then_some(stage.by.as_deref())
                .flatten(),
        ),
    };
    let seconds = stage.seconds(now);
    let bar = (!ended).then(|| match round {
        _ if stage.approved => Bar {
            wait: share(seconds),
            wait_tone: tone,
            work: 0.0,
            running: false,
        },
        Some(round) => {
            let taken = round.taken.unwrap_or(end).clamp(stage.from, end);
            Bar {
                wait: share(taken - stage.from),
                wait_tone: tone,
                work: share(end - taken),
                running: false,
            }
        }
        None => Bar {
            wait: 0.0,
            wait_tone: tone,
            work: share(seconds),
            running: false,
        },
    });
    let word = match round.and_then(|round| round.verdict) {
        Some(verdict) if !stage.approved => Some((verdict.as_str(), verdict_tone(verdict))),
        _ if stage.to.is_none() && !ended => Some(("now", "muted-foreground")),
        _ => None,
    };
    let until = stage.to.map_or_else(
        || " until now".to_string(),
        |to| format!(" to {}", clock_of(timeline, to)),
    );
    let moved = stage
        .by
        .as_deref()
        .map(|by| format!(", moved there by {by}"))
        .unwrap_or_default();
    Row {
        start: stage.from,
        day: None,
        at: clock_of(timeline, stage.from),
        sub: false,
        event: false,
        tone,
        title: format!(
            "{label}: from {}{until}{moved}",
            clock_of(timeline, stage.from)
        ),
        label,
        took: (!ended).then(|| took(seconds)),
        bar,
        word,
    }
}

/// A run's label in the list: the executor's own as its phase and recipe, with its wait to start
/// ("Test, just verify · queued 6 m", a build as "Build"); a run nobody holds (a gate's, on the
/// machine) as what it runs and the commit it ran at ("Certificate verify · e19d3a0").
fn run_label(run: &Run, waited: f64) -> String {
    let queued = |label: String| {
        if waited >= 60.0 {
            format!("{label} · queued {}", took(waited))
        } else {
            label
        }
    };
    match (run.phase.as_str(), run.by.as_deref()) {
        (_, None) => with(
            capitalised(&run.label),
            run.commit.as_deref().map(short_commit),
        ),
        ("build", Some(_)) => queued("Build".into()),
        (phase, Some(_)) => queued(format!("{}, {}", capitalised(phase), run.label)),
    }
}

fn run_row(run: &Run, timeline: &TaskTimeline, share: &dyn Fn(f64) -> f64) -> Row {
    let now = timeline.now;
    let (waited, ran) = (run.waited(now), run.ran(now));
    let tone = result_tone(run.result);
    let mut title = format!("{}: {}", run.label, run.result.as_str());
    for (word, value) in [
        ("at", run.commit.as_deref()),
        (
            "on",
            Some(run.host.as_str()).filter(|host| !host.is_empty()),
        ),
        ("by", run.by.as_deref()),
    ] {
        if let Some(value) = value {
            title.push_str(&format!(" {word} {value}"));
        }
    }
    if waited >= 60.0 {
        title.push_str(&format!(", queued {}", took(waited)));
    }
    // A build that went green says nothing more than that it was built.
    let word = (run.phase != "build" || run.result != RunResult::Green)
        .then(|| (run.result.as_str(), tone));
    Row {
        start: run.queued,
        day: None,
        at: clock_of(timeline, run.queued),
        sub: true,
        event: false,
        tone,
        label: run_label(run, waited),
        took: Some(took(if run.started.is_some() { ran } else { waited })),
        bar: Some(Bar {
            wait: share(waited),
            wait_tone: "primary",
            work: share(ran),
            running: run.result == RunResult::Running,
        }),
        word,
        title,
    }
}

/// What a gate change's detail says moved it, when it opens with a short reason before a colon:
/// "Main moved" of "Main moved: verify on the merge tree 4be91c2 running".
fn reason(detail: &str) -> Option<&str> {
    let (reason, _) = detail.split_once(": ")?;
    (!reason.is_empty() && reason.split_whitespace().count() <= 3).then_some(reason)
}

/// Each change of a gate's state after its first, but the review gate's, whose rounds say it, and
/// a gate met by a run that went green then, whose row says it.
fn gate_changes(timeline: &TaskTimeline) -> Vec<Row> {
    let green_at = |at: f64| {
        timeline.runs.iter().any(|run| {
            run.result == RunResult::Green
                && run.ended.is_some_and(|ended| (ended - at).abs() <= 60.0)
        })
    };
    let mut rows = Vec::new();
    for lane in timeline.gates.iter().filter(|lane| lane.name != REVIEW) {
        for pair in lane.states.windows(2) {
            let (before, span) = (&pair[0], &pair[1]);
            if span.state == GateState::Met && green_at(span.from) {
                continue;
            }
            let name = capitalised(&lane.name);
            let label = match span.state {
                GateState::Missing if before.state == GateState::Met => {
                    match reason(&span.detail) {
                        Some(reason) => format!("{reason}: {} again", lane.name),
                        None => format!("{name} missing again"),
                    }
                }
                GateState::Met => format!("{name} met"),
                GateState::Missing => format!("{name} missing"),
                GateState::Waived => with(format!("{name} waived"), span.by.as_deref()),
                GateState::Held => with(format!("{name} held"), span.by.as_deref()),
                GateState::Unknown => format!("{name} unknown"),
            };
            rows.push(Row {
                start: span.from,
                day: None,
                at: clock_of(timeline, span.from),
                sub: true,
                event: true,
                tone: gate_tone(span.state),
                title: if span.detail.is_empty() {
                    label.clone()
                } else {
                    span.detail.clone()
                },
                label,
                took: None,
                bar: None,
                word: None,
            });
        }
    }
    rows
}

/// The phone's list: each stage, and under it the builds, tests and gate changes that began while
/// it went on, by time. A stage left before a minute was out (one that would read "0 m") has no
/// row: what began in it goes under the stage before.
pub fn rows(timeline: &TaskTimeline) -> Vec<Row> {
    let now = timeline.now;
    let longest = timeline
        .stages
        .iter()
        .filter(|stage| !closed(stage))
        .map(|stage| stage.seconds(now))
        .chain(
            timeline
                .runs
                .iter()
                .map(|run| run.waited(now) + run.ran(now)),
        )
        .fold(60.0, f64::max);
    let share = |seconds: f64| seconds.max(0.0) / longest * BAR_SPAN;
    let mut under: Vec<Row> = timeline
        .runs
        .iter()
        .map(|run| run_row(run, timeline, &share))
        .chain(gate_changes(timeline))
        .collect();
    under.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut under = under.into_iter().peekable();
    let mut rows = Vec::new();
    let mut worked = Worked::default();
    for (index, stage) in timeline.stages.iter().enumerate() {
        let row = stage_row(timeline, stage, &mut worked, &share);
        let fleeting = !closed(stage) && stage.to.is_some() && stage.seconds(now) < 30.0;
        if !fleeting {
            rows.push(row);
        }
        let next = timeline.stages.get(index + 1).map(|next| next.from);
        while let Some(row) = under.next_if(|row| next.is_none_or(|next| row.start < next)) {
            rows.push(row);
        }
    }
    rows.extend(under);
    if date_of(timeline, timeline.created) != date_of(timeline, timeline.end()) {
        let mut last = None;
        for row in &mut rows {
            let day = date_of(timeline, row.start);
            if last.as_ref() != Some(&day) {
                row.day = Some(day.clone());
                last = Some(day);
            }
        }
    }
    rows
}

/// How a lane's segment is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// Worked or ran.
    Solid,
    /// Queued or waiting: lighter, its edge dashed.
    Waiting,
    /// Running now: striped.
    Running,
}

impl Look {
    const fn class(self) -> &'static str {
        match self {
            Self::Solid => "tl-seg",
            Self::Waiting => "tl-seg tl-waiting",
            Self::Running => "tl-seg tl-running",
        }
    }
}

/// Words drawn only at the track widths that hold them: from `from` pixels wide (always when
/// `None`) and below `to` (always when `None`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fit {
    pub text: String,
    pub from: Option<u32>,
    pub to: Option<u32>,
}

impl Fit {
    /// The classes that show it at the widths that hold it.
    fn class(&self) -> String {
        let mut class = String::from("tl-fit");
        if let Some(from) = self.from {
            class.push_str(&format!(" tl-from-{from}"));
        }
        if let Some(to) = self.to {
            class.push_str(&format!(" tl-to-{to}"));
        }
        class
    }
}

/// The first of [`WIDTHS`] at which `text`, with `pad` pixels beside it, fits in `room` percent of
/// the track: `Some(None)` at every width, `None` at none.
fn width_for(text: &str, room: f64, pad: f64) -> Option<Option<u32>> {
    if text.is_empty() || room <= 0.0 {
        return None;
    }
    let needed = (text.chars().count() as f64 * CHAR_PX + pad) * 100.0 / room;
    if needed <= f64::from(WIDTHS[0] - 100) {
        return Some(None);
    }
    WIDTHS
        .iter()
        .find(|width| f64::from(**width) >= needed)
        .map(|width| Some(*width))
}

/// Room a label beside a segment has: `room` percent of the track before the next, and the lanes'
/// end margin past the track's end when nothing follows it (`last`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Room {
    pub room: f64,
    pub last: bool,
}

impl Room {
    /// Its width, for the label's `max-width`: past it a label is cut short, never drawn over the
    /// next.
    fn css(self) -> String {
        if self.last {
            format!("calc({:.2}% + var(--end))", self.room)
        } else {
            format!("{:.2}%", self.room)
        }
    }
}

/// `text` as a label beside a segment, in `room`.
fn label(text: String, room: Room) -> Option<Fit> {
    let end = if room.last { END_PX } else { 0.0 };
    let pad = GAP_PX - end;
    if room.last && text.chars().count() as f64 * CHAR_PX + pad <= 0.0 {
        return Some(Fit {
            text,
            from: None,
            to: None,
        });
    }
    width_for(&text, room.room, pad).map(|from| Fit {
        text,
        from,
        to: None,
    })
}

/// `long` where it fits in a segment `room` percent wide, else `short` where that fits: each drawn
/// only at the widths that hold it.
fn fitted(long: &str, short: &str, room: f64) -> Vec<Fit> {
    let width_for = |text: &str, room: f64| width_for(text, room, PAD_PX);
    let mut fits = Vec::new();
    let long_from = width_for(long, room);
    if let Some(from) = long_from {
        fits.push(Fit {
            text: long.to_string(),
            from,
            to: None,
        });
    }
    if long_from == Some(None) || short.is_empty() || short == long {
        return fits;
    }
    if let Some(from) = width_for(short, room) {
        let to = long_from.flatten();
        if from.zip(to).is_none_or(|(from, to)| from < to) {
            fits.push(Fit {
                text: short.to_string(),
                from,
                to,
            });
        }
    }
    fits
}

/// A segment of a lane, placed in percent of the hours drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Seg {
    pub left: f64,
    pub width: f64,
    pub tone: &'static str,
    pub look: Look,
    /// Its words, at the widths that hold them.
    pub text: Vec<Fit>,
    /// A word right after it (a review's verdict) and its tone, where there is room before the
    /// next, and that room in percent: it is cut short there, never drawn over the next.
    pub after: Option<(Fit, &'static str, Room)>,
    /// A label under it (a run's), where there is room before the next in its row, and that room.
    pub under: Option<(Fit, Room)>,
    /// The row of its lane it is drawn in (runs that overlap take a row each).
    pub row: usize,
    /// All of it, for a pointer resting on it and for assistive technology.
    pub title: String,
    /// Whether the keyboard stops at it: a part the rest of its step follows (a run's queue before
    /// its run, a round's wait before its reviewer's time) is only drawn, the part after it
    /// saying all of it.
    pub stop: bool,
}

/// A lane of the wide view: its name, who worked in it, its segments.
#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    /// The lane's key, for its row's `data-lane`.
    pub key: &'static str,
    pub name: &'static str,
    pub who: String,
    /// How many rows its segments take.
    pub rows: usize,
    pub segs: Vec<Seg>,
}

/// A gate's state over the hours it held, placed in percent of the hours drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct GateBar {
    pub left: f64,
    pub width: f64,
    pub state: GateState,
    /// Its state's word, at the widths that hold it.
    pub word: Option<Fit>,
    pub title: String,
}

/// A gate's thin row.
#[derive(Clone, Debug, PartialEq)]
pub struct GateRow {
    pub name: String,
    pub spans: Vec<GateBar>,
}

/// An hour marked on the wide view's axis, in percent of the hours drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    pub left: f64,
    pub label: String,
}

/// The wide view: the hours, a line at now while the task is open, the lanes and the gates' rows.
#[derive(Clone, Debug, PartialEq)]
pub struct Lanes {
    pub ticks: Vec<Tick>,
    pub now: Option<Tick>,
    pub lanes: Vec<Lane>,
    pub gates: Vec<GateRow>,
}

impl Lanes {
    /// How many parts of it a keyboard stops at: each step's segment, then each gate's state.
    pub fn stops(&self) -> usize {
        self.lanes
            .iter()
            .map(|lane| lane.segs.iter().filter(|seg| seg.stop).count())
            .sum::<usize>()
            + self
                .gates
                .iter()
                .map(|gate| gate.spans.len())
                .sum::<usize>()
    }
}

/// The hours drawn: from the local hour the task was made in to the timeline's end, now beside
/// them.
struct Scale<'a> {
    timeline: &'a TaskTimeline,
    start: f64,
    span: f64,
    end: f64,
    now: f64,
}

impl<'a> Scale<'a> {
    fn of(timeline: &'a TaskTimeline) -> Scale<'a> {
        let created = timeline.created;
        let offset = timeline.offset_at(created);
        let start = created - (created + offset as f64).rem_euclid(3600.0);
        let end = timeline.end();
        Scale {
            timeline,
            start,
            span: (end - start).max(60.0),
            end,
            now: timeline.now,
        }
    }

    fn at(&self, at: f64) -> f64 {
        ((at - self.start) / self.span * 100.0).clamp(0.0, 100.0)
    }

    fn width(&self, from: f64, to: f64) -> f64 {
        self.at(to) - self.at(from)
    }

    fn clock(&self, at: f64) -> String {
        clock_of(self.timeline, at)
    }

    /// The hours marked: the step's multiples from local midnight (midnight itself for a step of a
    /// day or more), so a change of day is always marked, by its date; while the task is `open`,
    /// none so near now that its label would meet now's.
    fn ticks(&self, open: bool) -> Vec<Tick> {
        let step = TICK_HOURS
            .iter()
            .map(|hours| hours * 3600.0)
            .find(|step| self.span / step <= 8.0)
            .unwrap_or(168.0 * 3600.0);
        let every = (step / 60.0) as i64;
        let first = local(self.start, self.timeline.offset_at(self.start));
        let mut tick = self.start + ((-first).rem_euclid(every.min(1440))) as f64 * 60.0;
        let mut ticks = Vec::new();
        while tick <= self.end {
            let left = self.at(tick);
            if !open || self.at(self.now) - left >= 4.0 {
                let offset = self.timeline.offset_at(tick);
                let label = if local(tick, offset).rem_euclid(1440) == 0 {
                    date(tick, offset)
                } else {
                    clock(tick, offset)
                };
                ticks.push(Tick { left, label });
            }
            tick += step;
        }
        ticks
    }
}

/// The names in `names`, each once, in the order first met.
fn each_once<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for name in names {
        if !name.is_empty() && !seen.contains(&name) {
            seen.push(name);
        }
    }
    seen.join(", ")
}

/// A run's name under its bar: "build", the recipe's first word ("verify" for `just verify`), or
/// "running" while it runs; then its result, unless it went green.
fn short(run: &Run) -> String {
    let name = if run.result == RunResult::Running {
        "running".to_string()
    } else if run.phase == "build" {
        "build".to_string()
    } else {
        let label = run.label.strip_prefix("just ").unwrap_or(&run.label);
        label.split_whitespace().next().unwrap_or(label).to_string()
    };
    match run.result {
        RunResult::Red | RunResult::Unknown | RunResult::Queued => {
            format!("{name} {}", run.result.as_str())
        }
        RunResult::Green | RunResult::Running => name,
    }
}

/// The stage lane: each stretch the task held a status, the wait to land drawn as waiting; a close
/// is an instant, not drawn.
fn stage_segs(timeline: &TaskTimeline, scale: &Scale) -> Vec<Seg> {
    timeline
        .stages
        .iter()
        .filter(|stage| !closed(stage))
        .filter_map(|stage| {
            let to = stage.to.unwrap_or(scale.now);
            let width = scale.width(stage.from, to);
            (width > 0.0).then(|| {
                let word = if stage.approved {
                    "waiting to land".to_string()
                } else {
                    stage.status.replace('_', " ")
                };
                let length = took(to - stage.from);
                Seg {
                    left: scale.at(stage.from),
                    width,
                    tone: status_tone(&stage.status),
                    look: if stage.approved {
                        Look::Waiting
                    } else {
                        Look::Solid
                    },
                    text: fitted(&format!("{word} {length}"), &length, width),
                    after: None,
                    under: None,
                    row: 0,
                    title: format!(
                        "{word} {length}, from {} to {}",
                        scale.clock(stage.from),
                        scale.clock(to)
                    ),
                    stop: true,
                }
            })
        })
        .collect()
}

/// The executor lane: each claim, the first saying so where one executor held them all, each
/// naming its executor where several did.
fn claim_segs(timeline: &TaskTimeline, scale: &Scale, one: bool) -> Vec<Seg> {
    timeline
        .claims
        .iter()
        .enumerate()
        .map(|(index, claim)| {
            let to = claim.to.unwrap_or(scale.now);
            let width = scale.width(claim.from, to);
            let length = took(to - claim.from);
            let word = match (one, index) {
                (true, 0) => "claimed",
                (true, _) => "",
                (false, _) => claim.by.as_str(),
            };
            let long = if word.is_empty() {
                length.clone()
            } else {
                format!("{word} {length}")
            };
            Seg {
                left: scale.at(claim.from),
                width,
                tone: "primary",
                look: Look::Solid,
                text: fitted(&long, &length, width),
                after: None,
                under: None,
                row: 0,
                title: format!(
                    "{} held it {length}, from {} to {}",
                    claim.by,
                    scale.clock(claim.from),
                    scale.clock(to)
                ),
                stop: true,
            }
        })
        .collect()
}

/// The reviewer lane: each round's wait for a reviewer, drawn as waiting, then the reviewer's
/// time on it, its verdict after it where there is room before the next round.
fn round_segs(timeline: &TaskTimeline, scale: &Scale) -> Vec<Seg> {
    let mut segs = Vec::new();
    for (index, round) in timeline.rounds.iter().enumerate() {
        let to = round.to.unwrap_or(scale.now);
        let taken = round.taken.map(|taken| taken.clamp(round.from, to));
        let next = timeline.rounds.get(index + 1);
        let room = Room {
            room: next.map_or(100.0, |next| scale.at(next.from)) - scale.at(to),
            last: next.is_none(),
        };
        let after = round.verdict.and_then(|verdict| {
            let word = label(verdict.as_str().to_string(), room)?;
            Some((word, verdict_tone(verdict), room))
        });
        let name = format!("round {}", round.number);
        let by = round.by.as_deref().unwrap_or("nobody yet");
        let waited = took(taken.unwrap_or(to) - round.from);
        let verdict = round
            .verdict
            .map(|verdict| format!(", {}", verdict.as_str()))
            .unwrap_or_default();
        let title = match taken {
            Some(taken) => format!(
                "{name}: waited {waited} for a reviewer, then {by} took {}{verdict}",
                took(to - taken)
            ),
            None => format!("{name}: waited {waited} for a reviewer{verdict}"),
        };
        let wait = scale.width(round.from, taken.unwrap_or(to));
        if wait > 0.0 || taken.is_none() {
            segs.push(Seg {
                left: scale.at(round.from),
                width: wait,
                tone: "question",
                look: Look::Waiting,
                text: if taken.is_none() {
                    fitted(&name, "", wait)
                } else {
                    Vec::new()
                },
                after: if taken.is_none() { after.clone() } else { None },
                under: None,
                row: 0,
                title: title.clone(),
                stop: taken.is_none(),
            });
        }
        if let Some(taken) = taken {
            let width = scale.width(taken, to);
            segs.push(Seg {
                left: scale.at(taken),
                width,
                tone: "question",
                look: Look::Solid,
                text: fitted(&name, "", width),
                after,
                under: None,
                row: 0,
                title,
                stop: true,
            });
        }
    }
    segs
}

/// The rows of the builds and tests lane: each run in the first row free by its queue (runs that
/// overlap take a row each), at most [`RUN_ROWS`]; one that finds none shares the row that frees
/// first.
fn run_rows(runs: &[Run], now: f64) -> (Vec<usize>, usize) {
    let mut ends: Vec<f64> = Vec::new();
    let rows = runs
        .iter()
        .map(|run| {
            let end = run.ended.unwrap_or(now);
            let row = match ends.iter().position(|free| *free <= run.queued) {
                Some(row) => row,
                None if ends.len() < RUN_ROWS => {
                    ends.push(f64::NEG_INFINITY);
                    ends.len() - 1
                }
                None => ends
                    .iter()
                    .enumerate()
                    .min_by(|a, b| a.1.total_cmp(b.1))
                    .map_or(0, |(row, _)| row),
            };
            ends[row] = ends[row].max(end);
            row
        })
        .collect();
    (rows, ends.len().max(1))
}

/// The builds and tests lane: each run's queue, drawn as waiting with its name under it where
/// there is room before the next in its row, then its run in its result's tone, striped while it
/// runs.
fn run_segs(timeline: &TaskTimeline, scale: &Scale) -> (Vec<Seg>, usize) {
    let now = scale.now;
    let (rows, count) = run_rows(&timeline.runs, now);
    let mut segs = Vec::new();
    for (index, run) in timeline.runs.iter().enumerate() {
        let row = rows[index];
        let started = run.started.or(run.ended).unwrap_or(now);
        let next = timeline.runs[index + 1..]
            .iter()
            .zip(&rows[index + 1..])
            .find(|(_, next)| **next == row);
        let room = Room {
            room: next.map_or(100.0, |(next, _)| scale.at(next.queued)) - scale.at(run.queued),
            last: next.is_none(),
        };
        let length = took(if run.started.is_some() {
            run.ran(now)
        } else {
            run.waited(now)
        });
        let title = format!(
            "{}{}: queued {}, ran {}, {}",
            run.label,
            run.commit
                .as_deref()
                .map(|commit| format!(" at {commit}"))
                .unwrap_or_default(),
            took(run.waited(now)),
            took(run.ran(now)),
            run.result.as_str()
        );
        let under = format!("{} {length}", short(run));
        segs.push(Seg {
            left: scale.at(run.queued),
            width: scale.width(run.queued, started),
            tone: "primary",
            look: Look::Waiting,
            text: Vec::new(),
            after: None,
            under: label(under, room).map(|under| (under, room)),
            row,
            title: title.clone(),
            stop: run.started.is_none(),
        });
        if let Some(begun) = run.started {
            segs.push(Seg {
                left: scale.at(begun),
                width: scale.width(begun, run.ended.unwrap_or(now)),
                tone: result_tone(run.result),
                look: if run.result == RunResult::Running {
                    Look::Running
                } else {
                    Look::Solid
                },
                text: Vec::new(),
                after: None,
                under: None,
                row,
                title,
                stop: true,
            });
        }
    }
    (segs, count)
}

/// Each gate's thin row: each state from when it was taken to the next, the last to the end, its
/// word where it fits.
fn gate_rows(timeline: &TaskTimeline, scale: &Scale) -> Vec<GateRow> {
    timeline
        .gates
        .iter()
        .map(|lane| GateRow {
            name: lane.name.clone(),
            spans: lane
                .states
                .iter()
                .enumerate()
                .filter_map(|(index, span)| {
                    let to = lane
                        .states
                        .get(index + 1)
                        .map_or(scale.end, |next| next.from);
                    let width = scale.width(span.from, to);
                    (width > 0.0).then(|| {
                        let state = span.state.as_str();
                        let detail = if span.detail.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", span.detail)
                        };
                        GateBar {
                            left: scale.at(span.from),
                            width,
                            state: span.state,
                            word: width_for(state, width, PAD_PX).map(|from| Fit {
                                text: state.to_string(),
                                from,
                                to: None,
                            }),
                            title: format!(
                                "{} {state} from {}{detail}",
                                lane.name,
                                scale.clock(span.from)
                            ),
                        }
                    })
                })
                .collect(),
        })
        .collect()
}

/// The wide view of `timeline`: the stage lane always, the executor's, the reviewer's and the
/// builds and tests' once there is something to draw in them, and a row per gate.
pub fn lanes(timeline: &TaskTimeline) -> Lanes {
    let scale = Scale::of(timeline);
    let open = timeline.closed.is_none();
    let executors = each_once(timeline.claims.iter().map(|claim| claim.by.as_str()));
    let reviewers = each_once(
        timeline
            .rounds
            .iter()
            .filter_map(|round| round.by.as_deref()),
    );
    let hosts = each_once(timeline.runs.iter().map(|run| run.host.as_str()));
    let one = !executors.contains(", ");
    let mut lanes = vec![Lane {
        key: "stage",
        name: "Stage",
        who: String::new(),
        rows: 1,
        segs: stage_segs(timeline, &scale),
    }];
    if !timeline.claims.is_empty() {
        lanes.push(Lane {
            key: "executor",
            name: "Executor",
            who: executors,
            rows: 1,
            segs: claim_segs(timeline, &scale, one),
        });
    }
    if !timeline.rounds.is_empty() {
        lanes.push(Lane {
            key: "reviewer",
            name: "Reviewer",
            who: reviewers,
            rows: 1,
            segs: round_segs(timeline, &scale),
        });
    }
    if !timeline.runs.is_empty() {
        let (segs, rows) = run_segs(timeline, &scale);
        lanes.push(Lane {
            key: "runs",
            name: "Builds & tests",
            who: hosts,
            rows,
            segs,
        });
    }
    Lanes {
        ticks: scale.ticks(open),
        now: open.then(|| Tick {
            left: scale.at(scale.now),
            label: format!("now {}", scale.clock(scale.now)),
        }),
        lanes,
        gates: gate_rows(timeline, &scale),
    }
}

/// The timeline module's mount (`page-timeline`): the task the page's box asks for
/// (`reviewTimelineRequest`, sent through the shell's `reviewTimeline` with `review:timeline`),
/// none once the box is gone.
#[cfg(feature = "page-timeline")]
#[component]
pub fn TimelineApp() -> Element {
    use wasm_bindgen::JsValue;
    std::hint::black_box(&STYLE);
    let revision = crate::frame::use_revision(&["review:timeline"]);
    let _ = revision();
    let request = crate::frame::snapshot("reviewTimelineRequest");
    let field =
        |name: &str| js_sys::Reflect::get(&request, &name.into()).unwrap_or(JsValue::UNDEFINED);
    let Some(slug) = field("slug").as_string() else {
        return rsx! {};
    };
    let revision = field("revision").as_f64().unwrap_or_default();
    let owner = field("owner").as_f64().unwrap_or_default();
    // A new box starts afresh: a component is mounted anew only for a new key in a list.
    rsx! { for box_key in std::iter::once(format!("{owner}-{slug}")) {
        TimelineHost { key: "{box_key}", slug: slug.clone(), revision }
    } }
}

/// What a read of the timeline left: the answer shown, the validator it came with, and the
/// server's clock at it beside this browser's (a `304` moves the one by the other).
#[derive(Clone, Default)]
pub struct Kept {
    pub tag: Option<String>,
    pub at: (f64, f64),
}

/// The timeline of `slug`: read when it mounts, again whenever the task changes (`revision`, its
/// `updated`) or Try again is pressed, and each `every` seconds while the task is open and the
/// page shown. A read that fails after one that did not leaves the one shown.
#[component]
pub fn TimelineHost(slug: String, revision: f64) -> Element {
    let answer = use_signal(|| None::<Result<Rc<TaskTimeline>, String>>);
    let kept = use_signal(Kept::default);
    let mut tries = use_signal(|| 0_u32);
    let shown = match &*answer.read() {
        None => rsx! {},
        // A part of the page that failed says so in its place, quietly: the page's own errors are
        // its notice stack's, drawn once outside its flow.
        Some(Err(error)) => {
            let said = failed(error);
            rsx! { div { class: "tl-drawn tl-failed",
                p { class: "tl-note", "{said} "
                    button { class: "dx-button", "data-style": "outline", "data-size": "default", r#type: "button", onclick: move |_| tries += 1, "Try again" }
                }
            } }
        }
        Some(Ok(timeline)) => {
            rsx! { TimelineView { timeline: timeline.clone(), slug: slug.clone() } }
        }
    };
    // A read starts afresh for each revision and each Try again: a component is mounted anew only
    // for a new key in a list.
    rsx! {
        for read in std::iter::once(format!("{revision}-{tries}")) {
            TimelineReader { key: "{read}", slug: slug.clone(), answer, kept }
        }
        {shown}
    }
}

/// Reads the timeline of `slug` into `answer` when it mounts, and again after [`next_read`]'s
/// seconds while the task is open (a minute after a failed read), waiting while the page is hidden
/// and reading once it shows again.
#[component]
fn TimelineReader(
    slug: String,
    answer: Signal<Option<Result<Rc<TaskTimeline>, String>>>,
    kept: Signal<Kept>,
) -> Element {
    #[cfg(target_arch = "wasm32")]
    use_hook(move || {
        let (mut answer, mut kept) = (answer, kept);
        spawn(async move {
            loop {
                let tag = kept.peek().tag.clone();
                match read(&slug, tag.as_deref()).await {
                    Ok(Read::Fresh(timeline, tag)) => {
                        kept.set(Kept {
                            tag,
                            at: (timeline.now, js_sys::Date::now()),
                        });
                        answer.set(Some(Ok(Rc::new(timeline))));
                    }
                    Ok(Read::Same) => {
                        let (now, then) = kept.peek().at;
                        let moved = match &*answer.peek() {
                            Some(Ok(shown)) => Some(TaskTimeline {
                                now: now + (js_sys::Date::now() - then) / 1000.0,
                                ..(**shown).clone()
                            }),
                            _ => None,
                        };
                        if let Some(moved) = moved {
                            answer.set(Some(Ok(Rc::new(moved))));
                        }
                    }
                    Err(error) => {
                        if !matches!(*answer.peek(), Some(Ok(_))) {
                            answer.set(Some(Err(error)));
                        }
                    }
                }
                let Some(every) = next_read(answer.peek().as_ref()) else {
                    break;
                };
                crate::live::sleep(i32::try_from(every.saturating_mul(1000)).unwrap_or(i32::MAX))
                    .await;
                shown().await;
            }
        })
    });
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (slug, answer, kept);
    rsx! {}
}

/// What a read that failed says, from the reason it failed for: its first line, the way the gates
/// and a page that did not load say theirs.
pub fn failed(why: &str) -> String {
    let why = why
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_end_matches('.');
    if why.is_empty() {
        "The timeline could not be read.".into()
    } else {
        format!("The timeline could not be read: {why}.")
    }
}

/// Seconds until the next read after `answer`: the timeline's own `every` while its task is open;
/// a minute after a read that failed with nothing shown, so a phone back on its network reads it
/// by itself; none once the task is closed.
pub fn next_read(answer: Option<&Result<Rc<TaskTimeline>, String>>) -> Option<u32> {
    match answer? {
        Ok(timeline) => timeline.closed.is_none().then(|| timeline.every_seconds()),
        Err(_) => Some(symphony_api::timeline::TIMELINE_EVERY),
    }
}

/// Why a step of a read failed: a JavaScript error's message ("Failed to fetch" for a fetch the
/// network refused), never its stack, else the value's text.
#[cfg(target_arch = "wasm32")]
fn why_failed(value: wasm_bindgen::JsValue) -> String {
    use wasm_bindgen::JsCast;
    match value.dyn_ref::<js_sys::Error>() {
        Some(error) => String::from(error.message()),
        None => crate::live::describe(value),
    }
}

/// A read of the timeline: a new answer and its validator, or the one kept (`304`).
#[cfg(target_arch = "wasm32")]
enum Read {
    Fresh(TaskTimeline, Option<String>),
    Same,
}

/// `slug`'s timeline, as `/api/task-timeline` answers it, asked with `tag`, the last answer's
/// validator, when there is one. A read that has no answer in 15 s fails.
#[cfg(target_arch = "wasm32")]
async fn read(slug: &str, tag: Option<&str>) -> Result<Read, String> {
    use futures_util::future::{Either, select};
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    let window = web_sys::window().ok_or("No window")?;
    let path = format!(
        "{}?slug={}",
        symphony_api::TASK_TIMELINE_PATH,
        crate::request::encode(slug)
    );
    let url = crate::project::current().here(&crate::task_live::url(&path));
    let controller = web_sys::AbortController::new().map_err(why_failed)?;
    let init = web_sys::RequestInit::new();
    init.set_signal(Some(&controller.signal()));
    if let Some(tag) = tag {
        let headers = web_sys::Headers::new().map_err(why_failed)?;
        headers.set("If-None-Match", tag).map_err(why_failed)?;
        init.set_headers(&headers);
    }
    let operation = async {
        let response: web_sys::Response =
            JsFuture::from(window.fetch_with_str_and_init(&url, &init))
                .await
                .map_err(why_failed)?
                .unchecked_into();
        if response.status() == 304 {
            return Ok(Read::Same);
        }
        let tag = response.headers().get("ETag").ok().flatten();
        let body = match response.json() {
            Ok(body) => JsFuture::from(body).await,
            Err(error) => Err(error),
        };
        if !response.ok() {
            return Err(body
                .ok()
                .and_then(|body| crate::request::field(&body, "error").as_string())
                .unwrap_or_else(|| format!("the server answered {}", response.status())));
        }
        let timeline = serde_wasm_bindgen::from_value(body.map_err(why_failed)?)
            .map_err(|error| error.to_string())?;
        Ok(Read::Fresh(timeline, tag))
    };
    match select(
        std::pin::pin!(operation),
        std::pin::pin!(crate::live::sleep(15_000)),
    )
    .await
    {
        Either::Left((result, _)) => result,
        Either::Right(_) => {
            controller.abort();
            Err("no answer from the server".into())
        }
    }
}

/// Once the page is shown: at once if it is, else at the first change of visibility that shows it.
#[cfg(target_arch = "wasm32")]
async fn shown() {
    use wasm_bindgen::{JsCast, closure::Closure};
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    if !document.hidden() {
        return;
    }
    let (send, seen) = futures_channel::oneshot::channel::<()>();
    let send = std::cell::RefCell::new(Some(send));
    let watched = document.clone();
    let listener = Closure::<dyn FnMut()>::new(move || {
        if !watched.hidden()
            && let Some(send) = send.borrow_mut().take()
        {
            let _ = send.send(());
        }
    });
    /// The listener, taken off however the wait ends (a reader unmounted mid-wait included).
    struct Listening(web_sys::Document, Closure<dyn FnMut()>);
    impl Drop for Listening {
        fn drop(&mut self) {
            let _ = self.0.remove_event_listener_with_callback(
                "visibilitychange",
                self.1.as_ref().unchecked_ref(),
            );
        }
    }
    let _ = document
        .add_event_listener_with_callback("visibilitychange", listener.as_ref().unchecked_ref());
    let _listening = Listening(document, listener);
    let _ = seen.await;
}

/// The id of the part of the box the steps are drawn in, which the box clips to its height.
const BODY: &str = "tl-body";

/// The part of the box the steps are drawn in, the box, and whether the page is wide (the lanes).
#[cfg(target_arch = "wasm32")]
fn parts() -> Option<(web_sys::Element, web_sys::Element, bool)> {
    let window = web_sys::window()?;
    let document = window.document()?;
    let body = document.get_element_by_id(BODY)?;
    let slot = document.get_element_by_id(crate::timeline_slot::SLOT)?;
    let wide = window
        .inner_width()
        .ok()
        .and_then(|width| width.as_f64())
        .is_some_and(|width| width >= 900.0);
    Some((body, slot, wide))
}

/// The lanes' rows a cut may end under, wide: each lane, a gate's row among them, never the axis
/// or the Gates heading alone.
#[cfg(target_arch = "wasm32")]
const LANE_ROWS: &str = ".tl-lanes > .tl-lane:not(.tl-axis):not(.tl-gates-head)";

/// Where the lanes are cut when they do not fit in the room `held` (pixels from the top of the
/// part that holds them): under the last lane that fits whole, `bottoms` being where each lane
/// ends, top to bottom. None when every lane fits, or none does (the box then clips as it can).
pub fn lane_edge(bottoms: &[f64], held: f64) -> Option<f64> {
    if bottoms.last().is_none_or(|&last| last <= held + 1.0) {
        return None;
    }
    bottoms
        .iter()
        .copied()
        .take_while(|&bottom| bottom <= held + 1.0)
        .last()
}

/// The height a wide box `height` pixels tall keeps for its task's next visit, its lanes `drawn`
/// pixels tall overflowing the room `held` it leaves them, each lane ending at `bottoms`: as tall
/// as the lanes the tallest box ([`crate::timeline_slot::WIDE`]) would hold whole, not the lanes
/// this box holds, so a box kept short grows back once its timeline gains lanes. As tall as every
/// lane where the tallest box holds them all, or none whole.
pub fn kept_wide(height: f64, held: f64, drawn: f64, bottoms: &[f64]) -> f64 {
    let room = held + (f64::from(crate::timeline_slot::WIDE) - height).max(0.0);
    height - held + lane_edge(bottoms, room).unwrap_or(drawn)
}

/// After a draw: whether the steps overflow the box (and so "Show all" is offered), and the
/// height the timeline took, kept for the task's next visit so its box is reserved as tall. The
/// steps are measured by what is drawn of them (the list on a phone, the lanes wide), never by the
/// part that holds them, which is at least as tall as the box leaves it. Wide, lanes that overflow
/// are cut under the last that fits whole ([`lane_edge`]), never through one, and the box kept for
/// the next visit is as tall as the tallest box's cut ([`kept_wide`]).
#[cfg(target_arch = "wasm32")]
fn settle(slug: &str, all: bool) -> bool {
    use wasm_bindgen::JsCast;
    let Some((body, slot, wide)) = parts() else {
        return false;
    };
    let style = body
        .dyn_ref::<web_sys::HtmlElement>()
        .map(web_sys::HtmlElement::style);
    // Measured at the room the box leaves, not at an earlier cut.
    if let Some(style) = &style {
        let _ = style.remove_property("max-height");
    }
    let _ = body.remove_attribute("data-cut");
    let drawn = body
        .query_selector(if wide { ".tl-lanes" } else { ".tl-list" })
        .ok()
        .flatten()
        .map_or(0.0, |steps| steps.get_bounding_client_rect().height());
    let held = f64::from(body.client_height());
    let over = drawn > held + 1.0;
    let height = slot.get_bounding_client_rect().height();
    let (cut, natural) = if wide && !all && over {
        let top = body.get_bounding_client_rect().top();
        let rows = body.query_selector_all(LANE_ROWS).ok();
        let bottoms: Vec<f64> = rows
            .map(|rows| {
                (0..rows.length())
                    .filter_map(|n| rows.item(n)?.dyn_into::<web_sys::Element>().ok())
                    .map(|row| row.get_bounding_client_rect().bottom() - top)
                    .collect()
            })
            .unwrap_or_default();
        (
            lane_edge(&bottoms, held),
            kept_wide(height, held, drawn, &bottoms),
        )
    } else {
        (None, height - held + drawn)
    };
    if let (Some(cut), Some(style)) = (cut, &style) {
        let _ = style.set_property("max-height", &format!("{cut:.2}px"));
        let _ = body.set_attribute("data-cut", "");
    }
    if !all {
        crate::timeline_slot::keep(slug, wide, natural);
    }
    over
}

/// How many lanes the wide timeline draws: one for each of its own and one for the gates, all of
/// them under one heading.
pub fn lane_count(lanes: &Lanes) -> usize {
    lanes.lanes.len() + usize::from(!lanes.gates.is_empty())
}

/// What "Show all" says on a phone, where it shows the list's steps, and wide, where it shows the
/// lanes: what it reveals, counted.
pub fn show_all(steps: usize, lanes: usize) -> (String, String) {
    let words = |n: usize, word: &str| {
        let n = u32::try_from(n).unwrap_or(u32::MAX);
        format!("Show all {n} {}", plural(n, word))
    };
    (words(steps, "step"), words(lanes, "lane"))
}

/// The timeline drawn in its box: its totals, the steps (the phone's list and the wide lanes, the
/// style sheet shows one and keeps the list for assistive technology), what the bars mean, and
/// "Show all" when the steps overflow the box.
#[component]
pub fn TimelineView(timeline: Rc<TaskTimeline>, slug: String) -> Element {
    let mut all = use_signal(|| false);
    let mut clipped = use_signal(|| false);
    let detail = use_signal(String::new);
    let active = use_signal(|| 0_usize);
    let (first, machine) = totals(&timeline);
    let note = kept_note(&timeline);
    let rows = rows(&timeline);
    let lanes = lanes(&timeline);
    let running = timeline
        .runs
        .iter()
        .any(|run| run.result == RunResult::Running);
    // The legend's gate states in the approved picture's order, those the timeline holds.
    let states: Vec<GateState> = [
        GateState::Met,
        GateState::Missing,
        GateState::Waived,
        GateState::Held,
        GateState::Unknown,
    ]
    .into_iter()
    .filter(|state| {
        lanes
            .gates
            .iter()
            .any(|gate| gate.spans.iter().any(|span| span.state == *state))
    })
    .collect();
    // The style sheet shows one of the two: the list's steps on a phone, the lanes wide.
    let (more_steps, more_lanes) = show_all(rows.len(), lane_count(&lanes));
    #[cfg(target_arch = "wasm32")]
    {
        let slug = slug.clone();
        let drawn = timeline.clone();
        use_effect(use_reactive((&drawn,), move |(_,)| {
            let all = all();
            // Read, not peeked: the "Show all" a clip brings can take room from the steps (a line
            // of the foot), so the box is measured again with it there and cut at that room.
            let shown = clipped();
            let over = settle(&slug, all);
            // Showing all, the box has grown to hold every step and says nothing of whether its
            // own height clips them. So "Show the latest" stays the same button, with the focus
            // still on it, until the box is measured again at its own height.
            if !all && shown != over {
                clipped.set(over);
            }
        }));
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (&slug, &mut clipped);
    // A clipped list is drawn from its end on a phone (the style sheet's `tl-clipped`), so its
    // latest steps stay in view whatever room the box's foot leaves them.
    let class = match (all(), clipped()) {
        (true, _) => "tl-drawn tl-all",
        (false, true) => "tl-drawn tl-clipped",
        (false, false) => "tl-drawn",
    };
    rsx! {
        div { class,
            div { class: "tl-head",
                p { class: "tl-totals", span { "{first}" }
                    if let Some(machine) = &machine {
                        span { class: "tl-machine tl-narrow", "Machine {machine}" }
                        span { class: "tl-machine tl-wide", " · machine {machine}" }
                    }
                }
                if let Some(note) = note { p { class: "tl-note", "{note}" } }
            }
            div { class: "tl-body", id: BODY,
                ol { class: "tl-list", "aria-label": "Steps", for row in &rows { {row_view(row)} } }
                {lanes_view(&lanes, active, detail)}
            }
            div { class: "tl-foot",
                ul { class: "tl-legend",
                    li { span { class: "tl-key tl-key-work" } "worked or ran" }
                    li { span { class: "tl-key tl-key-wait" } "queued or waiting" }
                    if running { li { span { class: "tl-key tl-key-running" } "running now" } }
                    for state in states {
                        li { class: "tl-wide", span { class: "tl-key tl-g tl-g-{state.as_str()}" } "gate {state.as_str()}" }
                    }
                }
                // Only while a part of the lanes has the focus, in the legend's place beside "Show
                // all": a line of its own would take room from the lanes and cut through the one
                // the focus is in.
                if !detail().is_empty() { p { class: "tl-detail tl-wide", "aria-hidden": "true", "{detail}" } }
                if clipped() || all() {
                    button { class: "dx-button tl-more", "data-style": "outline", "data-size": "default", r#type: "button", "aria-expanded": "{all()}", onclick: move |_| all.toggle(),
                        if all() { "Show the latest" } else {
                            span { class: "tl-more-steps", "{more_steps}" }
                            span { class: "tl-more-lanes", "{more_lanes}" }
                        }
                    }
                }
            }
        }
    }
}

fn row_view(row: &Row) -> Element {
    let class = match (row.sub, row.event) {
        (false, _) => "tl-row",
        (true, false) => "tl-row tl-sub",
        (true, true) => "tl-row tl-sub tl-event",
    };
    rsx! {
        if let Some(day) = &row.day { li { class: "tl-day", "{day}" } }
        li { class, style: "--c:var(--{row.tone})", title: "{row.title}",
            span { class: "tl-at", "{row.at}" }
            span { class: "tl-mark", "aria-hidden": "true" }
            span { class: "tl-label", "{row.label}" }
            if let Some(took) = &row.took { span { class: "tl-took", "{took}" } }
            if let Some(bar) = &row.bar { span { class: "tl-bars",
                if bar.wait > 0.0 { span { class: "tl-wait", "aria-hidden": "true", style: "--c:var(--{bar.wait_tone});width:{bar.wait:.2}%" } }
                if bar.work > 0.0 { span { class: if bar.running { "tl-work tl-running" } else { "tl-work" }, "aria-hidden": "true", style: "width:{bar.work:.2}%" } }
                if let Some((word, tone)) = row.word { span { class: "tl-word", style: "--c:var(--{tone})", "{word}" } }
            } }
        }
    }
}

/// Moves the keyboard's stop to the lanes' part `to` and focuses it.
fn stop_at(mut active: Signal<usize>, to: usize) {
    active.set(to);
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        if let Some(part) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id(&format!("tl-stop-{to}")))
            .and_then(|part| part.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = part.focus();
        }
    }
}

/// The attributes and handlers of the lanes' part `index` of `count`: one stop of one arrow-key
/// group (the first, until another is chosen), its words for assistive technology, and its whole
/// detail shown in the legend's place under the lanes while it has the focus.
fn stop(
    index: usize,
    count: usize,
    title: &str,
    active: Signal<usize>,
    detail: Signal<String>,
) -> Vec<Attribute> {
    let said = title.to_string();
    let mut detail_in = detail;
    let mut detail_out = detail;
    let mut active_in = active;
    vec![
        Attribute::new("id", format!("tl-stop-{index}"), None, false),
        Attribute::new(
            "tabindex",
            if active() == index { "0" } else { "-1" },
            None,
            false,
        ),
        Attribute::new("role", "img", None, false),
        Attribute::new("aria-label", title.to_string(), None, false),
        dioxus::html::onfocus(move |_| {
            active_in.set(index);
            detail_in.set(said.clone());
        }),
        dioxus::html::onblur(move |_| detail_out.set(String::new())),
        dioxus::html::onkeydown(move |event: KeyboardEvent| {
            let last = count.saturating_sub(1);
            let to = match event.key() {
                Key::ArrowRight | Key::ArrowDown => (index + 1).min(last),
                Key::ArrowLeft | Key::ArrowUp => index.saturating_sub(1),
                Key::Home => 0,
                Key::End => last,
                _ => return,
            };
            event.prevent_default();
            stop_at(active, to);
        }),
    ]
}

fn fit_view(fit: &Fit) -> Element {
    rsx! { span { class: fit.class(), "{fit.text}" } }
}

fn lanes_view(lanes: &Lanes, active: Signal<usize>, detail: Signal<String>) -> Element {
    let count = lanes.stops();
    let mut index = 0;
    let mut next = || {
        index += 1;
        index - 1
    };
    let lane_rows: Vec<Element> = lanes
        .lanes
        .iter()
        .map(|lane| {
            let segs: Vec<Element> = lane
                .segs
                .iter()
                .map(|seg| {
                    let attributes = if seg.stop {
                        stop(next(), count, &seg.title, active, detail)
                    } else {
                        vec![Attribute::new("aria-hidden", "true", None, false)]
                    };
                    seg_view(seg, attributes)
                })
                .collect();
            rsx! {
                div { class: "tl-lane", "data-lane": lane.key, style: "--rows:{lane.rows}",
                    span { class: "tl-name", b { "{lane.name}" } if !lane.who.is_empty() { small { "{lane.who}" } } }
                    span { class: "tl-track", {segs.into_iter()} }
                }
            }
        })
        .collect();
    let gate_rows: Vec<Element> = lanes
        .gates
        .iter()
        .map(|gate| {
            let spans: Vec<Element> = gate
                .spans
                .iter()
                .map(|span| {
                    let attributes = stop(next(), count, &span.title, active, detail);
                    rsx! {
                        span { class: "tl-g tl-g-{span.state.as_str()}", style: "left:{span.left:.2}%;width:{span.width:.2}%", title: "{span.title}", ..attributes,
                            if let Some(word) = &span.word { {fit_view(word)} }
                        }
                    }
                })
                .collect();
            rsx! {
                div { class: "tl-lane tl-gate", "data-gate": "{gate.name}",
                    span { class: "tl-name", "{gate.name}" }
                    span { class: "tl-track", {spans.into_iter()} }
                }
            }
        })
        .collect();
    rsx! { div { class: "tl-lanes", role: "group", "aria-label": "Lanes",
        div { class: "tl-hours", "aria-hidden": "true",
            for tick in &lanes.ticks { span { class: "tl-hour", style: "left:{tick.left:.2}%" } }
            if let Some(now) = &lanes.now { span { class: "tl-now", style: "left:{now.left:.2}%" } }
        }
        div { class: "tl-lane tl-axis", "aria-hidden": "true",
            span { class: "tl-name" }
            span { class: "tl-track",
                for tick in &lanes.ticks { span { class: "tl-tick", style: "left:{tick.left:.2}%", "{tick.label}" } }
                if let Some(now) = &lanes.now { span { class: "tl-tick tl-nowtag", style: "left:{now.left:.2}%", "{now.label}" } }
            }
        }
        {lane_rows.into_iter()}
        if !lanes.gates.is_empty() {
            div { class: "tl-lane tl-gates-head", span { class: "tl-name", b { "Gates" } } }
            {gate_rows.into_iter()}
        }
    } }
}

fn seg_view(seg: &Seg, attributes: Vec<Attribute>) -> Element {
    let right = seg.left + seg.width;
    rsx! {
        span { class: seg.look.class(), style: "--c:var(--{seg.tone});--row:{seg.row};left:{seg.left:.2}%;width:{seg.width:.2}%", title: "{seg.title}", ..attributes,
            for fit in &seg.text { {fit_view(fit)} }
        }
        if let Some((word, tone, room)) = &seg.after { span { class: "tl-after {word.class()}", "aria-hidden": "true", style: "--c:var(--{tone});left:{right:.2}%;max-width:{room.css()}", "{word.text}" } }
        if let Some((under, room)) = &seg.under { span { class: "tl-under {under.class()}", "aria-hidden": "true", style: "--row:{seg.row};left:{seg.left:.2}%;max-width:{room.css()}", "{under.text}" } }
    }
}

#[cfg(test)]
#[path = "task_timeline/tests.rs"]
mod tests;
