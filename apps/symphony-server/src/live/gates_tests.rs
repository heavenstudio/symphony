//! `/api/task-gates` over a scratch board and a temporary repository: a project's plugin answers
//! (the shelf's, as its checkout's `gates.json` says), the Symphony's own gates where none does or
//! beside it, a failing or slow plugin as an unknown gate, and a plugin asked once a tip; and what
//! a read costs: one with nothing new asks the board once and reads nothing again.

use super::Hub;
use super::read_paths_tests::{observe, requests, served};
use crate::shelf;
use crate::testing::{
    Tree,
    board::{Fixture, NOW, Seed},
    git, repository, store_on,
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use symphony_api::{GateState, TaskGates};
use symphony_board::rows::MessageRow;

/// `seed` with a verdict `text` by `by` on `slug`'s thread, the `id`th line, at `NOW + id`.
fn verdict(seed: Seed, id: i64, slug: &str, by: &str, text: &str) -> Seed {
    said(seed, (id, "verdict"), slug, by, text)
}

/// `seed` with a line of `kind`, `text` by `by`, on `slug`'s thread, the `id`th, at `NOW + id`.
fn said(mut seed: Seed, (id, kind): (i64, &str), slug: &str, by: &str, text: &str) -> Seed {
    seed.dump.messages.push(MessageRow {
        id,
        slug: slug.into(),
        sender: by.into(),
        recipient: None,
        kind: kind.into(),
        text: text.into(),
        ts: NOW + f64::from(i32::try_from(id).unwrap()),
        person: false,
    });
    seed
}

/// How long a plugin that is meant to answer is waited for: a backstop only a stall reaches
/// (RULES.md `tests/no-test-fails-on-load`), as no plugin here but the held one is slow.
const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(30));

/// A hub over `root` and `f`'s board, its plugin answers waited for `wait`. A failed plugin is
/// asked again only past a backstop, so no test's steps, however slow, reach a retry.
fn hub(root: &Path, f: &Fixture, wait: Duration) -> Hub {
    let mut hub = Hub::new(root, &root.join("build/client"));
    let board = f.other_database();
    hub.connect_reader = Box::new(move |_| Ok(store_on(&board)));
    hub.gates = symphony::gates::Kept::new(symphony::gates::Limits {
        wait,
        retry: test_time::backstop(Duration::from_secs(60)),
        ..symphony::gates::Limits::default()
    });
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
             "detail": "Approved in round 2 of 2", "link": " JaVaScRiPt:alert(document.cookie)"},
            {"name": "certificate", "state": "missing", "commit": "4be91c2",
             "detail": "Verify on the merge tree 4be91c2 running", "link": "https://ci.example/runs/4be91c2"},
            {"name": "dependency", "state": "waived", "by": "cobalt-otter",
             "why": "tools/slot-pool-by-tree shares no code with this branch",
             "detail": "Waits on tools/slot-pool-by-tree, waived", "link": "/task/tools/slot-pool-by-tree"},
            {"name": "hold", "state": "held", "by": "glacial-merlin",
             "why": "the owner asked to hold it", "detail": "Held by its orchestrator", "link": "//evil.example/x"},
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
    let (shared, ask) = served(&root, hub(&root, &f, BACKSTOP));

    // The task page holds no room for gates the server has not answered yet.
    let (status, page) = ask("/api/task-page?slug=tools/x");
    assert_eq!(
        (status, page.get("gates")),
        (StatusCode::OK, None),
        "{page}"
    );

    let (status, answer) = ask("/api/task-gates?slug=tools/x");
    assert_eq!(status, StatusCode::OK, "{answer}");
    let tip = git_tip(&root, "tools/x");
    assert_eq!(
        (&answer["slug"], &answer["branch"], &answer["tip"]),
        (&json!("tools/x"), &json!("tools/x"), &json!(tip))
    );
    // The Symphony's stamps no assessment needs are waived, by the defaults, beside the plugin's.
    let by_default = |name: &str| {
        (
            name.to_string(),
            GateState::Waived,
            "Not needed by default: no assessment yet".to_string(),
        )
    };
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
            by_default("tests"),
            by_default("pictures"),
            by_default("owner"),
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
        "the plugin's four, missing, held, waived, met, its review in place of the Symphony's"
    );
    assert_eq!(
        answer["gates"][5]["why"],
        "tools/slot-pool-by-tree shares no code with this branch"
    );
    // Only a path of the app or a web address reaches the page as a link.
    let links: Vec<&Value> = [0, 1, 5, 6].map(|i| &answer["gates"][i]["link"]).to_vec();
    assert_eq!(
        links,
        [
            &json!("https://ci.example/runs/4be91c2"),
            &Value::Null,
            &json!("/task/tools/slot-pool-by-tree"),
            &Value::Null
        ],
        "the hold's //host and the review's javascript: are dropped"
    );
    let (status, page) = ask("/api/task-page?slug=tools/x");
    assert_eq!(
        (status, &page["gates"]),
        (
            StatusCode::OK,
            &json!({"count": 7, "waiting": 2, "unsigned": 4})
        ),
        "the page holds the room its gates took: the certificate and the stamps waived by default \
         say no one and no time"
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

    // A task that closes holds no room any more: the hub forgets it with the board's next change.
    f.store.move_to("tools/x", "cancelled", "owner").unwrap();
    observe(shared.api.get().unwrap(), &f.store, &[]);
    let (status, page) = ask("/api/task-page?slug=tools/x");
    assert_eq!(
        (status, page.get("gates")),
        (StatusCode::OK, None),
        "{page}"
    );
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
    // The slow plugin answers only once the test lets it, so its wait runs out first however
    // loaded the machine is, and a request that waited on it would never be answered.
    const RELEASED: &str = "slow.released";
    std::fs::write(
        root.join(shelf::GATES),
        json!({
            "tools/err": {"error": "land exited 2\nits log"},
            "tools/panic": {"panic": true},
            "tools/slow": {"held_until": RELEASED, "gates": [{"name": "certificate", "state": "met"}]},
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
    let review = (
        "review".to_string(),
        GateState::Missing,
        "Not reviewed yet".to_string(),
    );
    let by_default = |name: &str| {
        (
            name.to_string(),
            GateState::Waived,
            "Not needed by default: no assessment yet".to_string(),
        )
    };
    let unknown = |said: &str| {
        [
            review.clone(),
            ("shelf-gates".into(), GateState::Unknown, said.into()),
            by_default("tests"),
            by_default("pictures"),
            by_default("owner"),
        ]
    };
    // A plugin that errs or panics is said so within any wait; here one only a stall outlasts.
    let (shared, ask) = served(&root, hub(&root, &f, BACKSTOP));
    for (slug, said) in [
        ("tools/err", "land exited 2"),
        (
            "tools/panic",
            "shelf-gates panicked answering: gates.json says to panic for tools/panic",
        ),
    ] {
        let (status, answer) = ask(&format!("/api/task-gates?slug={slug}"));
        assert_eq!(status, StatusCode::OK, "{slug}: {answer}");
        assert_eq!(gates(&answer), unknown(said), "{slug}");
    }
    shared.stop();
    // A slow one is an unknown gate once its wait runs out.
    let (shared, ask) = served(&root, hub(&root, &f, Duration::from_millis(300)));
    let (status, answer) = ask("/api/task-gates?slug=tools/slow");
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(
        gates(&answer),
        unknown("shelf-gates gave no answer within 0.3 s")
    );
    // The slow answer, once it comes, is the one kept: no second call.
    std::fs::write(root.join(RELEASED), "").unwrap();
    test_time::until_true("the slow answer to come", BACKSTOP, || {
        let (_, answer) = ask("/api/task-gates?slug=tools/slow");
        gates(&answer)
            .iter()
            .any(|(name, _, _)| name == "certificate")
    })
    .unwrap();
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
    for branch in [
        "tools/green",
        "tools/unrun",
        "tools/asks",
        "tools/landed",
        "tools/brief/step",
    ] {
        git(&root, &["branch", branch]);
    }
    let base = git_tip(&root, "main");
    // Each branch at a tip of its own: a run at a commit counts for every branch there. Those with
    // none, tools/asks made at main's tip and tools/landed merged there, have nothing to check.
    let green = commit_on(&root, "tools/green");
    let unrun = commit_on(&root, "tools/unrun");
    commit_on(&root, "tools/landed");
    git(&root, &["merge", "-q", "--ff-only", "tools/landed"]);
    commit_on(&root, "tools/brief/step");
    // The board's recorder kept a green `just verify` at the tip, in the repository's common dir.
    let work = root.join(".git/symphony/tasks/work");
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
        run: None,
        reading: None,
    };
    symphony_board::cli::machine_work::recorded::keep(&work, &run).unwrap();
    // A run of another command at another task's tip says nothing of its check.
    run.operation_id = format!("{:032x}", 8);
    run.label = "just lint-rs".into();
    run.branch = Some("tools/unrun".into());
    symphony_board::cli::machine_work::recorded::keep(&work, &run).unwrap();
    let seed = Seed::default()
        .task("tools/green", "done")
        .task("tools/fixed", "done")
        .task("tools/dropped", "cancelled")
        .task("tools/unrun", "review")
        .role("tools/unrun", "lucid-heron", "reviewer")
        .task("tools/asks", "in_progress")
        .link("tools/asks", "doc", "docs/TOPIC.md")
        .task("tools/landed", "done")
        .task("tools/green/slice", "in_progress")
        .task("tools/brief", "ready")
        .task("tools/brief/step", "in_progress");
    let seed = verdict(seed, 1, "tools/green", "lucid-heron", "changes: not yet");
    let mut seed = verdict(seed, 2, "tools/green", "lucid-heron", "approve: it holds");
    seed.dump
        .tasks
        .iter_mut()
        .for_each(|task| match task.slug.as_str() {
            "tools/asks" => task.body = "Posted initial-card for the owner.".into(),
            "tools/green/slice" => task.parent = Some("tools/green".into()),
            "tools/brief/step" => {
                task.parent = Some("tools/brief".into());
                task.instruction = true;
            }
            _ => {}
        });
    let f = Fixture::new(seed);
    let (shared, ask) = served(&root, hub(&root, &f, BACKSTOP));

    // A task done with no assessment keeps the gates it had before stamps: its approve, which
    // names no commit (approved by time), is met, and no stamp is drawn.
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
    // Done with no round, a bug fix's waiver as before; cancelled, its review missing as before.
    // Neither has a branch, so no check.
    let (status, answer) = ask("/api/task-gates?slug=tools/fixed");
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(
        gates(&answer),
        [(
            "review".into(),
            GateState::Waived,
            "Landed with no review verdict".into()
        )]
    );
    assert_eq!(
        answer["gates"][0]["why"],
        "a bug fix lands on its evidence, with a reviewer only when its risk calls for one \
         (workflow/fast-bugfixes)"
    );
    let (status, answer) = ask("/api/task-gates?slug=tools/dropped");
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(
        gates(&answer),
        [(
            "review".into(),
            GateState::Missing,
            "Not reviewed yet".into()
        )]
    );
    // Its branch landed: it is on main, with nothing of its own left to check.
    let (status, answer) = ask("/api/task-gates?slug=tools/landed");
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(
        gates(&answer),
        [(
            "review".into(),
            GateState::Waived,
            "Landed with no review verdict".into()
        )]
    );
    assert_eq!(answer["tip"], json!(git_tip(&root, "tools/landed")));

    let (status, answer) = ask("/api/task-gates?slug=tools/unrun");
    assert_eq!(status, StatusCode::OK, "{answer}");
    // An open task's gates are its stamps, review needed by default, and the check beside them.
    let by_default = |name: &str| {
        (
            name.to_string(),
            GateState::Waived,
            "Not needed by default: no assessment yet".to_string(),
        )
    };
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
            by_default("tests"),
            by_default("pictures"),
            by_default("owner"),
        ]
    );

    // Its branch was made at main's tip and holds no commit yet: no check, whose run there would
    // be main's.
    let (status, answer) = ask("/api/task-gates?slug=tools/asks");
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["tip"], json!(base));
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
            by_default("tests"),
            by_default("pictures"),
        ]
    );
    assert_eq!(answer["gates"][1]["link"], "/docs/TOPIC.md#initial-card");

    // A subtask has no review gate: only its top-level task goes to review. Its branch, its slug
    // here, is not in the repository, so nothing gates it.
    let (status, answer) = ask("/api/task-gates?slug=tools/green/slice");
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(gates(&answer), []);
    assert_eq!(answer.get("tip"), None);
    // An instruction's work lands with its task: its own commits are checked there, not here.
    let (status, answer) = ask("/api/task-gates?slug=tools/brief/step");
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(gates(&answer), []);
    assert_eq!(answer["tip"], json!(git_tip(&root, "tools/brief/step")));
    assert!(
        !root.join(shelf::GATES_CALLS).exists(),
        "no plugin was asked"
    );
    shared.stop();
}

#[test]
fn an_assessed_task_reads_its_stamps_at_the_tip_and_offers_the_owners_approve_until_given() {
    use symphony_board::stamps::{Catalogue, Needs};
    let tree = Tree::new();
    let root = repository(&tree);
    git(&root, &["branch", "tools/own"]);
    let tip = commit_on(&root, "tools/own");
    let f = Fixture::new(Seed::default().task("tools/own", "in_progress"));
    let assessor = f.session("worker");
    let needs = Needs {
        stamps: vec!["owner".into()],
        tip: Some(tip.clone()),
        by: assessor.clone(),
        why: "the owner asked to see the page".into(),
        import: None,
    };
    f.store
        .set_needs("tools/own", &Catalogue::symphony(), &needs)
        .unwrap();
    let (shared, ask) = served(&root, hub(&root, &f, BACKSTOP));
    let short = tip.get(..7).unwrap();

    // Only the owner's stamp is needed at the tip the assessment read: it holds the task.
    let (status, answer) = ask("/api/task-gates?slug=tools/own");
    assert_eq!(status, StatusCode::OK, "{answer}");
    let left_out = |name: &str| {
        (
            name.to_string(),
            GateState::Waived,
            format!("Left out by the assessment at {short}"),
        )
    };
    assert_eq!(
        gates(&answer),
        [
            (
                "owner".into(),
                GateState::Held,
                format!("Waits on the owner's approve of {short}")
            ),
            left_out("tests"),
            left_out("review"),
            left_out("pictures"),
        ]
    );
    assert_eq!(answer["gates"][1]["by"], json!(assessor));
    assert_eq!(answer["gates"][1]["why"], "the owner asked to see the page");

    // The page offers the owner's approve of the tip; given, it is met and offered no more.
    let (status, offer) = ask("/api/task-offer?slug=tools/own");
    assert_eq!(status, StatusCode::OK, "{offer}");
    assert_eq!(
        offer,
        json!({"action": "owner-approve", "label": format!("Approve {short}"),
               "branch": "tools/own", "payload": tip})
    );
    let said = symphony::gates::stamps::approve_line(&tip).unwrap();
    f.store.say("tools/own", "owner", &said, None).unwrap();
    observe(shared.api.get().unwrap(), &f.store, &[]);
    let (_, answer) = ask("/api/task-gates?slug=tools/own");
    assert_eq!(
        gates(&answer)[3],
        (
            "owner".into(),
            GateState::Met,
            format!("Approved by the owner at {short}")
        ),
        "{answer}"
    );
    let (_, offer) = ask("/api/task-offer?slug=tools/own");
    assert_eq!(offer["payload"], Value::Null, "{offer}");

    // A commit after it: the approve is stale, and the defaults are needed again.
    git(&root, &["checkout", "-q", "tools/own"]);
    std::fs::write(root.join("work.txt"), "more work").unwrap();
    git(&root, &["commit", "-q", "-am", "more work"]);
    git(&root, &["checkout", "-q", "main"]);
    let later = git_tip(&root, "tools/own");
    let (_, answer) = ask("/api/task-gates?slug=tools/own");
    let now = later.get(..7).unwrap();
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
                format!("The owner approved {short}, not the tip {now}")
            ),
            left_out("tests"),
            left_out("pictures"),
        ]
    );
    let (_, offer) = ask("/api/task-offer?slug=tools/own");
    assert_eq!(offer["payload"], json!(later), "{offer}");
    shared.stop();
}

/// The person at the machine approves under the name they gave themselves, on a board naming
/// nobody: their approve meets the `owner` stamp once the hub reads their profiles' folder
/// (`Kept::machine_at`, as the server's gate names it), named by how they are shown, and the page
/// offers their approve no more; it counts for nothing while the hub reads none.
#[test]
fn the_machines_persons_approve_under_their_name_meets_the_owner_stamp() {
    use symphony_board::people::{Profile, Profiles};
    use symphony_board::stamps::{Catalogue, Needs};
    let tree = Tree::new();
    let root = repository(&tree);
    git(&root, &["branch", "tools/own"]);
    let tip = commit_on(&root, "tools/own");
    let f = Fixture::new(Seed::default().task("tools/own", "in_progress"));
    let needs = Needs {
        stamps: vec!["owner".into()],
        tip: Some(tip.clone()),
        by: f.session("worker"),
        why: "the owner asked to see the page".into(),
        import: None,
    };
    f.store
        .set_needs("tools/own", &Catalogue::symphony(), &needs)
        .unwrap();
    let said = symphony::gates::stamps::approve_line(&tip).unwrap();
    (f.other().for_machine("stefano"))
        .say("tools/own", "stefano", &said, None)
        .unwrap();
    let short = tip.get(..7).unwrap();
    let owner = |hub: Hub| {
        let (shared, ask) = served(&root, hub);
        let (status, answer) = ask("/api/task-gates?slug=tools/own");
        assert_eq!(status, StatusCode::OK, "{answer}");
        let (status, offer) = ask("/api/task-offer?slug=tools/own");
        assert_eq!(status, StatusCode::OK, "{offer}");
        shared.stop();
        let gate = gates(&answer)
            .into_iter()
            .find(|gate| gate.0 == "owner")
            .unwrap();
        (gate, offer["payload"].clone())
    };
    assert_eq!(
        owner(hub(&root, &f, BACKSTOP)),
        (
            (
                "owner".into(),
                GateState::Held,
                format!("Waits on the owner's approve of {short}")
            ),
            json!(tip)
        )
    );
    let machine = tree.path("machine");
    (Profiles {
        people: vec![Profile {
            name: "stefano".into(),
            display: "Stefano".into(),
            ..Profile::default()
        }],
        machine: Some("stefano".into()),
    })
    .write(&machine)
    .unwrap();
    let named = hub(&root, &f, BACKSTOP);
    named.gates.machine_at(machine);
    assert_eq!(
        owner(named),
        (
            (
                "owner".into(),
                GateState::Met,
                format!("Approved by Stefano at {short}")
            ),
            Value::Null
        )
    );
}

/// The owner's approve of the card a task's branch posted stands for its `owner` stamp: the page
/// reads it met, naming the card, and offers no approve of the tip.
#[test]
fn a_task_whose_posted_card_the_owner_approved_reads_its_owner_stamp_met_and_offers_no_approve() {
    use symphony_board::stamps::{Catalogue, Needs};
    let tree = Tree::new();
    let root = repository(&tree);
    let register = |card: &str| {
        format!("# Decisions\n\n<!-- register:start -->\n{card}<!-- register:end -->\n")
    };
    let look = "- [ ] **look** — Is the page right?\n  [2026-11-20 04:00] agent — Posted.\n";
    let approved = look.replace("[ ]", "[x]") + "  [2026-11-20 05:00] owner · approved — it is\n";
    git(&root, &["checkout", "-q", "-b", "tools/own"]);
    for (text, message) in [
        (register(look), "post look"),
        (register(&approved), "approve"),
    ] {
        std::fs::write(root.join("docs/DECISIONS.md"), text).unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", message]);
    }
    git(&root, &["checkout", "-q", "main"]);
    let tip = git_tip(&root, "tools/own");
    let f = Fixture::new(Seed::default().task("tools/own", "review"));
    let needs = Needs {
        stamps: vec!["owner".into()],
        tip: Some(tip.clone()),
        by: f.session("worker"),
        why: "the owner asked to see the page".into(),
        import: None,
    };
    f.store
        .set_needs("tools/own", &Catalogue::symphony(), &needs)
        .unwrap();
    let (shared, ask) = served(&root, hub(&root, &f, BACKSTOP));
    let short = tip.get(..7).unwrap();
    let (status, answer) = ask("/api/task-gates?slug=tools/own");
    assert_eq!(status, StatusCode::OK, "{answer}");
    let owner = gates(&answer).into_iter().find(|gate| gate.0 == "owner");
    assert_eq!(
        owner,
        Some((
            "owner".into(),
            GateState::Met,
            format!("Approved by the owner on card look, at {short}")
        )),
        "{answer}"
    );
    let (status, offer) = ask("/api/task-offer?slug=tools/own");
    assert_eq!(status, StatusCode::OK, "{offer}");
    assert_eq!(offer["payload"], Value::Null, "{offer}");
    shared.stop();
}

/// The recorder's finished run `id` of `label`, for `task` at `commit`, its exit `code`, kept in
/// `root`'s common dir.
fn keep_run(root: &Path, id: u32, task: &str, label: &str, (commit, code): (&str, i32)) {
    use symphony_board::cli::machine_work::recorded::{self, Phase, Record, Step};
    let at = NOW + f64::from(id) * 60.0;
    let run = Record {
        version: 1,
        operation_id: format!("{id:032x}"),
        parent_id: None,
        phase: Phase::Test,
        label: label.into(),
        state: Step::Finished,
        handle: Some("quiet-wren".into()),
        task: Some(task.into()),
        checkout: root.display().to_string(),
        branch: Some(task.into()),
        commit: Some(commit.into()),
        machine_id: None,
        host: "host".into(),
        pid: 10,
        child_pid: Some(11),
        queued_at: at,
        started_at: Some(at),
        ended_at: Some(at + 30.0),
        beat_at: at + 30.0,
        exit_code: Some(code),
        signal: None,
        reason: String::new(),
        run: None,
        reading: None,
    };
    recorded::keep(&root.join(".git/symphony/tasks/work"), &run).unwrap();
}

#[test]
fn a_tasks_tests_stamp_reads_the_runs_of_the_work_under_it_and_no_other_tasks() {
    use symphony_board::stamps::{Catalogue, Needs};
    let tree = Tree::new();
    let root = repository(&tree);
    git(&root, &["branch", "tools/runs"]);
    let tip = commit_on(&root, "tools/runs");
    let mut seed = Seed::default()
        .task("tools/runs", "in_progress")
        .task("tools/runs/work", "done")
        .task("tools/runs/work/deep", "done")
        .task("tools/other", "in_progress");
    seed.dump
        .tasks
        .iter_mut()
        .for_each(|task| match task.slug.as_str() {
            "tools/runs/work" => task.parent = Some("tools/runs".into()),
            "tools/runs/work/deep" => task.parent = Some("tools/runs/work".into()),
            _ => {}
        });
    let f = Fixture::new(seed);
    let needs = Needs {
        stamps: vec!["tests".into()],
        tip: Some(tip.clone()),
        by: f.session("worker"),
        why: "a store query and its test".into(),
        import: None,
    };
    f.store
        .set_needs("tools/runs", &Catalogue::symphony(), &needs)
        .unwrap();
    // Work two levels under the task ran its tests green at the tip; another task's run of the same
    // command there, newer and red, is not the task's.
    keep_run(&root, 1, "tools/runs/work/deep", "just test-rs", (&tip, 0));
    keep_run(&root, 2, "tools/other", "just test-rs", (&tip, 101));
    let (shared, ask) = served(&root, hub(&root, &f, BACKSTOP));
    let short = tip.get(..7).unwrap();

    let (status, answer) = ask("/api/task-gates?slug=tools/runs");
    assert_eq!(status, StatusCode::OK, "{answer}");
    let left_out = |name: &str| {
        (
            name.to_string(),
            GateState::Waived,
            format!("Left out by the assessment at {short}"),
        )
    };
    assert_eq!(
        gates(&answer),
        [
            left_out("review"),
            left_out("pictures"),
            left_out("owner"),
            (
                "tests".into(),
                GateState::Met,
                format!("Green at {short}: `just test-rs`")
            ),
        ]
    );
    assert_eq!(answer["gates"][3]["by"], "quiet-wren");
    assert_eq!(answer["gates"][3]["commit"], json!(tip));
    shared.stop();
}

#[test]
fn a_landed_tasks_stamps_stand_on_the_tip_its_land_merged_once_the_sweep_removed_its_branch() {
    use symphony_board::stamps::{NEEDS, Needs};
    let tree = Tree::new();
    let root = repository(&tree);
    // Each branch merged on main as the land merges it, then removed as the sweep removes it.
    let mut landed = Vec::new();
    for slug in ["tools/landed", "tools/merged"] {
        git(&root, &["branch", slug]);
        let tip = commit_on(&root, slug);
        git(&root, &["merge", "-q", "--no-ff", "-m", "land", slug]);
        let merge = git_tip(&root, "main");
        git(&root, &["branch", "-q", "-D", slug]);
        landed.push((tip, merge));
    }
    let [(tip, merge), (other, other_merge)] = <[(String, String); 2]>::try_from(landed).unwrap();
    let gone = "0123abcd0123abcd0123abcd0123abcd0123abcd";
    let mut seed = Seed::default()
        .task("tools/landed", "done")
        .task("tools/merged", "done")
        .task("tools/gone", "done");
    for (slug, at) in [
        ("tools/landed", &tip),
        ("tools/merged", &other),
        ("tools/gone", &gone.to_string()),
    ] {
        let needs = Needs {
            stamps: vec!["tests".into(), "review".into()],
            tip: Some(at.clone()),
            by: "slate-owl".into(),
            why: "a store query and its test".into(),
            import: None,
        };
        let task = (seed.dump.tasks.iter_mut())
            .find(|task| task.slug == slug)
            .unwrap();
        task.fields.insert(
            symphony_board::red::FIELDS.into(),
            [(NEEDS.to_string(), serde_json::to_value(needs).unwrap())].into(),
        );
    }
    // The landing's line names its merge, whose second parent is the tip; the land's own line,
    // where no landing names a merge, names the tip it merged.
    let seed = verdict(
        seed,
        1,
        "tools/landed",
        "lucid-heron",
        &format!("approve: {tip}"),
    );
    let landing = format!(
        "landed tools/landed on main at {}",
        merge.get(..12).unwrap()
    );
    let seed = verdict(seed, 2, "tools/landed", "quiet-plover", &landing);
    let seed = verdict(
        seed,
        3,
        "tools/merged",
        "lucid-heron",
        &format!("approve: {other}"),
    );
    let land = format!("land tools/merged: merged {other} as {other_merge}; moving main there");
    let seed = said(seed, (4, "msg"), "tools/merged", "quiet-plover", &land);
    // A landing naming no commit the repository holds, and no land's line: nothing to read at.
    let seed = verdict(
        seed,
        5,
        "tools/gone",
        "lucid-heron",
        &format!("approve: {gone}"),
    );
    let seed = verdict(
        seed,
        6,
        "tools/gone",
        "quiet-plover",
        "landed tools/gone on main at 0badc0ffee12",
    );
    keep_run(&root, 1, "tools/landed", "just test-rs", (&tip, 0));
    keep_run(&root, 2, "tools/merged", "just test-rs", (&other, 0));
    let f = Fixture::new(seed);
    let (shared, ask) = served(&root, hub(&root, &f, BACKSTOP));

    for (slug, tip) in [("tools/landed", &tip), ("tools/merged", &other)] {
        let (status, answer) = ask(&format!("/api/task-gates?slug={slug}"));
        assert_eq!(status, StatusCode::OK, "{slug}: {answer}");
        let short = tip.get(..7).unwrap();
        let left_out = format!("Left out by the assessment at {short}");
        assert_eq!(
            gates(&answer),
            [
                ("pictures".into(), GateState::Waived, left_out.clone()),
                ("owner".into(), GateState::Waived, left_out),
                (
                    "tests".into(),
                    GateState::Met,
                    format!("Green at {short}: `just test-rs`")
                ),
                (
                    "review".into(),
                    GateState::Met,
                    "Approved in round 1 of 1".into()
                ),
            ],
            "{slug}"
        );
        assert_eq!(answer["gates"][3]["commit"], json!(tip), "{slug}");
    }
    let (status, answer) = ask("/api/task-gates?slug=tools/gone");
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(
        gates(&answer)[..2],
        [
            (
                "tests".into(),
                GateState::Unknown,
                "No branch tools/gone in this repository, nor a land on its thread naming the \
                 commit it merged"
                    .into()
            ),
            (
                "review".into(),
                GateState::Unknown,
                "Approved in round 1 of 1, but no branch tools/gone is in this repository, nor a \
                 land on its thread naming the commit it merged"
                    .into()
            ),
        ]
    );
    shared.stop();
}

#[test]
fn the_sign_off_lane_follows_a_plugins_owner_gate_kept_for_the_task_as_it_is_now() {
    let tree = Tree::new();
    let root = shelf_project(&tree, &["tools/x", "tools/y"]);
    std::fs::write(
        root.join(shelf::GATES),
        json!({
            "tools/x": {"gates": [{"name": "owner", "state": "held", "by": "owner",
                                   "detail": "Waits on the owner's tier sign-off"}]},
            "tools/y": {"gates": [{"name": "owner", "state": "met", "by": "owner",
                                   "detail": "Signed off"}]},
        })
        .to_string(),
    )
    .unwrap();
    // tools/y posted a card that waits on the owner, so the Symphony's own rule lifts it alone.
    let seed = Seed::default()
        .session("worker")
        .task("tools/x", "in_progress")
        .role("tools/x", "worker", "executor")
        .task("tools/y", "in_progress")
        .role("tools/y", "worker", "executor")
        .link("tools/y", "doc", "docs/TOPIC.md");
    let f = Fixture::new(said(
        seed,
        (1, "msg"),
        "tools/y",
        "worker",
        "Please answer initial-card.",
    ));
    let (shared, ask) = served(&root, hub(&root, &f, BACKSTOP));
    let hub = shared.api.get().unwrap();
    let lane = || hub.task_signoff.lock().unwrap().clone();
    assert_eq!(lane(), ["tools/y"], "nothing kept yet: the Symphony's rule");
    assert_eq!(
        hub.waiting.lock().unwrap().lane,
        std::collections::BTreeMap::from([("tools/y".into(), vec!["initial-card".into()])])
    );
    assert!(calls(&root).is_empty(), "the lane asks the plugin nothing");

    // The pages ask the plugin; the lane reads what came back at the next look, the board unchanged.
    for slug in ["tools/x", "tools/y"] {
        let (status, answer) = ask(&format!("/api/task-gates?slug={slug}"));
        assert_eq!(status, StatusCode::OK, "{slug}: {answer}");
    }
    observe(hub, &f.store, &[]);
    assert_eq!(
        lane(),
        ["tools/x"],
        "the plugin's owner gate in the Symphony's place: held lifts tools/x, met leaves tools/y"
    );

    assert_eq!(
        hub.waiting.lock().unwrap().lane,
        std::collections::BTreeMap::from([("tools/x".into(), Vec::<String>::new())])
    );

    // In review, neither has an answer kept for it as it is now: the card rule decides again, so
    // tools/y, whose card still waits, is back and tools/x, which posted none, is out.
    for slug in ["tools/x", "tools/y"] {
        f.store.move_to(slug, "review", "worker").unwrap();
    }
    observe(hub, &f.store, &[]);
    assert_eq!(lane(), ["tools/y"]);
    assert_eq!(
        hub.waiting.lock().unwrap().lane,
        std::collections::BTreeMap::from([("tools/y".into(), vec!["initial-card".into()])])
    );
    assert_eq!(calls(&root).len(), 2, "asked by the pages alone");
    shared.stop();
}

#[test]
fn a_plugins_owner_gate_for_one_task_never_hides_another_tasks_card_from_the_lane() {
    let tree = Tree::new();
    let root = shelf_project(&tree, &["tools/x", "tools/z"]);
    // The plugin holds tools/x for the owner and says nothing of tools/z, whose card waits.
    std::fs::write(
        root.join(shelf::GATES),
        json!({"tools/x": {"gates": [{"name": "owner", "state": "held", "by": "owner",
                                      "detail": "Waits on the owner's tier sign-off"}]}})
        .to_string(),
    )
    .unwrap();
    let seed = Seed::default()
        .session("worker")
        .task("tools/x", "in_progress")
        .role("tools/x", "worker", "executor")
        .task("tools/z", "in_progress")
        .role("tools/z", "worker", "executor")
        .link("tools/z", "doc", "docs/TOPIC.md");
    let f = Fixture::new(said(
        seed,
        (1, "msg"),
        "tools/z",
        "worker",
        "Please answer initial-card.",
    ));
    let (shared, ask) = served(&root, hub(&root, &f, BACKSTOP));
    let hub = shared.api.get().unwrap();
    let lane = || hub.task_signoff.lock().unwrap().clone();
    assert_eq!(lane(), ["tools/z"], "before any read: its card");
    let read = |slug: &str| {
        let (status, answer) = ask(&format!("/api/task-gates?slug={slug}"));
        assert_eq!(status, StatusCode::OK, "{slug}: {answer}");
        observe(hub, &f.store, &[]);
    };
    read("tools/x");
    assert_eq!(
        lane(),
        ["tools/x", "tools/z"],
        "tools/x's page alone: the plugin's owner for tools/x leaves tools/z's card standing"
    );
    read("tools/z");
    assert_eq!(
        lane(),
        ["tools/x", "tools/z"],
        "tools/z's page too: the plugin's answer has no owner gate for it"
    );
    shared.stop();
}

/// `text` committed to `file` on `branch` of `root`, main checked out again after: its tip.
fn commit_more(root: &Path, branch: &str, file: &str, text: &str) -> String {
    git(root, &["checkout", "-q", branch]);
    std::fs::write(root.join(file), text).unwrap();
    git(root, &["add", file]);
    git(root, &["commit", "-q", "-m", text]);
    git(root, &["checkout", "-q", "main"]);
    git_tip(root, branch)
}

/// The state of the gate `name` in `answer`.
fn state(answer: &Value, name: &str) -> GateState {
    gates(answer)
        .into_iter()
        .find(|(gate, _, _)| gate == name)
        .unwrap_or_else(|| panic!("no gate {name} in {answer}"))
        .1
}

/// A decider's approve of the tip on the task's thread meets its `owner` gate, named as theirs,
/// only while the board gives them decide: who decides comes with the task's stamp, so a grant or
/// a revoke is read at the next read, and a read with nothing new still asks the board once.
#[test]
fn a_deciders_approve_meets_the_owner_gate_while_the_board_gives_them_decide() {
    use symphony_board::people::{Holder, Power};
    use symphony_board::stamps::{Catalogue, Needs};
    let tree = Tree::new();
    let root = repository(&tree);
    git(&root, &["branch", "tools/kept"]);
    let tip = commit_on(&root, "tools/kept");
    let f = Fixture::new(Seed::default().task("tools/kept", "review"));
    let needs = Needs {
        stamps: vec!["owner".into()],
        tip: Some(tip.clone()),
        by: f.session("worker"),
        why: "the owner asked to see it".into(),
        import: None,
    };
    f.store
        .set_needs("tools/kept", &Catalogue::symphony(), &needs)
        .unwrap();
    let (_shared, ask) = served(&root, hub(&root, &f, BACKSTOP));
    let owner_gate = || {
        let (status, answer) = ask("/api/task-gates?slug=tools/kept");
        assert_eq!(status, StatusCode::OK, "{answer}");
        (gates(&answer).into_iter())
            .find(|(name, _, _)| name == "owner")
            .map(|(_, state, detail)| (state, detail))
            .unwrap()
    };
    let by = f.session("orchestrator");
    f.store
        .add_person("ana", &[], &[Power::Speak], &by)
        .unwrap();
    let said = symphony::gates::stamps::approve_line(&tip).unwrap();
    (f.other().for_person("ana"))
        .say("tools/kept", "ana", &said, None)
        .unwrap();
    assert_eq!(owner_gate().0, GateState::Held, "no decide, no approve");
    f.store
        .grant(Holder::Person("ana"), &[Power::Decide], &by)
        .unwrap();
    let met = (GateState::Met, format!("Approved by ana at {}", &tip[..7]));
    assert_eq!(owner_gate(), met, "a grant after her approve counts it");
    let sent = requests(&f.store);
    assert_eq!(owner_gate(), met);
    assert_eq!(requests(&f.store) - sent, 1, "one request to the board");
    f.store
        .revoke(Holder::Person("ana"), &[Power::Decide], &by)
        .unwrap();
    assert_eq!(owner_gate().0, GateState::Held, "a revoke withdraws it");
}

/// What a gates read costs is counted, never timed: the requests the board is sent, and each part
/// read anew (`symphony::gates::Reads`).
#[test]
fn a_gates_read_with_nothing_new_asks_the_board_once_and_reads_nothing_again() {
    use symphony::gates::Reads;
    use symphony_board::stamps::{Catalogue, Needs};
    let tree = Tree::new();
    let root = repository(&tree);
    std::fs::write(
        root.join(symphony::project::FILE),
        "[verdicts]\ncheck = [\"just\", \"verify\"]\n",
    )
    .unwrap();
    git(&root, &["branch", "tools/kept"]);
    let tip = commit_on(&root, "tools/kept");
    let mut seed = Seed::default()
        .task("tools/kept", "in_progress")
        .task("tools/kept/work", "in_progress");
    seed.dump
        .tasks
        .iter_mut()
        .filter(|task| task.slug == "tools/kept/work")
        .for_each(|task| task.parent = Some("tools/kept".into()));
    let f = Fixture::new(seed);
    let needs = Needs {
        stamps: vec!["tests".into(), "owner".into()],
        tip: Some(tip.clone()),
        by: f.session("worker"),
        why: "it changes code the owner asked to see".into(),
        import: None,
    };
    f.store
        .set_needs("tools/kept", &Catalogue::symphony(), &needs)
        .unwrap();
    let (shared, ask) = served(&root, hub(&root, &f, BACKSTOP));
    let hub = shared.api.get().unwrap();
    let read = || {
        let sent = requests(&f.store);
        let (status, answer) = ask("/api/task-gates?slug=tools/kept");
        assert_eq!(status, StatusCode::OK, "{answer}");
        (answer, requests(&f.store) - sent, hub.gates.reads())
    };
    let states = |answer: &Value| ["tests", "owner", "check"].map(|name| state(answer, name));

    // The first read reads everything once: the task and its thread, the recorder's history (no
    // operation is in its folder), the catalogue, whether the tip holds work of its own, and the
    // cards its branch posted, as the thread holds no approve of the tip.
    let (first, _, counted) = read();
    assert_eq!(
        states(&first),
        [GateState::Missing, GateState::Held, GateState::Unknown],
        "{first}"
    );
    let all_once = Reads {
        tasks: 1,
        histories: 1,
        records: 0,
        catalogues: 1,
        merge_bases: 1,
        posted: 1,
    };
    assert_eq!(counted, all_once);

    // With nothing new, a read asks the board once, for the task's stamp, and reads nothing
    // again: the same answer.
    for _ in 0..3 {
        let (again, sent, counted) = read();
        assert_eq!(again, first);
        assert_eq!(sent, 1, "one request to the board");
        assert_eq!(counted, all_once, "nothing read again");
    }

    // A post on its thread: the task and its thread are read again, nothing else.
    let said = symphony::gates::stamps::approve_line(&tip).unwrap();
    f.store.say("tools/kept", "owner", &said, None).unwrap();
    let (posted, _, counted) = read();
    assert_eq!(state(&posted, "owner"), GateState::Met, "{posted}");
    let after_post = Reads {
        tasks: 2,
        ..all_once
    };
    assert_eq!(counted, after_post);

    // A run recorded for the work under it: the history is read again, the board's task is not.
    keep_run(&root, 1, "tools/kept/work", "just test-rs", (&tip, 0));
    let (ran, sent, counted) = read();
    assert_eq!(state(&ran, "tests"), GateState::Met, "{ran}");
    assert_eq!(sent, 1);
    let after_run = Reads {
        histories: 2,
        ..after_post
    };
    assert_eq!(counted, after_run);

    // A new tip: what stood on the old one stands no more, and only the merge base is read again,
    // with the cards, at the tip the thread's approve no longer names.
    let later = commit_more(&root, "tools/kept", "work.txt", "more work");
    let (moved, sent, counted) = read();
    assert_eq!(moved["tip"], json!(later));
    assert_ne!(states(&moved), states(&ran), "{moved}");
    assert_eq!(sent, 1);
    let after_tip = Reads {
        merge_bases: 2,
        posted: 2,
        ..after_run
    };
    assert_eq!(counted, after_tip);

    // A commit on main: the catalogue is read again at the main checkout's new head, and the
    // merge base and the cards past the default branch's new tip.
    commit_more(&root, "main", "main.txt", "on main");
    let (_, sent, counted) = read();
    assert_eq!(sent, 1);
    let after_main = Reads {
        catalogues: 2,
        merge_bases: 3,
        posted: 3,
        ..after_tip
    };
    assert_eq!(counted, after_main);

    // Each stands once read: the next read is one request again, and reads nothing.
    let (_, sent, counted) = read();
    assert_eq!((sent, counted), (1, after_main));
    shared.stop();
}
