//! The cap on an orchestrator's open tasks as the command line holds to it: the number is the
//! project's (`[board] orchestrator_slots` in the main checkout's `symphony.toml`), and claims,
//! moves into in_progress, the orchestrator role and `tasks api` are refused past it; the api's
//! `hold`, a landing's move to blocked, keeps the task's slot.

use crate::{Case, git, repository, run, scratch};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// A capped project's file.
fn cap(slots: i64) -> String {
    format!("[project]\nname = \"slots\"\n\n[board]\norchestrator_slots = {slots}\n")
}

/// A board of its own, run from the main checkout of a repository whose `symphony.toml` is
/// `manifest`, committed on main.
fn in_project(name: &str, manifest: &str) -> (Case, PathBuf) {
    let root = repository(&scratch(name));
    std::fs::write(root.join("symphony.toml"), manifest).unwrap();
    git(&root, &["add", "symphony.toml"]);
    git(&root, &["commit", "-q", "-m", "the project"]);
    let mut c = Case::new();
    c.env.cwd = root.clone();
    (c, root)
}

/// An orchestrator orchestrating `work/0` … `work/{open}`, the first `open` claimed by a worker:
/// the orchestrator's and the worker's handles.
fn holding(c: &Case, open: usize) -> (String, String) {
    let orch = c.register("orchestrator");
    let worker = c.register("worker");
    for i in 0..=open {
        let slug = format!("work/{i}");
        c.ok(&orch, &["create", &slug, "Work"]);
        c.ok(&orch, &["role", &slug, "orchestrator"]);
    }
    for i in 0..open {
        c.ok(&worker, &["claim", &format!("work/{i}")]);
    }
    (orch, worker)
}

/// What a claim refused at the cap adds: the way on under the reservation.
const RESERVE_HINT: &str = "; if the task is reserved for you, start work under the reservation now, renew it with `just tasks reserve work/2` within each hour and retry the claim after each renewal";

/// What the orchestrator holding `work/0` and `work/1` in progress, at a cap of two, is said as.
fn holds_two(orch: &str) -> String {
    format!(
        "{orch} holds 2 open tasks, the cap of 2 an orchestrator holds: work/0 (in_progress), work/1 (in_progress); a slot frees when one of them is done or cancelled"
    )
}

#[test]
fn a_claim_past_the_cap_the_project_file_sets_is_refused_naming_the_open_tasks() {
    let (c, _) = in_project("slots-claim", &cap(2));
    let (orch, worker) = holding(&c, 2);
    let ran = c.run_as(&worker, &["claim", "work/2"], "");
    assert_eq!(ran.code, 1, "{}", ran.err);
    assert_eq!(
        ran.err,
        format!(
            "✖ work/2 cannot start: {}{RESERVE_HINT}\n",
            holds_two(&orch)
        )
    );
    let shown = c.ok(&worker, &["show", "work/2"]);
    assert!(shown.contains("[ready"), "{shown}");
    assert!(
        !shown.contains("reserved"),
        "cap refusal reserves nothing: {shown}"
    );
    c.ok(&worker, &["reserve", "work/2"]);
    let ran = c.run_as(&worker, &["claim", "work/2"], "");
    assert_eq!(ran.code, 1, "{}", ran.err);
    assert_eq!(
        ran.err,
        format!(
            "✖ work/2 cannot start: {}{RESERVE_HINT}\n",
            holds_two(&orch)
        )
    );
    let shown = c.ok(&worker, &["show", "work/2"]);
    assert!(
        shown.contains(&format!("reserved {worker} (live)")),
        "{shown}"
    );
    c.ok(&worker, &["move", "work/0", "cancelled"]);
    c.ok(&worker, &["claim", "work/2"]);
    let shown = c.ok(&worker, &["show", "work/2"]);
    assert!(shown.contains("[in_progress"), "{shown}");
    assert!(
        shown.lines().all(|line| !line.starts_with("  reserved ")),
        "no current reservation after claim: {shown}"
    );
}

#[test]
fn a_move_back_from_blocked_and_the_orchestrator_role_on_open_work_are_refused_too() {
    let (c, _) = in_project("slots-move", &cap(2));
    let (orch, worker) = holding(&c, 2);
    c.ok(&worker, &["move", "work/1", "blocked"]);
    c.ok(&worker, &["claim", "work/2"]);
    let ran = c.run_as(&worker, &["move", "work/1", "in_progress"], "");
    assert_eq!(ran.code, 1, "{}", ran.err);
    assert!(
        ran.err.starts_with(&format!(
            "✖ work/1 cannot go from blocked to in_progress: {orch} holds 2 open tasks"
        )),
        "{}",
        ran.err
    );
    let other = c.register("worker");
    c.ok(&other, &["create", "loose", "Loose"]);
    c.ok(&other, &["claim", "loose"]);
    let ran = c.run_as(&orch, &["role", "loose", "orchestrator"], "");
    assert_eq!(ran.code, 1, "{}", ran.err);
    assert!(
        ran.err.starts_with(&format!(
            "✖ {orch} cannot take orchestrator on loose, which is in_progress: {orch} holds 2"
        )),
        "{}",
        ran.err
    );
}

#[test]
fn the_api_holds_the_same_cap() {
    let (c, _) = in_project("slots-api", &cap(2));
    let (orch, worker) = holding(&c, 2);
    let call = json!({"method": "claim", "args": {"slug": "work/2", "handle": worker}});
    let ran = run(&c.env, &["api"], &call.to_string());
    let answer: Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(
        answer,
        json!({"refused": format!("work/2 cannot start: {}{RESERVE_HINT}", holds_two(&orch))})
    );
}

#[test]
fn the_apis_hold_keeps_a_slot_that_a_move_back_takes_again_at_the_cap() {
    let (c, _) = in_project("slots-hold", &cap(2));
    let (orch, worker) = holding(&c, 2);
    let call = json!({"method": "hold", "args": {"slug": "work/1", "handle": worker}});
    let answer: Value =
        serde_json::from_str(&run(&c.env, &["api"], &call.to_string()).out).unwrap();
    assert_eq!(answer["ok"]["status"], "blocked", "{answer}");
    let ran = c.run_as(&worker, &["claim", "work/2"], "");
    assert_eq!(
        ran.err,
        format!(
            "✖ work/2 cannot start: {}{RESERVE_HINT}\n",
            holds_two(&orch).replace("work/1 (in_progress)", "work/1 (blocked, held)")
        )
    );
    c.ok(&worker, &["move", "work/1", "in_progress"]);
    let again = run(&c.env, &["api"], &call.to_string());
    let answer: Value = serde_json::from_str(&again.out).unwrap();
    assert_eq!(answer["ok"]["status"], "blocked", "{answer}");
}

#[test]
fn a_worktree_runs_under_the_main_checkouts_cap_not_its_own() {
    let (mut c, root) = in_project("slots-worktree", &cap(2));
    let tree = root.with_file_name("slots-worktree-branch");
    let _ = std::fs::remove_dir_all(&tree);
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "raise",
            tree.to_str().unwrap(),
        ],
    );
    std::fs::write(tree.join("symphony.toml"), cap(9)).unwrap();
    c.env.cwd = tree;
    let (_, worker) = holding(&c, 2);
    assert_eq!(c.run_as(&worker, &["claim", "work/2"], "").code, 1);
}

#[test]
fn no_project_file_and_no_key_cap_nothing() {
    let c = Case::new();
    let (_, worker) = holding(&c, 9);
    c.ok(&worker, &["claim", "work/9"]);
    let (c, _) = in_project("slots-none", "[board]\nexport = \"tasks.md\"\n");
    let (_, worker) = holding(&c, 9);
    c.ok(&worker, &["claim", "work/9"]);
}

#[test]
fn a_cap_that_cannot_be_read_is_said_and_caps_nothing() {
    let (c, root) = in_project("slots-broken", &cap(0));
    let (_, worker) = holding(&c, 2);
    let ran = c.run_as(&worker, &["claim", "work/2"], "");
    assert_eq!(ran.code, 0, "{}", ran.err);
    let file = Path::new(&root)
        .join("symphony.toml")
        .canonicalize()
        .unwrap();
    assert_eq!(
        ran.err,
        format!(
            "⚠ {}: [board] orchestrator_slots is 0; an orchestrator needs at least one slot; no cap on an orchestrator's open tasks applies\n",
            file.display()
        )
    );
}
