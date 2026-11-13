//! Each check of a run is one test operation of the board's recorder (`tasks run`), through the
//! candidate's own `tools/record.sh`: the record a Symphony reads for its Build and Test columns,
//! labelled `release NAME`, with how the check ended.

use std::fs;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{Fixture, NOW, common};

/// The operations the work folder's history holds, once it holds `count`, within a minute: the
/// recorder writes a finished operation from a process of its own after the check ended.
fn history(work: &std::path::Path, count: usize) -> Vec<Value> {
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        let kept: Vec<Value> = fs::read_to_string(work.join("history.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        if kept.len() >= count || Instant::now() > until {
            return kept;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn each_check_is_recorded_by_the_boards_recorder_as_a_test_of_its_own() {
    let fixture = Fixture::new();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/record.sh");
    fixture.write("tools/record.sh", &fs::read_to_string(script).unwrap());
    fixture.land("notes.txt", Some("with the recorder"), "the record script");
    let work = fixture.root.join("work");
    let recorder = common::tasks_binary().to_string();
    let work_folder = work.to_string_lossy().into_owned();
    let (code, out, err) = fixture.release_with(
        NOW,
        &["--check", "verify=true", "--check", "lint=exit 4"],
        &[
            ("TASKS_RECORDER", recorder.as_str()),
            ("TASKS_WORK", work_folder.as_str()),
        ],
    );
    assert_eq!(code, 1, "{out}{err}");
    let kept = history(&work, 2);
    let mut seen: Vec<(String, String, Option<i64>)> = kept
        .iter()
        .map(|op| {
            (
                op["label"].as_str().unwrap_or("").to_string(),
                op["phase"].as_str().unwrap_or("").to_string(),
                op["exit_code"].as_i64(),
            )
        })
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        [
            ("release lint".to_string(), "test".to_string(), Some(4)),
            ("release verify".to_string(), "test".to_string(), Some(0)),
        ],
        "{kept:#?}"
    );
    for op in &kept {
        assert_eq!(
            op["checkout"].as_str().unwrap(),
            fixture.worktree().to_string_lossy(),
            "recorded in the run's own checkout: {op}"
        );
        assert!(op["task"].is_null(), "a run is no task's: {op}");
    }
}
