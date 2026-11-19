//! `symphony-github deliver`, the four verbs a land delivered by a pull request asks: what each
//! writes to GitHub and in what order, that a second run and a lost answer make nothing twice,
//! what is refused before anything is sent, and that neither token is in anything said or kept.

use crate::github::{Fake, Fault};
use crate::{Rig, TOKEN};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use symphony_github::credential::Secret;
use symphony_github::deliver::{self, Remote};
use symphony_github::reconcile::Context;
use symphony_github::state::{Mapping, Origin};
use symphony_github::transport::{Failure, Request, Response, Transport};

/// The token of the GitHub App that writes the checks.
const CHECKS: &str = "ghs_fixtureCHECKS0123456789abcdefSECRET";
/// That App's number.
pub(crate) const APP: i64 = 42;
/// The number of the GitHub App that merges, whose token the settings' `token_file` holds.
pub(crate) const LANDING: i64 = 43;
pub(crate) const SLUG: &str = "sim/jump";
pub(crate) const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// The fake repository, asked as the App of the token a request carries.
struct As(Arc<Fake>, i64);

impl Transport for As {
    fn send(&self, request: &Request) -> Result<Response, Failure> {
        let (response, lost) = (self.0).handle_as(
            self.1,
            request.method,
            &request.path,
            request.body.as_deref(),
        );
        if lost {
            Err(Failure::Unknown(
                "the connection ended with no answer".into(),
            ))
        } else {
            Ok(response)
        }
    }
}

/// The project's git: where `origin` leads, and what was pushed.
pub(crate) struct Pushes {
    pub(crate) url: String,
    github: Arc<Fake>,
    refuses: bool,
    pub(crate) pushed: RefCell<Vec<(String, String)>>,
}

impl Remote for Pushes {
    fn url(&self) -> Result<String, String> {
        Ok(self.url.clone())
    }

    fn push(&self, head: &str, branch: &str) -> Result<(), String> {
        if self.refuses {
            return Err(" ! [rejected] (non-fast-forward)".into());
        }
        self.pushed.borrow_mut().push((head.into(), branch.into()));
        self.github.push(branch, head);
        Ok(())
    }
}

/// A delivery's rig: the exchange's, its settings serving `publish`, and every line it said.
pub(crate) struct Delivery {
    pub(crate) rig: Rig,
    pub(crate) remote: Pushes,
    said: RefCell<String>,
}

fn settings(checks: &std::path::Path, more: &str) -> String {
    format!(
        "pull_requests = \"publish\"\nchecks_token_file = \"{}\"\nchecks_app_id = {APP}\n\
         landing_app_id = {LANDING}\n{more}",
        checks.display()
    )
}

impl Delivery {
    /// A delivery to a repository protected as one needs it.
    pub(crate) fn new(name: &str, more: &str) -> Delivery {
        let rig = Rig::off(name);
        rig.github.protect(APP, LANDING);
        let checks = rig.dir.0.join("secret/checks");
        std::fs::write(&checks, format!("{CHECKS}\n")).unwrap();
        std::fs::set_permissions(&checks, std::fs::Permissions::from_mode(0o600)).unwrap();
        rig.enable(&settings(&checks, more));
        let remote = Pushes {
            url: "git@github.com:o/n.git".into(),
            github: Arc::clone(&rig.github),
            refuses: false,
            pushed: RefCell::default(),
        };
        Delivery {
            rig,
            remote,
            said: RefCell::default(),
        }
    }

    pub(crate) fn checks_file(&self) -> PathBuf {
        self.rig.dir.0.join("secret/checks")
    }

    /// The answer to `verb` of `request`.
    pub(crate) fn ask(&self, verb: &str, request: &Value) -> Value {
        let file = self.rig.dir.0.join("request.json");
        std::fs::write(&file, request.to_string()).unwrap();
        let github = Arc::clone(&self.rig.github);
        let connect = move |token: Secret| -> Result<Box<dyn Transport>, String> {
            let app = match token.expose() {
                CHECKS => APP,
                TOKEN => LANDING,
                _ => 0,
            };
            Ok(Box::new(As(Arc::clone(&github), app)))
        };
        let ctx = Context {
            source: &self.rig.settings,
            board: &self.rig.board,
            connect: &connect,
            now: self.rig.now.get(),
            repository: vec![self.rig.dir.0.join("repository")],
        };
        let mut out = Vec::new();
        let _alone = crate::no_start();
        let code = deliver::run(&ctx, &self.remote, verb, &file, &mut out).unwrap();
        assert_eq!(code, 0);
        let out = String::from_utf8(out).unwrap();
        self.said.borrow_mut().push_str(&out);
        assert_eq!(out.lines().count(), 1, "one line of JSON: {out}");
        serde_json::from_str(&out).unwrap()
    }

    pub(crate) fn publish(&self, snapshot: &Value) -> Value {
        self.ask(
            "publish",
            &json!({
                "verb": "publish", "slug": SLUG, "by": "amber-fox-1", "head": snapshot["head"],
                "branch": "main", "title": "sim/jump: the private title",
                "body": "the private brief", "snapshot": snapshot,
            }),
        )
    }

    pub(crate) fn merge(&self, pull: i64) -> Value {
        self.ask(
            "merge",
            &json!({
                "verb": "merge", "slug": SLUG, "by": "amber-fox-1", "head": HEAD,
                "repository": 77, "pull": pull, "method": "squash",
                "title": "sim/jump: the private title", "body": "the private brief",
            }),
        )
    }

    pub(crate) fn result(&self, pull: i64) -> Value {
        let request = json!({"verb": "result", "slug": SLUG, "repository": 77, "pull": pull});
        self.ask("result", &request)
    }

    /// The requests made that change the repository: each one's method and path.
    pub(crate) fn writes(&self) -> Vec<String> {
        (self.rig.github.asked().into_iter())
            .filter(|(method, ..)| method != "GET")
            .map(|(method, path, _)| format!("{method} {path}"))
            .collect()
    }

    /// Neither token is in anything said, kept or asked for by its address.
    pub(crate) fn assert_no_token(&self) {
        let folder = self.rig.dir.0.join("state");
        let mut texts = vec![self.said.borrow().clone()];
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                texts.push(std::fs::read_to_string(&path).unwrap());
            }
        }
        for (method, path, body) in self.rig.github.asked() {
            texts.push(format!("{method} {path} {}", body.unwrap_or_default()));
        }
        for text in texts {
            assert!(!text.contains(TOKEN) && !text.contains(CHECKS), "{text}");
        }
    }
}

/// The evidence of [`SLUG`] at `head`: `tests` and `review` as given, the others waived.
pub(crate) fn snapshot(head: &str, tests: &str, revision: &str) -> Value {
    let eligible = tests == "met";
    json!({
        "version": 1, "slug": SLUG, "task": 7, "status": "done", "head": head, "branch": "main",
        "policy": "p", "catalogue": "c", "assessed": true, "eligible": eligible,
        "assessment": {"by": "calm-owl-2", "tip": head, "why": "a private reason", "stamps": []},
        "stamps": [
            {"name": "tests", "standing": tests, "detail": "a private line", "evidence": []},
            {"name": "review", "standing": "met", "detail": "by a private handle", "evidence": []},
            {"name": "pictures", "standing": "waived", "detail": "", "evidence": []},
        ],
        "blocking": [], "revision": revision,
    })
}

fn names(checks: &[(String, String, i64)]) -> Vec<(&str, &str)> {
    (checks.iter())
        .map(|(name, conclusion, _)| (name.as_str(), conclusion.as_str()))
        .collect()
}

#[test]
fn nothing_is_asked_of_github_unless_the_settings_serve_publish() {
    for more in [None, Some("pull_requests = \"track\""), Some("")] {
        let d = Delivery::new("deliver-off", "");
        match more {
            None => d.rig.settings.set(None),
            Some(more) => d.rig.enable(more),
        }
        for verb in ["publish", "merge", "result", "landed"] {
            let answer = d.ask(
                verb,
                &json!({"slug": SLUG, "snapshot": snapshot(HEAD, "met", "r1")}),
            );
            assert_eq!(answer["state"], "refused", "{answer}");
            assert!(
                answer["why"].as_str().unwrap().starts_with("turned off: "),
                "{answer}"
            );
        }
        assert_eq!(d.rig.github.asked().len(), 0);
        assert!(d.remote.pushed.borrow().is_empty());
    }
}

#[test]
fn publish_pushes_the_head_opens_one_pull_request_and_writes_each_check_once() {
    let d = Delivery::new("deliver-publish", "");
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(
        answer,
        json!({
            "state": "published", "repository": {"id": 77, "name": "o/n"}, "head": HEAD,
            "pull": {"number": 100, "url": "https://github.com/o/n/pull/100"},
        })
    );
    assert_eq!(
        *d.remote.pushed.borrow(),
        [(HEAD.to_string(), SLUG.to_string())]
    );
    // Its public text is the slug: neither the title nor the brief the land handed over.
    let pulls = d.rig.github.pulls();
    assert_eq!(pulls.len(), 1);
    assert_eq!(pulls[0]["title"], SLUG);
    assert_eq!(
        pulls[0]["draft"], true,
        "a draft until its merge is asked for"
    );
    assert_eq!(pulls[0]["body"], "The Symphony task `sim/jump`.");
    assert_eq!(
        (&pulls[0]["head"]["ref"], &pulls[0]["base"]["ref"]),
        (&json!(SLUG), &json!("main"))
    );
    // The stamps first and what says they all stand last, each by the App.
    let checks = d.rig.github.checks(HEAD);
    assert_eq!(
        names(&checks),
        [
            ("symphony/assessment", "success"),
            ("symphony/tests", "success"),
            ("symphony/review", "success"),
            ("symphony/pictures", "neutral"),
            ("symphony/eligible", "success"),
        ]
    );
    assert!(checks.iter().all(|(.., app)| *app == APP));
    let said = format!("{:?}", d.rig.github.asked());
    for private in ["private", "calm-owl-2", "amber-fox-1"] {
        assert!(!said.contains(private), "{private} was sent: {said}");
    }
    // Asked again, it finds each and makes nothing.
    let writes = d.writes();
    assert_eq!(writes.len(), 6, "{writes:?}");
    assert_eq!(d.publish(&snapshot(HEAD, "met", "r1")), answer);
    assert_eq!(d.writes(), writes);
    assert_eq!(d.rig.state().pulls[SLUG].number, 100);
    d.assert_no_token();
}

#[test]
fn a_curated_text_is_the_pull_requests_and_the_commits_public_text() {
    let d = Delivery::new("deliver-curated", "");
    d.rig
        .board
        .file(SLUG, "the private title", "the private brief");
    let body = d.rig.dir.0.join("body.md");
    std::fs::write(&body, "What it does, for the public.\n").unwrap();
    d.rig
        .run(&[
            "publish",
            SLUG,
            "--title",
            "Jump higher",
            "--body-file",
            body.to_str().unwrap(),
        ])
        .ok();
    d.publish(&snapshot(HEAD, "met", "r1"));
    let pull = d.rig.github.pulls().remove(0);
    assert_eq!(
        (&pull["title"], &pull["body"]),
        (
            &json!("Jump higher"),
            &json!("What it does, for the public.")
        )
    );
    assert_eq!(d.merge(100)["state"], "merged");
    let merged = d.rig.github.pulls().remove(0)["merged_as"].clone();
    assert_eq!(
        merged,
        json!({
            "sha": HEAD, "merge_method": "squash", "commit_title": "sim/jump: Jump higher",
            "commit_message": format!("Squashed from {HEAD} by pull request #100."),
        })
    );
}

#[test]
fn a_lost_answer_is_looked_for_before_anything_is_made_again() {
    // The pull request was opened and its answer lost.
    let d = Delivery::new("deliver-lost", "");
    d.rig
        .github
        .fault("POST", "/repos/o/n/pulls", Fault::LoseAnswer);
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(answer["state"], "unavailable", "{answer}");
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .contains("the next run looks for it first"),
        "{answer}"
    );
    assert!(
        d.rig.github.checks(HEAD).is_empty(),
        "no check before its pull request is known"
    );
    // A check was written and its answer lost.
    d.rig
        .github
        .fault("POST", "/repos/o/n/check-runs", Fault::LoseAnswer);
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "unavailable"
    );
    assert_eq!(
        d.rig.github.pulls().len(),
        1,
        "found by its head, not opened twice"
    );
    assert_eq!(
        names(&d.rig.github.checks(HEAD)),
        [("symphony/assessment", "success")]
    );
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    let checks = d.rig.github.checks(HEAD);
    assert_eq!(checks.len(), 5, "each name once: {checks:?}");
    assert_eq!(
        d.writes()
            .iter()
            .filter(|write| write.starts_with("POST /repos/o/n/pulls"))
            .count(),
        1
    );
    let journal: Vec<String> = (d.rig.journal().iter())
        .map(|entry| format!("{} {:?}", entry.kind, entry.event))
        .collect();
    assert!(journal.contains(&"pull Unknown".to_string()), "{journal:?}");
    assert!(
        journal.contains(&"check Unknown".to_string()),
        "{journal:?}"
    );
    d.assert_no_token();
}

#[test]
fn evidence_that_changed_rewrites_its_checks_the_failing_verdict_first() {
    let d = Delivery::new("deliver-changed", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    let before = d.writes().len();
    let answer = d.publish(&snapshot(HEAD, "missing", "r2"));
    assert_eq!(
        answer["state"], "published",
        "what stands is said, whatever it is"
    );
    let rewritten = d.writes().split_off(before);
    assert_eq!(rewritten.len(), 5, "{rewritten:?}");
    assert!(
        rewritten
            .iter()
            .all(|write| write.starts_with("PATCH /repos/o/n/check-runs/"))
    );
    // The first rewritten is the one that said every stamp stood.
    let first = d
        .rig
        .github
        .asked()
        .into_iter()
        .find(|(method, ..)| method == "PATCH");
    let sent: Value = serde_json::from_str(&first.unwrap().2.unwrap()).unwrap();
    assert_eq!(
        (&sent["name"], &sent["conclusion"]),
        (&json!("symphony/eligible"), &json!("failure"))
    );
    let checks = d.rig.github.checks(HEAD);
    assert_eq!(checks.len(), 5, "each name once: {checks:?}");
    assert!(names(&checks).contains(&("symphony/tests", "failure")));
    assert!(names(&checks).contains(&("symphony/eligible", "failure")));
    // Another head carries none of them.
    let other = "b".repeat(40);
    d.publish(&snapshot(&other, "met", "r3"));
    assert_eq!(d.rig.github.checks(&other).len(), 5);
    assert_eq!(d.rig.github.pulls()[0]["head"]["sha"], other.as_str());
}

#[test]
fn only_the_named_apps_checks_count_and_no_other_token_writes_them() {
    // A check of the same name by another App is not ours: ours is written beside it.
    let d = Delivery::new("deliver-app", "");
    d.rig.github.foreign_check(HEAD, "symphony/eligible", 99);
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    let eligible: Vec<i64> = (d.rig.github.checks(HEAD).into_iter())
        .filter(|(name, ..)| name == "symphony/eligible")
        .map(|(.., app)| app)
        .collect();
    assert_eq!(eligible, [99, APP]);
    // The checks' token a person's: GitHub takes no check of it, and nothing says published.
    let d = Delivery::new("deliver-person", "");
    std::fs::write(d.checks_file(), "ghp_aPersonsOtherToken\n").unwrap();
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(answer["state"], "refused", "{answer}");
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .starts_with("GitHub took no check symphony/assessment"),
        "{answer}"
    );
    assert_eq!(d.rig.github.checks(HEAD).len(), 0);
    // A checks' token others can read is not read at all.
    std::fs::set_permissions(d.checks_file(), std::fs::Permissions::from_mode(0o644)).unwrap();
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .ends_with("is readable by others; chmod 600 it"),
        "{answer}"
    );
    d.assert_no_token();
}

#[test]
fn what_does_not_lead_to_the_repository_named_is_refused_before_anything_is_pushed() {
    let mut d = Delivery::new("deliver-elsewhere", "");
    d.remote.url = "git@github.com:o/another.git".into();
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(
        answer["why"],
        "origin is not o/n on GitHub.com: nothing is pushed anywhere else"
    );
    assert!(d.rig.github.asked().is_empty() && d.remote.pushed.borrow().is_empty());
    // Evidence of another shape, task or head, and a stamp no check is named for.
    d.remote.url = "https://github.com/O/N".into();
    let mut other = snapshot(HEAD, "met", "r1");
    other["version"] = json!(2);
    assert!(
        d.publish(&other)["why"]
            .as_str()
            .unwrap()
            .contains("of shape 2")
    );
    let mut other = snapshot(HEAD, "met", "r1");
    other["slug"] = json!("sim/other");
    assert!(
        d.publish(&other)["why"]
            .as_str()
            .unwrap()
            .contains("another task or head")
    );
    let mut other = snapshot(HEAD, "met", "r1");
    other["stamps"][0]["name"] = json!("te sts");
    assert!(
        d.publish(&other)["why"]
            .as_str()
            .unwrap()
            .contains("no check is named")
    );
    assert!(d.rig.github.asked().is_empty() && d.remote.pushed.borrow().is_empty());
    // A branch there that holds another history: no pull request, no check.
    d.remote.refuses = true;
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .starts_with("aaaaaaa was not pushed to the branch sim/jump of o/n: "),
        "{answer}"
    );
    assert_eq!(d.writes().len(), 0);
    // Another repository answering at the name the state is bound to.
    d.remote.refuses = false;
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    d.rig.github.replace_repository(78);
    let before = d.writes();
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .starts_with("o/n is repository 78, and what is kept here is bound to o/n (id 77)"),
        "{answer}"
    );
    assert_eq!(d.merge(100)["state"], "refused");
    assert_eq!(d.writes(), before);
}

#[test]
fn a_merge_is_one_squash_of_the_head_asked_once() {
    let d = Delivery::new("deliver-merge", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    // Another pull request than the one bound, and another method, ask nothing.
    assert!(
        d.merge(101)["why"]
            .as_str()
            .unwrap()
            .contains("is bound to pull request #100 of o/n (id 77)")
    );
    let squash = "5".repeat(40);
    assert_eq!(
        d.result(100),
        json!({"state": "open", "head": HEAD, "sha": null, "base": "main", "repository": 77})
    );
    assert_eq!(d.merge(100), json!({"state": "merged", "sha": squash}));
    let merged = d.rig.github.pulls().remove(0)["merged_as"].clone();
    assert_eq!(
        (
            &merged["sha"],
            &merged["merge_method"],
            &merged["commit_title"]
        ),
        (&json!(HEAD), &json!("squash"), &json!(SLUG))
    );
    // Marked ready just before, by the one mutation, naming the pull request by its node.
    let ready: Vec<Value> = (d.rig.github.asked().into_iter())
        .filter(|(method, path, _)| method == "POST" && path == "/graphql")
        .map(|(.., body)| serde_json::from_str(&body.unwrap()).unwrap())
        .collect();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0]["variables"], json!({"id": "PR_node100"}));
    // Asked again it reads that it is merged and asks for no second merge.
    assert_eq!(d.merge(100), json!({"state": "merged", "sha": squash}));
    assert_eq!(
        d.writes()
            .iter()
            .filter(|write| write.starts_with("PUT "))
            .count(),
        1
    );
    assert_eq!(
        d.result(100),
        json!({"state": "merged", "head": HEAD, "sha": squash, "base": "main", "repository": 77})
    );
    d.assert_no_token();
}

#[test]
fn a_merge_whose_answer_is_lost_is_unknown_and_its_result_is_read() {
    let d = Delivery::new("deliver-merge-lost", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    d.rig
        .github
        .fault("PUT", "/repos/o/n/pulls/100/merge", Fault::LoseAnswer);
    assert_eq!(d.merge(100)["state"], "unknown");
    assert_eq!(d.result(100)["state"], "merged");
    assert_eq!(d.merge(100)["state"], "merged");
    assert_eq!(
        d.writes()
            .iter()
            .filter(|write| write.starts_with("PUT "))
            .count(),
        1
    );
}

#[test]
fn a_merge_github_refuses_says_which_kind_and_merges_nothing() {
    for (planted, kind) in [
        (json!({"mergeable_state": "dirty"}), "conflict"),
        (json!({"mergeable_state": "blocked"}), "checks"),
        (
            json!({"head": {"ref": SLUG, "sha": "c".repeat(40), "repo": {"id": 77}}}),
            "head",
        ),
        (json!({"state": "closed"}), "closed"),
    ] {
        let d = Delivery::new("deliver-refused", "");
        d.publish(&snapshot(HEAD, "met", "r1"));
        d.rig.github.set_pull(100, &planted);
        let answer = d.merge(100);
        assert_eq!(
            (&answer["state"], &answer["kind"]),
            (&json!("refused"), &json!(kind)),
            "{answer}"
        );
        assert_eq!(d.rig.github.pulls()[0]["merged"], false);
        let asked = d
            .writes()
            .iter()
            .filter(|write| write.starts_with("PUT "))
            .count();
        assert_eq!(
            asked,
            usize::from(matches!(kind, "conflict" | "checks")),
            "{kind}"
        );
        assert_eq!(
            d.result(100)["state"],
            if kind == "closed" { "closed" } else { "open" }
        );
    }
    // A head that moved between the reading and the request, and a token that may not merge.
    for (status, kind) in [(409, "head"), (403, "permission")] {
        let d = Delivery::new("deliver-refused-status", "");
        d.publish(&snapshot(HEAD, "met", "r1"));
        d.rig.github.fault(
            "PUT",
            "/repos/o/n/pulls/100/merge",
            Fault::Status(status, Vec::new()),
        );
        let answer = d.merge(100);
        assert_eq!(answer["kind"], kind, "{answer}");
        assert_eq!(
            answer["why"],
            format!("GitHub merged nothing ({status}): planted")
        );
    }
}

#[test]
fn landed_keeps_the_commit_and_closes_the_mapped_issue_only_where_the_settings_say() {
    let squash = "5".repeat(40);
    for closes in [false, true] {
        let d = Delivery::new(
            "deliver-landed",
            if closes { "close_issues = true" } else { "" },
        );
        let request = json!({"verb": "landed", "slug": SLUG, "by": "amber-fox-1", "head": HEAD,
            "repository": 77, "pull": 100, "squash": squash});
        assert_eq!(
            d.ask("landed", &request)["why"],
            "sim/jump is bound to no pull request here"
        );
        d.publish(&snapshot(HEAD, "met", "r1"));
        d.merge(100);
        // The task is exchanged with an issue.
        let number = d.rig.github.open("Jump higher", "public", &[]);
        let mut state = d.rig.state();
        state.mappings.insert(
            SLUG.into(),
            Mapping {
                origin: Origin::Export,
                task_id: 7,
                generation: "generation-1".into(),
                issue_number: number,
                issue_id: 5000 + number,
                issue_node: String::new(),
                url: String::new(),
                local: String::new(),
                remote: String::new(),
                remote_state: "open".into(),
                comments_since: None,
                comments_seen: Vec::new(),
                flag: None,
            },
        );
        d.rig.store().save(&state).unwrap();
        let before = d.writes().len();
        let said = if closes {
            format!("issue #{number} is closed")
        } else {
            String::new()
        };
        for _ in 0..2 {
            assert_eq!(
                d.ask("landed", &request),
                json!({"state": "noted", "said": said})
            );
        }
        assert_eq!(
            d.writes().len() - before,
            usize::from(closes),
            "closed once, or never"
        );
        assert_eq!(
            d.rig.github.issue(number)["state"],
            if closes { "closed" } else { "open" }
        );
        assert_eq!(
            d.rig.state().pulls[SLUG].landed.as_deref(),
            Some(squash.as_str())
        );
        let status = d.rig.run(&["status"]).ok().out;
        assert!(
            status.contains("pull requests: publish (a land's delivery: `deliver`)"),
            "{status}"
        );
        assert!(
            status.contains("sim/jump is delivered by #100 https://github.com/o/n/pull/100 (landed as 555555555555)"),
            "{status}"
        );
        d.assert_no_token();
    }
}

#[test]
fn the_command_takes_one_verb_and_a_request_that_reads() {
    let d = Delivery::new("deliver-cli", "");
    let none = d.rig.dir.0.join("none.json");
    let ran = d
        .rig
        .run(&["deliver", "publish", "--request", none.to_str().unwrap()]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.err.starts_with("symphony-github: ") && ran.err.contains("none.json"),
        "{}",
        ran.err
    );
    std::fs::write(&none, "[]").unwrap();
    let ran = d
        .rig
        .run(&["deliver", "publish", "--request", none.to_str().unwrap()]);
    assert!(
        ran.err.ends_with("a request is one JSON object\n"),
        "{}",
        ran.err
    );
    let ran = d.rig.run(&["deliver", "--request", none.to_str().unwrap()]);
    assert!(ran.err.contains("deliver takes one VERB"), "{}", ran.err);
    std::fs::write(&none, "{}").unwrap();
    let ran = d
        .rig
        .run(&["deliver", "rebase", "--request", none.to_str().unwrap()])
        .ok();
    assert_eq!(
        ran.out,
        "{\"state\":\"refused\",\"why\":\"no verb \\\"rebase\\\"\"}\n"
    );
    assert_eq!(d.rig.github.asked().len(), 0);
}

#[test]
fn a_draft_stays_one_until_its_merge_and_a_mark_that_fails_merges_nothing() {
    // The answer to the mark is lost: nothing merged, and the next run reads the pull request.
    let d = Delivery::new("deliver-draft", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    assert_eq!(
        d.rig.github.pulls()[0]["draft"],
        true,
        "publishing again leaves it a draft"
    );
    d.rig.github.fault("POST", "/graphql", Fault::LoseAnswer);
    let answer = d.merge(100);
    assert_eq!(answer["state"], "unavailable", "{answer}");
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .contains("nothing was merged"),
        "{answer}"
    );
    assert_eq!(
        d.writes()
            .iter()
            .filter(|write| write.starts_with("PUT "))
            .count(),
        0
    );
    // It was marked: the next merge finds it ready and marks nothing twice.
    assert_eq!(d.merge(100)["state"], "merged");
    assert_eq!(
        d.writes()
            .iter()
            .filter(|write| *write == "POST /graphql")
            .count(),
        1
    );
    // A token that may not mark it: refused by kind, still a draft, nothing merged.
    let d = Delivery::new("deliver-draft-refused", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    d.rig
        .github
        .fault("POST", "/graphql", Fault::Status(403, Vec::new()));
    let answer = d.merge(100);
    assert_eq!(
        (&answer["state"], &answer["kind"]),
        (&json!("refused"), &json!("permission"))
    );
    assert_eq!(
        answer["why"],
        "GitHub left pull request #100 a draft (403): "
    );
    let pull = d.rig.github.pulls().remove(0);
    assert_eq!(
        (&pull["draft"], &pull["merged"]),
        (&json!(true), &json!(false))
    );
    let journal: Vec<String> = (d.rig.journal().iter())
        .map(|entry| format!("{} {:?}", entry.kind, entry.event))
        .collect();
    assert!(journal.contains(&"ready Failed".to_string()), "{journal:?}");
    d.assert_no_token();
}

/// The evidence as the board's own type writes it, so the two sides cannot drift apart: a land
/// that refuses a head it has a pull request for hands over exactly this, not eligible
/// (`Landing::bound_before` in the board's `land/delivery.rs`).
fn withdrawn(head: &str) -> Value {
    use symphony_board::land::evidence::{Assessment, Snapshot, Stamp, Standing, VERSION};
    let stamp = |name: &str, standing| Stamp {
        name: name.into(),
        standing,
        detail: "a private line".into(),
        evidence: Vec::new(),
    };
    serde_json::to_value(Snapshot {
        version: VERSION,
        slug: SLUG.into(),
        task: 7,
        status: "done".into(),
        head: head.into(),
        branch: "main".into(),
        policy: "p".into(),
        catalogue: "c".into(),
        assessment: Some(Assessment {
            by: "calm-owl-2".into(),
            tip: Some(head.into()),
            why: "a private reason".into(),
            stamps: vec!["tests".into(), "review".into()],
            import: None,
        }),
        assessed: true,
        stamps: vec![
            stamp("tests", Standing::Missing),
            stamp("review", Standing::Met),
            stamp("pictures", Standing::Waived),
        ],
        blocking: vec!["tests: red at the head".into()],
        eligible: false,
        revision: "withdrawn".into(),
    })
    .unwrap()
}

#[test]
fn evidence_the_board_withdrew_fails_the_required_check_of_a_pull_request_already_ready() {
    let d = Delivery::new("deliver-withdrawn", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    // A merge GitHub refused leaves the pull request ready, its checks green.
    d.rig
        .github
        .set_pull(100, &json!({"mergeable_state": "blocked"}));
    assert_eq!(d.merge(100)["kind"], "checks");
    assert_eq!(d.rig.github.pulls()[0]["draft"], false);
    assert!(names(&d.rig.github.checks(HEAD)).contains(&("symphony/eligible", "success")));
    // The board's own snapshot of the withdrawal, at that head.
    let before = d.writes().len();
    assert_eq!(d.publish(&withdrawn(HEAD))["state"], "published");
    let checks = d.rig.github.checks(HEAD);
    assert_eq!(checks.len(), 5, "each name once: {checks:?}");
    assert!(names(&checks).contains(&("symphony/eligible", "failure")));
    assert!(names(&checks).contains(&("symphony/tests", "failure")));
    let first: Value = serde_json::from_str(
        &(d.rig
            .github
            .asked()
            .into_iter()
            .skip_while(|(method, ..)| method != "PUT"))
        .find(|(method, ..)| method == "PATCH")
        .unwrap()
        .2
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        first["name"], "symphony/eligible",
        "the verdict fails before any stamp moves"
    );
    assert_eq!(
        d.writes().len() - before,
        5,
        "each check is of the new revision"
    );
    // At a head the branch moved to: pushed, so the pull request is at it, failing there.
    let other = "b".repeat(40);
    assert_eq!(d.publish(&withdrawn(&other))["head"], other.as_str());
    assert!(names(&d.rig.github.checks(&other)).contains(&("symphony/eligible", "failure")));
    assert_eq!(d.rig.github.pulls()[0]["head"]["sha"], other.as_str());
    d.assert_no_token();
}
