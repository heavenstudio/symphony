//! Board command exit codes, body-file input and subtask operations.

use crate::{Case, scratch};
use serde_json::Value;
use std::fs;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn slugs(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|t| t["slug"].as_str().unwrap())
        .collect()
}

#[test]
fn register_prints_only_the_handle() {
    let out = Case::new().ok("", &["register", "--kind", "worker"]);
    let handle = out.strip_suffix('\n').unwrap();
    let words: Vec<&str> = handle.split('-').collect();
    let lower = |w: &&str| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase());
    let number = |w: &&str| !w.is_empty() && w.chars().all(|c| c.is_ascii_digit());
    let shaped = match words.as_slice() {
        [a, b] => lower(a) && lower(b),
        [a, b, n] => lower(a) && lower(b) && number(n),
        _ => false,
    };
    assert!(shaped, "{out:?}");
}

#[test]
fn a_command_without_a_session_says_how_to_get_one() {
    let ran = Case::new().run_as("", &["claim", "sim/jump"], "");
    assert_eq!(ran.code, 2);
    assert!(ran.err.contains("TASKS_SESSION"), "{}", ran.err);
}

#[test]
fn the_as_flag_overrides_the_environment() {
    let c = Case::new();
    let (a, b) = (c.register("worker"), c.register("worker"));
    c.ok(&a, &["create", "sim/jump", "Jump"]);
    c.ok(&a, &["claim", "sim/jump", "--as", &b]);
    assert_eq!(
        c.store().get("sim/jump").unwrap().holders("executor"),
        [b.as_str()]
    );
}

#[test]
fn create_claim_move_review_approve() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    let exe = c.register_with("worker", &["--parent", &orch]);
    let rev = c.register_with("reviewer", &["--parent", &orch]);
    c.ok(
        &orch,
        &[
            "create",
            "sim/jump",
            "Jump departure",
            "--priority",
            "1",
            "--link",
            "docs/SPEC.md#Jump",
            "--body",
            "Make the jump leave cleanly.",
        ],
    );
    c.ok(&orch, &["role", "sim/jump", "orchestrator"]);
    c.ok(&exe, &["claim", "sim/jump"]);
    c.ok(&exe, &["move", "sim/jump", "review"]);
    c.ok(&rev, &["role", "sim/jump", "reviewer"]);
    c.ok(
        &rev,
        &["verdict", "sim/jump", "approve", "hash chain holds"],
    );
    let task = c.store().get("sim/jump").unwrap();
    assert_eq!(
        (task.status.as_str(), task.priority, task.links),
        ("done", 1, vec!["docs/SPEC.md#Jump".to_string()])
    );
}

#[test]
fn a_body_file_is_read() {
    let c = Case::new();
    let a = c.register("worker");
    let body = scratch("body-file").join("brief.md");
    fs::write(&body, "# Brief\n\nDo it.\n").unwrap();
    c.ok(
        &a,
        &[
            "create",
            "sim/jump",
            "Jump",
            "--body-file",
            body.to_str().unwrap(),
        ],
    );
    assert_eq!(
        c.store().get("sim/jump").unwrap().body,
        "# Brief\n\nDo it.\n"
    );
}

#[test]
fn a_refusal_is_exit_1_with_the_reason() {
    let c = Case::new();
    let (a, b) = (c.register("worker"), c.register("worker"));
    c.ok(&a, &["create", "sim/jump", "Jump"]);
    c.ok(&a, &["claim", "sim/jump"]);
    let ran = c.run_as(&b, &["claim", "sim/jump"], "");
    assert_eq!(ran.code, 1);
    assert!(ran.err.contains(&a), "{}", ran.err);
}

#[test]
fn owner_actors_are_refused_by_flag_and_environment_without_changing_the_board() {
    let c = Case::new();
    let agent = c.register("worker");
    c.ok(&agent, &["create", "sim/jump", "Jump"]);
    c.ok(&agent, &["claim", "sim/jump"]);
    let before = c.store().dump().unwrap();
    for command in [
        vec!["priority", "sim/jump", "0"],
        vec!["move", "sim/jump", "cancelled"],
        vec!["create", "sim/forged", "Forged"],
        vec!["say", "sim/jump", "Forged owner message"],
        vec!["claim", "sim/jump"],
    ] {
        for (who, prefix, suffix) in [
            (agent.as_str(), vec!["--as", "owner"], vec![]),
            (agent.as_str(), vec![], vec!["--as=owner"]),
            ("owner", vec![], vec![]),
            ("owner", vec![], vec!["--as="]),
        ] {
            let mut args = prefix;
            args.extend_from_slice(&command);
            args.extend_from_slice(&suffix);
            let ran = c.run_as(who, &args, "");
            assert_eq!(ran.code, 1, "{args:?} as {who}: {}", ran.err);
            assert!(ran.out.is_empty(), "{args:?}: {}", ran.out);
            assert!(
                ran.err.contains("the owner acts from the Symphony"),
                "{args:?}: {}",
                ran.err
            );
            assert_eq!(c.store().dump().unwrap(), before, "{args:?} as {who}");
        }
    }
    // The effective actor is the flag's agent, even when the environment says owner.
    c.ok("owner", &["priority", "sim/jump", "4", "--as", &agent]);
    assert_eq!(c.store().get("sim/jump").unwrap().priority, 4);
    assert_eq!(
        c.store().thread("sim/jump").unwrap().last().unwrap().sender,
        agent
    );
}

#[test]
fn list_json_and_its_status_filter() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(&a, &["create", "sim/a", "A"]);
    c.ok(&a, &["create", "sim/b", "B", "--backlog"]);
    let data = json(&c.ok("", &["list", "--json", "--status", "ready"]));
    assert_eq!(slugs(&data["tasks"]), ["sim/a"]);
}

#[test]
fn create_can_start_at_a_stage() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(
        &a,
        &["create", "docs/hulls", "Hull briefs", "--stage", "concept"],
    );
    assert_eq!(c.store().get("docs/hulls").unwrap().status, "concept");
}

/// A session and the data task `content/roster-stretch` it made: the session's handle.
fn data_task(c: &Case) -> String {
    let a = c.register("worker");
    c.ok(&a, &["create", "content/roster-stretch", "Roster"]);
    a
}

#[test]
fn create_refuses_a_brief_importing_from_the_worktree_of_a_dependency_not_done() {
    let c = Case::new();
    let a = data_task(&c);
    let ran = c.run_as(
        &a,
        &[
            "create",
            "art/hulls",
            "Hulls",
            "--depends",
            "content/roster-stretch",
            "--body",
            "Import content/x.json from .claude/worktrees/content-roster-stretch/content/x.json.",
        ],
        "",
    );
    assert_eq!(ran.code, 1);
    assert!(
        ran.err.contains("workflow/shared-data-lands-first"),
        "{}",
        ran.err
    );
    assert!(
        ran.err.contains(".claude/worktrees/content-roster-stretch"),
        "{}",
        ran.err
    );
    let named: String = ran
        .err
        .split(".claude/worktrees/")
        .nth(1)
        .unwrap()
        .chars()
        .take(25)
        .collect();
    assert!(
        !named.contains("stretch."),
        "the worktree is named with its sentence's stop: {}",
        ran.err
    );
    assert!(c.store().get("art/hulls").is_err());
}

#[test]
fn create_takes_a_review_or_a_cwd_brief_naming_a_worktree_it_does_not_depend_on() {
    let c = Case::new();
    let a = data_task(&c);
    c.ok(
        &a,
        &[
            "create",
            "review/x",
            "Review",
            "--body",
            "Read the diff in /r/.claude/worktrees/content-roster-stretch.",
        ],
    );
    c.ok(
        &a,
        &[
            "create",
            "art/other",
            "Other",
            "--depends",
            "content/roster-stretch",
            "--body",
            "Work in .claude/worktrees/art-other, on main's data.",
        ],
    );
}

#[test]
fn create_takes_artifact_overlays_under_a_dependencys_worktree() {
    let c = Case::new();
    let a = data_task(&c);
    c.ok(
        &a,
        &[
            "create",
            "art/hulls",
            "Hulls",
            "--depends",
            "content/roster-stretch",
            "--body",
            "See .claude/worktrees/content-roster-stretch/artifacts/hull.png",
        ],
    );
}

#[test]
fn create_takes_the_worktree_of_a_dependency_that_is_done() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(&a, &["create", "content/parent", "Parent"]);
    c.ok(
        &a,
        &[
            "create",
            "content/roster-stretch",
            "Roster",
            "--parent",
            "content/parent",
            "--instruction",
        ],
    );
    let store = c.store();
    store.claim("content/roster-stretch", &a).unwrap();
    store.done("content/roster-stretch", &a, "landed").unwrap();
    c.ok(
        &a,
        &[
            "create",
            "art/hulls",
            "Hulls",
            "--depends",
            "content/roster-stretch",
            "--body",
            "Old notes in .claude/worktrees/content-roster-stretch/notes.md",
        ],
    );
}

#[test]
fn list_mine_shows_only_the_tasks_it_holds() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(&a, &["create", "sim/a", "A"]);
    c.ok(&a, &["create", "sim/b", "B"]);
    c.ok(&a, &["claim", "sim/b"]);
    let out = c.ok(&a, &["list", "--mine"]);
    assert!(out.contains("sim/b") && !out.contains("sim/a"), "{out}");
}

#[test]
fn next_prints_the_best_slug_or_nothing() {
    let c = Case::new();
    let a = c.register("worker");
    assert_eq!(c.ok(&a, &["next"]).trim(), "");
    c.ok(&a, &["create", "sim/a", "A"]);
    assert!(c.ok(&a, &["next"]).contains("sim/a"));
}

#[test]
fn show_prints_the_roles_and_the_thread() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(&a, &["create", "sim/a", "Alpha"]);
    c.ok(&a, &["claim", "sim/a"]);
    c.ok(&a, &["say", "sim/a", "halfway there", "--to", "owner"]);
    let out = c.ok("", &["show", "sim/a"]);
    for said in ["Alpha", &format!("executor {a}"), "halfway there"] {
        assert!(out.contains(said), "{said:?} not in {out}");
    }
}

#[test]
fn reserve_release_and_show_the_dependencies() {
    let c = Case::new();
    let (a, b) = (c.register("worker"), c.register("worker"));
    c.ok(&a, &["create", "sim/base", "Base"]);
    c.ok(
        &a,
        &[
            "create",
            "sim/top",
            "Top",
            "--depends",
            "sim/base",
            "--parent",
            "sim/base",
        ],
    );
    c.ok(&a, &["reserve", "sim/top"]);
    assert_eq!(c.run_as(&b, &["reserve", "sim/top"], "").code, 1);
    c.ok(&a, &["release", "sim/top", "--role", "reserved"]);
    c.ok(&b, &["reserve", "sim/top"]);
    let out = c.ok("", &["show", "sim/top"]);
    assert!(out.contains("depends on: sim/base"), "{out}");
    assert!(out.contains("parent: sim/base"), "{out}");
    let data = json(&c.ok("", &["show", "sim/top", "--json"]));
    assert_eq!(data["task"]["depends"], serde_json::json!(["sim/base"]));
}

#[test]
fn who_json_and_all() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(&a, &["leave"]);
    assert!(!c.ok("", &["who"]).contains(&a));
    assert!(c.ok("", &["who", "--all"]).contains(&a));
    assert_eq!(
        json(&c.ok("", &["who", "--json"]))["sessions"],
        serde_json::json!([])
    );
}

#[test]
fn who_lists_the_sessions_as_a_tree_with_their_health() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    let child = c.register_with("worker", &["--parent", &orch]);
    let out = c.ok("", &["who"]);
    assert!(out.contains(&orch), "{out}");
    assert!(out.contains(&format!("  {child}")), "{out}");
    assert!(out.contains("live"), "{out}");
}

#[test]
fn inbox_prints_and_acknowledges() {
    let c = Case::new();
    let (a, b) = (c.register("worker"), c.register("worker"));
    c.ok(&a, &["create", "sim/a", "A"]);
    c.ok(&a, &["say", "sim/a", "ping", "--to", &b]);
    assert!(c.ok(&b, &["inbox"]).contains("ping"));
    assert_eq!(c.ok(&b, &["inbox"]).trim(), "");
}

#[test]
fn leave_releases_and_ends_the_session() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(&a, &["create", "sim/a", "A"]);
    c.ok(&a, &["claim", "sim/a"]);
    c.ok(&a, &["leave"]);
    assert_eq!(c.store().get("sim/a").unwrap().status, "ready");
    assert_eq!(c.run_as(&a, &["claim", "sim/a"], "").code, 1);
}

#[test]
fn export_writes_a_markdown_snapshot() {
    let c = Case::new();
    let a = c.register("worker");
    c.ok(
        &a,
        &["create", "sim/a", "Alpha", "--link", "docs/SPEC.md#Jump"],
    );
    c.ok(&a, &["claim", "sim/a"]);
    let out = scratch("export-snapshot").join("tasks.md");
    c.ok("", &["export", "--out", out.to_str().unwrap()]);
    let text = fs::read_to_string(&out).unwrap();
    for said in ["## in_progress", "**sim/a**", "[[SPEC#Jump]]", &a] {
        assert!(text.contains(said), "{said:?} not in {text}");
    }
}

#[test]
fn a_body_file_of_a_dash_is_read_from_stdin() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    let body = "line one\n\nline two with `code` and 'quotes' — and more\n";
    let ran = c.run_as(
        &orch,
        &["create", "a/long", "Long", "--body-file", "-"],
        body,
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(c.store().get("a/long").unwrap().body, body);
}

#[test]
fn a_body_file_that_cannot_be_read_is_a_refusal_naming_it() {
    let c = Case::new();
    let orch = c.register("orchestrator");
    let missing = scratch("body-missing").join("missing.md");
    let missing = missing.to_str().unwrap();
    let ran = c.run_as(
        &orch,
        &["create", "a/long", "Long", "--body-file", missing],
        "",
    );
    assert_eq!(ran.code, 1);
    assert!(
        ran.err.starts_with("✖ ") && ran.err.contains(missing),
        "{}",
        ran.err
    );
}

/// `a` over `a/b` over `a/b/c`, and `other`, made by an orchestrator; then a worker: both handles.
fn chain(c: &Case) -> (String, String) {
    let orch = c.register("orchestrator");
    c.ok(&orch, &["create", "a", "Top"]);
    c.ok(&orch, &["create", "a/b", "Middle", "--parent", "a"]);
    c.ok(&orch, &["create", "a/b/c", "Bottom", "--parent", "a/b"]);
    c.ok(&orch, &["create", "other", "Other"]);
    (orch, c.register("worker"))
}

#[test]
fn list_indents_children_under_their_parent_at_any_depth() {
    let c = Case::new();
    chain(&c);
    let out = c.ok("", &["list"]);
    let named = ["a", "a/b", "a/b/c", "other"];
    let rows: Vec<(&str, usize)> = out
        .lines()
        .map(|row| {
            let slug = row.split_whitespace().find(|w| named.contains(w)).unwrap();
            (slug, row.len() - row.trim_start_matches(' ').len())
        })
        .collect();
    let at = |slug: &str| rows.iter().position(|(s, _)| *s == slug).unwrap();
    let indent = |slug: &str| rows[at(slug)].1;
    assert_eq!(indent("a"), indent("other"), "{out}");
    assert!(indent("a/b") > indent("a"), "{out}");
    assert!(indent("a/b/c") > indent("a/b"), "{out}");
    assert_eq!(at("a/b/c"), at("a/b") + 1, "{out}");
    assert_eq!(at("a/b"), at("a") + 1, "{out}");
}

#[test]
fn list_with_a_filter_keeps_a_child_whose_parent_it_hides_at_the_top() {
    let c = Case::new();
    let (_, agent) = chain(&c);
    c.ok(&agent, &["claim", "a/b/c"]);
    let out = c.ok("", &["list", "--status", "in_progress"]);
    let rows: Vec<&str> = out.lines().collect();
    assert_eq!(rows.len(), 1, "{out}");
    assert!(rows[0].starts_with("p2"), "{out}");
}

#[test]
fn show_lists_the_children_with_their_status_and_holder() {
    let c = Case::new();
    let (_, agent) = chain(&c);
    c.ok(&agent, &["claim", "a/b"]);
    let out = c.ok("", &["show", "a"]);
    let below = out.split("subtasks:\n").nth(1).unwrap_or_default();
    let first: Vec<&str> = below
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    assert_eq!(first, ["a/b", "in_progress", "executor", &agent], "{out}");
    assert!(
        !out.contains("a/b/c"),
        "one level; the child's own show lists its own: {out}"
    );
    assert!(c.ok("", &["show", "a/b"]).contains("a/b/c"));
    let data = json(&c.ok("", &["show", "a", "--json"]));
    assert_eq!(slugs(&data["children"]), ["a/b"]);
}

#[test]
fn create_for_a_handle_reserves_it_and_next_offers_it_to_that_handle_alone() {
    let c = Case::new();
    let (orch, agent) = chain(&c);
    let other = c.register("worker");
    c.ok(
        &orch,
        &[
            "create",
            "a/review",
            "Review",
            "--parent",
            "a",
            "--for",
            &agent,
            "--instruction",
        ],
    );
    assert!(!c.ok(&other, &["next"]).contains("a/review"));
    assert!(c.ok(&agent, &["next"]).starts_with("a/review"));
    assert_eq!(c.run_as(&other, &["claim", "a/review"], "").code, 1);
    c.ok(&agent, &["claim", "a/review"]);
}

#[test]
fn done_closes_an_instruction_task_and_refuses_one_that_lands() {
    let c = Case::new();
    let (orch, agent) = chain(&c);
    c.ok(
        &orch,
        &["create", "a/m", "Measure", "--parent", "a", "--instruction"],
    );
    c.ok(&agent, &["claim", "a/m"]);
    assert!(
        c.ok(&agent, &["done", "a/m", "median 4 ms"])
            .contains("done")
    );
    assert_eq!(c.store().get("a/m").unwrap().status, "done");
    c.ok(&agent, &["claim", "a/b"]);
    let ran = c.run_as(&agent, &["done", "a/b", "finished"], "");
    assert_eq!(ran.code, 1);
    assert!(ran.err.contains("instruction"), "{}", ran.err);
}

#[test]
fn parent_moves_a_task_and_refuses_a_cycle() {
    let c = Case::new();
    let (orch, _) = chain(&c);
    c.ok(&orch, &["parent", "other", "a/b/c"]);
    assert_eq!(
        c.store().get("other").unwrap().parent.as_deref(),
        Some("a/b/c")
    );
    let ran = c.run_as(&orch, &["parent", "a", "other"], "");
    assert_eq!(ran.code, 1);
    assert!(ran.err.contains("ancestor"), "{}", ran.err);
    c.ok(&orch, &["parent", "other", "--none"]);
    assert_eq!(c.store().get("other").unwrap().parent, None);
}
