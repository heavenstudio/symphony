//! A task's kind on the command line and the api ([`symphony_board::kind`]): filed with
//! `create --kind` or the api's `create`, given or changed with `kind`, printed by `show`, and
//! refused naming the list for a word that is none of it.

use super::Case;
use serde_json::{Value, json};

#[test]
fn a_task_filed_with_a_kind_shows_it_and_one_given_a_kind_later_shows_that() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    c.ok(
        &orch,
        &["create", "app/charts", "Charts", "--kind", "feature"],
    );
    let shown = c.ok(&orch, &["show", "app/charts"]);
    assert!(shown.contains("\n  kind: feature\n  Stages: "), "{shown}");
    c.ok(&orch, &["create", "inbox/owner-ask", "An ask"]);
    assert!(!c.ok(&orch, &["show", "inbox/owner-ask"]).contains("kind:"));
    assert_eq!(
        c.ok(&orch, &["kind", "inbox/owner-ask", "fix"]),
        "inbox/owner-ask is fix\n"
    );
    let shown = c.ok(&orch, &["show", "inbox/owner-ask"]);
    assert!(shown.contains("\n  kind: fix\n"), "{shown}");
    assert!(
        shown.contains(&format!("{orch} gave it the kind fix")),
        "{shown}"
    );
    c.ok(&orch, &["kind", "inbox/owner-ask", "docs"]);
    let shown = c.ok(&orch, &["show", "inbox/owner-ask"]);
    assert!(
        shown.contains(&format!("{orch} changed its kind fix → docs")),
        "{shown}"
    );
    assert_eq!(
        c.store().get("inbox/owner-ask").unwrap().kind(),
        Some("docs")
    );
}

#[test]
fn an_unknown_kind_is_refused_naming_the_list_and_changes_nothing() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    c.ok(&orch, &["create", "app/plain", "Plain"]);
    let before = c.store().dump().unwrap();
    for args in [
        &["create", "app/odd", "Odd", "--kind", "bug"][..],
        &["kind", "app/plain", "chore"],
    ] {
        let ran = c.run_as(&orch, args, "");
        assert_eq!(ran.code, 2, "{args:?}: {}", ran.err);
        assert!(
            ran.err
                .contains("(choose from 'feature', 'fix', 'red', 'tests', 'docs')"),
            "{args:?}: {}",
            ran.err
        );
        assert_eq!(c.store().dump().unwrap(), before, "{args:?}");
    }
    let ran = c.run_as(&orch, &["kind", "app/gone", "fix"], "");
    assert_eq!(ran.code, 1, "{}", ran.err);
    assert!(ran.err.contains("no task 'app/gone'"), "{}", ran.err);
    c.ok(&orch, &["move", "app/plain", "cancelled"]);
    let ran = c.run_as(&orch, &["kind", "app/plain", "fix"], "");
    assert_eq!(ran.code, 1, "{}", ran.err);
    assert!(
        ran.err
            .contains("app/plain is cancelled: only an open task is given a kind"),
        "{}",
        ran.err
    );
}

#[test]
fn the_apis_create_takes_a_kind_and_refuses_one_of_no_kind() {
    let c = Case::new();
    let actor = c.register("worker");
    let call = |args: Value| {
        let ran = c.run_as(
            "",
            &["api"],
            &json!({"method": "create", "args": args}).to_string(),
        );
        assert_eq!(ran.code, 0, "{}", ran.err);
        serde_json::from_str::<Value>(&ran.out).unwrap()
    };
    let made = call(json!({"slug": "tests/wait", "title": "A wait", "by": actor, "kind": "tests"}));
    assert!(made["ok"].is_object(), "{made}");
    assert_eq!(c.store().get("tests/wait").unwrap().kind(), Some("tests"));
    let plain = call(json!({"slug": "tests/none", "title": "None", "by": actor, "kind": null}));
    assert!(plain["ok"].is_object(), "{plain}");
    assert_eq!(c.store().get("tests/none").unwrap().kind(), None);
    let refused = call(json!({"slug": "tests/odd", "title": "Odd", "by": actor, "kind": "chore"}));
    assert_eq!(
        refused,
        json!({"refused": "a task's kind is one of feature, fix, red, tests or docs, not 'chore'"})
    );
    assert!(c.store().get("tests/odd").is_err());
}
