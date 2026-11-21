//! `show`'s stages, `stats` and when each task took the status it holds (`list --json`, `search
//! --json`, `tasks api`'s `in_status_since`), on a board whose clock each command sets: every
//! expected time is written out here by hand.

use crate::Case;
use crate::common::{NOW, Seed, task};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::Instant;
use symphony_board::reports::{DAY, day as report_day, day_start};
use symphony_board::rows::{MessageRow, TaskRow};
use symphony_board::sys::local_minute;

#[path = "../perf/proxy.rs"]
#[allow(dead_code)]
mod proxy;

const H: f64 = 3600.0;
/// The tokens' line of `stats` on a board `TASKS_SERVER` names with no folder of day files named,
/// as every board of these tests is: its last.
const NO_DAY_FILES: &str = "Tokens: no day file read (a board TASKS_SERVER names keeps no reports \
                            unless SYMPHONY_REPORTS names their folder)\n";
/// When the window's tasks are made: ten days after the old one.
const START: f64 = NOW + 10.0 * 86_400.0;

/// The board with its clock `hours` after `from`.
fn at(c: &Case, from: f64, hours: f64) -> Case {
    let mut env = c.env.clone();
    env.vars
        .insert("TASKS_NOW".into(), (from + hours * H).to_string());
    Case { env }
}

/// A board with one task from before the window and, from `START` on: `sim/a` approved after 4
/// hours, `sim/b` after 16 and one bounce, `sim/c` blocked at 6, `sim/d` ready and unclaimed,
/// `sim/e` in the backlog, and a subtask.
fn pipeline() -> Case {
    pipeline_by().0
}

/// [`pipeline`], with the handles of its orchestrator, executor and reviewer.
fn pipeline_by() -> (Case, [String; 3]) {
    let c = Case::new();
    let owner = c.register("orchestrator");
    c.ok(&owner, &["create", "sim/old", "Old"]);
    let t = |hours: f64| at(&c, START, hours);
    let (orch, exe, rev) = (
        t(0.0).register("orchestrator"),
        t(0.0).register("worker"),
        t(0.0).register("reviewer"),
    );
    for slug in ["sim/a", "sim/b", "sim/c", "sim/d"] {
        t(0.0).ok(&orch, &["create", slug, "Work"]);
    }
    t(0.0).ok(&orch, &["create", "sim/e", "Later", "--stage", "backlog"]);
    t(0.0).ok(&orch, &["create", "sim/d/s", "Slice", "--parent", "sim/d"]);
    t(1.0).ok(&exe, &["claim", "sim/a"]);
    t(3.0).ok(&exe, &["claim", "sim/c"]);
    t(3.0).ok(&exe, &["move", "sim/a", "review"]);
    t(3.0).ok(&rev, &["role", "sim/a", "reviewer"]);
    t(4.0).ok(&rev, &["verdict", "sim/a", "approve", "fine"]);
    t(5.0).ok(&exe, &["claim", "sim/b"]);
    t(6.0).ok(&exe, &["move", "sim/c", "blocked", "--on", "parked"]);
    t(8.0).ok(&exe, &["move", "sim/b", "review"]);
    t(8.0).ok(&rev, &["role", "sim/b", "reviewer"]);
    t(10.0).ok(&rev, &["verdict", "sim/b", "changes", "a test is missing"]);
    t(13.0).ok(&exe, &["move", "sim/b", "review"]);
    t(16.0).ok(&rev, &["verdict", "sim/b", "approve"]);
    // The task from before the window is done inside it, 22 hours in.
    t(20.0).ok(&exe, &["claim", "sim/old"]);
    t(21.0).ok(&exe, &["move", "sim/old", "review"]);
    t(21.0).ok(&rev, &["role", "sim/old", "reviewer"]);
    t(22.0).ok(&rev, &["verdict", "sim/old", "approve"]);
    (c, [orch, exe, rev])
}

#[test]
fn show_says_where_a_tasks_time_went() {
    let (c, [orch, exe, rev]) = pipeline_by();
    let later = at(&c, START, 20.0);
    let shown = later.ok("", &["show", "sim/b"]);
    assert!(
        shown.contains(
            "\n  Stages: ready 5.0 h · in_progress 6.0 h (2 visits) · review 5.0 h (2 visits) · 1 bounce; first claimed after 5.0 h\n"
        ),
        "{shown}"
    );
    let shown: Value = serde_json::from_str(&later.ok("", &["show", "sim/b", "--json"])).unwrap();
    assert_eq!(
        shown["stages"],
        json!({
            "statuses": {
                "ready": {"first_entered": START, "seconds": 18_000.0, "visits": 1},
                "in_progress": {"first_entered": START + 5.0 * H, "seconds": 21_600.0, "visits": 2},
                "review": {"first_entered": START + 8.0 * H, "seconds": 18_000.0, "visits": 2},
                "done": {"first_entered": START + 16.0 * H, "seconds": 0.0, "visits": 1},
            },
            // Each stretch, who moved it there: made by the orchestrator, claimed and sent to review
            // by the executor, sent back and closed by the reviewer's verdicts.
            "segments": [
                {"status": "ready", "from": START, "to": START + 5.0 * H, "by": orch},
                {"status": "in_progress", "from": START + 5.0 * H, "to": START + 8.0 * H, "by": exe},
                {"status": "review", "from": START + 8.0 * H, "to": START + 10.0 * H, "by": exe},
                {"status": "in_progress", "from": START + 10.0 * H, "to": START + 13.0 * H, "by": rev},
                {"status": "review", "from": START + 13.0 * H, "to": START + 16.0 * H, "by": exe},
                {"status": "done", "from": START + 16.0 * H, "to": null, "by": rev},
            ],
            "unclaimed_seconds": 18_000.0,
            "first_claimed": START + 5.0 * H,
            "bounces": 1,
            "current": {"status": "done", "since": START + 16.0 * H},
            "closed": START + 16.0 * H,
        })
    );
    // Open and never claimed: counted up to the clock's now.
    let shown = later.ok("", &["show", "sim/d"]);
    assert!(
        shown.contains("\n  Stages: ready 20.0 h; never claimed in 20.0 h\n"),
        "{shown}"
    );
    let shown: Value = serde_json::from_str(&later.ok("", &["show", "sim/d", "--json"])).unwrap();
    assert_eq!(
        shown["stages"],
        json!({
            "statuses": {"ready": {"first_entered": START, "seconds": 72_000.0, "visits": 1}},
            "segments": [{"status": "ready", "from": START, "to": null, "by": orch}],
            "unclaimed_seconds": 72_000.0,
            "first_claimed": null,
            "bounces": 0,
            "current": {"status": "ready", "since": START},
            "closed": null,
        })
    );
}

/// The day `hours` after `START` falls on, as the thread's own times are written.
fn day(hours: f64) -> String {
    local_minute(START + hours * H)[..10].to_string()
}

#[test]
fn stats_says_where_the_pipelines_time_goes() {
    let c = pipeline();
    let later = at(&c, START, 48.0);
    // `sim/a` closed 4 hours in, `sim/b` 16 and `sim/old`, made before the window, 22: whichever
    // of the window's days those fall on here.
    let days: Vec<String> = [-24.0, 0.0, 24.0, 48.0]
        .iter()
        .map(|hours| {
            let closed = [4.0, 16.0, 22.0].iter().filter(|h| day(**h) == day(*hours));
            format!("  {}  {}\n", day(*hours), closed.count())
        })
        .collect();
    let wanted = format!(
        "Stats: 5 top-level tasks made in the last 3 days; every figure is theirs but done per day
Status: backlog 1 · ready 1 · done 2 · blocked 1
Done 2: lead time median 10.0 h · p75 13.0 h · p90 14.8 h
  ready        median 3.0 h · p75 4.0 h · p90 4.6 h  (2 tasks)
  in_progress  median 4.0 h · p75 5.0 h · p90 5.6 h  (2 tasks)
  review       median 3.0 h · p75 4.0 h · p90 4.6 h  (2 tasks)
Waiting before the first claim (a part of the statuses above, not added to them): median 3.0 h · p75 4.0 h · p90 4.6 h
Bounces (review → in_progress × tasks): 0 × 4 · 1 × 1
Done per day, local time (each top-level task done now, once, on the day of its last close in the last 3 days, whenever it was made):
{}Longest in their status:
  sim/d                        ready       48.0 h
  sim/e                        backlog     48.0 h
  sim/c                        blocked     42.0 h
Blocked: 1, median 42.0 h there
Ready with no executor: 1, median 48.0 h there
{NO_DAY_FILES}",
        days.concat()
    );
    assert_eq!(later.ok("", &["stats", "--days", "3"]), wanted);
}

#[test]
fn stats_as_json_carries_the_same_figures_in_seconds() {
    let c = pipeline();
    let later = at(&c, START, 48.0);
    let said: Value =
        serde_json::from_str(&later.ok("", &["stats", "--days", "3", "--json"])).unwrap();
    assert_eq!(said["days"], 3);
    assert_eq!(said["tasks"], 5);
    assert_eq!(
        said["by_status"],
        json!({
            "backlog": 1, "concept": 0, "design": 0, "ready": 1, "in_progress": 0, "review": 0,
            "done": 2, "blocked": 1, "cancelled": 0,
        })
    );
    let spread = |found: &Value, median: f64, p75: f64, p90: f64| {
        assert_eq!(found["tasks"], 2, "{found}");
        for (name, wanted) in [("median", median), ("p75", p75), ("p90", p90)] {
            let found = found[name].as_f64().unwrap();
            assert!((found - wanted * H).abs() < 1e-6, "{name}: {found}");
        }
    };
    spread(&said["lead_seconds"], 10.0, 13.0, 14.8);
    spread(&said["waiting_before_first_claim_seconds"], 3.0, 4.0, 4.6);
    spread(&said["stage_seconds"]["ready"], 3.0, 4.0, 4.6);
    spread(&said["stage_seconds"]["in_progress"], 4.0, 5.0, 5.6);
    spread(&said["stage_seconds"]["review"], 3.0, 4.0, 4.6);
    assert_eq!(said["stage_seconds"].as_object().unwrap().len(), 3);
    assert_eq!(
        said["bounces"],
        json!([{"bounces": 0, "tasks": 4}, {"bounces": 1, "tasks": 1}])
    );
    let done: u64 = (said["done_per_day"].as_array().unwrap().iter())
        .map(|day| day["done"].as_u64().unwrap())
        .sum();
    // The two made in the window and the one made before it.
    assert_eq!(done, 3);
    assert_eq!(said["done_per_day"][0]["day"], day(-24.0));
    assert_eq!(
        said["slowest"],
        json!([
            {"slug": "sim/d", "status": "ready", "seconds": 172_800.0},
            {"slug": "sim/e", "status": "backlog", "seconds": 172_800.0},
            {"slug": "sim/c", "status": "blocked", "seconds": 151_200.0},
        ])
    );
    assert_eq!(
        said["blocked"],
        json!({"tasks": 1, "median_seconds": 151_200.0})
    );
    assert_eq!(
        said["ready_unstaffed"],
        json!({"tasks": 1, "median_seconds": 172_800.0})
    );
}

#[test]
fn the_window_read_holds_top_level_tasks_their_executors_and_no_posts() {
    let c = pipeline();
    let records = c.store().stage_records(START).unwrap();
    let slugs: Vec<&str> = records.iter().map(|r| r.slug.as_str()).collect();
    // `sim/old` was made before `START` and done after it.
    assert_eq!(
        slugs,
        ["sim/old", "sim/a", "sim/b", "sim/c", "sim/d", "sim/e"]
    );
    assert_eq!(records[0].created, NOW);
    let executing: Vec<bool> = records.iter().map(|r| r.executing).collect();
    assert_eq!(executing, [true, true, true, true, false, false]);
    let said: Vec<(&str, &str)> = (records[2].lines.iter())
        .map(|l| (l.kind.as_str(), l.text.as_str()))
        .collect();
    assert_eq!(said.len(), 8, "{said:?}");
    assert_eq!(said[0].0, "system");
    assert!(said[0].1.starts_with("created by "), "{said:?}");
    // The orchestrator that filed it holds it from the create.
    assert!(said[1].1.ends_with(" joined as orchestrator"), "{said:?}");
    // A verdict is cut to the word that names it.
    assert_eq!(said[5], ("verdict", "changes: a test "));
    assert_eq!(said[7], ("verdict", "approve"));
    assert_eq!(records[2].created, START);
    assert_eq!(records[2].updated, START + 16.0 * H);
    assert_eq!(records[2].status, "done");
}

#[test]
fn the_default_window_is_fourteen_days_and_takes_in_the_old_task() {
    let c = pipeline();
    let said = at(&c, START, 48.0).ok("", &["stats"]);
    assert!(
        said.starts_with(
            "Stats: 6 top-level tasks made in the last 14 days; every figure is theirs but done per day\nStatus: backlog 1 · ready 1 · done 3 · blocked 1\n"
        ),
        "{said}"
    );
    assert!(
        said.contains("Ready with no executor: 1, median 48.0 h there\n"),
        "{said}"
    );
}

#[test]
fn stats_of_an_empty_board_and_a_window_of_no_days() {
    let c = Case::new();
    let today = &local_minute(NOW)[..10];
    let before = &local_minute(NOW - 86_400.0)[..10];
    assert_eq!(
        c.ok("", &["stats", "--days", "1"]),
        format!(
            "Stats: 0 top-level tasks made in the last 1 days; every figure is theirs but done per day\nDone 0\nDone per day, local time (each top-level task done now, once, on the day of its last close in the last 1 days, whenever it was made):\n  {before}  0\n  {today}  0\nBlocked: 0\nReady with no executor: 0\n{NO_DAY_FILES}"
        )
    );
    let refused = c.run_as("", &["stats", "--days", "0"], "");
    assert_eq!(refused.code, 1);
    assert_eq!(
        refused.err,
        "✖ --days is a number of days from 1 to 3650, not 0\n"
    );
    let refused = c.run_as("", &["stats", "--days", "3651"], "");
    assert_eq!(refused.code, 1);
    assert_eq!(
        refused.err,
        "✖ --days is a number of days from 1 to 3650, not 3651\n"
    );
    assert!(
        c.ok("", &["stats", "--days", "3650"])
            .starts_with("Stats: 0 top-level tasks made in the last 3650 days;")
    );
}

/// A task of [`done_twice_and_undone`]: its slug, its parent, its status now and the hour (from
/// the UTC midnight before `NOW`) it was last written, and its thread, each line its hour, kind and
/// text.
type Written = (
    &'static str,
    Option<&'static str>,
    &'static str,
    f64,
    Vec<(f64, &'static str, &'static str)>,
);

/// A board of what makes the roll-up's entries into done and the tasks done part, its threads as
/// the board writes them, every close on the hour: from the UTC midnight `D` before `NOW`, `sim/again`
/// approved at 3, sent back to review by its landing and approved again at 4; `sim/next` approved at
/// 20, sent back the next day and approved again at 24 + 3; `sim/cancelled` approved at 9 and
/// cancelled by a hand edit at 10, which no line records; `sim/reopened` approved at 24 + 2 and sent
/// back at 24 + 4; `sim/once` approved at 24 + 5, and its subtask closed at 24 + 6.
fn done_twice_and_undone() -> Case {
    let worked = |made: f64| {
        vec![
            (made, "system", "created by orch"),
            (made + 0.5, "system", "exe claimed it"),
            (made + 1.0, "system", "exe moved it in_progress → review"),
        ]
    };
    let with = |mut lines: Vec<(f64, &'static str, &'static str)>,
                more: &[(f64, &'static str, &'static str)]| {
        lines.extend_from_slice(more);
        lines
    };
    let back = "exe moved it done → review";
    let tasks: Vec<Written> = vec![
        (
            "sim/again",
            None,
            "done",
            4.0,
            with(
                worked(1.0),
                &[
                    (3.0, "verdict", "approve: fine"),
                    (3.5, "system", back),
                    (4.0, "verdict", "approve: the merge"),
                    (4.5, "verdict", "landed sim/again on main"),
                ],
            ),
        ),
        (
            "sim/next",
            None,
            "done",
            27.0,
            with(
                worked(5.0),
                &[
                    (20.0, "verdict", "approve"),
                    (25.0, "system", back),
                    (27.0, "verdict", "approve"),
                ],
            ),
        ),
        (
            "sim/cancelled",
            None,
            "cancelled",
            10.0,
            with(worked(6.0), &[(9.0, "verdict", "approve")]),
        ),
        (
            "sim/reopened",
            None,
            "review",
            28.0,
            with(
                worked(6.0),
                &[(26.0, "verdict", "approve"), (28.0, "system", back)],
            ),
        ),
        (
            "sim/once",
            None,
            "done",
            29.0,
            with(worked(25.0), &[(29.0, "verdict", "approve")]),
        ),
        (
            "sim/once/slice",
            Some("sim/once"),
            "done",
            30.0,
            vec![
                (25.0, "system", "created by orch"),
                (25.5, "system", "exe claimed it"),
                (30.0, "verdict", "done: merged in"),
            ],
        ),
    ];
    let d = day_start(NOW);
    let mut seed = Seed::default();
    let mut sid = 0;
    for (n, (slug, parent, status, updated, lines)) in tasks.into_iter().enumerate() {
        seed = seed.task_row(TaskRow {
            id: n as i64 + 1,
            parent: parent.map(str::to_string),
            created: d + lines[0].0 * H,
            updated: d + updated * H,
            ..task(slug, status)
        });
        for (hours, kind, text) in lines {
            sid += 1;
            seed.dump.messages.push(MessageRow {
                id: sid,
                slug: slug.into(),
                sender: "exe".into(),
                recipient: None,
                kind: kind.into(),
                text: text.into(),
                ts: d + hours * H,
            });
        }
    }
    let c = Case::new();
    c.store().copy(&seed.dump).expect("the seed copies");
    c
}

/// `stats`' tasks done per day and the roll-up's (`reports::day`, which the Reports page's Tasks
/// done sums) agree on one board read at one time: each top-level task done now, once, at its last
/// close. A task done twice counts once, a task closed one day and again the next counts the next,
/// and a task cancelled after its approve or reopened and still in review counts in neither, where
/// the roll-up's entries into done count every one. `stats` names its days by this machine's time
/// zone and the roll-up its hours by UTC, so the roll-up's hours are named here as `stats` names
/// them: each close is on the hour, so its hour's first second falls on its day in any zone.
#[test]
fn stats_and_the_roll_up_count_the_same_tasks_done_on_each_day() {
    let c = done_twice_and_undone();
    let d = day_start(NOW);
    let now = d + DAY + 12.0 * H;
    let said: Value =
        serde_json::from_str(&at(&c, now, 0.0).ok("", &["stats", "--days", "2", "--json"]))
            .unwrap();
    let mut stats: Vec<(String, u64)> = (said["done_per_day"].as_array().unwrap().iter())
        .map(|day| {
            (
                day["day"].as_str().unwrap().to_string(),
                day["done"].as_u64().unwrap(),
            )
        })
        .filter(|(_, done)| *done > 0)
        .collect();
    stats.sort();
    let read = c.store().report_records(d).unwrap();
    let days = [
        report_day("sim", d, d + DAY, now, &read),
        report_day("sim", d + DAY, now, now, &read),
    ];
    let total = |pick: fn(&symphony_api::ReportHour) -> u32| -> Vec<u32> {
        days.iter()
            .map(|day| day.hours.iter().map(pick).sum())
            .collect()
    };
    assert_eq!(
        total(|h| h.top.done.unwrap()),
        [1, 2],
        "sim/again the first UTC day; sim/next and sim/once the second"
    );
    assert_eq!(
        total(|h| h.top.entered.done),
        [4, 3],
        "sim/again twice, sim/next and sim/cancelled the first day; sim/next, sim/reopened and \
         sim/once the second"
    );
    let mut rolled: Vec<(String, u64)> = Vec::new();
    for hour in days.iter().flat_map(|day| &day.hours) {
        let done = u64::from(hour.top.done.unwrap());
        if done > 0 {
            let name = local_minute(hour.start)[..10].to_string();
            match rolled.iter_mut().find(|(day, _)| *day == name) {
                Some((_, count)) => *count += done,
                None => rolled.push((name, done)),
            }
        }
    }
    rolled.sort();
    assert_eq!(stats, rolled);
    assert_eq!(stats.iter().map(|(_, done)| done).sum::<u64>(), 3);
}

/// A board of `tasks` top-level tasks made at `NOW`, each claimed, sent to review, sent back,
/// reviewed again and approved, with two posts on its thread: eight messages a task.
fn seeded(tasks: usize) -> Case {
    let mut seed = Seed::default();
    let mut sid = 0;
    for n in 0..tasks {
        let slug = format!("sim/t{n:04}");
        seed = seed.task_row(TaskRow {
            id: n as i64 + 1,
            updated: NOW + 5.0 * H,
            ..task(&slug, "done")
        });
        let said = [
            ("system", "created by owner"),
            ("system", "teal claimed it"),
            ("msg", "started on it"),
            ("system", "teal moved it in_progress → review"),
            ("verdict", "changes: a test"),
            ("msg", "added the test"),
            ("system", "teal moved it in_progress → review"),
            ("verdict", "approve"),
        ];
        for (step, (kind, text)) in said.into_iter().enumerate() {
            sid += 1;
            seed.dump.messages.push(MessageRow {
                id: sid,
                slug: slug.clone(),
                sender: "teal".into(),
                recipient: None,
                kind: kind.into(),
                text: text.into(),
                ts: NOW + step as f64 * 1800.0,
            });
        }
    }
    let c = Case::new();
    c.store().copy(&seed.dump).expect("the seed copies");
    c
}

/// `stats` on a board of `tasks` tasks, through a relay that keeps each request: what it printed,
/// the requests it sent and the seconds it took.
fn measured(tasks: usize) -> (String, Vec<proxy::Request>, f64) {
    let c = seeded(tasks);
    let relay = proxy::Proxy::to(&c.env.vars["TASKS_SERVER"]);
    let mut env = c.env.clone();
    env.vars.insert("TASKS_SERVER".into(), relay.addr.clone());
    env.vars
        .insert("TASKS_NOW".into(), (NOW + 24.0 * H).to_string());
    let through = Case { env };
    let started = Instant::now();
    let said = through.ok("", &["stats"]);
    let took = started.elapsed().as_secs_f64();
    (said, relay.take(), took)
}

#[test]
fn stats_reads_the_board_once_however_many_tasks_it_holds() {
    let (few, few_requests, _) = measured(3);
    let (many, many_requests, took) = measured(300);
    eprintln!(
        "stats over 300 tasks and 2400 messages: {} requests, {:.0} ms",
        many_requests.len(),
        took * 1000.0
    );
    assert!(few.starts_with("Stats: 3 top-level tasks"), "{few}");
    assert!(many.starts_with("Stats: 300 top-level tasks"), "{many}");
    // Each task: claimed at half an hour, in review at 1.5 and 3 hours, approved at 3.5.
    assert!(
        many.contains(
            "Done 300: lead time median 3.5 h · p75 3.5 h · p90 3.5 h\n  ready        median 0.5 h · p75 0.5 h · p90 0.5 h  (300 tasks)\n  in_progress  median 2.0 h · p75 2.0 h · p90 2.0 h  (300 tasks)\n  review       median 1.0 h · p75 1.0 h · p90 1.0 h  (300 tasks)\nWaiting before the first claim (a part of the statuses above, not added to them): median 0.5 h · p75 0.5 h · p90 0.5 h\nBounces (review → in_progress × tasks): 1 × 300\n"
        ),
        "{many}"
    );
    let reads = |requests: &[proxy::Request]| {
        (requests.iter())
            .filter(|r| r.sql.contains("FROM message"))
            .count()
    };
    // One read of the schema's version to open the board, one of the window.
    assert_eq!((few_requests.len(), reads(&few_requests)), (2, 1));
    assert_eq!((many_requests.len(), reads(&many_requests)), (2, 1));
}

/// Each task `listed` (a `list --json` or `search --json` answer) names, with its `in_status_since`.
fn since_by_slug(listed: &str) -> BTreeMap<String, Value> {
    let listed: Value = serde_json::from_str(listed).unwrap();
    (listed["tasks"].as_array().unwrap().iter())
        .map(|t| {
            (
                t["slug"].as_str().unwrap().to_string(),
                t["in_status_since"].clone(),
            )
        })
        .collect()
}

/// [`pipeline_by`], and `sim/f` made 17 hours in, claimed at 18, in review at 19 and sent back at
/// 20, still in progress.
fn pipeline_sent_back() -> Case {
    let (c, [orch, exe, rev]) = pipeline_by();
    let t = |hours: f64| at(&c, START, hours);
    t(17.0).ok(&orch, &["create", "sim/f", "Again"]);
    t(18.0).ok(&exe, &["claim", "sim/f"]);
    t(19.0).ok(&exe, &["move", "sim/f", "review"]);
    t(19.0).ok(&rev, &["role", "sim/f", "reviewer"]);
    t(20.0).ok(&rev, &["verdict", "sim/f", "changes", "one more case"]);
    c
}

#[test]
fn list_says_when_each_task_took_the_status_it_holds() {
    let c = pipeline_sent_back();
    let later = at(&c, START, 30.0);
    let open = BTreeMap::from([
        // Blocked by its executor 6 hours in.
        ("sim/c".to_string(), json!(START + 6.0 * H)),
        // Never claimed, a subtask among them: ready, or in the backlog, since they were made.
        ("sim/d".to_string(), json!(START)),
        ("sim/d/s".to_string(), json!(START)),
        ("sim/e".to_string(), json!(START)),
        // In progress again from the changes verdict, not from its first claim.
        ("sim/f".to_string(), json!(START + 20.0 * H)),
    ]);
    assert_eq!(since_by_slug(&later.ok("", &["list", "--json"])), open);
    let mut all = open.clone();
    all.extend([
        // Done at its approve, whenever it was made.
        ("sim/a".to_string(), json!(START + 4.0 * H)),
        ("sim/b".to_string(), json!(START + 16.0 * H)),
        ("sim/old".to_string(), json!(START + 22.0 * H)),
    ]);
    assert_eq!(
        since_by_slug(&later.ok("", &["list", "--json", "--all"])),
        all
    );
    let blocked = since_by_slug(&later.ok("", &["list", "--json", "--status", "blocked"]));
    assert_eq!(
        blocked,
        BTreeMap::from([("sim/c".to_string(), json!(START + 6.0 * H))])
    );
    // Search's rows are list's: they carry it too.
    assert_eq!(
        since_by_slug(&later.ok("", &["search", "Again", "--json"])),
        BTreeMap::from([("sim/f".to_string(), json!(START + 20.0 * H))])
    );
    // The clock only counts: an hour later nothing moved, so nothing changed.
    assert_eq!(
        since_by_slug(&at(&c, START, 31.0).ok("", &["list", "--json", "--all"])),
        all
    );
}

#[test]
fn the_api_says_when_each_task_named_took_its_status_and_refuses_one_no_task_has() {
    let c = pipeline_sent_back();
    let later = at(&c, START, 30.0);
    let call = |slugs: Value| -> Value {
        let input = json!({"method": "in_status_since", "args": {"slugs": slugs}}).to_string();
        let ran = later.run_as("", &["api"], &input);
        assert_eq!(ran.code, 0, "{}", ran.err);
        serde_json::from_str(&ran.out).unwrap()
    };
    assert_eq!(
        call(json!(["sim/f", "sim/a", "sim/c", "sim/f"])),
        json!({"ok": {
            "sim/a": START + 4.0 * H,
            "sim/c": START + 6.0 * H,
            "sim/f": START + 20.0 * H,
        }})
    );
    assert_eq!(call(json!([])), json!({"ok": {}}));
    assert_eq!(
        call(json!(["sim/a", "sim/nope"])),
        json!({"refused": "no task 'sim/nope'"})
    );
}

/// A board whose orchestrator filed two tasks on the arm `control`, two on `no-review`, one on
/// `later` and one on none, `START` on. `control`: `exp/a1` approved after 8 hours, 4 of them in
/// review over two rounds, and `exp/a2` after 10, 6 in review. `no-review`: `exp/b1` approved
/// after 3 hours, 1 in review, and `exp/b2` sent back twice and still in progress. `exp/c1`, on
/// `later`, and `exp/plain` were never claimed.
fn experiment() -> Case {
    let c = Case::new();
    let t = |hours: f64| at(&c, START, hours);
    let (orch, exe, rev) = (
        t(0.0).register("orchestrator"),
        t(0.0).register("worker"),
        t(0.0).register("reviewer"),
    );
    for (slug, arm) in [
        ("exp/a1", "control"),
        ("exp/b1", "no-review"),
        ("exp/a2", "control"),
        ("exp/b2", "no-review"),
        ("exp/c1", "later"),
    ] {
        t(0.0).ok(&orch, &["create", slug, "Work", "--arm", arm]);
    }
    t(0.0).ok(&orch, &["create", "exp/plain", "Work"]);
    let review = |hours: f64, slug: &str| {
        t(hours).ok(&exe, &["move", slug, "review"]);
    };
    let verdict = |hours: f64, slug: &str, verdict: &str| {
        t(hours).ok(&rev, &["verdict", slug, verdict, "read"]);
    };
    for (hours, slug) in [
        (1.0, "exp/a1"),
        (2.0, "exp/a2"),
        (1.0, "exp/b1"),
        (1.0, "exp/b2"),
    ] {
        t(hours).ok(&exe, &["claim", slug]);
    }
    review(3.0, "exp/a1");
    t(3.0).ok(&rev, &["role", "exp/a1", "reviewer"]);
    verdict(5.0, "exp/a1", "changes");
    review(6.0, "exp/a1");
    verdict(8.0, "exp/a1", "approve");
    review(4.0, "exp/a2");
    t(4.0).ok(&rev, &["role", "exp/a2", "reviewer"]);
    verdict(10.0, "exp/a2", "approve");
    review(2.0, "exp/b1");
    t(2.0).ok(&rev, &["role", "exp/b1", "reviewer"]);
    verdict(3.0, "exp/b1", "approve");
    review(4.0, "exp/b2");
    t(4.0).ok(&rev, &["role", "exp/b2", "reviewer"]);
    verdict(5.0, "exp/b2", "changes");
    review(6.0, "exp/b2");
    verdict(7.0, "exp/b2", "changes");
    c
}

#[test]
fn stats_gives_each_arms_tasks_done_lead_review_and_bounces() {
    let later = at(&experiment(), START, 12.0);
    let said = later.ok("", &["stats", "--days", "1"]);
    assert!(
        said.ends_with(&format!(
            "\nArms (the tasks filed with --arm; lead and review are the medians of the done):
  control      tasks 2 · done 2 · lead 9.0 h · review 5.0 h · bounces 1
  later        tasks 1 · done 0 · lead - · review - · bounces 0
  no-review    tasks 2 · done 1 · lead 3.0 h · review 1.0 h · bounces 2
{NO_DAY_FILES}"
        )),
        "{said}"
    );
    // The window's own figures count every task, the one on no arm too.
    assert!(
        said.starts_with("Stats: 6 top-level tasks made in the last 1 days;"),
        "{said}"
    );
    let json: Value =
        serde_json::from_str(&later.ok("", &["stats", "--days", "1", "--json"])).unwrap();
    let spread = |tasks: u64, hours: f64| json!({"tasks": tasks, "median": hours * H, "p75": hours * H, "p90": hours * H});
    assert_eq!(
        json["arms"]["no-review"],
        json!({
            "tasks": 2, "done": 1, "lead_seconds": spread(1, 3.0),
            "review_seconds": spread(1, 1.0), "bounces": 2,
        })
    );
    assert_eq!(
        json["arms"]["later"],
        json!({"tasks": 1, "done": 0, "lead_seconds": null, "review_seconds": null, "bounces": 0})
    );
    let control = &json["arms"]["control"];
    assert_eq!(
        (&control["tasks"], &control["done"], &control["bounces"]),
        (&json!(2), &json!(2), &json!(1))
    );
    assert_eq!(control["lead_seconds"]["median"], json!(9.0 * H));
    assert_eq!(control["review_seconds"]["median"], json!(5.0 * H));
    let names: Vec<&String> = json["arms"].as_object().unwrap().keys().collect();
    assert_eq!(names, ["control", "later", "no-review"]);
}

#[test]
fn a_board_with_no_arm_says_nothing_of_arms() {
    let later = at(&pipeline(), START, 48.0);
    assert!(!later.ok("", &["stats", "--days", "3"]).contains("Arms"));
    let json: Value =
        serde_json::from_str(&later.ok("", &["stats", "--days", "3", "--json"])).unwrap();
    assert_eq!(json["arms"], json!({}));
}

#[test]
fn a_task_shows_the_arm_it_was_filed_on_and_keeps_it() {
    let c = experiment();
    let shown = c.ok("", &["show", "exp/a1"]);
    assert!(shown.contains("\n  arm: control\n  Stages: "), "{shown}");
    assert!(!c.ok("", &["show", "exp/plain"]).contains("arm:"));
    let store = c.store();
    assert_eq!(store.get("exp/b2").unwrap().arm(), Some("no-review"));
    assert_eq!(store.get("exp/plain").unwrap().arm(), None);
    // The window's read carries it, in the order the tasks were made.
    let arms: Vec<Option<String>> = (store.stage_records(START).unwrap().into_iter())
        .map(|record| record.arm)
        .collect();
    let arm = |name: &str| Some(name.to_string());
    assert_eq!(
        arms,
        [
            arm("control"),
            arm("no-review"),
            arm("control"),
            arm("no-review"),
            arm("later"),
            None
        ]
    );
}

#[test]
fn an_arm_that_is_no_plain_word_files_nothing() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    for arm in ["", "Control", "two words", "-x", &"x".repeat(33)] {
        let ran = c.run_as(
            &orch,
            &["create", "exp/bad", "Work", &format!("--arm={arm}")],
            "",
        );
        assert_eq!(ran.code, 1, "{arm}: {}", ran.err);
        assert!(
            ran.err.contains("an arm is one word of lowercase letters"),
            "{}",
            ran.err
        );
    }
    assert!(c.store().get("exp/bad").is_err());
    c.ok(&orch, &["create", "exp/good", "Work", "--arm", "b2"]);
    assert_eq!(c.store().get("exp/good").unwrap().arm(), Some("b2"));
}

/// `list --json --all` on a board of `tasks` tasks, through a relay that keeps each request: what
/// it printed and the requests it sent.
fn listed_measured(tasks: usize) -> (String, Vec<proxy::Request>) {
    let c = seeded(tasks);
    let relay = proxy::Proxy::to(&c.env.vars["TASKS_SERVER"]);
    let mut env = c.env.clone();
    env.vars.insert("TASKS_SERVER".into(), relay.addr.clone());
    let through = Case { env };
    let said = through.ok("", &["list", "--json", "--all"]);
    (said, relay.take())
}

#[test]
fn list_reads_when_tasks_took_their_status_in_one_request_however_many_it_lists() {
    let (few, few_requests) = listed_measured(3);
    let (many, many_requests) = listed_measured(300);
    // Each seeded task was approved three and a half hours after it was made.
    let since = |listed: &str| -> Vec<Value> { since_by_slug(listed).into_values().collect() };
    assert_eq!(since(&few), vec![json!(NOW + 3.5 * H); 3]);
    assert_eq!(since(&many), vec![json!(NOW + 3.5 * H); 300]);
    let reads = |requests: &[proxy::Request]| {
        (requests.iter())
            .filter(|r| r.sql.contains("WHERE kind IN ['system', 'verdict']"))
            .count()
    };
    assert_eq!(reads(&few_requests), 1);
    assert_eq!(reads(&many_requests), 1);
    assert_eq!(few_requests.len(), many_requests.len());
}

/// What `beat` hands the board: `n` of each kind of `model`, `n * 1000` read from the cache.
fn spent(model: &str, n: u64) -> symphony_board::usage::Spent {
    symphony_board::usage::Spent {
        model: model.into(),
        speed: "standard".into(),
        tokens: symphony_board::usage::Tokens {
            input: n,
            cache_write_5m: n,
            cache_write_1h: n,
            cache_read: n * 1000,
            output: n,
            calls: n,
        },
    }
}

/// The acceptance's fixture: `sim/jump` worked by an executor on a priced model and one the price
/// table lacks (round 1), reviewed (round 2) and landed, the day's roll-up run before the first
/// token and after the landing as the shared server runs it. `show --json`, the task page's route
/// (its wire type), `stats --json` and its text then say the same tokens and estimated cost.
#[test]
fn show_the_task_route_and_stats_say_the_same_tokens_and_cost() {
    let c = Case::new();
    let d = day_start(NOW);
    let t = |hours: f64| at(&c, d, hours);
    let folder = std::env::temp_dir().join(format!(
        "symphony-stats-tokens-{}-{}",
        std::process::id(),
        c.env.vars["TASKS_DATABASE"]
    ));
    let roll = |hours: f64| {
        let now = d + hours * H;
        let read = t(hours).store().report_records(d).unwrap();
        let before = symphony_board::reports::read(&folder, &symphony_api::run_day(d)).unwrap();
        let gained = (before.as_ref())
            .and_then(|file| Some((file.usage.as_ref()?, file.read)))
            .map(|(base, from)| {
                symphony_board::reports::spend::gained(base, from, &read.usage, now)
            })
            .unwrap_or_default();
        let mut day = report_day("sim", d, now, now, &read);
        symphony_board::reports::spend::fill(&mut day, &read.usage, before.as_ref(), &gained);
        symphony_board::reports::write(&folder, &day).unwrap();
    };
    let (orch, exe, rev) = (
        t(0.5).register("orchestrator"),
        t(0.5).register("worker"),
        t(0.5).register("reviewer"),
    );
    t(0.5).ok(&orch, &["create", "sim/jump", "Jump"]);
    t(0.5).ok(&exe, &["claim", "sim/jump"]);
    roll(0.75);
    let store = t(1.0).store();
    assert!(store.bind_agent("exec", &exe, false).unwrap());
    assert!(store.bind_agent("rev", &rev, false).unwrap());
    let beat = |hours: f64, key: &str, spent: &[symphony_board::usage::Spent]| {
        (t(hours).store())
            .agent_beat_spending(key, &symphony_board::Beat::default(), None, None, spent)
            .unwrap()
    };
    beat(
        1.0,
        "exec",
        &[spent("claude-opus-5-5", 3), spent("gpt-x", 2)],
    );
    t(2.0).ok(&exe, &["move", "sim/jump", "review"]);
    t(2.0).ok(&rev, &["role", "sim/jump", "reviewer"]);
    beat(2.5, "rev", &[spent("claude-opus-5-5", 1)]);
    t(3.0).ok(&rev, &["verdict", "sim/jump", "approve", "fine"]);
    (t(3.5).store())
        .landed(
            "sim/jump",
            &orch,
            "landed sim/jump on main at 3e0c661",
            false,
            &["done"],
        )
        .unwrap();
    roll(4.0);
    let mut shown: Value =
        serde_json::from_str(&t(5.0).ok("", &["show", "sim/jump", "--json"])).unwrap();
    let mut tokens = shown["tokens"].take();
    assert_eq!(tokens["by_role"].as_array().unwrap().len(), 2);
    assert_eq!(tokens["by_round"].as_array().unwrap().len(), 2);
    assert_eq!(tokens["cost"]["unpriced"][0]["model"], "gpt-x");
    tokens.as_object_mut().unwrap().remove("rows");
    // The task page's route answers this, read into its wire type and written back the same.
    let wire: symphony_api::TaskTokens = serde_json::from_value(tokens.clone()).unwrap();
    assert_eq!(serde_json::to_value(&wire).unwrap(), tokens);
    let mut with_files = t(5.0);
    with_files.env.vars.insert(
        "SYMPHONY_REPORTS".into(),
        folder.to_string_lossy().into_owned(),
    );
    let stats: Value =
        serde_json::from_str(&with_files.ok("", &["stats", "--days", "1", "--json"])).unwrap();
    let told = &stats["tokens"];
    assert_eq!(told["prices"], tokens["cost"]["prices"]);
    let today = &told["per_day"][0];
    assert_eq!(today["day"], symphony_api::run_day(d).as_str());
    assert_eq!(
        today["by_role"], tokens["by_role"],
        "the day's tokens by role, from the hours the roll-ups told, are the task's"
    );
    let landed = &told["landed"];
    assert_eq!(
        (landed["tasks"].as_u64(), landed["without"].as_u64()),
        (Some(1), Some(0))
    );
    let dearest = &landed["dearest"][0];
    assert_eq!(dearest["slug"], "sim/jump");
    assert_eq!(
        (&dearest["tokens"], &dearest["cost"]),
        (&tokens["total"], &tokens["cost"])
    );
    assert_eq!(landed["median_dollars"], tokens["cost"]["dollars"]);
    let round = |n: usize| &tokens["by_round"][n];
    assert_eq!(
        (
            &landed["first_round"]["tokens"],
            &landed["first_round"]["cost"]
        ),
        (&round(0)["tokens"], &round(0)["cost"])
    );
    assert_eq!(
        (
            &landed["later_rounds"]["tokens"],
            &landed["later_rounds"]["cost"]
        ),
        (&round(1)["tokens"], &round(1)["cost"])
    );
    assert_eq!(landed["again"], 1);
    // The text says each figure is an estimate, with the day of the prices, and what had none.
    let text = with_files.ok("", &["stats", "--days", "1"]);
    let prices = tokens["cost"]["prices"].as_str().unwrap();
    for said in [
        format!("estimate at API list prices read {prices}, never money spent"),
        "An hour's tokens are what the board's usage gained between two roll-ups, shared among \
         the hours between them by time"
            .to_string(),
        "no price for gpt-x 2k tokens, in no figure".to_string(),
        "Cost per landed task: 1 landed with tokens recorded, median".to_string(),
        "1 of 1 went past their first round".to_string(),
    ] {
        assert!(text.contains(&said), "{said:?} not in {text}");
    }
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn stats_without_day_files_says_so_and_prints_the_rest() {
    let c = pipeline();
    let mut empty = at(&c, START, 20.0);
    let folder = std::env::temp_dir().join(format!("symphony-stats-none-{}", std::process::id()));
    empty.env.vars.insert(
        "SYMPHONY_REPORTS".into(),
        folder.to_string_lossy().into_owned(),
    );
    let text = empty.ok("", &["stats"]);
    assert!(text.contains("Done per day"), "{text}");
    assert!(text.contains("Tokens: no day file in these days"), "{text}");
    let json: Value = serde_json::from_str(&empty.ok("", &["stats", "--json"])).unwrap();
    assert_eq!(json["tokens"]["per_day"], json!([]));
    assert_eq!(json["tokens"]["landed"]["median_dollars"], Value::Null);
    // A board TASKS_SERVER names, with no folder named, keeps none: said, never an error.
    let unnamed = at(&c, START, 20.0).ok("", &["stats"]);
    assert!(unnamed.contains("Tokens: no day file read ("), "{unnamed}");
}
