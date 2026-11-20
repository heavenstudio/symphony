use super::*;

const WANTED: Wanted<'static> = Wanted {
    repository: 77,
    base: "main",
    app: 42,
    operation: Operation::Task,
};

fn repository() -> Value {
    json!({
        "id": 77, "default_branch": "main", "archived": false, "allow_squash_merge": true,
        "allow_merge_commit": false, "allow_rebase_merge": false, "allow_auto_merge": false,
        "permissions": {"admin": false, "push": true},
    })
}

/// The rules a delivery needs, from ruleset 1, which nobody bypasses.
fn rules() -> Vec<Value> {
    vec![
        json!({"type": "pull_request", "ruleset_id": 1,
               "parameters": {"allowed_merge_methods": ["squash"]}}),
        json!({"type": "required_status_checks", "ruleset_id": 1, "parameters": {
            "required_status_checks": [{"context": ELIGIBLE, "integration_id": 42}]}}),
        json!({"type": "non_fast_forward", "ruleset_id": 1}),
        json!({"type": "deletion", "ruleset_id": 1}),
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
    assert_eq!(missing.len(), 5, "{missing:?}");
    assert!(missing[0].starts_with("whether the App's token administers the repository is not"));
    assert!(missing[3].starts_with("the repository is archived, or does not say it is not"));
    assert!(missing[4].starts_with("the repository's merge methods are not shown"));
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
            "the App's token administers the repository: what merges must not be able to change \
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
                "whether the App's token administers the repository is not shown: the repository \
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
}

#[test]
fn a_rule_that_restricts_updates_is_refused_whoever_it_lets_through() {
    // GitHub's auto-merge, armed by the App, merges only where no rule asks for a bypass, and the
    // check alone holds a merge to its evidence (docs/GITHUB.md).
    for (rule, named) in [
        (json!({"type": "update", "ruleset_id": 2}), "ruleset 2"),
        (json!({"type": "update", "ruleset_id": 1}), "ruleset 1"),
        (json!({"type": "update"}), "one from no ruleset named"),
    ] {
        let mut rules = rules();
        rules.push(rule);
        assert_eq!(
            judge(&repository(), &rules, &bypasses(), &WANTED),
            [format!(
                "a rule restricts updates of main ({named}): a pull request whose {ELIGIBLE} is \
                 green merges only past it, and the check alone holds a merge to its evidence"
            )]
        );
    }
    let mut two = rules();
    two.push(json!({"type": "update", "ruleset_id": 2}));
    two.push(json!({"type": "update", "ruleset_id": 1}));
    assert!(judge(&repository(), &two, &bypasses(), &WANTED)[0].contains("(ruleset 1, ruleset 2)"));
}

#[test]
fn auto_merge_allowed_or_not_is_no_part_of_the_policy() {
    let mut repository = repository();
    for allowed in [json!(true), json!(false), json!(null)] {
        repository["allow_auto_merge"] = allowed;
        assert_eq!(
            judge(&repository, &rules(), &bypasses(), &WANTED),
            Vec::<String>::new()
        );
    }
}

#[test]
fn a_read_made_by_who_administers_is_judged_on_its_rules_alone() {
    let mut administered = repository();
    administered["permissions"]["admin"] = json!(true);
    assert_eq!(
        judge(&administered, &rules(), &bypasses(), &WANTED).len(),
        1
    );
    assert_eq!(
        judge_rules(&administered, &rules(), &bypasses(), &WANTED),
        Vec::<String>::new()
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
fn a_repository_that_imports_permits_merge_commits_beside_squash_and_no_linear_history() {
    assert_eq!(Operation::of(false), Operation::Task);
    assert_eq!(Operation::of(true), Operation::Imports);
    assert_eq!(Operation::Imports.permitted(), ["merge", "squash"]);
    let imports = Wanted {
        operation: Operation::Imports,
        ..WANTED
    };
    let mut merges = repository();
    merges["allow_merge_commit"] = json!(true);
    let mut both = rules();
    both[0]["parameters"]["allowed_merge_methods"] = json!(["merge", "squash"]);
    assert_eq!(
        judge(&merges, &both, &bypasses(), &imports),
        Vec::<String>::new()
    );
    // Squash alone is a task's, and not what imports too.
    assert_eq!(
        judge(&merges, &rules(), &bypasses(), &imports),
        ["main permits these merge methods: squash; a delivery takes exactly: merge, squash"]
    );
    // A rule that requires a linear history refuses the merge commit an import is, and only that.
    let linear = json!({"type": "required_linear_history", "ruleset_id": 1});
    both.push(linear.clone());
    assert_eq!(
        judge(&merges, &both, &bypasses(), &imports),
        [
            "a rule requires a linear history of main, which refuses the merge commit an import \
             is delivered by"
        ]
    );
    let mut tasks = rules();
    tasks.push(linear);
    assert_eq!(
        judge(&repository(), &tasks, &bypasses(), &WANTED),
        Vec::<String>::new()
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

/// A transport that keeps what it was asked and answers each request with `answer`.
struct Kept {
    asked: std::cell::RefCell<Vec<crate::transport::Request>>,
    answer: (u16, String),
}

impl crate::transport::Transport for Kept {
    fn send(
        &self,
        request: &crate::transport::Request,
    ) -> Result<crate::transport::Response, crate::transport::Failure> {
        self.asked.borrow_mut().push(request.clone());
        Ok(crate::transport::Response {
            status: self.answer.0,
            headers: BTreeMap::new(),
            body: self.answer.1.clone(),
        })
    }
}

/// Ruleset 2 as verified of repository 77 when GitHub said it last changed at 04:03:25.191Z,
/// said as gh says it, in another zone, and bypassed by `bypass`.
fn verified(bypass: &[(&str, Option<i64>, &str)]) -> Verified {
    Verified {
        repository: 77,
        updated_at: "2026-11-20T05:03:25.191+01:00".into(),
        bypass: (bypass.iter())
            .map(|(kind, id, mode)| ((*kind).into(), *id, (*mode).into()))
            .collect(),
        at: 1,
    }
}

/// What a ruleset that shows no time it last changed is.
const SHOWS_NO_TIME: &str =
    "it shows no time it last changed that reads, so its verification cannot be held to it";

/// Ruleset 2 as the App's token reads it.
fn ruleset(updated_at: &str, can: &str) -> Value {
    json!({"id": 2, "enforcement": "active", "updated_at": updated_at,
           "current_user_can_bypass": can, "rules": []})
}

fn reading(
    status: u16,
    body: &Value,
    verified: Option<&Verified>,
) -> (Result<Bypass, Value>, Vec<crate::transport::Request>) {
    let kept = Kept {
        asked: std::cell::RefCell::default(),
        answer: (status, body.to_string()),
    };
    let host = Host {
        transport: &kept,
        repository: crate::config::Repository {
            owner: "o".into(),
            name: "n".into(),
        },
    };
    let read = bypass(&host, verified, 2, &WANTED);
    (read, kept.asked.into_inner())
}

const AT: &str = "2026-11-20T04:03:25.191Z";

#[test]
fn who_bypasses_a_ruleset_is_what_was_verified_while_it_is_unchanged() {
    let none = verified(&[]);
    let team = verified(&[("Team", Some(7), "pull_request")]);
    let unknown = |why: &str| Bypass::Unknown(why.into());
    let never = format!("it was never verified: {VERIFY}");
    let elsewhere = Verified {
        repository: 78,
        ..none.clone()
    };
    let own = verified(&[("Integration", Some(42), "pull_request")]);
    for (status, body, verified, read) in [
        // The same instant, whichever zone it is said in.
        (
            200,
            ruleset(AT, "never"),
            Some(&none),
            Bypass::Actors(Vec::new()),
        ),
        (
            200,
            ruleset(AT, "never"),
            Some(&team),
            Bypass::Actors(vec![("Team".into(), Some(7), "pull_request".into())]),
        ),
        // The App's own bypass is read, whatever was verified.
        (
            200,
            ruleset(AT, "always"),
            Some(&none),
            Bypass::Actors(vec![("Integration".into(), Some(42), "always".into())]),
        ),
        (
            200,
            ruleset(AT, "pull_requests_only"),
            Some(&own),
            Bypass::Actors(vec![(
                "Integration".into(),
                Some(42),
                "pull_request".into(),
            )]),
        ),
        (
            200,
            json!({"id": 2, "enforcement": "active", "updated_at": AT}),
            Some(&none),
            unknown("whether the App bypasses it is not shown"),
        ),
        // Changed since: an actor added or taken out, its enforcement, anything.
        (
            200,
            ruleset("2026-11-20T04:05:05.939Z", "never"),
            Some(&none),
            unknown(&format!(
                "it changed at 2026-11-20T04:05:05.939Z after it was verified as last changed at \
                 2026-11-20T05:03:25.191+01:00: {VERIFY}"
            )),
        ),
        // Never verified, or verified of another repository: not known, never empty.
        (200, ruleset(AT, "never"), None, unknown(&never)),
        (200, ruleset(AT, "never"), Some(&elsewhere), unknown(&never)),
        (
            200,
            json!({"id": 2, "enforcement": "disabled", "updated_at": AT}),
            Some(&none),
            unknown("it does not read as active"),
        ),
        (
            200,
            json!({"id": 3, "enforcement": "active", "updated_at": AT}),
            Some(&none),
            unknown("it does not read as active"),
        ),
        (
            404,
            json!({"message": "Not Found"}),
            Some(&none),
            unknown("GitHub answered 404 to the App's token"),
        ),
        (
            403,
            json!({"message": "Resource not accessible by integration"}),
            Some(&none),
            unknown("GitHub answered 403 to the App's token"),
        ),
    ] {
        let (answer, asked) = reading(status, &body, verified);
        assert_eq!(answer, Ok(read), "{body}");
        // One request, a read, of the ruleset and of nothing else, with nothing sent in it.
        assert_eq!(
            asked,
            [crate::transport::Request {
                method: "GET",
                path: "/repos/o/n/rulesets/2".into(),
                body: None,
            }]
        );
    }
}

#[test]
fn a_ruleset_or_a_record_with_no_time_it_last_changed_holds_nobody() {
    let timeless = Verified {
        updated_at: String::new(),
        ..verified(&[])
    };
    for (body, verified, why) in [
        (
            json!({"id": 2, "enforcement": "active", "current_user_can_bypass": "never"}),
            verified(&[]),
            SHOWS_NO_TIME.to_string(),
        ),
        (
            ruleset("yesterday", "never"),
            verified(&[]),
            SHOWS_NO_TIME.to_string(),
        ),
        (
            ruleset(AT, "never"),
            timeless,
            format!("its verification kept no time it last changed that reads: {VERIFY}"),
        ),
    ] {
        let (answer, _) = reading(200, &body, Some(&verified));
        assert_eq!(answer, Ok(Bypass::Unknown(why)), "{body}");
    }
}

#[test]
fn a_ruleset_github_gives_no_answer_for_is_unavailable_and_of_the_policy() {
    // No answer to judge is unavailable, never a list, and of the policy.
    for status in [401, 429, 500] {
        let (answer, asked) = reading(status, &json!({}), Some(&verified(&[])));
        let stopped = answer.unwrap_err();
        assert_eq!(
            (&stopped["state"], &stopped["kind"]),
            (&json!("unavailable"), &json!("policy")),
            "{status}"
        );
        assert!(
            (stopped["why"].as_str().unwrap())
                .starts_with("ruleset 2 was not read with the App's token: "),
            "{stopped}"
        );
        assert_eq!(asked.len(), 1);
    }
}

#[test]
fn githubs_times_read_as_one_instant_whichever_zone_says_them() {
    let at = instant(AT).unwrap();
    for same in [
        "2026-11-20T05:03:25.191+01:00",
        "2026-11-19T23:33:25.191-04:30",
        "2026-11-20T04:03:25.19100Z",
    ] {
        assert_eq!(instant(same), Some(at), "{same}");
    }
    assert_eq!(instant("2026-11-20T04:03:25Z"), Some(at - 191));
    assert_eq!(instant("2026-11-20T04:03:25.5Z"), Some(at - 191 + 500));
    assert_eq!(instant("1970-01-01T00:00:00Z"), Some(0));
    assert_ne!(instant("2026-11-20T04:03:25.192Z"), Some(at));
    for wrong in [
        "",
        "2026-11-20",
        "2026-11-20T04:03:25",
        "2026-11-20T04:03:25.Z",
        "2026-11-20T04:03:25.191",
        "2026-11-20T04:03:25.191+0100",
        "2026-11-20T04:03:25.191+24:00",
        "2026-11-20T04:03:25.191 01:00",
        "2026-11-20T04:03:25.1234567890Z",
        "2026-13-20T04:03:25Z",
        "2026-11-20T04:03:25.191Zjunk",
    ] {
        assert_eq!(instant(wrong), None, "{wrong:?}");
    }
}
