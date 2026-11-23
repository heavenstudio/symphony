//! `tasks api` as a contract: the calls a registered project's own programs make through it today,
//! held. Each is its request as the program sends it, and the fields of its answer the program
//! reads, each with its JSON type. Within the api's major a field is added, never removed, renamed
//! or retyped (`src/cli/api.rs`), so a change that would break one of these calls fails here, at
//! its own task's gate, rather than in the project. A field no caller reads is not held, nor is a
//! call no project's program makes.
//!
//! Not held: `record_waits` under `assessment/2`, which needs the producer's publication evidence
//! in the git common dir of the process's own folder. Its answer is `assessment/1`'s, held below,
//! and the `publication` it was given.

use super::answer;
use crate::Case;
use serde_json::{Value, json};

/// Whether `value` reads as `kind`: `integer` (an `i64`), `number` (an `f64`, as an integer also
/// reads), `string`, `boolean` or `null`; `A|B` either.
fn fits(value: &Value, kind: &str) -> bool {
    kind.split('|').any(|kind| match kind {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "integer" => value.is_i64(),
        "number" => value.is_number(),
        "string" => value.is_string(),
        other => panic!("no JSON type {other}"),
    })
}

/// What of `value`, at `path`, does not read as `shape`. A shape is a type's name for a field
/// ([`fits`]); an object for the fields it holds, each by name, more being free; `[]` for a list
/// whose elements are not read; and `[SHAPE]` for a list of at least one element, each read as
/// `SHAPE`, so a held element shape is always read on something.
fn misfits(value: &Value, shape: &Value, path: &str, out: &mut Vec<String>) {
    match shape {
        Value::String(kind) => {
            if !fits(value, kind) {
                out.push(format!("{path} is {value}, not {kind}"));
            }
        }
        Value::Object(fields) => {
            let Some(object) = value.as_object() else {
                out.push(format!("{path} is {value}, not an object"));
                return;
            };
            for (name, shape) in fields {
                match object.get(name) {
                    Some(field) => misfits(field, shape, &format!("{path}.{name}"), out),
                    None => out.push(format!("{path}.{name} is missing")),
                }
            }
        }
        Value::Array(element) => match (value.as_array(), element.as_slice()) {
            (None, _) => out.push(format!("{path} is {value}, not a list")),
            (Some(_), []) => {}
            (Some(items), [_]) if items.is_empty() => {
                out.push(format!("{path} is empty: nothing held is read"));
            }
            (Some(items), [element]) => {
                for (i, item) in items.iter().enumerate() {
                    misfits(item, element, &format!("{path}[{i}]"), out);
                }
            }
            (Some(_), _) => panic!("{path}: a list's shape is one element's"),
        },
        other => panic!("{path}: no shape {other}"),
    }
}

/// One call of the scenario: a held one reads `reads` of its answer, `Null` when it reads none
/// (served, its answer unread); one that only sets the board up for the next is `None`.
struct Step {
    call: Value,
    reads: Option<Value>,
}

fn held(method: &str, args: Value, reads: Value) -> Step {
    Step {
        call: json!({"method": method, "args": args}),
        reads: Some(reads),
    }
}

fn set(method: &str, args: Value) -> Step {
    Step {
        call: json!({"method": method, "args": args}),
        reads: None,
    }
}

/// A task's fields, as the review tool's landing (`get`, `tasks`, `tasks_for_branch`), its
/// alerts (`get`'s `body`), notice delivery and the art tool (`tasks`'s `title` and `body`) read
/// them, its `parent` of the type given.
fn task(parent: &str) -> Value {
    json!({
        "slug": "string", "title": "string", "body": "string", "status": "string",
        "instruction": "boolean", "parent": parent,
        "roles": [{"handle": "string", "role": "string", "health": "string"}],
    })
}

/// The calls, in an order each is served in, on a board with a live orchestrator and the sessions
/// named.
fn scenario(exe: &str, rev: &str, filer: &str, child: &str, expected: &Value) -> Vec<Step> {
    let none = Value::Null;
    vec![
        held(
            "version",
            json!({}),
            json!({"api": {"major": "integer", "minor": "integer"}, "schema": "integer"}),
        ),
        // Notice delivery's worker, and the review tool's run's own.
        held(
            "register",
            json!({"kind": "worker"}),
            json!({"handle": "string"}),
        ),
        // The review tool's owned worker, its whole receipt read back as proof.
        held(
            "register",
            json!({"kind": "worker", "worktree": "/w/run", "branch": "tools/run", "pid": 4321}),
            json!({
                "handle": "string", "kind": "string", "parent": "null", "worktree": "string",
                "branch": "string", "model": "string", "pid": "integer", "host": "string",
                "affinity": [], "connected": "number", "left_at": "null",
            }),
        ),
        // The review tool's `create`, every field its `NewTask` sends; `red` only when set.
        held(
            "create",
            json!({"slug": "sim/held", "title": "Held", "by": exe, "body": "b", "priority": 1,
                "status": "ready", "parent": null, "links": [], "depends": [], "assignee": null,
                "instruction": false}),
            none.clone(),
        ),
        // A project's program reading a task's execution attempts: none on a task never run.
        held(
            "attempts",
            json!({"slug": "sim/held"}),
            json!({"attempts": []}),
        ),
        held(
            "create",
            json!({"slug": "tests/main-red-held", "title": "Red", "by": filer, "body": "",
                "priority": 0, "status": "ready", "parent": null, "links": [], "depends": [],
                "assignee": null, "instruction": false, "red": "main"}),
            none.clone(),
        ),
        held(
            "unmark_red",
            json!({"slug": "tests/main-red-held", "by": filer}),
            none.clone(),
        ),
        held(
            "mark_red",
            json!({"slug": "tests/main-red-held", "red": "main", "by": filer}),
            none.clone(),
        ),
        held(
            "claim",
            json!({"slug": "sim/held", "handle": exe}),
            none.clone(),
        ),
        held(
            "move",
            json!({"slug": "sim/held", "status": "review", "handle": exe, "from": ["in_progress"]}),
            none.clone(),
        ),
        set(
            "take_role",
            json!({"slug": "sim/held", "handle": rev, "role": "reviewer"}),
        ),
        held(
            "verdict",
            json!({"slug": "sim/held", "handle": rev, "verdict": "changes", "text": "fix it"}),
            none.clone(),
        ),
        held(
            "ever_executed",
            json!({"slug": "sim/held"}),
            json!(["string"]),
        ),
        held(
            "say",
            json!({"slug": "sim/held", "sender": exe, "text": "a post", "to": "@orchestrator"}),
            none.clone(),
        ),
        held(
            "say",
            json!({"slug": "sim/held", "sender": exe, "text": "once", "to": "@executor",
                "key": "held:1", "adopt": true}),
            none.clone(),
        ),
        held(
            "notify",
            json!({"slug": "sim/held", "actor": exe, "handle": exe, "text": "room freed"}),
            none.clone(),
        ),
        held(
            "thread",
            json!({"slug": "sim/held"}),
            json!([{"id": "integer", "slug": "string", "sender": "string",
                "recipient": "string|null", "kind": "string", "text": "string", "ts": "number"}]),
        ),
        held(
            "move",
            json!({"slug": "sim/held", "status": "review", "handle": exe}),
            none.clone(),
        ),
        held(
            "hold",
            json!({"slug": "sim/held", "handle": exe}),
            none.clone(),
        ),
        held("get", json!({"slug": "sim/child"}), task("string")),
        held("tasks", json!({}), json!([task("string|null")])),
        held(
            "tasks",
            json!({"status": null}),
            json!([task("string|null")]),
        ),
        held("tasks", json!({"status": "blocked"}), json!([task("null")])),
        held(
            "tasks_for_branch",
            json!({"branch": "sim/held"}),
            json!([task("null")]),
        ),
        held("ancestors", json!({"slug": "sim/child"}), json!(["string"])),
        held(
            "sessions",
            json!({}),
            json!([{"handle": "string", "worktree": "string", "branch": "string",
                "connected": "number"}]),
        ),
        set("bind_agent", json!({"agent": "agent-held", "handle": exe})),
        held(
            "handle_for_agent",
            json!({"agent": "agent-held"}),
            json!("string"),
        ),
        set(
            "record_launch",
            json!({"slug": "sim/held", "handle": exe, "role": "executor",
                "worktree": "/w/held", "branch": "sim/held"}),
        ),
        held(
            "sessions_of",
            json!({"handle": exe}),
            json!([{"slug": "string", "worktree": "string", "started": "number"}]),
        ),
        held(
            "assessment_state",
            json!({"slug": "sim/parent"}),
            json!({"protocol": "string", "slug": "string", "status": "string",
                "version": "integer"}),
        ),
        held(
            "record_waits",
            json!({"protocol": "assessment/1", "slug": "sim/parent", "record": "aaa: changes",
                "assessment": "held", "actor": exe, "expected": expected}),
            json!({"protocol": "string", "slug": "string", "id": "string", "record": "string",
                "sid": "integer", "at": "number", "current": "boolean"}),
        ),
        held(
            "landed",
            json!({"slug": "sim/held", "handle": exe, "text": "landed sim/held on main at 0123456789ab",
                "requested": true, "from": ["review", "blocked"]}),
            json!({"from": "string"}),
        ),
        held("leave", json!({"handle": child}), none.clone()),
        // Notice delivery reads `left_at` of a recipient, the review tool the whole receipt.
        held(
            "session",
            json!({"handle": child}),
            json!({
                "handle": "string", "kind": "string", "parent": "string", "worktree": "string",
                "branch": "string", "model": "string", "pid": "integer", "host": "string",
                "affinity": ["string"], "connected": "number", "left_at": "number",
            }),
        ),
    ]
}

#[test]
fn the_calls_a_projects_programs_make_answer_the_fields_they_read() {
    let mut c = Case::new();
    // A project's programs call from its checkout, whose project scopes its attempts.
    let root = crate::repository(&crate::scratch("contract-project"));
    std::fs::write(root.join("symphony.toml"), "[project]\nid = \"sim\"\n").unwrap();
    c.env.cwd = root;
    // An orchestrator that stays, given every task filed: each task has a role to read.
    let orch = c.register_settled("orchestrator");
    c.ok(&orch, &["wait", "--timeout", "0"]);
    let (exe, rev) = (c.register("worker"), c.register("reviewer"));
    // Each call served: its result.
    let set_up = |calls: Value| -> Vec<Value> {
        let said = answer(&c.env, calls);
        let said = said.as_array().unwrap();
        said.iter()
            .map(|one| one.get("ok").cloned())
            .collect::<Option<_>>()
            .unwrap_or_else(|| panic!("setting up: {said:?}"))
    };
    let sessions = set_up(json!([
        {"method": "register", "args": {"kind": "filer"}},
        {"method": "register", "args": {"kind": "worker", "parent": orch, "pid": 4242,
            "affinity": ["sim"], "worktree": "/w/child", "branch": "sim/child"}},
    ]));
    let (filer, child) = (
        sessions[0]["handle"].as_str().unwrap(),
        sessions[1]["handle"].as_str().unwrap(),
    );
    // A subtask is given no orchestrator of its own: its executor is the role read on it.
    let tasks = set_up(json!([
        {"method": "create", "args": {"slug": "sim/parent", "title": "Parent", "by": exe}},
        {"method": "create", "args": {"slug": "sim/child", "title": "Child", "by": exe,
            "parent": "sim/parent"}},
        {"method": "claim", "args": {"slug": "sim/child", "handle": child}},
        {"method": "assessment_state", "args": {"slug": "sim/parent"}},
    ]));
    let mut expected = tasks[3].clone();
    expected.as_object_mut().unwrap().remove("protocol");
    let mut wrong = Vec::new();
    for Step { call, reads } in scenario(&exe, &rev, filer, child, &expected) {
        let said = answer(&c.env, call.clone());
        let Some(result) = said.get("ok") else {
            wrong.push(format!("{call} was not served: {said}"));
            continue;
        };
        if let Some(reads) = reads.filter(|reads| !reads.is_null()) {
            misfits(result, &reads, call["method"].as_str().unwrap(), &mut wrong);
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// `version` is the build's, so it answers without the board, and it takes nothing.
#[test]
fn version_answers_this_builds_api_and_schema_without_the_board() {
    let mut env = crate::env();
    env.vars
        .insert("TASKS_SERVER".into(), crate::UNREACHED_SERVER.into());
    assert_eq!(
        answer(&env, json!({"method": "version"})),
        json!({"ok": {"api": {"major": 2, "minor": 6},
            "schema": symphony_board::schema::VERSION}})
    );
    let said = answer(&env, json!({"method": "version", "args": {"major": 2}}));
    assert!(
        said["error"]
            .as_str()
            .is_some_and(|why| why.contains("major")),
        "{said}"
    );
}

/// The reader tells: a field gone, a type changed, a list with nothing to read, a shape not an
/// object, each named by where it is.
#[test]
fn a_field_gone_or_retyped_is_named() {
    let shape = json!({"slug": "string", "parent": "string|null",
        "roles": [{"handle": "string"}], "links": []});
    let mut out = Vec::new();
    let good = json!({"slug": "a", "parent": null, "roles": [{"handle": "h", "since": 1.0}],
        "links": [], "more": 1});
    misfits(&good, &shape, "get", &mut out);
    assert_eq!(out, Vec::<String>::new());
    let bad = json!({"parent": 3, "roles": [{"handle": "h"}, {}], "links": {}});
    misfits(&bad, &shape, "get", &mut out);
    // A map's order is serde_json's, which a workspace build may change.
    out.sort();
    assert_eq!(
        out,
        [
            "get.links is {}, not a list",
            "get.parent is 3, not string|null",
            "get.roles[1].handle is missing",
            "get.slug is missing",
        ]
    );
    out.clear();
    misfits(
        &json!({"roles": []}),
        &json!({"roles": [{}]}),
        "tasks[0]",
        &mut out,
    );
    misfits(&json!([1]), &json!({}), "say", &mut out);
    misfits(&json!(2.5), &json!("integer"), "sid", &mut out);
    misfits(&json!(2), &json!("number"), "at", &mut out);
    assert_eq!(
        out,
        [
            "tasks[0].roles is empty: nothing held is read",
            "say is [1], not an object",
            "sid is 2.5, not integer",
        ]
    );
}
