//! `symphony-github rulesets [--verify]` as who administers the repository runs it: through a gh
//! CLI that answers what GitHub answers them (`Delivery::gh`), of the repository in memory. It
//! says each ruleset in effect and who bypasses it, judges the policy as a delivery does, and
//! with `--verify` keeps the rulesets where it holds, which a delivery then takes.

use crate::deliver::{APP, Delivery, HEAD, snapshot};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use symphony_github::state::{Store, Verified};

/// The rulesets the state keeps as verified, none where there is no state.
fn kept(d: &Delivery) -> BTreeMap<i64, Verified> {
    Store::existing(&d.rig.dir.0.join("state"))
        .and_then(|store| store.state().unwrap())
        .map(|state| state.rulesets)
        .unwrap_or_default()
}

/// When GitHub says ruleset `id` last changed.
fn changed_at(d: &Delivery, id: i64) -> String {
    let read = d
        .rig
        .github
        .person_reads(&format!("/repos/o/n/rulesets/{id}"));
    let read: Value = serde_json::from_str(&read.body).unwrap();
    read["updated_at"].as_str().unwrap().to_string()
}

/// `rulesets ARGS` run with `gh` as the gh CLI.
fn with_gh(d: &Delivery, gh: &Path, args: &[&str]) -> crate::Ran {
    let _alone = crate::starting();
    let mut all = vec!["rulesets"];
    all.extend_from_slice(args);
    crate::run_gh(
        &all,
        &d.rig.board,
        &d.rig.settings,
        &d.connect,
        d.rig.now.get(),
        vec![d.rig.dir.0.join("repository")],
        gh.to_str().unwrap(),
    )
}

/// A gh CLI that runs `body` whatever it is asked.
fn script(d: &Delivery, body: &str) -> std::path::PathBuf {
    let gh = d.rig.dir.0.join("other-gh");
    std::fs::write(&gh, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    gh
}

const HOLDS: &str =
    "holds; `--verify` keeps it, which a delivery takes while each ruleset is unchanged\n";

#[test]
fn it_says_each_ruleset_and_who_bypasses_it_and_keeps_only_with_verify() {
    let d = Delivery::unverified("rulesets-said", "");
    let team = json!([{"actor_type": "Team", "actor_id": 7, "bypass_mode": "pull_request"}]);
    d.rig
        .github
        .ruleset(9, &team, &json!([{"type": "creation"}]));
    // A name is said in plain characters alone.
    (d.rig.github).set_ruleset(9, &json!({"name": "evil\n\"name\"`$(x)"}));
    let (one, nine) = (changed_at(&d, 1), changed_at(&d, 9));
    let said = format!(
        "o/n (id 77), main: 2 ruleset(s) in effect, read with gh\n  ruleset 1 \"ruleset 1\" \
         (active), last changed {one}: bypassed by nobody\n  ruleset 9 \"evilnamex\" (active), \
         last changed {nine}: bypassed by Team 7 (pull_request)\n"
    );
    let ran = d.rulesets(&[]);
    assert_eq!(
        (ran.code, ran.out.as_str()),
        (0, format!("{said}{HOLDS}").as_str())
    );
    assert_eq!(ran.err, "");
    // gh read it all: nothing was asked with the App's token.
    assert_eq!(d.rig.github.asked().len(), 0);
    // Without --verify it keeps nothing, and a delivery still knows no ruleset.
    assert_eq!(kept(&d), BTreeMap::new());
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(answer["state"], "refused", "{answer}");
    // Verified, each ruleset is kept as it was read, and a delivery takes it.
    d.rig.now.set(1_234);
    let ran = d.verify();
    assert_eq!(
        (ran.code, ran.out),
        (
            0,
            format!(
                "{said}holds, and verified: 2 ruleset(s) kept, which a delivery takes until one \
                 changes\n"
            )
        )
    );
    let verified = |updated_at: &str, bypass: &[(&str, i64, &str)]| Verified {
        repository: 77,
        updated_at: updated_at.into(),
        bypass: (bypass.iter())
            .map(|(kind, id, mode)| ((*kind).into(), Some(*id), (*mode).into()))
            .collect(),
        at: 1_234,
    };
    assert_eq!(
        kept(&d),
        BTreeMap::from([
            (1, verified(&one, &[])),
            (9, verified(&nine, &[("Team", 7, "pull_request")])),
        ])
    );
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    d.assert_no_token();
}

#[test]
fn a_policy_that_does_not_hold_says_each_missing_line_and_keeps_the_earlier_record() {
    let d = Delivery::new("rulesets-missing", "");
    let before = kept(&d);
    assert_eq!(before.len(), 1, "{before:?}");
    let admins = json!([{"actor_type": "RepositoryRole", "actor_id": 5, "bypass_mode": "always"}]);
    d.rig
        .github
        .set_ruleset(1, &json!({"bypass_actors": admins}));
    d.rig
        .github
        .ruleset(2, &json!([]), &json!([{"type": "update"}]));
    let (one, two) = (changed_at(&d, 1), changed_at(&d, 2));
    let by = "holds not everyone on main: ruleset 1 is bypassed by RepositoryRole 5 (always)";
    let said = |tail: &str| {
        format!(
            "o/n (id 77), main: 2 ruleset(s) in effect, read with gh\n  ruleset 1 \"ruleset 1\" \
             (active), last changed {one}: bypassed by RepositoryRole 5 (always)\n  ruleset 2 \
             \"ruleset 2\" (active), last changed {two}: bypassed by nobody\ndoes not hold: a \
             delivery would be refused{tail}\n  missing: the rule that requires a pull request \
             {by}\n  missing: the rule that blocks a force push {by}\n  missing: the rule that \
             blocks its deletion {by}\n  missing: the rule that requires symphony/eligible from \
             App 42 {by}\n  missing: a rule restricts updates of main (ruleset 2): a pull request \
             whose symphony/eligible is green merges only past it, and the check alone holds a \
             merge to its evidence\n"
        )
    };
    let ran = d.rulesets(&[]);
    assert_eq!((ran.code, ran.out), (1, said("")));
    // A refused verification keeps nothing: the earlier record stands.
    let ran = d.verify();
    assert_eq!((ran.code, ran.out), (1, said("; nothing kept")));
    assert_eq!(kept(&d), before);
    // Put right, verifying again replaces it.
    d.rig.github.set_ruleset(1, &json!({"bypass_actors": []}));
    d.rig.github.ruleset(2, &Value::Null, &Value::Null);
    d.verify().ok();
    let after = kept(&d);
    assert_ne!(after, before);
    assert_eq!(after[&1].updated_at, changed_at(&d, 1));
    assert_eq!(after[&1].bypass, Vec::new());
}

#[test]
fn a_gh_not_signed_in_as_an_administrator_keeps_nothing_and_says_why() {
    let d = Delivery::unverified("rulesets-not-admin", "");
    d.rig
        .github
        .set_ruleset(1, &json!({"bypass_actors": Value::Null}));
    let at = changed_at(&d, 1);
    let why = "who bypasses ruleset 1 is not known (its bypass list is not shown: gh is not \
               signed in as someone who administers the repository)";
    let ran = d.verify();
    assert_eq!(ran.code, 1, "{ran:?}");
    let lines: Vec<&str> = ran.out.lines().collect();
    let read = format!(
        "  ruleset 1 \"ruleset 1\" (active), last changed {at}: who bypasses it is not known (its \
         bypass list is not shown: gh is not signed in as someone who administers the repository)"
    );
    assert_eq!(
        lines[..3],
        [
            "o/n (id 77), main: 1 ruleset(s) in effect, read with gh",
            read.as_str(),
            "does not hold: a delivery would be refused; nothing kept",
        ]
    );
    assert_eq!(lines.len(), 7, "{}", ran.out);
    for line in &lines[3..] {
        assert!(
            line.starts_with("  missing: the rule that ") && line.ends_with(why),
            "{line}"
        );
    }
    assert_eq!(kept(&d), BTreeMap::new());
    // A ruleset that answers as another is not read as itself, and nothing is kept either.
    d.rig
        .github
        .set_ruleset(1, &json!({"bypass_actors": [], "id": 8}));
    let ran = d.verify();
    assert_eq!(ran.code, 1, "{ran:?}");
    assert!(
        ran.out
            .contains(": who bypasses it is not known (it does not read as itself)\n"),
        "{}",
        ran.out
    );
    assert_eq!(kept(&d), BTreeMap::new());
}

#[test]
fn a_repository_other_than_the_one_bound_is_refused_and_nothing_kept() {
    let d = Delivery::new("rulesets-rebound", "");
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    let before = kept(&d);
    d.rig.github.replace_repository(78);
    let ran = d.verify();
    assert_eq!(
        (ran.code, ran.out.as_str()),
        (
            1,
            "not read whole, nothing kept: o/n is repository 78 on GitHub, and this project is \
             bound to 77 (o/n)\n"
        )
    );
    assert_eq!(kept(&d), before);
}

#[test]
fn gh_failing_is_said_and_nothing_kept() {
    let d = Delivery::new("rulesets-gh-fails", "");
    let before = kept(&d);
    // A page gh does not answer: the rules are not read whole.
    let filler: Vec<Value> = (0..150).map(|_| json!({"type": "creation"})).collect();
    d.rig.github.ruleset(0, &json!([]), &json!(filler));
    let ran = d.verify();
    assert_eq!(
        (ran.code, ran.out.as_str()),
        (
            1,
            "not read whole, nothing kept: gh api \
             repos/o/n/rules/branches/main?per_page=100&page=2: gh: Not Found (HTTP 404)\n"
        )
    );
    // gh not there.
    let absent = d.rig.dir.0.join("no-gh");
    let ran = with_gh(&d, &absent, &["--verify"]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.out.starts_with(&format!(
            "not read whole, nothing kept: {} is not run: ",
            absent.display()
        )),
        "{}",
        ran.out
    );
    // gh answering what is not JSON, a repository of no number, or saying why it failed.
    for (body, said) in [
        (
            "echo 'not json'",
            "not read whole, nothing kept: gh api repos/o/n: ",
        ),
        (
            "echo '{}'",
            "not read whole, nothing kept: gh api repos/o/n: names no repository number\n",
        ),
        (
            "printf 'HTTP 401: Bad credentials\\007\\nmore\\n' >&2; exit 1",
            "not read whole, nothing kept: gh api repos/o/n: HTTP 401: Bad credentials\n",
        ),
    ] {
        let ran = with_gh(&d, &script(&d, body), &["--verify"]);
        assert_eq!(ran.code, 1, "{body}");
        assert!(ran.out.starts_with(said), "{body}: {}", ran.out);
    }
    assert_eq!(kept(&d), before);
}

#[test]
fn settings_that_serve_no_delivery_ask_gh_nothing() {
    let d = Delivery::new("rulesets-off", "");
    let before = kept(&d);
    // A gh that leaves a mark wherever it is run.
    let mark = d.rig.dir.0.join("gh-ran");
    let gh = script(&d, &format!("touch '{}'; exit 1", mark.display()));
    let off = "turned off: the committed settings do not say enabled = true and pull_requests = \
               \"publish\"\n";
    for (settings, said) in [
        (None, off),
        (Some("pull_requests = \"track\"\napp_id = 42"), off),
        (
            Some("pull_requests = \"publish\""),
            "turned off: its settings do not read: symphony-github.toml: pull_requests = \
             \"publish\" needs app_id: the GitHub App token_file's token is of, whose checks the \
             default branch requires\n",
        ),
        (
            Some("issues = \"import\"\nimport_labels = [\"symphony\"]"),
            off,
        ),
    ] {
        match settings {
            None => d.rig.settings.set(None),
            Some(more) => d.rig.enable(more),
        }
        let ran = with_gh(&d, &gh, &["--verify"]);
        assert_eq!((ran.code, ran.out.as_str()), (1, said), "{settings:?}");
    }
    d.rig.settings.set(Some("enabled = maybe\n".into()));
    let ran = with_gh(&d, &gh, &["--verify"]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.out
            .starts_with("turned off: its settings do not read: "),
        "{}",
        ran.out
    );
    // What does not read as a branch or an argument is refused before gh is run.
    d.rig
        .enable(&format!("pull_requests = \"publish\"\napp_id = {APP}"));
    for args in [
        &["--branch", "-x y"][..],
        &["--verify", "more"],
        &["--branch"],
    ] {
        assert_eq!(with_gh(&d, &gh, args).code, 1, "{args:?}");
    }
    assert!(!mark.exists(), "gh was run");
    assert_eq!(kept(&d), before);
}
