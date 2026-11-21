//! A delivery GitHub merges by itself: checks pending while the evidence is short of standing and
//! failing where it is red or withdrawn, `arm` turning on GitHub's auto-merge at the head, which
//! merges once `symphony/eligible` passes, `disarm` and `close`, and what `result` says of a pull
//! request armed or held by checks that are not the App's.

use crate::deliver::{Delivery, HEAD, SLUG, names, snapshot};
use crate::github::Fault;
use serde_json::{Value, json};

/// [`snapshot`], its `tests` stamp marked red where `red` says.
fn evidence(head: &str, tests: &str, red: bool, revision: &str) -> Value {
    let mut evidence = snapshot(head, tests, revision);
    if red {
        evidence["stamps"][0]["red"] = json!(true);
    }
    evidence
}

/// A delivery whose repository allows auto-merge, its pull request published at [`HEAD`] with
/// its tests still to stand: open, ready, its verdict pending.
fn published(name: &str) -> Delivery {
    let d = Delivery::new(name, "");
    d.rig
        .github
        .set_repository(&json!({"allow_auto_merge": true}));
    assert_eq!(
        d.publish(&snapshot(HEAD, "missing", "r1"))["state"],
        "published"
    );
    d
}

/// The GraphQL requests sent, each one's body.
fn mutations(d: &Delivery) -> Vec<Value> {
    (d.rig.github.asked().into_iter())
        .filter(|(method, path, _)| method == "POST" && path == "/graphql")
        .map(|(.., body)| serde_json::from_str(&body.unwrap()).unwrap())
        .collect()
}

#[test]
fn evidence_short_of_standing_is_pending_and_a_red_stamp_fails() {
    let d = Delivery::new("armed-pending", "");
    let mut short = snapshot(HEAD, "missing", "r1");
    short["assessed"] = json!(false);
    d.publish(&short);
    assert_eq!(
        names(&d.rig.github.checks(HEAD)),
        [
            ("symphony/eligible", "in_progress"),
            ("symphony/assessment", "in_progress"),
            ("symphony/tests", "in_progress"),
            ("symphony/review", "success"),
            ("symphony/pictures", "neutral"),
        ]
    );
    // A pending check is a run in progress: no conclusion is sent.
    let first: Value = serde_json::from_str(
        &(d.rig.github.asked().into_iter())
            .find(|(method, path, _)| method == "POST" && path == "/repos/o/n/check-runs")
            .unwrap()
            .2
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first["name"], "symphony/eligible");
    assert_eq!(first["status"], "in_progress");
    assert!(first.get("conclusion").is_none(), "{first}");
    // Asked again, each pending check is found as it is and nothing is written.
    let writes = d.writes();
    d.publish(&short);
    assert_eq!(d.writes(), writes);
    // Red: the stamp and the verdict fail, the rest as they were.
    d.publish(&evidence(HEAD, "missing", true, "r2"));
    let checks = d.rig.github.checks(HEAD);
    assert!(names(&checks).contains(&("symphony/tests", "failure")));
    assert!(names(&checks).contains(&("symphony/eligible", "failure")));
    assert!(names(&checks).contains(&("symphony/assessment", "success")));
    // Green again and still short of a stamp: pending, as nothing stood at this head yet.
    d.publish(&evidence(HEAD, "missing", false, "r3"));
    assert!(names(&d.rig.github.checks(HEAD)).contains(&("symphony/eligible", "in_progress")));
    // Standing: each one completes, the pending ones rewritten in place.
    let before = d.writes().len();
    d.publish(&snapshot(HEAD, "met", "r4"));
    let rewritten = d.writes().split_off(before);
    assert!(
        (rewritten.iter()).all(|write| write.starts_with("PATCH ")),
        "{rewritten:?}"
    );
    assert!(names(&d.rig.github.checks(HEAD)).contains(&("symphony/eligible", "success")));
    d.assert_no_token();
}

#[test]
fn arm_turns_on_auto_merge_once_and_github_merges_when_the_verdict_passes() {
    let d = published("armed-merge");
    let armed = json!({"state": "armed", "pull": 100, "head": HEAD});
    assert_eq!(d.arm(100, HEAD), armed);
    // One mutation, at the head, by the squash and the text a merge would ask for.
    let sent = mutations(&d);
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0]["query"]
            .as_str()
            .unwrap()
            .contains("enablePullRequestAutoMerge(input:")
    );
    assert_eq!(
        sent[0]["variables"],
        json!({
            "id": "PR_node100", "method": "SQUASH", "head": HEAD, "headline": SLUG,
            "body": format!("Squashed from {HEAD} by pull request #100."),
        })
    );
    // Asked again it reads that it is armed, and turns nothing on twice.
    assert_eq!(d.arm(100, HEAD), armed);
    assert_eq!(mutations(&d).len(), 1);
    let result = d.result(100);
    assert_eq!(
        (&result["state"], &result["auto_merge"]),
        (&json!("open"), &json!(true))
    );
    // Its verdict passing is what merges it, by GitHub alone: no merge is asked here.
    d.publish(&snapshot(HEAD, "met", "r2"));
    let result = d.result(100);
    assert_eq!(result["state"], "merged", "{result}");
    assert_eq!(result["sha"], "5".repeat(40));
    let merged = d.rig.github.pulls().remove(0)["merged_as"].clone();
    assert_eq!(
        (
            &merged["sha"],
            &merged["merge_method"],
            &merged["commit_title"]
        ),
        (&json!(HEAD), &json!("squash"), &json!(SLUG))
    );
    assert!(!(d.writes().iter()).any(|write| write.starts_with("PUT ")));
    assert_eq!(
        d.arm(100, HEAD),
        json!({"state": "merged", "sha": "5".repeat(40)})
    );
    let journal: Vec<String> = (d.rig.journal().iter())
        .map(|entry| format!("{} {:?}", entry.kind, entry.event))
        .collect();
    assert!(journal.contains(&"arm Intent".to_string()), "{journal:?}");
    assert!(journal.contains(&"arm Ack".to_string()), "{journal:?}");
    d.assert_no_token();
}

#[test]
fn a_pull_request_github_can_merge_already_takes_no_auto_merge_and_is_merged_by_the_land() {
    let d = Delivery::new("armed-mergeable", "");
    d.rig
        .github
        .set_repository(&json!({"allow_auto_merge": true}));
    d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(
        d.arm(100, HEAD),
        json!({"state": "mergeable", "pull": 100, "head": HEAD})
    );
    assert_eq!(d.rig.github.pulls()[0]["auto_merge"], Value::Null);
    assert_eq!(d.merge(100)["state"], "merged");
}

#[test]
fn arm_answers_a_pull_request_as_github_holds_it() {
    // Closed, or at another head: nothing turned on.
    let d = published("armed-closed");
    d.rig.github.set_pull(100, &json!({"state": "closed"}));
    assert_eq!(d.arm(100, HEAD), json!({"state": "closed"}));
    let d = published("armed-head");
    let other = "c".repeat(40);
    let answer = d.arm(100, &other);
    assert_eq!(
        (&answer["state"], &answer["kind"]),
        (&json!("refused"), &json!("head")),
        "{answer}"
    );
    assert_eq!(mutations(&d), Vec::<Value>::new());
    // A rule restricting updates since: refused by the policy, before anything is turned on.
    let d = published("armed-policy");
    d.rig
        .github
        .ruleset(2, &json!([]), &json!([{"type": "update"}]));
    let answer = d.arm(100, HEAD);
    assert_eq!(answer["kind"], "policy", "{answer}");
    assert_eq!(mutations(&d), Vec::<Value>::new());
    // A repository that allows no auto-merge, and one in conflict with its base.
    for (planted, kind) in [
        (json!({"allow_auto_merge": false}), "permission"),
        (Value::Null, "conflict"),
    ] {
        let d = published("armed-refused");
        if planted.is_null() {
            d.rig
                .github
                .set_pull(100, &json!({"mergeable_state": "dirty"}));
        } else {
            d.rig.github.set_repository(&planted);
        }
        let answer = d.arm(100, HEAD);
        assert_eq!(
            (&answer["state"], &answer["kind"]),
            (&json!("refused"), &json!(kind)),
            "{answer}"
        );
        assert!(
            answer["why"]
                .as_str()
                .unwrap()
                .starts_with("GitHub armed no auto-merge of pull request #100 (200): "),
            "{answer}"
        );
        assert_eq!(d.rig.github.pulls()[0]["auto_merge"], Value::Null);
    }
    // A draft is marked ready first, then armed.
    let d = published("armed-draft");
    d.rig.github.set_pull(100, &json!({"draft": true}));
    assert_eq!(d.arm(100, HEAD)["state"], "armed");
    assert_eq!(mutations(&d).len(), 2);
    assert_eq!(d.rig.github.pulls()[0]["draft"], false);
    // An answer lost on the way back: not known, and the next arm reads that it is on.
    let d = published("armed-lost");
    d.rig.github.fault("POST", "/graphql", Fault::LoseAnswer);
    let answer = d.arm(100, HEAD);
    assert_eq!(answer["state"], "unavailable", "{answer}");
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .contains("the next run reads it first"),
        "{answer}"
    );
    assert_eq!(d.arm(100, HEAD)["state"], "armed");
    assert_eq!(mutations(&d).len(), 1, "turned on once");
    d.assert_no_token();
}

#[test]
fn disarm_turns_auto_merge_off_so_a_verdict_that_passes_later_merges_nothing() {
    let d = published("armed-disarm");
    d.arm(100, HEAD);
    assert_eq!(d.end("disarm", 100), json!({"state": "disarmed"}));
    assert_eq!(d.rig.github.pulls()[0]["auto_merge"], Value::Null);
    // Off already: nothing sent.
    assert_eq!(d.end("disarm", 100), json!({"state": "disarmed"}));
    assert_eq!(mutations(&d).len(), 2);
    d.publish(&snapshot(HEAD, "met", "r2"));
    assert_eq!(d.result(100)["state"], "open", "nothing merged it");
    // Merged first, or closed: said as it is.
    let d = published("armed-disarm-merged");
    d.arm(100, HEAD);
    d.publish(&snapshot(HEAD, "met", "r2"));
    assert_eq!(
        d.end("disarm", 100),
        json!({"state": "merged", "sha": "5".repeat(40)})
    );
    let d = published("armed-disarm-closed");
    d.rig.github.set_pull(100, &json!({"state": "closed"}));
    assert_eq!(d.end("disarm", 100), json!({"state": "closed"}));
    // Refused while it is still open: said by kind, auto-merge still on.
    let d = published("armed-disarm-refused");
    d.arm(100, HEAD);
    d.rig
        .github
        .fault("POST", "/graphql", Fault::Status(403, Vec::new()));
    let answer = d.end("disarm", 100);
    assert_eq!(
        answer,
        json!({
            "state": "refused", "kind": "permission",
            "why": "GitHub left auto-merge on for pull request #100 (403): planted",
        })
    );
    assert!(d.rig.github.pulls()[0]["auto_merge"].is_object());
    // A failure GitHub answers for: not known whether it was turned off.
    d.rig
        .github
        .fault("POST", "/graphql", Fault::Status(502, Vec::new()));
    let answer = d.end("disarm", 100);
    assert_eq!(answer["state"], "unavailable", "{answer}");
    assert_eq!(d.end("disarm", 100), json!({"state": "disarmed"}));
    d.assert_no_token();
}

#[test]
fn close_turns_auto_merge_off_then_closes_the_pull_request_unmerged() {
    let d = published("armed-close");
    d.arm(100, HEAD);
    assert_eq!(d.end("close", 100), json!({"state": "closed"}));
    let pull = d.rig.github.pulls().remove(0);
    assert_eq!(
        (&pull["state"], &pull["merged"], &pull["auto_merge"]),
        (&json!("closed"), &json!(false), &Value::Null)
    );
    // Off before it is closed, so nothing merges it in between.
    let writes = d.writes();
    let (off, closed) = (
        writes.iter().rposition(|w| w == "POST /graphql").unwrap(),
        writes
            .iter()
            .position(|w| w == "PATCH /repos/o/n/pulls/100")
            .unwrap(),
    );
    assert!(off < closed, "{writes:?}");
    assert!(
        d.remote.pushed.borrow().len() == 1,
        "its branch is left as it is"
    );
    // Closed again: nothing sent. Merged already: said so.
    let before = d.writes().len();
    assert_eq!(d.end("close", 100), json!({"state": "closed"}));
    assert_eq!(d.writes().len(), before);
    let d = published("armed-close-merged");
    d.arm(100, HEAD);
    d.publish(&snapshot(HEAD, "met", "r2"));
    assert_eq!(
        d.end("close", 100),
        json!({"state": "merged", "sha": "5".repeat(40)})
    );
    // A close GitHub refuses leaves it open, said by kind.
    let d = published("armed-close-refused");
    d.rig.github.fault(
        "PATCH",
        "/repos/o/n/pulls/100",
        Fault::Status(403, Vec::new()),
    );
    let answer = d.end("close", 100);
    assert_eq!(
        (&answer["state"], &answer["kind"]),
        (&json!("refused"), &json!("permission")),
        "{answer}"
    );
    assert_eq!(d.result(100)["state"], "open");
    d.assert_no_token();
}

#[test]
fn result_says_each_check_at_the_head_that_is_not_the_apps_and_did_not_pass() {
    let d = published("armed-foreign");
    d.rig
        .github
        .foreign_check_as(HEAD, "ci/build", 9, "failure");
    d.rig.github.foreign_check_as(HEAD, "ci/lint", 9, "success");
    d.rig
        .github
        .foreign_check_as(HEAD, "ci/flaky", 9, "timed_out");
    d.rig
        .github
        .foreign_check_as(&"d".repeat(40), "ci/elsewhere", 9, "failure");
    // The App's own red verdict is not another's.
    d.publish(&evidence(HEAD, "missing", true, "r2"));
    let result = d.result(100);
    assert_eq!(
        result["foreign"],
        json!([
            {"name": "ci/build", "conclusion": "failure"},
            {"name": "ci/flaky", "conclusion": "timed_out"},
        ])
    );
    assert_eq!(
        (&result["auto_merge"], &result["mergeable_state"]),
        (&json!(false), &json!("clean"))
    );
    // In conflict with its base, said as GitHub says it.
    d.rig
        .github
        .set_pull(100, &json!({"mergeable_state": "dirty"}));
    assert_eq!(d.result(100)["mergeable_state"], "dirty");
    // Commit statuses are read nowhere.
    assert!(
        !(d.rig.github.asked().iter()).any(|(_, path, _)| path.ends_with("/status")),
        "no status read"
    );
    // A pull request no longer open reads no checks.
    d.rig.github.set_pull(100, &json!({"state": "closed"}));
    let reads = d.rig.github.listings(HEAD);
    assert_eq!(d.result(100)["foreign"], json!([]));
    assert_eq!(d.rig.github.listings(HEAD), reads);
}

/// The auto-merge of the one pull request as the fake holds it: its title and message.
fn armed_text(d: &Delivery) -> (Value, Value) {
    let on = d.rig.github.pulls().remove(0)["auto_merge"].clone();
    (on["commit_title"].clone(), on["commit_message"].clone())
}

/// A curated text written for [`SLUG`], titled `title`.
fn curate(d: &Delivery, title: &str) {
    let body = d.rig.dir.0.join("body.md");
    std::fs::write(&body, "What it does, for the public.\n").unwrap();
    d.rig
        .run(&[
            "publish",
            SLUG,
            "--title",
            title,
            "--body-file",
            body.to_str().unwrap(),
        ])
        .ok();
}

#[test]
fn armed_at_an_earlier_head_it_is_armed_again_and_the_squash_names_the_new_head() {
    let d = published("armed-new-head");
    d.arm(100, HEAD);
    // A new commit published: GitHub keeps the auto-merge on, with the text of the first head.
    let next = "b".repeat(40);
    assert_eq!(
        d.publish(&snapshot(&next, "missing", "r2"))["state"],
        "published"
    );
    assert_eq!(
        armed_text(&d).1,
        json!(format!("Squashed from {HEAD} by pull request #100."))
    );
    assert_eq!(
        d.arm(100, &next),
        json!({"state": "armed", "pull": 100, "head": next})
    );
    // Turned off, then on at the new head with its text.
    let sent = mutations(&d);
    assert_eq!(sent.len(), 3, "{sent:?}");
    assert!(
        sent[1]["query"]
            .as_str()
            .unwrap()
            .contains("disablePullRequestAutoMerge(input:")
    );
    assert_eq!(
        sent[2]["variables"],
        json!({
            "id": "PR_node100", "method": "SQUASH", "head": next, "headline": SLUG,
            "body": format!("Squashed from {next} by pull request #100."),
        })
    );
    // The same head and text again: nothing sent.
    assert_eq!(d.arm(100, &next)["state"], "armed");
    assert_eq!(mutations(&d).len(), 3);
    // On by this text and another method: armed again as well.
    let mut on = d.rig.github.pulls().remove(0)["auto_merge"].clone();
    on["merge_method"] = json!("merge");
    d.rig.github.set_pull(100, &json!({"auto_merge": on}));
    assert_eq!(d.arm(100, &next)["state"], "armed");
    assert_eq!(mutations(&d).len(), 5);
    assert_eq!(
        d.rig.github.pulls()[0]["auto_merge"]["merge_method"],
        "squash"
    );
    d.publish(&snapshot(&next, "met", "r3"));
    let merged = d.rig.github.pulls().remove(0)["merged_as"].clone();
    assert_eq!(
        merged,
        json!({
            "sha": next, "merge_method": "squash", "commit_title": SLUG,
            "commit_message": format!("Squashed from {next} by pull request #100."),
        })
    );
    d.assert_no_token();
}

#[test]
fn a_curated_text_written_after_the_arm_reaches_the_squash() {
    let d = published("armed-curated-later");
    d.arm(100, HEAD);
    assert_eq!(armed_text(&d).0, json!(SLUG));
    d.rig
        .board
        .file(SLUG, "the private title", "the private brief");
    curate(&d, "Jump higher");
    assert_eq!(d.arm(100, HEAD)["state"], "armed");
    assert_eq!(mutations(&d).len(), 3, "off, then on by the curated title");
    // Curated again, the title as it was: the text is the same, and nothing is sent.
    curate(&d, "Jump higher");
    assert_eq!(d.arm(100, HEAD)["state"], "armed");
    assert_eq!(mutations(&d).len(), 3);
    d.publish(&snapshot(HEAD, "met", "r2"));
    let merged = d.rig.github.pulls().remove(0)["merged_as"].clone();
    assert_eq!(
        (&merged["commit_title"], &merged["commit_message"]),
        (
            &json!("sim/jump: Jump higher"),
            &json!(format!("Squashed from {HEAD} by pull request #100."))
        )
    );
}

#[test]
fn a_disarm_made_and_an_arming_lost_or_refused_leaves_it_off_for_the_next_run_to_arm() {
    let next = "b".repeat(40);
    let rearmed = |name: &str| {
        let d = published(name);
        d.arm(100, HEAD);
        d.publish(&snapshot(&next, "missing", "r2"));
        d
    };
    // The arming refused after the disarm: said by kind, auto-merge off.
    let d = rearmed("armed-again-refused");
    d.rig.github.fault("POST", "/graphql", Fault::Pass);
    d.rig
        .github
        .fault("POST", "/graphql", Fault::Status(403, Vec::new()));
    let answer = d.arm(100, &next);
    assert_eq!(
        (&answer["state"], &answer["kind"]),
        (&json!("refused"), &json!("permission")),
        "{answer}"
    );
    assert_eq!(d.rig.github.pulls()[0]["auto_merge"], Value::Null);
    let journal: Vec<String> = (d.rig.journal().iter())
        .map(|entry| format!("{} {:?}", entry.kind, entry.event))
        .collect();
    assert_eq!(
        journal[journal.len() - 4..],
        ["disarm Intent", "disarm Ack", "arm Intent", "arm Failed"],
        "{journal:?}"
    );
    // Off, the next run arms it with one mutation, at the new head.
    assert_eq!(d.arm(100, &next)["state"], "armed");
    assert_eq!(mutations(&d).len(), 4);
    assert_eq!(
        armed_text(&d).1,
        json!(format!("Squashed from {next} by pull request #100."))
    );
    // The arming never reaching GitHub is the same to this run as its answer lost: unavailable.
    // Here it was carried out, and the next run reads this head's text and sends nothing.
    let d = rearmed("armed-again-lost");
    d.rig.github.fault("POST", "/graphql", Fault::Pass);
    d.rig.github.fault("POST", "/graphql", Fault::LoseAnswer);
    let answer = d.arm(100, &next);
    assert_eq!(answer["state"], "unavailable", "{answer}");
    let journal = d.rig.journal();
    let last = journal.last().unwrap();
    assert_eq!(format!("{} {:?}", last.kind, last.event), "arm Unknown");
    assert_eq!(d.arm(100, &next)["state"], "armed");
    assert_eq!(mutations(&d).len(), 3);
    // The arming never carried out, its answer GitHub's own failure: not known, auto-merge off.
    let d = rearmed("armed-again-unanswered");
    d.rig.github.fault("POST", "/graphql", Fault::Pass);
    d.rig
        .github
        .fault("POST", "/graphql", Fault::Status(502, Vec::new()));
    let answer = d.arm(100, &next);
    assert_eq!(answer["state"], "unavailable", "{answer}");
    assert_eq!(d.rig.github.pulls()[0]["auto_merge"], Value::Null);
    let journal: Vec<String> = (d.rig.journal().iter())
        .map(|entry| format!("{} {:?}", entry.kind, entry.event))
        .collect();
    assert_eq!(
        journal[journal.len() - 4..],
        ["disarm Intent", "disarm Ack", "arm Intent", "arm Unknown"],
        "{journal:?}"
    );
    // The next run arms it: one enable, at the new head with its text.
    assert_eq!(d.arm(100, &next)["state"], "armed");
    let sent = mutations(&d);
    assert_eq!(sent.len(), 4, "{sent:?}");
    assert!(
        sent[3]["query"]
            .as_str()
            .unwrap()
            .contains("enablePullRequestAutoMerge(input:")
    );
    assert_eq!(sent[3]["variables"]["head"], json!(next));
    assert_eq!(
        armed_text(&d).1,
        json!(format!("Squashed from {next} by pull request #100."))
    );
    // The disarm itself refused: the first arming stands, said as a disarm says it.
    let d = rearmed("armed-again-held");
    d.rig
        .github
        .fault("POST", "/graphql", Fault::Status(403, Vec::new()));
    let answer = d.arm(100, &next);
    assert_eq!(
        answer["why"],
        "GitHub left auto-merge on for pull request #100 (403): planted"
    );
    assert_eq!(
        armed_text(&d).1,
        json!(format!("Squashed from {HEAD} by pull request #100."))
    );
    // Its answer lost: not known, and nothing armed after it.
    d.rig.github.fault("POST", "/graphql", Fault::LoseAnswer);
    let answer = d.arm(100, &next);
    assert_eq!(answer["state"], "unavailable", "{answer}");
    assert_eq!(d.rig.github.pulls()[0]["auto_merge"], Value::Null);
    assert_eq!(d.arm(100, &next)["state"], "armed");
    d.assert_no_token();
}
