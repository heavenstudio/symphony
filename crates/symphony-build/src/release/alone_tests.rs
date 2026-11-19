use super::*;
use crate::release::checks::Load;
use crate::release::group::Bound;
use crate::release::waits::far;
use crate::test_tree::Tree;

/// The nextest lane's own log of a run red in two cases of `runner-fixture`'s library.
const NEXTEST: &str = include_str!("fixtures/tests-nextest-red.log");

/// A shared-fixture lane's own log, red in one test of `symphony-build`'s release target.
const RELEASE: &str = "     Running tests/release/main.rs (.target/debug/deps/release-0123)

running 2 tests
test reds::one_fails ... FAILED
test reds::passes ... ok

failures:
    reds::one_fails

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.00s

error: test failed, to rerun pass `-p symphony-build --test release`
error: 1 target failed:
    `-p symphony-build --test release`
";

/// A step or check named `name`, run as `command`, exit `code`, its log `log`.
fn ran(name: &str, command: &str, code: i32, log: PathBuf) -> Ran {
    Ran {
        name: name.into(),
        command: command.into(),
        exit_code: Some(code),
        signal: None,
        seconds: 9.0,
        log,
        steps: Vec::new(),
        error: None,
        timed_out: false,
        interrupted: false,
        load: Some(Load {
            averages: [31.92, 70.68, 171.07],
            processors: 16,
        }),
        alone: None,
    }
}

/// A run of the tests' lanes in `tree`: each lane `(NAME, CODE, LOG)` with a log of its own in
/// `lanes/`, its output then in the step's log as the runner gives it, between `head` (after the
/// runner's first line) and `tail` (after the lanes' outputs, the runner's last lines).
fn lanes_run(tree: &Tree, lanes: &[(&str, i32, &str)], head: &str, tail: &str) -> Ran {
    let folder = tree.path("lanes");
    let mut log = format!("test-lanes: logs in {}\n{head}", folder.display());
    let mut parts = Vec::new();
    for (name, code, text) in lanes {
        let own = tree.write(&format!("lanes/{name}.log"), text);
        log.push_str(text);
        parts.push(ran(
            &format!("verify/tests/{name}"),
            &format!("tools/test-lanes.sh {name}"),
            *code,
            own,
        ));
    }
    log.push_str(tail);
    let code = lanes
        .iter()
        .map(|(_, code, _)| *code)
        .find(|code| *code != 0);
    Ran {
        steps: parts,
        ..ran(
            "verify/tests",
            "tools/test-lanes.sh",
            code.unwrap_or(0),
            tree.write("tests.log", &log),
        )
    }
}

/// The end the runner says once it ran every lane, and what verify adds after it.
fn ended() -> String {
    format!("{ENDED}\n\nverify: the tests ran against the client the client check built\n")
}

/// A red run of the nextest lane and the release lane, ended.
fn red_run(tree: &Tree) -> Ran {
    lanes_run(
        tree,
        &[("nextest", 100, NEXTEST), ("release", 101, RELEASE)],
        "",
        &ended(),
    )
}

#[test]
fn the_runner_is_read_for_its_folder_said_first_its_end_said_last_and_its_lanes_said_red() {
    let tree = Tree::new();
    let log = tree.write(
        "a.log",
        &format!(
            "test-lanes: logs in /lanes\n     Running tests/a.rs (x/a-1)\n\
             test-lanes: logs in /printed/by/a/test\n\
             error: the lane libraries is red (no exit status it could read) and listed no target \
             that failed, to rerun pass `--workspace --lib`\n\
             error: the lane libraries is red (again)\n{}",
            ended()
        ),
    );
    assert_eq!(
        said(&log),
        Said {
            folder: Some(PathBuf::from("/lanes")),
            ended: true,
            lanes: vec!["libraries".into()],
        }
    );
    // Its folder only before any target started, its end only as its last line.
    let log = tree.write(
        "b.log",
        &format!("     Running tests/a.rs (x/a-1)\ntest-lanes: logs in /printed\n{ENDED}\nmore\n"),
    );
    assert_eq!(said(&log), Said::default());
    let log = tree.write("c.log", "test-lanes: logs in \n");
    assert_eq!(said(&log), Said::default(), "an empty folder is none");
    assert_eq!(said(&tree.path("none.log")), Said::default());
}

#[test]
fn each_failure_a_red_run_of_the_lanes_names_is_rerun_over_the_folder_its_runner_said() {
    let tree = Tree::new();
    let step = red_run(&tree);
    assert_eq!(
        plan(&step),
        Some(Ok((
            tree.path("lanes"),
            vec![
                "-p runner-fixture --lib -- fails --exact".to_string(),
                "-p runner-fixture --lib -- timeout --exact".into(),
                "-p symphony-build --test release -- reds::one_fails --exact".into(),
            ]
        )))
    );

    // A lane that listed no target of its own, named red by the runner, is rerun by its own line;
    // a lane run before the scheduler writes no log of its own, and is named in the step's.
    let tree = Tree::new();
    let mut step = lanes_run(
        &tree,
        &[(
            "libraries",
            1,
            "     Running unittests src/lib.rs (x/board-1)\nrunning 9 tests\n",
        )],
        "",
        &format!(
            "error: the lane libraries is red (no exit status it could read) and listed no target \
             that failed, to rerun pass `--workspace --lib`\n{}",
            ended()
        ),
    );
    let log = step.log.clone();
    step.steps.push(ran(
        "verify/tests/build",
        "tools/test-lanes.sh build",
        101,
        log.clone(),
    ));
    fs::write(
        &log,
        format!(
            "{}error: test failed, to rerun pass `-p symphony-build --test build`\n{}",
            fs::read_to_string(&log).unwrap(),
            ended()
        ),
    )
    .unwrap();
    let (_, each) = plan(&step).unwrap().unwrap();
    assert_eq!(
        each,
        ["--workspace --lib", "-p symphony-build --test build"]
    );
}

#[test]
fn only_a_red_test_run_of_the_lanes_is_rerun() {
    let tree = Tree::new();
    let red = red_run(&tree);
    let green = Ran {
        exit_code: Some(0),
        steps: Vec::new(),
        ..red.clone()
    };
    let not_run = Ran {
        exit_code: None,
        error: Some("a step it needs was red".into()),
        ..red.clone()
    };
    let other = Ran {
        command: "just size-check".into(),
        ..red.clone()
    };
    let cargo = Ran {
        command: "just test-rs -p symphony-build".into(),
        ..red.clone()
    };
    for step in [green, not_run, other, cargo] {
        assert_eq!(plan(&step), None, "{step:?}");
    }
    assert!(runs_lanes(
        "SYMPHONY_UI_DIST=\"$PWD/x\" tools/test-lanes.sh browser"
    ));
    assert!(runs_lanes("/abs/tools/test-lanes.sh"));
    assert!(!runs_lanes("tools/test-lanes.sh.bak"));
}

#[test]
fn nothing_is_rerun_and_the_step_says_why_when_its_red_is_more_than_failures_it_names() {
    let refused = |step: &Ran| match plan(step) {
        Some(Err(why)) => why,
        other => panic!("{other:?} for {step:?}"),
    };
    let tree = Tree::new();
    let red = red_run(&tree);

    // Killed, or never ended with a status of its own.
    let killed = Ran {
        exit_code: Some(1),
        signal: Some(9),
        ..red.clone()
    };
    assert!(refused(&killed).starts_with("it ended without a status of its own"));

    // Its runner did not say it ran every lane to its end: what failed may be more than its tests.
    let tree = Tree::new();
    let cut = lanes_run(&tree, &[("release", 101, RELEASE)], "", "");
    assert!(
        refused(&cut).starts_with("its runner did not say it ran every lane"),
        "{}",
        refused(&cut)
    );

    // No folder its runner said, before any test.
    let tree = Tree::new();
    let step = red_run(&tree);
    let text = fs::read_to_string(&step.log).unwrap();
    fs::write(
        &step.log,
        text.replacen("test-lanes: logs in ", "elsewhere: ", 1),
    )
    .unwrap();
    assert!(refused(&step).starts_with("its runner named no folder"));

    // A target that found files of its build gone.
    let tree = Tree::new();
    let gone = lanes_run(
        &tree,
        &[(
            "store",
            101,
            "     Running tests/store.rs (/t/debug/deps/store-1)\n\
             error: test failed, to rerun pass `-p symphony-board --test store`\n\n\
             Caused by:\n  could not execute process `/t/debug/deps/store-1` (never executed)\n\n\
             Caused by:\n  No such file or directory (os error 2)\n",
        )],
        "",
        &ended(),
    );
    assert!(refused(&gone).starts_with("a target found files of its build gone"));

    // A red lane that names no failure of its own: the nextest lane red with no case failed.
    let tree = Tree::new();
    let unnamed = lanes_run(
        &tree,
        &[
            ("nextest", 104, "error: creating test list failed\n"),
            ("release", 101, RELEASE),
        ],
        "",
        &ended(),
    );
    assert_eq!(
        refused(&unnamed),
        "its lane `nextest` is red (exit 104) and names no failed test or target"
    );

    // A red run whose log names nothing failed.
    let tree = Tree::new();
    let mut nothing = lanes_run(&tree, &[], "", &ended());
    nothing.exit_code = Some(2);
    assert_eq!(refused(&nothing), "its log names no failed test or target");

    // More than twelve: a break, not load.
    let tree = Tree::new();
    let mut many = String::from("     Running tests/many.rs (x/many-1)\n");
    for n in 0..=MOST {
        many.push_str(&format!("test case_{n} ... FAILED\n"));
    }
    many.push_str("error: test failed, to rerun pass `-p p --test many`\n");
    let thirteen = lanes_run(&tree, &[("many", 101, &many)], "", &ended());
    assert_eq!(
        refused(&thirteen),
        "it names 13 failures, more than the 12 the run reruns: a break, not load"
    );
    // Twelve are rerun.
    let tree = Tree::new();
    let twelve = many.replacen("test case_0 ... FAILED\n", "", 1);
    let twelve = lanes_run(&tree, &[("many", 101, &twelve)], "", &ended());
    assert_eq!(plan(&twelve).unwrap().unwrap().1.len(), MOST);
}

/// `step` with its failures rerun, each `(ARGS, CODE)`.
fn reran(step: &Ran, reruns: &[(&str, i32)]) -> Ran {
    let reruns = (reruns.iter())
        .map(|(args, code)| ran(args, "tools/test-lanes.sh --alone", *code, PathBuf::new()))
        .collect();
    Ran {
        alone: Some(Alone {
            reruns,
            why_not: None,
        }),
        ..step.clone()
    }
}

#[test]
fn a_step_red_only_in_failures_that_each_passed_alone_is_green_and_so_is_its_check() {
    let tree = Tree::new();
    let red = red_run(&tree);
    let passed = reran(&red, &[("a", 0), ("b", 0)]);
    assert!(passed.green(), "its lanes' reds are those failures");
    assert_eq!(passed.ended(), "exit 100, every failure passed alone");
    let lint = ran("verify/lint", "just lint-rs", 0, PathBuf::new());
    let check = |steps: Vec<Ran>| Ran {
        steps,
        ..ran("verify", "just verify", 1, PathBuf::new())
    };
    let verify = check(vec![lint.clone(), passed.clone()]);
    assert!(verify.green(), "red only by a step that passed alone");
    assert_eq!(verify.ended(), "exit 1, every failure passed alone");
    assert!(steps::reds(&verify).is_empty());

    // Red beside it in another step, or ended without deciding: red.
    let red_lint = Ran {
        exit_code: Some(1),
        ..lint.clone()
    };
    assert!(!check(vec![red_lint, passed.clone()]).green());
    for stopped in [
        Ran {
            timed_out: true,
            ..verify.clone()
        },
        Ran {
            interrupted: true,
            ..verify.clone()
        },
        Ran {
            exit_code: None,
            signal: Some(9),
            ..verify.clone()
        },
        Ran {
            error: Some("cannot clean its group".into()),
            ..verify.clone()
        },
    ] {
        assert!(!stopped.green(), "{stopped:?}");
    }
    // A check that ended red with every step green, none of them by its reruns: its own red.
    assert!(!check(vec![lint.clone()]).green());

    // One that failed again, was stopped, or none rerun: red, and its check with it.
    let failed_again = reran(&red, &[("a", 0), ("b", 100)]);
    let stopped = Ran {
        alone: Some(Alone {
            reruns: vec![Ran {
                interrupted: true,
                ..ran("a", "x", 0, PathBuf::new())
            }],
            why_not: None,
        }),
        ..red.clone()
    };
    let none = Ran {
        alone: Some(Alone {
            reruns: Vec::new(),
            why_not: Some("it names 13 failures".into()),
        }),
        ..red.clone()
    };
    let mut voided = check(vec![lint.clone(), passed.clone()]);
    void(&mut voided, "the checkout changed");
    assert_eq!(
        voided.steps[1].alone.as_ref().unwrap().why_not.as_deref(),
        Some("the checkout changed")
    );
    assert!(!voided.green(), "reruns that judge nothing excuse nothing");
    for step in [failed_again, stopped, none, Ran { alone: None, ..red }] {
        assert!(!step.green(), "{step:?}");
        assert_eq!(step.ended(), "exit 100");
        assert!(!check(vec![lint.clone(), step]).green());
    }
}

#[test]
fn void_keeps_why_a_step_had_none_rerun() {
    let tree = Tree::new();
    let red = red_run(&tree);
    let none = Ran {
        alone: Some(Alone {
            reruns: Vec::new(),
            why_not: Some("it names 13 failures".into()),
        }),
        ..red
    };
    let mut check = Ran {
        steps: vec![none],
        ..ran("verify", "just verify", 1, PathBuf::new())
    };
    void(&mut check, "the run stopped");
    assert_eq!(
        check.steps[0].alone.as_ref().unwrap().why_not.as_deref(),
        Some("it names 13 failures")
    );
}

/// A checkout whose runner, run `--alone`, notes each failure it was given in `alone.txt` with the
/// folder it was given, and passes the ones named in `passes`.
fn checkout(passes: &str) -> Tree {
    let tree = Tree::new();
    tree.write(
        "tools/test-lanes.sh",
        &format!(
            "#!/bin/sh\n[ \"$1\" = --alone ] || exit 64\nfolder=\"$2\"; shift 2\n\
             printf '%s|%s\\n' \"$folder\" \"$*\" >> alone.txt\n\
             echo \"reran $*\"\ncase \"$*\" in {passes}) exit 0 ;; esac\nexit 100\n"
        ),
    );
    let script = tree.path("tools/test-lanes.sh");
    let mut mode = fs::metadata(&script).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o755);
    fs::set_permissions(&script, mode).unwrap();
    tree
}

#[test]
fn each_failure_is_rerun_alone_one_at_a_time_in_the_checkout_with_a_log_and_load_of_its_own() {
    let tree = checkout("*fails*|*one_fails*");
    let logs = tree.path("logs");
    let lint = ran("verify/lint", "just lint-rs", 0, PathBuf::new());
    let size = ran("verify/size-check", "just size-check", 1, PathBuf::new());
    let mut verify = Ran {
        steps: vec![lint, size, red_run(&tree)],
        ..ran("verify", "just verify", 1, PathBuf::new())
    };
    let mut out = Vec::new();
    let within = Within::bound(far());
    assert!(rerun(&mut verify, &tree.root, &logs, within, &mut out));
    // Each in its turn, over the folder the runner said, the next only once the last ended.
    let folder = tree.path("lanes").display().to_string();
    assert_eq!(
        tree.read("alone.txt"),
        format!(
            "{folder}|-p runner-fixture --lib -- fails --exact\n\
             {folder}|-p runner-fixture --lib -- timeout --exact\n\
             {folder}|-p symphony-build --test release -- reds::one_fails --exact\n"
        )
    );
    assert_eq!(verify.steps[0].alone, None, "a green step reruns nothing");
    assert_eq!(
        verify.steps[1].alone, None,
        "nor a red step that runs no tests"
    );
    let alone = verify.steps[2].alone.clone().unwrap();
    assert_eq!(alone.why_not, None);
    let said: Vec<(&str, Option<i32>, bool)> = (alone.reruns.iter())
        .map(|rerun| (rerun.name.as_str(), rerun.exit_code, rerun.load.is_some()))
        .collect();
    assert_eq!(
        said,
        [
            ("-p runner-fixture --lib -- fails --exact", Some(0), true),
            (
                "-p runner-fixture --lib -- timeout --exact",
                Some(100),
                true
            ),
            (
                "-p symphony-build --test release -- reds::one_fails --exact",
                Some(0),
                true
            ),
        ]
    );
    let first = &alone.reruns[0];
    assert_eq!(first.log, logs.join("verify-tests.alone/1.log"));
    assert_eq!(
        first.command,
        format!("tools/test-lanes.sh --alone {folder} -p runner-fixture --lib -- fails --exact")
    );
    assert!(
        fs::read_to_string(&first.log)
            .unwrap()
            .contains("reran -p runner-fixture --lib -- fails --exact\n")
    );
    assert!(!alone.passed() && !verify.steps[2].green() && !verify.green());
    assert_eq!(
        alone
            .passed_alone()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>(),
        [
            "-p runner-fixture --lib -- fails --exact",
            "-p symphony-build --test release -- reds::one_fails --exact"
        ]
    );
    let out = crate::release::waits::untimed(&String::from_utf8(out).unwrap());
    for line in [
        "release:   verify/tests reruns its N failures alone, one at a time, over its own build:\n"
            .to_string(),
        format!(
            "release:     `-p runner-fixture --lib -- fails --exact` passed alone (exit N, N s); \
             log {}\n",
            crate::release::waits::untimed(&first.log.display().to_string())
        ),
        "release:     `-p runner-fixture --lib -- timeout --exact` failed again alone (exit N, N s)"
            .into(),
        "release:   verify/tests stays red: N of its N failures did not pass alone\n".into(),
    ] {
        assert!(out.contains(&line), "{line:?} in:\n{out}");
    }

    // Every one passing alone: the step and the check green.
    let tree = checkout("*");
    let logs = tree.path("logs");
    let mut verify = Ran {
        steps: vec![red_run(&tree)],
        ..ran("verify", "just verify", 1, PathBuf::new())
    };
    let mut out = Vec::new();
    assert!(rerun(&mut verify, &tree.root, &logs, within, &mut out));
    assert!(verify.steps[0].alone.as_ref().unwrap().passed());
    assert!(verify.green());
    assert!(String::from_utf8(out).unwrap().contains(
        "release:   verify/tests: every failure passed alone, so it is no red of the release; the \
         run files one task for them\n"
    ));
}

#[test]
fn a_step_that_reruns_nothing_says_why_and_a_rerun_past_the_runs_time_never_starts_and_is_red() {
    let tree = checkout("*");
    let logs = tree.path("logs");
    let mut cut = lanes_run(&tree, &[("release", 101, RELEASE)], "", "");
    cut.name = "verify/tests".into();
    let mut verify = Ran {
        steps: vec![cut],
        ..ran("verify", "just verify", 1, PathBuf::new())
    };
    let mut out = Vec::new();
    assert!(!rerun(
        &mut verify,
        &tree.root,
        &logs,
        Within::bound(far()),
        &mut out
    ));
    let why = verify.steps[0]
        .alone
        .as_ref()
        .unwrap()
        .why_not
        .clone()
        .unwrap();
    assert!(why.starts_with("its runner did not say"), "{why}");
    assert_eq!(
        String::from_utf8(out).unwrap(),
        format!("release:   verify/tests reruns nothing alone: {why}\n")
    );
    assert!(!tree.path("alone.txt").exists());
    assert!(!verify.green());

    // The run's time is up: each rerun is red without starting, never passed over.
    let mut verify = Ran {
        steps: vec![red_run(&tree)],
        ..ran("verify", "just verify", 1, PathBuf::new())
    };
    let late = Within::bound(Bound::until(Instant::now()));
    let mut out = Vec::new();
    assert!(!rerun(&mut verify, &tree.root, &logs, late, &mut out));
    let alone = verify.steps[0].alone.as_ref().unwrap();
    assert_eq!(alone.reruns.len(), 3);
    assert!(alone.reruns.iter().all(|rerun| rerun.error.as_deref()
        == Some(crate::release::NOT_STARTED)
        && !rerun.green()
        && !failed_again(rerun)
        && said_of(rerun) == "not rerun"));
    assert!(!tree.path("alone.txt").exists());
    assert!(!verify.green());
}

#[test]
fn the_folder_for_the_reruns_logs_that_cannot_be_made_reruns_nothing() {
    let tree = checkout("*");
    let blocked = tree.write("logs", "a file where the logs go");
    let mut verify = Ran {
        steps: vec![red_run(&tree)],
        ..ran("verify", "just verify", 1, PathBuf::new())
    };
    let mut out = Vec::new();
    assert!(!rerun(
        &mut verify,
        &tree.root,
        &blocked,
        Within::bound(far()),
        &mut out
    ));
    let why = verify.steps[0]
        .alone
        .as_ref()
        .unwrap()
        .why_not
        .clone()
        .unwrap();
    assert!(
        why.starts_with("the run cannot make the folder of its reruns' logs"),
        "{why}"
    );
    assert!(!tree.path("alone.txt").exists());
}

#[test]
fn the_failures_that_passed_alone_are_listed_with_their_step_and_said_in_the_notes() {
    let tree = Tree::new();
    let red = red_run(&tree);
    let mut first = reran(&red, &[("a", 0), ("b", 100)]);
    first.alone.as_mut().unwrap().reruns[0].seconds = 3.0;
    let voided = Ran {
        name: "verify/other".into(),
        alone: Some(Alone {
            why_not: Some("the run stopped".into()),
            ..reran(&red, &[("c", 0)]).alone.unwrap()
        }),
        ..red.clone()
    };
    let checks = vec![Ran {
        steps: vec![first, voided],
        ..ran("verify", "just verify", 1, PathBuf::new())
    }];
    let passed: Vec<(&str, &str)> = (passed_in(&checks).into_iter())
        .map(|(step, rerun)| (step.name.as_str(), rerun.name.as_str()))
        .collect();
    assert_eq!(passed, [("verify/tests", "a")]);
    let load = "load 31.92, 70.68, 171.07 (over 1, 5 and 15 minutes) on 16 processors";
    assert_eq!(
        notes(&checks, "tests/passed-alone-r1"),
        [
            "Failed in this run and passed rerun alone, so no red of it \
             (tests/passed-alone-r1 is theirs):"
                .to_string(),
            format!(
                "- `a`, in verify/tests: failed (exit 100, {load}); passed alone (exit 0, 3 s, {load})."
            ),
        ]
    );
    let unloaded = Ran {
        load: None,
        ..ran("x", "y", 0, PathBuf::new())
    };
    assert_eq!(load_of(&unloaded), "load not recorded");
    assert!(notes(&[], "tests/passed-alone-r1").is_empty());
}

#[test]
fn a_rerun_is_said_by_how_it_ended_and_only_a_red_of_its_own_failed_again() {
    let rerun = |code: Option<i32>| ran("a", "x", code.unwrap_or(1), PathBuf::new());
    let said = |ran: Ran| (said_of(&ran), failed_again(&ran));
    assert_eq!(said(rerun(Some(0))), ("passed alone", false));
    assert_eq!(said(rerun(Some(100))), ("failed again alone", true));
    let killed = Ran {
        exit_code: None,
        signal: Some(11),
        ..rerun(None)
    };
    assert_eq!(said(killed), ("failed again alone", true));
    let stopped = Ran {
        interrupted: true,
        ..rerun(Some(0))
    };
    assert_eq!(said(stopped), ("stopped", false));
    let late = Ran {
        timed_out: true,
        exit_code: None,
        signal: Some(9),
        ..rerun(None)
    };
    assert_eq!(said(late), ("stopped at the run's time", false));
    let never = Ran {
        exit_code: None,
        error: Some("not started".into()),
        ..rerun(None)
    };
    assert_eq!(said(never), ("not rerun", false));
}
