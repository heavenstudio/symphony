//! `show`'s stages and `stats`, on a board whose clock each command sets: every expected time is
//! written out here by hand.

use crate::Case;
use crate::common::{NOW, Seed, task};
use serde_json::{Value, json};
use std::time::Instant;
use symphony_board::rows::{MessageRow, TaskRow};
use symphony_board::sys::local_minute;

#[path = "../perf/proxy.rs"]
#[allow(dead_code)]
mod proxy;

const H: f64 = 3600.0;
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
    t(6.0).ok(&exe, &["move", "sim/c", "blocked"]);
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
Done per day (every top-level task closed in the last 3 days, whenever it was made):
{}Longest in their status:
  sim/d                        ready       48.0 h
  sim/e                        backlog     48.0 h
  sim/c                        blocked     42.0 h
Blocked: 1, median 42.0 h there
Ready with no executor: 1, median 48.0 h there
",
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
    assert_eq!(said.len(), 7, "{said:?}");
    assert_eq!(said[0].0, "system");
    assert!(said[0].1.starts_with("created by "), "{said:?}");
    // A verdict is cut to the word that names it.
    assert_eq!(said[4], ("verdict", "changes: a test "));
    assert_eq!(said[6], ("verdict", "approve"));
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
            "Stats: 0 top-level tasks made in the last 1 days; every figure is theirs but done per day\nDone 0\nDone per day (every top-level task closed in the last 1 days, whenever it was made):\n  {before}  0\n  {today}  0\nBlocked: 0\nReady with no executor: 0\n"
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
