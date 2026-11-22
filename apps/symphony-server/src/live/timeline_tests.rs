//! `/api/task-timeline` over a scratch board, a temporary repository and its recorder's work folder:
//! the approved pictures' task (two review rounds, a build, its tests, one running now, a gate met
//! then missing again) gives the timeline the pictures draw, the fixture the client reads; a task
//! with no run kept has no runs.

use super::Hub;
use super::read_paths_tests::{observe, served_with};
use crate::shelf;
use crate::testing::{
    Tree,
    board::{Fixture, NOW, Seed},
    git, repository, store_on,
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::path::Path;
use symphony_board::cli::machine_work::recorded::{self, Phase, Record, Step};
use symphony_board::rows::MessageRow;

const SLUG: &str = "tools/verify-stops-when-moved";
/// The pictures' now, 15:20 on their clock; the board's clock reads `NOW`, so every time the
/// fixture names is moved by the difference.
const PICTURES_NOW: f64 = 1_794_403_200.0;
const SHIFT: f64 = NOW - PICTURES_NOW;

/// `time` on the pictures' clock, on the board's.
fn shifted(time: f64) -> f64 {
    time + SHIFT
}

/// The fixture the client reads, its times on the board's clock and its zone `zone`.
fn expected(zone: &Value) -> Value {
    fn shift(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                for (key, field) in fields.iter_mut() {
                    let time = [
                        "now",
                        "created",
                        "closed",
                        "first_claimed",
                        "from",
                        "to",
                        "taken",
                        "queued",
                        "started",
                        "ended",
                    ]
                    .contains(&key.as_str());
                    match field.as_f64() {
                        Some(at) if time => *field = json!(shifted(at)),
                        _ => shift(field),
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(shift),
            _ => {}
        }
    }
    let mut fixture: Value = serde_json::from_str(include_str!(
        "../../../../crates/symphony-api/fixtures/task-timeline.json"
    ))
    .unwrap();
    shift(&mut fixture);
    fixture["zone"] = zone.clone();
    fixture
}

/// The pictures' thread, as the board keeps it.
fn thread() -> Vec<MessageRow> {
    let line = |id: i64, kind: &str, sender: &str, ts: f64, text: &str| MessageRow {
        id,
        slug: SLUG.into(),
        sender: sender.into(),
        recipient: None,
        kind: kind.into(),
        text: text.into(),
        ts: shifted(ts),
    };
    vec![
        line(
            1,
            "system",
            "glacial-merlin",
            1_794_380_520.0,
            "created by glacial-merlin",
        ),
        line(
            2,
            "system",
            "quiet-wren",
            1_794_382_980.0,
            "quiet-wren claimed it",
        ),
        line(
            3,
            "system",
            "quiet-wren",
            1_794_389_700.0,
            "quiet-wren moved it in_progress → review",
        ),
        line(
            4,
            "system",
            "lucid-heron",
            1_794_390_780.0,
            "lucid-heron joined as reviewer",
        ),
        line(
            5,
            "verdict",
            "lucid-heron",
            1_794_392_820.0,
            "changes: the verify keeps running after the branch moves",
        ),
        line(
            6,
            "system",
            "lucid-heron",
            1_794_392_880.0,
            "lucid-heron released reviewer (released)",
        ),
        line(
            7,
            "system",
            "quiet-wren",
            1_794_397_020.0,
            "quiet-wren moved it in_progress → review",
        ),
        line(
            8,
            "system",
            "lucid-heron",
            1_794_397_260.0,
            "lucid-heron joined as reviewer",
        ),
    ]
}

/// The run `n` the recorder kept for `task`, at `commit`, by `by`.
fn run(
    n: u32,
    task: &str,
    (phase, label): (Phase, &str),
    (commit, by): (&str, Option<&str>),
    (queued, started, ended): (f64, f64, Option<f64>),
) -> Record {
    Record {
        version: 1,
        operation_id: format!("{n:032x}"),
        parent_id: None,
        phase,
        label: label.into(),
        state: if ended.is_some() {
            Step::Finished
        } else {
            Step::Running
        },
        handle: by.map(str::to_string),
        task: Some(task.into()),
        checkout: "/work".into(),
        branch: Some(task.into()),
        commit: Some(commit.into()),
        machine_id: None,
        host: "studio".into(),
        pid: 10,
        child_pid: Some(11),
        queued_at: shifted(queued),
        started_at: Some(shifted(started)),
        ended_at: ended.map(shifted),
        beat_at: shifted(ended.unwrap_or(started)),
        exit_code: ended.map(|_| 0),
        signal: None,
        reason: String::new(),
        run: None,
        reading: None,
    }
}

/// The pictures' gates as the project's plugin answers them, on the board's clock.
fn gates() -> Value {
    let at = |time: f64| json!(shifted(time));
    json!({SLUG: {"gates": [
        {"name": "certificate", "state": "missing", "by": "studio", "at": at(1_794_401_400.0),
         "commit": "4be91c2", "detail": "Main moved: verify on the merge tree 4be91c2 running",
         "past": [
            {"state": "missing", "at": at(1_794_398_700.0), "by": "studio",
             "detail": "Verify on the merge tree e19d3a0 running"},
            {"state": "met", "at": at(1_794_400_440.0), "by": "studio",
             "detail": "Verify on the merge tree e19d3a0 green"}]},
        {"name": "dependency", "state": "waived", "by": "cobalt-otter", "at": at(1_794_399_120.0),
         "why": "tools/slot-pool-by-tree shares no code with this branch",
         "detail": "Waits on tools/slot-pool-by-tree, waived",
         "past": [{"state": "missing", "at": at(1_794_370_000.0),
                   "detail": "Waits on tools/slot-pool-by-tree"}]},
        {"name": "nightly", "state": "met", "by": "nightly", "at": at(1_794_363_240.0),
         "commit": "a71f0d3", "detail": "Night of 11-10 green on main a71f0d3"},
        {"name": "review", "state": "met", "by": "lucid-heron", "at": at(1_794_398_700.0),
         "detail": "Approved in round 2 of 2",
         "past": [{"state": "missing", "at": at(1_794_392_820.0), "by": "lucid-heron",
                   "detail": "Changes asked in round 1: the verify keeps running after the branch moves"}]},
    ]}})
}

/// A hub over `root` and `f`'s board.
fn hub(root: &Path, f: &Fixture) -> Hub {
    let mut hub = Hub::new(root, &root.join("build/client"));
    let board = f.other_client();
    hub.connect_reader = Box::new(move |_| Ok(store_on(&board)));
    hub.refresh_page("", root).unwrap();
    observe(&hub, &f.store, &[]);
    hub
}

#[test]
fn the_pictures_task_gives_the_timeline_they_draw_and_a_task_with_no_runs_has_none() {
    let tree = Tree::new();
    shelf::install().unwrap();
    let root = repository(&tree);
    std::fs::write(root.join(symphony::project::FILE), shelf::MANIFEST).unwrap();
    git(&root, &["branch", SLUG]);
    std::fs::write(root.join(shelf::GATES), gates().to_string()).unwrap();

    // The recorder's work folder in the repository's common dir: the finished runs in its history,
    // the one running now in its operations, its lease held as its recorder holds it.
    let work = recorded::in_common(&root.join(".git"));
    let wren = Some("quiet-wren");
    let finished = [
        run(
            1,
            SLUG,
            (Phase::Build, "just lint-rs"),
            ("9ac5149", wren),
            (1_794_385_680.0, 1_794_385_860.0, Some(1_794_386_100.0)),
        ),
        run(
            2,
            SLUG,
            (Phase::Test, "just verify"),
            ("9ac5149", wren),
            (1_794_387_120.0, 1_794_387_480.0, Some(1_794_388_920.0)),
        ),
        run(
            3,
            SLUG,
            (Phase::Test, "just verify"),
            ("c3d2e1f", wren),
            (1_794_394_800.0, 1_794_394_920.0, Some(1_794_396_300.0)),
        ),
        run(
            4,
            SLUG,
            (Phase::Test, "certificate verify"),
            ("e19d3a0", None),
            (1_794_398_700.0, 1_794_399_000.0, Some(1_794_400_440.0)),
        ),
        // Another task's run says nothing of this one.
        run(
            6,
            "tools/other",
            (Phase::Test, "just verify"),
            ("9ac5149", wren),
            (1_794_387_000.0, 1_794_387_100.0, Some(1_794_387_200.0)),
        ),
    ];
    for record in &finished {
        recorded::keep(&work, record).unwrap();
    }
    let running = run(
        5,
        SLUG,
        (Phase::Test, "certificate verify"),
        ("4be91c2", None),
        (1_794_401_520.0, 1_794_402_120.0, None),
    );
    let operations = work.join(recorded::OPERATIONS);
    std::fs::create_dir_all(&operations).unwrap();
    let path = operations.join(format!("{}.json", running.operation_id));
    recorded::write(&path, &running).unwrap();
    let lease = std::fs::File::create(path.with_extension("lease")).unwrap();
    lease.lock().unwrap();

    let mut seed = Seed::default()
        .task(SLUG, "review")
        .task("tools/quiet", "ready")
        .role(SLUG, "quiet-wren", "executor")
        .role(SLUG, "lucid-heron", "reviewer");
    for task in &mut seed.dump.tasks {
        if task.slug == SLUG {
            task.created_by = "glacial-merlin".into();
            task.created = shifted(1_794_380_520.0);
            task.updated = shifted(1_794_397_260.0);
        }
    }
    seed.dump.messages.extend(thread());
    seed.dump.messages.extend((1000..1260).map(|id| MessageRow {
        id,
        slug: "tools/quiet".into(),
        sender: "outside".into(),
        recipient: Some("@orchestrator".into()),
        kind: "msg".into(),
        text: format!("Full text of message {id}"),
        ts: NOW,
    }));
    let f = Fixture::new(seed);
    let (shared, send) = served_with(&root, hub(&root, &f));
    let ask = |path: &str| {
        let (status, _, body) = send(path, &[]);
        (status, serde_json::from_slice::<Value>(&body).unwrap())
    };

    let path = format!("/api/task-timeline?slug={SLUG}");
    let (status, headers, body) = send(&path, &[]);
    let answer: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status, StatusCode::OK, "{answer}");
    // Tagged by what it says less the server's clock, so the minute's read is a 304 while nothing
    // happened, its body not sent again.
    let mut clockless = answer.clone();
    clockless["now"] = Value::Null;
    let digest = <sha2::Sha256 as sha2::Digest>::digest(serde_json::to_vec(&clockless).unwrap());
    let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    let etag = headers[axum::http::header::ETAG]
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(etag, format!("W/\"{hex}\""));
    let (status, _, body) = send(&path, &[("If-None-Match", &etag)]);
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert!(body.is_empty());
    // This machine's clock: its offset at the task's creation, then at each change since, whole
    // minutes each.
    let zone = answer["zone"].clone();
    assert_eq!(zone[0]["from"], json!(shifted(1_794_380_520.0)), "{zone}");
    assert!(
        zone.as_array()
            .unwrap()
            .iter()
            .all(|change| change["seconds"]
                .as_i64()
                .is_some_and(|offset| offset % 60 == 0)),
        "{zone}"
    );
    // The task is open and never assessed: the stamps the Symphony's catalogue leaves out by
    // default have no lane, as the pictures have none.
    let history: symphony_api::timeline::History =
        serde_json::from_value(answer["history"].clone()).unwrap();
    assert!(history.sources_complete);
    assert_eq!(
        history.sources,
        vec![symphony_api::timeline::HistorySource {
            slug: SLUG.into(),
            state: symphony_api::timeline::HistoryState::Complete,
            first_id: Some(1),
            last_id: Some(8),
            before: None,
        }]
    );
    assert_eq!(history.messages.len(), 1);
    assert_eq!(
        (history.messages[0].id, history.messages[0].at),
        (5, shifted(1_794_392_820.0))
    );
    assert_eq!(
        history.messages[0].from.state,
        symphony_api::timeline::EndpointState::Known
    );
    assert_eq!(
        history.messages[0].to.state,
        symphony_api::timeline::EndpointState::Everyone
    );
    assert_eq!(
        history
            .roles
            .iter()
            .map(|span| (span.role.as_str(), span.from_id, span.to_id))
            .collect::<Vec<_>>(),
        [
            ("executor", 2, None),
            ("reviewer", 4, Some(6)),
            ("reviewer", 8, None)
        ]
    );
    let mut legacy = answer.clone();
    legacy.as_object_mut().unwrap().remove("history");
    assert_eq!(legacy, expected(&zone));
    assert!(answer.get("runs_from").is_none(), "nothing rotated out");

    // No run kept for it: no runs at all, not runs of nothing. Never in review, it has no review
    // lane yet: the Symphony's own review gate, its only stamp needed, starts at the move to review.
    let (status, answer) = ask("/api/task-timeline?slug=tools/quiet");
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["runs"], json!([]));
    assert_eq!(answer["rounds"], json!([]));
    assert_eq!(
        answer["stages"],
        json!([{"status": "ready", "from": NOW}]),
        "made ready at the board's clock and waiting since"
    );
    assert_eq!(answer["gates"], json!([]));
    let history: symphony_api::timeline::History =
        serde_json::from_value(answer["history"].clone()).unwrap();
    assert_eq!(
        history.sources[0].state,
        symphony_api::timeline::HistoryState::Partial
    );
    assert_eq!(history.sources[0].before, Some(1004));
    assert_eq!(
        history.messages.len(),
        symphony_api::timeline::TIMELINE_HISTORY_LIMIT
    );
    assert!(
        history
            .messages
            .iter()
            .all(|message| message.to.state == symphony_api::timeline::EndpointState::Unknown)
    );
    let (status, older) = ask(&format!(
        "/api/task-history?slug=tools/quiet&before={}",
        history.sources[0].before.unwrap()
    ));
    assert_eq!(status, StatusCode::OK);
    let older: symphony_api::TaskHistory = serde_json::from_value(older).unwrap();
    assert_eq!(
        older
            .entries
            .iter()
            .map(|entry| entry.message.id)
            .collect::<Vec<_>>(),
        [1000, 1001, 1002, 1003]
    );
    assert_eq!(older.entries[0].message.text, "Full text of message 1000");
    assert_eq!(older.before, None);
    assert_eq!(older.after, None);
    // Its gates list the stamps the timeline leaves out, waived.
    let (status, answer) = ask("/api/task-gates?slug=tools/quiet");
    assert_eq!(status, StatusCode::OK, "{answer}");
    let waived: Vec<&str> = (answer["gates"].as_array().unwrap().iter())
        .filter(|gate| gate["state"] == "waived")
        .map(|gate| gate["name"].as_str().unwrap())
        .collect();
    assert_eq!(waived, ["tests", "pictures", "owner"], "{answer}");

    let (status, answer) = ask("/api/task-timeline?slug=tools/missing");
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    let (status, answer) = ask("/api/task-timeline");
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert_eq!(answer["error"], "slug is required");
    drop(lease);
    shared.stop();
}
