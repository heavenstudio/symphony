//! A red run of the tests' lanes reruns each failure its log names alone, one at a time, in the same
//! checkout and over the same build (docs/workflow/RELEASES.md, "The release run", step 5): one that passes
//! alone is no red, and the run files one p1 task for every such test of it; one that fails again
//! is red as before, its task saying it failed twice; more than twelve failures are red with none
//! rerun; and a candidate's rerun red only in tests that passed alone releases it.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use super::candidate::{branch, hands, holding, record};
use super::{Fixture, NOW, git};

/// A stand-in verify: its steps lint and tests, run as `sh steps.sh [STEP...]`. Its tests step runs
/// as `tools/test-lanes.sh` does: it says the folder of its lanes' logs first, fails in the target
/// `-p fixture --test flaky` each test `OUTSIDE/failing` names, a line each, and says it ran every
/// lane to its end last unless `OUTSIDE/cut` is there.
const STEPS: &str = r#"d="$SYMPHONY_CHECK_STEPS"
code=0
for step in ${*:-lint tests}; do
    status=0
    if [ "$step" = tests ]; then
        mkdir -p "$d/lanes"
        printf 'build\n' > "$d/lanes/binaries.json"
        {
            printf 'test-lanes: logs in %s\n' "$d/lanes"
            echo '     Running tests/flaky.rs (.target/debug/deps/flaky-0123)'
            while read -r name; do
                [ -n "$name" ] && status=101 && echo "test $name ... FAILED"
            done < OUTSIDE/failing
            if [ "$status" != 0 ]; then
                echo 'error: test failed, to rerun pass `-p fixture --test flaky`'
            fi
            [ -e OUTSIDE/cut ] || echo 'test-lanes: ended: every lane asked for ran'
        } > "$d/tests.log"
        command=tools/test-lanes.sh
    else
        echo "$step green" > "$d/$step.log"; command="just $step"
    fi
    [ "$status" = 0 ] || code=1
    printf '%s\t%s\t1\t%s\t%s\t1.50 2.50 3.50 4\n' "$step" "$status" "$step.log" "$command" \
        >> "$d/steps.tsv"
done
exit $code
"#;

/// The stand-in runner, as the run reruns a failure with it: `--alone FOLDER ARGS`, noted in
/// `OUTSIDE/alone.txt` with the folder's build there, and passing when `OUTSIDE/passes-alone` names
/// the test its arguments name.
const RUNNER: &str = r#"#!/bin/sh
[ "$1" = --alone ] || exit 64
folder="$2"; shift 2
[ -e "$folder/binaries.json" ] || exit 65
echo "$*" >> OUTSIDE/alone.txt
echo "running $*"
name="$6"
grep -qx -- "$name" OUTSIDE/passes-alone 2>/dev/null && exit 0
echo "test $name ... FAILED"
exit 100
"#;

const VERIFY: &str = "verify=sh steps.sh";

/// The folder outside the repository the stand-ins read, and write what they reran in.
fn outside(fixture: &Fixture) -> PathBuf {
    fixture.root.join("outside")
}

/// The stand-in verify and runner landed, each test `failing` names failing in the run and each
/// `passes` names passing alone.
fn landed(fixture: &Fixture, failing: &[&str], passes: &[&str]) {
    let outside = outside(fixture);
    fs::create_dir_all(&outside).unwrap();
    plant(fixture, failing, passes);
    let at = |text: &str| text.replace("OUTSIDE", &outside.to_string_lossy());
    fixture.write("tools/test-lanes.sh", &at(RUNNER));
    let runner = fixture.main.join("tools/test-lanes.sh");
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755)).unwrap();
    fixture.land(
        "steps.sh",
        Some(&at(STEPS)),
        "a verify whose tests run in lanes",
    );
}

/// Each test `failing` names failing in the next run and each `passes` names passing alone.
fn plant(fixture: &Fixture, failing: &[&str], passes: &[&str]) {
    let lines = |names: &[&str]| {
        names
            .iter()
            .map(|name| format!("{name}\n"))
            .collect::<String>()
    };
    let outside = outside(fixture);
    fs::write(outside.join("failing"), lines(failing)).unwrap();
    fs::write(outside.join("passes-alone"), lines(passes)).unwrap();
    let _ = fs::remove_file(outside.join("alone.txt"));
}

/// What the stand-in runner reran, in order.
fn reran(fixture: &Fixture) -> Vec<String> {
    let said = fs::read_to_string(outside(fixture).join("alone.txt")).unwrap_or_default();
    said.lines().map(str::to_string).collect()
}

/// The open tasks the runs filed for the tests that passed alone, by slug: title, priority, red
/// mark and body.
fn passed_alone(fixture: &Fixture) -> Vec<(String, String, i64, bool, String)> {
    (fixture.store().tasks(None).unwrap().into_iter())
        .filter(|task| task.slug.starts_with("tests/passed-alone-"))
        .map(|task| {
            let red = task.red();
            (task.slug, task.title, task.priority, red, task.body)
        })
        .collect()
}

#[test]
fn a_test_that_fails_in_company_and_passes_alone_is_no_red_and_has_one_p1_task_of_its_run() {
    let fixture = Fixture::new();
    landed(
        &fixture,
        &["store::busy", "store::slow"],
        &["store::busy", "store::slow"],
    );
    let tip = git(&fixture.main, &["rev-parse", "HEAD"]);
    let out = fixture.run(NOW, &["--check", VERIFY], 0);
    assert_eq!(
        reran(&fixture),
        [
            "-p fixture --test flaky -- store::busy --exact",
            "-p fixture --test flaky -- store::slow --exact"
        ],
        "each failure alone, one at a time, over the build the run's lanes used"
    );
    for said in [
        "release:   verify/tests reruns its 2 failures alone, one at a time, over its own build:\n",
        "release:     `-p fixture --test flaky -- store::busy --exact` passed alone (exit 0, ",
        "release:   verify/tests: every failure passed alone, so it is no red of the release; the \
         run files one task for them\n",
        "release: green; tagged release-2026-11-12, pushed to origin\n",
    ] {
        assert!(out.contains(said), "{said:?} in:\n{out}");
    }
    assert_eq!(fixture.tagged("release-2026-11-12"), tip);
    assert!(fixture.reds().is_empty(), "no red: {:?}", fixture.reds());

    // One p1 task for both, no red, naming each test's two results and the load at each.
    let filed = passed_alone(&fixture);
    assert_eq!(filed.len(), 1, "{filed:?}");
    let (slug, title, priority, red, body) = &filed[0];
    let run = fixture.runs().pop().unwrap();
    let id = run["id"].as_str().unwrap();
    assert!(slug.starts_with("tests/passed-alone-2026-11-12"), "{slug}");
    assert!(!slug.starts_with("tests/release-red-"));
    assert_eq!(
        title,
        &format!("Passed alone: 2 tests failed in release run {id} and passed rerun alone")
    );
    assert_eq!((*priority, *red), (1, false));
    let load = "load 1.50, 2.50, 3.50 (over 1, 5 and 15 minutes) on 4 processors";
    for said in [
        format!(
            "- `-p fixture --test flaky -- store::busy --exact`, in `verify/tests`: failed in the \
             run (exit 101, {load}; log "
        ),
        "Find why each failed in company and passed alone".into(),
        "Never a timeout, a budget or a tolerance widened, a retry added or a test ignored to pass. \
         It holds no release"
            .into(),
        format!("\n\nrun: {id}"),
    ] {
        assert!(body.contains(&said), "{said:?} in:\n{body}");
    }
    assert!(body.contains("store::slow"), "{body}");

    // The record keeps both results and the load of each; the tag's notes say them.
    let tests = &run["checks"][0]["steps"][1];
    assert_eq!(tests["exit_code"], 101);
    let reruns = tests["alone"]["reruns"].as_array().unwrap();
    assert_eq!(reruns.len(), 2, "{tests}");
    assert_eq!(reruns[0]["exit_code"], 0);
    assert!(reruns[0]["load"].is_object(), "{tests}");
    assert!(
        reruns[0]["log"]
            .as_str()
            .unwrap()
            .ends_with("/verify-tests.alone/1.log"),
        "{tests}"
    );
    let message = fixture.message("release-2026-11-12");
    assert!(
        message.contains(&format!(
            "Failed in this run and passed rerun alone, so no red of it ({slug} is theirs):\n\
             - `-p fixture --test flaky -- store::busy --exact`, in verify/tests: failed (exit \
             101, {load}); passed alone (exit 0, "
        )),
        "{message}"
    );
}

#[test]
fn a_test_that_fails_again_alone_is_red_saying_it_failed_twice_and_the_one_that_passed_has_its_task()
 {
    let fixture = Fixture::new();
    landed(
        &fixture,
        &["store::busy", "store::broken"],
        &["store::busy"],
    );
    let out = fixture.run(NOW, &["--check", VERIFY], 1);
    for said in [
        "release:     `-p fixture --test flaky -- store::broken --exact` failed again alone (exit \
         100, ",
        "release:   verify/tests stays red: 1 of its 2 failures did not pass alone\n",
        "release: red: verify/tests; no release",
    ] {
        assert!(out.contains(said), "{said:?} in:\n{out}");
    }
    assert!(fixture.remote_tags().is_empty());
    let reds = fixture.reds();
    let red = &reds["tests/release-red-verify-tests"].body;
    assert!(
        red.contains(
            "- `-p fixture --test flaky -- store::broken --exact`: failed again alone (exit 100, "
        ) && red.contains("): it failed twice. Log: "),
        "{red}"
    );
    assert!(
        red.contains("- `-p fixture --test flaky -- store::busy --exact`: passed alone (exit 0, "),
        "{red}"
    );
    let filed = passed_alone(&fixture);
    assert_eq!(filed.len(), 1, "{filed:?}");
    assert!(filed[0].4.contains("store::busy") && !filed[0].4.contains("store::broken"));
}

#[test]
fn more_than_twelve_failures_are_red_with_none_rerun_and_a_cut_run_reruns_nothing() {
    let fixture = Fixture::new();
    let names: Vec<String> = (1..=13).map(|n| format!("store::case_{n:02}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    landed(&fixture, &names, &names);
    let out = fixture.run(NOW, &["--check", VERIFY], 1);
    assert!(
        out.contains(
            "release:   verify/tests reruns nothing alone: it names 13 failures, more than the 12 \
             the run reruns: a break, not load\n"
        ),
        "{out}"
    );
    assert!(reran(&fixture).is_empty(), "{:?}", reran(&fixture));
    let red = &fixture.reds()["tests/release-red-verify-tests"].body;
    assert!(
        red.contains(
            "The run reran none of its failures alone: it names 13 failures, more than the 12 the \
             run reruns: a break, not load.\n"
        ),
        "{red}"
    );
    assert!(passed_alone(&fixture).is_empty());

    // Twelve are rerun; a run whose runner did not say it ended reruns none.
    plant(&fixture, &names[..12], &names[..12]);
    fixture.run(NOW + 3600.0, &["--check", VERIFY], 0);
    assert_eq!(reran(&fixture).len(), 12);
    fixture.land("notes.txt", Some("later"), "a change after the release");
    plant(&fixture, &["store::busy"], &["store::busy"]);
    fs::write(outside(&fixture).join("cut"), "").unwrap();
    let out = fixture.run(NOW + 2.0 * 3600.0, &["--check", VERIFY], 1);
    assert!(
        out.contains("release:   verify/tests reruns nothing alone: its runner did not say it ran"),
        "{out}"
    );
    assert!(reran(&fixture).is_empty());
}

#[test]
fn a_candidates_rerun_red_only_in_tests_that_pass_alone_releases_it_and_counts_no_red_rerun() {
    let fixture = holding("candidate_reruns = 1\n");
    landed(&fixture, &["store::broken"], &[]);
    let tip = git(&fixture.main, &["rev-parse", "HEAD"]);
    fixture.run(NOW, &["--check", VERIFY], 1);
    let red = "tests/release-red-verify-tests";
    assert_eq!(record(&fixture)["reds"][0]["slug"], red);
    assert_eq!(branch(&fixture), Some(tip.clone()));

    // Its fix is made outside this repository; the rerun's tests fail in company and pass alone.
    plant(&fixture, &["store::busy"], &["store::busy"]);
    let hands = hands(&fixture);
    let store = fixture.store();
    (store.take_role(red, &hands.orchestrator, "orchestrator")).unwrap();
    store
        .move_with(
            red,
            "cancelled",
            &hands.orchestrator,
            Some("fixed outside this repository"),
        )
        .unwrap();
    let out = fixture.run(NOW + 60.0, &["--check", VERIFY, "--rerun"], 0);
    for said in [
        "release: `verify` reruns tests, the rest carried green from this tree\n",
        "release:   verify/tests: every failure passed alone, so it is no red of the release; the \
         run files one task for them\n",
        "release: green; tagged release-2026-11-12, pushed to origin\n",
    ] {
        assert!(out.contains(said), "{said:?} in:\n{out}");
    }
    assert_eq!(
        reran(&fixture),
        ["-p fixture --test flaky -- store::busy --exact"]
    );
    assert_eq!(fixture.tagged("release-2026-11-12"), tip);
    assert_eq!(passed_alone(&fixture).len(), 1);
    assert_ne!(record(&fixture)["state"], "dropped", "{}", record(&fixture));
}
