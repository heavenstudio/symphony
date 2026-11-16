//! What an open task waits on, and the status a blocked one was blocked from, as the task board's
//! listing sends them (`task-waits-on.json`), with the board's orchestrators and the ready tasks
//! that wait to start. Each is optional or defaulted, so a server and a client of either side of
//! the change still read each other.

use serde_json::json;
use symphony_api::{TaskFields, TaskListing, Tasks, WAITS_KINDS, WaitsOn};

#[test]
fn the_listing_with_what_each_task_waits_on_reads_and_writes_back_as_the_same_json() {
    let (written, fixture) = super::round_trip::<TaskListing>("task-waits-on.json");
    assert_eq!(written, fixture);
    let listing: TaskListing = serde_json::from_str(&super::fixture("task-waits-on.json")).unwrap();
    let line = |slug: &str| {
        let task = listing.tasks.iter().find(|t| t.slug == slug).unwrap();
        (task.waits_on.clone(), task.from.clone())
    };
    assert_eq!(
        line("app/on-a-card"),
        (
            Some(WaitsOn {
                kind: "decision".into(),
                who: "task-first-visit-head-first".into(),
                why: "which part of the page comes first".into(),
                since: 1_794_389_700.0,
            }),
            Some("in_progress".into())
        )
    );
    // A block that names nothing is sent in the stage it stopped in, `from` beside it.
    let (stopped, from) = line("app/stopped");
    assert_eq!(stopped.unwrap().why, "no reason recorded");
    assert_eq!(from.as_deref(), Some("review"));
    // A closed task waits on nothing, and says no field for it.
    assert_eq!(line("tools/landed"), (None, None));
    assert!(written["tasks"][4].get("waits_on").is_none());
    assert!(written["tasks"][4].get("from").is_none());
    let health: Vec<(&str, &str)> = (listing.orchestrators.iter())
        .map(|o| (o.handle.as_str(), o.health.as_str()))
        .collect();
    assert_eq!(
        health,
        [("glacial-heron", "left"), ("sable-finch", "stale")]
    );
    assert_eq!(listing.unstarted, 1);
    for task in &listing.tasks {
        if let Some(waits) = &task.waits_on {
            assert!(WAITS_KINDS.contains(&waits.kind.as_str()), "{waits:?}");
        }
    }
}

/// A server before the change sends none of the fields: each reads as none or nothing.
#[test]
fn a_listing_and_a_snapshot_without_the_fields_still_read() {
    let task = json!({"slug": "a", "title": "A", "body": "", "status": "blocked",
        "priority": 2, "parent": null, "created_by": "owner", "created": 1.0, "updated": 2.0,
        "links": [], "depends": [], "roles": [], "instruction": false});
    let listing: TaskListing = serde_json::from_value(json!({"tasks": [task], "next": null}))
        .expect("an older listing reads");
    assert_eq!(listing.tasks[0].waits_on, None);
    assert_eq!(listing.tasks[0].from, None);
    assert_eq!(
        listing.orchestrators,
        Vec::<symphony_api::Orchestrator>::new()
    );
    assert_eq!(listing.unstarted, 0);
    let tasks: Tasks =
        serde_json::from_value(json!({"version": "v", "tasks": [task], "sessions": []}))
            .expect("an older snapshot reads");
    assert_eq!(
        tasks.orchestrators,
        Vec::<symphony_api::Orchestrator>::new()
    );
    assert_eq!(tasks.unstarted, 0);
}

/// The feed sends what a task waits on only when it changes; one of no kind, and a `from` of no
/// status, clears it.
#[test]
fn a_task_change_carries_what_it_waits_on_only_when_that_changes() {
    let bare = serde_json::to_value(TaskFields::default()).unwrap();
    assert_eq!(bare, json!({}));
    let cleared = TaskFields {
        waits_on: Some(WaitsOn::default()),
        from: Some(String::new()),
        ..TaskFields::default()
    };
    let written = serde_json::to_value(&cleared).unwrap();
    assert_eq!(
        written,
        json!({"waits_on": {"kind": "", "who": "", "why": "", "since": 0.0}, "from": ""})
    );
    let read: TaskFields = serde_json::from_value(written).unwrap();
    assert_eq!(read, cleared);
}

/// The kinds a line takes, in the order a list of them keeps, each once.
#[test]
fn the_kinds_are_seven_each_named_once() {
    let mut kinds = WAITS_KINDS.to_vec();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(kinds.len(), WAITS_KINDS.len());
    assert_eq!(
        WAITS_KINDS,
        [
            "decision", "nobody", "check", "task", "working", "queued", "parked"
        ]
    );
}
