//! A task's timeline: what it went through, read from `/api/task-timeline`
//! ([`symphony_api::timeline`]) once its section nears the screen, and again each minute while the
//! task is open, since a build or a gate moves without the task changing.
//!
//! - On a phone, a list ([`rows`]): a row per step with its start, its label, how long it took and
//!   a bar as long, its wait drawn lighter; the builds, tests and gate changes indented under the
//!   stage they happened in; the verdict after a review's bar, and a striped bar for what runs now.
//! - Wide, lanes over the same hours ([`lanes`]): the stage, the executor, the reviewer, builds and
//!   tests, and a thin row per gate (missing dotted, met solid, waived grey), with a line at now.
//!
//! Both are drawn, and `assets/task-timeline.css` shows the one the page's width calls for. The
//! component puts that sheet in the page itself, as `attach.rs` does: a stylesheet asset would be
//! named in the shell's manifest, which every first visit loads.

use crate::viewport::NearViewport as Near;
use crate::words::plural;
use dioxus::prelude::*;
use symphony_api::gates::REVIEW;
use symphony_api::timeline::{Run, RunResult, Stage, Verdict};
use symphony_api::{GateState, TaskTimeline};

/// The timeline's style sheet, put in the page by the first timeline to mount (the host build draws
/// no page).
#[cfg(any(target_arch = "wasm32", test))]
pub const STYLE: &str = include_str!("../assets/task-timeline.css");

/// How much of a row the longest step's bar takes on a phone, in percent: the rest is for the word
/// after it.
const BAR_SPAN: f64 = 72.0;

/// A lane's segment shows its words from this share of the hours drawn, its duration alone from
/// [`SHORT`], and nothing under it.
const WORDY: f64 = 0.08;
const SHORT: f64 = 0.03;

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
            "Machine {}, queued {}",
            took(timeline.machine_seconds()),
            took(timeline.queued_seconds())
        )
    });
    (first, machine)
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
        |to| format!(" to {}", clock(to, timeline.offset)),
    );
    let moved = stage
        .by
        .as_deref()
        .map(|by| format!(", moved there by {by}"))
        .unwrap_or_default();
    Row {
        start: stage.from,
        day: None,
        at: clock(stage.from, timeline.offset),
        sub: false,
        event: false,
        tone,
        title: format!(
            "{label}: from {}{until}{moved}",
            clock(stage.from, timeline.offset)
        ),
        label,
        took: (!ended).then(|| took(seconds)),
        bar,
        word,
    }
}

fn run_row(run: &Run, timeline: &TaskTimeline, share: &dyn Fn(f64) -> f64) -> Row {
    let now = timeline.now;
    let (waited, ran) = (run.waited(now), run.ran(now));
    let phase = match run.phase.as_str() {
        "build" => "Build".to_string(),
        "test" => "Test".to_string(),
        other => capitalised(other),
    };
    let mut label = format!("{phase}, {}", run.label);
    if waited >= 60.0 {
        label.push_str(&format!(" · queued {}", took(waited)));
    }
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
    Row {
        start: run.queued,
        day: None,
        at: clock(run.queued, timeline.offset),
        sub: true,
        event: false,
        tone,
        label,
        took: Some(took(if run.started.is_some() { ran } else { waited })),
        bar: Some(Bar {
            wait: share(waited),
            wait_tone: "primary",
            work: share(ran),
            running: run.result == RunResult::Running,
        }),
        word: Some((run.result.as_str(), tone)),
        title,
    }
}

/// Each change of a gate's state after its first, but the review gate's, whose rounds say it.
fn gate_changes(timeline: &TaskTimeline) -> Vec<Row> {
    let mut rows = Vec::new();
    for lane in timeline.gates.iter().filter(|lane| lane.name != REVIEW) {
        for pair in lane.states.windows(2) {
            let (before, span) = (&pair[0], &pair[1]);
            let word = match span.state {
                GateState::Met => "met",
                GateState::Missing if before.state == GateState::Met => "missing again",
                GateState::Missing => "missing",
                GateState::Waived => "waived",
                GateState::Held => "held",
                GateState::Unknown => "unknown",
            };
            let by = matches!(span.state, GateState::Waived | GateState::Held)
                .then_some(span.by.as_deref())
                .flatten();
            let label = with(format!("{} {word}", capitalised(&lane.name)), by);
            rows.push(Row {
                start: span.from,
                day: None,
                at: clock(span.from, timeline.offset),
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
/// it went on, by time.
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
        rows.push(stage_row(timeline, stage, &mut worked, &share));
        let next = timeline.stages.get(index + 1).map(|next| next.from);
        while let Some(row) = under.next_if(|row| next.is_none_or(|next| row.start < next)) {
            rows.push(row);
        }
    }
    rows.extend(under);
    if date(timeline.created, timeline.offset) != date(timeline.end(), timeline.offset) {
        let mut last = None;
        for row in &mut rows {
            let day = date(row.start, timeline.offset);
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

/// A segment of a lane, placed in percent of the hours drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Seg {
    pub left: f64,
    pub width: f64,
    pub tone: &'static str,
    pub look: Look,
    pub text: String,
    /// A word right after it (a review's verdict) and its tone.
    pub after: Option<(&'static str, &'static str)>,
    /// A label under it (a run's), and how wide it may grow before the next one, in percent.
    pub under: Option<(String, f64)>,
    pub title: String,
}

/// A lane of the wide view: its name, who worked in it, its segments.
#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    /// The lane's key, for its row's `data-lane`.
    pub key: &'static str,
    pub name: &'static str,
    pub who: String,
    pub segs: Vec<Seg>,
}

/// A gate's thin row: its states, placed in percent of the hours drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct GateRow {
    pub name: String,
    pub spans: Vec<(f64, f64, GateState, String)>,
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

/// The hours drawn: from the local hour the task was made in to the timeline's end, now and the
/// server's offset beside them.
struct Scale {
    start: f64,
    span: f64,
    end: f64,
    now: f64,
    offset: i64,
}

impl Scale {
    fn of(timeline: &TaskTimeline) -> Scale {
        let (created, offset) = (timeline.created, timeline.offset);
        let start = created - (created + offset as f64).rem_euclid(3600.0);
        let end = timeline.end();
        Scale {
            start,
            span: (end - start).max(60.0),
            end,
            now: timeline.now,
            offset,
        }
    }

    fn at(&self, at: f64) -> f64 {
        ((at - self.start) / self.span * 100.0).clamp(0.0, 100.0)
    }

    fn width(&self, from: f64, to: f64) -> f64 {
        self.at(to) - self.at(from)
    }

    /// `word` and `took` in a segment of `width` percent, as far as they fit.
    fn fit(word: &str, took: &str, width: f64) -> String {
        if width >= WORDY * 100.0 && !word.is_empty() {
            format!("{word} {took}")
        } else if width >= SHORT * 100.0 {
            took.to_string()
        } else {
            String::new()
        }
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
        let first = local(self.start, self.offset);
        let mut tick = self.start + ((-first).rem_euclid(every.min(1440))) as f64 * 60.0;
        let mut ticks = Vec::new();
        while tick <= self.end {
            let left = self.at(tick);
            if !open || self.at(self.now) - left >= 4.0 {
                let label = if local(tick, self.offset).rem_euclid(1440) == 0 {
                    date(tick, self.offset)
                } else {
                    clock(tick, self.offset)
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
/// "running" while it runs.
fn short(run: &Run) -> String {
    if run.result == RunResult::Running {
        return "running".into();
    }
    if run.phase == "build" {
        return "build".into();
    }
    let label = run.label.strip_prefix("just ").unwrap_or(&run.label);
    label.split_whitespace().next().unwrap_or(label).to_string()
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
                    text: Scale::fit(&word, &length, width),
                    after: None,
                    under: None,
                    title: format!(
                        "{word} {length}, from {} to {}",
                        clock(stage.from, scale.offset),
                        clock(to, scale.offset)
                    ),
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
            Seg {
                left: scale.at(claim.from),
                width,
                tone: "primary",
                look: Look::Solid,
                text: Scale::fit(word, &length, width),
                after: None,
                under: None,
                title: format!("{} held it {length}", claim.by),
            }
        })
        .collect()
}

/// The reviewer lane: each round's wait for a reviewer, drawn as waiting, then the reviewer's
/// time on it, its verdict after it.
fn round_segs(timeline: &TaskTimeline, scale: &Scale) -> Vec<Seg> {
    let mut segs = Vec::new();
    for round in &timeline.rounds {
        let to = round.to.unwrap_or(scale.now);
        let taken = round.taken.map(|taken| taken.clamp(round.from, to));
        let after = round
            .verdict
            .map(|verdict| (verdict.as_str(), verdict_tone(verdict)));
        let name = format!("round {}", round.number);
        let by = round.by.as_deref().unwrap_or("nobody yet");
        let waited = took(taken.unwrap_or(to) - round.from);
        let title = match taken {
            Some(taken) => format!(
                "{name}: waited {waited} for a reviewer, then {by} took {}",
                took(to - taken)
            ),
            None => format!("{name}: waited {waited} for a reviewer"),
        };
        let wait = scale.width(round.from, taken.unwrap_or(to));
        if wait > 0.0 || taken.is_none() {
            segs.push(Seg {
                left: scale.at(round.from),
                width: wait,
                tone: "question",
                look: Look::Waiting,
                text: if taken.is_none() && wait >= SHORT * 100.0 {
                    name.clone()
                } else {
                    String::new()
                },
                after: if taken.is_none() { after } else { None },
                under: None,
                title: title.clone(),
            });
        }
        if let Some(taken) = taken {
            let width = scale.width(taken, to);
            segs.push(Seg {
                left: scale.at(taken),
                width,
                tone: "question",
                look: Look::Solid,
                text: if width >= SHORT * 100.0 {
                    name
                } else {
                    String::new()
                },
                after,
                under: None,
                title,
            });
        }
    }
    segs
}

/// The builds and tests lane: each run's queue, drawn as waiting with its name under it, then its
/// run in its result's tone, striped while it runs.
fn run_segs(timeline: &TaskTimeline, scale: &Scale) -> Vec<Seg> {
    let now = scale.now;
    let mut segs = Vec::new();
    for (index, run) in timeline.runs.iter().enumerate() {
        let started = run.started.or(run.ended).unwrap_or(now);
        let room = timeline
            .runs
            .get(index + 1)
            .map_or(100.0, |next| scale.at(next.queued))
            - scale.at(run.queued);
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
        segs.push(Seg {
            left: scale.at(run.queued),
            width: scale.width(run.queued, started),
            tone: "primary",
            look: Look::Waiting,
            text: String::new(),
            after: None,
            under: Some((format!("{} {length}", short(run)), room.max(SHORT * 100.0))),
            title: title.clone(),
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
                text: String::new(),
                after: None,
                under: None,
                title,
            });
        }
    }
    segs
}

/// Each gate's thin row: each state from when it was taken to the next, the last to the end.
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
                        let title = if span.detail.is_empty() {
                            format!("{} {}", lane.name, span.state.as_str())
                        } else {
                            span.detail.clone()
                        };
                        (scale.at(span.from), width, span.state, title)
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
        segs: stage_segs(timeline, &scale),
    }];
    if !timeline.claims.is_empty() {
        lanes.push(Lane {
            key: "executor",
            name: "Executor",
            who: executors,
            segs: claim_segs(timeline, &scale, one),
        });
    }
    if !timeline.rounds.is_empty() {
        lanes.push(Lane {
            key: "reviewer",
            name: "Reviewer",
            who: reviewers,
            segs: round_segs(timeline, &scale),
        });
    }
    if !timeline.runs.is_empty() {
        lanes.push(Lane {
            key: "runs",
            name: "Builds & tests",
            who: hosts,
            segs: run_segs(timeline, &scale),
        });
    }
    Lanes {
        ticks: scale.ticks(open),
        now: open.then(|| Tick {
            left: scale.at(scale.now),
            label: format!("now {}", clock(scale.now, scale.offset)),
        }),
        lanes,
        gates: gate_rows(timeline, &scale),
    }
}

/// The task's timeline, read once its section nears the screen and again whenever the task changes
/// (`revision`, its `updated`).
#[component]
pub fn TaskTimelinePanel(slug: String, revision: f64) -> Element {
    #[cfg(target_arch = "wasm32")]
    use_hook(put_style);
    let answer = use_signal(|| None::<Result<TaskTimeline, String>>);
    let mut tries = use_signal(|| 0_u32);
    rsx! { Near { id: "timeline-{slug}", height: 360,
        TimelineReader { key: "{revision}-{tries}", slug: slug.clone(), answer }
        section { class: "tl",
            h2 { "Timeline" }
            match answer() {
                None => rsx! { p { class: "tl-note", "Reading the timeline…" } },
                // A part of the page that failed says so in its place, quietly: the page's own
                // errors are its notice stack's, drawn once outside its flow.
                Some(Err(error)) => rsx! { p { class: "tl-note", "The timeline could not be read: {error} "
                    crate::components::button::Button { onclick: move |_| tries += 1, "Try again" }
                } },
                Some(Ok(timeline)) => rsx! { TimelineView { timeline } },
            }
        }
    } }
}

/// Reads the timeline of `slug` into `answer` when it mounts, and each minute after while the task
/// is open. A read that fails after one that did not leaves the one shown.
#[component]
fn TimelineReader(slug: String, answer: Signal<Option<Result<TaskTimeline, String>>>) -> Element {
    #[cfg(target_arch = "wasm32")]
    use_hook(move || {
        let mut answer = answer;
        spawn(async move {
            loop {
                let read = read(&slug).await;
                if read.is_ok() || !matches!(*answer.peek(), Some(Ok(_))) {
                    answer.set(Some(read));
                }
                let open =
                    matches!(&*answer.peek(), Some(Ok(timeline)) if timeline.closed.is_none());
                if !open {
                    break;
                }
                crate::live::sleep(60_000).await;
            }
        })
    });
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (slug, answer);
    rsx! {}
}

/// `slug`'s timeline, as `/api/task-timeline` answers it.
#[cfg(target_arch = "wasm32")]
async fn read(slug: &str) -> Result<TaskTimeline, String> {
    let encoded = js_sys::encode_uri_component(slug)
        .as_string()
        .unwrap_or_default();
    crate::task_live::get(&format!(
        "{}?slug={encoded}",
        symphony_api::TASK_TIMELINE_PATH
    ))
    .await
}

/// Put [`STYLE`] in the page once.
#[cfg(target_arch = "wasm32")]
fn put_style() {
    const ID: &str = "task-timeline-style";
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if document.get_element_by_id(ID).is_some() {
        return;
    }
    if let (Ok(style), Some(head)) = (document.create_element("style"), document.head()) {
        style.set_id(ID);
        style.set_text_content(Some(STYLE));
        let _ = head.append_child(&style);
    }
}

/// The timeline drawn: its totals, the phone's list and the wide lanes (the style sheet shows one),
/// and what the bars mean.
#[component]
pub fn TimelineView(timeline: TaskTimeline) -> Element {
    let (first, machine) = totals(&timeline);
    let rows = rows(&timeline);
    let lanes = lanes(&timeline);
    let running = timeline
        .runs
        .iter()
        .any(|run| run.result == RunResult::Running);
    rsx! {
        p { class: "tl-totals", span { "{first}" } if let Some(machine) = machine { span { class: "tl-machine", "{machine}" } } }
        div { class: "tl-card card",
            ol { class: "tl-list", for row in rows { {row_view(&row)} } }
            {lanes_view(&lanes)}
        }
        ul { class: "tl-legend",
            li { span { class: "tl-key tl-key-work" } "worked or ran" }
            li { span { class: "tl-key tl-key-wait" } "queued or waiting" }
            if running { li { span { class: "tl-key tl-key-running" } "running now" } }
            if !timeline.gates.is_empty() {
                li { class: "tl-wide", span { class: "tl-key tl-g-met" } "gate met" }
                li { class: "tl-wide", span { class: "tl-key tl-g-missing" } "gate missing" }
                li { class: "tl-wide", span { class: "tl-key tl-g-waived" } "gate waived" }
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

fn lanes_view(lanes: &Lanes) -> Element {
    rsx! { div { class: "tl-lanes",
        div { class: "tl-hours", "aria-hidden": "true",
            for tick in &lanes.ticks { span { class: "tl-hour", style: "left:{tick.left:.2}%" } }
            if let Some(now) = &lanes.now { span { class: "tl-now", style: "left:{now.left:.2}%" } }
        }
        div { class: "tl-lane tl-axis",
            span { class: "tl-name" }
            span { class: "tl-track",
                for tick in &lanes.ticks { span { class: "tl-tick", style: "left:{tick.left:.2}%", "{tick.label}" } }
                if let Some(now) = &lanes.now { span { class: "tl-tick tl-nowtag", style: "left:{now.left:.2}%", "{now.label}" } }
            }
        }
        for lane in &lanes.lanes {
            div { class: "tl-lane", "data-lane": lane.key,
                span { class: "tl-name", b { "{lane.name}" } if !lane.who.is_empty() { small { "{lane.who}" } } }
                span { class: "tl-track", for seg in &lane.segs { {seg_view(seg)} } }
            }
        }
        if !lanes.gates.is_empty() {
            div { class: "tl-lane tl-gates-head", span { class: "tl-name", b { "Gates" } } }
            for gate in &lanes.gates {
                div { class: "tl-lane tl-gate", "data-gate": "{gate.name}",
                    span { class: "tl-name", "{gate.name}" }
                    span { class: "tl-track",
                        for (left, width, state, title) in &gate.spans {
                            span { class: "tl-g tl-g-{state.as_str()}", style: "left:{left:.2}%;width:{width:.2}%", title: "{title}" }
                        }
                    }
                }
            }
        }
    } }
}

fn seg_view(seg: &Seg) -> Element {
    let right = seg.left + seg.width;
    rsx! {
        span { class: seg.look.class(), style: "--c:var(--{seg.tone});left:{seg.left:.2}%;width:{seg.width:.2}%", title: "{seg.title}", "{seg.text}" }
        if let Some((word, tone)) = seg.after { span { class: "tl-after", style: "--c:var(--{tone});left:{right:.2}%", "{word}" } }
        if let Some((under, room)) = &seg.under { span { class: "tl-under", style: "left:{seg.left:.2}%;max-width:{room:.2}%", "{under}" } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../../crates/symphony-api/fixtures/task-timeline.json");

    /// The approved pictures' task: two review rounds, a build and its tests, one running now, a
    /// gate met then missing again.
    fn pictures() -> TaskTimeline {
        serde_json::from_str(FIXTURE).unwrap()
    }

    fn html(timeline: TaskTimeline) -> String {
        fn page(timeline: TaskTimeline) -> Element {
            rsx! { TimelineView { timeline } }
        }
        let mut dom = VirtualDom::new_with_props(page, timeline);
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }

    #[test]
    fn a_length_reads_to_the_minute_in_hours_and_days_and_never_below_nothing() {
        let cases = [
            (-5.0, "0 m"),
            (0.0, "0 m"),
            (29.0, "0 m"),
            (31.0, "1 m"),
            (2460.0, "41 m"),
            (3580.0, "1 h"),
            (6720.0, "1 h 52 m"),
            (22_680.0, "6 h 18 m"),
            (86_400.0, "1 d"),
            (2.0 * 86_400.0 + 3.0 * 3600.0 + 20.0 * 60.0, "2 d 3 h"),
        ];
        for (seconds, said) in cases {
            assert_eq!(took(seconds), said, "{seconds} s");
        }
    }

    #[test]
    fn a_time_reads_on_the_servers_local_clock_and_its_day_by_the_calendar() {
        let midnight = 20_768.0 * 86_400.0;
        assert_eq!(clock(midnight, 0), "00:00");
        assert_eq!(clock(midnight - 1.0, 0), "23:59");
        assert_eq!(clock(midnight, 7200), "02:00");
        assert_eq!(clock(midnight, -60), "23:59");
        assert_eq!(date(midnight - 1.0, 0), date(midnight - 86_400.0, 0));
        assert_ne!(date(midnight, -60), date(midnight, 0));
        assert_eq!(civil(0), (1970, 1, 1));
        for (year, month, day) in [(2026, 11, 11), (2024, 2, 29), (2000, 3, 1), (1969, 12, 31)] {
            let minutes = crate::words::Moment::from_parts(year, month, day, 0, 0)
                .unwrap()
                .minutes();
            assert_eq!(
                civil(minutes.div_euclid(1440)),
                (year, i64::from(month), i64::from(day))
            );
        }
        let pictures = pictures();
        assert_eq!(clock(pictures.created, pictures.offset), "09:02");
        assert_eq!(date(pictures.created, pictures.offset), "11-11");
    }

    #[test]
    fn the_pictures_task_heads_its_timeline_with_its_totals() {
        let (first, machine) = totals(&pictures());
        assert_eq!(first, "6 h 18 m so far · 2 review rounds · 1 bounce");
        assert_eq!(machine.as_deref(), Some("Machine 1 h 33 m, queued 26 m"));
        let closed = TaskTimeline {
            closed: Some(pictures().now),
            rounds: Vec::new(),
            bounces: 0,
            runs: Vec::new(),
            ..pictures()
        };
        assert_eq!(
            totals(&closed),
            (
                "6 h 18 m in all · 0 review rounds · 0 bounces".to_string(),
                None
            )
        );
    }

    /// Each row as (at, label, took, word, under its stage).
    fn said(rows: &[Row]) -> Vec<(String, String, String, String, bool)> {
        rows.iter()
            .map(|row| {
                (
                    row.at.clone(),
                    row.label.clone(),
                    row.took.clone().unwrap_or_default(),
                    row.word
                        .map(|(word, _)| word)
                        .unwrap_or_default()
                        .to_string(),
                    row.sub,
                )
            })
            .collect()
    }

    #[test]
    fn the_phone_list_puts_each_run_and_gate_change_under_the_stage_it_began_in() {
        let rows = rows(&pictures());
        let expected = [
            ("09:02", "Ready, unclaimed", "41 m", "", false),
            ("09:43", "In progress · quiet-wren", "1 h 52 m", "", false),
            (
                "10:28",
                "Build, just lint-rs · queued 3 m",
                "4 m",
                "green",
                true,
            ),
            (
                "10:52",
                "Test, just verify · queued 6 m",
                "24 m",
                "green",
                true,
            ),
            (
                "11:35",
                "Review, round 1 · lucid-heron",
                "52 m",
                "changes",
                false,
            ),
            ("12:27", "In progress again", "1 h 10 m", "", false),
            (
                "13:00",
                "Test, just verify · queued 2 m",
                "23 m",
                "green",
                true,
            ),
            (
                "13:37",
                "Review, round 2 · lucid-heron",
                "28 m",
                "approved",
                false,
            ),
            (
                "14:05",
                "Approved, waiting to land",
                "1 h 15 m",
                "now",
                false,
            ),
            (
                "14:05",
                "Test, certificate verify · queued 5 m",
                "24 m",
                "green",
                true,
            ),
            ("14:12", "Dependency waived · cobalt-otter", "", "", true),
            ("14:34", "Certificate met", "", "", true),
            ("14:50", "Certificate missing again", "", "", true),
            (
                "14:52",
                "Test, certificate verify · queued 10 m",
                "18 m",
                "running",
                true,
            ),
        ]
        .map(|(at, label, took, word, sub)| {
            (at.into(), label.into(), took.into(), word.into(), sub)
        });
        assert_eq!(said(&rows), expected);
        // One day: no day marks.
        assert!(rows.iter().all(|row| row.day.is_none()));
        // The longest step, in progress for 1 h 52 m, fills the bar's span; the rest are as long as
        // their time.
        let bar = |index: usize| rows[index].bar.clone().unwrap();
        assert_eq!(bar(1).work, BAR_SPAN);
        assert!((bar(0).work - 41.0 / 112.0 * BAR_SPAN).abs() < 1e-9);
        // A review's bar waits for its reviewer, lighter, then works.
        let round = bar(4);
        assert!(
            (round.wait - 18.0 / 112.0 * BAR_SPAN).abs() < 1e-9,
            "{round:?}"
        );
        assert!(
            (round.work - 34.0 / 112.0 * BAR_SPAN).abs() < 1e-9,
            "{round:?}"
        );
        // Waiting to land is all wait; what runs now is striped after its queue; a gate change is
        // an instant.
        assert_eq!((bar(8).work, bar(8).wait > 0.0), (0.0, true));
        assert!(bar(13).running && !bar(9).running);
        assert_eq!(bar(13).wait_tone, "primary");
        assert!(rows[10].event && rows[10].bar.is_none());
        assert_eq!(
            rows[12].title,
            "Main moved: verify on the merge tree 4be91c2 running"
        );
        assert_eq!(rows[13].tone, "primary");
        assert_eq!(rows[3].tone, "like");
    }

    #[test]
    fn with_no_run_kept_there_is_no_run_row_no_machine_time_and_no_runs_lane() {
        let timeline = TaskTimeline {
            runs: Vec::new(),
            ..pictures()
        };
        let rows = rows(&timeline);
        assert!(
            rows.iter()
                .all(|row| !row.label.starts_with("Build") && !row.label.starts_with("Test")),
            "{:?}",
            said(&rows)
        );
        assert_eq!(rows.len(), 9, "six stages and three gate changes");
        assert_eq!(totals(&timeline).1, None);
        assert!(lanes(&timeline).lanes.iter().all(|lane| lane.key != "runs"));
        let html = html(timeline);
        assert!(!html.contains("Machine"), "{html}");
        assert!(!html.contains("data-lane=\"runs\""), "{html}");
        assert!(!html.contains("tl-running"), "{html}");
        assert!(!html.contains("running now"), "{html}");
    }

    #[test]
    fn a_timeline_over_two_days_marks_each_days_first_row() {
        let pictures = pictures();
        // Made at 23:30 the day before, ready until 09:43.
        let made = pictures.created - (9.0 * 3600.0 + 32.0 * 60.0);
        let mut timeline = TaskTimeline {
            created: made,
            ..pictures
        };
        timeline.stages[0].from = made;
        let rows = rows(&timeline);
        let days: Vec<_> = rows
            .iter()
            .map(|row| (row.at.as_str(), row.day.as_deref()))
            .filter(|(_, day)| day.is_some())
            .collect();
        assert_eq!(days, [("23:30", Some("11-10")), ("09:43", Some("11-11"))]);
        assert_eq!(rows[0].label, "Ready, unclaimed");
        assert_eq!(rows[0].took.as_deref(), Some("10 h 13 m"));
    }

    #[test]
    fn a_stage_says_who_moved_it_only_where_its_name_says_nothing_else() {
        let pictures = pictures();
        let stage = |status: &str, by: &str| Stage {
            status: status.into(),
            from: pictures.created,
            to: Some(pictures.created + 600.0),
            by: Some(by.into()),
            ..Stage::default()
        };
        let timeline = TaskTimeline {
            first_claimed: Some(pictures.created + 300.0),
            stages: vec![
                stage("ready", "glacial-merlin"),
                stage("blocked", "cobalt-otter"),
                Stage {
                    to: None,
                    ..stage("done", "lucid-heron")
                },
            ],
            rounds: Vec::new(),
            runs: Vec::new(),
            gates: Vec::new(),
            closed: Some(pictures.created + 600.0),
            ..pictures
        };
        let rows = rows(&timeline);
        assert_eq!(
            said(&rows),
            [
                ("09:02", "Ready", "10 m", "", false),
                ("09:02", "Blocked · cobalt-otter", "10 m", "", false),
                ("09:02", "Done · lucid-heron", "", "", false),
            ]
            .map(|(at, label, took, word, sub)| (
                at.into(),
                label.into(),
                took.into(),
                word.into(),
                sub
            ))
        );
        assert_eq!((rows[1].tone, rows[2].tone), ("dislike", "like"));
        assert!(rows[2].bar.is_none(), "a close is an instant");
        assert!(lanes(&timeline).now.is_none(), "a closed task has no now");
    }

    #[test]
    fn the_wide_lanes_fit_their_words_to_their_width_and_mark_the_hours_and_now() {
        let lanes = lanes(&pictures());
        let ticks: Vec<_> = lanes.ticks.iter().map(|tick| tick.label.as_str()).collect();
        assert_eq!(
            ticks,
            [
                "09:00", "10:00", "11:00", "12:00", "13:00", "14:00", "15:00"
            ]
        );
        assert!(lanes.ticks[0].left.abs() < 1e-9);
        assert!((lanes.ticks[6].left - 360.0 / 380.0 * 100.0).abs() < 1e-9);
        let now = lanes.now.unwrap();
        assert_eq!((now.left, now.label.as_str()), (100.0, "now 15:20"));
        let lane = |key: &str| lanes.lanes.iter().find(|lane| lane.key == key).unwrap();
        let texts = |key: &str| -> Vec<String> {
            lane(key).segs.iter().map(|seg| seg.text.clone()).collect()
        };
        assert_eq!(
            lanes
                .lanes
                .iter()
                .map(|lane| (lane.name, lane.who.as_str()))
                .collect::<Vec<_>>(),
            [
                ("Stage", ""),
                ("Executor", "quiet-wren"),
                ("Reviewer", "lucid-heron"),
                ("Builds & tests", "studio"),
            ]
        );
        assert_eq!(
            texts("stage"),
            [
                "ready 41 m",
                "in progress 1 h 52 m",
                "review 52 m",
                "in progress 1 h 10 m",
                "28 m",
                "waiting to land 1 h 15 m",
            ]
        );
        assert_eq!(lane("stage").segs[5].look, Look::Waiting);
        assert_eq!(texts("executor"), ["claimed 1 h 52 m", "1 h 10 m"]);
        assert_eq!(texts("reviewer"), ["", "round 1", "", "round 2"]);
        let after: Vec<_> = lane("reviewer")
            .segs
            .iter()
            .filter_map(|seg| seg.after)
            .collect();
        assert_eq!(after, [("changes", "dislike"), ("approved", "like")]);
        let under: Vec<_> = lane("runs")
            .segs
            .iter()
            .filter_map(|seg| seg.under.as_ref().map(|(under, _)| under.as_str()))
            .collect();
        assert_eq!(
            under,
            [
                "build 4 m",
                "verify 24 m",
                "verify 23 m",
                "certificate 24 m",
                "running 18 m"
            ]
        );
        let looks: Vec<_> = lane("runs").segs.iter().map(|seg| seg.look).collect();
        assert_eq!(
            looks.iter().filter(|look| **look == Look::Running).count(),
            1
        );
        assert_eq!(looks.last(), Some(&Look::Running));
        let gates: Vec<_> = lanes
            .gates
            .iter()
            .map(|gate| {
                (
                    gate.name.as_str(),
                    gate.spans.iter().map(|span| span.2).collect::<Vec<_>>(),
                )
            })
            .collect();
        assert_eq!(
            gates,
            [
                (
                    "certificate",
                    vec![GateState::Missing, GateState::Met, GateState::Missing]
                ),
                ("dependency", vec![GateState::Missing, GateState::Waived]),
                ("nightly", vec![GateState::Met]),
                ("review", vec![GateState::Missing, GateState::Met]),
            ]
        );
        // The nightly, met before the task, holds from the hour drawn first to now.
        let nightly = &lanes.gates[2].spans[0];
        assert!(
            (nightly.0 - 2.0 / 380.0 * 100.0).abs() < 1e-9
                && (nightly.0 + nightly.1 - 100.0).abs() < 1e-9
        );
    }

    #[test]
    fn a_long_timeline_marks_fewer_hours_and_a_midnight_by_its_date() {
        let pictures = pictures();
        let made = pictures.now - 30.0 * 3600.0;
        let mut timeline = TaskTimeline {
            created: made,
            ..pictures
        };
        timeline.stages[0].from = made;
        let ticks: Vec<_> = lanes(&timeline)
            .ticks
            .into_iter()
            .map(|tick| tick.label)
            .collect();
        assert_eq!(
            ticks,
            [
                "12:00", "16:00", "20:00", "11-11", "04:00", "08:00", "12:00"
            ]
        );
        // A step of days marks each midnight by its date.
        let made = pictures.now - 40.0 * 86_400.0;
        let mut timeline = TaskTimeline {
            created: made,
            ..timeline
        };
        timeline.stages[0].from = made;
        let ticks: Vec<_> = lanes(&timeline)
            .ticks
            .into_iter()
            .map(|tick| tick.label)
            .collect();
        assert_eq!(
            ticks,
            ["10-03", "10-10", "10-17", "10-24", "10-31", "11-07"]
        );
    }

    #[test]
    fn the_host_render_draws_the_list_the_lanes_and_what_the_bars_mean() {
        let html = html(pictures());
        assert!(
            html.contains("6 h 18 m so far · 2 review rounds · 1 bounce"),
            "{html}"
        );
        assert!(html.contains("Machine 1 h 33 m, queued 26 m"), "{html}");
        assert_eq!(html.matches("<li class=\"tl-row").count(), 14, "{html}");
        assert_eq!(
            html.matches("class=\"tl-row tl-sub tl-event\"").count(),
            3,
            "{html}"
        );
        for lane in ["stage", "executor", "reviewer", "runs"] {
            assert!(
                html.contains(&format!("data-lane=\"{lane}\"")),
                "{lane}: {html}"
            );
        }
        assert_eq!(
            html.matches("class=\"tl-lane tl-gate\"").count(),
            4,
            "{html}"
        );
        // What runs now: striped in the list and in its lane, and the legend says so.
        assert_eq!(html.matches("tl-work tl-running").count(), 1, "{html}");
        assert_eq!(html.matches("tl-seg tl-running").count(), 1, "{html}");
        assert!(html.contains("running now"), "{html}");
        assert!(html.contains(">now 15:20<"), "{html}");
        assert!(
            html.contains(">changes<") && html.contains(">approved<"),
            "{html}"
        );
        assert!(html.contains("class=\"tl-g tl-g-waived\""), "{html}");
        assert!(html.contains("--c:var(--question)"), "{html}");
    }

    #[test]
    fn the_style_sheet_shows_the_list_on_a_phone_and_the_lanes_from_900_px_in_theme_colours() {
        let (phone, wide) = STYLE
            .split_once("@media (min-width: 900px)")
            .expect("one width switches the views");
        assert!(phone.contains(".tl-lanes { --name:9rem; --end:5.5rem; display:none;"));
        assert!(wide.contains(".tl-list { display:none; }"));
        assert!(wide.contains(".tl-lanes { display:block; }"));
        // Every colour is a theme token (mixed in sRGB at most), so an imported theme colours the
        // timeline too.
        for written in ["#", "rgb(", "rgba(", "hsl("] {
            assert!(
                !STYLE.contains(written),
                "a colour written out ({written}): {STYLE}"
            );
        }
    }

    #[test]
    fn the_panel_is_a_section_near_the_screen_that_says_it_reads_until_its_answer_comes() {
        fn page() -> Element {
            rsx! { TaskTimelinePanel { slug: "tools/new", revision: 1.0 } }
        }
        let mut dom = VirtualDom::new(page);
        dom.rebuild_in_place();
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("id=\"timeline-tools/new\""), "{html}");
        assert!(html.contains("<h2>Timeline</h2>"), "{html}");
        assert!(html.contains(">Reading the timeline…</p>"), "{html}");
        assert!(!html.contains("role=\"alert\""), "{html}");
    }

    #[test]
    fn a_task_with_nothing_but_its_making_draws_one_stage_and_no_other_lane() {
        let now = 1_794_403_200.0;
        let timeline = TaskTimeline {
            slug: "tools/new".into(),
            now,
            offset: 7200,
            created: now - 120.0,
            stages: vec![Stage {
                status: "ready".into(),
                from: now - 120.0,
                ..Stage::default()
            }],
            ..TaskTimeline::default()
        };
        let rows = rows(&timeline);
        assert_eq!(
            said(&rows),
            [(
                "15:18".into(),
                "Ready, unclaimed".into(),
                "2 m".into(),
                "now".into(),
                false
            )]
        );
        let lanes = lanes(&timeline);
        assert_eq!(lanes.lanes.len(), 1);
        assert!(lanes.gates.is_empty());
        let html = html(timeline);
        assert!(
            html.contains("2 m so far · 0 review rounds · 0 bounces"),
            "{html}"
        );
        assert!(!html.contains("gate met"), "{html}");
    }
}
