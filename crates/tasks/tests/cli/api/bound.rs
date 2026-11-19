//! `tasks api` since minor 1, on the board of a fixture repository of its own (no `TASKS_SERVER`,
//! so its home's recovery authority answers): a task's number in its shape, the token `bound`
//! reads, the edit `edit_bound` makes by it, and where an integration keeps its state.

use super::answer;
use crate::recovery::Alpha;
use crate::{Case, git};
use serde_json::{Value, json};

fn call(alpha: &Alpha, method: &str, args: Value) -> Value {
    answer(&alpha.env(), json!({"method": method, "args": args}))
}

#[test]
fn a_task_keeps_its_number_through_retitles_and_is_edited_only_by_its_token() {
    let alpha = Alpha::new("api-bound");
    let registered = call(&alpha, "register", json!({"kind": "worker"}));
    let by = registered["ok"]["handle"]
        .as_str()
        .expect("a session")
        .to_string();
    let by = by.as_str();
    let made = call(
        &alpha,
        "create",
        json!({"slug": "a/one", "title": "One", "by": by, "body": "First"}),
    );
    let id = made["ok"]["id"].as_i64().expect("a task's number");
    let other = call(
        &alpha,
        "create",
        json!({"slug": "a/two", "title": "Two", "by": by}),
    );
    assert_eq!(other["ok"]["id"], json!(id + 1));

    // A board never bound reads no token.
    let unbound = call(&alpha, "bound", json!({"slug": "a/one"}));
    assert!(
        unbound["refused"]
            .as_str()
            .is_some_and(|why| why.contains("bound to no recovery generation")),
        "{unbound}"
    );
    assert_eq!(alpha.run(&["recovery", "rebind"]).0, 0);

    let read = call(&alpha, "bound", json!({"slug": "a/one"}));
    let token = read["ok"]["bound"].clone();
    // The version the filing left it at, which each write since moves by one.
    let version = token["version"].as_i64().expect("a version");
    assert_eq!(
        read["ok"],
        json!({
            "bound": {"compat": 1, "generation": token["generation"], "id": id, "version": version},
            "title": "One", "body": "First",
        })
    );
    assert_eq!(
        token["generation"].as_str().map(str::len),
        Some(36),
        "{token}"
    );

    // The old retitle, as before minor 1, keeps the number and moves the version.
    let amended = call(
        &alpha,
        "amend",
        json!({"slug": "a/one", "title": "One again", "by": by}),
    );
    assert_eq!(
        (&amended["ok"]["id"], &amended["ok"]["title"]),
        (&json!(id), &json!("One again"))
    );

    let edit =
        json!({"slug": "a/one", "bound": token, "by": by, "title": "Edited", "body": "Second"});
    let stale = call(&alpha, "edit_bound", edit.clone());
    assert!(
        stale["refused"]
            .as_str()
            .is_some_and(|why| why.contains("the task changed since this was read")),
        "{stale}"
    );
    let now = call(&alpha, "get", json!({"slug": "a/one"}));
    assert_eq!(
        (&now["ok"]["title"], &now["ok"]["body"]),
        (&json!("One again"), &json!("First"))
    );

    let token = call(&alpha, "bound", json!({"slug": "a/one"}))["ok"]["bound"].clone();
    let mut fresh = edit.clone();
    fresh["bound"] = token.clone();
    fresh["links"] = json!(["docs/X.md"]);
    let wrote = call(&alpha, "edit_bound", fresh);
    assert_eq!(
        wrote["ok"],
        json!({
            "bound": {"compat": 1, "generation": token["generation"], "id": id, "version": version + 2},
            "title": "Edited", "body": "Second",
        })
    );
    let now = call(&alpha, "get", json!({"slug": "a/one"}));
    assert_eq!(
        (
            &now["ok"]["id"],
            &now["ok"]["title"],
            &now["ok"]["body"],
            &now["ok"]["links"]
        ),
        (
            &json!(id),
            &json!("Edited"),
            &json!("Second"),
            &json!(["docs/X.md"])
        )
    );
    let listed = call(&alpha, "tasks", json!({}));
    let ids: Vec<&Value> = listed["ok"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| &task["id"])
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(
        ids.contains(&&json!(id)) && ids.contains(&&json!(id + 1)),
        "{ids:?}"
    );

    // A brief alone, with the title left out; and the calls it cannot read.
    let next = wrote["ok"]["bound"].clone();
    let briefed = call(
        &alpha,
        "edit_bound",
        json!({"slug": "a/one", "bound": next, "by": by, "body": "Third"}),
    );
    assert_eq!(
        (&briefed["ok"]["title"], &briefed["ok"]["body"]),
        (&json!("Edited"), &json!("Third"))
    );
    let next = briefed["ok"]["bound"].clone();
    for (args, why) in [
        (
            json!({"slug": "a/one", "bound": next, "by": by, "brief": "x"}),
            "unknown field `brief`",
        ),
        (
            json!({"slug": "a/one", "bound": {"id": id}, "by": by, "body": "x"}),
            "argument bound",
        ),
        (
            json!({"slug": "a/one", "by": by, "body": "x"}),
            "no argument bound",
        ),
        (
            json!({"slug": "a/one", "bound": next, "body": "x"}),
            "no argument by",
        ),
    ] {
        let answer = call(&alpha, "edit_bound", args);
        assert!(
            answer["error"]
                .as_str()
                .is_some_and(|said| said.contains(why)),
            "{why}: {answer}"
        );
    }
    let nothing = call(
        &alpha,
        "edit_bound",
        json!({"slug": "a/one", "bound": next, "by": by}),
    );
    assert!(
        nothing["refused"]
            .as_str()
            .is_some_and(|why| why.contains("an edit gives a title, a brief or links")),
        "{nothing}"
    );

    // After a rebind, as after a copy or a restore, the token read before writes nothing.
    assert_eq!(alpha.run(&["recovery", "rebind"]).0, 0);
    let late = call(
        &alpha,
        "edit_bound",
        json!({"slug": "a/one", "bound": next, "by": by, "body": "Late"}),
    );
    assert!(
        late["refused"]
            .as_str()
            .is_some_and(|why| why.contains("copied or restored since this was read")),
        "{late}"
    );
    assert_eq!(
        call(&alpha, "get", json!({"slug": "a/one"}))["ok"]["body"],
        json!("Third")
    );
}

#[test]
fn a_board_named_by_its_address_refuses_every_bound_call() {
    let c = Case::new();
    let by = c.register("worker");
    let env = c.env;
    answer(
        &env,
        json!({"method": "create", "args": {"slug": "sim/a", "title": "A", "by": by}}),
    );
    let read = answer(&env, json!({"method": "bound", "args": {"slug": "sim/a"}}));
    assert!(
        read["refused"]
            .as_str()
            .is_some_and(|why| why.contains("opened with no recovery authority")),
        "{read}"
    );
}

#[test]
fn an_integrations_state_is_one_folder_from_every_worktree_and_outlives_them() {
    let alpha = Alpha::new("api-integration-state");
    let state = |cwd: &std::path::Path, name: &str| {
        let mut env = alpha.env();
        env.cwd = cwd.to_path_buf();
        answer(
            &env,
            json!({"method": "integration_state", "args": {"name": name}}),
        )
    };
    let common = alpha.root.join(".git").canonicalize().unwrap();
    let expected = json!({"ok": {"path": common.join("symphony/integrations/github")}});
    assert_eq!(state(&alpha.root, "github"), expected);

    let linked = alpha.root.parent().unwrap().join("linked");
    git(
        &alpha.root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "side",
            linked.to_str().unwrap(),
        ],
    );
    let below = linked.join("deep/er");
    std::fs::create_dir_all(&below).unwrap();
    assert_eq!(state(&linked, "github"), expected);
    assert_eq!(state(&below, "github"), expected);

    // The program makes its folder; the worktree that asked goes, and the state stays.
    let path = common.join("symphony/integrations/github");
    assert!(!path.exists(), "the answer makes nothing");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("mappings.json"), "{}").unwrap();
    git(
        &alpha.root,
        &["worktree", "remove", "--force", linked.to_str().unwrap()],
    );
    assert_eq!(state(&alpha.root, "github"), expected);
    assert_eq!(
        std::fs::read_to_string(path.join("mappings.json")).unwrap(),
        "{}"
    );

    // Each integration has its own, and a name is never a path.
    assert_eq!(
        state(&alpha.root, "issue-exchange")["ok"]["path"],
        json!(common.join("symphony/integrations/issue-exchange"))
    );
    for name in ["", "../tasks", "a/b", "GitHub"] {
        let refused = state(&alpha.root, name);
        assert!(
            refused["refused"]
                .as_str()
                .is_some_and(|why| why.contains("an integration's name is")),
            "{name}: {refused}"
        );
    }
    // No board was reached or started for any of it.
    assert!(!common.join("symphony/tasks").exists());

    // Outside a repository there is no project, so no state.
    let outside = std::env::temp_dir();
    let nowhere = state(&outside, "github");
    assert!(nowhere["error"].is_string(), "{nowhere}");
}
