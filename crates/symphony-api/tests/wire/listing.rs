//! The one server's project list (`/api/projects`) and the feed region with a project's task asks,
//! as the desktop app reads them. The fields it reads are pinned here, so they can be added to
//! but never renamed.

use serde_json::{Value, json};
use symphony_api::{Change, Delta, ProjectList};

/// Every field the app reads, by its place in `projects.json` written back from its type: a field
/// renamed or moved in the types fails here, whatever the fixture says.
const PINNED: [&str; 16] = [
    "/installation",
    "/pin",
    "/projects/0/id",
    "/projects/0/name",
    "/projects/0/refused",
    "/projects/0/unwell",
    "/projects/0/version",
    "/projects/0/waiting",
    "/projects/0/next/title",
    "/projects/0/next/href",
    "/projects/0/asks/questions/owner",
    "/projects/0/asks/verdicts/owner/id",
    "/projects/0/asks/verdicts/owner/task",
    "/projects/0/asks/verdicts/owner/verdict",
    "/projects/0/asks/verdicts/owner/reviewer",
    "/projects/0/asks/verdicts/owner/at",
];

#[test]
fn the_project_list_reads_and_writes_back_as_the_same_json() {
    let (written, fixture) = super::round_trip::<ProjectList>("projects.json");
    assert_eq!(written, fixture);
}

#[test]
fn every_field_the_app_reads_is_where_it_reads_it_and_says_none_as_null() {
    let (written, _) = super::round_trip::<ProjectList>("projects.json");
    for pointer in PINNED {
        assert!(written.pointer(pointer).is_some(), "{pointer}: {written}");
    }
    assert_eq!(
        written["projects"][0]["asks"]["verdicts"]["owner"]["verdict"],
        "changes"
    );
    // A refused project still names every field, each null, so the app reads none rather than an
    // older server.
    let refused = &written["projects"][2];
    for field in ["unwell", "version", "waiting", "next", "asks"] {
        assert_eq!(refused.get(field), Some(&Value::Null), "{field}: {refused}");
    }
    assert!(refused["refused"].is_string());
}

#[test]
fn a_feed_event_carries_a_projects_task_asks_in_a_region_of_its_own() {
    let event = json!({"version": 9, "changes": [
        {"region": "task-asks", "data": {
            "questions": {"owner": 1},
            "verdicts": {"owner": {"id": 7, "task": "tools/x", "verdict": "approve",
                "reviewer": "slate-wren", "at": 1_763_213_045.25}}
        }},
        {"region": "counts", "data": {"waiting": 1, "docs_waiting": 1, "spot_check": 0,
            "answered_today": 0, "tasks": null}}
    ]});
    let delta: Delta = serde_json::from_value(event.clone()).unwrap();
    let Change::TaskAsks(asks) = &delta.changes[0] else {
        panic!("{:?}", delta.changes[0]);
    };
    assert_eq!(asks.questions["owner"], 1);
    assert_eq!(asks.verdicts["owner"].task, "tools/x");
    assert!(matches!(&delta.changes[1], Change::Counts(counts) if counts.waiting == 1));
    assert_eq!(serde_json::to_value(&delta).unwrap(), event);
    let bare = json!({"version": 9, "changes": [{"region": "task-asks"}]});
    let err = serde_json::from_value::<Delta>(bare)
        .unwrap_err()
        .to_string();
    assert!(err.contains("the task-asks region has no data"), "{err}");
}
