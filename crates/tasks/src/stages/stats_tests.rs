//! The pipeline over a window, each expected figure written out by hand.

use super::stats::{Row, Sitting, Spread, Waiting, spread, summarize};
use super::tests::{H, T0, at, stage, straight_through_stages};
use super::{CLOSED, Stages};

const D: f64 = 86_400.0;

fn near(found: f64, wanted: f64) {
    assert!((found - wanted).abs() < 1e-6, "{found} is not {wanted}");
}

fn spread_is(found: Option<Spread>, n: usize, median: f64, p75: f64, p90: f64) {
    let found = found.expect("a spread");
    assert_eq!(found.n, n);
    near(found.median, median * H);
    near(found.p75, p75 * H);
    near(found.p90, p90 * H);
}

/// A task made `made` hours in, holding `status` since `since` hours in, its stages given as
/// (status, hours held).
fn row(slug: &str, status: &str, made: f64, since: f64, held: &[(&str, f64)]) -> Row {
    Row {
        slug: slug.into(),
        status: status.into(),
        created: at(made),
        executing: false,
        stages: Stages {
            stages: held.iter().map(|(s, h)| stage(s, made, *h, 1)).collect(),
            unclaimed_seconds: held.first().map_or(0.0, |(_, h)| h * H),
            first_claimed: None,
            bounces: 0,
            current: status.into(),
            current_since: at(since),
            closed: CLOSED.contains(&status).then_some(at(since)),
        },
    }
}

/// When each done task closed: the window's three, one made long before and closed 70 hours in,
/// and one closed an hour before the window.
const CLOSES: [f64; 5] = [
    T0 + 4.0 * H,
    T0 + 32.0 * H,
    T0 + 36.0 * H,
    T0 + 70.0 * H,
    T0 - H,
];

fn day(time: f64) -> String {
    format!("d{}", ((time - T0) / D).floor())
}

fn window() -> Vec<Row> {
    let done = |slug: &str, made: f64, took: f64, held: &[(&str, f64)]| {
        row(slug, "done", made, made + took, held)
    };
    let mut b = done(
        "sim/b",
        24.0,
        8.0,
        &[("ready", 2.0), ("in_progress", 4.0), ("review", 2.0)],
    );
    b.stages.bounces = 2;
    let mut f = row("sim/f", "ready", 20.0, 24.0, &[("ready", 48.0)]);
    f.executing = true;
    let mut g = row("sim/g", "in_progress", 60.0, 66.0, &[("in_progress", 6.0)]);
    (g.stages.bounces, g.executing) = (1, true);
    vec![
        done(
            "sim/a",
            0.0,
            4.0,
            &[("ready", 1.0), ("in_progress", 2.0), ("review", 1.0)],
        ),
        b,
        done("sim/c", 24.0, 12.0, &[("ready", 3.0), ("in_progress", 9.0)]),
        row("sim/d", "blocked", 40.0, 48.0, &[("blocked", 24.0)]),
        row("sim/e", "ready", 60.0, 60.0, &[("ready", 12.0)]),
        f,
        g,
        row("sim/h", "cancelled", 10.0, 11.0, &[("ready", 1.0)]),
        row("sim/i", "ready", 48.0, 48.0, &[("ready", 24.0)]),
    ]
}

#[test]
fn the_window_is_counted_spread_and_ranked() {
    let found = summarize(&window(), &CLOSES, at(72.0), 3, &day);
    assert_eq!((found.days, found.tasks), (3, 9));
    let counts: Vec<(&str, usize)> = (found.by_status.iter())
        .map(|(status, count)| (status.as_str(), *count))
        .collect();
    assert_eq!(
        counts,
        [
            ("backlog", 0),
            ("concept", 0),
            ("design", 0),
            ("ready", 3),
            ("in_progress", 1),
            ("review", 0),
            ("done", 3),
            ("blocked", 1),
            ("cancelled", 1),
        ]
    );
    // Lead times of 4, 8 and 12 hours; waits of 1, 2 and 3.
    spread_is(found.lead, 3, 8.0, 10.0, 11.2);
    spread_is(found.unclaimed, 3, 2.0, 2.5, 2.8);
    let staged: Vec<&str> = found.per_stage.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(staged, ["ready", "in_progress", "review"]);
    spread_is(Some(found.per_stage[0].1), 3, 2.0, 2.5, 2.8);
    // 2, 4 and 9 hours in progress; only two of the three were ever in review, 1 and 2 hours.
    spread_is(Some(found.per_stage[1].1), 3, 4.0, 6.5, 8.0);
    spread_is(Some(found.per_stage[2].1), 2, 1.5, 1.75, 1.9);
    assert_eq!(found.bounces, [(0, 7), (1, 1), (2, 1)]);
    let per_day: Vec<(&str, usize)> = (found.done_per_day.iter())
        .map(|(day, done)| (day.as_str(), *done))
        .collect();
    // The window's own three, one made before it and closed on its third day, and none for the
    // close from before the window.
    assert_eq!(per_day, [("d0", 1), ("d1", 2), ("d2", 1), ("d3", 0)]);
    let sitting = |slug: &str, status: &str, hours: f64| Sitting {
        slug: slug.into(),
        status: status.into(),
        seconds: hours * H,
    };
    assert_eq!(
        found.slowest,
        [
            sitting("sim/f", "ready", 48.0),
            sitting("sim/d", "blocked", 24.0),
            sitting("sim/i", "ready", 24.0),
            sitting("sim/e", "ready", 12.0),
            sitting("sim/g", "in_progress", 6.0),
        ]
    );
    assert_eq!(
        found.blocked,
        Waiting {
            count: 1,
            median_seconds: Some(86_400.0)
        }
    );
    // sim/e and sim/i: sim/f is ready too, but held.
    assert_eq!(
        found.unstaffed,
        Waiting {
            count: 2,
            median_seconds: Some(64_800.0)
        }
    );
}

#[test]
fn an_empty_window_has_its_days_and_nothing_else() {
    let found = summarize(&[], &[], at(30.0), 1, &day);
    assert_eq!(found.tasks, 0);
    assert_eq!((found.lead, found.unclaimed), (None, None));
    assert!(found.per_stage.is_empty() && found.bounces.is_empty() && found.slowest.is_empty());
    assert_eq!(
        found.done_per_day,
        [("d0".to_string(), 0), ("d1".to_string(), 0)]
    );
    let none = Waiting {
        count: 0,
        median_seconds: None,
    };
    assert_eq!((found.blocked, found.unstaffed), (none, none));
    assert!(found.by_status.iter().all(|(_, count)| *count == 0));
}

#[test]
fn only_the_ten_longest_waits_are_named() {
    let rows: Vec<Row> = (0..12u8)
        .map(|n| {
            let since = f64::from(n);
            row(&format!("sim/t{n:02}"), "review", 0.0, since, &[])
        })
        .collect();
    let found = summarize(&rows, &[], at(20.0), 1, &day);
    let slugs: Vec<&str> = found.slowest.iter().map(|s| s.slug.as_str()).collect();
    assert_eq!(
        slugs,
        [
            "sim/t00", "sim/t01", "sim/t02", "sim/t03", "sim/t04", "sim/t05", "sim/t06", "sim/t07",
            "sim/t08", "sim/t09"
        ]
    );
    near(found.slowest[9].seconds, 11.0 * H);
}

#[test]
fn a_spread_falls_between_its_neighbours() {
    assert_eq!(spread(Vec::new()), None);
    let one = spread(vec![5.0]).unwrap();
    assert_eq!((one.n, one.median, one.p75, one.p90), (1, 5.0, 5.0, 5.0));
    let four = spread(vec![4.0, 1.0, 3.0, 2.0]).unwrap();
    assert_eq!(four.n, 4);
    near(four.median, 2.5);
    near(four.p75, 3.25);
    near(four.p90, 3.7);
}

#[test]
fn one_done_tasks_status_rows_add_up_to_its_lead_time() {
    let row = Row {
        slug: "sim/a".into(),
        status: "done".into(),
        created: T0,
        executing: false,
        stages: straight_through_stages(),
    };
    let found = summarize(&[row], &[at(3.0)], at(24.0), 1, &day);
    let rows: Vec<(&str, f64)> = (found.per_stage.iter())
        .map(|(status, held)| (status.as_str(), held.median))
        .collect();
    assert_eq!(
        rows,
        [
            ("ready", 1800.0),
            ("in_progress", 5400.0),
            ("review", 3600.0)
        ]
    );
    assert_eq!(found.lead.map(|lead| lead.median), Some(10_800.0));
    // 1800 + 5400 + 3600 = 10800: the wait before the claim is the ready row again, not a fourth.
    assert_eq!(found.unclaimed.map(|wait| wait.median), Some(1800.0));
}

#[test]
fn a_day_an_hour_short_or_long_is_named_once_and_none_is_skipped() {
    // Days that start at 0, 24, 47 and 71 hours: the third is 23 hours long. Two days back from
    // 71.5 hours is 23.5, and whole days on from there (47.5, 71.5) never fall on the second.
    let short = |time: f64| {
        let hours = (time - T0) / H;
        let starts = [71.0, 47.0, 24.0, 0.0];
        let day = starts.iter().position(|start| hours >= *start).unwrap();
        format!("d{}", 3 - day)
    };
    let found = summarize(&[], &[at(30.0), at(46.5), at(47.0)], at(71.5), 2, &short);
    let per_day: Vec<(&str, usize)> = (found.done_per_day.iter())
        .map(|(day, done)| (day.as_str(), *done))
        .collect();
    assert_eq!(per_day, [("d0", 0), ("d1", 2), ("d2", 1), ("d3", 0)]);
}
