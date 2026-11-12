use super::*;
use serde_json::json;
use std::collections::BTreeMap;
use symphony_api::{MachineWork, TaskListing, Tasks};

fn fixture() -> (Tasks, MachineWork) {
    let mut records = Vec::new();
    let mut operations = Vec::new();
    for (status, kind, count) in [
        ("in_progress", "build", 37),
        ("in_progress", "test", 3),
        ("in_progress", "command", 2),
        ("review", "build", 1),
        ("done", "test", 1),
    ] {
        for index in 0..count {
            let slug = format!("{status}-{kind}-{index:02}");
            records.push(
                serde_json::from_value(json!({"slug":slug,"title":slug,"body":"brief",
                "status":status,"priority":1,"parent":null,"created_by":"owner","created":0.0,
                "updated":0.0,"links":[],"depends":[],"roles":[],"instruction":false}))
                .unwrap(),
            );
            operations.push(
                json!({"machine_id":"alpha","operation_id":slug,"parent_id":null,
                "handle":"worker","task":slug,"checkout":"opaque","recipe":"fixture",
                "kind":kind,"state":"running","resource":"native","reason":"","phase_since":null}),
            );
        }
    }
    let work = serde_json::from_value(json!({"freshness":"fresh","machines":[{
        "machine_id":"alpha","label":"Alpha","producer_id":"fixture","sequence":1,
        "age_ms":0,"freshness":"fresh","complete":true,"reason":"","counts":null,
        "operations":operations}],"unassigned":[]}))
    .unwrap();
    (
        Tasks {
            version: String::new(),
            tasks: records,
            sessions: vec![],
        },
        work,
    )
}

async fn page(query: &str, search: &str, scores: &BTreeMap<String, f64>) -> TaskListing {
    let (tasks, work) = fixture();
    let request = Request::builder()
        .uri(format!("/api/task-board?{query}"))
        .body(())
        .unwrap();
    let response = project(
        &request,
        tasks,
        search,
        scores,
        work,
        vec!["review-build-00".into()],
    );
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn total(page: &TaskListing, stage: &str) -> usize {
    page.counts
        .iter()
        .find(|(s, _)| s == stage)
        .map_or(0, |(_, n)| *n)
}

#[tokio::test]
async fn build_pages_count_the_complete_window_before_paging_and_keep_lifecycle_unchanged() {
    let scores = BTreeMap::new();
    let first = page("status=build&activity=1", "", &scores).await;
    assert_eq!(first.tasks.len(), 30);
    assert_eq!(total(&first, "build"), 37);
    assert_eq!(total(&first, "test"), 3);
    assert_eq!(total(&first, "in_progress"), 2);
    assert_eq!(total(&first, "review"), 1);
    assert_eq!(
        total(&first, "done"),
        0,
        "old finished task is outside the window"
    );
    assert!(
        first
            .tasks
            .iter()
            .all(|task| task.status == "in_progress" && task.body.is_empty())
    );
    assert_eq!(
        first
            .lifecycle_counts
            .iter()
            .find(|(s, _)| s == "in_progress")
            .unwrap()
            .1,
        42
    );
    let cursor: String = first
        .next
        .as_ref()
        .unwrap()
        .bytes()
        .map(|byte| format!("%{byte:02X}"))
        .collect();
    let next = page(
        &format!("status=build&activity=1&cursor={cursor}"),
        "",
        &scores,
    )
    .await;
    assert_eq!(next.tasks.len(), 7);
    assert_eq!(total(&next, "build"), 37);
    assert_eq!(next.tasks[0].slug, "in_progress-build-30");
    let counts = page("counts=1&activity=1", "", &scores).await;
    assert!(counts.tasks.is_empty());
    assert_eq!(total(&counts, "build"), 37);
    let offline = page("status=in_progress&activity=0&all=1", "", &scores).await;
    assert_eq!(offline.tasks.len(), 42);
    assert_eq!(total(&offline, "build"), 0);
    assert_eq!(total(&offline, "in_progress"), 42);
    let legacy = page("status=in_progress&all=1", "", &scores).await;
    assert_eq!(legacy.tasks.len(), 42);
    assert_eq!(total(&legacy, "in_progress"), 42);
    assert!(!legacy.statuses.contains(&"build".to_string()));
    assert_eq!(
        page("status=done&older=1", "", &scores).await.tasks.len(),
        1
    );
    let signoff = page("status=signoff", "", &scores).await;
    assert_eq!(signoff.tasks[0].slug, "review-build-00");
    assert_eq!(signoff.tasks[0].status, "review");
}

#[tokio::test]
async fn search_filter_totals_are_full_and_stable_across_build_pages() {
    let scores: BTreeMap<_, _> = (0..35)
        .map(|index| (format!("in_progress-build-{index:02}"), 1.0))
        .collect();
    let first = page("status=build&activity=1", "build", &scores).await;
    assert_eq!(total(&first, "build"), 35);
    assert_eq!(total(&first, "test"), 0);
    assert!(first.signoff.is_empty());
    let cursor: String = first
        .next
        .as_ref()
        .unwrap()
        .bytes()
        .map(|byte| format!("%{byte:02X}"))
        .collect();
    let next = page(
        &format!("status=build&activity=1&cursor={cursor}"),
        "build",
        &scores,
    )
    .await;
    assert_eq!(next.tasks.len(), 5);
    assert_eq!(next.tasks[0].slug, "in_progress-build-30");
    assert_eq!(total(&next, "build"), 35);
}

#[tokio::test]
async fn finished_window_is_applied_before_stage_counts_including_searches() {
    let (mut tasks, work) = fixture();
    let finished = tasks
        .tasks
        .iter_mut()
        .find(|task| task.status == "done")
        .unwrap();
    finished.updated = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        - 2.0 * 86_400.0;
    let scores = BTreeMap::from([(finished.slug.clone(), 1.0)]);
    for (days, expected) in [("1", 0), ("7", 1), ("invalid", 0)] {
        let request = Request::builder()
            .uri(format!("/api/task-board?counts=1&activity=1&days={days}"))
            .body(())
            .unwrap();
        let response = project(
            &request,
            tasks.clone(),
            "done",
            &scores,
            work.clone(),
            vec![],
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let listing: TaskListing = serde_json::from_slice(&bytes).unwrap();
        assert!(listing.tasks.is_empty());
        assert_eq!(total(&listing, "done"), expected, "days={days}");
        assert_eq!(total(&listing, "build"), 0, "search must precede counting");
    }
}

/// `work=0` leaves the machine's work out of a listing, which a page then reads from
/// `/api/machine-work`, and nothing else: the stages and counts still place each task by it, so a
/// listing's validator moves with its tasks and not with a recorder's heartbeat.
#[tokio::test]
async fn a_listing_without_work_places_tasks_by_it_and_leaves_it_out() {
    let scores = BTreeMap::new();
    for query in [
        "counts=1&activity=1",
        "status=build&activity=1",
        "status=review",
    ] {
        let with = page(query, "", &scores).await;
        let without = page(&format!("{query}&work=0"), "", &scores).await;
        assert!(!with.machine_work.machines.is_empty(), "{query}");
        assert_eq!(without.machine_work, MachineWork::default(), "{query}");
        assert_eq!(
            TaskListing {
                machine_work: MachineWork::default(),
                ..with
            },
            without,
            "{query}"
        );
    }
    // Only `work=0` leaves it out.
    for query in ["work=1", "work=", "work=false"] {
        let listing = page(&format!("counts=1&activity=1&{query}"), "", &scores).await;
        assert!(!listing.machine_work.machines.is_empty(), "{query}");
    }
    let built = page("counts=1&activity=1&work=0", "", &scores).await;
    assert_eq!(total(&built, "build"), 37);
}
