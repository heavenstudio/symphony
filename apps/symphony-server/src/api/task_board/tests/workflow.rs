//! A board a custom workflow holds: each held task in its phase's column, blocked or not, the
//! process overlays over it, its coordination state counted and selected as a wait facet, and the
//! active generation the page reads the definition by.

use super::*;
use symphony_api::workflow::TaskPlace;
use symphony_api::{Task, WaitingColumn};

/// `slug` held at `phase`/`state`, its status the one the position projects to.
fn held(slug: &str, status: &str, phase: &str, state: &str, since: f64) -> Task {
    Task {
        workflow: Some(TaskPlace {
            phase: phase.into(),
            state: state.into(),
            resume: None,
            revision: 3,
            manual: None,
        }),
        ..task(slug.into(), status, 2, since)
    }
}

/// A board with work in Build (one working, one blocked there), one awaiting review in Verify, a
/// task the board's own writers keep in review and one they finished.
fn board() -> Vec<Task> {
    vec![
        held("w/working", "in_progress", "build", "working", 10.0),
        held("w/stuck", "blocked", "build", "blocked", 20.0),
        held("w/second", "review", "verify", "awaiting_review", 30.0),
        task("legacy/review".into(), "review", 2, 40.0),
        task("legacy/done".into(), "done", 2, f64::MAX / 2.0),
    ]
}

async fn waiting(records: &[Task], query: &str) -> TaskListing {
    listing_of(records, &format!("view=waiting&{query}")).await
}

fn columns(list: &TaskListing) -> Vec<WaitingColumn> {
    list.waiting.as_ref().unwrap().columns.clone()
}

/// The status a listing's `query` answers with, and its body, while `generation` is active.
async fn raw(records: &[Task], query: &str, generation: Option<u64>) -> (StatusCode, String) {
    let request = Request::builder()
        .uri(format!("/api/task-board?{query}"))
        .body(())
        .unwrap();
    let tasks = Tasks {
        version: String::new(),
        tasks: records.to_vec(),
        sessions: vec![],
        orchestrators: vec![],
        unstarted: 0,
    };
    let response = project(
        &request,
        tasks,
        "",
        &BTreeMap::new(),
        MachineWork::default(),
        vec![],
        generation,
    );
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn a_held_task_goes_in_its_phase_column_blocked_or_not_and_a_status_word_never_takes_it() {
    let list = waiting(&board(), "all=1").await;
    assert_eq!(
        columns(&list),
        vec![
            WaitingColumn {
                status: "done".into(),
                ordinary: 1,
                blocked: 0,
                oldest: Some(f64::MAX / 2.0),
            },
            WaitingColumn {
                status: "phase:build".into(),
                ordinary: 1,
                blocked: 1,
                oldest: Some(10.0),
            },
            WaitingColumn {
                status: "phase:verify".into(),
                ordinary: 1,
                blocked: 0,
                oldest: Some(30.0),
            },
            WaitingColumn {
                status: "review".into(),
                ordinary: 1,
                blocked: 0,
                oldest: Some(40.0),
            },
        ]
    );
    // Its block is the workflow's state, never the board's own unnamed block made ordinary.
    let stuck = list.tasks.iter().find(|t| t.slug == "w/stuck").unwrap();
    assert_eq!(stuck.status, "blocked");
    let build = waiting(&board(), "status=phase:build").await;
    assert_eq!(slugs(&build), ["w/stuck", "w/working"]);
    assert_eq!(
        build.tasks[0].workflow.as_ref().map(|p| p.state.as_str()),
        Some("blocked")
    );
    // A phase named as a status is still the phase's column, and the status's column keeps its own.
    let named = vec![
        held("w/named", "review", "review", "waiting", 1.0),
        task("legacy/review".into(), "review", 2, 2.0),
    ];
    let review = waiting(&named, "status=review").await;
    assert_eq!(slugs(&review), ["legacy/review"]);
    let phase = waiting(&named, "status=phase:review").await;
    assert_eq!(slugs(&phase), ["w/named"]);
    // Every other listing keeps its lifecycle stages: a held task counts and is selected by its
    // status, and no count names a phase.
    let plain = listing_of(&board(), "").await;
    assert_eq!(total(&plain, "in_progress"), 1);
    assert_eq!(total(&plain, "blocked"), 1);
    assert_eq!(total(&plain, "review"), 2);
    assert!(
        plain
            .counts
            .iter()
            .all(|(stage, _)| !stage.starts_with(PHASE)),
        "{:?}",
        plain.counts
    );
    let working = listing_of(&board(), "status=in_progress").await;
    assert_eq!(slugs(&working), ["w/working"]);
}

#[tokio::test]
async fn open_held_tasks_count_by_state_and_a_state_lists_them_across_their_phases() {
    let list = waiting(&board(), "counts=1").await;
    assert_eq!(
        list.waiting.as_ref().unwrap().states,
        vec![
            ("awaiting_review".into(), 1),
            ("blocked".into(), 1),
            ("working".into(), 1)
        ]
    );
    // Two phases share a state: its facet lists both, oldest wait first, as a wait kind's does.
    let mut two = board();
    two.push(held("w/fixing", "in_progress", "verify", "working", 5.0));
    let working = waiting(&two, "wait=state:working").await;
    assert_eq!(slugs(&working), ["w/fixing", "w/working"]);
    assert_eq!(
        columns(&working)
            .iter()
            .map(|c| (c.status.as_str(), c.ordinary))
            .collect::<Vec<_>>(),
        [("phase:build", 1), ("phase:verify", 1)]
    );
    // The states and the kinds are counted before the facet chosen.
    let blocked = waiting(&board(), "wait=state:blocked").await;
    assert_eq!(slugs(&blocked), ["w/stuck"]);
    assert_eq!(
        blocked.waiting.as_ref().unwrap().states,
        list.waiting.as_ref().unwrap().states
    );
    assert_eq!(
        blocked.waiting.as_ref().unwrap().kinds,
        list.waiting.as_ref().unwrap().kinds
    );
    let none = waiting(&board(), "wait=state:failed").await;
    assert!(none.tasks.is_empty() && columns(&none).is_empty());
    // A finished held task is in no state's count, and no state's facet lists it.
    let mut finished = board();
    finished.push(held("w/accepted", "done", "verify", "accepted", 50.0));
    let counted = waiting(&finished, "counts=1&all=1").await;
    assert_eq!(counted.waiting.as_ref().unwrap().states.len(), 3);
    assert!(
        waiting(&finished, "wait=state:accepted&all=1")
            .await
            .tasks
            .is_empty()
    );
}

#[tokio::test]
async fn a_board_no_workflow_holds_sends_no_states() {
    let legacy = vec![task("legacy/review".into(), "review", 2, 40.0)];
    let (status, body) = raw(&legacy, "view=waiting&counts=1", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("\"states\""), "{body}");
    assert!(!body.contains("\"workflow\""), "{body}");
    let (_, body) = raw(&board(), "view=waiting&status=phase:build", Some(4)).await;
    assert!(body.contains("\"states\""), "{body}");
    assert!(body.contains("\"workflow\":{\"phase\":\"build\""), "{body}");
    // The generation the page reads the definition by, active though no open task is held yet.
    let (_, body) = raw(&legacy, "view=waiting", Some(4)).await;
    let list: TaskListing = serde_json::from_str(&body).unwrap();
    assert_eq!(list.waiting.as_ref().unwrap().workflow, Some(4));
    assert!(list.waiting.as_ref().unwrap().states.is_empty());
}

#[tokio::test]
async fn the_build_overlay_takes_a_held_task_its_machine_builds() {
    let records = vec![held("w/built", "in_progress", "build", "working", 1.0)];
    let work: MachineWork = serde_json::from_value(json!({"freshness":"fresh","machines":[{
        "machine_id":"alpha","label":"Alpha","producer_id":"fixture","sequence":1,
        "age_ms":0,"freshness":"fresh","complete":true,"reason":"","counts":null,
        "operations":[{"machine_id":"alpha","operation_id":"w/built","parent_id":null,
        "handle":"worker","task":"w/built","checkout":"opaque","recipe":"fixture",
        "kind":"build","state":"running","resource":"native","reason":"","phase_since":null}]}],
        "unassigned":[]}))
    .unwrap();
    let read = |activity: bool| {
        let request = Request::builder()
            .uri(format!(
                "/api/task-board?view=waiting&activity={}",
                u8::from(activity)
            ))
            .body(())
            .unwrap();
        let tasks = Tasks {
            version: String::new(),
            tasks: records.clone(),
            sessions: vec![],
            orchestrators: vec![],
            unstarted: 0,
        };
        project(
            &request,
            tasks,
            "",
            &BTreeMap::new(),
            work.clone(),
            vec![],
            None,
        )
    };
    for (activity, column) in [(true, "build"), (false, "phase:build")] {
        let bytes = axum::body::to_bytes(read(activity).into_body(), usize::MAX)
            .await
            .unwrap();
        let list: TaskListing = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(columns(&list)[0].status, column, "activity {activity}");
    }
}

#[test]
fn the_waiting_view_selects_a_phase_by_its_id_alone_and_no_other_listing_does() {
    let argument = |query: &str| {
        let request = Request::builder()
            .uri(format!("/api/task-board?{query}"))
            .body(())
            .unwrap();
        status_argument(&request)
    };
    for good in [
        "view=waiting&status=phase:build",
        "view=waiting&status=phase:verify-2",
        "view=waiting&status=phase:a_b",
    ] {
        assert_eq!(argument(good), Ok(()), "{good}");
    }
    for bad in [
        "view=waiting&status=phase:",
        "view=waiting&status=phase:Build",
        "view=waiting&status=phase:2nd",
        "view=waiting&status=phases:build",
        "view=waiting&status=build:phase",
        "status=phase:build",
        "view=tree&status=phase:build",
    ] {
        assert!(argument(bad).is_err(), "{bad}");
    }
}

/// A state's facet names a workflow ID; any other wait word is refused, naming both forms.
#[tokio::test]
async fn a_state_facet_names_a_workflow_id() {
    for good in ["state:awaiting_review", "state:x", "decision"] {
        let (status, body) = raw(&board(), &format!("view=waiting&wait={good}"), None).await;
        assert_eq!(status, StatusCode::OK, "{good}: {body}");
    }
    for bad in [
        "state:Blocked",
        "state:",
        "state:1st",
        "state:a%20b",
        "states:x",
        "blocked",
    ] {
        let (status, body) = raw(&board(), &format!("view=waiting&wait={bad}"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {body}");
        assert!(body.contains("or state:STATE"), "{body}");
    }
}
