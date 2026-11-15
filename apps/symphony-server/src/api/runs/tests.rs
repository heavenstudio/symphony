//! The Runs page's reads over fixture day files: the asks read from the query; the list newest
//! first, each run once across the days its lines are in, its steps left out, its filters and its
//! pages, reading no further back than a full page needs; what is not read named; a run opened on
//! its steps in order; and the routes refusing a bad ask before any hub and answering through one.

use super::*;
use crate::testing::Tree;
use std::sync::Arc;
use symphony_api::{FailedTest, RUN_SCHEMA};

/// 2026-11-15, 00:00 UTC.
const TODAY: f64 = 1_794_700_800.0;
const NOW: f64 = TODAY + 12.0 * 3_600.0;

/// A run's id, 32 hex digits, from a short name.
fn id(name: &str) -> String {
    format!("{name:0>32}")
}

/// A start line of `name`, asked for at `queued`, of `pipeline`.
fn start(name: &str, pipeline: &str, queued: f64) -> RunLine {
    RunLine {
        schema: RUN_SCHEMA,
        run: id(name),
        parent: None,
        project: Some("symphony".into()),
        machine: None,
        pipeline: pipeline.into(),
        step: None,
        label: format!("just {pipeline}"),
        task: None,
        handle: None,
        trigger: "owner".into(),
        branch: Some("main".into()),
        commit: None,
        tree: None,
        dirty: Some(false),
        queued,
        started: Some(queued + 1.0),
        ended: None,
        end: None,
        exit: None,
        signal: None,
        cpu_s: None,
        cpu_partial: None,
        rerun_of: None,
        log: None,
        log_bytes: None,
        suites: None,
        failed: None,
    }
}

/// `line` as its end line, ended `after` seconds after it started, as `end`.
fn ended(line: &RunLine, end: End, after: f64) -> RunLine {
    RunLine {
        ended: Some(line.started.unwrap() + after),
        end: Some(end),
        exit: Some(if end == End::Green { 0 } else { 1 }),
        cpu_s: Some(after / 2.0),
        ..line.clone()
    }
}

/// `line` as a step `name` of the run or step `parent`.
fn step_of(line: RunLine, parent: &str, name: &str) -> RunLine {
    RunLine {
        parent: Some(id(parent)),
        step: Some(name.into()),
        label: name.into(),
        ..line
    }
}

/// The day file of the UTC day starting at `day` in `runs`, holding `lines` and then `tail`.
fn write_day(runs: &Path, day: f64, lines: &[RunLine], tail: &str) {
    std::fs::create_dir_all(runs).unwrap();
    let mut text: String = lines
        .iter()
        .map(|line| serde_json::to_string(line).unwrap() + "\n")
        .collect();
    text.push_str(tail);
    std::fs::write(runs.join(format!("{}.jsonl", run_day(day))), text).unwrap();
}

fn names(page: &RunsPage) -> Vec<String> {
    page.runs
        .iter()
        .map(|run| run.run.trim_start_matches('0').to_string())
        .collect()
}

#[test]
fn an_ask_names_each_filter_whole_and_refuses_what_none_could_match() {
    assert_eq!(
        asked(None, None, None, None, None).unwrap(),
        Filters::default()
    );
    assert_eq!(
        asked(Some(""), Some(""), Some(""), Some(""), Some("")).unwrap(),
        Filters::default(),
        "an empty filter is none"
    );
    assert_eq!(
        asked(
            Some("review-callback"),
            Some("timed_out"),
            Some("tools/runs-page"),
            Some("tools/runs-page"),
            Some("1794700800.5")
        )
        .unwrap(),
        Filters {
            pipeline: Some("review-callback".into()),
            end: Some(Some(End::TimedOut)),
            task: Some("tools/runs-page".into()),
            branch: Some("tools/runs-page".into()),
            before: Some(1_794_700_800.5),
        }
    );
    assert_eq!(
        asked(None, Some("started"), None, None, None).unwrap().end,
        Some(None),
        "a run with no end line yet"
    );
    assert_eq!(
        asked(Some("two words"), None, None, None, None).unwrap_err(),
        "pipeline is a pipeline's name, not \"two words\""
    );
    for end in ["Green", "running", "red "] {
        assert_eq!(
            asked(None, Some(end), None, None, None).unwrap_err(),
            format!(
                "end is green, red, timed_out, interrupted, incomplete or started, not {end:?}"
            )
        );
    }
    for before in ["soon", "NaN", "inf"] {
        assert_eq!(
            asked(None, None, None, None, Some(before)).unwrap_err(),
            format!("before is a time in seconds, not {before:?}")
        );
    }
}

#[test]
fn the_list_is_each_run_once_newest_first_without_its_steps() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    // Yesterday: a run that ended today, whose end line is in today's file, and one that ended.
    let overnight = start("a", "verify", TODAY - 600.0);
    let yesterday = start("b", "lint", TODAY - 3_600.0);
    write_day(
        &runs,
        TODAY - DAY,
        &[
            yesterday.clone(),
            overnight.clone(),
            ended(&yesterday, End::Green, 30.0),
        ],
        "",
    );
    let lost = start("c", "test", TODAY + 60.0);
    let red = start("d", "test", TODAY + 120.0);
    let step = step_of(start("e", "verify", TODAY + 30.0), "a", "tests");
    write_day(
        &runs,
        TODAY,
        &[
            lost.clone(),
            red.clone(),
            step.clone(),
            ended(&step, End::Green, 5.0),
            ended(&overnight, End::Red, 900.0),
            ended(&red, End::Red, 10.0),
            // A sweep's second end line of a run that had ended: the first stands.
            ended(&red, End::Incomplete, 20.0),
        ],
        // A line a writer that died cut short, then an empty one.
        "{\"schema\":1,\"run\":\"ff\n\n",
    );
    let page = page(&runs, &Filters::default(), NOW);
    assert_eq!(names(&page), ["d", "c", "a", "b"]);
    assert_eq!(page.runs[0].end, Some(End::Red));
    assert_eq!(page.runs[0].ended, red.started.map(|at| at + 10.0));
    assert_eq!(
        page.runs[1], lost,
        "a run with no end line is its start line"
    );
    assert_eq!(
        page.runs[2],
        ended(&overnight, End::Red, 900.0),
        "a run whose lines are in two days' files is its end line"
    );
    assert!(!page.more);
    assert_eq!(page.pipelines, ["lint", "test", "verify"]);
    assert_eq!(page.unread, Vec::<String>::new());
}

#[test]
fn the_filters_take_a_pipeline_an_end_a_task_and_a_branch_whole() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let mut lines = Vec::new();
    for (name, pipeline, task, branch, end) in [
        ("1", "test", Some("tools/a"), "tools/a", Some(End::Green)),
        ("2", "test", Some("tools/ab"), "tools/ab", Some(End::Red)),
        ("3", "lint", Some("tools/a"), "tools/a", Some(End::Red)),
        ("4", "test", None, "main", None),
        ("5", "client", Some("tools/a"), "main", Some(End::TimedOut)),
    ] {
        let mut line = start(name, pipeline, TODAY + name.parse::<f64>().unwrap() * 60.0);
        line.task = task.map(Into::into);
        line.branch = Some(branch.into());
        lines.push(line.clone());
        if let Some(end) = end {
            lines.push(ended(&line, end, 5.0));
        }
    }
    write_day(&runs, TODAY, &lines, "");
    let ask = |pipeline, end, task, branch| {
        names(&page(
            &runs,
            &asked(pipeline, end, task, branch, None).unwrap(),
            NOW,
        ))
    };
    assert_eq!(ask(Some("test"), None, None, None), ["4", "2", "1"]);
    assert_eq!(ask(None, Some("red"), None, None), ["3", "2"]);
    assert_eq!(ask(None, Some("started"), None, None), ["4"]);
    assert_eq!(ask(None, None, Some("tools/a"), None), ["5", "3", "1"]);
    assert_eq!(ask(None, None, None, Some("main")), ["5", "4"]);
    assert_eq!(
        ask(
            Some("test"),
            Some("red"),
            Some("tools/ab"),
            Some("tools/ab")
        ),
        ["2"]
    );
    assert_eq!(
        ask(Some("verify"), None, None, None),
        Vec::<String>::new(),
        "a pipeline no run names matches none"
    );
    let named = page(
        &runs,
        &asked(Some("verify"), None, None, None, None).unwrap(),
        NOW,
    );
    assert_eq!(
        named.pipelines,
        ["client", "lint", "test", "verify"],
        "the pipeline asked for is offered with those read"
    );
}

#[test]
fn a_full_page_reads_no_older_day_and_the_next_page_starts_before_its_last_run() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let count = u32::try_from(RUNS_PAGE).unwrap() + 5;
    let today: Vec<RunLine> = (0..count)
        .map(|n| {
            let line = start(&format!("1{n:03}"), "test", TODAY + 60.0 * f64::from(n));
            ended(&line, End::Green, 5.0)
        })
        .collect();
    write_day(&runs, TODAY, &today, "");
    // Yesterday's file is no regular file: read, it would be named as not read.
    std::fs::create_dir_all(runs.join(format!("{}.jsonl", run_day(TODAY - DAY)))).unwrap();
    let first = page(&runs, &Filters::default(), NOW);
    assert_eq!(first.runs.len(), RUNS_PAGE);
    assert!(first.more);
    assert_eq!(
        first.unread,
        Vec::<String>::new(),
        "a page today's file fills reads no other"
    );
    assert_eq!(first.runs[0].run, id(&format!("1{:03}", RUNS_PAGE + 4)));
    let before = first.runs.last().unwrap().queued;
    let next = page(
        &runs,
        &Filters {
            before: Some(before),
            ..Filters::default()
        },
        NOW,
    );
    assert_eq!(names(&next), ["1004", "1003", "1002", "1001", "1000"]);
    assert!(!next.more);
    assert_eq!(
        next.unread.len(),
        1,
        "the last page reads back to the window's start"
    );
    assert!(
        next.unread[0].ends_with("not a regular file"),
        "{:?}",
        next.unread
    );
}

#[test]
fn a_day_file_not_read_is_named_and_days_past_the_window_are_never_read() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let outside = tree.path("outside.jsonl");
    let old = start("9", "test", TODAY - 10.0 * DAY);
    write_day(&runs, TODAY, &[start("1", "test", TODAY + 5.0)], "");
    std::fs::write(&outside, serde_json::to_string(&old).unwrap() + "\n").unwrap();
    // A link is not followed out of the day files' folder.
    std::os::unix::fs::symlink(
        &outside,
        runs.join(format!("{}.jsonl", run_day(TODAY - DAY))),
    )
    .unwrap();
    write_day(&runs, TODAY - 10.0 * DAY, std::slice::from_ref(&old), "");
    write_day(
        &runs,
        TODAY - f64::from(RUNS_DAYS) * DAY,
        &[start("8", "test", TODAY - f64::from(RUNS_DAYS) * DAY)],
        "",
    );
    let page = page(&runs, &Filters::default(), NOW);
    assert_eq!(
        names(&page),
        ["1", "9"],
        "the fifteenth day back is past the window"
    );
    assert_eq!(page.unread.len(), 1, "{:?}", page.unread);
    assert!(
        page.unread[0].starts_with(&format!("{}", runs.join("2026-11-14.jsonl").display())),
        "{:?}",
        page.unread
    );
    // A folder with no day files has no runs.
    let empty = super::page(&tree.path("none"), &Filters::default(), NOW);
    assert_eq!(empty, RunsPage::default());
}

#[test]
fn a_run_opens_on_its_steps_each_after_the_one_it_ran_inside() {
    let tree = Tree::new();
    let runs = tree.path("runs");
    let verify = start("a", "verify", TODAY - 300.0);
    let lint = step_of(start("b", "verify", TODAY - 200.0), "a", "lint");
    write_day(&runs, TODAY - DAY, &[verify.clone(), lint.clone()], "");
    let client_lint = step_of(start("c", "verify", TODAY + 10.0), "b", "client-lint");
    let tests = step_of(start("d", "verify", TODAY + 20.0), "a", "tests");
    let failed = |name: &str| FailedTest {
        suite: Some("symphony-server".into()),
        name: name.into(),
        line: "assertion failed".into(),
    };
    let red_tests = RunLine {
        failed: Some(vec![failed("one"), failed("two")]),
        ..ended(&tests, End::Red, 60.0)
    };
    let other = step_of(start("e", "test", TODAY + 15.0), "f", "tests");
    write_day(
        &runs,
        TODAY,
        &[
            client_lint.clone(),
            tests.clone(),
            other,
            ended(&client_lint, End::Green, 3.0),
            ended(&lint, End::Green, 300.0),
            red_tests.clone(),
        ],
        "",
    );
    let opened = run(&runs, &id("a"), NOW).unwrap();
    assert_eq!(opened.run, verify, "a run still running is its start line");
    assert_eq!(
        opened
            .steps
            .iter()
            .map(|step| step.step.clone().unwrap())
            .collect::<Vec<_>>(),
        ["lint", "client-lint", "tests"],
        "a step's own steps come before its next sibling"
    );
    assert_eq!(opened.steps[0], ended(&lint, End::Green, 300.0));
    assert_eq!(
        opened.steps[2], red_tests,
        "a step that failed two tests names them"
    );
    // A step opens on its own steps, its parent left out.
    let step = run(&runs, &id("b"), NOW).unwrap();
    assert_eq!(step.run.parent, Some(id("a")));
    assert_eq!(step.steps.len(), 1);
    assert_eq!(
        run(&runs, &id("7"), NOW).unwrap_err(),
        format!("no run {} in the last 14 days' files", id("7"))
    );
}

#[test]
fn a_loop_of_parents_takes_each_step_once() {
    let one = step_of(start("b", "verify", TODAY), "c", "one");
    let two = step_of(start("c", "verify", TODAY + 1.0), "b", "two");
    let top = step_of(start("d", "verify", TODAY + 2.0), "a", "top");
    let steps = [one.clone(), two.clone(), top.clone()];
    assert_eq!(under(&id("a"), &steps), [top]);
    assert_eq!(
        under(&id("b"), &steps),
        [two],
        "the step it started from is not met again"
    );
    assert_eq!(under(&id("c"), &steps), [one]);
}

#[tokio::test]
async fn the_routes_refuse_a_bad_ask_before_any_hub_and_answer_through_one() {
    use axum::http::StatusCode;
    let tree = Tree::new();
    let root = crate::testing::repository(&tree);
    let shared = crate::front::Shared::new(
        1,
        1234,
        &root,
        None,
        crate::gate::Gate::new(Arc::new(|_| Ok(None)), Arc::new(std::time::Instant::now)),
    );
    let ask = |uri: String| {
        let shared = shared.clone();
        let request = axum::http::Request::builder()
            .uri(uri)
            .header("Host", "localhost:1234")
            .body(())
            .unwrap();
        async move {
            let response = crate::api::answer(&shared, request).await.unwrap();
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            (
                status,
                serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            )
        }
    };
    let (status, said) = ask("/api/runs?end=running".into()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{said}");
    let (status, said) = ask("/api/run".into()).await;
    assert_eq!(
        (status, said),
        (
            StatusCode::BAD_REQUEST,
            serde_json::json!({"error": "run is required: a run's id"})
        )
    );
    let (status, _) = ask("/api/runs".into()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "no hub yet");

    let runs = tree.path("runs");
    let red = start("d", "test", TODAY + 120.0);
    write_day(
        &runs,
        TODAY,
        &[red.clone(), ended(&red, End::Red, 10.0)],
        "",
    );
    let mut hub = crate::live::Hub::new(&root, &root.join("build/client"));
    hub.clock = Box::new(|| NOW);
    let none = || Err("not this test's".to_string());
    hub.report_folders
        .set(super::super::reports::Folders {
            reports: none(),
            agents: none(),
            health: none(),
            runs: Some(runs.clone()),
        })
        .unwrap();
    assert!(shared.api.set(Arc::new(hub)).is_ok());
    let (status, said) = ask("/api/runs?pipeline=test&end=red".into()).await;
    assert_eq!(status, StatusCode::OK);
    let page: RunsPage = serde_json::from_value(said).unwrap();
    assert_eq!(page.runs, [ended(&red, End::Red, 10.0)]);
    let (status, said) = ask("/api/runs?pipeline=lint".into()).await;
    assert_eq!(
        (status, said["runs"].clone()),
        (StatusCode::OK, serde_json::json!([]))
    );
    let (status, said) = ask(format!("/api/run?run={}", id("d"))).await;
    assert_eq!(status, StatusCode::OK);
    let opened: RunPage = serde_json::from_value(said).unwrap();
    assert_eq!(opened.run.end, Some(End::Red));
    let (status, said) = ask("/api/run?run=..%2F..%2Fconfig".into()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{said}");
    let (status, said) = ask(format!("/api/run?run={}", id("e"))).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{said}");
}

#[tokio::test]
async fn a_project_that_keeps_no_runs_where_the_server_reads_says_so() {
    let tree = Tree::new();
    let root = crate::testing::repository(&tree);
    let hub = crate::live::Hub::new(&root, &root.join("build/client"));
    let none = || Err("not this test's".to_string());
    hub.report_folders
        .set(super::super::reports::Folders {
            reports: none(),
            agents: none(),
            health: none(),
            runs: None,
        })
        .unwrap();
    let said = (
        StatusCode::NOT_FOUND,
        "this project keeps no runs where this server reads".to_string(),
    );
    assert_eq!(answer(&hub, &Filters::default()).unwrap_err(), said);
    assert_eq!(answer_run(&hub, &id("a")).unwrap_err(), said);
    assert_eq!(
        answer_run(&hub, "a").unwrap_err().0,
        StatusCode::BAD_REQUEST
    );
}
