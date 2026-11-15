//! The runs' roll-up over fixture day files of three days, every figure counted here by hand from
//! the lines: two pipelines and a lint, three suites, a red and a green on the same tree, a rerun,
//! a tree that changed, a dirty run, a verify whose step reports its tests, two reds on one tree
//! that each pass the other's test, eleven tasks, and lines that do not parse.

use super::*;
use symphony_api::reports::Spread;
use symphony_api::{FailedTest, RUN_SCHEMA, Suite};

/// A UTC midnight: the first of the three days.
const D0: f64 = 1_799_971_200.0;
const H: f64 = 3_600.0;

/// `hours` into day `day` (0, 1 or 2).
fn at(day: u8, hours: f64) -> f64 {
    D0 + f64::from(day) * DAY + hours * H
}

/// The clock of every case: 10:00 on the third day.
fn now() -> f64 {
    at(2, 10.0)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "symphony-runs-rollup-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A run line being made: a clean green run of `pipeline` labelled `label`, that started at
/// `started` and ran `took` seconds for 1 s of CPU.
struct L(RunLine);

fn run(id: &str, pipeline: &str, label: &str, started: f64, took: f64) -> L {
    L(RunLine {
        schema: RUN_SCHEMA,
        run: id.into(),
        parent: None,
        project: Some("symphony".into()),
        machine: None,
        pipeline: pipeline.into(),
        step: None,
        label: label.into(),
        task: None,
        handle: None,
        trigger: "owner".into(),
        branch: None,
        commit: None,
        tree: None,
        dirty: Some(false),
        queued: started,
        started: Some(started),
        ended: Some(started + took),
        end: Some(End::Green),
        exit: Some(0),
        signal: None,
        cpu_s: Some(1.0),
        cpu_partial: None,
        rerun_of: None,
        log: None,
        log_bytes: None,
        suites: None,
        failed: None,
    })
}

impl L {
    fn end(mut self, end: End) -> L {
        self.0.end = Some(end);
        self
    }
    fn tree(mut self, tree: &str) -> L {
        self.0.tree = Some(tree.into());
        self
    }
    fn dirty(mut self) -> L {
        self.0.dirty = Some(true);
        self
    }
    fn task(mut self, task: &str) -> L {
        self.0.task = Some(task.into());
        self
    }
    fn cpu(mut self, cpu: Option<f64>) -> L {
        self.0.cpu_s = cpu;
        self
    }
    fn step_of(mut self, parent: &str, step: &str) -> L {
        self.0.parent = Some(parent.into());
        self.0.step = Some(step.into());
        self
    }
    fn never_started(mut self) -> L {
        self.0.started = None;
        self.0.cpu_s = None;
        self.0.end = Some(End::Incomplete);
        self
    }
    fn started_only(mut self) -> L {
        self.0.end = None;
        self.0.ended = None;
        self.0.exit = None;
        self.0.cpu_s = None;
        self
    }
    /// Its suites as (name, tests failed, seconds), and its failures as (suite, test): the
    /// fields its log's tests fill (`crate::runs::suites`).
    fn tests(mut self, suites: &[(&str, u32, f64)], failed: &[(Option<&str>, &str)]) -> L {
        self.0.suites = Some(
            suites
                .iter()
                .map(|(name, failed, s)| Suite {
                    name: (*name).into(),
                    passed: 10,
                    failed: *failed,
                    ignored: 0,
                    s: *s,
                    error: false,
                })
                .collect(),
        );
        self.0.failed = Some(
            failed
                .iter()
                .map(|(suite, name)| FailedTest {
                    suite: suite.map(str::to_string),
                    name: (*name).into(),
                    line: "assertion failed".into(),
                })
                .collect(),
        );
        self
    }
}

fn spread(n: u32, median: f64, p90: f64) -> Spread {
    Spread { n, median, p90 }
}

fn spent(runs: u32, cpu_s: f64, wall_s: f64, cpu_partial: bool) -> Cost {
    Cost {
        runs,
        cpu_s,
        wall_s,
        cpu_partial,
    }
}

fn pipeline(
    name: &str,
    ends: [u32; 5],
    pass_rate: Option<f64>,
    duration: Option<Spread>,
) -> PipelineDay {
    let [green, red, timed_out, interrupted, incomplete] = ends;
    PipelineDay {
        pipeline: name.into(),
        runs: ends.iter().sum(),
        green,
        red,
        timed_out,
        interrupted,
        incomplete,
        pass_rate,
        duration,
    }
}

fn suite(name: &str, runs: u32, failed: u32, time: Spread) -> SuiteDay {
    SuiteDay {
        name: name.into(),
        runs,
        failed,
        pass_rate: f64::from(runs - failed) / f64::from(runs),
        time,
    }
}

fn flaky(suite: Option<&str>, name: &str, flaky: u32, red: u32) -> FlakyTest {
    FlakyTest {
        suite: suite.map(str::to_string),
        name: name.into(),
        flaky,
        red,
    }
}

fn task(name: &str, cost: Cost) -> TaskCost {
    TaskCost {
        task: name.into(),
        cost,
    }
}

const STORE: Option<&str> = Some("store");

/// The three days' lines, as the recorder would have appended them.
fn lines() -> Vec<RunLine> {
    let test = "just test-rs";
    [
        // Day 0. A red and its rerun, green, on the same tree; then a red whose fix changed the tree.
        run("a1", "test", test, at(0, 1.0), 100.0)
            .end(End::Red)
            .tree("T1")
            .task("alpha")
            .cpu(Some(50.0))
            .tests(
                &[("store", 1, 40.0), ("reports", 0, 10.0)],
                &[(STORE, "store::flaky")],
            ),
        run("a2", "test", test, at(0, 2.0), 80.0)
            .tree("T1")
            .task("alpha")
            .cpu(Some(40.0))
            .tests(&[("store", 0, 38.0), ("reports", 0, 12.0)], &[]),
        run("a3", "test", test, at(0, 3.0), 60.0)
            .end(End::Red)
            .tree("T2")
            .task("beta")
            .cpu(Some(30.0))
            .tests(
                &[("reports", 1, 9.0)],
                &[(Some("reports"), "reports::changed")],
            ),
        run("a4", "test", test, at(0, 4.0), 60.0)
            .tree("T3")
            .task("beta")
            .cpu(Some(30.0))
            .tests(&[("reports", 0, 11.0)], &[]),
        // Day 1. A verify whose step reports its tests, red, then the same tree dirty and green; a
        // test run timed out; a lint for no task.
        run("v1", "verify", "just verify", at(1, 1.0), 600.0)
            .end(End::Red)
            .tree("T4")
            .task("gamma")
            .cpu(Some(1000.0))
            .tests(
                &[("api", 1, 5.0), ("store", 0, 41.0)],
                &[(Some("api"), "api::dirty")],
            ),
        run("v1s", "verify", "tests", at(1, 1.1), 400.0)
            .step_of("v1", "tests")
            .end(End::Red)
            .tree("T4")
            .task("gamma")
            .cpu(Some(900.0))
            .tests(
                &[("api", 1, 5.0), ("store", 0, 41.0)],
                &[(Some("api"), "api::dirty")],
            ),
        run("v2", "verify", "just verify", at(1, 2.0), 500.0)
            .tree("T4")
            .dirty()
            .task("gamma")
            .cpu(Some(800.0))
            .tests(&[("api", 0, 5.5), ("store", 0, 40.0)], &[]),
        run("v2s", "verify", "tests", at(1, 2.1), 300.0)
            .step_of("v2", "tests")
            .tree("T4")
            .dirty()
            .task("gamma")
            .cpu(Some(700.0))
            .tests(&[("api", 0, 5.5), ("store", 0, 40.0)], &[]),
        run("t1", "test", "just test-rs -p x", at(1, 3.0), 3600.0)
            .end(End::TimedOut)
            .task("delta")
            .cpu(Some(3000.0)),
        run("l1", "lint", "just lint-rs", at(1, 4.0), 120.0).cpu(Some(200.0)),
        // Day 2, under way. Two reds on one tree, each passing the other's test; two reds on one
        // tree whose failures name no suite; the flaky test again on another tree; an interrupted
        // run whose CPU is not known, one that never started, one still running; and lints, so
        // that eleven tasks ran.
        run("b1", "test", test, at(2, 1.0), 50.0)
            .end(End::Red)
            .tree("T5")
            .task("t01")
            .cpu(Some(20.0))
            .tests(&[("store", 1, 39.0)], &[(STORE, "store::one")]),
        run("b2", "test", test, at(2, 1.5), 50.0)
            .end(End::Red)
            .tree("T5")
            .task("t02")
            .cpu(Some(21.0))
            .tests(&[("store", 1, 39.5)], &[(STORE, "store::two")]),
        run("c1", "test", "just test-rs -p y", at(2, 2.0), 50.0)
            .end(End::Red)
            .tree("T6")
            .task("t03")
            .cpu(Some(22.0)),
        run("c2", "test", "just test-rs -p y", at(2, 2.5), 50.0)
            .end(End::Red)
            .tree("T6")
            .task("t04")
            .cpu(Some(23.0)),
        run("d1", "test", test, at(2, 3.0), 50.0)
            .end(End::Red)
            .tree("T7")
            .task("t05")
            .cpu(Some(24.0))
            .tests(&[("store", 1, 40.0)], &[(STORE, "store::flaky")]),
        run("d2", "test", test, at(2, 3.5), 50.0)
            .tree("T7")
            .task("t06")
            .cpu(Some(25.0))
            .tests(&[("store", 0, 40.0)], &[]),
        run("e1", "test", "just test-rs -p z", at(2, 4.0), 10.0)
            .end(End::Interrupted)
            .task("t07")
            .cpu(None),
        run("f1", "test", "just test-rs -p w", at(2, 5.0), 0.0)
            .never_started()
            .task("t08")
            .cpu(None),
        run("g1", "verify", "just verify", at(2, 6.0), 0.0)
            .started_only()
            .task("t99"),
        run("x9", "lint", "just lint-rs", at(2, 7.0), 60.0)
            .task("t09")
            .cpu(Some(5.0)),
        run("x10", "lint", "just lint-rs", at(2, 7.5), 60.0)
            .task("t10")
            .cpu(Some(30.0)),
        run("x11", "lint", "just lint-rs", at(2, 8.0), 60.0)
            .task("t11")
            .cpu(Some(26.0)),
    ]
    .into_iter()
    .map(|line| line.0)
    .collect()
}

/// `lines` appended to their day files in `runs`, with c1's and c2's failures that name no suite,
/// and a line cut short, one that is no JSON and an empty one.
fn write_days(runs: &Path, lines: &[RunLine]) {
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for line in lines {
        let mut value = serde_json::to_value(line).unwrap();
        if matches!(line.run.as_str(), "c1" | "c2") {
            let name = if line.run == "c1" {
                "nameless"
            } else {
                "other"
            };
            value["failed"] = serde_json::json!([{"name": name, "line": "panicked"}]);
        }
        let file = files.entry(line.day()).or_default();
        file.push_str(&value.to_string());
        file.push('\n');
    }
    files
        .get_mut(&run_day(at(2, 0.0)))
        .unwrap()
        .push_str("{\"schema\":1,\"run\":\n\nnot json\n");
    for (day, text) in files {
        std::fs::write(runs.join(format!("{day}.jsonl")), text).unwrap();
    }
}

/// The days every case reads: the three of the fixture and the eleven before the first, none of
/// which has a file.
fn days() -> Vec<String> {
    (0..14)
        .map(|back| run_day(at(2, 0.0) - f64::from(back) * DAY))
        .rev()
        .collect()
}

/// `day` with every figure that is a fraction rounded to a millionth, so a sum or a quantile
/// compares as the hand count writes it.
fn rounded(mut day: RunsDay) -> RunsDay {
    let round = |x: &mut f64| *x = (*x * 1e6).round() / 1e6;
    for spread in day
        .pipelines
        .iter_mut()
        .filter_map(|p| p.duration.as_mut())
        .chain(day.suites.iter_mut().map(|s| &mut s.time))
    {
        round(&mut spread.median);
        round(&mut spread.p90);
    }
    for pipeline in &mut day.pipelines {
        if let Some(rate) = pipeline.pass_rate.as_mut() {
            round(rate);
        }
    }
    for suite in &mut day.suites {
        round(&mut suite.pass_rate);
    }
    for cost in std::iter::once(&mut day.cost).chain(day.tasks.iter_mut().map(|t| &mut t.cost)) {
        round(&mut cost.cpu_s);
        round(&mut cost.wall_s);
    }
    day
}

fn base(day: u8) -> RunsDay {
    let start = at(day, 0.0);
    let until = now().min(start + DAY);
    RunsDay {
        schema: RUNS_DAY_SCHEMA,
        project: "symphony".into(),
        day: run_day(start),
        start,
        read: now(),
        until,
        closed: until == start + DAY,
        ..RunsDay::default()
    }
}

#[test]
fn three_days_of_runs_give_every_figure_counted_by_hand() {
    let runs = scratch("figures");
    write_days(&runs, &lines());
    let read = read(&runs, &days()).unwrap();
    assert_eq!(
        read.len(),
        lines().len(),
        "every line read once, the broken ones passed over"
    );

    // Day 0: test runs of 100, 80, 60 and 60 s; the red on T1 green on its rerun, the red on T2
    // never green on T2, as its fix changed the tree.
    assert_eq!(
        rounded(day("symphony", at(0, 0.0), now(), &read)),
        rounded(RunsDay {
            pipelines: vec![pipeline(
                "test",
                [2, 2, 0, 0, 0],
                Some(0.5),
                Some(spread(4, 70.0, 94.0))
            )],
            suites: vec![
                suite("reports", 4, 1, spread(4, 10.5, 11.7)),
                suite("store", 2, 1, spread(2, 39.0, 39.8)),
            ],
            slowest: vec!["store".into(), "reports".into()],
            flaky: vec![flaky(STORE, "store::flaky", 1, 0)],
            cost: spent(4, 150.0, 300.0, false),
            tasks: vec![
                task("alpha", spent(2, 90.0, 180.0, false)),
                task("beta", spent(2, 60.0, 120.0, false))
            ],
            ..base(0)
        }),
    );

    // Day 1: the verify's suites counted once, from its steps; the dirty green passes nothing; the
    // steps cost nothing more than their runs.
    assert_eq!(
        rounded(day("symphony", at(1, 0.0), now(), &read)),
        rounded(RunsDay {
            pipelines: vec![
                pipeline(
                    "lint",
                    [1, 0, 0, 0, 0],
                    Some(1.0),
                    Some(spread(1, 120.0, 120.0))
                ),
                pipeline(
                    "test",
                    [0, 0, 1, 0, 0],
                    Some(0.0),
                    Some(spread(1, 3600.0, 3600.0))
                ),
                pipeline(
                    "verify",
                    [1, 1, 0, 0, 0],
                    Some(0.5),
                    Some(spread(2, 550.0, 590.0))
                ),
            ],
            suites: vec![
                suite("api", 2, 1, spread(2, 5.25, 5.45)),
                suite("store", 2, 0, spread(2, 40.5, 40.9)),
            ],
            slowest: vec!["store".into(), "api".into()],
            flaky: vec![flaky(STORE, "store::flaky", 1, 0)],
            cost: spent(4, 5000.0, 4820.0, false),
            tasks: vec![
                task("delta", spent(1, 3000.0, 3600.0, false)),
                task("gamma", spent(2, 1800.0, 1100.0, false)),
            ],
            ..base(1)
        }),
        "a day that has ended, its flaky tests looking back over its own days alone"
    );

    // Day 2, under way: eleven tasks, the ten costliest kept (t08's run never started, so it took
    // nothing and is the one left out); the flaky test twice, on two trees.
    let tasks = [
        ("t10", 30.0, 60.0),
        ("t11", 26.0, 60.0),
        ("t06", 25.0, 50.0),
        ("t05", 24.0, 50.0),
        ("t04", 23.0, 50.0),
        ("t03", 22.0, 50.0),
        ("t02", 21.0, 50.0),
        ("t01", 20.0, 50.0),
        ("t09", 5.0, 60.0),
        ("t07", 0.0, 10.0),
    ]
    .map(|(name, cpu, wall)| task(name, spent(1, cpu, wall, name == "t07")))
    .to_vec();
    assert_eq!(
        rounded(day("symphony", at(2, 0.0), now(), &read)),
        rounded(RunsDay {
            pipelines: vec![
                pipeline(
                    "lint",
                    [3, 0, 0, 0, 0],
                    Some(1.0),
                    Some(spread(3, 60.0, 60.0))
                ),
                pipeline(
                    "test",
                    [1, 5, 0, 1, 1],
                    Some(1.0 / 6.0),
                    Some(spread(7, 50.0, 50.0))
                ),
            ],
            suites: vec![suite("store", 4, 3, spread(4, 39.75, 40.0))],
            slowest: vec!["store".into()],
            flaky: vec![
                flaky(STORE, "store::flaky", 2, 0),
                flaky(STORE, "store::one", 1, 0),
                flaky(STORE, "store::two", 1, 0),
            ],
            cost: spent(11, 196.0, 490.0, true),
            tasks,
            ..base(2)
        }),
        "the running verify counts nowhere; the reds that name no suite pass nothing"
    );
}

#[test]
fn the_roll_up_writes_the_days_due_whole_and_a_day_file_it_cannot_read_writes_nothing() {
    let runs = scratch("write");
    write_days(&runs, &lines());
    let gate = Gate::default();
    let due = [at(1, 0.0), at(2, 0.0)];
    let written = roll_up(&runs, "symphony", now(), &due, &gate).unwrap();
    assert_eq!(written, vec![run_day(at(1, 0.0)), run_day(at(2, 0.0))]);
    let out = folder(&runs);
    assert_eq!(out, runs.join("rollup"));
    let file = |day: u8| std::fs::read(out.join(report_file(&run_day(at(day, 0.0))))).ok();
    let read = read(&runs, &days()).unwrap();
    for day_of in [1, 2] {
        let bytes = file(day_of).unwrap();
        assert_eq!(bytes.last(), Some(&b'\n'));
        let written: RunsDay = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(written, day("symphony", at(day_of, 0.0), now(), &read));
    }
    assert!(file(0).is_none(), "a day not due is not written");
    assert!(
        crate::reports::closed(&out, &run_day(at(1, 0.0))),
        "the day that ended is final, as the board's roll-up reads it"
    );
    assert!(!crate::reports::closed(&out, &run_day(at(2, 0.0))));
    let before = (file(1), file(2));

    // A day file that is a folder, or a link, is not read: nothing is written.
    let first = runs.join(format!("{}.jsonl", run_day(at(0, 0.0))));
    let aside = runs.join("aside");
    std::fs::rename(&first, &aside).unwrap();
    std::fs::create_dir(&first).unwrap();
    let later = now() + H;
    let why = roll_up(&runs, "symphony", later, &due, &gate).unwrap_err();
    assert!(why.ends_with("not a regular file"), "{why}");
    std::fs::remove_dir(&first).unwrap();
    std::os::unix::fs::symlink(&aside, &first).unwrap();
    let why = roll_up(&runs, "symphony", later, &due, &gate).unwrap_err();
    assert!(why.starts_with(&first.display().to_string()), "{why}");
    assert_eq!(
        (file(1), file(2)),
        before,
        "each roll-up as the last one left it"
    );

    std::fs::remove_file(&first).unwrap();
    std::fs::rename(&aside, &first).unwrap();
    roll_up(&runs, "symphony", later, &due, &gate).unwrap();
    assert_ne!(file(2), before.1, "read again, today's is written again");
}

#[test]
fn a_gate_held_or_closed_writes_nothing_and_no_day_file_is_no_runs() {
    let runs = scratch("gate");
    let gate = Gate::default();
    gate.hold();
    let due = [at(2, 0.0)];
    assert_eq!(roll_up(&runs, "symphony", now(), &due, &gate), Ok(vec![]));
    gate.close(now());
    assert_eq!(roll_up(&runs, "symphony", now(), &due, &gate), Ok(vec![]));
    assert!(
        !folder(&runs).exists(),
        "nothing written while the gate is not open"
    );
    assert_eq!(
        roll_up(&runs, "symphony", now(), &[], &Gate::default()),
        Ok(vec![])
    );

    let written = roll_up(&runs, "symphony", now(), &due, &Gate::default()).unwrap();
    assert_eq!(written, vec![run_day(at(2, 0.0))]);
    let day: RunsDay = serde_json::from_slice(
        &std::fs::read(folder(&runs).join(report_file(&written[0]))).unwrap(),
    )
    .unwrap();
    assert_eq!(
        day,
        base(2),
        "a project with no run lines: a day of no runs"
    );
}

#[test]
fn a_test_passes_in_a_green_run_or_a_red_one_that_reported_its_binary_without_it() {
    let failing = (STORE, "store::x");
    let green = run("g", "test", "t", at(0, 1.0), 1.0).0;
    assert!(passed(&green, failing), "green: every test of it passed");
    assert!(
        passed(&green, (None, "nameless")),
        "even one whose binary is not known"
    );
    let red = |suites: &[(&str, u32, f64)], failed: &[(Option<&str>, &str)]| {
        run("r", "test", "t", at(0, 1.0), 1.0)
            .end(End::Red)
            .tests(suites, failed)
            .0
    };
    assert!(passed(
        &red(&[("store", 1, 1.0)], &[(STORE, "store::y")]),
        failing
    ));
    assert!(
        !passed(&red(&[("store", 1, 1.0)], &[(STORE, "store::x")]), failing),
        "it failed there"
    );
    assert!(
        !passed(&red(&[("other", 1, 1.0)], &[(Some("other"), "y")]), failing),
        "its binary not reported: it may not have run"
    );
    assert!(
        !passed(
            &red(&[("store", 1, 1.0)], &[(STORE, "store::y")]),
            (None, "store::x")
        ),
        "a failure that names no binary passes only in a green run"
    );
    let mut unreported = red(&[], &[]);
    unreported.suites = None;
    unreported.failed = None;
    assert!(
        !passed(&unreported, failing),
        "a red that says nothing of its tests"
    );
    // A binary that ended with no result line (it crashed) ran its tests to no end of theirs.
    let mut crashed = red(&[("store", 0, 1.0)], &[]);
    crashed.suites.as_mut().unwrap()[0].error = true;
    assert!(!passed(&crashed, failing), "its binary crashed");
    assert_eq!(
        suites([&crashed].into_iter()),
        [SuiteDay {
            name: "store".into(),
            runs: 1,
            failed: 1,
            pass_rate: 0.0,
            time: spread(1, 1.0, 1.0),
        }],
        "a crashed suite counts as failed, though no test of it did"
    );
    for end in [End::TimedOut, End::Interrupted, End::Incomplete] {
        let stopped = run("s", "test", "t", at(0, 1.0), 1.0).end(end).0;
        assert!(!passed(&stopped, failing), "{end:?}: it may not have run");
    }
}

#[test]
fn a_pipeline_of_stopped_runs_has_no_pass_rate_and_one_never_started_no_duration() {
    let stopped = [
        run("i", "verify", "v", at(0, 1.0), 5.0)
            .end(End::Interrupted)
            .0,
        run("n", "verify", "v", at(0, 2.0), 0.0).never_started().0,
    ];
    let refs: Vec<&RunLine> = stopped.iter().collect();
    assert_eq!(
        pipelines(&refs),
        vec![pipeline(
            "verify",
            [0, 0, 0, 1, 1],
            None,
            Some(spread(1, 5.0, 5.0))
        )]
    );
    assert_eq!(pipelines(&refs[1..])[0].duration, None);
    let mut short = run("p", "verify", "v", at(0, 3.0), 5.0).0;
    short.cpu_partial = Some(true);
    assert_eq!(
        cost([&short].into_iter()),
        spent(1, 1.0, 5.0, true),
        "known short"
    );
    assert_eq!(
        cost(refs.iter().copied()),
        spent(2, 1.0, 5.0, false),
        "a run that never started took no CPU"
    );
}

#[test]
fn the_slowest_suites_are_the_ten_of_longest_median_and_a_tie_goes_by_name() {
    // Eleven suites, s00 the fastest at 1 s; s09 and s10 the slowest, tied at 10 s.
    let names: Vec<String> = (0..11).map(|n| format!("s{n:02}")).collect();
    let suites: Vec<(&str, u32, f64)> = names
        .iter()
        .zip(0_u32..)
        .map(|(name, n)| (name.as_str(), 0, f64::from(n.min(9) + 1)))
        .collect();
    let line = run("t", "test", "just test-rs", at(0, 1.0), 60.0)
        .tests(&suites, &[])
        .0;
    let day = day("symphony", at(0, 0.0), now(), &[line]);
    assert_eq!(day.suites.len(), 11, "every suite kept");
    assert_eq!(
        day.slowest,
        [
            "s09", "s10", "s08", "s07", "s06", "s05", "s04", "s03", "s02", "s01"
        ],
        "ten, the slowest first, s00 left out"
    );
}
