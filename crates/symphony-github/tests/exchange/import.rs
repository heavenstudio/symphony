//! `symphony-github deliver` of an import (`operation` `import`): pushed and merged by a merge
//! commit only where the settings pin the upstream (`upstream_id`), the upstream read at its
//! address is that repository and not the one delivered to, and its history makes nothing private
//! public; and everything else squashed as a task, whatever it asks.

use crate::deliver::{APP, Delivery, HEAD, SLUG, snapshot};
use crate::github::Fault;
use serde_json::{Value, json};

/// What a case changes of a delivery before it publishes.
type Change = Box<dyn Fn(&Delivery)>;

/// The upstream commit imported.
const UPSTREAM: &str = "cccccccccccccccccccccccccccccccccccccccc";
/// The upstream's number at GitHub, as the settings pin it.
const PINNED: i64 = 88;

/// A delivery to a repository that imports from `up/stream`: the settings pin it, the default
/// branch permits merge commits beside squash, and both repositories are public.
fn importing(name: &str) -> Delivery {
    let d = Delivery::unverified(name, &format!("upstream_id = {PINNED}\n"));
    d.rig
        .github
        .set_repository(&json!({"allow_merge_commit": true, "private": false}));
    permits(&d, &["merge", "squash"]).ok();
    upstream(
        &d,
        json!({"id": PINNED, "full_name": "up/stream", "private": false}),
    );
    d
}

/// The default branch's rule that nobody bypasses permitting `methods`, as protected otherwise,
/// and what verifying its rulesets said: they are kept where the policy holds.
fn permits(d: &Delivery, methods: &[&str]) -> crate::Ran {
    let github = &d.rig.github;
    github.protect(APP);
    github.set_ruleset(
        1,
        &json!({"rules": [
            {"type": "pull_request", "parameters": {"allowed_merge_methods": methods}},
            {"type": "required_status_checks", "parameters": {
                "strict_required_status_checks_policy": false,
                "required_status_checks": [{"context": "symphony/eligible", "integration_id": APP}],
            }},
            {"type": "non_fast_forward"},
            {"type": "deletion"},
        ]}),
    );
    d.verify()
}

/// What `up/stream` says of itself from now.
fn upstream(d: &Delivery, about: Value) {
    d.rig.github.neighbour("up/stream", about);
}

/// The evidence of [`SLUG`] at [`HEAD`] standing, its assessment naming the import of `import`.
fn imported(import: Option<&str>) -> Value {
    let mut snapshot = snapshot(HEAD, "met", "r1");
    if let Some(import) = import {
        snapshot["assessment"]["import"] = json!(import);
    }
    snapshot
}

/// What a land says it delivers, laid over `request`: an import of [`UPSTREAM`] from `url`.
fn of(mut request: Value, url: &str) -> Value {
    request["operation"] = json!("import");
    request["upstream"] = json!({"url": url, "branch": "main", "commit": UPSTREAM});
    request
}

fn publish(d: &Delivery, snapshot: &Value, url: Option<&str>) -> Value {
    let request = json!({
        "verb": "publish", "slug": SLUG, "by": "amber-fox-1", "head": HEAD, "branch": "main",
        "title": "Merge branch 'sim/jump'", "body": "the private brief", "snapshot": snapshot,
        "operation": "task",
    });
    d.ask(
        "publish",
        &url.map_or_else(|| request.clone(), |url| of(request.clone(), url)),
    )
}

/// What a land asks to merge pull request 100 by `method`, as a task.
fn merge_request(method: &str) -> Value {
    json!({
        "verb": "merge", "slug": SLUG, "by": "amber-fox-1", "head": HEAD, "repository": 77,
        "pull": 100, "method": method, "title": "Merge branch 'sim/jump'",
        "body": "the private brief", "operation": "task",
    })
}

fn merge(d: &Delivery, method: &str, url: Option<&str>) -> Value {
    let request = merge_request(method);
    d.ask(
        "merge",
        &url.map_or_else(|| request.clone(), |url| of(request.clone(), url)),
    )
}

/// The `why` of `answer`, a refusal of `d`'s that pushed nothing.
fn not_pushed(d: &Delivery, answer: &Value) -> String {
    assert!(d.remote.pushed.borrow().is_empty(), "{answer}");
    why(answer).to_string()
}

/// The `why` of a refusal.
fn why(answer: &Value) -> &str {
    assert_eq!(answer["state"], "refused", "{answer}");
    answer["why"].as_str().unwrap()
}

const URL: &str = "https://github.com/up/stream.git";

#[test]
fn an_import_of_the_upstream_pinned_is_pushed_and_merged_by_a_merge_commit() {
    let d = importing("import-merged");
    let answer = publish(&d, &imported(Some(UPSTREAM)), Some(URL));
    assert_eq!(answer["state"], "published", "{answer}");
    assert_eq!(
        answer["upstream"],
        json!({"id": PINNED, "name": "up/stream"})
    );
    assert_eq!(
        *d.remote.pushed.borrow(),
        [(HEAD.to_string(), SLUG.to_string())]
    );
    // A merge commit, titled as a land's own merge, of the head and no other.
    assert_eq!(merge(&d, "merge", Some(URL))["state"], "merged");
    let merged = d.rig.github.pulls().remove(0)["merged_as"].clone();
    assert_eq!(
        (
            &merged["sha"],
            &merged["merge_method"],
            &merged["commit_title"]
        ),
        (
            &json!(HEAD),
            &json!("merge"),
            &json!("Merge branch 'sim/jump'")
        )
    );
    assert_eq!(
        merged["commit_message"],
        format!("Merged from {HEAD} by pull request #100, an import of {UPSTREAM}.")
    );
    d.assert_no_token();
}

#[test]
fn a_task_in_a_repository_that_imports_is_squashed_and_asks_for_nothing_else() {
    let d = importing("import-task");
    assert_eq!(publish(&d, &imported(None), None)["state"], "published");
    assert!(publish(&d, &imported(None), None).get("upstream").is_none());
    // A task asking for a merge commit, or an import asking for a squash, merges nothing.
    for (method, url) in [("merge", None), ("squash", Some(URL))] {
        assert_eq!(
            why(&merge(&d, method, url)),
            "a merge is of one whole commit: a task's squashed, an import's by a merge commit"
        );
    }
    assert!(!d.writes().iter().any(|write| write.starts_with("PUT ")));
    assert_eq!(merge(&d, "squash", None)["state"], "merged");
    let merged = d.rig.github.pulls().remove(0)["merged_as"].clone();
    assert_eq!(merged["merge_method"], "squash");
}

#[test]
fn an_import_the_settings_or_the_upstream_do_not_bear_out_is_refused_before_anything_is_pushed() {
    // Settings that pin no upstream deliver no import, as their branch permits squash alone.
    let d = Delivery::new("import-unpinned", "");
    d.rig.github.set_repository(&json!({"private": false}));
    upstream(&d, json!({"id": PINNED, "private": false}));
    assert_eq!(
        not_pushed(&d, &publish(&d, &imported(Some(UPSTREAM)), Some(URL))),
        "the settings name no upstream_id: no import is delivered here, from \
         https://github.com/up/stream.git or any other"
    );
    let refusals: [(&str, Change, &str); 8] = [
        (
            "https://example.com/up/stream",
            Box::new(|_| {}),
            "an import names its upstream by its address on GitHub.com and the commit it imports \
             whole",
        ),
        (
            "https://github.com/no/where",
            Box::new(|_| {}),
            "the upstream no/where is not readable with the App's token (404)",
        ),
        (
            URL,
            Box::new(|d| upstream(d, json!({"id": 89, "private": false}))),
            "the upstream up/stream is repository 89, not the 88 upstream_id pins: nothing is \
             imported from another",
        ),
        (
            URL,
            Box::new(|d| upstream(d, json!({"id": PINNED, "private": true}))),
            "the upstream up/stream is private and o/n is not: an import would publish its history",
        ),
        (
            URL,
            Box::new(|d| upstream(d, json!({"id": PINNED}))),
            "whether the upstream or this repository is private is not shown: an import is made \
             only where nothing private is published",
        ),
        (
            URL,
            Box::new(|d| {
                let moved = Fault::Status(301, Vec::new());
                d.rig.github.fault("GET", "/repos/up/stream", moved);
            }),
            "the upstream up/stream moved (301)",
        ),
        (
            "https://github.com/o/n",
            Box::new(|_| {}),
            "the upstream o/n is repository 77, not the 88 upstream_id pins: nothing is imported \
             from another",
        ),
        (
            URL,
            Box::new(|d| {
                // Verified while the settings pinned no upstream, as a task's policy: a
                // verification keeps nothing of one that does not hold, as this one for an import.
                let task = format!("pull_requests = \"publish\"\napp_id = {APP}\n");
                d.rig.enable(&task);
                permits(d, &["squash"]).ok();
                d.rig.enable(&format!("{task}upstream_id = {PINNED}\n"));
            }),
            "main permits these merge methods: squash; a delivery takes exactly: merge, squash",
        ),
    ];
    for (url, change, said) in refusals {
        let d = importing("import-refused");
        change(&d);
        let refused = not_pushed(&d, &publish(&d, &imported(Some(UPSTREAM)), Some(url)));
        assert!(refused.contains(said), "{url}: {refused}");
    }
}

#[test]
fn an_import_into_itself_or_of_other_evidence_is_refused_and_private_into_private_is_not() {
    // The repository delivered to is no upstream of its own.
    let d = Delivery::unverified("import-self", "upstream_id = 77\n");
    (d.rig.github).set_repository(&json!({"allow_merge_commit": true, "private": false}));
    permits(&d, &["merge", "squash"]).ok();
    assert_eq!(
        not_pushed(
            &d,
            &publish(
                &d,
                &imported(Some(UPSTREAM)),
                Some("https://github.com/o/n")
            )
        ),
        "upstream_id is the repository delivered to: an import is of another"
    );
    // A private upstream into a private repository publishes nothing private.
    let d = importing("import-private");
    d.rig.github.set_repository(&json!({"private": true}));
    upstream(&d, json!({"id": PINNED, "private": true}));
    assert_eq!(
        publish(&d, &imported(Some(UPSTREAM)), Some(URL))["state"],
        "published"
    );
    // Evidence that names another import, or none, than the operation asked for.
    let d = importing("import-other");
    let other = "d".repeat(40);
    let mut earlier = imported(Some(UPSTREAM));
    earlier["assessment"]["tip"] = json!(other);
    for (snapshot, url) in [
        (imported(Some(&other)), Some(URL)),
        (imported(None), Some(URL)),
        (imported(Some(UPSTREAM)), None),
        (earlier.clone(), Some(URL)),
    ] {
        assert_eq!(
            not_pushed(&d, &publish(&d, &snapshot, url)),
            "the evidence names another import, or none, than the operation asked for"
        );
    }
    // An import named at another head binds nothing at this one, which is a task's.
    assert_eq!(publish(&d, &earlier, None)["state"], "published");
}

#[test]
fn a_rule_requiring_a_linear_history_refuses_an_import_and_nothing_else() {
    let d = importing("import-linear");
    d.rig
        .github
        .ruleset(3, &json!([]), &json!([{"type": "required_linear_history"}]));
    let refused = why(&publish(&d, &imported(Some(UPSTREAM)), Some(URL))).to_string();
    assert!(
        refused.contains(
            "a rule requires a linear history of main, which refuses the merge commit an import is \
             delivered by"
        ),
        "{refused}"
    );
    // A repository that imports nothing takes the rule as it is.
    let d = Delivery::new("import-linear-task", "");
    d.rig
        .github
        .ruleset(3, &json!([]), &json!([{"type": "required_linear_history"}]));
    assert_eq!(publish(&d, &imported(None), None)["state"], "published");
}

#[test]
fn an_import_whose_upstream_changed_since_it_was_published_is_not_merged() {
    let d = importing("import-changed");
    assert_eq!(
        publish(&d, &imported(Some(UPSTREAM)), Some(URL))["state"],
        "published"
    );
    upstream(
        &d,
        json!({"id": PINNED, "full_name": "up/stream", "private": true}),
    );
    assert_eq!(
        why(&merge(&d, "merge", Some(URL))),
        "the upstream up/stream is private and o/n is not: an import would publish its history"
    );
    assert!(!d.writes().iter().any(|write| write.starts_with("PUT ")));
}

#[test]
fn an_import_is_armed_to_merge_by_a_merge_commit_while_its_upstream_bears_it_out() {
    let d = importing("import-armed");
    d.rig
        .github
        .set_repository(&json!({"allow_auto_merge": true}));
    let mut short = imported(Some(UPSTREAM));
    short["stamps"][0]["standing"] = json!("missing");
    short["eligible"] = json!(false);
    assert_eq!(publish(&d, &short, Some(URL))["state"], "published");
    let arm = |method: &str| {
        let mut request = merge_request(method);
        request["verb"] = json!("arm");
        d.ask("arm", &of(request, URL))
    };
    // Squashed it would lose the upstream's history: refused before anything is asked.
    let writes = d.writes();
    assert!(why(&arm("squash")).starts_with("a merge is of one whole commit"));
    assert_eq!(d.writes(), writes);
    // The upstream turned private since: nothing turned on.
    upstream(
        &d,
        json!({"id": PINNED, "full_name": "up/stream", "private": true}),
    );
    assert!(why(&arm("merge")).starts_with("the upstream up/stream is private"));
    assert_eq!(d.writes(), writes);
    upstream(
        &d,
        json!({"id": PINNED, "full_name": "up/stream", "private": false}),
    );
    assert_eq!(
        arm("merge"),
        json!({"state": "armed", "pull": 100, "head": HEAD})
    );
    let armed = d.rig.github.pulls().remove(0)["auto_merge"].clone();
    assert_eq!(
        (
            &armed["merge_method"],
            &armed["commit_title"],
            &armed["commit_message"]
        ),
        (
            &json!("merge"),
            &json!("Merge branch 'sim/jump'"),
            &json!(format!(
                "Merged from {HEAD} by pull request #100, an import of {UPSTREAM}."
            ))
        )
    );
}
