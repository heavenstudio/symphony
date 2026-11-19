//! `tasks api`'s `attempts`: a task's execution attempts, read as the board's own service under
//! the board scope of the project the call runs in, its past attempts and what was bound to them
//! included; never another project's, and never a task the board does not hold.

use super::answer;
use crate::{Case, repository, scratch};
use serde_json::json;
use std::time::Instant;
use symphony_board::execution::{Act, Adapter, Admission, Coordinator, Grant, board_scope};
use symphony_plugin::execution::{
    Authorization, Call, Capabilities, ContentRef, Input, Operation, Outcome, Principal,
    PrincipalKind, Profile, Scope, Unknown,
};

/// A provider no admission or cancel reaches.
struct Unreached;

impl Adapter for Unreached {
    fn execute(&self, _: &Call, _: &Authorization, _: Instant) -> Outcome {
        Outcome::Unknown(Unknown::Other("nothing here is sent".into()))
    }
}

fn owner(scope: &Scope) -> Grant {
    let owner = Principal::new(PrincipalKind::Human, "local".into(), "owner".into());
    Grant::from_host(owner, scope.clone(), &[Act::Start, Act::Cancel], "policy-1")
}

fn admit(store: &symphony_board::Store, scope: &Scope, task: &str) -> String {
    let mut capabilities = Capabilities::default();
    capabilities.operations = vec![Operation::Start, Operation::Cancel];
    let profile = Profile::new(scope.clone(), "cloud".into(), capabilities);
    let worker = Principal::new(PrincipalKind::Service, "board".into(), "worker".into());
    let worker = Grant::from_host(worker, scope.clone(), &[Act::Work], "policy-1");
    let c = Coordinator::new(store, profile, &Unreached, worker).unwrap();
    let input = Input::new(
        "r1".into(),
        "d1".into(),
        "commit".into(),
        ContentRef::new("manifest".into(), "digest".into()),
    );
    let asked = Admission::new(task, "executor/amber", "executor", input).handle("amber");
    let attempt = c.admit(&owner(scope), asked).unwrap();
    c.cancel(&owner(scope), &attempt.id).unwrap();
    attempt.id
}

/// A case whose calls run in a checkout of the project `proj`.
fn in_project(name: &str) -> Case {
    let mut c = Case::new();
    let root = repository(&scratch(name));
    std::fs::write(root.join("symphony.toml"), "[project]\nid = \"proj\"\n").unwrap();
    c.env.cwd = root;
    c
}

#[test]
fn a_tasks_attempts_are_its_history_under_the_projects_board_scope() {
    let c = in_project("attempts-history");
    let actor = c.register("worker");
    let store = c.store();
    for slug in ["sim/run", "sim/idle"] {
        let made = answer(
            &c.env,
            json!({"method": "create", "args": {"slug": slug, "title": "T", "by": actor}}),
        );
        assert!(made.get("ok").is_some(), "{made}");
    }
    let id = admit(&store, &board_scope("proj", &store), "sim/run");
    let said = answer(
        &c.env,
        json!({"method": "attempts", "args": {"slug": "sim/run"}}),
    );
    let attempts = said["ok"]["attempts"]
        .as_array()
        .unwrap_or_else(|| panic!("{said}"));
    assert_eq!(attempts.len(), 1, "{said}");
    let attempt = &attempts[0];
    assert_eq!(attempt["id"], json!(id));
    assert_eq!(attempt["handle"], json!("amber"));
    assert_eq!(attempt["lane"], json!("executor/amber"));
    assert_eq!(
        (
            &attempt["dispatch"],
            &attempt["ending"],
            &attempt["cancellation"]
        ),
        (&json!("terminal"), &json!("cancelled"), &json!("confirmed"))
    );
    assert_eq!(attempt["initiator"], json!("human local/owner"));
    assert_eq!(attempt["current"], json!(true));
    assert_eq!(
        attempt["controls"],
        json!({"cancel": false, "stop": [], "resume": [], "view": false, "unserved": null}),
        "tasks api offers no control: the host that holds the provider does"
    );
    assert_eq!(
        answer(
            &c.env,
            json!({"method": "attempts", "args": {"slug": "sim/idle"}})
        ),
        json!({"ok": {"attempts": []}})
    );
    // A task the board does not hold, and a slug not given, are refused before any attempt is read.
    let missing = answer(
        &c.env,
        json!({"method": "attempts", "args": {"slug": "sim/none"}}),
    );
    assert!(missing.get("refused").is_some(), "{missing}");
    let unnamed = answer(&c.env, json!({"method": "attempts", "args": {}}));
    assert!(
        unnamed["error"]
            .as_str()
            .is_some_and(|why| why.contains("slug")),
        "{unnamed}"
    );
}

#[test]
fn another_projects_attempt_and_a_call_from_no_project_are_refused() {
    let c = in_project("attempts-another");
    let actor = c.register("worker");
    let store = c.store();
    let made = answer(
        &c.env,
        json!({"method": "create", "args": {"slug": "sim/run", "title": "T", "by": actor}}),
    );
    assert!(made.get("ok").is_some(), "{made}");
    admit(&store, &board_scope("another-project", &store), "sim/run");
    let said = answer(
        &c.env,
        json!({"method": "attempts", "args": {"slug": "sim/run"}}),
    );
    assert!(
        said["refused"]
            .as_str()
            .is_some_and(|why| why.contains("another project's or board's attempt")),
        "{said}"
    );
    let mut outside = c.env.clone();
    outside.cwd = scratch("attempts-outside");
    let said = answer(
        &outside,
        json!({"method": "attempts", "args": {"slug": "sim/run"}}),
    );
    assert!(said.get("refused").is_some(), "{said}");
}
