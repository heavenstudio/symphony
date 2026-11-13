//! The Reports page, `/reports`, a page kind of its own (`page-reports`): how the project's work
//! flows, how many agents work it and in which roles, its decisions, its code and its board's
//! health, by the hour, the day or the week, so a change to a gate or a limit shows in the speed
//! and the quality of what lands (the owner, 2026-11-13). It draws the one read
//! [`symphony_api::REPORTS_PATH`] answers ([`symphony_api::report_page`]), summed by the server from
//! the project's day files: four figures over the window, then the charts on one time axis, two
//! columns on a wide screen. A period a source has no record of is a gap, hatched, never a zero.

use dioxus::prelude::*;
use symphony_api::agent_activity::RoleLevel;
use symphony_api::report_page::{Grain, ReportBucket, ReportsPage};

mod chart;
use chart::float;
pub use chart::{
    Axis, ChartCard, ChartData, GhostChart, Mark, Scale, Segment, SeriesData, Unit, coverage, runs,
    segments, shown,
};

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

/// Where a series' value comes from: none where its source has no record of the period.
struct Series {
    name: &'static str,
    mark: Mark,
    value: fn(&ReportBucket) -> Option<f64>,
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

/// The mean of the sessions running work in `role` (`orchestrator`, `executor`, `reviewer`), or
/// every other role's together for `None`.
fn role(levels: &[RoleLevel], role: Option<&str>) -> f64 {
    const NAMED: [&str; 3] = ["orchestrator", "executor", "reviewer"];
    levels
        .iter()
        .filter(|level| match role {
            Some(role) => level.role == role,
            None => !NAMED.contains(&level.role.as_str()),
        })
        .map(|level| level.mean)
        .sum()
}

/// A count as a float: far below 2^52.
#[expect(clippy::cast_precision_loss, reason = "a count far below 2^52")]
fn float_u64(count: u64) -> f64 {
    count as f64
}

/// The page's sections, each a heading and its charts.
const SECTIONS: [(&str, &[Spec]); 5] = [
    (
        "Flow",
        &[
            Spec {
                id: "done",
                title: "Tasks done",
                note: "Top-level tasks that reached done, per {per}.",
                unit: Unit::Count,
                source: Source::Rollup,
                series: &[Series {
                    name: "Done",
                    mark: Mark::Bar,
                    value: |b| b.flow.map(|f| f64::from(f.entered.done)),
                }],
            },
            Spec {
                id: "open",
                title: "Open work",
                note: "Top-level tasks in each open status at the end of each {per}.",
                unit: Unit::Count,
                source: Source::Rollup,
                series: &[
                    Series {
                        name: "Ready",
                        mark: Mark::Bar,
                        value: |b| b.flow.map(|f| f64::from(f.held.ready)),
                    },
                    Series {
                        name: "In progress",
                        mark: Mark::Bar,
                        value: |b| b.flow.map(|f| f64::from(f.held.in_progress)),
                    },
                    Series {
                        name: "Review",
                        mark: Mark::Bar,
                        value: |b| b.flow.map(|f| f64::from(f.held.review)),
                    },
                    Series {
                        name: "Blocked",
                        mark: Mark::Bar,
                        value: |b| b.flow.map(|f| f64::from(f.held.blocked)),
                    },
                ],
            },
            Spec {
                id: "task-hours",
                title: "Task-hours",
                note: "Hours top-level tasks spent in each open status, per {per}.",
                unit: Unit::Hours,
                source: Source::Rollup,
                series: &[
                    Series {
                        name: "Ready",
                        mark: Mark::Bar,
                        value: |b| b.flow.map(|f| f.task_hours.ready),
                    },
                    Series {
                        name: "In progress",
                        mark: Mark::Bar,
                        value: |b| b.flow.map(|f| f.task_hours.in_progress),
                    },
                    Series {
                        name: "Review",
                        mark: Mark::Bar,
                        value: |b| b.flow.map(|f| f.task_hours.review),
                    },
                    Series {
                        name: "Blocked",
                        mark: Mark::Bar,
                        value: |b| b.flow.map(|f| f.task_hours.blocked),
                    },
                ],
            },
            Spec {
                id: "lead",
                title: "Lead time",
                note: "Creation to close of the top-level tasks closed in each {per}.",
                unit: Unit::Seconds,
                source: Source::Rollup,
                series: &[
                    Series {
                        name: "Median",
                        mark: Mark::Line,
                        value: |b| b.lead.map(|s| s.median),
                    },
                    Series {
                        name: "p90",
                        mark: Mark::Line,
                        value: |b| b.lead.map(|s| s.p90),
                    },
                ],
            },
            Spec {
                id: "waits",
                title: "Waits between hands",
                note: "The median wait that ended in each {per}, by kind.",
                unit: Unit::Seconds,
                source: Source::Rollup,
                series: &[
                    Series {
                        name: "Review to reviewer",
                        mark: Mark::Line,
                        value: |b| b.waits.and_then(|w| w.review_to_reviewer).map(|s| s.median),
                    },
                    Series {
                        name: "Changes to resumed",
                        mark: Mark::Line,
                        value: |b| b.waits.and_then(|w| w.changes_to_resumed).map(|s| s.median),
                    },
                    Series {
                        name: "Changes to review",
                        mark: Mark::Line,
                        value: |b| b.waits.and_then(|w| w.changes_to_review).map(|s| s.median),
                    },
                    Series {
                        name: "Approved to landed",
                        mark: Mark::Line,
                        value: |b| b.waits.and_then(|w| w.approved_to_landed).map(|s| s.median),
                    },
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
                        value: |b| {
                            b.agents
                                .as_ref()
                                .map(|a| role(&a.running, Some("orchestrator")))
                        },
                    },
                    Series {
                        name: "Executors",
                        mark: Mark::Bar,
                        value: |b| {
                            b.agents
                                .as_ref()
                                .map(|a| role(&a.running, Some("executor")))
                        },
                    },
                    Series {
                        name: "Reviewers",
                        mark: Mark::Bar,
                        value: |b| {
                            b.agents
                                .as_ref()
                                .map(|a| role(&a.running, Some("reviewer")))
                        },
                    },
                    Series {
                        name: "Other roles",
                        mark: Mark::Bar,
                        value: |b| b.agents.as_ref().map(|a| role(&a.running, None)),
                    },
                    Series {
                        name: "Sessions, each once",
                        mark: Mark::Line,
                        value: |b| b.agents.as_ref().map(|a| a.running_agents.mean),
                    },
                ],
            },
            Spec {
                id: "sessions",
                title: "Sessions",
                note: "Sessions that connected, and changes to stale and to lost, per {per}.",
                unit: Unit::Count,
                source: Source::Agents,
                series: &[
                    Series {
                        name: "Started",
                        mark: Mark::Line,
                        value: |b| b.agents.as_ref().map(|a| float_u64(a.started)),
                    },
                    Series {
                        name: "Went stale",
                        mark: Mark::Line,
                        value: |b| b.agents.as_ref().map(|a| float_u64(a.went_stale)),
                    },
                    Series {
                        name: "Went lost",
                        mark: Mark::Line,
                        value: |b| b.agents.as_ref().map(|a| float_u64(a.went_lost)),
                    },
                ],
            },
            Spec {
                id: "load",
                title: "Machine load",
                note: "The machine's load average over a minute, its peak and mean in each {per}.",
                unit: Unit::Load,
                source: Source::Agents,
                series: &[
                    Series {
                        name: "Peak",
                        mark: Mark::Line,
                        value: |b| b.agents.as_ref().and_then(|a| a.load_1m).map(|g| g.peak),
                    },
                    Series {
                        name: "Mean",
                        mark: Mark::Line,
                        value: |b| b.agents.as_ref().and_then(|a| a.load_1m).map(|g| g.mean),
                    },
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
                    Series {
                        name: "Answered",
                        mark: Mark::Bar,
                        value: |b| b.decisions.map(|d| f64::from(d.answered)),
                    },
                    Series {
                        name: "Auto-approved",
                        mark: Mark::Bar,
                        value: |b| b.decisions.map(|d| f64::from(d.auto_approved)),
                    },
                    Series {
                        name: "Open",
                        mark: Mark::Line,
                        value: |b| b.decisions.map(|d| f64::from(d.open)),
                    },
                ],
            },
            Spec {
                id: "commits",
                title: "Commits",
                note: "Commits that reached the default branch, and its merges, per {per}.",
                unit: Unit::Count,
                source: Source::Rollup,
                series: &[
                    Series {
                        name: "Commits",
                        mark: Mark::Bar,
                        value: |b| b.git.map(|g| f64::from(g.commits)),
                    },
                    Series {
                        name: "Merges",
                        mark: Mark::Bar,
                        value: |b| b.git.map(|g| f64::from(g.merges)),
                    },
                ],
            },
            Spec {
                id: "lines",
                title: "Lines changed",
                note: "Lines added and removed on the default branch, per {per}; generated files \
                       and lock files left out.",
                unit: Unit::Count,
                source: Source::Rollup,
                series: &[
                    Series {
                        name: "Added",
                        mark: Mark::Bar,
                        value: |b| b.git.map(|g| float_u64(g.added)),
                    },
                    Series {
                        name: "Removed",
                        mark: Mark::Below,
                        value: |b| b.git.map(|g| float_u64(g.removed)),
                    },
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
                    Series {
                        name: "Bounces",
                        mark: Mark::Line,
                        value: |b| b.flow.map(|f| f64::from(f.bounces)),
                    },
                    Series {
                        name: "Reds filed",
                        mark: Mark::Line,
                        value: |b| b.flow.map(|f| f64::from(f.reds_filed)),
                    },
                    Series {
                        name: "Reverts",
                        mark: Mark::Line,
                        value: |b| b.git.map(|g| f64::from(g.reverts)),
                    },
                    Series {
                        name: "Changes asked",
                        mark: Mark::Line,
                        value: |b| b.verdicts.map(|v| f64::from(v.changes)),
                    },
                ],
            },
            Spec {
                id: "fix",
                title: "Time to fix a red",
                note: "Filing to close of the reds closed done in each {per}.",
                unit: Unit::Seconds,
                source: Source::Rollup,
                series: &[
                    Series {
                        name: "Median",
                        mark: Mark::Line,
                        value: |b| b.red_fix.map(|s| s.median),
                    },
                    Series {
                        name: "p90",
                        mark: Mark::Line,
                        value: |b| b.red_fix.map(|s| s.p90),
                    },
                ],
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
                Series {
                    name: "Slow",
                    mark: Mark::Line,
                    value: |b| b.health.as_ref().map(|h| float_u64(h.slow)),
                },
                Series {
                    name: "Unanswered",
                    mark: Mark::Line,
                    value: |b| b.health.as_ref().map(|h| float_u64(h.unanswered)),
                },
                Series {
                    name: "Failed passes",
                    mark: Mark::Line,
                    value: |b| {
                        b.health
                            .as_ref()
                            .map(|h| float_u64(h.dispatch.failed + h.sweep.failed))
                    },
                },
            ],
        }],
    ),
];

/// Every section's heading and its charts, ready to draw, out of `page`.
pub fn charts(page: &ReportsPage) -> Vec<(&'static str, Vec<ChartData>)> {
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
                            values: page.buckets.iter().map(series.value).collect(),
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
fn cover(bucket: &ReportBucket, source: Source) -> f64 {
    let length = (bucket.end - bucket.start).max(1.0);
    let seconds = match source {
        Source::Rollup => f64::from(bucket.rolled) * HOUR,
        Source::Agents => bucket.agents.as_ref().map_or(0.0, |a| a.observed_s),
        Source::Health => bucket.health.as_ref().map_or(0.0, |h| h.covered_s),
    };
    (seconds / length).clamp(0.0, 1.0)
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
pub fn axis(page: &ReportsPage) -> Axis {
    let count = page.buckets.len();
    let every = count.div_ceil(6).max(1);
    let marks = page
        .marks
        .iter()
        .filter_map(|mark| {
            let at = page
                .buckets
                .iter()
                .position(|b| b.start <= mark.at && mark.at < b.end)?;
            let bucket = &page.buckets[at];
            let within = (mark.at - bucket.start) / (bucket.end - bucket.start).max(1.0);
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
        page.buckets
            .iter()
            .enumerate()
            .filter(|(_, bucket)| {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "an hour since 1970 fits an i64"
                )]
                let hour = (bucket.start / HOUR).round() as i64;
                hour.rem_euclid(step) == 0
            })
            .map(|(at, bucket)| (at, tick(page.grain, bucket.start)))
            .collect()
    } else {
        // Days and weeks counted back from the last, which is always labelled.
        let mut ticks: Vec<(usize, String)> = (0..count)
            .rev()
            .step_by(every)
            .map(|at| (at, tick(page.grain, page.buckets[at].start)))
            .collect();
        ticks.reverse();
        ticks
    };
    Axis {
        periods: page
            .buckets
            .iter()
            .map(|b| period(page.grain, b.start))
            .collect(),
        hours: page
            .buckets
            .iter()
            .map(|b| ((b.end - b.start) / HOUR).max(0.0))
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
pub fn figures(page: &ReportsPage) -> [Figure; 4] {
    let per = page.grain.word();
    let flows: Vec<_> = page.buckets.iter().filter_map(|b| b.flow).collect();
    let done: u32 = flows.iter().map(|f| f.entered.done).sum();
    let done = if flows.is_empty() {
        missing("Done", "no roll-up in the window")
    } else {
        Figure {
            label: "Done",
            value: done.to_string(),
            detail: format!(
                "{} a {per} on average",
                Unit::Mean.format(((f64::from(done) / float(flows.len())) * 10.0).round() / 10.0)
            ),
        }
    };
    let leads: Vec<_> = page.buckets.iter().filter_map(|b| b.lead).collect();
    let closed: u32 = leads.iter().map(|lead| lead.n).sum();
    let lead = if page.grain == Grain::Hour {
        missing("Lead time", "kept by the day")
    } else if closed == 0 {
        missing("Lead time", "no task closed in the window")
    } else {
        let median = leads
            .iter()
            .map(|lead| lead.median * f64::from(lead.n))
            .sum::<f64>()
            / f64::from(closed);
        let p90 = leads.iter().map(|lead| lead.p90).fold(0.0, f64::max);
        Figure {
            label: "Lead time",
            value: Unit::Seconds.format(median),
            detail: format!(
                "median of {closed} closed · p90 up to {}",
                Unit::Seconds.format(p90)
            ),
        }
    };
    let watched: Vec<_> = page
        .buckets
        .iter()
        .filter_map(|b| b.agents.as_ref())
        .collect();
    let seconds: f64 = watched.iter().map(|a| a.observed_s).sum();
    let agents = if seconds <= 0.0 {
        missing("Agents at work", "no agents' hours in the window")
    } else {
        let mean = watched
            .iter()
            .map(|a| a.running_agents.mean * a.observed_s)
            .sum::<f64>()
            / seconds;
        let peak = watched
            .iter()
            .map(|a| a.running_agents.peak)
            .max()
            .unwrap_or(0);
        Figure {
            label: "Agents at work",
            value: Unit::Mean.format((mean * 10.0).round() / 10.0),
            detail: format!("running on average · {peak} at most"),
        }
    };
    let cards: Vec<_> = page.buckets.iter().filter_map(|b| b.decisions).collect();
    let decisions = match cards.last() {
        None => missing("Open decisions", "no cards read in the window"),
        Some(last) => Figure {
            label: "Open decisions",
            value: last.open.to_string(),
            detail: format!(
                "{} auto-approved · {} answered in the window",
                cards.iter().map(|c| c.auto_approved).sum::<u32>(),
                cards.iter().map(|c| c.answered).sum::<u32>()
            ),
        },
    };
    [done, lead, agents, decisions]
}

/// Whether any source recorded any period of `page`.
fn recorded(page: &ReportsPage) -> bool {
    page.buckets
        .iter()
        .any(|b| b.flow.is_some() || b.agents.is_some() || b.health.is_some() || b.git.is_some())
}

/// What the page says while the roll-up has written no file: when the next one is due.
pub fn first_roll_up(page: &ReportsPage) -> String {
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
    page: Option<ReportsPage>,
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
                Controls { grain, days, onask }
            }
            if let Some(error) = error {
                p { class: "reports-error", role: "alert", "The reports could not be read: {error}" }
            }
            match page {
                None => rsx! { GhostReports {} },
                // A new window is a new axis: the period tapped on the last one means nothing on it.
                Some(page) => rsx! { Report { key: "{page.grain.word()}-{page.days}-{page.from}", page } },
            }
        }
    }
}

#[component]
fn Controls(grain: Grain, days: u32, onask: EventHandler<(Grain, u32)>) -> Element {
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
fn Report(page: ReportsPage) -> Element {
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
                            crate::viewport::NearViewport { key: "{chart.id}", height: 330, class: "chart-slot", ghost: rsx! { GhostChart {} },
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
#[component]
fn GhostReports() -> Element {
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
            for n in 0..2 { GhostChart { key: "{n}" } }
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
#[cfg(all(target_arch = "wasm32", feature = "page-reports"))]
#[component]
pub fn ReportsRoute() -> Element {
    use crate::request::{self, Answer};
    let mut window = use_signal(|| chosen(|key| crate::platform::current().load(key)));
    let mut page = use_signal(|| None::<ReportsPage>);
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
                        match serde_wasm_bindgen::from_value::<ReportsPage>(value) {
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
