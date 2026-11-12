//! `/api/task-timeline` over a scratch board, a temporary repository and its recorder's work folder:
//! the approved pictures' task (two review rounds, a build, its tests, one running now, a gate met
//! then missing again) gives the timeline the pictures draw, the fixture the client reads; a task
//! with no run kept has no runs.

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

/// The fixture the client reads, its times on the board's clock and its offset `offset`.
fn expected(offset: &Value) -> Value {
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
    fixture["offset"] = offset.clone();
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
    let f = Fixture::new(seed);
    let (shared, ask) = served(&root, hub(&root, &f));

    let (status, answer) = ask(&format!("/api/task-timeline?slug={SLUG}"));
    assert_eq!(status, StatusCode::OK, "{answer}");
    let offset = answer["offset"].clone();
    assert!(
        offset.as_i64().is_some_and(|offset| offset % 60 == 0),
        "{offset}"
    );
    assert_eq!(answer, expected(&offset));

    // No run kept for it: no runs at all, not runs of nothing. The Symphony's own review gate is
    // its one lane, as the gates answer has it.
    let (status, answer) = ask("/api/task-timeline?slug=tools/quiet");
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["runs"], json!([]));
    assert_eq!(answer["rounds"], json!([]));
    assert_eq!(
        answer["stages"],
        json!([{"status": "ready", "from": NOW}]),
        "made ready at the board's clock and waiting since"
    );
    assert_eq!(
        answer["gates"],
        json!([{"name": "review", "states": [
            {"state": "missing", "from": NOW, "detail": "Not reviewed yet"}]}])
    );

    let (status, answer) = ask("/api/task-timeline?slug=tools/missing");
    assert_eq!(status, StatusCode::NOT_FOUND, "{answer}");
    let (status, answer) = ask("/api/task-timeline");
    assert_eq!(status, StatusCode::BAD_REQUEST, "{answer}");
    assert_eq!(answer["error"], "slug is required");
    drop(lease);
    shared.stop();
}
