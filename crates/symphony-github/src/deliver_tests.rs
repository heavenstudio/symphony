use super::*;
use std::path::Path;

fn repository() -> Repository {
    Repository {
        owner: "Owner".into(),
        name: "a.name".into(),
    }
}

#[test]
fn only_an_address_of_the_repository_on_github_names_it() {
    for url in [
        "https://github.com/Owner/a.name",
        "https://github.com/owner/A.NAME.git",
        "https://github.com/Owner/a.name/",
        "git@github.com:Owner/a.name.git",
        "ssh://git@github.com/Owner/a.name",
    ] {
        assert!(names(url, &repository()), "{url}");
    }
    for url in [
        "https://github.com/Owner/a.name2",
        "https://github.com/Other/a.name",
        "https://github.example.com/Owner/a.name",
        "https://example.com/github.com/Owner/a.name",
        "http://github.com/Owner/a.name",
        "/srv/git/Owner/a.name.git",
        "",
    ] {
        assert!(!names(url, &repository()), "{url}");
    }
}

fn snapshot(eligible: bool, standing: &str) -> Snapshot {
    Snapshot {
        version: 1,
        slug: "sim/jump".into(),
        task: 7,
        head: "a".repeat(40),
        assessed: eligible,
        eligible,
        revision: "r1".into(),
        stamps: vec![Stamp {
            name: "tests".into(),
            standing: standing.into(),
            red: false,
        }],
        assessment: None,
    }
}

fn said(snapshot: &Snapshot, withdrawn: bool) -> Vec<(String, &'static str)> {
    (checks(snapshot, 77, withdrawn).unwrap().into_iter())
        .map(|check| (check.name.clone(), check.said()))
        .collect()
}

#[test]
fn the_verdict_is_last_when_it_passes_and_first_when_it_does_not() {
    assert_eq!(
        said(&snapshot(true, "met"), false),
        [
            (ASSESSMENT.to_string(), "success"),
            ("symphony/tests".to_string(), "success"),
            (ELIGIBLE.to_string(), "success"),
        ]
    );
    // Short of standing, nothing red: pending, which holds GitHub's merge and fails nothing.
    assert_eq!(
        said(&snapshot(false, "missing"), false),
        [
            (ELIGIBLE.to_string(), "pending"),
            (ASSESSMENT.to_string(), "pending"),
            ("symphony/tests".to_string(), "pending"),
        ]
    );
    assert_eq!(said(&snapshot(true, "waived"), false)[1].1, "neutral");
    assert!(checks(&snapshot(true, "perhaps"), 77, false).is_err());
}

#[test]
fn a_red_stamp_or_evidence_withdrawn_fails_the_verdict_and_nothing_else_does() {
    let mut red = snapshot(false, "missing");
    red.assessed = true;
    red.stamps[0].red = true;
    assert_eq!(
        said(&red, false),
        [
            (ELIGIBLE.to_string(), "failure"),
            (ASSESSMENT.to_string(), "success"),
            ("symphony/tests".to_string(), "failure"),
        ]
    );
    let red_title = &checks(&red, 77, false).unwrap()[0].title;
    assert_eq!(red_title, "A stamp the task needs is red at this commit");
    // Withdrawn: evidence that stood at the head fails the verdict, its stamps pending.
    let withdrawn = checks(&snapshot(false, "missing"), 77, true).unwrap();
    assert_eq!(
        (withdrawn[0].said(), withdrawn[0].title.as_str()),
        ("failure", WITHDRAWN)
    );
    assert_eq!(withdrawn[2].said(), "pending");
    // Standing again at that head, it passes whatever stood before.
    assert_eq!(said(&snapshot(true, "met"), true)[2].1, "success");
    // A red mark on a stamp that is met or waived says nothing.
    let mut met = snapshot(true, "met");
    met.stamps[0].red = true;
    assert_eq!(said(&met, false)[2], (ELIGIBLE.to_string(), "success"));
}

#[test]
fn evidence_stood_where_the_newest_verdict_passed_or_failed_as_withdrawn() {
    let run = |id: i64, conclusion: Value, title: &str| json!({"id": id, "name": ELIGIBLE, "conclusion": conclusion, "output": {"title": title}});
    assert!(!withdrawn(&[]));
    assert!(withdrawn(&[run(1, json!("success"), "x")]));
    assert!(withdrawn(&[run(1, json!("failure"), WITHDRAWN)]));
    assert!(!withdrawn(&[run(1, json!("failure"), "red")]));
    assert!(!withdrawn(&[run(1, Value::Null, "pending")]));
    // The newest run of the name is the one GitHub goes by.
    assert!(!withdrawn(&[
        run(2, Value::Null, "pending"),
        run(1, json!("success"), "x")
    ]));
    assert!(withdrawn(&[
        run(1, Value::Null, "pending"),
        run(2, json!("success"), "x")
    ]));
    let other = json!({"id": 3, "name": "symphony/tests", "conclusion": "success"});
    assert!(!withdrawn(&[other]));
}

#[test]
fn a_pending_check_is_a_run_in_progress_found_again_as_one() {
    let head = "a".repeat(40);
    let wanted = checks(&snapshot(false, "missing"), 77, false).unwrap();
    let pending = &wanted[0];
    let sent = pending.sent(&head);
    assert_eq!(sent["status"], "in_progress");
    assert!(sent.get("conclusion").is_none(), "{sent}");
    let mut listed = sent.clone();
    listed["conclusion"] = Value::Null;
    assert!(pending.stands(&listed, &head));
    // Completed, or of another head, it is not the pending check.
    let mut completed = listed.clone();
    completed["status"] = json!("completed");
    assert!(!pending.stands(&completed, &head));
    assert!(!pending.stands(&listed, &"b".repeat(40)));
    let passed = &checks(&snapshot(true, "met"), 77, false).unwrap()[2];
    let sent = passed.sent(&head);
    assert_eq!(
        (&sent["status"], &sent["conclusion"]),
        (&json!("completed"), &json!("success"))
    );
    assert!(passed.stands(&sent, &head));
    assert!(!pending.stands(&sent, &head));
}

#[test]
fn a_check_is_found_again_only_by_everything_it_says() {
    let id = |snapshot: &Snapshot, repository: i64| {
        checks(snapshot, repository, false).unwrap()[1].id.clone()
    };
    let first = snapshot(true, "met");
    assert_eq!(id(&first, 77), id(&snapshot(true, "met"), 77));
    assert_ne!(id(&first, 77), id(&first, 78), "another repository");
    let mut other = snapshot(true, "met");
    other.revision = "r2".into();
    assert_ne!(id(&first, 77), id(&other, 77), "other evidence");
    let mut other = snapshot(true, "met");
    other.head = "b".repeat(40);
    assert_ne!(id(&first, 77), id(&other, 77), "another head");
    let mut other = snapshot(true, "met");
    other.task = 8;
    assert_ne!(id(&first, 77), id(&other, 77), "another task of the slug");
}

fn git(dir: &Path, args: &[&str]) -> String {
    let ran = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        ran.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8_lossy(&ran.stdout).trim().to_string()
}

#[test]
fn the_projects_git_pushes_a_head_to_its_branch_and_never_over_another_history() {
    let dir = std::env::temp_dir().join(format!(
        "symphony-github-deliver-git-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let (main, bare) = (dir.join("main"), dir.join("origin.git"));
    std::fs::create_dir_all(&main).unwrap();
    git(
        &dir,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(&main, &["init", "-q", "-b", "main"]);
    let remote = Git { main: main.clone() };
    assert!(remote.url().is_err(), "no origin");
    git(&main, &["remote", "add", "origin", bare.to_str().unwrap()]);
    assert_eq!(remote.url().unwrap(), bare.to_str().unwrap());
    git(&main, &["commit", "-q", "--allow-empty", "-m", "one"]);
    let one = git(&main, &["rev-parse", "HEAD"]);
    remote.push(&one, "sim/jump").unwrap();
    remote.push(&one, "sim/jump").unwrap();
    assert_eq!(git(&bare, &["rev-parse", "refs/heads/sim/jump"]), one);
    // A commit that does not descend from what the branch holds there is not forced over it.
    git(&main, &["checkout", "-q", "--orphan", "other"]);
    git(
        &main,
        &["commit", "-q", "--allow-empty", "-m", "another history"],
    );
    let other = git(&main, &["rev-parse", "HEAD"]);
    assert!(remote.push(&other, "sim/jump").is_err());
    assert_eq!(git(&bare, &["rev-parse", "refs/heads/sim/jump"]), one);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A merge is asked by the method its request names for its kind of head: a task's squashed, an
/// import's by a merge commit. The other method, none, and a head that is no whole commit are
/// refused in the same words.
#[test]
fn a_merges_method_is_its_kinds_own_and_its_head_one_whole_commit() {
    let head = "a".repeat(40);
    let asked = |method: &str| json!({"method": method});
    assert_eq!(merge_method(&asked("squash"), &head, false), Ok("squash"));
    assert_eq!(merge_method(&asked("merge"), &head, true), Ok("merge"));
    let refusal =
        refused("a merge is of one whole commit: a task's squashed, an import's by a merge commit");
    for (request, head, imported) in [
        (asked("merge"), head.as_str(), false),
        (asked("squash"), head.as_str(), true),
        (asked("rebase"), head.as_str(), false),
        (json!({}), head.as_str(), false),
        (json!({"method": null}), head.as_str(), true),
        (asked("squash"), "a".repeat(39).as_str(), false),
        (asked("squash"), "main", false),
        (asked("merge"), "", true),
        (asked("squash"), "g".repeat(40).as_str(), false),
    ] {
        assert_eq!(
            merge_method(&request, head, imported),
            Err(refusal.clone()),
            "{request} at {head:?}, imported {imported}"
        );
    }
}

/// A transport that answers every request `status` with `headers`.
struct Answers(u16, Vec<(&'static str, &'static str)>);

impl Transport for Answers {
    fn send(&self, _: &Request) -> Result<crate::transport::Response, Failure> {
        Ok(crate::transport::Response {
            status: self.0,
            headers: (self.1.iter())
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect(),
            body: r#"{"message":"said"}"#.into(),
        })
    }
}

#[test]
fn a_rate_limit_stops_a_request_and_any_other_403_is_handed_to_its_verb() {
    let sent = |status, headers: &[(&'static str, &'static str)]| {
        let transport = Answers(status, headers.to_vec());
        let host = Host {
            transport: &transport,
            repository: repository(),
        };
        assert_eq!(host.path("/pulls/1"), "/repos/Owner/a.name/pulls/1");
        host.send("PUT", host.path("/pulls/1/merge"), None)
            .map(|answer| answer.map_err(|Lost(why)| why))
    };
    let limit = unavailable("GitHub's rate limit: nothing more is sent now");
    for (status, headers) in [
        // The primary limit, with no `retry-after`: no refusal of the verb's.
        (
            403,
            vec![("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "900")],
        ),
        (403, vec![("x-ratelimit-remaining", "0")]),
        (403, vec![("retry-after", "30")]),
        (429, vec![]),
        (429, vec![("retry-after", "30")]),
    ] {
        assert_eq!(
            sent(status, &headers),
            Err(limit.clone()),
            "{status} {headers:?}"
        );
    }
    for (status, headers) in [
        (403, vec![]),
        (403, vec![("x-ratelimit-remaining", "12")]),
        (422, vec![("x-ratelimit-remaining", "0")]),
        (422, vec![("retry-after", "30")]),
        (200, vec![("x-ratelimit-remaining", "0")]),
    ] {
        assert_eq!(
            sent(status, &headers),
            Ok(Ok((status, json!({"message": "said"})))),
            "{status} {headers:?}"
        );
    }
}
