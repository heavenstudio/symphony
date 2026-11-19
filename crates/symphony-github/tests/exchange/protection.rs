//! What a delivery refuses of a repository whose default branch is not protected for one: asked
//! through the verbs a land asks (`deliver.rs`'s rig), of the repository in memory, which
//! answers its rules and rulesets and holds a merge to them as GitHub does. Each case starts
//! from the protection a delivery needs and takes one thing from it.

use crate::deliver::{APP, Delivery, HEAD, LANDING, READER, SLUG, snapshot};
use crate::github::Fault;
use serde_json::{Value, json};

/// The rules of ruleset 1 as `Fake::protect` makes them, for a case to change.
fn firm_rules() -> Value {
    json!([
        {"type": "pull_request", "parameters": {"allowed_merge_methods": ["squash"]}},
        {"type": "required_status_checks", "parameters": {
            "strict_required_status_checks_policy": false,
            "required_status_checks": [{"context": "symphony/eligible", "integration_id": APP}],
        }},
        {"type": "non_fast_forward"},
        {"type": "deletion"},
    ])
}

fn landing() -> Value {
    json!([{"actor_type": "Integration", "actor_id": LANDING, "bypass_mode": "always"}])
}

/// `rules` with the rule of type `kind` left out.
fn without(rules: &Value, kind: &str) -> Value {
    json!(
        (rules.as_array().unwrap().iter())
            .filter(|rule| rule["type"] != json!(kind))
            .collect::<Vec<_>>()
    )
}

/// The lines of what an answer says is missing.
fn missing(answer: &Value) -> Vec<String> {
    assert_eq!(
        (&answer["state"], &answer["kind"]),
        (&json!("refused"), &json!("policy")),
        "{answer}"
    );
    (answer["missing"].as_array().unwrap().iter())
        .map(|line| line.as_str().unwrap().to_string())
        .collect()
}

/// A publication refused for its policy: nothing pushed and nothing written to GitHub.
fn refused_unpublished(d: &Delivery) -> Vec<String> {
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    let missing = missing(&answer);
    assert!(d.remote.pushed.borrow().is_empty(), "pushed: {answer}");
    assert_eq!(d.writes(), Vec::<String>::new());
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .contains("Nothing was pushed or merged"),
        "{answer}"
    );
    d.assert_no_token();
    missing
}

#[test]
fn a_branch_nothing_protects_is_published_to_and_merged_into_by_nobody() {
    let d = Delivery::new("protection-absent", "");
    d.rig.github.ruleset(1, &Value::Null, &Value::Null);
    d.rig.github.ruleset(2, &Value::Null, &Value::Null);
    assert_eq!(
        refused_unpublished(&d),
        [
            "no rule in effect for main requires a pull request",
            "no rule in effect for main blocks a force push",
            "no rule in effect for main blocks its deletion",
            "no rule in effect for main requires symphony/eligible from App 42",
            "no rule restricts updates of main to the landing App 43: anyone who may write merges \
             a pull request whose symphony/eligible is green, its evidence withdrawn since or not",
        ]
    );
    // The refusal is journaled, with what was missing and no digest of a policy that held.
    let journal = d.rig.journal();
    let last = journal.last().unwrap();
    assert_eq!((last.kind.as_str(), last.target.as_str()), ("policy", SLUG));
    assert_eq!(last.result.as_array().unwrap().len(), 5);
}

/// What a case takes from the protection a delivery needs.
type Change = fn(&Delivery);

/// Each case refuses the publication, naming `line` among what is missing.
fn missed(cases: &[(&str, Change, &str)]) {
    for (case, change, line) in cases {
        let d = Delivery::new("protection-missing", "");
        change(&d);
        let missing = refused_unpublished(&d);
        assert!(
            missing.iter().any(|said| said.starts_with(line)),
            "{case}: {missing:?}"
        );
    }
}

#[test]
fn each_rule_a_delivery_needs_is_missed_by_name() {
    let cases: [(&str, Change, &str); 7] = [
        (
            "another App's check",
            |d| {
                let mut rules = firm_rules();
                rules[1]["parameters"]["required_status_checks"][0]["integration_id"] = json!(99);
                d.rig.github.ruleset(1, &json!([]), &rules);
            },
            "symphony/eligible is required of main from another source than App 42",
        ),
        (
            "a check from any source",
            |d| {
                let mut rules = firm_rules();
                rules[1]["parameters"]["required_status_checks"] =
                    json!([{"context": "symphony/eligible"}]);
                d.rig.github.ruleset(1, &json!([]), &rules);
            },
            "symphony/eligible is required of main from another source than App 42",
        ),
        (
            "another check required, not ours",
            |d| {
                let mut rules = firm_rules();
                rules[1]["parameters"]["required_status_checks"][0]["context"] = json!("ci/build");
                d.rig.github.ruleset(1, &json!([]), &rules);
            },
            "no rule in effect for main requires symphony/eligible from App 42",
        ),
        (
            "held up to date",
            |d| {
                let mut rules = firm_rules();
                rules[1]["parameters"]["strict_required_status_checks_policy"] = json!(true);
                d.rig.github.ruleset(1, &json!([]), &rules);
            },
            "a rule holds main's pull requests up to date with it before they merge",
        ),
        (
            "no pull request required",
            |d| {
                (d.rig.github).ruleset(1, &json!([]), &without(&firm_rules(), "pull_request"));
            },
            "no rule in effect for main requires a pull request",
        ),
        (
            "a force push allowed",
            |d| {
                (d.rig.github).ruleset(1, &json!([]), &without(&firm_rules(), "non_fast_forward"));
            },
            "no rule in effect for main blocks a force push",
        ),
        (
            "deletion allowed",
            |d| (d.rig.github).ruleset(1, &json!([]), &without(&firm_rules(), "deletion")),
            "no rule in effect for main blocks its deletion",
        ),
    ];
    missed(&cases);
}

#[test]
fn what_the_repository_permits_beside_a_delivery_is_missed_by_name() {
    let cases: [(&str, Change, &str); 10] = [
        (
            "a merge commit permitted beside the squash",
            |d| {
                d.rig
                    .github
                    .set_repository(&json!({"allow_merge_commit": true}));
                let mut rules = firm_rules();
                rules[0]["parameters"] = json!({});
                d.rig.github.ruleset(1, &json!([]), &rules);
            },
            "main permits these merge methods: merge, squash; a delivery takes exactly: squash",
        ),
        (
            "no squash permitted",
            |d| {
                d.rig
                    .github
                    .set_repository(&json!({"allow_squash_merge": false}));
            },
            "main permits these merge methods: none; a delivery takes exactly: squash",
        ),
        (
            "merge settings not shown",
            |d| {
                d.rig
                    .github
                    .set_repository(&json!({"allow_squash_merge": null}));
            },
            "the repository's merge methods are not shown to the merging token",
        ),
        (
            "auto-merge",
            |d| {
                d.rig
                    .github
                    .set_repository(&json!({"allow_auto_merge": true}));
            },
            "auto-merge is allowed",
        ),
        (
            "a merge queue",
            |d| {
                let mut rules = firm_rules();
                rules
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"type": "merge_queue"}));
                d.rig.github.ruleset(1, &json!([]), &rules);
            },
            "main merges through a merge queue",
        ),
        (
            "another default branch",
            |d| {
                d.rig
                    .github
                    .set_repository(&json!({"default_branch": "trunk"}));
            },
            "main is not the repository's default branch as GitHub names it",
        ),
        (
            "archived",
            |d| d.rig.github.set_repository(&json!({"archived": true})),
            "the repository is archived",
        ),
        (
            "a repository that does not say what the token may do",
            |d| {
                d.rig.github.set_repository(&json!({"permissions": null}));
            },
            "whether the merging token administers the repository is not shown",
        ),
        (
            "a token whose administration is not a plain no",
            |d| {
                d.rig
                    .github
                    .set_repository(&json!({"permissions": {"push": true}}));
            },
            "whether the merging token administers the repository is not shown",
        ),
        (
            "a token that administers",
            |d| {
                d.rig
                    .github
                    .set_repository(&json!({"permissions": {"admin": true}}));
            },
            "the merging token administers the repository",
        ),
    ];
    missed(&cases);
}

#[test]
fn a_rule_someone_bypasses_or_whose_bypass_is_not_shown_holds_nobody() {
    // A role let through the pull request and the check.
    let d = Delivery::new("protection-bypassed", "");
    let admins = json!([{"actor_type": "RepositoryRole", "actor_id": 5, "bypass_mode": "always"}]);
    d.rig.github.ruleset(1, &admins, &firm_rules());
    let missing = refused_unpublished(&d);
    assert_eq!(missing.len(), 4, "{missing:?}");
    assert_eq!(
        missing[0],
        "the rule that requires a pull request holds not everyone on main: ruleset 1 is bypassed \
         by RepositoryRole 5 (always)"
    );
    // The landing App itself let through them: what merges would merge past the evidence.
    d.rig.github.ruleset(1, &landing(), &firm_rules());
    assert!(
        refused_unpublished(&d)[3].ends_with("ruleset 1 is bypassed by Integration 43 (always)")
    );
    // A list the token is not shown is not an empty one.
    d.rig.github.ruleset(1, &Value::Null, &firm_rules());
    assert_eq!(
        refused_unpublished(&d)[1],
        "the rule that blocks a force push holds not everyone on main: who bypasses ruleset 1 is \
         not known (its bypass list is not shown to the policy token)"
    );
    // The update restriction: nobody through, others through beside the App, an App's mode
    // nobody knows, and a list not shown.
    d.rig.github.ruleset(1, &json!([]), &firm_rules());
    let team = json!({"actor_type": "Team", "actor_id": 7, "bypass_mode": "pull_request"});
    for (bypass, why) in [
        (
            json!([]),
            "ruleset 2 lets nobody through, the landing App neither",
        ),
        (
            json!([landing()[0], team]),
            "ruleset 2 lets through Integration 43 (always), Team 7 (pull_request)",
        ),
        (
            json!([{"actor_type": "Integration", "actor_id": 99, "bypass_mode": "always"}]),
            "ruleset 2 lets through Integration 99 (always)",
        ),
        (
            json!([{"actor_type": "Integration", "actor_id": LANDING, "bypass_mode": "sometimes"}]),
            "ruleset 2 lets through Integration 43 (sometimes)",
        ),
        (
            json!([{"actor_type": "OrganizationAdmin", "bypass_mode": "always"}]),
            "ruleset 2 lets through OrganizationAdmin of no number (always)",
        ),
        (
            Value::Null,
            "who ruleset 2 lets through is not known (its bypass list is not shown to the policy \
             token)",
        ),
    ] {
        d.rig
            .github
            .ruleset(2, &bypass, &json!([{"type": "update"}]));
        let missing = refused_unpublished(&d);
        assert_eq!(
            missing,
            [format!(
                "updates of main are not restricted to the landing App 43 alone (Integration 43): \
                 {why}"
            )]
        );
    }
    // Each mode GitHub has lets the App through, and it alone.
    for mode in ["pull_request", "exempt"] {
        let app = json!([{"actor_type": "Integration", "actor_id": LANDING, "bypass_mode": mode}]);
        d.rig.github.ruleset(2, &app, &json!([{"type": "update"}]));
        assert_eq!(
            d.publish(&snapshot(HEAD, "met", "r1"))["state"],
            "published"
        );
    }
}

#[test]
fn the_rules_in_effect_are_judged_whole_an_organizations_among_them() {
    // The pull request and the check come from an organization's ruleset, the rest from the
    // repository's: together they hold, and no one answer says so.
    let d = Delivery::new("protection-inherited", "");
    let rules = firm_rules();
    let inherited = json!([rules[0], rules[1]]);
    d.rig
        .github
        .ruleset(1, &json!([]), &json!([rules[2], rules[3]]));
    d.rig.github.ruleset(9, &json!([]), &inherited);
    (d.rig.github).set_ruleset(9, &json!({"source_type": "Organization", "source": "o"}));
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    let asked: Vec<String> = (d.rig.github.asked().into_iter())
        .map(|(_, path, _)| path)
        .filter(|path| path.contains("/rule"))
        .collect();
    assert_eq!(
        asked,
        [
            "/repos/o/n/rules/branches/main?per_page=100&page=1",
            "/repos/o/n/rulesets/1",
            "/repos/o/n/rulesets/2",
            "/repos/o/n/rulesets/9",
        ]
    );
    // An organization's ruleset whose bypass list a repository's token is not shown: the rules
    // it alone gives are not known to hold everyone.
    let d = Delivery::new("protection-inherited-unknown", "");
    d.rig
        .github
        .ruleset(1, &json!([]), &json!([rules[2], rules[3]]));
    d.rig.github.ruleset(9, &Value::Null, &inherited);
    let missing = refused_unpublished(&d);
    assert_eq!(missing.len(), 2, "{missing:?}");
    assert!(missing[1].ends_with(
        "requires symphony/eligible from App 42 holds not everyone on main: who bypasses ruleset 9 \
         is not known (its bypass list is not shown to the policy token)"
    ));
    // The same rule given by both: the repository's own, read and bypassed by nobody, holds.
    d.rig.github.ruleset(1, &json!([]), &firm_rules());
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    // A ruleset that is not in effect gives no rule, whatever it holds.
    let d = Delivery::new("protection-inactive", "");
    d.rig
        .github
        .set_ruleset(2, &json!({"enforcement": "evaluate"}));
    assert!(refused_unpublished(&d)[0].starts_with("no rule restricts updates of main"));
}

#[test]
fn every_page_of_the_rules_is_read_before_any_is_judged() {
    // The rules a delivery needs are past the first page.
    let d = Delivery::new("protection-pages", "");
    let filler: Vec<Value> = (0..150).map(|_| json!({"type": "creation"})).collect();
    d.rig.github.ruleset(0, &json!([]), &json!(filler));
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    let pages = (d.rig.github.asked().iter())
        .filter(|(_, path, _)| path.contains("/rules/branches/main"))
        .count();
    assert_eq!(pages, 2);
    // More pages than are read are judged not at all.
    let d = Delivery::new("protection-too-many", "");
    let filler: Vec<Value> = (0..1000).map(|_| json!({"type": "creation"})).collect();
    d.rig.github.ruleset(0, &json!([]), &json!(filler));
    assert_eq!(
        refused_unpublished(&d),
        ["main has more rules than 10 pages list: they are not judged in part"]
    );
    // A rule that names no type is not passed over.
    let d = Delivery::new("protection-unreadable-rule", "");
    d.rig
        .github
        .ruleset(3, &json!([]), &json!([{"parameters": {}}]));
    assert_eq!(
        refused_unpublished(&d),
        ["a rule of main does not read: it names no type"]
    );
}

#[test]
fn a_policy_github_does_not_show_is_refused_and_one_it_does_not_answer_is_unavailable() {
    let rules = "/repos/o/n/rules/branches/main";
    for (path, status, state, why) in [
        (
            rules,
            404,
            "refused",
            "the rules of main are not readable with the merging token (GitHub answered 404)",
        ),
        (
            rules,
            403,
            "refused",
            "the rules of main are not readable with the merging token (GitHub answered 403)",
        ),
        (
            rules,
            500,
            "unavailable",
            "GitHub answered 500 to GET /repos/o/n/rules/branches/main?per_page=100&page=1",
        ),
        (
            rules,
            429,
            "unavailable",
            "GitHub's rate limit: nothing more is sent now",
        ),
        (
            rules,
            401,
            "unavailable",
            "GitHub refused the credential (401)",
        ),
        (
            "/repos/o/n/rulesets/2",
            500,
            "unavailable",
            "GitHub answered 500 to GET /repos/o/n/rulesets/2",
        ),
        (
            "/repos/o/n/rulesets/2",
            404,
            "refused",
            "who ruleset 2 lets through is not known (GitHub answered 404 to the policy token)",
        ),
    ] {
        let d = Delivery::new("protection-unavailable", "");
        d.rig
            .github
            .fault("GET", path, Fault::Status(status, Vec::new()));
        let answer = d.publish(&snapshot(HEAD, "met", "r1"));
        assert_eq!(answer["state"], state, "{path} {status}: {answer}");
        assert!(answer["why"].as_str().unwrap().contains(why), "{answer}");
        assert!(d.remote.pushed.borrow().is_empty());
        assert_eq!(d.writes(), Vec::<String>::new());
        // Read again by the next run, which goes on where it holds.
        assert_eq!(
            d.publish(&snapshot(HEAD, "met", "r1"))["state"],
            "published"
        );
    }
    // An answer lost on its way back is no policy read.
    let d = Delivery::new("protection-lost", "");
    d.rig.github.fault("GET", rules, Fault::LoseAnswer);
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(answer["state"], "unavailable", "{answer}");
    assert!(d.remote.pushed.borrow().is_empty());
    // A ruleset that answers for another number, or not as active, is not read as the rule's.
    let d = Delivery::new("protection-other-ruleset", "");
    d.rig.github.set_ruleset(1, &json!({"id": 8}));
    assert!(refused_unpublished(&d)[0].ends_with("(it does not read as active)"));
}

#[test]
fn a_policy_that_changed_since_the_publication_stops_the_merge() {
    let d = Delivery::new("protection-changed", "");
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    let held = |d: &Delivery| -> Vec<String> {
        (d.rig.journal().iter())
            .filter(|entry| entry.kind == "policy" && entry.result == json!("main"))
            .map(|entry| entry.op.clone())
            .collect()
    };
    assert_eq!(held(&d).len(), 1);
    // The update restriction is taken away between the publication and the merge.
    d.rig.github.ruleset(2, &Value::Null, &Value::Null);
    let writes = d.writes();
    let answer = d.merge(100);
    assert_eq!(missing(&answer).len(), 1, "{answer}");
    assert_eq!(
        d.writes(),
        writes,
        "neither marked ready nor asked to merge"
    );
    let pull = d.rig.github.pulls().remove(0);
    assert_eq!(
        (&pull["draft"], &pull["merged"]),
        (&json!(true), &json!(false))
    );
    assert_eq!(d.result(100)["state"], "open");
    // Put back as it was, the merge goes on, under the policy first read.
    d.rig.github.protect(APP, LANDING);
    assert_eq!(d.merge(100)["state"], "merged");
    let digests = held(&d);
    assert_eq!(digests.len(), 2);
    assert_eq!(digests[0], digests[1]);
    // Another policy that holds is another digest.
    let d = Delivery::new("protection-other", "");
    let app = json!([{"actor_type": "Integration", "actor_id": LANDING, "bypass_mode": "exempt"}]);
    d.rig.github.ruleset(2, &app, &json!([{"type": "update"}]));
    d.publish(&snapshot(HEAD, "met", "r1"));
    assert_ne!(held(&d)[0], digests[0]);
    // A pull request merged already is answered as merged whatever the policy is by then.
    let d = Delivery::new("protection-merged", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(d.merge(100)["state"], "merged");
    d.rig.github.ruleset(1, &Value::Null, &Value::Null);
    assert_eq!(d.merge(100)["state"], "merged");
    assert_eq!(d.result(100)["state"], "merged");
}

#[test]
fn a_head_that_moved_is_refused_as_a_head_whatever_the_policy() {
    for protected in [true, false] {
        let d = Delivery::new("protection-head", "");
        d.publish(&snapshot(HEAD, "met", "r1"));
        d.rig.github.push(SLUG, &"c".repeat(40));
        if !protected {
            d.rig.github.ruleset(2, &Value::Null, &Value::Null);
        }
        let writes = d.writes();
        let answer = d.merge(100);
        assert_eq!(
            (&answer["state"], &answer["kind"]),
            (&json!("refused"), &json!("head")),
            "{answer}"
        );
        assert_eq!(d.writes(), writes);
    }
}

#[test]
fn evidence_that_blocks_is_published_whatever_the_policy() {
    // A head published while the policy held; the evidence is withdrawn and the policy gone.
    let d = Delivery::new("protection-withdrawn", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    d.rig.github.ruleset(1, &Value::Null, &Value::Null);
    d.rig.github.ruleset(2, &Value::Null, &Value::Null);
    let answer = d.publish(&snapshot(HEAD, "missing", "r2"));
    assert_eq!(answer["state"], "published", "{answer}");
    let eligible: Vec<String> = (d.rig.github.checks(HEAD).into_iter())
        .filter(|(name, ..)| name == "symphony/eligible")
        .map(|(_, conclusion, _)| conclusion)
        .collect();
    assert_eq!(eligible, ["failure"]);
    // By its checks alone: nothing was pushed for it and no pull request touched.
    assert_eq!(d.remote.pushed.borrow().len(), 1);
    assert!(
        d.writes()[6..]
            .iter()
            .all(|write| write.contains("/check-runs")),
        "{:?}",
        d.writes()
    );
    // The same evidence standing again is not published, until the policy holds again.
    missing(&d.publish(&snapshot(HEAD, "met", "r3")));
    // A head the pull request is not at is not pushed past the policy to say so.
    let moved = "c".repeat(40);
    missing(&d.publish(&snapshot(&moved, "missing", "r4")));
    assert_eq!(d.remote.pushed.borrow().len(), 1);
    assert_eq!(d.rig.github.checks(&moved).len(), 0);
    // Nor is a pull request opened for a task bound to none.
    let d = Delivery::new("protection-withdrawn-unbound", "");
    d.rig.github.ruleset(2, &Value::Null, &Value::Null);
    missing(&d.publish(&snapshot(HEAD, "missing", "r1")));
    assert!(d.remote.pushed.borrow().is_empty());
    assert_eq!(d.writes(), Vec::<String>::new());
    // Where the policy holds, evidence that blocks is published as any is, the head with it.
    let d = Delivery::new("protection-withdrawn-held", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(
        d.publish(&snapshot(&moved, "missing", "r2"))["state"],
        "published"
    );
    assert_eq!(d.remote.pushed.borrow().len(), 2);
}

#[test]
fn only_the_landing_app_merges_a_pull_request_whose_check_is_green() {
    // The policy a delivery requires, as the host holds it: a person who may write, with the
    // check green, merges nothing.
    let d = Delivery::new("protection-writer", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    std::fs::write(&d.rig.token_file, "ghp_aPersonWhoMayWrite\n").unwrap();
    let answer = d.merge(100);
    assert_eq!(answer["state"], "refused", "{answer}");
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .contains("Cannot update this protected ref"),
        "{answer}"
    );
    assert_eq!(d.rig.github.pulls()[0]["merged"], false);
    // Nor does the App that writes the checks.
    std::fs::copy(d.checks_file(), &d.rig.token_file).unwrap();
    assert_eq!(d.merge(100)["state"], "refused");
    assert_eq!(d.rig.github.pulls()[0]["merged"], false);
}

#[test]
fn preflight_says_what_a_delivery_would_find_and_writes_nothing() {
    let d = Delivery::new("protection-command", "");
    let ran = d.run(&["preflight"]);
    assert_eq!(ran.code, 0, "{}{}", ran.out, ran.err);
    let lines: Vec<&str> = ran.out.lines().collect();
    assert_eq!(
        lines[0],
        "o/n (id 77, bound to nothing here yet), main: checks from App 42, merged by App 43, bypass \
         lists read by App 44"
    );
    assert!(
        lines[1].starts_with("holds: a delivery would go on (policy "),
        "{}",
        ran.out
    );
    // Nothing kept and nothing asked but reads.
    assert!(!d.rig.dir.0.join("state").exists());
    assert_eq!(d.rig.github.writes(), 0);
    // Each thing missing on a line of its own.
    d.rig.github.ruleset(2, &Value::Null, &Value::Null);
    d.rig
        .github
        .set_repository(&json!({"allow_auto_merge": true}));
    let ran = d.run(&["preflight", "--branch", "main"]);
    assert_eq!(ran.code, 1);
    let lines: Vec<&str> = ran.out.lines().collect();
    assert_eq!(lines[1], "does not hold: a delivery would be refused");
    assert_eq!(lines.len(), 4, "{}", ran.out);
    assert!(lines[2].starts_with("  missing: auto-merge is allowed"));
    assert!(lines[3].starts_with("  missing: no rule restricts updates of main"));
    // Another branch than the default has no rule and is not the default.
    let ran = d.run(&["preflight", "--branch", "release/1"]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.out
            .contains("release/1 is not the repository's default branch"),
        "{}",
        ran.out
    );
    // What does not read as a branch or an argument is refused before anything is asked.
    let asked = d.rig.github.asked().len();
    for args in [
        &["preflight", "--branch", "-x y"][..],
        &["preflight", "more"],
        &["preflight", "--branch"],
    ] {
        assert_eq!(d.run(args).code, 1);
    }
    assert_eq!(d.rig.github.asked().len(), asked);
    // GitHub not answering is said, and is no pass.
    d.rig
        .github
        .fault("GET", "/repos/o/n", Fault::Status(500, Vec::new()));
    let ran = d.run(&["preflight"]);
    assert_eq!(
        (ran.code, ran.out.as_str()),
        (1, "GitHub answered 500 to GET /repos/o/n\n")
    );
    // Once a delivery bound the repository, it is held to that number.
    d.rig.github.protect(APP, LANDING);
    d.rig
        .github
        .set_repository(&json!({"allow_auto_merge": false}));
    d.publish(&snapshot(HEAD, "met", "r1"));
    d.rig.github.replace_repository(78);
    let ran = d.run(&["preflight"]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.out.starts_with("o/n (id 77, as bound here), main"),
        "{}",
        ran.out
    );
    assert!(
        ran.out
            .contains("the repository answering is number 78, not the 77 bound")
    );
    assert!(!ran.out.contains(crate::TOKEN));
}

#[test]
fn preflight_asks_nothing_where_the_settings_serve_no_delivery() {
    let d = Delivery::new("protection-command-off", "");
    for settings in [
        None,
        Some("pull_requests = \"track\""),
        Some("webhook = true"),
    ] {
        match settings {
            None => d.rig.settings.set(None),
            Some(more) => d.rig.enable(more),
        }
        let ran = d.run(&["preflight"]);
        assert_eq!(ran.code, 1);
        assert!(ran.out.starts_with("turned off: "), "{}", ran.out);
    }
    assert_eq!(d.rig.github.asked().len(), 0);
    // A token file that does not hold as one must is said, and nothing is asked.
    let d = Delivery::new("protection-command-token", "");
    std::fs::remove_file(&d.rig.token_file).unwrap();
    let ran = d.run(&["preflight"]);
    assert_eq!(ran.code, 1);
    assert!(ran.out.starts_with("token_file "), "{}", ran.out);
    assert_eq!(d.rig.github.asked().len(), 0);
    // A repository that no longer answers at its name is not readable, bound or not.
    let d = Delivery::new("protection-command-moved", "");
    d.rig.github.rename("n2");
    let ran = d.run(&["preflight"]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.out.ends_with(
            "  missing: the repository is not readable with the merging token (GitHub answered \
             301)\n"
        ),
        "{}",
        ran.out
    );
}

#[test]
fn who_bypasses_the_rules_is_read_with_the_policy_token_and_it_is_sent_with_nothing_else() {
    let d = Delivery::new("protection-reader", "");
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    assert_eq!(d.merge(100)["state"], "merged");
    assert_eq!(d.result(100)["state"], "merged");
    assert_eq!(d.run(&["preflight"]).code, 0);
    // Over a whole delivery and a preflight, the policy App asked for the two rulesets and
    // nothing else, and wrote nothing.
    let asked = d.rig.github.asked_as(READER);
    assert_eq!(asked.len(), 6, "{asked:?}");
    for (method, path) in &asked {
        assert_eq!(method, "GET");
        assert!(
            ["/repos/o/n/rulesets/1", "/repos/o/n/rulesets/2"].contains(&path.as_str()),
            "{path}"
        );
    }
    // Neither other App asked for a ruleset: the lists come from the policy token alone.
    for app in [APP, LANDING] {
        assert!(
            (d.rig.github.asked_as(app).iter()).all(|(_, path)| !path.contains("/rulesets/")),
            "App {app}"
        );
    }
    d.assert_no_token();
}

#[test]
fn a_policy_token_shown_no_list_or_not_there_delivers_nothing() {
    // The landing token in the policy token's place is shown no list, as GitHub shows it none.
    let d = Delivery::new("protection-reader-hidden", "");
    std::fs::copy(&d.rig.token_file, d.policy_file()).unwrap();
    let hidden = refused_unpublished(&d);
    assert_eq!(hidden.len(), 5, "{hidden:?}");
    for line in &hidden {
        assert!(
            line.ends_with("(its bypass list is not shown to the policy token)"),
            "{line}"
        );
    }
    let ran = d.run(&["preflight"]);
    assert_eq!(ran.code, 1);
    assert_eq!(ran.out.matches("  missing: ").count(), 5, "{}", ran.out);
    // No policy token: refused as a policy that does not hold, with nothing asked of the
    // rulesets, and said by the preflight before it reads anything.
    let d = Delivery::new("protection-reader-absent", "");
    std::fs::remove_file(d.policy_file()).unwrap();
    let absent = refused_unpublished(&d);
    assert_eq!(absent.len(), 1, "{absent:?}");
    assert!(
        absent[0].starts_with(
            "who bypasses the rules of main cannot be read, policy_token_file not being usable: \
             token_file "
        ),
        "{absent:?}"
    );
    assert_eq!(
        d.rig.github.asked_as(READER),
        Vec::<(String, String)>::new()
    );
    let ran = d.run(&["preflight"]);
    assert_eq!(ran.code, 1);
    assert!(ran.out.lines().last().unwrap().starts_with("token_file "));
    // Evidence that blocks is still said where the policy token is gone: a withdrawal waits
    // on no third credential.
    let d = Delivery::new("protection-reader-withdrawal", "");
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    std::fs::remove_file(d.policy_file()).unwrap();
    let answer = d.publish(&snapshot(HEAD, "missing", "r2"));
    assert_eq!(answer["state"], "published", "{answer}");
    assert!(
        (d.rig.github.checks(HEAD).iter())
            .any(|(name, conclusion, _)| name == "symphony/eligible" && conclusion == "failure"),
        "{:?}",
        d.rig.github.checks(HEAD)
    );
    // And a merge is refused there.
    assert_eq!(missing(&d.merge(100)).len(), 1);
    assert_eq!(d.rig.github.pulls()[0]["merged"], false);
}
