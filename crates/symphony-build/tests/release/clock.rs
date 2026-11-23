//! The hosted clock: where a project names `[releases] hosted`, `--due`, which a landing's
//! `on_land` command runs, also starts a run of main's tip once `[releases] hosted_every_minutes`
//! passed since the last run started, short of the count, unless the tip is judged already. That
//! run is verified on GitHub's runners or not at all: what they give no result of is recorded
//! unjudged, files no red and is asked for again after the interval, and nothing of it is verified
//! here. The count's run is verified here as ever, and no run of main's tip starts while a release
//! candidate is open.

use std::fs;

use serde_json::{Value, json};

use super::cadence::landing;
use super::hosted::{Stand, ended, fixture, job, line, ran_here, tip};
use super::{Fixture, NOW};

/// Half an hour: the clock's interval unless the project says.
const INTERVAL: f64 = 1800.0;

/// The stand-in answers a run of one step, `lint`, that ended `conclusion`.
fn answers(stand: &Stand, commit: &str, conclusion: &str) {
    let exit = if conclusion == "success" { "0" } else { "1" };
    stand.artifact(
        "artifact-7.zip",
        &line("lint", exit, "lint.log"),
        &[("lint.log", "the lint's log\n")],
    );
    stand.answer(&ended(
        commit,
        conclusion,
        &json!([job(2, "lint", conclusion, None)]),
        &json!([{"id": 7, "name": "verify-lint", "file": "artifact-7.zip"}]),
    ));
}

/// `release --due SLUG COMMIT` at `now` with the run's own checks, as a landing's command runs
/// it, which must end with `code`: its stdout.
fn due(fixture: &Fixture, now: f64, code: i32) -> String {
    let landed = tip(fixture);
    fixture.run(now, &["--due", "tools/last", &landed], code)
}

/// The hosted clock's line of `out`.
fn clock_line(out: &str) -> &str {
    out.lines()
        .find(|line| line.starts_with("hosted clock: "))
        .unwrap_or_default()
}

#[test]
fn a_landing_starts_a_run_on_githubs_runners_once_the_interval_passed_and_the_tip_is_not_judged() {
    let (fixture, stand) = fixture();
    let workflow = tip(&fixture);
    answers(&stand, &workflow, "success");
    let first = landing(&fixture, "tools/one");

    // No run on record: the first landing starts one, far short of the count of 20.
    let out = due(&fixture, NOW, 0);
    for said in [
        "next run: after 19 more landings (1 of 20), or the scheduled run\n",
        "hosted clock: due, every 30 min on GitHub's runners; no run is on record: the next \
         landing starts a run of origin/main's tip, or `just release --due`\n",
        "release: due: the hosted clock: a run on origin/main's tip starts, verified on GitHub's \
         runners or not at all\n",
        "release: verify verified on GitHub (",
        "release: green; tagged release-2026-11-12, pushed to origin",
    ] {
        assert!(out.contains(said), "{said:?} in:\n{out}");
    }
    assert!(!ran_here(&fixture), "nothing is verified here:\n{out}");
    assert_eq!(fixture.tagged("release-2026-11-12"), first);
    let run = fixture.runs().pop().unwrap();
    let cause = run["cause"].as_str().unwrap();
    assert!(
        cause.starts_with("the hosted clock, every 30 min: no run is on record; 1 landings since "),
        "{cause}"
    );
    assert!(cause.ends_with(", the last tools/one"), "{cause}");
    let id = run["id"].as_str().unwrap().to_string();

    // Ten minutes later a landing moved the tip: the interval has not passed, and nothing runs.
    let second = landing(&fixture, "tools/two");
    let out = due(&fixture, NOW + 600.0, 0);
    let said = clock_line(&out);
    assert!(
        said.starts_with(&format!(
            "hosted clock: every 30 min on GitHub's runners; the run {id} started "
        )) && said.contains(" (10 min ago): the first landing from ")
            && said.ends_with(" starts a run of origin/main's tip"),
        "{out}"
    );
    assert!(!out.contains("release: due"), "{out}");
    assert_eq!(fixture.runs().len(), 1);
    assert_eq!(stand.asked().len(), 1);
    // `--status` says the same, and runs nothing.
    let status = fixture.run(NOW + 600.0, &["--status"], 0);
    assert_eq!(clock_line(&status), said, "{status}");

    // At the interval the same tip is run, and its record says what started it.
    let out = due(&fixture, NOW + INTERVAL, 0);
    assert!(
        out.contains(&format!(
            "hosted clock: due, every 30 min on GitHub's runners; the run {id} started "
        )) && out.contains("release: due: the hosted clock: a run on origin/main's tip starts"),
        "{out}"
    );
    assert!(!ran_here(&fixture), "{out}");
    assert_eq!(stand.asked().len(), 2);
    assert_eq!(stand.asked()[1]["commit"], second.as_str());
    assert_eq!(fixture.remote_tags().len(), 2, "{out}");
    let run = fixture.runs().pop().unwrap();
    let cause = run["cause"].as_str().unwrap();
    assert!(
        cause.starts_with(&format!(
            "the hosted clock, every 30 min: the run {id} started "
        )) && cause.contains(" (30 min ago); 1 landings since release-2026-11-12 (")
            && cause.ends_with(", the last tools/two"),
        "{cause}"
    );

    // A tip judged already starts none, however long ago the last run started.
    let out = due(&fixture, NOW + 10.0 * INTERVAL, 0);
    assert_eq!(
        clock_line(&out),
        format!(
            "hosted clock: every 30 min on GitHub's runners; origin/main's tip ({}) is judged \
             already: no run until it moves",
            &second[..12]
        ),
        "{out}"
    );
    assert_eq!(fixture.runs().len(), 2);
    assert_eq!(stand.asked().len(), 2);
}

#[test]
fn a_clock_run_github_gives_no_result_of_verifies_nothing_here_and_is_asked_again_in_time() {
    let (fixture, stand) = fixture();
    let workflow = tip(&fixture);
    landing(&fixture, "tools/one");
    let unanswered: Vec<(Option<Value>, &str)> = vec![
        (
            Some(json!({"state": "unavailable", "why": "GitHub answered 503 to GET /repos/o/n"})),
            "unavailable: GitHub answered 503 to GET /repos/o/n",
        ),
        (
            Some(
                json!({"state": "refused", "why": "turned off: the committed settings do not say"}),
            ),
            "refused: turned off: the committed settings do not say",
        ),
        (
            Some(json!({"state": "asleep"})),
            "the command answered the state \"asleep\"",
        ),
        (None, "symphony-github: no working directory"),
    ];
    let mut now = NOW;
    for (n, (answer, why)) in unanswered.iter().enumerate() {
        match answer {
            Some(answer) => stand.answer(answer),
            None => fs::write(stand.folder.join("fails"), "").unwrap(),
        }
        let out = due(&fixture, now, 0);
        for said in [
            "; the hosted clock's run verifies nothing here\n",
            "release: unjudged: no release, no red; the hosted clock asks again once its \
             interval has passed, and the count's run verifies here\n",
        ] {
            assert!(out.contains(said), "{said:?} in:\n{out}");
        }
        let said = (out.lines())
            .find(|line| line.starts_with("release: verify not verified: "))
            .unwrap_or_else(|| panic!("{out}"));
        assert!(said.contains(why), "{why:?} in {said}");
        assert!(!ran_here(&fixture), "nothing is verified here:\n{out}");
        assert_eq!(stand.asked().len(), n + 1);
        let runs = fixture.runs();
        assert_eq!(runs.len(), n + 1);
        let run = runs.last().unwrap();
        assert_eq!(run["result"], "unjudged");
        assert_eq!(run["checks"], json!([]));
        assert_eq!(run["hosted"][0]["judged"], "nowhere");
        assert!(run["hosted"][0]["why"].as_str().unwrap().contains(why));
        assert!(run["tag"].is_null() && run["error"].is_null(), "{run}");

        // Within the interval of that run's start nothing asks again, whatever lands.
        landing(&fixture, &format!("tools/more-{n}"));
        let out = due(&fixture, now + INTERVAL - 60.0, 0);
        assert!(!out.contains("release: due"), "{out}");
        assert!(
            clock_line(&out).contains(" (29 min ago): the first landing from "),
            "{out}"
        );
        assert_eq!(stand.asked().len(), n + 1);
        now += INTERVAL;
    }
    assert!(fixture.remote_tags().is_empty());
    assert_eq!(fixture.reds().len(), 0, "{:?}", fixture.reds());
    let status = fixture.run(now - 60.0, &["--status"], 0);
    assert!(
        status.contains("unjudged, ") && status.contains("; verify not verified: "),
        "{status}"
    );

    // Once GitHub answers, the next run after the interval judges the tip and releases it.
    fs::remove_file(stand.folder.join("fails")).unwrap();
    answers(&stand, &workflow, "success");
    let out = due(&fixture, now, 0);
    assert!(
        out.contains("release: green; tagged release-2026-11-12, pushed to origin"),
        "{out}"
    );
    assert!(!ran_here(&fixture), "{out}");
    assert_eq!(fixture.tagged("release-2026-11-12"), tip(&fixture));
}

#[test]
fn a_clock_run_of_a_check_githubs_runners_do_not_serve_runs_nothing_here() {
    let (fixture, stand) = fixture();
    landing(&fixture, "tools/one");
    let ran = fixture.root.join("check-ran-here");
    let check = format!("verify=touch '{}'", ran.display());
    let landed = tip(&fixture);
    let out = fixture.run(NOW, &["--check", &check, "--due", "tools/last", &landed], 0);
    assert!(
        out.contains(&format!(
            "release: verify not verified: GitHub's runners verify `just verify` alone, never \
             `touch '{}'`; the hosted clock's run verifies nothing here\n",
            ran.display()
        )),
        "{out}"
    );
    assert!(!ran.exists(), "{out}");
    assert!(stand.asked().is_empty(), "nothing it serves, nothing asked");
    assert_eq!(fixture.runs().pop().unwrap()["result"], "unjudged");
}

#[test]
fn the_counts_run_is_verified_here_when_github_gives_no_result() {
    let (fixture, stand) = fixture();
    fixture.land(
        "symphony.toml",
        Some(&Fixture::project_file(&format!(
            "every_landings = 2\n{}",
            stand.setting()
        ))),
        "count every two landings",
    );
    stand.answer(&json!({"state": "unavailable", "why": "GitHub answered 503"}));
    landing(&fixture, "tools/one");
    landing(&fixture, "tools/two");
    let out = due(&fixture, NOW, 0);
    for said in [
        "next run: due: 2 of 2 landed; the next landing starts it, or `just release --due`\n",
        "hosted clock: every 30 min on GitHub's runners; the count's run is due first\n",
        "release: verify verified here, not on GitHub: unavailable: GitHub answered 503\n",
        "release: green; tagged release-2026-11-12, pushed to origin",
    ] {
        assert!(out.contains(said), "{said:?} in:\n{out}");
    }
    assert!(!out.contains("the hosted clock: a run"), "{out}");
    assert!(
        ran_here(&fixture),
        "the count's run is this machine's:\n{out}"
    );
    let run = fixture.runs().pop().unwrap();
    assert!(
        run["cause"]
            .as_str()
            .unwrap()
            .starts_with("2 landings since "),
        "{run}"
    );
    assert_eq!(run["hosted"][0]["judged"], "here");
}

#[test]
fn a_red_clock_run_holds_its_commit_and_no_run_of_main_starts_while_that_candidate_is_open() {
    let (fixture, stand) = fixture();
    fixture.land(
        "symphony.toml",
        Some(&Fixture::project_file(&format!(
            "candidate_reruns = 3\n{}",
            stand.setting()
        ))),
        "hold a release candidate",
    );
    let workflow = tip(&fixture);
    answers(&stand, &workflow, "failure");
    let red = landing(&fixture, "tools/one");
    let out = due(&fixture, NOW, 1);
    assert!(
        out.contains("release:   verify/lint red (exit 1, 12 s); log "),
        "{out}"
    );
    assert!(
        !ran_here(&fixture),
        "a red there is not verified here:\n{out}"
    );
    assert_eq!(
        fixture.reds().keys().collect::<Vec<_>>(),
        ["tests/release-red-verify-lint"]
    );
    assert!(
        out.contains(&format!("holds the release candidate {}", &red[..12])),
        "{out}"
    );

    // Hours later and landings on, the candidate's red still waits: main's tip is not run.
    landing(&fixture, "tools/two");
    let out = due(&fixture, NOW + 4.0 * INTERVAL, 0);
    assert!(
        out.contains(&format!(
            "next run: held for the release candidate {}",
            &red[..12]
        )),
        "{out}"
    );
    assert_eq!(
        clock_line(&out),
        "hosted clock: every 30 min on GitHub's runners; the release candidate comes first"
    );
    assert!(!out.contains("release: due"), "{out}");
    assert_eq!(stand.asked().len(), 1);
    assert_eq!(fixture.runs().len(), 1);
}

#[test]
fn the_clock_is_off_at_0_minutes_and_with_the_cadence_and_only_the_count_starts_a_run() {
    for off in ["hosted_every_minutes = 0\n", "every_landings = 0\n"] {
        let (fixture, stand) = fixture();
        fixture.land(
            "symphony.toml",
            Some(&Fixture::project_file(&format!("{off}{}", stand.setting()))),
            "no clock",
        );
        landing(&fixture, "tools/one");
        let out = due(&fixture, NOW + 100.0 * INTERVAL, 0);
        assert!(!out.contains("hosted clock"), "{off}: {out}");
        assert!(!out.contains("release: due"), "{off}: {out}");
        assert!(stand.asked().is_empty(), "{off}");
        assert!(fixture.runs().is_empty(), "{off}");
    }
}
