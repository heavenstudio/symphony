//! The Reports page's read over fixture day files in a scratch folder: whole UTC periods back from
//! the clock, the last cut at the end of its hour; each source's part of a period missing where no
//! file holds an hour of it; the roll-up's hours, the agents' hour lines (two writers in one hour
//! taken together) and the health lines summed a period at a time; the asked grain and days
//! bounded; a file too large, a link, a FIFO or one that does not read named and left out; and the
//! route, refused before any hub for a bad ask, answering the hub's folders at its clock, `304` for
//! a body the client holds, from the answer the hub kept (`kept_tests.rs`).

use super::*;
use crate::testing::Tree;
use symphony_api::agent_activity::{AgentLine, SessionHour};
use symphony_api::reports::{Red, Round, Wait};
use symphony_api::{HourRequests, HourSlowest, HourWrites};

/// Monday 2026-11-09, 00:00 UTC.
pub(super) const MONDAY: f64 = 1_794_182_400.0;

/// Wednesday 10:30 of that week.
pub(super) fn wednesday() -> f64 {
    MONDAY + 2.0 * DAY + 10.5 * HOUR
}

pub(super) fn folders(tree: &Tree) -> Folders {
    Folders {
        reports: Ok(tree.path("reports")),
        agents: Ok(tree.path("agents")),
        health: Ok(tree.path("health")),
        runs: Some(tree.path("runs")),
    }
}

fn none(_: f64) -> Result<Vec<ReportMark>, String> {
    Ok(Vec::new())
}

/// A roll-up hour at `start` with `done` tasks done and `review` held in review at its end.
pub(super) fn hour(start: f64, done: u32, review: u32) -> symphony_api::ReportHour {
    let mut hour = symphony_api::ReportHour {
        start,
        landed: done,
        ..Default::default()
    };
    hour.top.entered.done = done;
    hour.top.done = Some(done);
    hour.top.held.review = review;
    hour.top.task_hours.in_progress = 1.5;
    hour
}

/// The day file of the day `start` begins, holding `hours`, written as the roll-up writes it.
pub(super) fn report(
    tree: &Tree,
    start: f64,
    hours: Vec<symphony_api::ReportHour>,
    edit: impl FnOnce(&mut ReportDay),
) {
    let mut day = ReportDay {
        schema: symphony_api::REPORT_SCHEMA,
        project: "p".into(),
        day: run_day(start),
        start,
        hours,
        ..ReportDay::default()
    };
    edit(&mut day);
    std::fs::create_dir_all(tree.path("reports")).unwrap();
    symphony_board::reports::write(&tree.path("reports"), &day).unwrap();
}

/// The fixture week: Monday's roll-up holds 09:00 and 10:00, Tuesday has no file, Wednesday's holds
/// 09:00 alone; the agents' Monday file holds 10:00 from two writers, with lines of other types and
/// a line cut short; the health's Wednesday file holds 09:00 and the half of 10:00 so far; and a
/// roll-up file of October, outside every window.
pub(super) fn week(tree: &Tree) {
    let mut nine = hour(MONDAY + 9.0 * HOUR, 1, 3);
    nine.decisions = Some(DecisionCounts {
        open: 4,
        answered: 1,
        ..DecisionCounts::default()
    });
    nine.waits.review_to_reviewer = vec![wait(600.0), wait(1800.0)];
    let mut ten = hour(MONDAY + 10.0 * HOUR, 2, 1);
    ten.top.bounces = 1;
    ten.reds_filed = 1;
    ten.decisions = Some(DecisionCounts {
        open: 2,
        answered: 1,
        auto_approved: 2,
        withdrawn: 0,
    });
    ten.git = Some(GitCounts {
        commits: 3,
        merges: 1,
        added: 100,
        removed: 20,
        reverts: 1,
    });
    ten.waits.review_to_reviewer = vec![wait(1200.0)];
    let red = Red {
        slug: "red/a".into(),
        filed: MONDAY + HOUR,
        closed: Some(MONDAY + 3.0 * HOUR),
        status: "done".into(),
        ..Red::default()
    };
    let monday_red = red.clone();
    report(tree, MONDAY, vec![nine, ten], move |day| {
        day.top.lead = Some(Spread {
            n: 2,
            median: 3600.0,
            p90: 7200.0,
        });
        day.rounds = vec![Round {
            number: 1,
            changes: 1,
            approved: 2,
        }];
        day.reds = vec![monday_red];
    });
    report(
        tree,
        MONDAY + 2.0 * DAY,
        vec![hour(MONDAY + 2.0 * DAY + 9.0 * HOUR, 4, 2)],
        move |day| {
            day.top.lead = Some(Spread {
                n: 1,
                median: 1800.0,
                p90: 1800.0,
            });
            // The red closed on Monday is listed again: its fix counts once, on Monday.
            day.reds = vec![red];
        },
    );
    report(tree, MONDAY - 39.0 * DAY, vec![], |_| {});
    std::fs::write(tree.path("reports/notes.json"), "{}").unwrap();

    let ten = MONDAY + 10.0 * HOUR;
    let first = AgentHour {
        start: ten,
        from: ten,
        to: ten + 1200.0,
        observed_s: 1200.0,
        samples: 4,
        running_agents: Level { peak: 2, mean: 1.5 },
        running_roles: vec![role("executor", 2, 1.5)],
        holding_agents: Level { peak: 2, mean: 2.0 },
        live: vec![KindLevel {
            kind: "claude".into(),
            peak: 3,
            mean: 2.5,
        }],
        sessions: vec![SessionHour {
            kind: "claude".into(),
            model: "opus".into(),
            started: 1,
            went_stale: 1,
            stale_s: 60.0,
            ..SessionHour::default()
        }],
        load_1m: Some(Gauge {
            peak: 4.0,
            mean: 2.0,
        }),
        ..AgentHour::default()
    };
    let second = AgentHour {
        from: ten + 1200.0,
        to: ten + HOUR,
        observed_s: 2400.0,
        samples: 8,
        running_agents: Level { peak: 3, mean: 3.0 },
        running_roles: vec![role("executor", 1, 1.0), role("reviewer", 2, 2.0)],
        sessions: vec![SessionHour {
            kind: "claude".into(),
            model: "opus".into(),
            started: 2,
            went_lost: 1,
            lost_s: 30.0,
            ..SessionHour::default()
        }],
        load_1m: None,
        ..first.clone()
    };
    crate::live::agent_activity::append(
        &tree.path("agents"),
        &[AgentLine::Hour(first), AgentLine::Hour(second)],
    )
    .unwrap();
    let file = tree.path(&format!("agents/{}.jsonl", run_day(MONDAY)));
    let mut text = std::fs::read_to_string(&file).unwrap();
    text.push_str("{\"type\":\"start\",\"schema\":1,\"at\":1,\"pid\":1,\"generation\":1,\"sessions\":0,\"left\":0}\n");
    text.push_str("{\"type\":\"hour\",\"schema\":1,\"start\":");
    std::fs::write(&file, text).unwrap();

    let wednesday_nine = MONDAY + 2.0 * DAY + 9.0 * HOUR;
    let health = |start: f64, to: f64, requests: u64, slowest: f64, p90: f64| HealthHour {
        start,
        from: start,
        to,
        requests: HourRequests {
            count: requests,
            slow: requests / 30,
            unanswered: u64::from(requests > 50),
            slowest: Some(HourSlowest {
                ms: slowest,
                ..HourSlowest::default()
            }),
        },
        writes: HourWrites {
            count: 10,
            p90_ms: Some(p90),
            ..HourWrites::default()
        },
        ..HealthHour::default()
    };
    let mut nine = health(wednesday_nine, wednesday_nine + HOUR, 100, 2500.0, 40.0);
    nine.dispatch.ok = 50;
    nine.dispatch.failed = 1;
    symphony_board::board_health::hours::append(
        &tree.path("health"),
        &[
            nine,
            health(
                wednesday_nine + HOUR,
                wednesday_nine + 1.5 * HOUR,
                20,
                900.0,
                60.0,
            ),
        ],
    )
    .unwrap();
}

fn wait(seconds: f64) -> Wait {
    Wait {
        slug: "t".into(),
        seconds,
    }
}

fn role(role: &str, peak: u64, mean: f64) -> RoleLevel {
    RoleLevel {
        role: role.into(),
        peak,
        mean,
    }
}

#[test]
fn a_window_is_whole_periods_back_from_now_its_last_cut_at_the_end_of_the_current_hour() {
    let now = wednesday();
    let end = MONDAY + 2.0 * DAY + 11.0 * HOUR;
    let hours = periods(Grain::Hour, 2, now);
    assert_eq!(hours.len(), 48);
    assert_eq!(hours[0], (end - 48.0 * HOUR, end - 47.0 * HOUR));
    assert_eq!(hours.last(), Some(&(end - HOUR, end)));
    let days = periods(Grain::Day, 14, now);
    assert_eq!(days.len(), 14);
    assert_eq!(days[0], (MONDAY - 11.0 * DAY, MONDAY - 10.0 * DAY));
    assert_eq!(days.last(), Some(&(MONDAY + 2.0 * DAY, end)));
    let weeks = periods(Grain::Week, 84, now);
    assert_eq!(weeks.len(), 12);
    assert_eq!(weeks[0], (MONDAY - 11.0 * WEEK, MONDAY - 10.0 * WEEK));
    assert_eq!(weeks.last(), Some(&(MONDAY, end)));
    for periods in [hours, days, weeks] {
        assert!(periods.windows(2).all(|two| two[0].1 == two[1].0));
    }
    // By the week, as many whole weeks as fit, one at least: 90 days is twelve.
    assert_eq!(periods(Grain::Week, 3, now), [(MONDAY, end)]);
    assert_eq!(periods(Grain::Week, 90, now).len(), 12);
    // A clock on the hour is in the hour it begins.
    assert_eq!(
        periods(Grain::Hour, 1, MONDAY).last(),
        Some(&(MONDAY, MONDAY + HOUR))
    );
    // Sunday's last minutes are the week before's; Monday 00:00 begins one.
    assert_eq!(week_of(MONDAY - 60.0), MONDAY - WEEK);
    assert_eq!(week_of(MONDAY), MONDAY);
    assert_eq!(week_of(MONDAY + 6.9 * DAY), MONDAY);
}

#[test]
fn the_ask_takes_a_grain_by_its_word_and_days_bounded_by_it() {
    for (grain, days, taken) in [
        (None, None, (Grain::Day, 14)),
        (Some(""), Some(""), (Grain::Day, 14)),
        (Some("hour"), None, (Grain::Hour, 2)),
        (Some("hour"), Some("30"), (Grain::Hour, 7)),
        (Some("week"), None, (Grain::Week, 84)),
        (Some("week"), Some("0"), (Grain::Week, 1)),
        (Some("day"), Some("400"), (Grain::Day, 90)),
        (Some("day"), Some("90"), (Grain::Day, 90)),
    ] {
        assert_eq!(asked(grain, days), Ok(taken), "{grain:?} {days:?}");
    }
    assert_eq!(
        asked(Some("month"), None),
        Err("grain is hour, day or week, not month".into())
    );
    assert_eq!(
        asked(Some("Day"), None),
        Err("grain is hour, day or week, not Day".into())
    );
    for days in ["-3", "2.5", "two", "99999999999"] {
        assert_eq!(
            asked(None, Some(days)),
            Err(format!("days is a whole number of days, not {days}"))
        );
    }
}

#[test]
fn a_day_file_s_name_reads_back_as_its_first_second_and_any_other_name_as_none() {
    assert_eq!(day_start("1970-01-01"), Some(0.0));
    assert_eq!(day_start("2026-11-09"), Some(MONDAY));
    assert_eq!(day_start("2024-02-29"), Some(1_709_164_800.0));
    for name in [
        "2026-02-29",
        "2026-13-01",
        "2026-00-10",
        "2026-11-31",
        "2026-11-9x",
        "2026-1-091",
        "+026-11-09",
        "2026_11_09",
        "2026-11-09x",
        "",
    ] {
        assert_eq!(day_start(name), None, "{name}");
    }
}

#[test]
fn days_sum_their_hours_and_a_day_no_file_holds_is_missing_never_zero() {
    let tree = Tree::new();
    week(&tree);
    let mut asked_from = None;
    let page = page(&folders(&tree), Grain::Day, 3, wednesday(), |since| {
        asked_from = Some(since);
        Ok(vec![
            ReportMark {
                at: MONDAY - 1.0,
                commit: "before".into(),
                subject: "before the window".into(),
                changed: vec!["settings".into()],
            },
            ReportMark {
                at: MONDAY + 8.0 * HOUR,
                commit: "abc".into(),
                subject: "gates".into(),
                changed: vec!["settings".into(), "skill work".into()],
            },
        ])
    });
    assert_eq!(
        asked_from,
        Some(MONDAY),
        "the marks are read from the window's start"
    );
    assert_eq!(
        (page.grain, page.days, page.from, page.to),
        (Grain::Day, 3, MONDAY, MONDAY + 2.0 * DAY + 11.0 * HOUR)
    );
    assert_eq!(page.unread, Vec::<String>::new());
    assert_eq!(
        page.first,
        Some(MONDAY - 39.0 * DAY),
        "the oldest file, whatever the window"
    );
    assert_eq!(
        page.marks
            .iter()
            .map(|m| (m.commit.as_str(), m.changed.join(", ")))
            .collect::<Vec<_>>(),
        [("abc", "settings, skill work".to_string())]
    );
    let [monday, tuesday, wednesday] = page.buckets.as_slice() else {
        panic!("{:?}", page.buckets);
    };

    assert_eq!(
        (monday.start, monday.end, monday.rolled),
        (MONDAY, MONDAY + DAY, 2)
    );
    let flow = monday.flow.unwrap();
    assert_eq!(
        (
            flow.entered.done,
            flow.done,
            flow.held.review,
            flow.task_hours.in_progress
        ),
        (3, Some(3), 1, 3.0),
        "the tasks held are the last hour's, the rest added up"
    );
    assert_eq!((flow.bounces, flow.landed, flow.reds_filed), (1, 3, 1));
    assert_eq!(
        monday.decisions,
        Some(DecisionCounts {
            open: 2,
            answered: 2,
            auto_approved: 2,
            withdrawn: 0
        })
    );
    assert_eq!(
        monday.git.map(|g| (g.commits, g.added, g.removed)),
        Some((3, 100, 20))
    );
    assert_eq!(
        monday.waits.unwrap().review_to_reviewer,
        Some(Spread {
            n: 3,
            median: 1200.0,
            p90: 1680.0
        })
    );
    assert_eq!(monday.waits.unwrap().approved_to_landed, None);
    assert_eq!(monday.lead.map(|l| (l.n, l.median)), Some((2, 3600.0)));
    assert_eq!(
        monday.verdicts,
        Some(BucketVerdicts {
            changes: 1,
            approved: 2
        })
    );
    assert_eq!(monday.red_fix.map(|s| (s.n, s.median)), Some((1, 7200.0)));
    assert!(monday.agents.is_some());
    assert_eq!(monday.health, None);

    // Tuesday has no file of any source: every part missing.
    assert_eq!(
        tuesday,
        &ReportBucket {
            start: MONDAY + DAY,
            end: MONDAY + 2.0 * DAY,
            ..ReportBucket::default()
        }
    );

    // Wednesday, under way: its one roll-up hour, the red listed again not counted again, and the
    // health of the hour and a half its lines cover.
    assert_eq!(wednesday.rolled, 1);
    assert_eq!(
        wednesday.flow.map(|f| (f.entered.done, f.done)),
        Some((4, Some(4)))
    );
    assert_eq!(wednesday.decisions, None, "no hour of it read the cards");
    assert_eq!(wednesday.git, None, "no hour of it read git");
    assert_eq!(wednesday.waits, None);
    assert_eq!(wednesday.red_fix, None);
    assert_eq!(wednesday.agents, None);
    let health = wednesday.health.as_ref().unwrap();
    assert_eq!(
        (
            health.covered_s,
            health.requests,
            health.slow,
            health.unanswered,
            health.writes
        ),
        (5400.0, 120, 3, 1, 20)
    );
    assert_eq!((health.dispatch.ok, health.dispatch.failed), (50, 1));
    assert_eq!(
        (health.slowest_ms, health.write_p90_ms),
        (Some(2500.0), Some(60.0)),
        "the slowest and the largest p90 of its hours"
    );
}

#[test]
fn two_writers_of_one_hour_are_taken_together_as_the_agents_doc_reads_an_hour() {
    let tree = Tree::new();
    week(&tree);
    let page = page(&folders(&tree), Grain::Hour, 3, wednesday(), none);
    assert_eq!(page.buckets.len(), 72);
    let at = |start: f64| {
        page.buckets
            .iter()
            .find(|bucket| bucket.start == start)
            .unwrap()
    };
    let agents = at(MONDAY + 10.0 * HOUR).agents.clone().unwrap();
    assert_eq!((agents.observed_s, agents.samples), (3600.0, 12));
    assert_eq!(
        agents.running_agents,
        Level { peak: 3, mean: 2.5 },
        "the largest peak; the mean weighted by the seconds each watched"
    );
    assert_eq!(
        agents.running,
        [role("executor", 2, 1.167), role("reviewer", 2, 1.333)]
    );
    assert_eq!(agents.holding_agents, Level { peak: 2, mean: 2.0 });
    assert_eq!(agents.started, 2, "the line that ran latest's");
    assert_eq!(
        (
            agents.went_stale,
            agents.went_lost,
            agents.stale_s,
            agents.lost_s
        ),
        (1, 1, 60.0, 30.0)
    );
    assert_eq!(
        agents.load_1m,
        Some(Gauge {
            peak: 4.0,
            mean: 2.0
        }),
        "over the line that read it alone"
    );
    assert_eq!(agents.load_15m, None);
    // The hours around it have no line: missing, as the roll-up's hour before its first.
    assert_eq!(at(MONDAY + 9.0 * HOUR).agents, None);
    assert!(at(MONDAY + 9.0 * HOUR).flow.is_some());
    assert_eq!(at(MONDAY + 8.0 * HOUR).flow, None);
    // By the hour there is no lead time nor any verdict: the roll-up keeps those by the day.
    assert!(
        page.buckets
            .iter()
            .all(|b| b.lead.is_none() && b.verdicts.is_none())
    );
}

#[test]
fn a_week_sums_its_days_and_weighs_their_lead_times_by_their_closes() {
    let tree = Tree::new();
    week(&tree);
    let page = page(&folders(&tree), Grain::Week, 7, wednesday(), none);
    let [week] = page.buckets.as_slice() else {
        panic!("{:?}", page.buckets);
    };
    assert_eq!((week.start, week.rolled), (MONDAY, 3));
    assert_eq!(
        week.flow.map(|f| (f.entered.done, f.done, f.held.review)),
        Some((7, Some(7), 2))
    );
    assert_eq!(
        week.lead,
        Some(Spread {
            n: 3,
            median: 3000.0,
            p90: 7200.0
        })
    );
    assert_eq!(week.red_fix.map(|s| s.n), Some(1));
    assert!(week.agents.is_some() && week.health.is_some());
}

/// A roll-up hour at `start` with `done` entries into done, of a file written before the hours
/// counted their tasks done.
fn uncounted(start: f64, done: u32) -> symphony_api::ReportHour {
    let mut hour = hour(start, done, 0);
    hour.top.done = None;
    hour
}

/// The period of each bucket's flow, as its entries into done and its tasks done.
fn done_by_period(page: &ReportsPage) -> Vec<Option<(u32, Option<u32>)>> {
    (page.buckets.iter())
        .map(|bucket| bucket.flow.map(|flow| (flow.entered.done, flow.done)))
        .collect()
}

/// The tasks done ([`BucketFlow::done`]) of a period are the sum of its roll-up hours only when
/// every one of them counts them: one hour of a file that does not record them leaves the period
/// none, missing, never the other hours' part of it shown whole, while its entries into done are
/// still summed.
#[test]
fn the_tasks_done_sum_the_hours_that_count_them_and_a_period_none_counts_has_none() {
    let tree = Tree::new();
    report(
        &tree,
        MONDAY,
        vec![
            uncounted(MONDAY + 9.0 * HOUR, 5),
            hour(MONDAY + 10.0 * HOUR, 2, 0),
        ],
        |_| {},
    );
    report(
        &tree,
        MONDAY + DAY,
        vec![uncounted(MONDAY + DAY + 9.0 * HOUR, 4)],
        |_| {},
    );
    report(
        &tree,
        MONDAY + 2.0 * DAY,
        vec![hour(MONDAY + 2.0 * DAY + 9.0 * HOUR, 3, 0)],
        |_| {},
    );
    let page = page(&folders(&tree), Grain::Day, 3, wednesday(), none);
    assert_eq!(
        done_by_period(&page),
        [Some((7, None)), Some((4, None)), Some((3, Some(3)))],
        "Monday counts its 10:00 alone, so none; Tuesday none of its hours; Wednesday all of them"
    );
    let text = serde_json::to_string(&page.buckets[0].flow).unwrap();
    assert!(text.contains(r#""done":null"#), "{text}");
}

/// The week the first file that counts its tasks done arrives in also holds files written before
/// it, which are never written again: that week has none, a gap, never the count of its later days
/// alone; the week after, every file of which counts them, has their sum.
#[test]
fn a_week_that_spans_the_first_file_counting_the_tasks_done_has_none() {
    let tree = Tree::new();
    report(
        &tree,
        MONDAY,
        vec![uncounted(MONDAY + 9.0 * HOUR, 30)],
        |_| {},
    );
    report(
        &tree,
        MONDAY + DAY,
        vec![hour(MONDAY + DAY + 9.0 * HOUR, 2, 0)],
        |_| {},
    );
    let next = MONDAY + 7.0 * DAY;
    report(&tree, next, vec![hour(next + 9.0 * HOUR, 3, 0)], |_| {});
    report(
        &tree,
        next + DAY,
        vec![hour(next + DAY + 9.0 * HOUR, 1, 0)],
        |_| {},
    );
    let page = page(
        &folders(&tree),
        Grain::Week,
        14,
        wednesday() + 7.0 * DAY,
        none,
    );
    assert_eq!(
        (page.buckets.iter().map(|b| (b.start, b.rolled))).collect::<Vec<_>>(),
        [(MONDAY, 2), (next, 2)]
    );
    assert_eq!(
        done_by_period(&page),
        [Some((32, None)), Some((4, Some(4)))],
        "the first week's 2 are a part of its count, so it has none"
    );
}

/// `hours` in one phase or state each, `(id, label, hours)`.
fn spent(entries: &[(&str, &str, f64)]) -> Vec<Spent> {
    (entries.iter())
        .map(|(id, label, hours)| Spent {
            id: (*id).into(),
            label: (*label).into(),
            hours: *hours,
        })
        .collect()
}

/// A roll-up hour at `start` whose top-level tasks spent `phases` and `states`, and whose subtasks
/// spent an hour in a phase of their own.
fn placed(
    start: f64,
    phases: &[(&str, &str, f64)],
    states: &[(&str, &str, f64)],
) -> symphony_api::ReportHour {
    let mut hour = hour(start, 0, 0);
    hour.workflow = Some(symphony_api::reports::Placed {
        top: PlacedHours {
            phases: spent(phases),
            states: spent(states),
        },
        subtasks: PlacedHours {
            phases: spent(&[("subtask", "Subtask", 1.0)]),
            states: spent(&[("working", "Working", 1.0)]),
        },
    });
    hour
}

/// A period sums the top-level tasks' hours by phase and by state over its roll-up hours that
/// record them, each under its ID and label, a label renamed since being an entry of its own; the
/// subtasks are left out, as the period's flow leaves them; and a period no hour of which records
/// them (a board that runs no workflow) has none, which its answer leaves out.
#[test]
fn a_period_sums_its_hours_by_phase_and_by_state_and_one_none_records_has_none() {
    let tree = Tree::new();
    report(
        &tree,
        MONDAY,
        vec![
            placed(
                MONDAY + 9.0 * HOUR,
                &[("build", "Build", 0.75), ("verify", "Verify", 0.25)],
                &[("awaiting", "Awaiting", 0.25), ("working", "Working", 0.75)],
            ),
            placed(
                MONDAY + 10.0 * HOUR,
                &[("build", "Make", 0.5), ("build", "Build", 0.5)],
                &[("working", "Working", 1.0)],
            ),
            // An hour that records no workflow adds nothing and takes nothing away.
            hour(MONDAY + 11.0 * HOUR, 0, 0),
        ],
        |_| {},
    );
    report(
        &tree,
        MONDAY + DAY,
        vec![hour(MONDAY + DAY + 9.0 * HOUR, 1, 0)],
        |_| {},
    );
    let days = page(&folders(&tree), Grain::Day, 3, wednesday(), none);
    let monday = days.buckets[0].workflow.as_ref().unwrap();
    assert_eq!(
        monday.phases,
        spent(&[
            ("build", "Build", 1.25),
            ("verify", "Verify", 0.25),
            ("build", "Make", 0.5),
        ]),
        "summed under each ID and label, a label first met later after the rest"
    );
    assert_eq!(
        monday.states,
        spent(&[("awaiting", "Awaiting", 0.25), ("working", "Working", 1.75)])
    );
    assert_eq!(
        (
            days.buckets[1].workflow.as_ref(),
            days.buckets[2].workflow.as_ref()
        ),
        (None, None),
        "Tuesday's hours record none, Wednesday has no file"
    );
    let text = serde_json::to_string(&days.buckets[1]).unwrap();
    assert!(!text.contains("workflow"), "{text}");
    let by_hour = page(&folders(&tree), Grain::Hour, 1, MONDAY + 11.5 * HOUR, none);
    let ten = (by_hour.buckets.iter())
        .find(|bucket| bucket.start == MONDAY + 10.0 * HOUR)
        .unwrap();
    assert_eq!(
        ten.workflow.as_ref().unwrap().phases,
        spent(&[("build", "Make", 0.5), ("build", "Build", 0.5)]),
        "an hour's entries as its file wrote them"
    );
}

#[test]
fn with_no_file_yet_every_period_is_missing_and_the_first_roll_up_is_none() {
    let tree = Tree::new();
    let page = page(&folders(&tree), Grain::Day, 14, wednesday(), none);
    assert_eq!(page.buckets.len(), 14);
    assert!(page.buckets.iter().all(|bucket| *bucket
        == ReportBucket {
            start: bucket.start,
            end: bucket.end,
            ..ReportBucket::default()
        }));
    assert_eq!((page.first, page.unread.len()), (None, 0));
    // A folder the project has none of is said, and the others still read.
    week(&tree);
    let missing = Folders {
        reports: Err("a board TASKS_SERVER names keeps no day files".into()),
        ..folders(&tree)
    };
    let page = page_of(&missing, Grain::Day, 3);
    assert_eq!(
        page.unread,
        ["reports: a board TASKS_SERVER names keeps no day files"]
    );
    assert_eq!(page.first, None);
    assert!(page.buckets.iter().all(|b| b.flow.is_none()));
    assert!(page.buckets[0].agents.is_some());
    // Marks that cannot be read are said too.
    let page = super::page(&folders(&tree), Grain::Day, 3, wednesday(), |_| {
        Err("git log of main failed: no such branch".into())
    });
    assert_eq!(
        page.unread,
        ["settings and instructions marks: git log of main failed: no such branch"]
    );
}

fn page_of(folders: &Folders, grain: Grain, days: u32) -> ReportsPage {
    page(folders, grain, days, wednesday(), none)
}

#[test]
fn a_file_too_large_a_link_a_fifo_or_one_that_does_not_read_is_named_and_left_out() {
    let tree = Tree::new();
    week(&tree);
    // Monday's roll-up past the size read, Tuesday's a link to it, Wednesday's no day file.
    let monday = tree.path(&format!("reports/{}.json", run_day(MONDAY)));
    let big = std::fs::OpenOptions::new()
        .write(true)
        .open(&monday)
        .unwrap();
    big.set_len(REPORTS_FILE_BYTES + 1).unwrap();
    let tuesday = tree.path(&format!("reports/{}.json", run_day(MONDAY + DAY)));
    std::os::unix::fs::symlink(&monday, &tuesday).unwrap();
    let wednesday_file = tree.path(&format!("reports/{}.json", run_day(MONDAY + 2.0 * DAY)));
    std::fs::write(&wednesday_file, "{\"schema\":1,").unwrap();
    // The agents' Tuesday a FIFO, which no read waits on.
    let _fifo =
        crate::testing::fifo(&tree.path(&format!("agents/{}.jsonl", run_day(MONDAY + DAY))));
    let page = page_of(&folders(&tree), Grain::Day, 3);
    assert_eq!(page.unread.len(), 4, "{:#?}", page.unread);
    let names = |path: &Path, says: &str| {
        let path = path.display().to_string();
        assert!(
            page.unread
                .iter()
                .any(|line| line.starts_with(&format!("{path}: ")) && line.contains(says)),
            "{path} {says}: {:#?}",
            page.unread
        );
    };
    names(&monday, "bytes, over the 4194304 a day file is read to");
    names(&tuesday, "");
    names(&wednesday_file, "EOF");
    names(
        &tree.path(&format!("agents/{}.jsonl", run_day(MONDAY + DAY))),
        "not a regular file",
    );
    assert!(
        page.buckets.iter().all(|b| b.flow.is_none()),
        "no roll-up hour was read"
    );
    assert!(
        page.buckets[0].agents.is_some(),
        "the other sources still read"
    );
    assert!(page.buckets[2].health.is_some());
}

#[test]
fn a_read_takes_its_bytes_from_the_answer_s_and_refuses_one_past_them() {
    let tree = Tree::new();
    let path = tree.write("a.jsonl", "0123456789");
    let mut budget = 25;
    assert_eq!(read(&path, &mut budget), Ok(Some("0123456789".into())));
    assert_eq!(read(&path, &mut budget), Ok(Some("0123456789".into())));
    assert_eq!(budget, 5);
    let refused = read(&path, &mut budget).unwrap_err();
    assert!(
        refused.ends_with("not read, as the 67108864 bytes one answer reads were read"),
        "{refused}"
    );
    assert_eq!(budget, 5, "a file refused takes nothing");
    assert_eq!(read(&tree.path("none.jsonl"), &mut budget), Ok(None));
    std::fs::create_dir(tree.path("folder")).unwrap();
    assert!(
        read(&tree.path("folder"), &mut budget)
            .unwrap_err()
            .ends_with("not a regular file")
    );
}

#[tokio::test]
async fn the_route_refuses_a_bad_ask_first_and_answers_the_hubs_folders_at_its_clock() {
    use axum::http::StatusCode;
    use std::sync::Arc;
    let tree = Tree::new();
    let root = crate::testing::repository(&tree);
    let shared = crate::front::Shared::new(1, 1234, &root, None, crate::testing::open_gate());
    let ask = |uri: &str, tag: Option<&str>| {
        let shared = shared.clone();
        let mut request = axum::http::Request::builder()
            .uri(uri)
            .header("Host", "localhost:1234");
        if let Some(tag) = tag {
            request = request.header("If-None-Match", tag);
        }
        let request = request.body(()).unwrap();
        async move {
            let response = crate::api::answer(&shared, request).await.unwrap();
            let status = response.status();
            let tag = response
                .headers()
                .get("etag")
                .map(|tag| tag.to_str().unwrap().to_string());
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            (status, tag, body)
        }
    };
    let (status, _, body) = ask("/api/reports?grain=month", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({ "error": "grain is hour, day or week, not month" })
    );
    let (status, _, _) = ask("/api/reports?days=x", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, _) = ask("/api/reports", None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "no hub yet");
    assert!(shared.api.get().is_none(), "no hub was started");

    let mut hub = crate::live::Hub::new(&root, &root.join("build/client"));
    hub.clock = Box::new(wednesday);
    week(&tree);
    hub.report_folders.set(folders(&tree)).unwrap();
    assert!(shared.api.set(Arc::new(hub)).is_ok());
    let (status, tag, body) = ask("/api/reports?grain=day&days=3", None).await;
    assert_eq!(status, StatusCode::OK);
    let page: ReportsPage = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        page,
        super::page(&folders(&tree), Grain::Day, 3, wednesday(), none),
        "the scratch repository changed no settings file in the window"
    );
    // The same hour, no file changed: the client's body is still current.
    let tag = tag.expect("a JSON read names its body");
    let (status, _, body) = ask("/api/reports?grain=day&days=3", Some(&tag)).await;
    assert_eq!((status, body.len()), (StatusCode::NOT_MODIFIED, 0));
    // It was answered from the kept one: its files and git were read for the first alone.
    let kept = &shared.api.get().unwrap().reports_kept;
    assert_eq!((kept.passes(), kept.gits()), (1, 1));
    // The landed tasks' read: refused as the page's is, answered from the same folders and clock.
    let (status, _, body) = ask("/api/reports/landed?grain=month", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({ "error": "grain is hour, day or week, not month" })
    );
    landings(&tree);
    let (status, tag, body) = ask("/api/reports/landed?grain=day&days=3", None).await;
    assert_eq!(status, StatusCode::OK);
    let landed: LandedPage = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        landed,
        landed_page(&folders(&tree).reports, Grain::Day, 3, wednesday())
    );
    assert_eq!(landed.tasks.len(), 2);
    let tag = tag.expect("a JSON read names its body");
    let (status, _, _) = ask("/api/reports/landed?grain=day&days=3", Some(&tag)).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
}

/// `n` million input tokens of `role` on Opus 5.5, $4 a million at the table every build embeds,
/// in `round`.
fn opus(role: &str, round: u32, n: u64) -> TokenSum {
    TokenSum {
        role: role.into(),
        round,
        model: "claude-opus-5-5".into(),
        speed: "standard".into(),
        input: n * 1_000_000,
        calls: 1,
        ..TokenSum::default()
    }
}

#[test]
fn a_period_sums_its_hours_tokens_by_role_and_prices_them_and_an_hour_with_none_told_is_unknown() {
    let tree = Tree::new();
    let mut nine = hour(MONDAY + 9.0 * HOUR, 0, 0);
    nine.tokens = Some(vec![opus("reviewer", 0, 1), opus("executor", 0, 2)]);
    let mut ten = hour(MONDAY + 10.0 * HOUR, 0, 0);
    ten.tokens = Some(vec![
        opus("executor", 0, 1),
        TokenSum {
            model: "gpt-x".into(),
            ..opus("worker", 0, 3)
        },
    ]);
    let mut eleven = hour(MONDAY + 11.0 * HOUR, 0, 0);
    eleven.tokens = Some(Vec::new());
    // Twelve: the roll-up wrote the hour and told no tokens of it.
    let twelve = hour(MONDAY + 12.0 * HOUR, 0, 0);
    report(&tree, MONDAY, vec![nine, ten, eleven, twelve], |_| {});
    let said = |spend: &Option<Vec<RoleSpend>>| {
        spend.as_ref().map(|roles| {
            (roles.iter())
                .map(|role| {
                    let cost = &role.cost;
                    (
                        role.role.clone(),
                        role.tokens.input,
                        cost.dollars,
                        cost.unpriced.len(),
                    )
                })
                .collect::<Vec<_>>()
        })
    };
    let now = MONDAY + 12.5 * HOUR;
    let by_hour = page(&folders(&tree), Grain::Hour, 1, now, none);
    assert_eq!(by_hour.prices, Prices::embedded().read);
    let hours: Vec<_> = (by_hour.buckets.iter())
        .filter(|bucket| bucket.start >= MONDAY + 9.0 * HOUR)
        .map(|bucket| said(&bucket.spend))
        .collect();
    assert_eq!(
        hours,
        [
            Some(vec![
                ("executor".to_string(), 2_000_000, 8.0, 0),
                ("reviewer".to_string(), 1_000_000, 4.0, 0),
            ]),
            Some(vec![
                ("executor".to_string(), 1_000_000, 4.0, 0),
                ("worker".to_string(), 3_000_000, 0.0, 1),
            ]),
            Some(vec![]),
            None,
        ],
        "executor first; the model with no price named, in no dollar; none gained is known"
    );
    let by_day = page(&folders(&tree), Grain::Day, 1, now, none);
    assert_eq!(
        said(&by_day.buckets[0].spend),
        Some(vec![
            ("executor".to_string(), 3_000_000, 12.0, 0),
            ("reviewer".to_string(), 1_000_000, 4.0, 0),
            ("worker".to_string(), 3_000_000, 0.0, 1),
        ])
    );
    let landed: Vec<_> = by_day.buckets.iter().map(|bucket| bucket.landed).collect();
    assert_eq!(landed, [None], "no task landed");
}

/// A task landed in the window, of `kind` with `lines` added, having taken `spent`.
fn landed(slug: &str, at: f64, spent: Vec<TokenSum>) -> Landed {
    Landed {
        slug: slug.into(),
        at,
        spent,
        ..Landed::default()
    }
}

/// `landed` given `kind` and `lines` added.
fn kinded(mut landed: Landed, kind: &str, lines: u64) -> Landed {
    landed.kind = Some(kind.into());
    landed.lines = Some(symphony_api::reports::Lines {
        added: lines,
        removed: 0,
    });
    landed
}

/// Monday lands a task Wednesday lands again, a cheap one with its lines and one with no tokens;
/// Wednesday's landing of the first was reviewed twice and changed lines.
fn landings(tree: &Tree) {
    report(tree, MONDAY, vec![hour(MONDAY, 0, 0)], |day| {
        day.landed = vec![
            landed("a/again", MONDAY + HOUR, vec![opus("executor", 1, 9)]),
            kinded(
                landed("a/cheap", MONDAY + 2.0 * HOUR, vec![opus("executor", 1, 1)]),
                "docs",
                50,
            ),
            landed("a/none", MONDAY + 3.0 * HOUR, vec![]),
        ];
    });
    let wednesday_start = MONDAY + 2.0 * DAY;
    report(
        tree,
        wednesday_start,
        vec![hour(wednesday_start, 0, 0)],
        |day| {
            day.landed = vec![kinded(
                landed(
                    "a/again",
                    wednesday_start + HOUR,
                    vec![
                        opus("executor", 1, 2),
                        opus("reviewer", 2, 1),
                        opus("executor", 3, 1),
                    ],
                ),
                "fix",
                400,
            )];
        },
    );
}

#[test]
fn a_period_counts_the_tasks_landed_in_it_once_each_at_their_last_landing() {
    let tree = Tree::new();
    landings(&tree);
    let page = page(&folders(&tree), Grain::Day, 3, wednesday(), none);
    let landed: Vec<Option<BucketLanded>> =
        page.buckets.iter().map(|bucket| bucket.landed).collect();
    assert_eq!(
        landed,
        [
            // a/again is Wednesday's: its Monday landing is in no period.
            Some(BucketLanded {
                tasks: 2,
                priced: 1,
                median: Some(4.0),
                p90: Some(4.0),
                lined: 1,
                lined_cost: 4.0,
                lines: 50,
            }),
            None,
            Some(BucketLanded {
                tasks: 1,
                priced: 1,
                median: Some(16.0),
                p90: Some(16.0),
                lined: 1,
                lined_cost: 16.0,
                lines: 400,
            }),
        ]
    );
    // A window that holds only Wednesday sees its landing alone.
    let today = super::page(&folders(&tree), Grain::Day, 1, wednesday(), none);
    assert_eq!(today.buckets[0].landed.map(|landed| landed.tasks), Some(1));
}

#[test]
fn the_landed_tab_prices_each_task_once_at_its_last_landing_with_its_figures_and_groups() {
    let tree = Tree::new();
    landings(&tree);
    let page = landed_page(&Ok(tree.path("reports")), Grain::Day, 3, wednesday());
    assert_eq!((page.grain, page.days), (Grain::Day, 3));
    // As the page's window: to the end of the clock's hour.
    assert_eq!(
        (page.from, page.to),
        (MONDAY, MONDAY + 2.0 * DAY + 11.0 * HOUR)
    );
    assert_eq!(
        page.tasks,
        [
            LandedRow {
                slug: "a/again".into(),
                at: MONDAY + 2.0 * DAY + HOUR,
                kind: Some("fix".into()),
                first: 8.0,
                later: 8.0,
                tokens: 4_000_000,
                unpriced: false,
                reviews: 2,
                lines: Some(400),
            },
            LandedRow {
                slug: "a/cheap".into(),
                at: MONDAY + 2.0 * HOUR,
                kind: Some("docs".into()),
                first: 4.0,
                later: 0.0,
                tokens: 1_000_000,
                unpriced: false,
                reviews: 0,
                lines: Some(50),
            },
        ]
    );
    assert_eq!(page.without, 1);
    assert_eq!(
        page.figures,
        LandedFigures {
            tasks: 2,
            median: Some(10.0),
            mean: Some(10.0),
            p90: Some(14.8),
            cost: 20.0,
            later: 8.0,
            back: 1,
            lined: 2,
            lined_cost: 20.0,
            lines: 450,
            unlined: 0,
            unread: 0,
        }
    );
    let kinds: Vec<(Option<&str>, u32, f64)> = (page.kinds.iter())
        .map(|row| (row.kind.as_deref(), row.tasks, row.cost))
        .collect();
    assert_eq!(kinds, [(Some("fix"), 1, 16.0), (Some("docs"), 1, 4.0)]);
    let reviews: Vec<(u32, u32, f64, f64)> = (page.reviews.iter())
        .map(|row| (row.reviews, row.tasks, row.first, row.later))
        .collect();
    assert_eq!(reviews, [(0, 1, 4.0, 0.0), (2, 1, 8.0, 8.0)]);
    // Monday's a/cheap is the first landing that names a kind.
    assert_eq!(page.kinds_from, Some(MONDAY + 2.0 * HOUR));
    assert_eq!(page.prices, Prices::embedded().read);
    assert!(page.unread.is_empty(), "{:?}", page.unread);
    // Wednesday alone: a/again, of no landing before it.
    let today = landed_page(&Ok(tree.path("reports")), Grain::Day, 1, wednesday());
    assert_eq!(today.tasks.len(), 1);
    assert_eq!(today.kinds_from, Some(MONDAY + 2.0 * DAY + HOUR));
}

#[test]
fn a_landed_tab_with_no_landing_no_folder_or_a_file_that_does_not_read_says_so() {
    let tree = Tree::new();
    report(&tree, MONDAY, vec![hour(MONDAY, 0, 0)], |_| {});
    let empty = landed_page(&Ok(tree.path("reports")), Grain::Day, 3, wednesday());
    assert!(empty.tasks.is_empty());
    assert_eq!((empty.without, empty.figures.tasks), (0, 0));
    assert_eq!((empty.figures.median, empty.kinds_from), (None, None));
    assert!(empty.kinds.is_empty() && empty.reviews.is_empty() && empty.unread.is_empty());
    let none = landed_page(&Err("no common dir".into()), Grain::Week, 28, wednesday());
    assert_eq!(none.unread, ["reports: no common dir"]);
    let path = tree
        .path("reports")
        .join(report_file(&run_day(MONDAY + 2.0 * DAY)));
    std::fs::write(&path, "{not json").unwrap();
    let broken = landed_page(&Ok(tree.path("reports")), Grain::Day, 3, wednesday());
    assert_eq!(broken.unread.len(), 1);
    assert!(
        broken.unread[0].starts_with(&path.display().to_string()),
        "{:?}",
        broken.unread
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    let folder = landed_page(&Ok(tree.path("reports")), Grain::Day, 3, wednesday());
    assert_eq!(
        folder.unread,
        [format!("{}: not a regular file", path.display())]
    );
}
