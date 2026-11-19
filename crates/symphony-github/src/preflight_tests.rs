use super::*;

const WANTED: Wanted<'static> = Wanted {
    repository: 77,
    base: "main",
    checks_app: 42,
    landing_app: 43,
    operation: Operation::Task,
};

fn repository() -> Value {
    json!({
        "id": 77, "default_branch": "main", "archived": false, "allow_squash_merge": true,
        "allow_merge_commit": false, "allow_rebase_merge": false, "allow_auto_merge": false,
        "permissions": {"admin": false, "push": true},
    })
}

/// The rules a delivery needs: those nobody bypasses from ruleset 1, the update restriction
/// from ruleset 2.
fn rules() -> Vec<Value> {
    vec![
        json!({"type": "pull_request", "ruleset_id": 1,
               "parameters": {"allowed_merge_methods": ["squash"]}}),
        json!({"type": "required_status_checks", "ruleset_id": 1, "parameters": {
            "required_status_checks": [{"context": ELIGIBLE, "integration_id": 42}]}}),
        json!({"type": "non_fast_forward", "ruleset_id": 1}),
        json!({"type": "deletion", "ruleset_id": 1}),
        json!({"type": "update", "ruleset_id": 2}),
    ]
}

fn bypasses() -> BTreeMap<i64, Bypass> {
    BTreeMap::from([
        (1, Bypass::Actors(Vec::new())),
        (
            2,
            Bypass::Actors(vec![("Integration".into(), Some(43), "always".into())]),
        ),
    ])
}

#[test]
fn the_policy_a_delivery_needs_misses_nothing() {
    assert_eq!(
        judge(&repository(), &rules(), &bypasses(), &WANTED),
        Vec::<String>::new()
    );
    assert_eq!(Operation::Task.permitted(), ["squash"]);
}

#[test]
fn another_repository_or_one_that_says_nothing_of_itself_is_not_the_one_bound() {
    let mut other = repository();
    other["id"] = json!(78);
    assert_eq!(
        judge(&other, &rules(), &bypasses(), &WANTED),
        ["the repository answering is number 78, not the 77 bound"]
    );
    // An answer that holds none of what is read is missed on every count, never passed.
    let missing = judge(&json!({}), &rules(), &bypasses(), &WANTED);
    assert_eq!(missing.len(), 6, "{missing:?}");
    assert!(missing[2].starts_with("the repository is archived, or does not say it is not"));
    assert!(missing[3].starts_with("whether the merging token administers the repository is not"));
    assert!(missing[4].starts_with("auto-merge is allowed, or the repository's merge settings"));
}

#[test]
fn a_token_is_no_administrator_only_where_the_repository_says_so() {
    let judged = |admin: Value| {
        let mut repository = repository();
        repository["permissions"]["admin"] = admin;
        judge(&repository, &rules(), &bypasses(), &WANTED)
    };
    assert_eq!(judged(json!(false)), Vec::<String>::new());
    assert_eq!(
        judged(json!(true)),
        [
            "the merging token administers the repository: what merges must not be able to change \
          the rules it is held to"
        ]
    );
    // What is not a plain no is not known, whatever it reads as.
    for admin in [
        json!(null),
        json!("false"),
        json!(0),
        json!({}),
        json!([false]),
    ] {
        assert_eq!(
            judged(admin.clone()),
            [
                "whether the merging token administers the repository is not shown: the repository \
              does not say it does not"
            ],
            "{admin}"
        );
    }
    let mut none = repository();
    none.as_object_mut().unwrap().remove("permissions");
    assert_eq!(judge(&none, &rules(), &bypasses(), &WANTED).len(), 1);
}

#[test]
fn a_rule_of_a_ruleset_never_read_holds_nobody() {
    // A rule that names no ruleset.
    let mut rules = rules();
    rules[2] = json!({"type": "non_fast_forward"});
    assert_eq!(
        judge(&repository(), &rules, &bypasses(), &WANTED),
        ["the rule that blocks a force push holds not everyone on main: one from no ruleset named"]
    );
    // One whose ruleset was not read.
    rules[2] = json!({"type": "non_fast_forward", "ruleset_id": 5});
    assert_eq!(
        judge(&repository(), &rules, &bypasses(), &WANTED),
        ["the rule that blocks a force push holds not everyone on main: ruleset 5 was not read"]
    );
    rules[2] = json!({"type": "non_fast_forward", "ruleset_id": 1});
    rules[4] = json!({"type": "update"});
    let missing = judge(&repository(), &rules, &bypasses(), &WANTED);
    assert!(
        missing[0].ends_with("one from no ruleset read"),
        "{missing:?}"
    );
}

#[test]
fn a_rule_nobody_bypasses_narrows_the_methods_and_one_somebody_does_narrows_none() {
    let mut repository = repository();
    repository["allow_merge_commit"] = json!(true);
    // Narrowed to the squash by the rule that holds everyone.
    assert_eq!(
        judge(&repository, &rules(), &bypasses(), &WANTED),
        Vec::<String>::new()
    );
    // The same rule from a ruleset someone bypasses narrows nothing for them.
    let mut rules = rules();
    rules.push(json!({"type": "pull_request", "ruleset_id": 1, "parameters": {}}));
    rules[0]["ruleset_id"] = json!(2);
    assert_eq!(
        judge(&repository, &rules, &bypasses(), &WANTED),
        ["main permits these merge methods: merge, squash; a delivery takes exactly: squash"]
    );
}

#[test]
fn what_github_says_of_an_actor_is_said_in_plain_characters() {
    let mut bypasses = bypasses();
    bypasses.insert(
        1,
        Bypass::Actors(vec![(
            "Team\u{1b}[31m; rm".into(),
            Some(7),
            "al ways\n".into(),
        )]),
    );
    let missing = judge(&repository(), &rules(), &bypasses, &WANTED);
    assert!(
        missing[0].ends_with("ruleset 1 is bypassed by Team31mrm 7 (always)"),
        "{missing:?}"
    );
}
