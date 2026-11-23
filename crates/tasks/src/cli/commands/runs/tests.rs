//! What `runs` prints of a task's recorded runs at a tip: each command's newest run there and the
//! runs before it, each red one's log with the tests it failed, the `tests` stamp's reading, the
//! `[verdicts] check` as its gate reads it, and the task's runs at other commits apart.

use super::*;
use crate::cli::machine_work::recorded::Phase;

const TIP: &str = "4be91c2aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const EARLIER: &str = "7c0ffee1bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SLUG: &str = "tools/x";
const CHECK: &str = "just symphony-cli docs-audit --check";

/// A run of `label` for `task` at `commit` as the recorder keeps it, by `agent-ID`, begun at `id`
/// thousand seconds and taking a minute, finished with `exit` unless `state` says otherwise.
fn run(id: u8, task: &str, label: &str, commit: &str, state: Step, exit: i32) -> Record {
    let started = 1000.0 * f64::from(id);
    let phase = if label.contains("test") {
        Phase::Test
    } else {
        Phase::Build
    };
    Record {
        version: 1,
        operation_id: format!("{id:032x}"),
        parent_id: None,
        phase,
        label: label.into(),
        state,
        handle: Some(format!("agent-{id}")),
        task: Some(task.into()),
        checkout: "/checkout".into(),
        branch: Some(SLUG.into()),
        commit: Some(commit.into()),
        machine_id: None,
        host: "host".into(),
        pid: 10,
        child_pid: (state != Step::Queued).then_some(11),
        queued_at: started - 5.0,
        started_at: (state != Step::Queued).then_some(started),
        ended_at: (state == Step::Finished).then_some(started + 60.0),
        beat_at: started,
        exit_code: (state == Step::Finished).then_some(exit),
        signal: None,
        reason: String::new(),
        run: None,
        reading: None,
    }
}

fn green(id: u8, label: &str, commit: &str) -> Record {
    run(id, SLUG, label, commit, Step::Finished, 0)
}

fn red(id: u8, label: &str, commit: &str, exit: i32) -> Record {
    run(id, SLUG, label, commit, Step::Finished, exit)
}

/// The minute `seconds` reads as here, as `runs` prints it.
fn at(seconds: f64) -> String {
    local_minute(seconds)
}

/// What `runs` prints of `runs` for the task and `tools/x/work` under it, at the branch's tip, no
/// run's failed tests kept.
fn printed(runs: &[Record], check: Result<&str, &str>) -> Vec<String> {
    printed_failing(runs, check, &|_| None)
}

/// [`printed`], the tests each run failed as `failed` says them.
fn printed_failing(
    runs: &[Record],
    check: Result<&str, &str>,
    failed: &dyn Fn(&Record) -> Option<Vec<FailedTest>>,
) -> Vec<String> {
    let ours = [SLUG.to_string(), "tools/x/work".to_string()];
    lines(&Shown {
        slug: SLUG,
        tip: TIP,
        from: &Tip::Branch(SLUG.into()),
        runs,
        ours: &ours,
        check,
        failed,
    })
}

#[test]
fn each_command_at_the_tip_is_its_newest_run_and_the_check_with_no_run_is_said() {
    let runs = [
        // At an earlier tip: none of these is the tip's.
        green(1, "just test-rs -p board", EARLIER),
        green(2, "just lint-rs", EARLIER),
        // At the tip: a green, and a red then green of one command.
        green(3, "just lint-rs", TIP),
        red(4, "just test-rs -p board", TIP, 101),
        green(5, "just test-rs -p board", TIP),
        // Another task's run at the tip, and one of no task, are not the task's.
        run(6, "tools/y", "just test-rs -p y", TIP, Step::Finished, 1),
    ];
    assert_eq!(
        printed(&runs, Ok(CHECK)),
        [
            format!("TIP tools/x {TIP}"),
            "RUNS at 4be91c2, each command's newest run there:".into(),
            format!(
                "  green  build  just lint-rs           {}  60 s  agent-3",
                at(3060.0)
            ),
            format!(
                "  green  test   just test-rs -p board  {}  60 s  agent-5  1 earlier here, 1 red",
                at(5060.0)
            ),
            "TESTS met: Green at 4be91c2: `just lint-rs`, `just test-rs -p board`".into(),
            format!("CHECK `{CHECK}`, the project's [verdicts] check: no run at 4be91c2"),
            format!(
                "ELSEWHERE 2 runs of the task at other commits, the newest at 7c0ffee, {}: none \
                 are this tip's",
                at(2060.0)
            ),
        ]
    );
}

#[test]
fn a_red_command_says_why_and_work_under_the_task_says_whose_it_is() {
    let mut signalled = red(2, "just test-rs -p board", TIP, 0);
    signalled.exit_code = None;
    signalled.signal = Some(9);
    let runs = [
        red(1, "just lint-rs", TIP, 101),
        signalled,
        run(
            3,
            "tools/x/work",
            "just test-rs -p work",
            TIP,
            Step::Running,
            0,
        ),
        run(4, SLUG, "just client-check", TIP, Step::Queued, 0),
    ];
    let shown = printed(&runs, Ok(""));
    assert_eq!(
        shown[1..],
        [
            "RUNS at 4be91c2, each command's newest run there:".to_string(),
            format!(
                "  red      build  just lint-rs           {}  60 s  agent-1  exit 101",
                at(1060.0)
            ),
            format!(
                "  red      test   just test-rs -p board  {}  60 s  agent-2  ended by signal 9",
                at(2060.0)
            ),
            format!(
                "  running  test   just test-rs -p work   {}  -     agent-3  for tools/x/work",
                at(3000.0)
            ),
            format!(
                "  queued   build  just client-check      {}  -     agent-4",
                at(3995.0)
            ),
            "TESTS not met: `just lint-rs` red at 4be91c2: exit 101 (1 more red)".into(),
        ],
        "no CHECK line where the project names none, no ELSEWHERE with nothing elsewhere"
    );
}

#[test]
fn with_nothing_at_the_tip_it_says_so_and_where_the_task_last_ran() {
    let only_earlier = [green(1, "just test-rs -p board", EARLIER)];
    assert_eq!(
        printed(&only_earlier, Ok(CHECK))[1..],
        [
            "RUNS none at 4be91c2: no build or test of the task is recorded there".to_string(),
            "TESTS not met: No build or test at 4be91c2: the last ran at 7c0ffee".into(),
            format!("CHECK `{CHECK}`, the project's [verdicts] check: no run at 4be91c2"),
            format!(
                "ELSEWHERE 1 run of the task at other commits, the newest at 7c0ffee, {}: none is \
                 this tip's",
                at(1060.0)
            ),
        ]
    );
    assert_eq!(
        printed(&[], Err("symphony.toml: expected a table"))[1..],
        [
            "RUNS none at 4be91c2: no build or test of the task is recorded there".to_string(),
            "TESTS not met: No build or test recorded for it at 4be91c2".into(),
            "CHECK cannot tell: symphony.toml: expected a table".into(),
        ]
    );
}

#[test]
fn the_check_is_read_as_its_gate_reads_it_whoever_ran_it_at_the_tip() {
    let check = |runs: &[Record]| -> String {
        let shown = printed(runs, Ok(CHECK));
        let line = shown.iter().find(|line| line.starts_with("CHECK "));
        line.expect("a check line").clone()
    };
    let said = |rest: String| format!("CHECK `{CHECK}`, the project's [verdicts] check: {rest}");
    // A run of no task's at the tip is the check's, though not the task's: its gate reads it.
    let mut unattributed = green(1, CHECK, TIP);
    unattributed.task = None;
    unattributed.handle = None;
    assert_eq!(
        check(&[unattributed]),
        said(format!("green at 4be91c2, {}, by no session", at(1060.0)))
    );
    // The newest decides: green then red is red; at another commit it is no run here.
    assert_eq!(
        check(&[green(1, CHECK, TIP), red(2, CHECK, TIP, 1)]),
        said(format!(
            "red at 4be91c2: exit 1, {}, by agent-2",
            at(2060.0)
        ))
    );
    assert_eq!(
        check(&[green(1, CHECK, EARLIER)]),
        said("no run at 4be91c2".into())
    );
    assert_eq!(
        check(&[run(1, SLUG, CHECK, TIP, Step::Running, 0)]),
        said(format!(
            "running at 4be91c2 since {}, by agent-1",
            at(1000.0)
        ))
    );
    assert_eq!(
        check(&[run(1, SLUG, CHECK, TIP, Step::Queued, 0)]),
        said(format!("queued at 4be91c2 since {}, by agent-1", at(995.0)))
    );
}

#[test]
fn a_tip_named_with_tip_says_so() {
    let shown = lines(&Shown {
        slug: SLUG,
        tip: TIP,
        from: &Tip::Named,
        runs: &[],
        ours: &[SLUG.to_string()],
        check: Ok(""),
        failed: &|_| None,
    });
    assert_eq!(shown[0], format!("TIP {TIP} (--tip)"));
}

/// A command its runner refused at the tip is named apart with the refusal's line, is no row of
/// its command, and leaves the `tests` stamp and the check as the runs that ran left them.
#[test]
fn a_command_its_runner_refused_is_named_apart_with_its_line() {
    let refusal = |mut run: Record, line: &str| {
        let said = serde_json::json!({"pipeline": "test", "trigger": "owner", "refused": line});
        run.run = Some(serde_json::from_value(said).unwrap());
        run
    };
    let typed = "just test-rs --test hooks own_names owner_words";
    let usage = "error: unexpected argument 'owner_words' found";
    let runs = [
        green(1, "just lint-rs", TIP),
        green(2, "just test-rs --test hooks", TIP),
        refusal(red(3, typed, TIP, 1), usage),
        green(4, CHECK, TIP),
        refusal(
            run(5, "tools/x/work", CHECK, TIP, Step::Finished, 2),
            "error: justfile does not contain recipe `symphony-cl`",
        ),
    ];
    assert_eq!(
        printed(&runs, Ok(CHECK))[1..],
        [
            "RUNS at 4be91c2, each command's newest run there:".to_string(),
            format!(
                "  green  build  just lint-rs                          {}  60 s  agent-1",
                at(1060.0)
            ),
            format!(
                "  green  test   just test-rs --test hooks             {}  60 s  agent-2",
                at(2060.0)
            ),
            format!(
                "  green  build  just symphony-cli docs-audit --check  {}  60 s  agent-4",
                at(4060.0)
            ),
            "REFUSED at 4be91c2, each by its runner before it built or ran anything: no red and \
             no green there"
                .into(),
            format!(
                "  refused  test   {typed}  {}  agent-3  {usage}",
                at(3060.0)
            ),
            format!(
                "  refused  build  just symphony-cli docs-audit --check             {}  agent-5  \
                 error: justfile does not contain recipe `symphony-cl`; for tools/x/work",
                at(5060.0)
            ),
            "TESTS met: Green at 4be91c2: `just lint-rs`, `just test-rs --test hooks`, `just \
             symphony-cli docs-audit --check`"
                .into(),
            format!(
                "CHECK `{CHECK}`, the project's [verdicts] check: green at 4be91c2, {}, by agent-4",
                at(4060.0)
            ),
        ]
    );
    // Alone at the tip it is no run there, and the stamp says what was typed.
    assert_eq!(
        printed(&[refusal(red(3, typed, TIP, 1), usage)], Ok(""))[1..],
        [
            "RUNS none at 4be91c2: no build or test of the task is recorded there".to_string(),
            "REFUSED at 4be91c2, each by its runner before it built or ran anything: no red and \
             no green there"
                .into(),
            format!("  refused  test  {typed}  {}  agent-3  {usage}", at(3060.0)),
            format!(
                "TESTS not met: No build or test ran at 4be91c2: `{typed}` was refused by its \
                 runner and never ran ({usage})"
            ),
        ]
    );
}

/// A run of `label` for the task at `commit` that kept its log at `log` in `/board/runs`, a run of a
/// pipeline as the recorder keeps one, refused by its runner with `refused` when that names a line.
fn kept(mut run: Record, log: &str, refused: Option<&str>) -> Record {
    let kept = serde_json::json!({
        "pipeline": "test", "trigger": "owner", "runs": "/board/runs", "log": log,
        "refused": refused,
    });
    run.run = Some(serde_json::from_value(kept).unwrap());
    run
}

#[test]
fn each_red_run_at_the_tip_that_kept_a_log_is_followed_by_the_logs_path_and_no_other_run_is() {
    let board = "just test-rs -p board";
    let runs = [
        // A red at another commit is none of the tip's.
        kept(red(1, board, EARLIER, 101), "logs/2026-01-01/e.log", None),
        // Red once, then green at the same tip: the red is still named, as which test failed then
        // is what tells a flake from a race.
        kept(red(3, board, TIP, 101), "logs/2026-01-01/a.log", None),
        kept(green(4, "just check", TIP), "logs/2026-01-01/b.log", None),
        // A red recorded with no pipeline kept no log: nothing to name.
        red(5, "just test-client", TIP, 1),
        kept(green(6, board, TIP), "logs/2026-01-01/c.log", None),
        // Two reds of one command, the newer its newest run there, the one that keeps the stamp
        // from standing: each named, the older first.
        kept(
            red(7, "just lint-rs", TIP, 1),
            "logs/2026-01-01/d.log",
            None,
        ),
        kept(
            red(8, "just lint-rs", TIP, 2),
            "logs/2026-01-01/f.log",
            None,
        ),
        // A red under a run still going: the red is named, the run that goes is no red.
        kept(
            red(9, "just client-lint", TIP, 1),
            "logs/2026-01-01/g.log",
            None,
        ),
        kept(
            run(11, SLUG, "just client-lint", TIP, Step::Running, 0),
            "logs/2026-01-01/i.log",
            None,
        ),
        // A command its runner refused ran nothing, so its log holds no failed test.
        kept(
            red(10, "just test-rs -p nothing", TIP, 2),
            "logs/2026-01-01/h.log",
            Some("error: no such package"),
        ),
    ];
    let lines = printed(&runs, Ok(""));
    let first = lines.iter().position(|line| line.contains("LOG")).unwrap();
    let logs: Vec<&str> = (lines[first..].iter())
        .map(String::as_str)
        .take_while(|line| line.contains("LOG"))
        .collect();
    let log = |label: &str, seconds: f64, file: &str| {
        format!(
            "  LOG of the red `{label}` at {}, what failed is at its end: /board/runs/logs/2026-01-01/{file}",
            at(seconds)
        )
    };
    assert_eq!(
        logs,
        [
            log(board, 3060.0, "a.log"),
            log("just lint-rs", 7060.0, "d.log"),
            log("just lint-rs", 8060.0, "f.log"),
            log("just client-lint", 9060.0, "g.log"),
        ],
        "{lines:#?}"
    );
    // They follow the rows of the tip's runs, before the refused and the stamp.
    assert!(lines[first - 1].starts_with("  "), "{lines:#?}");
    assert!(
        lines[first + logs.len()].starts_with("REFUSED"),
        "{lines:#?}"
    );
    assert_eq!(lines.iter().filter(|line| line.contains("LOG")).count(), 4);
}

/// `n` failed tests of `suite`, or of none known: `t0`, `t1`, ….
fn failing(n: usize, suite: Option<&str>) -> Vec<FailedTest> {
    (0..n)
        .map(|i| FailedTest {
            suite: suite.map(str::to_string),
            name: format!("t{i}"),
            line: format!("assertion {i} failed"),
        })
        .collect()
}

#[test]
fn a_red_runs_log_is_followed_by_the_first_tests_it_failed_then_how_many_more() {
    let board = "just test-rs -p board";
    let runs = [
        // As many failed as are named: each named, with its binary, and no count.
        kept(red(1, board, TIP, 101), "logs/2026-01-01/a.log", None),
        // One more than are named: the rest counted.
        kept(
            red(2, "just lint-rs", TIP, 101),
            "logs/2026-01-01/b.log",
            None,
        ),
        // Many, of no binary known: named bare, the rest counted.
        kept(
            red(3, "just test-rs -p q", TIP, 101),
            "logs/2026-01-01/c.log",
            None,
        ),
        // Red with its tests kept, none of them failed (a binary that crashed): its log alone.
        kept(
            red(4, "just test-rs -p crash", TIP, 101),
            "logs/2026-01-01/d.log",
            None,
        ),
        // Red with no tests kept (swept, or no test binary): its log alone.
        kept(
            red(5, "just client-lint", TIP, 1),
            "logs/2026-01-01/e.log",
            None,
        ),
        // Green with a failed test kept (one retried green): no LOG, so nothing named.
        kept(green(6, "just check", TIP), "logs/2026-01-01/f.log", None),
    ];
    // Each run's id is its number ([`run`]).
    let failed = |run: &Record| match u8::from_str_radix(&run.operation_id, 16).unwrap() {
        1 => Some(failing(FAILED_NAMED, Some("tests/cli.rs (cli)"))),
        2 => Some(failing(FAILED_NAMED + 1, Some("tests/a.rs (a)"))),
        3 => Some(failing(40, None)),
        4 => Some(Vec::new()),
        6 => Some(failing(1, None)),
        _ => None,
    };
    let lines = printed_failing(&runs, Ok(""), &failed);
    let first = lines.iter().position(|line| line.contains("LOG")).unwrap();
    let under = lines[first..]
        .iter()
        .take_while(|line| line.starts_with("  LOG") || line.starts_with("    "))
        .cloned()
        .collect::<Vec<_>>();
    let log = |label: &str, id: f64, file: &str| {
        format!(
            "  LOG of the red `{label}` at {}, what failed is at its end: /board/runs/logs/2026-01-01/{file}",
            at(1000.0 * id + 60.0)
        )
    };
    let named = |range: std::ops::Range<usize>, suite: &str| -> Vec<String> {
        range.map(|i| format!("    failed t{i}{suite}")).collect()
    };
    let mut expected = vec![log(board, 1.0, "a.log")];
    expected.extend(named(0..FAILED_NAMED, " in tests/cli.rs (cli)"));
    expected.push(log("just lint-rs", 2.0, "b.log"));
    expected.extend(named(0..FAILED_NAMED, " in tests/a.rs (a)"));
    expected.push("    and 1 more failed".into());
    expected.push(log("just test-rs -p q", 3.0, "c.log"));
    expected.extend(named(0..FAILED_NAMED, ""));
    expected.push(format!("    and {} more failed", 40 - FAILED_NAMED));
    expected.push(log("just test-rs -p crash", 4.0, "d.log"));
    expected.push(log("just client-lint", 5.0, "e.log"));
    assert_eq!(under, expected, "{lines:#?}");
    // They come before the stamp's line, as the LOG lines do.
    assert!(
        lines[first + under.len()].starts_with("TESTS"),
        "{lines:#?}"
    );
}
