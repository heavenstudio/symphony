//! A check that lists its steps, as `just verify` does, is told by them: each red step is a task of
//! its own with its own log, closed when that step is green, and the check files nothing of its own
//! while its steps hold its red; a red of the check filed before it listed steps and each red step's
//! name each other, and the check's closes once the check is green.

use super::{Fixture, NOW};

/// A stand-in verify: its steps lint, size-check and tests, each red while the candidate holds a
/// file `red-STEP`, listed in the folder the run names, each with a log of its own.
const STEPS: &str = r#"d="$SYMPHONY_CHECK_STEPS"
code=0
for step in lint size-check tests; do
    if [ -e "red-$step" ]; then
        status=1; code=1; echo "$step said it is red" > "$d/$step.log"
    else
        status=0; echo "$step green" > "$d/$step.log"
    fi
    printf '%s\t%s\t1\t%s\tjust %s\n' "$step" "$status" "$step.log" "$step" >> "$d/steps.tsv"
done
exit $code
"#;
const VERIFY: &str = "verify=sh steps.sh";

#[test]
fn each_red_step_is_its_own_task_with_its_own_log_and_closes_when_it_is_green() {
    let fixture = Fixture::new();
    fixture.land("steps.sh", Some(STEPS), "a verify that lists its steps");
    fixture.land("red-size-check", Some("red"), "past a size budget");
    let planted = fixture.land("red-tests", Some("red"), "a test red");

    let out = fixture.run(NOW, &["--check", VERIFY], 1);
    for said in [
        "nightly: verify red (exit 1, ",
        "nightly:   verify/lint green (exit 0, 1 s); log ",
        "nightly:   verify/size-check red (exit 1, 1 s); log ",
        "nightly: red: verify/size-check, verify/tests; no release",
        "nightly: filed tests/nightly-red-verify-size-check, tests/nightly-red-verify-tests\n",
    ] {
        assert!(out.contains(said), "{said:?} in:\n{out}");
    }
    let reds = fixture.reds();
    assert_eq!(
        reds.keys().collect::<Vec<_>>(),
        [
            "tests/nightly-red-verify-size-check",
            "tests/nightly-red-verify-tests"
        ],
        "one task a red step, and none for the check its steps hold"
    );
    let run = fixture.runs().last().unwrap().clone();
    let steps = run["checks"][0]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 3, "{run}");
    let log = steps[1]["log"].as_str().unwrap().to_string();
    let logs = fixture.records().join("logs");
    assert!(
        log.starts_with(&logs.to_string_lossy().into_owned())
            && log.ends_with("/verify.steps/size-check.log"),
        "the step's log is kept with the run's: {log}"
    );
    let red = &reds["tests/nightly-red-verify-size-check"];
    assert!(red.red() && red.priority == 0, "{red:?}");
    assert_eq!(
        red.title,
        format!("Nightly red: verify/size-check on {}", &planted[..12])
    );
    for said in [
        "The Symphony nightly is red in `verify/size-check` on main at ".to_string(),
        "Command: `just size-check`, in the nightly's clean checkout of that commit: exit 1."
            .into(),
        format!("Log: {log}"),
        "```\nsize-check said it is red\n```".into(),
        "key: verify-size-check\ncheck: verify/size-check".into(),
    ] {
        assert!(red.body.contains(&said), "{said:?} in:\n{}", red.body);
    }
    assert!(
        reds["tests/nightly-red-verify-tests"]
            .body
            .contains("```\ntests said it is red\n```")
    );

    // The size fixed: its task closed, the tests' told again.
    fixture.land("red-size-check", None, "within the budget");
    let out = fixture.run(NOW + 3600.0, &["--check", VERIFY], 1);
    assert!(
        out.contains("nightly: closed tests/nightly-red-verify-size-check"),
        "{out}"
    );
    assert!(
        out.contains("nightly: updated tests/nightly-red-verify-tests"),
        "{out}"
    );
    assert_eq!(
        fixture.reds()["tests/nightly-red-verify-size-check"].status,
        "cancelled"
    );

    // All green: released, and the last red closed.
    fixture.land("red-tests", None, "the test fixed");
    let out = fixture.run(NOW + 7200.0, &["--check", VERIFY], 0);
    assert!(
        out.contains("nightly: closed tests/nightly-red-verify-tests"),
        "{out}"
    );
    assert_eq!(fixture.remote_tags(), ["release-2026-11-12"]);
    assert!(!fixture.reds().contains_key("tests/nightly-red-verify"));
}

#[test]
fn a_red_of_the_check_filed_before_it_listed_steps_hears_of_them_and_closes_when_it_is_green() {
    let fixture = Fixture::new();
    fixture.land("red", Some("planted"), "a change that turns the check red");
    fixture.night(NOW, 1);
    assert_eq!(
        fixture.reds().keys().collect::<Vec<_>>(),
        ["tests/nightly-red-verify"]
    );

    // The check now lists its steps, and its tests are red.
    fixture.land("steps.sh", Some(STEPS), "a verify that lists its steps");
    let red = fixture.land("red-tests", Some("red"), "a test red");
    let out = fixture.run(NOW + 3600.0, &["--check", VERIFY], 1);
    assert!(
        out.contains("nightly: filed tests/nightly-red-verify-tests"),
        "{out}"
    );
    assert!(
        out.contains("nightly: updated tests/nightly-red-verify"),
        "{out}"
    );
    let thread = fixture.thread("tests/nightly-red-verify");
    let run = fixture.runs().last().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        thread.last().unwrap(),
        &format!(
            "Red on {} (run {run}, 2026-11-12 11:00) in its steps alone, each a task of its own: \
             `verify/tests` (exit 1) in tests/nightly-red-verify-tests. Whoever holds this task \
             reads those first, as one can be this same failure. This task closes once `verify` \
             is green.",
            &red[..12]
        )
    );
    assert_eq!(fixture.reds()["tests/nightly-red-verify"].status, "ready");
    // The step's red names the check's, as the two can be one failure.
    assert!(
        fixture.reds()["tests/nightly-red-verify-tests"]
            .body
            .contains(
                "The open red of `verify` itself, tests/nightly-red-verify, was filed before this \
                 one"
            ),
        "{}",
        fixture.reds()["tests/nightly-red-verify-tests"].body
    );

    fixture.land("red-tests", None, "the test fixed");
    let out = fixture.run(NOW + 7200.0, &["--check", VERIFY], 0);
    assert!(
        out.contains("nightly: closed tests/nightly-red-verify-tests, tests/nightly-red-verify\n"),
        "{out}"
    );
}
