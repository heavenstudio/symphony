//! `symphony-github verify`, what a release run asks to have its commit verified on GitHub's
//! machines: the workflow started once for that commit on the default branch, its run waited
//! for and believed only as the bound repository's, that workflow's and that commit's, each job's
//! log and each artifact downloaded, and nothing started where it is turned off, the commit is
//! not there or GitHub refuses; a GitHub that does not answer is said as that, never as a result.

use crate::github::{Fault, SIGNATURE};
use crate::{Ran, Rig, TOKEN};
use serde_json::{Value, json};
use std::path::PathBuf;
use symphony_github::state::Event;

const COMMIT: &str = "cccccccccccccccccccccccccccccccccccccccc";
const DISPATCH: &str = "POST /repos/o/n/actions/workflows/release-verify.yml/dispatches";

/// A rig serving hosted verification, the commit pushed, a run of two jobs to come.
fn rig(name: &str) -> Rig {
    let rig = Rig::new(name, "verification = \"hosted\"");
    rig.github.actions(|actions| {
        actions.commits.push(COMMIT.into());
        actions.jobs = vec![
            ("The commit's steps", "success", Some("steps: [lint]\n")),
            ("lint", "success", Some("verify: lint: green (12 s)\n")),
        ];
        actions.artifacts = vec![("verify-lint", "PK-lint")];
    });
    rig
}

/// The folder a run's logs are downloaded into.
fn folder(rig: &Rig) -> PathBuf {
    let folder = rig.dir.0.join("hosted");
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

fn request(rig: &Rig) -> Value {
    json!({"commit": COMMIT, "folder": folder(rig), "by": "release run"})
}

/// `verify --request FILE` of `request`: what it ran as.
fn run(rig: &Rig, request: &Value) -> Ran {
    let file = rig.dir.0.join("verify.json");
    std::fs::write(&file, request.to_string()).unwrap();
    rig.run(&["verify", "--request", file.to_str().unwrap()])
}

/// The same, which must answer one line of JSON holding no token and no download's signature.
fn ask(rig: &Rig, request: &Value) -> Value {
    let ran = run(rig, request).ok();
    assert_eq!(ran.out.lines().count(), 1, "one line of JSON: {}", ran.out);
    assert!(!ran.out.contains(TOKEN), "{}", ran.out);
    assert!(!ran.out.contains(SIGNATURE), "{}", ran.out);
    serde_json::from_str(&ran.out).unwrap()
}

/// The requests made that change the repository.
fn writes(rig: &Rig) -> Vec<String> {
    (rig.github.asked().into_iter())
        .filter(|(method, ..)| method != "GET")
        .map(|(method, path, _)| format!("{method} {path}"))
        .collect()
}

fn events(rig: &Rig) -> Vec<Event> {
    (rig.journal().iter())
        .filter(|entry| entry.kind == "verify")
        .map(|entry| entry.event)
        .collect()
}

/// An answer that verified nothing, with why.
fn stopped(answer: &Value, state: &str, why: &str) {
    assert_eq!(answer["state"], state, "{answer}");
    let said = answer["why"].as_str().unwrap_or_default();
    assert!(said.contains(why), "{said}");
}

#[test]
fn a_commit_is_verified_by_one_run_started_for_it_whose_logs_are_downloaded() {
    let rig = rig("verify-once");
    let answer = ask(&rig, &request(&rig));
    assert_eq!(answer["state"], "ended", "{answer}");
    assert_eq!(answer["commit"], COMMIT);
    assert_eq!(
        answer["run"],
        json!({
            "id": 9000, "url": "https://github.com/o/n/actions/runs/9000", "attempt": 1,
            "conclusion": "success", "workflow_commit": "f".repeat(40),
            "started": "2026-01-01T00:00:00Z", "ended": "2026-01-01T00:49:00Z",
        })
    );
    assert_eq!(
        answer["jobs"],
        json!([
            {"id": 500, "name": "The commit's steps", "conclusion": "success", "seconds": 90,
             "url": "https://github.com/o/n/actions/job/500", "log": "job-500.log",
             "no_log": null},
            {"id": 501, "name": "lint", "conclusion": "success", "seconds": 90,
             "url": "https://github.com/o/n/actions/job/501", "log": "job-501.log",
             "no_log": null},
        ])
    );
    assert_eq!(
        answer["artifacts"],
        json!([{"id": 700, "name": "verify-lint", "file": "artifact-700.zip"}])
    );
    assert_eq!(answer["missing"], json!([]));
    let read = |name: &str| std::fs::read_to_string(folder(&rig).join(name)).unwrap();
    assert_eq!(read("job-501.log"), "verify: lint: green (12 s)\n");
    assert_eq!(read("artifact-700.zip"), "PK-lint");
    // One write, the dispatch, on the default branch for the commit alone.
    assert_eq!(writes(&rig), [DISPATCH]);
    let sent = (rig.github.asked().into_iter())
        .find(|(method, ..)| method == "POST")
        .and_then(|(.., body)| body)
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&sent).unwrap(),
        json!({"ref": "main", "inputs": {"commit": COMMIT}})
    );
    assert_eq!(events(&rig), [Event::Intent, Event::Ack]);
    let journal = serde_json::to_string(&rig.journal()).unwrap();
    assert!(!journal.contains(TOKEN) && !journal.contains(SIGNATURE));
    // Its lock is free once it answered: a land's delivery takes the same one.
    assert!(rig.store().lock().is_ok());
}

#[test]
fn nothing_is_asked_of_github_while_verification_is_not_turned_on() {
    for more in ["", "releases = \"publish\"", "verification = \"off\""] {
        let rig = rig("verify-off");
        rig.enable(more);
        stopped(&ask(&rig, &request(&rig)), "refused", "turned off");
        assert!(rig.github.asked().is_empty(), "{more}");
    }
    let rig = rig("verify-absent");
    rig.settings.set(None);
    stopped(&ask(&rig, &request(&rig)), "refused", "turned off");
    assert_eq!(rig.github.asked().len(), 0, "{:?}", rig.github.asked());
}

#[test]
fn a_verification_turned_off_names_its_own_setting_and_unread_settings_say_why() {
    let rig = rig("verify-off-said");
    rig.enable("releases = \"publish\"");
    assert_eq!(
        ask(&rig, &request(&rig)),
        json!({
            "state": "refused",
            "why": "turned off: the committed settings do not say enabled = true and \
                    verification = \"hosted\"",
        })
    );
    rig.settings
        .set(Some("verification = \"sometimes\"\n".into()));
    let answer = ask(&rig, &request(&rig));
    let why = answer["why"].as_str().unwrap();
    assert!(
        why.starts_with("turned off: its settings do not read: ") && why.contains("sometimes"),
        "{answer}"
    );
    assert!(rig.github.asked().is_empty());
}

#[test]
fn a_request_that_names_no_whole_commit_or_no_folder_starts_nothing() {
    let rig = rig("verify-request");
    for commit in ["main", "cccccccc", "", &"g".repeat(40)] {
        let asked = json!({"commit": commit, "folder": folder(&rig)});
        stopped(&ask(&rig, &asked), "refused", "40 hexadecimal digits");
    }
    for at in [
        json!("relative"),
        json!(rig.dir.0.join("absent")),
        Value::Null,
    ] {
        let asked = json!({"commit": COMMIT, "folder": at});
        stopped(&ask(&rig, &asked), "refused", "folder is the absolute path");
    }
    let asked = json!({"commit": COMMIT, "folder": folder(&rig), "seconds": "long"});
    stopped(&ask(&rig, &asked), "refused", "seconds is how long");
    assert_eq!(rig.github.asked().len(), 0, "{:?}", rig.github.asked());
    // A request that is no JSON object asks nothing and is an error of the command line.
    let file = rig.dir.0.join("verify.json");
    std::fs::write(&file, "[]").unwrap();
    let ran = rig.run(&["verify", "--request", file.to_str().unwrap()]);
    assert_ne!(ran.code, 0);
    assert!(
        ran.err.contains("a request is one JSON object"),
        "{}",
        ran.err
    );
    assert_ne!(rig.run(&["verify"]).code, 0);
    assert_ne!(
        rig.run(&["verify", "--request", file.to_str().unwrap(), "more"])
            .code,
        0
    );
}

#[test]
fn a_repository_that_is_not_the_one_bound_is_not_asked_to_verify() {
    let rig = rig("verify-bound");
    assert_eq!(ask(&rig, &request(&rig))["state"], "ended");
    rig.github.replace_repository(78);
    stopped(
        &ask(&rig, &request(&rig)),
        "refused",
        "bound to o/n (id 77)",
    );
    assert_eq!(writes(&rig), [DISPATCH], "no second dispatch");
}

#[test]
fn a_commit_github_does_not_hold_or_a_branch_without_the_workflow_starts_nothing() {
    let rig = rig("verify-absent-commit");
    rig.github.actions(|actions| actions.commits.clear());
    stopped(
        &ask(&rig, &request(&rig)),
        "unavailable",
        "o/n does not hold the commit cccc",
    );
    assert_eq!(writes(&rig).len(), 0, "{:?}", writes(&rig));
    let rig = self::rig("verify-no-workflow");
    rig.github.actions(|actions| actions.workflow = false);
    stopped(
        &ask(&rig, &request(&rig)),
        "refused",
        "o/n has no workflow release-verify.yml on its default branch",
    );
    assert_eq!(writes(&rig).len(), 0, "{:?}", writes(&rig));
    assert_eq!(events(&rig).len(), 0, "{:?}", events(&rig));
}

#[test]
fn a_dispatch_github_refuses_is_said_with_what_it_needs_and_journaled_failed() {
    for (status, state, why) in [
        (
            403,
            "refused",
            "token_file's token needs actions read and write",
        ),
        (
            404,
            "refused",
            "token_file's token needs actions read and write",
        ),
        (
            422,
            "refused",
            "GitHub refused to start release-verify.yml (422): planted",
        ),
        (400, "unavailable", "GitHub answered 400 to the start"),
    ] {
        let rig = rig("verify-refused");
        (rig.github).fault(
            "POST",
            "/repos/o/n/actions/workflows",
            Fault::Status(status, vec![]),
        );
        stopped(&ask(&rig, &request(&rig)), state, why);
        assert_eq!(events(&rig), [Event::Intent, Event::Failed], "{status}");
        assert!(rig.github.actions(|actions| actions.runs.is_empty()));
    }
}

#[test]
fn a_dispatch_whose_answer_is_lost_is_looked_for_and_read_when_it_shows() {
    let rig = rig("verify-lost");
    (rig.github).fault("POST", "/repos/o/n/actions/workflows", Fault::LoseAnswer);
    let answer = ask(&rig, &request(&rig));
    assert_eq!(answer["state"], "ended", "{answer}");
    assert_eq!(events(&rig), [Event::Intent, Event::Unknown, Event::Ack]);
    // One that never reached GitHub shows no run: nothing is read as its result.
    let rig = self::rig("verify-lost-unsent");
    (rig.github).fault(
        "POST",
        "/repos/o/n/actions/workflows",
        Fault::Status(502, vec![]),
    );
    stopped(
        &ask(&rig, &request(&rig)),
        "unavailable",
        "GitHub listed no run of it within 120 s",
    );
    assert_eq!(events(&rig), [Event::Intent, Event::Unknown]);
}

#[test]
fn a_run_that_never_shows_or_cannot_be_read_is_unavailable_and_never_a_result() {
    let rig = rig("verify-unlisted");
    rig.github.actions(|actions| actions.listed = false);
    stopped(
        &ask(&rig, &request(&rig)),
        "unavailable",
        "GitHub listed no run of it within 120 s",
    );
    // A run titled for another commit is not this request's.
    let rig = self::rig("verify-other-title");
    (rig.github)
        .actions(|actions| actions.over = json!({"display_title": "Release verification of main"}));
    stopped(
        &ask(&rig, &request(&rig)),
        "unavailable",
        "GitHub listed no run",
    );
    let rig = self::rig("verify-unread");
    (rig.github).fault(
        "GET",
        "/repos/o/n/actions/runs/9000",
        Fault::Status(404, vec![]),
    );
    stopped(
        &ask(&rig, &request(&rig)),
        "unavailable",
        "reading the run 9000: GitHub answered 404",
    );
    let rig = self::rig("verify-no-jobs");
    (rig.github).fault(
        "GET",
        "/repos/o/n/actions/runs/9000/jobs",
        Fault::Status(500, vec![]),
    );
    stopped(
        &ask(&rig, &request(&rig)),
        "unavailable",
        "GitHub answered 500",
    );
    let rig = self::rig("verify-no-artifacts");
    (rig.github).fault(
        "GET",
        "/repos/o/n/actions/runs/9000/artifacts",
        Fault::Status(404, vec![]),
    );
    stopped(
        &ask(&rig, &request(&rig)),
        "unavailable",
        "listing the artifacts of the run 9000: GitHub answered 404",
    );
}

#[test]
fn a_run_still_going_is_waited_for_and_one_past_its_time_is_late_not_a_result() {
    let rig = rig("verify-waits");
    rig.github.actions(|actions| actions.pending = 3);
    // A read GitHub leaves unanswered while it waits is read again.
    (rig.github).fault(
        "GET",
        "/repos/o/n/actions/runs/9000",
        Fault::Status(502, vec![]),
    );
    let answer = ask(&rig, &request(&rig));
    assert_eq!(answer["state"], "ended", "{answer}");
    let reads = (rig.github.asked().iter())
        .filter(|(_, path, _)| path == "/repos/o/n/actions/runs/9000")
        .count();
    assert_eq!(reads, 5, "one unanswered, three in progress, one ended");

    let rig = self::rig("verify-late");
    rig.github.actions(|actions| actions.pending = 100);
    let mut asked = request(&rig);
    asked["seconds"] = json!(60);
    let answer = ask(&rig, &asked);
    stopped(&answer, "late", "the run 9000 had not ended within 60 s");
    assert_eq!(answer["run"]["id"], 9000);
    assert_eq!(answer["run"]["conclusion"], Value::Null);
    assert!(
        answer["jobs"].is_null(),
        "no job is read of a run not ended"
    );

    let rig = self::rig("verify-unanswered");
    for _ in 0..20 {
        (rig.github).fault(
            "GET",
            "/repos/o/n/actions/runs/9000",
            Fault::Status(502, vec![]),
        );
    }
    stopped(
        &ask(&rig, &request(&rig)),
        "unavailable",
        "the run 9000 could not be read 20 times in a row: GitHub answered 502",
    );
}

#[test]
fn a_run_github_says_is_anothers_is_not_believed() {
    for (over, why) in [
        (
            json!({"repository": {"id": 5}}),
            "it is of the repository 5, not the one bound here (77)",
        ),
        (
            json!({"path": ".github/workflows/other.yml"}),
            "it ran the workflow \".github/workflows/other.yml\"",
        ),
        (json!({"event": "push"}), "it was started by \"push\""),
        (
            json!({"head_branch": "feature"}),
            "it ran the workflow of \"feature\", not of the default branch main",
        ),
        (
            json!({"head_sha": "main"}),
            "it names no commit its workflow file is of",
        ),
        (json!({"conclusion": null}), "it has not ended"),
    ] {
        let rig = rig("verify-disbelieved");
        rig.github.actions(|actions| actions.over = over.clone());
        let answer = ask(&rig, &request(&rig));
        stopped(&answer, "refused", "the run 9000 is not believed: ");
        stopped(&answer, "refused", why);
        assert!(answer["jobs"].is_null(), "{answer}");
        assert!(std::fs::read_dir(folder(&rig)).unwrap().next().is_none());
    }
}

// A run the workflow's list names is found by its title, so one titled for another commit is
// never taken; the run a dispatch's own answer names is read as it is, and its title judged.
#[test]
fn a_run_the_dispatch_names_is_read_without_a_search_and_believed_only_titled_for_the_commit() {
    let named = rig("verify-named");
    named.github.actions(|actions| actions.named = true);
    let answer = ask(&named, &request(&named));
    assert_eq!(answer["state"], "ended", "{answer}");
    assert_eq!(answer["run"]["id"], 9000);
    let lists = (named.github.asked().iter())
        .filter(|(_, path, _)| path.contains("/actions/workflows/release-verify.yml/runs"))
        .count();
    assert_eq!(
        lists,
        1,
        "the list before the start alone: {:?}",
        named.github.asked()
    );

    let rig = rig("verify-named-other");
    rig.github.actions(|actions| {
        actions.named = true;
        actions.over = json!({
            "display_title": format!("Release verification of {}", "a".repeat(40)),
        });
    });
    let answer = ask(&rig, &request(&rig));
    stopped(
        &answer,
        "refused",
        "the run 9000 is not believed: it is titled ",
    );
    stopped(
        &answer,
        "refused",
        &format!(
            "not for the commit {}",
            request(&rig)["commit"].as_str().unwrap()
        ),
    );
    assert!(answer["jobs"].is_null(), "{answer}");
    assert!(std::fs::read_dir(folder(&rig)).unwrap().next().is_none());
}

#[test]
fn a_red_run_is_answered_as_it_ended_each_job_as_github_concluded_it() {
    let rig = rig("verify-red");
    rig.github.actions(|actions| {
        actions.conclusion = "failure";
        actions.jobs = vec![
            ("lint", "failure", Some("red\n")),
            ("tests", "cancelled", None),
            ("hosted", "timed_out", Some("late\n")),
        ];
        actions.artifacts = vec![("verify-lint", "PK"), ("verify-hosted", "PK")];
    });
    (rig.github).fault(
        "GET",
        "/repos/o/n/actions/artifacts/701",
        Fault::Status(410, vec![]),
    );
    let answer = ask(&rig, &request(&rig));
    assert_eq!(answer["state"], "ended", "{answer}");
    assert_eq!(answer["run"]["conclusion"], "failure");
    let jobs: Vec<(&str, &str, Option<&str>)> = (answer["jobs"].as_array().unwrap().iter())
        .map(|job| {
            (
                job["name"].as_str().unwrap(),
                job["conclusion"].as_str().unwrap(),
                job["log"].as_str(),
            )
        })
        .collect();
    assert_eq!(
        jobs,
        [
            ("lint", "failure", Some("job-500.log")),
            ("tests", "cancelled", None),
            ("hosted", "timed_out", Some("job-502.log")),
        ]
    );
    assert_eq!(
        answer["jobs"][1]["no_log"],
        "GitHub answered 404 and no download"
    );
    // An artifact that cannot be downloaded is said, and the rest are kept.
    assert_eq!(answer["artifacts"].as_array().unwrap().len(), 1);
    assert_eq!(
        answer["missing"],
        json!(["verify-hosted: GitHub answered 410 and no download"])
    );
}

#[test]
fn a_read_before_the_start_that_github_does_not_answer_starts_nothing() {
    for (path, why) in [
        ("/repos/o/n/commits/", "reading the commit cccc"),
        (
            "/repos/o/n/actions/workflows/release-verify.yml/runs",
            "listing the runs of release-verify.yml: GitHub answered 400",
        ),
    ] {
        let rig = rig("verify-unread-before");
        (rig.github).fault("GET", path, Fault::Status(400, vec![]));
        stopped(&ask(&rig, &request(&rig)), "unavailable", why);
        assert!(writes(&rig).is_empty(), "{path}");
        assert!(events(&rig).is_empty(), "{path}");
    }
    // A repository that names no default branch is asked to start nothing either.
    let rig = rig("verify-no-branch");
    (rig.github).set_repository(&json!({"default_branch": "-x y"}));
    stopped(
        &ask(&rig, &request(&rig)),
        "unavailable",
        "the repository names no default branch",
    );
    assert_eq!(writes(&rig).len(), 0, "{:?}", writes(&rig));
}

#[test]
fn while_another_run_writes_the_state_nothing_is_started() {
    let rig = rig("verify-second-writer");
    assert_eq!(ask(&rig, &request(&rig))["state"], "ended");
    let held = rig.store().lock().unwrap();
    stopped(
        &ask(&rig, &request(&rig)),
        "unavailable",
        "one writer at a time",
    );
    drop(held);
    assert_eq!(writes(&rig), [DISPATCH], "no second dispatch");
}

#[test]
fn a_second_request_starts_a_run_of_its_own_and_reads_that_one() {
    let rig = rig("verify-twice");
    assert_eq!(ask(&rig, &request(&rig))["run"]["id"], 9000);
    assert_eq!(ask(&rig, &request(&rig))["run"]["id"], 9001);
    assert_eq!(writes(&rig), [DISPATCH, DISPATCH]);
}

#[test]
fn status_says_hosted_verification_only_where_it_is_turned_on() {
    let rig = rig("verify-status");
    let on = rig.run(&["status"]).ok().out;
    assert!(on.contains("verification: hosted"), "{on}");
    rig.enable("");
    let off = rig.run(&["status"]).ok().out;
    assert!(!off.contains("verification"), "{off}");
}
