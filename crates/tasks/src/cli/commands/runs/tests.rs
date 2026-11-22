//! What `runs` prints of a task's recorded runs at a tip: each command's newest run there and the
//! runs before it, the `tests` stamp's reading, the `[verdicts] check` as its gate reads it, and
//! the task's runs at other commits apart.

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

/// What `runs` prints of `runs` for the task and `tools/x/work` under it, at the branch's tip.
fn printed(runs: &[Record], check: Result<&str, &str>) -> Vec<String> {
    let ours = [SLUG.to_string(), "tools/x/work".to_string()];
    lines(&Shown {
        slug: SLUG,
        tip: TIP,
        from: &Tip::Branch(SLUG.into()),
        runs,
        ours: &ours,
        check,
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

#[test]
fn a_red_run_that_kept_a_log_is_followed_by_the_logs_path_and_no_other_run_is() {
    let kept = |log: &str| -> recorded::Run {
        serde_json::from_value(serde_json::json!({
            "pipeline": "verify", "trigger": "owner", "runs": "/board/runs", "log": log,
        }))
        .unwrap()
    };
    let mut failed = red(3, "just test-rs -p board", TIP, 101);
    failed.run = Some(kept("logs/2026-01-01/a.log"));
    let mut passed = green(4, "just check", TIP);
    passed.run = Some(kept("logs/2026-01-01/b.log"));
    // A red recorded with no pipeline kept no log: nothing to name.
    let bare = red(5, "just test-client", TIP, 1);
    let lines = printed(&[failed, passed, bare], Ok(""));
    let logs: Vec<&String> = lines.iter().filter(|line| line.contains("LOG")).collect();
    assert_eq!(
        logs,
        [
            "  LOG of the red `just test-rs -p board`, what failed is at its end: \
          /board/runs/logs/2026-01-01/a.log"
        ],
        "{lines:#?}"
    );
}
