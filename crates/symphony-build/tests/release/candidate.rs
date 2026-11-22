//! The release candidate (docs/workflow/RELEASES.md): a run of main's tip red in a check holds that commit,
//! the local branch `release-candidate`, until its reds' own lands answer them; while it waits main's
//! tip is not run; a red's own land advances it to the newest approved commit of the fix that holds
//! the candidate and the fix alone, once the remote's main holds the merge; its rerun runs what was
//! red and what its fixes changed, and releases it; and its bounds drop it, which makes a run of
//! main's tip due.

use std::fs;

use symphony_board::NewSession;

use super::{CHECK, Fixture, NOW, git};

/// The red task of the stand-in check.
pub(super) const RED: &str = "tests/release-red-verify";

/// The fixture holding release candidates, with `releases` more lines of its `[releases]` table
/// (its bounds the project's defaults unless they say).
pub(super) fn holding(releases: &str) -> Fixture {
    let fixture = Fixture::new();
    let releases = if releases.contains("candidate_") {
        releases.to_string()
    } else {
        format!("candidate_reruns = 3\n{releases}")
    };
    fixture.land(
        "symphony.toml",
        Some(&Fixture::project_file(&releases)),
        "hold release candidates",
    );
    fixture
}

/// The fixture holding the commit that plants `red` as the release candidate, after its first
/// run: the commit.
pub(super) fn held(fixture: &Fixture) -> String {
    let red = fixture.land("red", Some("planted"), "a red change");
    let out = fixture.night(NOW, 1);
    assert!(
        out.contains(&format!(
            "release: holds the release candidate {} (first judged at {}, 1 run of it): its reds' \
             fixes start from the branch release-candidate\n",
            &red[..12],
            &red[..12]
        )),
        "{out}"
    );
    red
}

/// The candidate's record.
pub(super) fn record(fixture: &Fixture) -> serde_json::Value {
    let text = fs::read_to_string(fixture.records().join("candidate.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// The commit the local branch `release-candidate` names, none when there is no such branch.
pub(super) fn branch(fixture: &Fixture) -> Option<String> {
    let at = git(
        &fixture.main,
        &[
            "for-each-ref",
            "--format=%(objectname)",
            "refs/heads/release-candidate",
        ],
    );
    (!at.is_empty()).then_some(at)
}

/// The sessions that staff a red on the board: its orchestrator, its executor and a fresh reviewer.
pub(super) struct Hands {
    pub(super) orchestrator: String,
    executor: String,
    reviewer: String,
}

pub(super) fn hands(fixture: &Fixture) -> Hands {
    let store = fixture.store();
    let handle = |kind: &str| store.register(&NewSession::new(kind)).unwrap().row.handle;
    Hands {
        orchestrator: handle("orchestrator"),
        executor: handle("worker"),
        reviewer: handle("reviewer"),
    }
}

/// The red task `slug` approved naming `commit`, as a red filed unmarked is: claimed by its
/// executor the `first` time, in review, and a fresh reviewer's approve.
pub(super) fn approve(fixture: &Fixture, hands: &Hands, slug: &str, commit: &str, first: bool) {
    let store = fixture.store();
    if first {
        store
            .take_role(slug, &hands.orchestrator, "orchestrator")
            .unwrap();
        store.claim(slug, &hands.executor).unwrap();
    }
    store.move_to(slug, "review", &hands.executor).unwrap();
    (store.take_role(slug, &hands.reviewer, "reviewer")).unwrap();
    store
        .verdict(
            slug,
            &hands.reviewer,
            "approve",
            &format!("{commit}: the fix"),
        )
        .unwrap();
}

/// The branch `slug` started at `release-candidate`, `file` written with `text` (removed with
/// none) and committed: the commit.
pub(super) fn fix(fixture: &Fixture, slug: &str, file: &str, text: Option<&str>) -> String {
    git(
        &fixture.main,
        &["checkout", "-q", "-b", slug, "release-candidate"],
    );
    match text {
        Some(text) => fixture.write(file, text),
        None => fs::remove_file(fixture.main.join(file)).unwrap(),
    }
    let commit = fixture.commit(&format!("fix {slug}"));
    git(&fixture.main, &["checkout", "-q", "main"]);
    commit
}

/// `slug` landed on main as `just land` merges it, and pushed when `push`: the merge.
pub(super) fn land(fixture: &Fixture, slug: &str, push: bool) -> String {
    git(&fixture.main, &["checkout", "-q", "main"]);
    git(
        &fixture.main,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            &format!("Merge branch '{slug}'"),
            slug,
        ],
    );
    if push {
        git(&fixture.main, &["push", "-q", "origin", "main"]);
    }
    git(&fixture.main, &["rev-parse", "HEAD"])
}

/// `release --due SLUG MERGE` with the stand-in check at `now`, as the land's command runs it,
/// which must end with `code`: its stdout.
pub(super) fn due(fixture: &Fixture, now: f64, slug: &str, merge: &str, code: i32) -> String {
    fixture.run(now, &["--check", CHECK, "--due", slug, merge], code)
}

#[test]
fn a_red_run_holds_its_commit_until_its_reds_own_land_answers_it_and_the_rerun_releases_it() {
    let fixture = holding("");
    let red = held(&fixture);
    assert_eq!(branch(&fixture), Some(red.clone()));
    assert!(
        !fixture.remote_refs().contains("release-candidate"),
        "never pushed: {}",
        fixture.remote_refs()
    );
    let candidate = record(&fixture);
    assert_eq!(
        (&candidate["version"], &candidate["state"]),
        (&1.into(), &"open".into())
    );
    assert_eq!(candidate["commit"], red.as_str());
    assert_eq!(candidate["reds"][0]["slug"], RED);
    assert_eq!(candidate["rounds"], 1);
    assert!(
        fixture.reds()[RED].body.contains(&format!(
            "The release candidate: this run holds {} as the commit to release, the local branch \
             `release-candidate` (never pushed)",
            &red[..12]
        )),
        "{}",
        fixture.reds()[RED].body
    );

    // While its red waits, main's tip is not run: not by the schedule, not by `--rerun`.
    let status = fixture.run(NOW + 60.0, &["--status"], 0);
    for said in [
        format!(
            "release candidate: {}, first judged {} by the run ",
            &red[..12],
            &red[..12]
        ),
        format!("  {RED} (verify, round 1): open ("),
        format!("  rerun: waits for {RED}\n"),
        format!(
            "next run: held for the release candidate {}: its rerun waits for {RED}, and main's \
             tip is not run while it is open\n",
            &red[..12]
        ),
    ] {
        assert!(status.contains(&said), "{said}\n\n{status}");
    }
    let runs = fixture.runs().len();
    let out = fixture.night(NOW + 120.0, 0);
    assert!(
        out.ends_with(&format!(
            "release: the release candidate {} waits for {RED}: nothing runs\n",
            &red[..12]
        )),
        "{out}"
    );
    let (code, _, err) = fixture.release_at(NOW + 180.0, &["--check", CHECK, "--rerun"]);
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains(&format!(
            "the release candidate {} waits for {RED}: --rerun reruns it once every red of it is \
             answered",
            &red[..12]
        )),
        "{err}"
    );
    assert_eq!(fixture.runs().len(), runs, "nothing ran");

    // A task forked at the candidate lands: it is no red's, and moves nothing.
    git(
        &fixture.main,
        &["checkout", "-q", "-b", "tools/stray", "release-candidate"],
    );
    fixture.write("stray.txt", "stray");
    fixture.commit("stray work");
    let stray = land(&fixture, "tools/stray", true);
    let out = due(&fixture, NOW + 240.0, "tools/stray", &stray, 0);
    assert!(
        out.contains("next run: held for the release candidate "),
        "{out}"
    );
    assert_eq!(record(&fixture)["commit"], red.as_str());
    assert_eq!(branch(&fixture), Some(red.clone()));
    assert_eq!(fixture.runs().len(), runs);

    // The red's own fix, started at the candidate, approved and landed: the candidate takes it,
    // and its rerun releases the candidate and the fix alone.
    let hands = hands(&fixture);
    let fixed = fix(&fixture, RED, "red", None);
    approve(&fixture, &hands, RED, &fixed, true);
    let merge = land(&fixture, RED, true);
    let out = due(&fixture, NOW + 300.0, RED, &merge, 0);
    for said in [
        format!(
            "release: advanced the release candidate from {} to {}, the newest approved commit of \
             {RED}'s land ({})\n",
            &red[..12],
            &fixed[..12],
            &merge[..12]
        ),
        format!(
            "release: due: the rerun of the release candidate {} starts\n",
            &fixed[..12]
        ),
        format!(
            "release: round 2 of the release candidate of {} (origin/main at {}) in ",
            &fixed[..12],
            &merge[..12]
        ),
        "release: green; tagged release-2026-11-12, pushed to origin\n".into(),
        format!(
            "release: the release candidate {} is released as release-2026-11-12\n",
            &fixed[..12]
        ),
    ] {
        assert!(out.contains(&said), "{said}\n\n{out}");
    }
    assert_eq!(
        fixture.tagged("release-2026-11-12"),
        fixed,
        "the candidate and its fix, not main's tip, which holds the stray task"
    );
    assert_eq!(branch(&fixture), None, "its branch goes with it");
    let candidate = record(&fixture);
    assert_eq!(candidate["state"], "released");
    assert_eq!(candidate["advances"][0]["to"], fixed.as_str());
    assert_eq!(candidate["reds"][0]["answer"]["by"], "landed");
    let rerun = fixture.runs().pop().unwrap();
    assert_eq!(
        (&rerun["commit"], &rerun["tip"]),
        (&fixed.as_str().into(), &merge.as_str().into())
    );
    assert_eq!(rerun["rerun"]["round"], 2);
    assert_eq!(rerun["rerun"]["first"], red.as_str());
    let message = fixture.message("release-2026-11-12");
    for said in [
        format!(
            "Release candidate: first judged {} by the run {} (",
            &red[..12],
            candidate["first_run"].as_str().unwrap()
        ),
        "), released after 1 rerun.\n".into(),
        format!(
            "Its reds and how each was answered:\n- {RED} (verify): advanced it to {}\n",
            &fixed[..12]
        ),
    ] {
        assert!(message.contains(&said), "{said}\n\n{message}");
    }
    let status = fixture.run(NOW + 360.0, &["--status"], 0);
    assert!(
        status.contains("last release candidate: released, "),
        "{status}"
    );
    assert!(
        status.contains(&format!(
            "last run: green, 2026-11-12 10:05 (1 min ago), on {} (round 2 of the release \
             candidate first judged at {})",
            &fixed[..12],
            &red[..12]
        )),
        "{status}"
    );
}

/// A red whose assessment needs no review lands from review with no approve: the candidate takes
/// the tip its landing's line says its needed stamps stood on, as it takes an approved commit.
#[test]
fn a_reds_land_by_its_needed_stamps_alone_advances_it_to_the_tip_they_stood_on() {
    let fixture = holding("");
    let red = held(&fixture);
    let hands = hands(&fixture);
    let fixed = fix(&fixture, RED, "red", None);
    let store = fixture.store();
    (store.take_role(RED, &hands.orchestrator, "orchestrator")).unwrap();
    store.claim(RED, &hands.executor).unwrap();
    store.move_to(RED, "review", &hands.executor).unwrap();
    let merge = land(&fixture, RED, true);
    let needed = symphony_board::land::needs::Needed {
        stamps: vec!["tests".into()],
        ..Default::default()
    };
    let line = format!(
        "landed {RED} on main at {}{}",
        &merge[..12],
        needed.stood(&fixed)
    );
    (store.landed(RED, &hands.orchestrator, &line, false, &["review"])).unwrap();
    let out = due(&fixture, NOW + 300.0, RED, &merge, 0);
    assert!(
        out.contains(&format!(
            "release: advanced the release candidate from {} to {}, the newest approved commit of \
             {RED}'s land",
            &red[..12],
            &fixed[..12]
        )),
        "{out}"
    );
    assert_eq!(fixture.tagged("release-2026-11-12"), fixed);
}

#[test]
fn a_land_the_remote_lacks_yet_waits_for_its_push() {
    let fixture = holding("");
    let red = held(&fixture);
    let hands = hands(&fixture);
    let fixed = fix(&fixture, RED, "red", None);
    approve(&fixture, &hands, RED, &fixed, true);
    let merge = land(&fixture, RED, false);
    let out = due(&fixture, NOW + 300.0, RED, &merge, 0);
    assert!(
        out.contains(&format!(
            "release: {RED} landed here ({}), but origin/main does not hold it yet: the release \
             candidate waits for its push\n",
            &merge[..12]
        )),
        "{out}"
    );
    assert!(
        out.contains("next run: held for the release candidate"),
        "{out}"
    );
    assert_eq!(record(&fixture)["commit"], red.as_str());
    assert_eq!(branch(&fixture), Some(red.clone()));
    let status = fixture.run(NOW + 360.0, &["--status"], 0);
    assert!(
        status.contains(&format!(
            "  {RED} (verify, round 1): open (landed here, not pushed)\n"
        )),
        "{status}"
    );

    // Its retry pushed: the next count advances and reruns it.
    git(&fixture.main, &["push", "-q", "origin", "main"]);
    due(&fixture, NOW + 420.0, RED, &merge, 0);
    assert_eq!(fixture.tagged("release-2026-11-12"), fixed);
}

#[test]
fn the_red_reruns_bound_drops_it_and_main_is_run_at_once() {
    let fixture = holding("candidate_reruns = 1\n");
    held(&fixture);
    let hands = hands(&fixture);
    // A fix that fixes nothing.
    let missed = fix(&fixture, RED, "notes.txt", Some("not the fix"));
    approve(&fixture, &hands, RED, &missed, true);
    let merge = land(&fixture, RED, true);
    let out = due(&fixture, NOW + 300.0, RED, &merge, 1);
    assert!(
        out.contains(&format!(
            "release: the release candidate {} is dropped: 1 rerun came back red, as many as \
             [releases] candidate_reruns allows; the next run checks main's tip\n",
            &missed[..12]
        )),
        "{out}"
    );
    assert_eq!(branch(&fixture), None);
    let candidate = record(&fixture);
    assert_eq!(candidate["state"], "dropped");
    assert_eq!(candidate["ended"]["due"], true);
    let filed = &fixture.reds()["tests/release-red-verify-2"].body;
    assert!(
        filed.starts_with(&format!(
            "The Symphony release run is red in `verify` on the release candidate at {} (run ",
            &missed[..12]
        )),
        "what it checked: {filed}"
    );
    assert!(
        !filed.contains("this run holds") && !filed.contains("`release-candidate`"),
        "a red of a dropped candidate is main's, fixed there: {filed}"
    );

    // The drop made a run of main's tip due, whatever the count: it runs, and holds main's tip.
    let out = fixture.run(NOW + 360.0, &["--check", CHECK, "--due"], 1);
    assert!(
        out.contains(
            "release: due: the release candidate was dropped, so a run on origin/main's tip starts\n"
        ),
        "{out}"
    );
    let candidate = record(&fixture);
    assert_eq!(candidate["state"], "open");
    assert_eq!(candidate["first"], merge.as_str(), "main's tip, held anew");
    assert_eq!(branch(&fixture), Some(merge));
}

#[test]
fn the_hours_bound_drops_it_and_main_is_run_at_once() {
    let fixture = holding("candidate_hours = 1\n");
    held(&fixture);
    // Fixed on main meanwhile, by no red's task.
    let fixed = fixture.land("red", None, "fixed on main");
    let out = fixture.run(NOW + 1800.0, &["--check", CHECK, "--due"], 0);
    assert!(
        out.contains("next run: held for the release candidate"),
        "within its hour: {out}"
    );
    let out = fixture.run(NOW + 3600.0, &["--check", CHECK, "--due"], 0);
    for said in [
        "release: dropped the release candidate: it was first judged 1 hour ago, at 2026-11-12 \
         10:00: [releases] candidate_hours holds one 1\n",
        "release: due: the release candidate was dropped, so a run on origin/main's tip starts\n",
        "release: green; tagged release-2026-11-12, pushed to origin\n",
    ] {
        assert!(out.contains(said), "{said}\n\n{out}");
    }
    assert_eq!(fixture.tagged("release-2026-11-12"), fixed);
    let candidate = record(&fixture);
    assert_eq!(
        (&candidate["state"], &candidate["ended"]["due"]),
        (&"dropped".into(), &false.into()),
        "the run its drop made due started"
    );
}

#[test]
fn a_release_on_main_that_passes_it_drops_it() {
    let fixture = holding("");
    let red = held(&fixture);
    let fixed = fixture.land("red", None, "fixed on main");
    // Released out of cycle, which holds no candidate and runs whatever one is held.
    fixture.run(NOW + 60.0, &["--check", CHECK, "--commit", &fixed], 0);
    assert_eq!(fixture.tagged("release-2026-11-12"), fixed);
    assert_eq!(record(&fixture)["state"], "open");
    let out = fixture.night(NOW + 120.0, 0);
    assert!(
        out.contains(&format!(
            "release: dropped the release candidate: the release release-2026-11-12 ({}) passed \
             it: its tip {} does not descend from it, and a release only moves forward\n",
            &fixed[..12],
            &red[..12]
        )),
        "{out}"
    );
    assert!(
        out.contains("release: origin/main is release-2026-11-12 already"),
        "main's tip run at once: {out}"
    );
    assert_eq!(record(&fixture)["state"], "dropped");
    assert_eq!(
        record(&fixture)["ended"]["due"],
        false,
        "main's tip released already: the run its drop made due is followed"
    );
    assert_eq!(branch(&fixture), None);
}

#[test]
fn a_red_closed_as_covered_by_another_reds_land_is_answered_by_it_and_the_rerun_releases() {
    let fixture = holding("");
    let outside = fixture.root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let script = STEPS.replace("OUTSIDE", &outside.to_string_lossy());
    fixture.write("steps.sh", &script);
    fixture.write("red-lint", "red");
    fixture.write("red-tests", "red");
    let red = fixture.commit("two reds");
    git(&fixture.main, &["push", "-q", "origin", "main"]);
    let verify = "verify=sh steps.sh";
    fixture.run(NOW, &["--check", verify], 1);
    assert_eq!(record(&fixture)["commit"], red.as_str());
    let (lint, tests) = (
        "tests/release-red-verify-lint",
        "tests/release-red-verify-tests",
    );

    // The lint red's fix fixes both; the tests red is closed as covered by its landing.
    git(
        &fixture.main,
        &["checkout", "-q", "-b", lint, "release-candidate"],
    );
    for file in ["red-lint", "red-tests"] {
        fs::remove_file(fixture.main.join(file)).unwrap();
    }
    let fixed = fixture.commit("fix both reds");
    git(&fixture.main, &["checkout", "-q", "main"]);
    let hands = hands(&fixture);
    approve(&fixture, &hands, lint, &fixed, true);
    let merge = land(&fixture, lint, true);
    let store = fixture.store();
    let landed = format!("landed {lint} on main at {}", &merge[..12]);
    (store.landed(lint, &hands.orchestrator, &landed, false, &["done"])).unwrap();
    (store.take_role(tests, &hands.orchestrator, "orchestrator")).unwrap();
    let note = "its fix landed with the lint red's";
    (store.close_covered(tests, &hands.orchestrator, lint, &merge, note)).unwrap();

    let out = due(&fixture, NOW + 300.0, lint, &merge, 0);
    for said in [
        format!(
            "release: {tests} is closed as covered by {lint}'s land ({}), which advanced the \
             release candidate to {}: its red is answered\n",
            &merge[..12],
            &fixed[..12]
        ),
        "release: green; tagged release-2026-11-12, pushed to origin\n".into(),
    ] {
        assert!(out.contains(&said), "{said}\n\n{out}");
    }
    assert_eq!(fixture.tagged("release-2026-11-12"), fixed);
    assert_eq!(record(&fixture)["state"], "released");
    let message = fixture.message("release-2026-11-12");
    assert!(
        message.contains(&format!(
            "- {tests} (verify/tests): covered by {lint}'s land, which advanced it to {}\n",
            &fixed[..12]
        )),
        "{message}"
    );
}

#[test]
fn a_drops_run_of_main_stopped_before_its_checks_is_asked_for_again() {
    let fixture = holding("");
    held(&fixture);
    fixture.run(NOW + 60.0, &["--drop"], 0);
    // The remote cannot be fetched: the run stops before its first check.
    let origin = fixture.origin.to_string_lossy().into_owned();
    let nowhere = fixture.root.join("nowhere");
    fs::create_dir_all(&nowhere).unwrap();
    let unset = ["remote", "set-url", "origin", nowhere.to_str().unwrap()];
    git(&fixture.main, &unset);
    let (code, out, err) = fixture.release_at(NOW + 120.0, &["--check", CHECK, "--due"]);
    assert_eq!(code, 1, "{out}\n{err}");
    assert!(
        out.contains(
            "release: due: the release candidate was dropped, so a run on origin/main's tip starts\n"
        ),
        "{out}"
    );
    assert!(fixture.reds().contains_key("tests/release-red-run"));
    assert_eq!(
        record(&fixture)["ended"]["due"],
        true,
        "a run that reached no check does not follow the drop"
    );

    git(&fixture.main, &["remote", "set-url", "origin", &origin]);
    let out = fixture.run(NOW + 180.0, &["--check", CHECK, "--due"], 1);
    assert!(
        out.contains(
            "release: due: the release candidate was dropped, so a run on origin/main's tip starts\n"
        ),
        "asked for again: {out}"
    );
    let candidate = record(&fixture);
    assert_eq!(
        candidate["state"], "open",
        "main's tip, red, is held anew: {candidate:#}"
    );
}

/// A stand-in verify that runs the steps it is named (all of lint, size-check and tests when none
/// is), each listed in the folder the run names; a step is red while the checkout holds
/// `red-STEP` or the folder `OUTSIDE` does, and each step that runs is written in `OUTSIDE/ran`.
const STEPS: &str = r#"d="$SYMPHONY_CHECK_STEPS"
code=0
for step in ${*:-lint size-check tests}; do
    if [ -e "red-$step" ] || [ -e "OUTSIDE/red-$step" ]; then
        status=1; code=1; echo "$step said it is red" > "$d/$step.log"
    else
        status=0; echo "$step green" > "$d/$step.log"
    fi
    printf '%s\t%s\t1\t%s\tjust %s\n' "$step" "$status" "$step.log" "$step" >> "$d/steps.tsv"
    echo "$step" >> "OUTSIDE/ran"
done
exit $code
"#;

#[test]
fn a_rerun_on_the_same_tree_runs_its_red_steps_alone_and_carries_the_rest_green() {
    let fixture = holding("");
    let outside = fixture.root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let script = STEPS.replace("OUTSIDE", &outside.to_string_lossy());
    fixture.land("steps.sh", Some(&script), "a verify that lists its steps");
    fs::write(outside.join("red-tests"), "red").unwrap();
    let tip = git(&fixture.main, &["rev-parse", "HEAD"]);
    let verify = "verify=sh steps.sh";
    fixture.run(NOW, &["--check", verify], 1);
    let red = "tests/release-red-verify-tests";
    assert_eq!(record(&fixture)["reds"][0]["slug"], red);
    assert_eq!(
        fs::read_to_string(outside.join("ran")).unwrap(),
        "lint\nsize-check\ntests\n"
    );

    // Its fix is made outside this repository, and its task cancelled saying where.
    fs::remove_file(outside.join("red-tests")).unwrap();
    let hands = hands(&fixture);
    let store = fixture.store();
    store
        .take_role(red, &hands.orchestrator, "orchestrator")
        .unwrap();
    store
        .move_with(
            red,
            "cancelled",
            &hands.orchestrator,
            Some("fixed outside this repository"),
        )
        .unwrap();
    let out = fixture.run(NOW + 60.0, &["--check", verify, "--rerun"], 0);
    for said in [
        format!(
            "release: {red} is cancelled with no landing: its red is answered, its fix made \
             outside this repository\n"
        ),
        "release: `verify` reruns tests, the rest carried green from this tree\n".into(),
        "release: green; tagged release-2026-11-12, pushed to origin\n".into(),
    ] {
        assert!(out.contains(&said), "{said}\n\n{out}");
    }
    assert_eq!(
        fs::read_to_string(outside.join("ran")).unwrap(),
        "lint\nsize-check\ntests\ntests\n",
        "the red step alone ran again"
    );
    assert_eq!(fixture.tagged("release-2026-11-12"), tip);
    let rerun = fixture.runs().pop().unwrap();
    assert_eq!(rerun["checks"][0]["command"], "sh steps.sh tests");
    let carried: Vec<&str> = (rerun["rerun"]["carried"].as_array().unwrap().iter())
        .map(|kept| kept["name"].as_str().unwrap())
        .collect();
    assert_eq!(carried, ["verify/lint", "verify/size-check"]);
    let message = fixture.message("release-2026-11-12");
    assert!(message.contains("Green: verify.\n"), "{message}");
    assert!(
        message.contains(&format!(
            "- {red} (verify/tests): answered outside this repository\n\
             Carried green from a commit whose tree is this one's:\n\
             - verify/lint: green on {}",
            &tip[..12]
        )),
        "{message}"
    );
}

#[test]
fn rerun_and_drop_are_refused_with_no_candidate_and_a_drop_makes_main_due() {
    let fixture = holding("");
    let (code, _, err) = fixture.release_at(NOW, &["--check", CHECK, "--rerun"]);
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("no release candidate is held, so there is none to rerun"),
        "{err}"
    );
    let (code, _, err) = fixture.release_at(NOW, &["--drop"]);
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("no release candidate is held, so there is none to drop"),
        "{err}"
    );
    let (code, _, err) = fixture.release_at(NOW, &["--drop", "--rerun"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("are one each"), "{err}");
    assert!(fixture.runs().is_empty(), "nothing ran");

    let red = held(&fixture);
    let first = record(&fixture)["first_run"].clone();
    let out = fixture.run(NOW + 60.0, &["--drop"], 0);
    assert_eq!(
        out,
        format!(
            "release: dropped the release candidate {}: the next landing starts a run of main's \
             tip, or `just release`\n",
            &red[..12]
        )
    );
    assert_eq!(branch(&fixture), None);
    let status = fixture.run(NOW + 120.0, &["--status"], 0);
    for said in [
        ": dropped by `just release --drop`; a run of main's tip is due; first judged ",
        "next run: due: the release candidate was dropped (dropped by `just release --drop`): the \
         next landing starts a run of origin/main's tip, or `just release --due`\n",
    ] {
        assert!(status.contains(said), "{said}\n\n{status}");
    }
    let (code, _, err) = fixture.release_at(NOW + 180.0, &["--drop"]);
    assert_eq!(code, 1, "dropped once: {err}");

    let out = fixture.run(NOW + 240.0, &["--check", CHECK, "--due"], 1);
    assert!(
        out.contains(
            "release: due: the release candidate was dropped, so a run on origin/main's tip starts\n"
        ),
        "{out}"
    );
    let candidate = record(&fixture);
    assert_eq!(candidate["state"], "open", "main's tip, red, is held anew");
    assert_ne!(candidate["first_run"], first);
}

#[test]
fn a_record_this_build_cannot_read_stops_the_run_and_is_left_as_it_is() {
    let fixture = holding("");
    fs::create_dir_all(fixture.records()).unwrap();
    let newer = "{\"version\": 9, \"state\": \"open\"}\n";
    fs::write(fixture.records().join("candidate.json"), newer).unwrap();
    let (code, _, err) = fixture.release_at(NOW, &["--check", CHECK]);
    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("is of version 9, a newer build's than this one (1)"),
        "{err}"
    );
    assert_eq!(
        fs::read_to_string(fixture.records().join("candidate.json")).unwrap(),
        newer
    );
    assert!(
        fixture.reds().contains_key("tests/release-red-run"),
        "the board hears of it: {:?}",
        fixture.reds().keys()
    );
    assert!(fixture.remote_tags().is_empty());
}
