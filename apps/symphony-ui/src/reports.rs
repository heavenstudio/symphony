//! The Reports page, `/reports`: how the project's work flows, how many agents work it and in which
//! roles, its decisions, its code and its board's health, by the hour, the day or the week, so a
//! change to a gate or a limit shows in the speed and the quality of what lands (the owner,
//! 2026-11-13). It draws the one read [`symphony_api::REPORTS_PATH`] answers
//! ([`symphony_api::report_page`]), summed by the server from the project's day files: four figures
//! over the window, then the charts on one time axis, two columns on a wide screen. A period a
//! source has no record of is a gap, hatched, never a zero.
//!
//! The task pages' module draws it (`page-tasks`), as it draws the Agents page: a module of its own
//! would carry the whole runtime again, some 560 KB in each build the distribution keeps, where
//! that module already holds all but the page's own code.

use dioxus::prelude::*;
use serde::Deserialize;
use symphony_api::report_page::{Grain, ReportMark};

mod chart;
pub use chart::{
    Axis, ChartCard, ChartData, Mark, Scale, Segment, SeriesData, Unit, coverage, ghost_chart,
    runs, segments, shown,
};
use chart::{float, short};

const STYLE: Asset = asset!(
    "/assets/reports.css",
    AssetOptions::css().with_static_head(true)
);

const HOUR: f64 = 3_600.0;
const DAY: f64 = 86_400.0;

/// The windows the page offers for each grain, in days.
pub fn ranges(grain: Grain) -> &'static [u32] {
    match grain {
        Grain::Hour => &[1, 2, 7],
        Grain::Day => &[7, 14, 30, 90],
        Grain::Week => &[28, 84],
    }
}

/// A window's name on its button: `24 hours`, `14 days`, `12 weeks`.
pub fn range_name(grain: Grain, days: u32) -> String {
    match (grain, days) {
        (Grain::Hour, 1) => "24 hours".into(),
        (Grain::Week, days) => format!("{} weeks", days / 7),
        (_, days) => format!("{days} days"),
    }
}

/// The grain's name on its button and in the page's words.
fn grain_name(grain: Grain) -> &'static str {
    match grain {
        Grain::Hour => "Hours",
        Grain::Day => "Days",
        Grain::Week => "Weeks",
    }
}

/// A value of the answer as the browser parsed it (kept by `serde_wasm_bindgen::preserve`), or as
/// `serde_json` did on the host.
#[cfg(target_arch = "wasm32")]
type Raw = wasm_bindgen::JsValue;
#[cfg(not(target_arch = "wasm32"))]
type Raw = serde_json::Value;

/// The member `key` of `value`: none where it has none, `null` kept.
#[cfg(target_arch = "wasm32")]
fn member(value: &Raw, key: &str) -> Option<Raw> {
    js_sys::Reflect::get(value, &key.into())
        .ok()
        .filter(|found| !found.is_undefined())
}
#[cfg(not(target_arch = "wasm32"))]
fn member(value: &Raw, key: &str) -> Option<Raw> {
    value.get(key).cloned()
}

/// `value`'s items, none where it is no list.
#[cfg(target_arch = "wasm32")]
fn items(value: &Raw) -> Vec<Raw> {
    if js_sys::Array::is_array(value) {
        js_sys::Array::from(value).iter().collect()
    } else {
        Vec::new()
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn items(value: &Raw) -> Vec<Raw> {
    value.as_array().cloned().unwrap_or_default()
}

/// `value` as text, none where it is no string.
#[cfg(target_arch = "wasm32")]
fn text(value: &Raw) -> Option<String> {
    value.as_string()
}
#[cfg(not(target_arch = "wasm32"))]
fn text(value: &Raw) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// One period of the answer ([`symphony_api::report_page::ReportBucket`] on the wire), read by
/// path where the page needs a value rather than into the wire's structs: their deserializer
/// alone would weigh more than the rest of the page in each build the distribution keeps.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct Bucket(
    #[cfg_attr(target_arch = "wasm32", serde(with = "serde_wasm_bindgen::preserve"))] Raw,
);

impl Bucket {
    /// The number at `path` (`flow.held.ready`): none where a member on the way is missing or
    /// `null`, a source with no record of the period, and zero where only the last is missing, as
    /// a count by status leaves out its zeros.
    pub fn number(&self, path: &str) -> Option<f64> {
        let mut value = self.0.clone();
        let mut keys = path.split('.').peekable();
        while let Some(key) = keys.next() {
            match member(&value, key) {
                Some(found) if !found.is_null() => value = found,
                None if keys.peek().is_none() && value.is_object() => return Some(0.0),
                _ => return None,
            }
        }
        value.as_f64()
    }

    /// Whether the source `key` (`flow`, `agents`, `health`…) recorded the period.
    pub fn has(&self, key: &str) -> bool {
        member(&self.0, key).is_some_and(|found| !found.is_null())
    }

    /// The items of the list at `key` of the member `source`, each read the same way.
    fn list(&self, source: &str, key: &str) -> Vec<Bucket> {
        member(&self.0, source)
            .and_then(|found| member(&found, key))
            .map(|found| items(&found).into_iter().map(Bucket).collect())
            .unwrap_or_default()
    }

    /// The text at `key`, empty where there is none.
    fn text(&self, key: &str) -> String {
        member(&self.0, key)
            .and_then(|found| text(&found))
            .unwrap_or_default()
    }

    /// Its first second, and its end.
    fn span(&self) -> (f64, f64) {
        let start = self.number("start").unwrap_or(0.0);
        (start, self.number("end").unwrap_or(start))
    }
}

/// The answer of [`symphony_api::REPORTS_PATH`] as the page reads it: the window and what it says
/// of the periods ([`symphony_api::report_page::ReportsPage`] on the wire), each period kept as
/// the browser parsed it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Page {
    pub grain: Grain,
    pub days: u32,
    pub from: f64,
    pub to: f64,
    #[serde(default)]
    pub buckets: Vec<Bucket>,
    #[serde(default)]
    pub first: Option<f64>,
    #[serde(default)]
    pub marks: Vec<ReportMark>,
    #[serde(default)]
    pub unread: Vec<String>,
}

/// Where a series' value is read from in a period.
#[derive(Clone, Copy)]
enum Pick {
    /// The number at a path of the period.
    At(&'static str),
    /// The mean of the sessions running work in a role (`orchestrator`, `executor`, `reviewer`),
    /// or every other role's together for none.
    Role(Option<&'static str>),
    /// The sum of the numbers at two paths.
    Sum(&'static str, &'static str),
}

impl Pick {
    /// The value in `bucket`: none where its source has no record of the period.
    fn of(self, bucket: &Bucket) -> Option<f64> {
        const NAMED: [&str; 3] = ["orchestrator", "executor", "reviewer"];
        match self {
            Pick::At(path) => bucket.number(path),
            Pick::Role(role) => bucket.has("agents").then(|| {
                bucket
                    .list("agents", "running")
                    .iter()
                    .filter(|level| {
                        let name = level.text("role");
                        match role {
                            Some(role) => name == role,
                            None => !NAMED.contains(&name.as_str()),
                        }
                    })
                    .filter_map(|level| level.number("mean"))
                    .sum()
            }),
            Pick::Sum(one, other) => Some(bucket.number(one)? + bucket.number(other)?),
        }
    }
}

/// A series: its name, how it is drawn and where its value is read from.
struct Series {
    name: &'static str,
    mark: Mark,
    pick: Pick,
}

/// The source a chart's coverage is read from.
#[derive(Clone, Copy)]
enum Source {
    Rollup,
    Agents,
    Health,
}

struct Spec {
    id: &'static str,
    title: &'static str,
    /// What it shows; `{per}` is the period's word.
    note: &'static str,
    unit: Unit,
    source: Source,
    series: &'static [Series],
}

/// A series drawn as `mark`, read at `path`.
const fn at(name: &'static str, mark: Mark, path: &'static str) -> Series {
    Series {
        name,
        mark,
        pick: Pick::At(path),
    }
}

/// A bar read at `path`.
const fn bar(name: &'static str, path: &'static str) -> Series {
    at(name, Mark::Bar, path)
}

/// A line read at `path`.
const fn line(name: &'static str, path: &'static str) -> Series {
    at(name, Mark::Line, path)
}

/// The page's sections, each a heading and its charts.
const SECTIONS: [(&str, &[Spec]); 5] = [
    (
        "Flow",
        &[
            Spec {
                id: "done",
                title: "Tasks done",
                note: "Top-level tasks done, per {per}, each once at its last close: one approved \
                       again after its landing sent it back counts once. Hours and days are UTC; \
                       just tasks stats counts the same tasks by this machine's days.",
                unit: Unit::Count,
                source: Source::Rollup,
                series: &[bar("Done", "flow.done")],
            },
            Spec {
                id: "open",
                title: "Open work",
                note: "Top-level tasks in each open status at the end of each {per}.",
                unit: Unit::Count,
                source: Source::Rollup,
                series: &[
                    bar("Ready", "flow.held.ready"),
                    bar("In progress", "flow.held.in_progress"),
                    bar("Review", "flow.held.review"),
                    bar("Blocked", "flow.held.blocked"),
                ],
            },
            Spec {
                id: "task-hours",
                title: "Task-hours",
                note: "Hours top-level tasks spent in each open status, per {per}.",
                unit: Unit::Hours,
                source: Source::Rollup,
                series: &[
                    bar("Ready", "flow.task_hours.ready"),
                    bar("In progress", "flow.task_hours.in_progress"),
                    bar("Review", "flow.task_hours.review"),
                    bar("Blocked", "flow.task_hours.blocked"),
                ],
            },
            Spec {
                id: "lead",
                title: "Lead time",
                note: "Creation to last close of the top-level tasks done in each {per}.",
                unit: Unit::Seconds,
                source: Source::Rollup,
                series: &[line("Median", "lead.median"), line("p90", "lead.p90")],
            },
            Spec {
                id: "waits",
                title: "Waits between hands",
                note: "The median wait that ended in each {per}, by kind.",
                unit: Unit::Seconds,
                source: Source::Rollup,
                series: &[
                    line("Review to reviewer", "waits.review_to_reviewer.median"),
                    line("Changes to resumed", "waits.changes_to_resumed.median"),
                    line("Changes to review", "waits.changes_to_review.median"),
                    line("Approved to landed", "waits.approved_to_landed.median"),
                ],
            },
        ],
    ),
    (
        "Agents",
        &[
            Spec {
                id: "agents",
                title: "Agents at work",
                note: "Sessions running a role on a task in progress or in review, the mean over \
                       each {per}; a session with two roles counts in both.",
                unit: Unit::Mean,
                source: Source::Agents,
                series: &[
                    Series {
                        name: "Orchestrators",
                        mark: Mark::Bar,
                        pick: Pick::Role(Some("orchestrator")),
                    },
                    Series {
                        name: "Executors",
                        mark: Mark::Bar,
                        pick: Pick::Role(Some("executor")),
                    },
                    Series {
                        name: "Reviewers",
                        mark: Mark::Bar,
                        pick: Pick::Role(Some("reviewer")),
                    },
                    Series {
                        name: "Other roles",
                        mark: Mark::Bar,
                        pick: Pick::Role(None),
                    },
                    line("Sessions, each once", "agents.running_agents.mean"),
                ],
            },
            Spec {
                id: "sessions",
                title: "Sessions",
                note: "Sessions that connected, and changes to stale and to lost, per {per}.",
                unit: Unit::Count,
                source: Source::Agents,
                series: &[
                    line("Started", "agents.started"),
                    line("Went stale", "agents.went_stale"),
                    line("Went lost", "agents.went_lost"),
                ],
            },
            Spec {
                id: "load",
                title: "Machine load",
                note: "The machine's load average over a minute, its peak and mean in each {per}.",
                unit: Unit::Load,
                source: Source::Agents,
                series: &[
                    line("Peak", "agents.load_1m.peak"),
                    line("Mean", "agents.load_1m.mean"),
                ],
            },
        ],
    ),
    (
        "Decisions and code",
        &[
            Spec {
                id: "decisions",
                title: "Decisions",
                note: "Cards answered and auto-approved in each {per}, and those open at its end.",
                unit: Unit::Count,
                source: Source::Rollup,
                series: &[
                    bar("Answered", "decisions.answered"),
                    bar("Auto-approved", "decisions.auto_approved"),
                    line("Open", "decisions.open"),
                ],
            },
            Spec {
                id: "commits",
                title: "Commits",
                note: "Commits that reached the default branch, and its merges, per {per}.",
                unit: Unit::Count,
                source: Source::Rollup,
                series: &[bar("Commits", "git.commits"), bar("Merges", "git.merges")],
            },
            Spec {
                id: "lines",
                title: "Lines changed",
                note: "Lines added and removed on the default branch, per {per}; generated files \
                       and lock files left out.",
                unit: Unit::Count,
                source: Source::Rollup,
                series: &[
                    bar("Added", "git.added"),
                    at("Removed", Mark::Below, "git.removed"),
                ],
            },
        ],
    ),
    (
        "Quality",
        &[
            Spec {
                id: "defects",
                title: "Bounces, reds and reverts",
                note: "Moves from review back to work, reds filed, reverts, and changes asked by \
                       review, per {per}.",
                unit: Unit::Count,
                source: Source::Rollup,
                series: &[
                    line("Bounces", "flow.bounces"),
                    line("Reds filed", "flow.reds_filed"),
                    line("Reverts", "git.reverts"),
                    line("Changes asked", "verdicts.changes"),
                ],
            },
            Spec {
                id: "fix",
                title: "Time to fix a red",
                note: "Filing to close of the reds closed done in each {per}.",
                unit: Unit::Seconds,
                source: Source::Rollup,
                series: &[line("Median", "red_fix.median"), line("p90", "red_fix.p90")],
            },
        ],
    ),
    (
        "Board health",
        &[Spec {
            id: "health",
            title: "Board requests",
            note: "Requests to the board that took a second or more, that got no answer, and \
                   failed dispatch and sweep passes, per {per}.",
            unit: Unit::Count,
            source: Source::Health,
            series: &[
                line("Slow", "health.slow"),
                line("Unanswered", "health.unanswered"),
                Series {
                    name: "Failed passes",
                    mark: Mark::Line,
                    pick: Pick::Sum("health.dispatch.failed", "health.sweep.failed"),
                },
            ],
        }],
    ),
];

/// Every section's heading and its charts, ready to draw, out of `page`.
pub fn charts(page: &Page) -> Vec<(&'static str, Vec<ChartData>)> {
    let per = page.grain.word();
    SECTIONS
        .iter()
        .map(|(heading, specs)| {
            let charts = specs
                .iter()
                .map(|spec| ChartData {
                    id: spec.id,
                    title: spec.title,
                    note: spec.note.replace("{per}", per),
                    unit: spec.unit,
                    series: spec
                        .series
                        .iter()
                        .map(|series| SeriesData {
                            name: series.name,
                            mark: series.mark,
                            values: page.buckets.iter().map(|b| series.pick.of(b)).collect(),
                        })
                        .collect(),
                    cover: page
                        .buckets
                        .iter()
                        .map(|bucket| cover(bucket, spec.source))
                        .collect(),
                    instead: (spec.id == "lead" && page.grain == Grain::Hour).then_some(
                        "The roll-up keeps lead time by the day: switch to Days or Weeks to see it.",
                    ),
                })
                .collect();
            (*heading, charts)
        })
        .collect()
}

/// How much of `bucket` `source` recorded, from 0 to 1.
fn cover(bucket: &Bucket, source: Source) -> f64 {
    let (start, end) = bucket.span();
    let seconds = match source {
        Source::Rollup => bucket.number("rolled").unwrap_or(0.0) * HOUR,
        Source::Agents => bucket.number("agents.observed_s").unwrap_or(0.0),
        Source::Health => bucket.number("health.covered_s").unwrap_or(0.0),
    };
    (seconds / (end - start).max(1.0)).clamp(0.0, 1.0)
}

/// The UTC date of `at`: its weekday's name, its day, its month's name and its hour.
fn date(at: f64) -> (&'static str, u32, &'static str, i64) {
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    #[expect(
        clippy::cast_possible_truncation,
        reason = "seconds since 1970 as whole days and hours fit an i64"
    )]
    let (days, hour) = (
        (at / DAY).floor() as i64,
        (at.rem_euclid(DAY) / HOUR).floor() as i64,
    );
    let (_, month, day) = crate::words::civil_from_days(days);
    let weekday = WEEKDAYS[usize::try_from((days + 4).rem_euclid(7)).unwrap_or(0)];
    let month = crate::words::MONTHS[usize::try_from(month.saturating_sub(1)).unwrap_or(0)];
    (weekday, day, month, hour)
}

/// A period as the readout and the table name it.
pub fn period(grain: Grain, start: f64) -> String {
    let (weekday, day, month, hour) = date(start);
    match grain {
        Grain::Hour => format!("{weekday} {day} {month}, {hour:02}:00 UTC"),
        Grain::Day => format!("{weekday} {day} {month}"),
        Grain::Week => format!("Week of {day} {month}"),
    }
}

/// A period as the axis labels it: its day, or its hour by the hour (its day at midnight).
fn tick(grain: Grain, start: f64) -> String {
    let (_, day, month, hour) = date(start);
    match grain {
        Grain::Hour if hour != 0 => format!("{hour:02}:00"),
        _ => format!("{day} {month}"),
    }
}

/// The page's one time axis out of `page`: each period's name and hours, at most six labels
/// under the plot (by the hour on the clock's round hours, else counted back from the last period)
/// and the settings changes.
pub fn axis(page: &Page) -> Axis {
    let count = page.buckets.len();
    let every = count.div_ceil(6).max(1);
    let spans: Vec<(f64, f64)> = page.buckets.iter().map(Bucket::span).collect();
    let marks = page
        .marks
        .iter()
        .filter_map(|mark| {
            let at = spans
                .iter()
                .position(|(start, end)| *start <= mark.at && mark.at < *end)?;
            let (start, end) = spans[at];
            let within = (mark.at - start) / (end - start).max(1.0);
            Some((
                (float(at) + within) / float(count) * 100.0,
                at,
                format!("{} ({})", mark.subject, mark.commit),
            ))
        })
        .collect();
    let ticks = if page.grain == Grain::Hour {
        // Hours a round step apart, on the clock's own hours: midnights, noons, every sixth.
        let step = [1_i64, 2, 3, 4, 6, 12, 24, 48]
            .into_iter()
            .find(|step| count <= 6 * usize::try_from(*step).unwrap_or(usize::MAX))
            .unwrap_or(48);
        spans
            .iter()
            .enumerate()
            .filter(|(_, (start, _))| {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "an hour since 1970 fits an i64"
                )]
                let hour = (start / HOUR).round() as i64;
                hour.rem_euclid(step) == 0
            })
            .map(|(at, (start, _))| (at, tick(page.grain, *start)))
            .collect()
    } else {
        // Days and weeks counted back from the last, which is always labelled.
        let mut ticks: Vec<(usize, String)> = (0..count)
            .rev()
            .step_by(every)
            .map(|at| (at, tick(page.grain, spans[at].0)))
            .collect();
        ticks.reverse();
        ticks
    };
    Axis {
        periods: spans
            .iter()
            .map(|(start, _)| period(page.grain, *start))
            .collect(),
        hours: spans
            .iter()
            .map(|(start, end)| ((end - start) / HOUR).max(0.0))
            .collect(),
        ticks,
        marks,
    }
}

/// One of the four figures over the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Figure {
    pub label: &'static str,
    pub value: String,
    pub detail: String,
}

fn missing(label: &'static str, why: &str) -> Figure {
    Figure {
        label,
        value: "–".into(),
        detail: why.into(),
    }
}

/// The four figures over the window: the tasks done, the lead time, the agents at work and the
/// decisions open at its end.
pub fn figures(page: &Page) -> [Figure; 4] {
    let per = page.grain.word();
    // The periods `source` recorded, and the sum of the numbers at `path` over them.
    let recorded =
        |source: &str| -> Vec<&Bucket> { page.buckets.iter().filter(|b| b.has(source)).collect() };
    let total = |of: &[&Bucket], path: &str| -> f64 {
        of.iter().filter_map(|b| b.number(path)).sum::<f64>() + 0.0
    };
    let rolled = recorded("flow");
    // The periods whose roll-up counted the tasks done: a day file may not record them.
    let flows: Vec<&Bucket> = (rolled.iter().copied())
        .filter(|b| b.number("flow.done").is_some())
        .collect();
    let done = if rolled.is_empty() {
        missing("Done", "no roll-up in the window")
    } else if flows.is_empty() {
        missing("Done", "no roll-up in the window counted them")
    } else {
        let done = total(&flows, "flow.done");
        Figure {
            label: "Done",
            value: short(done, 0),
            detail: format!(
                "{} a {per} on average",
                Unit::Mean.format((done / float(flows.len()) * 10.0).round() / 10.0)
            ),
        }
    };
    let leads = recorded("lead");
    let closed = total(&leads, "lead.n");
    let lead = if page.grain == Grain::Hour {
        missing("Lead time", "kept by the day")
    } else if closed <= 0.0 {
        missing("Lead time", "no task closed in the window")
    } else {
        let median = leads
            .iter()
            .filter_map(|b| Some(b.number("lead.median")? * b.number("lead.n")?))
            .sum::<f64>()
            / closed;
        let p90 = leads
            .iter()
            .filter_map(|b| b.number("lead.p90"))
            .fold(0.0, f64::max);
        Figure {
            label: "Lead time",
            value: Unit::Seconds.format(median),
            detail: format!(
                "median of {} closed · p90 up to {}",
                short(closed, 0),
                Unit::Seconds.format(p90)
            ),
        }
    };
    let watched = recorded("agents");
    let seconds = total(&watched, "agents.observed_s");
    let agents = if seconds <= 0.0 {
        missing("Agents at work", "no agents' hours in the window")
    } else {
        let mean = watched
            .iter()
            .filter_map(|b| {
                Some(b.number("agents.running_agents.mean")? * b.number("agents.observed_s")?)
            })
            .sum::<f64>()
            / seconds;
        let peak = watched
            .iter()
            .filter_map(|b| b.number("agents.running_agents.peak"))
            .fold(0.0, f64::max);
        Figure {
            label: "Agents at work",
            value: Unit::Mean.format((mean * 10.0).round() / 10.0),
            detail: format!("running on average · {} at most", short(peak, 0)),
        }
    };
    let cards = recorded("decisions");
    let decisions = match cards.last() {
        None => missing("Open decisions", "no cards read in the window"),
        Some(last) => Figure {
            label: "Open decisions",
            value: short(last.number("decisions.open").unwrap_or(0.0), 0),
            detail: format!(
                "{} auto-approved · {} answered in the window",
                short(total(&cards, "decisions.auto_approved"), 0),
                short(total(&cards, "decisions.answered"), 0)
            ),
        },
    };
    [done, lead, agents, decisions]
}

/// Whether any source recorded any period of `page`.
fn recorded(page: &Page) -> bool {
    page.buckets.iter().any(|b| {
        ["flow", "agents", "health", "git"]
            .iter()
            .any(|source| b.has(source))
    })
}

/// What the page says while the roll-up has written no file: when the next one is due.
pub fn first_roll_up(page: &Page) -> String {
    let (_, _, _, hour) = date(page.to);
    format!(
        "No roll-up yet. The machine's shared server writes this project's first day file at its \
         first board tick after it starts, then once an hour: the next is due by {hour:02}:00 UTC. \
         A server of a project's own writes none."
    )
}

/// The page under its header: the window's controls, its four figures, the settings changes and
/// the charts, or the ghost of them while the first answer is on its way.
#[component]
pub fn ReportsView(
    grain: Grain,
    days: u32,
    page: Option<Page>,
    #[props(default)] error: Option<String>,
    onask: EventHandler<(Grain, u32)>,
) -> Element {
    std::hint::black_box(&STYLE);
    let busy = page
        .as_ref()
        .is_none_or(|page| page.grain != grain || page.days != days);
    let ready = if page.is_some() {
        "reports"
    } else {
        crate::trouble::ready(false, error.is_some())
    };
    rsx! {
        div { class: "reports", "data-ready": ready, "aria-busy": busy,
            header { class: "reports-head",
                div {
                    h1 { "Reports" }
                    p { class: "reports-sub",
                        "{grain_name(grain)} · the last {range_name(grain, days)} · times in UTC"
                    }
                }
                {controls(grain, days, onask)}
            }
            if let Some(error) = error {
                p { class: "reports-error", role: "alert", "The reports could not be read: {error}" }
            }
            match page {
                None => ghost_reports(),
                // A new window is a new axis: the period tapped on the last one means nothing on it.
                Some(page) => rsx! { Report { key: "{page.grain.word()}-{page.days}-{page.from}", page } },
            }
        }
    }
}

/// The grain's buttons and the window's. This and the ghosts are functions rather than
/// components, drawn again with what holds them: a component's own props and memo would only add
/// to the module.
fn controls(grain: Grain, days: u32, onask: EventHandler<(Grain, u32)>) -> Element {
    use crate::components::button::{Button, ButtonSize};
    rsx! {
        div { class: "reports-controls",
            div { role: "group", "aria-label": "Period",
                for each in [Grain::Hour, Grain::Day, Grain::Week] {
                    Button { key: "{each.word()}", size: ButtonSize::Sm, "aria-pressed": each == grain,
                        "data-grain": each.word(),
                        actions: crate::entry_events::click(move || onask.call((each, each.default_days()))),
                        "{grain_name(each)}"
                    }
                }
            }
            div { role: "group", "aria-label": "Window",
                for each in ranges(grain).iter().copied() {
                    Button { key: "{each}", size: ButtonSize::Sm, "aria-pressed": each == days,
                        "data-days": "{each}",
                        actions: crate::entry_events::click(move || onask.call((grain, each))),
                        "{range_name(grain, each)}"
                    }
                }
            }
        }
    }
}

/// One window's report. The period tapped is the same on every chart.
#[component]
fn Report(page: Page) -> Element {
    let mut tapped = use_signal(|| None::<usize>);
    let selected = tapped();
    let figures = figures(&page);
    let axis = axis(&page);
    let charts = charts(&page);
    let drawn = recorded(&page);
    let unread = page.unread.len();
    let files = crate::words::plural(u32::try_from(unread).unwrap_or(u32::MAX), "file");
    rsx! {
        if page.first.is_none() {
            section { class: "reports-empty", "aria-label": "No roll-up yet",
                p { "{first_roll_up(&page)}" }
            }
        }
        if !page.unread.is_empty() {
            details { class: "reports-unread",
                summary { "{unread} {files} not read: their hours show as gaps" }
                ul { for line in page.unread.iter() { li { key: "{line}", "{line}" } } }
            }
        }
        dl { class: "reports-figures",
            for figure in figures {
                div { key: "{figure.label}", class: "reports-figure",
                    dt { "{figure.label}" }
                    dd { class: "value", "{figure.value}" }
                    dd { class: "detail", "{figure.detail}" }
                }
            }
        }
        if !axis.marks.is_empty() {
            ul { class: "reports-marks", "aria-label": "Settings changes in the window",
                for (_, at, text) in axis.marks.iter() {
                    li { key: "{text}", "{axis.periods[*at]}: settings changed, {text}" }
                }
            }
        }
        if drawn {
            for (heading, section) in charts {
                section { key: "{heading}", class: "reports-section", "aria-label": "{heading}",
                    h2 { "{heading}" }
                    div { class: "reports-charts",
                        for chart in section {
                            crate::viewport::NearViewport { key: "{chart.id}", height: 330, class: "chart-slot", ghost: ghost_chart(),
                                ChartCard { chart, axis: axis.clone(), selected, onselect: move |at| tapped.set(Some(at)) }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The page waiting for its first answer: one status line, four figures' boxes and two charts'.
fn ghost_reports() -> Element {
    rsx! {
        p { class: "status-text", role: "status", "Loading the reports…" }
        div { class: "reports-figures ghost-figures", "aria-hidden": "true",
            for n in 0..4 {
                div { key: "{n}", class: "ghost-figure",
                    span { class: "bone", style: "--w:50%" }
                    span { class: "bone value" }
                    span { class: "bone", style: "--w:80%" }
                }
            }
        }
        div { class: "reports-charts",
            {ghost_chart()}
            {ghost_chart()}
        }
    }
}

/// The page's grain and window as this browser last chose them, else a fortnight by the day.
pub fn chosen(load: impl Fn(&str) -> Option<String>) -> (Grain, u32) {
    let grain = load("reports-grain")
        .and_then(|word| Grain::parse(&word))
        .unwrap_or_default();
    let days = load("reports-days")
        .and_then(|days| days.parse().ok())
        .filter(|days| ranges(grain).contains(days))
        .unwrap_or(grain.default_days());
    (grain, days)
}

/// The page, its read and its window: the window this browser last chose, read at once, again
/// whenever another is chosen, and every five minutes while the page is open, naming the copy
/// shown so an hour no file changed in answers `304`. A read that fails keeps what is shown and
/// says why.
#[cfg(all(target_arch = "wasm32", feature = "page-tasks"))]
#[component]
pub fn ReportsRoute() -> Element {
    use crate::request::{self, Answer};
    let mut window = use_signal(|| chosen(|key| crate::platform::current().load(key)));
    let mut page = use_signal(|| None::<Page>);
    let mut error = use_signal(|| None::<String>);
    let mut etag = use_signal(String::new);
    let mut generation = use_signal(|| 0_u64);
    use_effect(move || {
        let (grain, days) = window();
        generation += 1;
        let ticket = *generation.peek();
        spawn(async move {
            loop {
                let url = format!(
                    "{}?grain={}&days={days}",
                    symphony_api::REPORTS_PATH,
                    grain.word()
                );
                let kept = etag.peek().clone();
                let answer = request::get_if(&url, &kept).await;
                if ticket != *generation.peek() {
                    return;
                }
                match answer {
                    Ok(Answer::Same) => error.set(None),
                    Ok(Answer::Fresh(value, tag)) => {
                        match serde_wasm_bindgen::from_value::<Page>(value) {
                            Ok(read) => {
                                etag.set(tag);
                                page.set(Some(read));
                                error.set(None);
                            }
                            Err(e) => error.set(Some(e.to_string())),
                        }
                    }
                    Err(why) => error.set(Some(why)),
                }
                crate::live::sleep(300_000).await;
                if ticket != *generation.peek() {
                    return;
                }
            }
        });
    });
    let (grain, days) = window();
    rsx! {
        main { class: "reports-page",
            ReportsView { grain, days, page: page(), error: error(),
                onask: move |(grain, days): (Grain, u32)| {
                    let platform = crate::platform::current();
                    platform.save("reports-grain", grain.word());
                    platform.save("reports-days", &days.to_string());
                    etag.set(String::new());
                    window.set((grain, days));
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "reports/tests.rs"]
mod tests;
