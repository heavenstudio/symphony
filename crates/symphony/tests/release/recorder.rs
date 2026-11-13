//! Each check of a run is one test operation of the board's recorder (`tasks run`), through the
//! candidate's own `tools/record.sh`: the record a Symphony reads for its Build and Test columns,
//! labelled `release NAME`, with how the check ended.
//!
//! A check inherits the run's environment, so its recorder reaches the fixture's scratch board (its
//! `TASKS_SERVER` and a database of the test's own, never the live board's `tasks`) and takes the
//! scale of its waits named here ([`SCALE`]), and the script waits long for its recorder's probe
//! ([`PROBE_WAIT`]): a loaded machine never leaves a check unrecorded (docs/MACHINE_WORK.md, "A
//! test's waits"). A recorder grown slow is the board's tests' to turn red.

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use super::{Fixture, NOW, common};

/// Names a scale of the recorder's waits, which it takes on a scratch board alone.
const SCALE_VAR: &str = "TASKS_RUN_WAIT_SCALE";
/// The scale, as the board's recorder tests take it: 20 s for the recorder's word that it watches
/// its check and for the board's answer, 12 s for git's.
const SCALE: &str = "20";
/// Names, in seconds, how long `tools/record.sh` waits for its recorder's probe.
const PROBE_WAIT_VAR: &str = "TASKS_RECORDER_PROBE_WAIT";
/// That wait: the backstop of a test's wait, where the script's own is 2 s.
const PROBE_WAIT: &str = "120";
/// How long the recorder took at most to finish an operation after its check ended, when this was
/// a bound on the wall: the allowance of the wait for it ([`test_time::backstop`]).
const RECORDED: Duration = Duration::from_secs(60);

/// The operations the work folder's history holds, once it holds `count`: the recorder writes a
/// finished operation from a process of its own after the check ended, so it is waited for on its
/// condition, with a backstop only a stall reaches. A wait that gives up says what the history and
/// the folder of operations in flight hold: never recorded, or recorded and never finished.
fn history(work: &Path, count: usize) -> Vec<Value> {
    let kept = || -> Vec<Value> {
        fs::read_to_string(work.join("history.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    };
    test_time::until(
        format!("{count} operations kept"),
        test_time::backstop(RECORDED),
        || {
            let kept = kept();
            (kept.len() >= count).then_some(kept)
        },
    )
    .unwrap_or_else(|stall| {
        let mut operations: Vec<String> = fs::read_dir(work.join("operations"))
            .map(|entries| {
                entries
                    .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into()))
                    .collect()
            })
            .unwrap_or_default();
        operations.sort();
        panic!(
            "{stall}: the history keeps {:#?}; in flight: {operations:?}",
            kept()
        )
    })
}

#[test]
fn each_check_is_recorded_by_the_boards_recorder_as_a_test_of_its_own() {
    let fixture = Fixture::new();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/record.sh");
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
            (SCALE_VAR, SCALE),
            (PROBE_WAIT_VAR, PROBE_WAIT),
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
