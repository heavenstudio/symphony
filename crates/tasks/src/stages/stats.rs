//! The pipeline over a window of tasks: how many hold each status, how long the closed ones took
//! and where, how often review sent work back, what was finished each day, and what sits longest
//! now. Pure: [`summarize`] is given each task's [`Stages`] and the time.

use super::Stages;
use crate::STATUSES;
use std::collections::BTreeMap;

/// How many of the tasks that sat longest in their status are named.
pub const SLOWEST: usize = 10;
const DAY: f64 = 86_400.0;

/// One task of the window.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub slug: String,
    pub status: String,
    pub created: f64,
    /// Whether a session holds its executor role.
    pub executing: bool,
    /// The arm it was filed on ([`crate::arm`]).
    pub arm: Option<String>,
    pub stages: Stages,
}

/// The window's tasks filed on one arm ([`crate::arm`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Arm {
    pub name: String,
    pub tasks: usize,
    pub done: usize,
    /// Creation to close of its done tasks.
    pub lead: Option<Spread>,
    /// The time in review of its done tasks that were reviewed.
    pub review: Option<Spread>,
    /// How often review sent its tasks back, all of them together: its rework rounds.
    pub bounces: u32,
}

/// How a set of durations spreads, in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spread {
    /// How many durations.
    pub n: usize,
    pub median: f64,
    pub p75: f64,
    pub p90: f64,
}

/// An open task and how long it has held its status.
#[derive(Debug, Clone, PartialEq)]
pub struct Sitting {
    pub slug: String,
    pub status: String,
    pub seconds: f64,
}

/// How many tasks wait in one state, and the median of how long each has.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Waiting {
    pub count: usize,
    pub median_seconds: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub days: i64,
    pub tasks: usize,
    /// Every status, in the board's order.
    pub by_status: Vec<(String, usize)>,
    /// Creation to close of the done tasks.
    pub lead: Option<Spread>,
    /// The done tasks' wait before their first claim: a part of the statuses' time below, never
    /// added to it.
    pub unclaimed: Option<Spread>,
    /// Each open status a done task held, over the done tasks that held it.
    pub per_stage: Vec<(String, Spread)>,
    /// How many tasks review sent back how often: bounces, then tasks.
    pub bounces: Vec<(u32, usize)>,
    /// Tasks done on each day of the window, oldest first: every close given, one a task, its last
    /// while it holds done ([`super::Stages::done_at`]), whenever its task was made.
    pub done_per_day: Vec<(String, usize)>,
    /// The open tasks that held their status longest, longest first.
    pub slowest: Vec<Sitting>,
    pub blocked: Waiting,
    /// Ready, with no executor.
    pub unstaffed: Waiting,
    /// Each arm a task of the window was filed on, by name; a task filed on none is in none.
    pub arms: Vec<Arm>,
}

/// The rows filed on each arm, by name.
fn arms(rows: &[Row]) -> Vec<Arm> {
    let mut by_arm: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for row in rows {
        if let Some(arm) = &row.arm {
            by_arm.entry(arm).or_default().push(row);
        }
    }
    (by_arm.into_iter())
        .map(|(name, rows)| {
            let done = || rows.iter().filter(|r| r.status == "done");
            Arm {
                name: name.into(),
                tasks: rows.len(),
                done: done().count(),
                lead: spread(
                    done()
                        .filter_map(|r| r.stages.lead_seconds(r.created))
                        .collect(),
                ),
                review: spread(
                    done()
                        .filter_map(|r| r.stages.stage("review").map(|s| s.seconds))
                        .collect(),
                ),
                bounces: rows.iter().map(|r| r.stages.bounces).sum(),
            }
        })
        .collect()
}

/// The value `fraction` of the way through `sorted`, between its neighbours where it falls
/// between two.
fn quantile(sorted: &[f64], fraction: f64) -> f64 {
    let at = fraction * (sorted.len() - 1) as f64;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let below = at.floor() as usize;
    let above = (below + 1).min(sorted.len() - 1);
    sorted[below] + (sorted[above] - sorted[below]) * (at - below as f64)
}

/// The spread of `values`; none of none.
pub fn spread(mut values: Vec<f64>) -> Option<Spread> {
    if values.is_empty() {
        return None;
    }
    crate::order::sort_by(&mut values, &f64::total_cmp);
    Some(Spread {
        n: values.len(),
        median: quantile(&values, 0.5),
        p75: quantile(&values, 0.75),
        p90: quantile(&values, 0.9),
    })
}

fn waiting(ages: Vec<f64>) -> Waiting {
    Waiting {
        count: ages.len(),
        median_seconds: spread(ages).map(|s| s.median),
    }
}

/// The names of the days from `since` to `now`, oldest first. A day is asked for every six hours,
/// so one an hour short of or over 24 is neither skipped nor named twice.
fn days_between(since: f64, now: f64, day: &dyn Fn(f64) -> String) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut at = since;
    while at < now {
        names.push(day(at));
        at += DAY / 4.0;
    }
    names.push(day(now));
    names.dedup();
    names
}

/// The pipeline over `rows`, the tasks made in the last `days` days before `now`, and `closes`,
/// when each task done in those days was closed, whenever it was made; `day` names the day a time
/// falls on.
pub fn summarize(
    rows: &[Row],
    closes: &[f64],
    now: f64,
    days: i64,
    day: &dyn Fn(f64) -> String,
) -> Summary {
    let by_status = STATUSES
        .iter()
        .map(|status| {
            let count = rows.iter().filter(|r| r.status == *status).count();
            ((*status).to_string(), count)
        })
        .collect();
    let done: Vec<&Row> = rows.iter().filter(|r| r.status == "done").collect();
    let lead = spread(
        done.iter()
            .filter_map(|r| r.stages.lead_seconds(r.created))
            .collect(),
    );
    let unclaimed = spread(done.iter().map(|r| r.stages.unclaimed_seconds).collect());
    let per_stage = STATUSES
        .iter()
        .filter(|status| !super::CLOSED.contains(status))
        .filter_map(|status| {
            let held = done
                .iter()
                .filter_map(|r| r.stages.stage(status).map(|s| s.seconds))
                .collect();
            Some(((*status).to_string(), spread(held)?))
        })
        .collect();
    let mut bounced: BTreeMap<u32, usize> = BTreeMap::new();
    for row in rows {
        *bounced.entry(row.stages.bounces).or_default() += 1;
    }
    #[allow(clippy::cast_precision_loss)]
    let since = now - days.max(0) as f64 * DAY;
    let mut done_per_day: Vec<(String, usize)> = (days_between(since, now, day).into_iter())
        .map(|name| (name, 0))
        .collect();
    for closed in closes.iter().filter(|closed| **closed >= since) {
        let name = day(*closed);
        if let Some((_, count)) = done_per_day.iter_mut().find(|(d, _)| *d == name) {
            *count += 1;
        }
    }
    let open = || rows.iter().filter(|r| r.stages.closed.is_none());
    let age = |r: &Row| (now - r.stages.current_since).max(0.0);
    let mut slowest: Vec<Sitting> = open()
        .map(|r| Sitting {
            slug: r.slug.clone(),
            status: r.status.clone(),
            seconds: age(r),
        })
        .collect();
    crate::order::sort_by(&mut slowest, &|a, b| {
        b.seconds
            .total_cmp(&a.seconds)
            .then_with(|| a.slug.cmp(&b.slug))
    });
    slowest.truncate(SLOWEST);
    Summary {
        days,
        tasks: rows.len(),
        by_status,
        lead,
        unclaimed,
        per_stage,
        bounces: bounced.into_iter().collect(),
        done_per_day,
        slowest,
        blocked: waiting(open().filter(|r| r.status == "blocked").map(age).collect()),
        unstaffed: waiting(
            open()
                .filter(|r| r.status == "ready" && !r.executing)
                .map(age)
                .collect(),
        ),
        arms: arms(rows),
    }
}
