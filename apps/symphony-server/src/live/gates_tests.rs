//! `/api/task-gates` over a scratch board and a temporary repository: a project's plugin answers
//! (the shelf's, as its checkout's `gates.json` says), the Symphony's own gates where none does or
//! beside it, a failing or slow plugin as an unknown gate, and a plugin asked once a tip.

use super::Hub;
use super::read_paths_tests::{observe, served, store_on};
use crate::shelf;
use crate::testing::{
    Tree,
    board::{Fixture, NOW, Seed},
    git, repository,
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, Instant};
use symphony_api::{GateState, TaskGates};
use symphony_board::rows::MessageRow;

/// `seed` with a verdict `text` by `by` on `slug`'s thread, the `id`th line, at `NOW + id`.
fn verdict(mut seed: Seed, id: i64, slug: &str, by: &str, text: &str) -> Seed {
    seed.dump.messages.push(MessageRow {
        id,
        slug: slug.into(),
        sender: by.into(),
        recipient: None,
        kind: "verdict".into(),
        text: text.into(),
        ts: NOW + f64::from(i32::try_from(id).unwrap()),
    });
    seed
}

/// A hub over `root` and `f`'s board, its plugin answers waited for `wait`.
fn hub(root: &Path, f: &Fixture, wait: Duration) -> Hub {
    let mut hub = Hub::new(root, &root.join("build/client"));
    let board = f.other_client();
    hub.connect_reader = Box::new(move |_| Ok(store_on(&board)));
    hub.gates = symphony::gates::Kept::new(wait, Duration::from_secs(60));
    hub.refresh_page("", root).unwrap();
    observe(&hub, &f.store, &[]);
    hub
}

/// The answer's gates as (name, state, detail).
fn gates(answer: &Value) -> Vec<(String, GateState, String)> {
    let answer: TaskGates = serde_json::from_value(answer.clone()).unwrap();
    answer
        .gates
        .into_iter()
        .map(|gate| (gate.name, gate.state, gate.detail))
        .collect()
}

fn calls(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join(shelf::GATES_CALLS))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// A repository holding the shelf's project file, `branches` cut from main.
fn shelf_project(tree: &Tree, branches: &[&str]) -> std::path::PathBuf {
    shelf::install().unwrap();
    let root = repository(tree);
    std::fs::write(root.join(symphony::project::FILE), shelf::MANIFEST).unwrap();
    for branch in branches {
        git(&root, &["branch", branch]);
    }
    root
}

#[test]
fn a_plugins_gates_come_through_the_api_what_the_task_waits_on_first() {
    let tree = Tree::new();
    let root = shelf_project(&tree, &["tools/x"]);
    std::fs::write(
        root.join(shelf::GATES),
        json!({"tools/x": {"gates": [
            {"name": "review", "state": "met", "by": "lucid-heron", "at": 1794398700.0,
             "detail": "Approved in round 2 of 2"},
            {"name": "certificate", "state": "missing", "commit": "4be91c2",
             "detail": "Verify on the merge tree 4be91c2 running"},
            {"name": "dependency", "state": "waived", "by": "cobalt-otter",
             "why": "tools/slot-pool-by-tree shares no code with this branch",
             "detail": "Waits on tools/slot-pool-by-tree, waived"},
            {"name": "hold", "state": "held", "by": "glacial-merlin",
             "why": "the owner asked to hold it", "detail": "Held by its orchestrator"},
        ]}})
        .to_string(),
    )
    .unwrap();
    // The Symphony's review says missing: the plugin's of the same name takes its place.
    let seed = Seed::default()
        .task("tools/x", "review")
        .role("tools/x", "lucid-heron", "reviewer");
    let f = Fixture::new(verdict(
        seed,
        1,
        "tools/x",
        "lucid-heron",
        "changes: not yet",
    ));
    let (shared, ask) = served(&root, hub(&root, &f, Duration::from_secs(30)));

    let (status, answer) = ask("/api/task-gates?slug=tools/x");
    assert_eq!(status, StatusCode::OK, "{answer}");
    let tip = git_tip(&root, "tools/x");
    assert_eq!(
        (&answer["slug"], &answer["branch"], &answer["tip"]),
        (&json!("tools/x"), &json!("tools/x"), &json!(tip))
    );
    assert_eq!(
        gates(&answer),
        [
            (
                "certificate".into(),
                GateState::Missing,
                "Verify on the merge tree 4be91c2 running".into()
            ),
            (
                "hold".into(),
                GateState::Held,
                "Held by its orchestrator".into()
            ),
            (
                "dependency".into(),
                GateState::Waived,
                "Waits on tools/slot-pool-by-tree, waived".into()
            ),
            (
                "review".into(),
                GateState::Met,
                "Approved in round 2 of 2".into()
            ),
        ],
        "exactly the plugin's four, missing, held, waived, met"
    );
    assert_eq!(
        answer["gates"][2]["why"],
        "tools/slot-pool-by-tree shares no code with this branch"
    );
    assert_eq!(calls(&root), [format!("tools/x tools/x {tip}")]);

    // Kept by tip: the same tip asks the plugin nothing; a new one asks it again.
    let (status, again) = ask("/api/task-gates?slug=tools/x");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, answer);
    assert_eq!(calls(&root).len(), 1);
    let moved = commit_on(&root, "tools/x");
    assert_ne!(moved, tip);
    let (status, answer) = ask("/api/task-gates?slug=tools/x");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(answer["tip"], json!(moved));
    assert_eq!(
        calls(&root),
        [
            format!("tools/x tools/x {tip}"),
            format!("tools/x tools/x {moved}")
        ]
    );

    // A task the plugin says nothing of keeps the Symphony's review, and a task not on the board
    // or no slug at all is no answer.
    let (status, answer) = ask("/api/task-gates?slug=tools/missing");
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    let (status, answer) = ask("/api/task-gates");
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert_eq!(answer["error"], "slug is required");
    shared.stop();
}

/// A commit on `branch` in `root`, main checked out again after: the branch's new tip.
fn commit_on(root: &Path, branch: &str) -> String {
    git(root, &["checkout", "-q", branch]);
    std::fs::write(root.join("work.txt"), branch).unwrap();
    git(root, &["add", "work.txt"]);
    git(root, &["commit", "-q", "-m", "work"]);
    git(root, &["checkout", "-q", "main"]);
    git_tip(root, branch)
}

fn git_tip(root: &Path, branch: &str) -> String {
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["rev-parse", branch])
            .current_dir(root),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    String::from_utf8(done.stdout).unwrap().trim().to_string()
}

#[test]
fn a_plugin_that_errs_panics_or_is_slow_is_one_unknown_gate_and_the_answer_still_arrives() {
    let tree = Tree::new();
    let root = shelf_project(&tree, &["tools/err", "tools/panic", "tools/slow"]);
    std::fs::write(
        root.join(shelf::GATES),
        json!({
            "tools/err": {"error": "land exited 2\nits log"},
            "tools/panic": {"panic": true},
            "tools/slow": {"sleep_ms": 3000, "gates": [{"name": "certificate", "state": "met"}]},
        })
        .to_string(),
    )
    .unwrap();
    let f = Fixture::new(
        Seed::default()
            .task("tools/err", "in_progress")
            .task("tools/panic", "in_progress")
            .task("tools/slow", "in_progress"),
    );
    let (shared, ask) = served(&root, hub(&root, &f, Duration::from_millis(300)));
    let review = (
        "review".to_string(),
        GateState::Missing,
        "Not reviewed yet".to_string(),
    );
    for (slug, said) in [
        ("tools/err", "land exited 2"),
        ("tools/panic", "shelf-gates panicked"),
        ("tools/slow", "shelf-gates gave no answer within 0.3 s"),
    ] {
        let began = Instant::now();
        let (status, answer) = ask(&format!("/api/task-gates?slug={slug}"));
        assert_eq!(status, StatusCode::OK, "{slug}: {answer}");
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{slug} waited on its plugin"
        );
        assert_eq!(
            gates(&answer),
            [
                review.clone(),
                ("shelf-gates".into(), GateState::Unknown, said.into()),
            ],
            "{slug}"
        );
    }
    // The slow answer, once it comes, is the one kept: no second call.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let (_, answer) = ask("/api/task-gates?slug=tools/slow");
        if gates(&answer)
            .iter()
            .any(|(name, _, _)| name == "certificate")
        {
            break;
        }
        assert!(Instant::now() < deadline, "the slow answer never came");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        calls(&root)
            .iter()
            .filter(|call| call.starts_with("tools/slow "))
            .count(),
        1
    );
    shared.stop();
}

#[test]
fn with_no_plugin_the_symphony_answers_review_check_and_owner_from_the_board_and_the_runs() {
    let tree = Tree::new();
    let root = repository(&tree);
    std::fs::write(
        root.join(symphony::project::FILE),
        "[verdicts]\ncheck = [\"just\", \"verify\"]\n",
    )
    .unwrap();
    for branch in ["tools/green", "tools/unrun", "tools/asks"] {
        git(&root, &["branch", branch]);
    }
    // Each branch at a tip of its own: a run at a commit counts for every branch there.
    let green = commit_on(&root, "tools/green");
    // The board's recorder kept a green `just verify` at the tip, in the repository's common dir.
    let work = root.join(".git/sf-tasks/work");
    let mut run = symphony_board::cli::machine_work::recorded::Record {
        version: 1,
        operation_id: format!("{:032x}", 7),
        parent_id: None,
        phase: symphony_board::cli::machine_work::recorded::Phase::Test,
        label: "just verify".into(),
        state: symphony_board::cli::machine_work::recorded::Step::Finished,
        handle: Some("quiet-wren".into()),
        task: Some("tools/green".into()),
        checkout: root.display().to_string(),
        branch: Some("tools/green".into()),
        commit: Some(green.clone()),
        machine_id: None,
        host: "host".into(),
        pid: 10,
        child_pid: Some(11),
        queued_at: NOW,
        started_at: Some(NOW),
        ended_at: Some(NOW + 1440.0),
        beat_at: NOW + 1440.0,
        exit_code: Some(0),
        signal: None,
        reason: String::new(),
    };
    symphony_board::cli::machine_work::recorded::keep(&work, &run).unwrap();
    // A run of another command at another task's tip says nothing of its check.
    run.operation_id = format!("{:032x}", 8);
    run.label = "just lint-rs".into();
    run.branch = Some("tools/unrun".into());
    symphony_board::cli::machine_work::recorded::keep(&work, &run).unwrap();
    let seed = Seed::default()
        .task("tools/green", "done")
        .task("tools/unrun", "review")
        .role("tools/unrun", "lucid-heron", "reviewer")
        .task("tools/asks", "in_progress")
        .link("tools/asks", "doc", "docs/TOPIC.md")
        .task("tools/green/slice", "in_progress");
    let seed = verdict(seed, 1, "tools/green", "lucid-heron", "changes: not yet");
    let mut seed = verdict(seed, 2, "tools/green", "lucid-heron", "approve: it holds");
    seed.dump
        .tasks
        .iter_mut()
        .for_each(|task| match task.slug.as_str() {
            "tools/asks" => task.body = "Posted initial-card for the owner.".into(),
            "tools/green/slice" => task.parent = Some("tools/green".into()),
            _ => {}
        });
    let f = Fixture::new(seed);
    let (shared, ask) = served(&root, hub(&root, &f, Duration::from_secs(30)));

    let (status, answer) = ask("/api/task-gates?slug=tools/green");
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(
        gates(&answer),
        [
            (
                "review".into(),
                GateState::Met,
                "Approved in round 2 of 2".into()
            ),
            (
                "check".into(),
                GateState::Met,
                format!("`just verify` green at {}", green.get(..7).unwrap())
            ),
        ]
    );
    assert_eq!(answer["gates"][0]["by"], "lucid-heron");
    assert_eq!(answer["gates"][0]["past"][0]["state"], "missing");
    assert_eq!(answer["gates"][1]["by"], "quiet-wren");
    assert_eq!(answer["gates"][1]["commit"], json!(green));

    let (status, answer) = ask("/api/task-gates?slug=tools/unrun");
    assert_eq!(status, StatusCode::OK, "{answer}");
    let unrun = git_tip(&root, "tools/unrun");
    assert_eq!(
        gates(&answer),
        [
            (
                "review".into(),
                GateState::Missing,
                "In review, no verdict yet".into()
            ),
            (
                "check".into(),
                GateState::Unknown,
                format!("`just verify` has not run at {}", unrun.get(..7).unwrap())
            ),
        ]
    );

    let (status, answer) = ask("/api/task-gates?slug=tools/asks");
    assert_eq!(status, StatusCode::OK, "{answer}");
    let asks = git_tip(&root, "tools/asks");
    assert_eq!(
        gates(&answer),
        [
            (
                "review".into(),
                GateState::Missing,
                "Not reviewed yet".into()
            ),
            (
                "owner".into(),
                GateState::Held,
                "Card initial-card in docs/TOPIC.md waits on the owner".into()
            ),
            (
                "check".into(),
                GateState::Unknown,
                format!("`just verify` has not run at {}", asks.get(..7).unwrap())
            ),
        ]
    );
    assert_eq!(answer["gates"][1]["link"], "/docs/TOPIC.md#initial-card");

    // A subtask has no review gate: only its top-level task goes to review. Its branch, its slug
    // here, is not in the repository, so its check cannot be told.
    let (status, answer) = ask("/api/task-gates?slug=tools/green/slice");
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(
        gates(&answer),
        [(
            "check".into(),
            GateState::Unknown,
            "No branch tools/green/slice in this repository".into()
        )]
    );
    assert_eq!(answer.get("tip"), None);
    assert!(
        !root.join(shelf::GATES_CALLS).exists(),
        "no plugin was asked"
    );
    shared.stop();
}
