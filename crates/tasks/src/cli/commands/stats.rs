//! `stats`: where the pipeline's time goes ([`crate::stages::stats`]) over the top-level tasks
//! made in the last `--days` days, as text or as JSON. One read of the board
//! ([`Store::stage_records`]), whatever its size.

use super::{Context, Stop};
use crate::cli::shape;
use crate::pyjson::{self, Py};
use crate::stages::stats::{Row, Spread, Summary, Waiting, summarize};
use crate::stages::{Facts, hours, stages};
use crate::{Store, sys};

/// How far back `stats` looks unless `--days` says.
const DAYS: i64 = 14;
const DAY: f64 = 86_400.0;

pub(super) fn run(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let days = c.parsed.int("days", DAYS);
    if days < 1 {
        return Err(Stop::Refused(format!(
            "--days is a number of days, 1 or more, not {days}"
        )));
    }
    let now = store.now();
    #[allow(clippy::cast_precision_loss)]
    let since = now - days as f64 * DAY;
    let rows: Vec<Row> = store
        .stage_records(since)?
        .into_iter()
        .map(|r| {
            let facts = Facts {
                status: &r.status,
                created: r.created,
                updated: r.updated,
            };
            let stages = stages(facts, &r.lines, now);
            Row {
                slug: r.slug,
                status: r.status,
                created: r.created,
                executing: r.executing,
                stages,
            }
        })
        .collect();
    let summary = summarize(&rows, now, days, &|at| {
        sys::local_minute(at).chars().take(10).collect()
    });
    if c.parsed.flag("json") {
        c.say(&pyjson::dumps(&json(&summary)));
    } else {
        for line in text(&summary) {
            c.say(&line);
        }
    }
    Ok(())
}

fn spread_text(s: &Spread) -> String {
    format!(
        "median {} · p75 {} · p90 {}",
        hours(s.median),
        hours(s.p75),
        hours(s.p90)
    )
}

fn waiting_text(label: &str, w: &Waiting) -> String {
    match w.median_seconds {
        Some(median) => format!("{label}: {}, median {} there", w.count, hours(median)),
        None => format!("{label}: 0"),
    }
}

/// The summary as `stats` prints it.
fn text(s: &Summary) -> Vec<String> {
    let mut lines = vec![format!(
        "Stats: {} top-level tasks made in the last {} days",
        s.tasks, s.days
    )];
    let held: Vec<String> = (s.by_status.iter())
        .filter(|(_, count)| *count > 0)
        .map(|(status, count)| format!("{status} {count}"))
        .collect();
    if !held.is_empty() {
        lines.push(format!("Status: {}", held.join(" · ")));
    }
    if let Some(lead) = &s.lead {
        lines.push(format!("Done {}: lead time {}", lead.n, spread_text(lead)));
        let stages = (s.unclaimed.iter().map(|u| ("unclaimed", u))).chain(
            s.per_stage
                .iter()
                .map(|(status, held)| (status.as_str(), held)),
        );
        for (name, held) in stages {
            lines.push(format!(
                "  {name:<12} {}  ({} tasks)",
                spread_text(held),
                held.n
            ));
        }
    } else {
        lines.push("Done 0".into());
    }
    let bounced: Vec<String> = (s.bounces.iter())
        .map(|(bounces, tasks)| format!("{bounces} × {tasks}"))
        .collect();
    if !bounced.is_empty() {
        lines.push(format!(
            "Bounces (review → in_progress × tasks): {}",
            bounced.join(" · ")
        ));
    }
    lines.push("Done per day:".into());
    for (day, count) in &s.done_per_day {
        lines.push(format!("  {day}  {count}"));
    }
    if !s.slowest.is_empty() {
        lines.push("Longest in their status:".into());
        for sitting in &s.slowest {
            lines.push(format!(
                "  {:<28} {:<11} {}",
                sitting.slug,
                sitting.status,
                hours(sitting.seconds)
            ));
        }
    }
    lines.push(waiting_text("Blocked", &s.blocked));
    lines.push(waiting_text("Ready with no executor", &s.unstaffed));
    lines
}

fn count(n: usize) -> Py {
    Py::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

fn spread_json(s: &Spread) -> Py {
    Py::obj(vec![
        ("tasks", count(s.n)),
        ("median", Py::Float(s.median)),
        ("p75", Py::Float(s.p75)),
        ("p90", Py::Float(s.p90)),
    ])
}

fn waiting_json(w: &Waiting) -> Py {
    Py::obj(vec![
        ("tasks", count(w.count)),
        ("median_seconds", Py::opt_float(w.median_seconds)),
    ])
}

/// The summary as `stats --json` prints it; every duration in seconds.
fn json(s: &Summary) -> Py {
    let opt = |spread: &Option<Spread>| spread.as_ref().map_or(Py::Null, spread_json);
    Py::obj(vec![
        ("days", Py::Int(s.days)),
        ("tasks", count(s.tasks)),
        (
            "by_status",
            shape::keyed(s.by_status.iter().map(|(k, n)| (k.clone(), count(*n)))),
        ),
        ("lead_seconds", opt(&s.lead)),
        ("unclaimed_seconds", opt(&s.unclaimed)),
        (
            "stage_seconds",
            shape::keyed(
                (s.per_stage.iter()).map(|(status, held)| (status.clone(), spread_json(held))),
            ),
        ),
        (
            "bounces",
            Py::List(
                (s.bounces.iter())
                    .map(|(bounces, tasks)| {
                        Py::obj(vec![
                            ("bounces", Py::Int(i64::from(*bounces))),
                            ("tasks", count(*tasks)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "done_per_day",
            Py::List(
                (s.done_per_day.iter())
                    .map(|(day, done)| Py::obj(vec![("day", Py::str(day)), ("done", count(*done))]))
                    .collect(),
            ),
        ),
        (
            "slowest",
            Py::List(
                (s.slowest.iter())
                    .map(|sitting| {
                        Py::obj(vec![
                            ("slug", Py::str(&sitting.slug)),
                            ("status", Py::str(&sitting.status)),
                            ("seconds", Py::Float(sitting.seconds)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("blocked", waiting_json(&s.blocked)),
        ("ready_unstaffed", waiting_json(&s.unstaffed)),
    ])
}
