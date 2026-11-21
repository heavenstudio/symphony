//! `stats`: where the pipeline's time goes ([`crate::stages::stats`]) over the top-level tasks
//! made in the last `--days` days, and how many were done on each of those days whenever they
//! were made, as text or as JSON. One read of the board ([`Store::stage_records`]), whatever its
//! size. Under them, the tokens the board recorded in those days and their estimated cost
//! ([`tokens`]), from the hourly roll-up's day files and no read of the board.

mod tokens;

use super::{Context, Stop};
use crate::cli::shape;
use crate::json_object::object;
use crate::stages::stats::{Row, Spread, Summary, Waiting, summarize};
use crate::stages::{Facts, hours, stages};
use crate::{Store, sys};
use serde_json::Value;

/// How far back `stats` looks unless `--days` says.
const DAYS: i64 = 14;
/// The longest window: ten years.
const MOST_DAYS: i64 = 3650;
const DAY: f64 = 86_400.0;

pub(super) fn run(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let days = c.parsed.int("days", DAYS);
    if !(1..=MOST_DAYS).contains(&days) {
        return Err(Stop::Refused(format!(
            "--days is a number of days from 1 to {MOST_DAYS}, not {days}"
        )));
    }
    let now = store.now();
    #[allow(clippy::cast_precision_loss)]
    let since = now - days as f64 * DAY;
    // The read holds the tasks made in the window and the done ones written in it: the first are
    // the rows, and every done one closed in it counts on its day.
    let (mut rows, mut closes) = (Vec::new(), Vec::new());
    for r in store.stage_records(since)? {
        let facts = Facts {
            status: &r.status,
            created: r.created,
            updated: r.updated,
        };
        let stages = stages(facts, &r.lines, now);
        closes.extend(stages.done_at());
        if r.created >= since {
            rows.push(Row {
                slug: r.slug,
                status: r.status,
                created: r.created,
                executing: r.executing,
                arm: r.arm,
                stages,
            });
        }
    }
    let summary = summarize(&rows, &closes, now, days, &|at| {
        sys::local_minute(at).chars().take(10).collect()
    });
    let told = tokens::told(c.env, now, days, crate::usage::Prices::embedded());
    if c.parsed.flag("json") {
        let mut json = json(&summary);
        if let Some(fields) = json.as_object_mut() {
            fields.insert("tokens".into(), tokens::json(&told));
        }
        c.say(&serde_json::to_string_pretty(&json).expect("JSON values serialize"));
    } else {
        for line in text(&summary).into_iter().chain(tokens::text(&told)) {
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
        "Stats: {} top-level tasks made in the last {} days; every figure is theirs but done per day",
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
        for (name, held) in &s.per_stage {
            lines.push(format!(
                "  {name:<12} {}  ({} tasks)",
                spread_text(held),
                held.n
            ));
        }
        if let Some(waited) = &s.unclaimed {
            lines.push(format!(
                "Waiting before the first claim (a part of the statuses above, not added to them): {}",
                spread_text(waited)
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
    lines.push(format!(
        "Done per day, local time (each top-level task done now, once, on the day of its last close \
         in the last {} days, whenever it was made):",
        s.days
    ));
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
    if !s.arms.is_empty() {
        lines.push(
            "Arms (the tasks filed with --arm; lead and review are the medians of the done):"
                .into(),
        );
        let median =
            |spread: &Option<Spread>| spread.as_ref().map_or("-".into(), |s| hours(s.median));
        for arm in &s.arms {
            lines.push(format!(
                "  {:<12} tasks {} · done {} · lead {} · review {} · bounces {}",
                arm.name,
                arm.tasks,
                arm.done,
                median(&arm.lead),
                median(&arm.review),
                arm.bounces
            ));
        }
    }
    lines
}

fn count(n: usize) -> Value {
    Value::from(i64::try_from(n).unwrap_or(i64::MAX))
}

fn spread_json(s: &Spread) -> Value {
    object(vec![
        ("tasks", count(s.n)),
        ("median", Value::from(s.median)),
        ("p75", Value::from(s.p75)),
        ("p90", Value::from(s.p90)),
    ])
}

fn waiting_json(w: &Waiting) -> Value {
    object(vec![
        ("tasks", count(w.count)),
        ("median_seconds", Value::from(w.median_seconds)),
    ])
}

/// The summary as `stats --json` prints it; every duration in seconds.
fn json(s: &Summary) -> Value {
    let opt = |spread: &Option<Spread>| spread.as_ref().map_or(Value::Null, spread_json);
    object(vec![
        ("days", Value::from(s.days)),
        ("tasks", count(s.tasks)),
        (
            "by_status",
            shape::keyed(s.by_status.iter().map(|(k, n)| (k.clone(), count(*n)))),
        ),
        ("lead_seconds", opt(&s.lead)),
        (
            "stage_seconds",
            shape::keyed(
                (s.per_stage.iter()).map(|(status, held)| (status.clone(), spread_json(held))),
            ),
        ),
        ("waiting_before_first_claim_seconds", opt(&s.unclaimed)),
        (
            "bounces",
            Value::Array(
                (s.bounces.iter())
                    .map(|(bounces, tasks)| {
                        object(vec![
                            ("bounces", Value::from(i64::from(*bounces))),
                            ("tasks", count(*tasks)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "done_per_day",
            Value::Array(
                (s.done_per_day.iter())
                    .map(|(day, done)| {
                        object(vec![
                            ("day", Value::from(day.as_str())),
                            ("done", count(*done)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "slowest",
            Value::Array(
                (s.slowest.iter())
                    .map(|sitting| {
                        object(vec![
                            ("slug", Value::from(sitting.slug.as_str())),
                            ("status", Value::from(sitting.status.as_str())),
                            ("seconds", Value::from(sitting.seconds)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("blocked", waiting_json(&s.blocked)),
        ("ready_unstaffed", waiting_json(&s.unstaffed)),
        (
            "arms",
            shape::keyed(s.arms.iter().map(|arm| {
                (
                    arm.name.clone(),
                    object(vec![
                        ("tasks", count(arm.tasks)),
                        ("done", count(arm.done)),
                        ("lead_seconds", opt(&arm.lead)),
                        ("review_seconds", opt(&arm.review)),
                        ("bounces", Value::from(i64::from(arm.bounces))),
                    ]),
                )
            })),
        ),
    ])
}
