//! Each check of a run is one test operation of the board's recorder (`tasks run`), through the
//! candidate's own `tools/record.sh`: the record a Symphony reads for its Build and Test columns,
//! labelled `release NAME`, with how the check ended.
//!
//! A check inherits the run's environment, so its recorder reaches the fixture's scratch board (its
//! `TASKS_SERVER` and a database of the test's own, never the live board's `tasks`) and takes the
//! scale of its waits named here ([`SCALE`]), and the script waits long for its recorder's probe
//! ([`PROBE_WAIT`]): a loaded machine never leaves a check unrecorded (docs/workflow/MACHINE_WORK.md, "A
//! test's waits"). A recorder grown slow is the board's tests' to turn red.
//!
//! A recorder writes its record from a process of its own, after its check ended, for as long as
//! the board takes to say which task the check was for. The run looks at its checkout after each
//! check and stops on a process working there, and never on that recorder, which works at the
//! root: each test first holds the run to having run every check, in the run's own words, so a
//! run that stopped fails by what it said and not by a wait for a record no check would make.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
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

/// A run of the two checks `verify` (green) and `lint` (exit 4) in `fixture`, recorded through the
/// candidate's own copy of this repository's `tools/record.sh` by `recorder` into the work folder
/// it returns. The run ran both to their end and is red on `lint` alone: it stopped on nothing.
fn checked(fixture: &Fixture, recorder: &str) -> std::path::PathBuf {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/record.sh");
    fixture.write("tools/record.sh", &fs::read_to_string(script).unwrap());
    fixture.land("notes.txt", Some("with the recorder"), "the record script");
    let work = fixture.root.join("work");
    let work_folder = work.to_string_lossy().into_owned();
    let (code, out, err) = fixture.release_with(
        NOW,
        &["--check", "verify=true", "--check", "lint=exit 4"],
        &[
            ("TASKS_RECORDER", recorder),
            ("TASKS_WORK", work_folder.as_str()),
            (SCALE_VAR, SCALE),
            (PROBE_WAIT_VAR, PROBE_WAIT),
        ],
    );
    assert_eq!(code, 1, "{out}{err}");
    assert!(out.contains("release: verify green (exit 0"), "{out}{err}");
    assert!(out.contains("release: lint red (exit 4"), "{out}{err}");
    assert!(!err.contains("is in use by"), "{out}{err}");
    work
}

/// What `kept` holds of each operation, sorted: its label, its phase and its exit code.
fn seen(kept: &[Value]) -> Vec<(String, String, Option<i64>)> {
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
    seen
}

/// Each check as it should be kept: a test, labelled `release NAME`, with its exit code.
fn both() -> [(String, String, Option<i64>); 2] {
    [
        ("release lint".to_string(), "test".to_string(), Some(4)),
        ("release verify".to_string(), "test".to_string(), Some(0)),
    ]
}

#[test]
fn each_check_is_recorded_by_the_boards_recorder_as_a_test_of_its_own() {
    let fixture = Fixture::new();
    let work = checked(&fixture, common::tasks_binary());
    let kept = history(&work, 2);
    assert_eq!(seen(&kept), both(), "{kept:#?}");
    for op in &kept {
        assert_eq!(
            op["checkout"].as_str().unwrap(),
            fixture.worktree().to_string_lossy(),
            "recorded in the run's own checkout: {op}"
        );
        assert!(op["task"].is_null(), "a run is no task's: {op}");
    }
}

/// A board's PostgreSQL that never answers: a socket where a board's server keeps its own, under
/// `/tmp` ([`symphony_board::server::postgres::socket_folder`]), that takes a connection and says
/// nothing on it. Its folder is removed with it.
struct Silent {
    folder: PathBuf,
    _socket: UnixListener,
}

impl Silent {
    /// The silent board of a server on the data `data` would serve.
    fn at(data: &Path) -> Silent {
        let folder = symphony_board::server::postgres::socket_folder(data);
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let socket = folder.join(symphony_board::server::postgres::SOCKET_FILE);
        Silent {
            _socket: UnixListener::bind(&socket).unwrap(),
            folder,
        }
    }
}

impl Drop for Silent {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.folder);
    }
}

/// A recorder still asking a board that never answers which task its check was for, for longer
/// than the run gives a process to leave its checkout (5 s; the wait is 20 s at [`SCALE`]), stops
/// no run: the next check runs, and both are kept once their recorders gave the board up.
#[test]
fn a_recorder_that_outlives_its_check_never_holds_the_runs_checkout() {
    let fixture = Fixture::new();
    // Takes a connection and says nothing on it: the board's answer never comes.
    let silent = Silent::at(&fixture.root.join("silent"));
    let recorder = fixture.root.join("recorder-of-a-silent-board");
    fs::write(
        &recorder,
        format!(
            "#!/bin/sh\n[ \"$2\" = --probe ] || export TASKS_SERVER=postgresql:{}\nexec '{}' \"$@\"\n",
            silent.folder.display(),
            common::tasks_binary()
        ),
    )
    .unwrap();
    fs::set_permissions(&recorder, fs::Permissions::from_mode(0o755)).unwrap();
    let work = checked(&fixture, &recorder.to_string_lossy());
    let kept = history(&work, 2);
    assert_eq!(seen(&kept), both(), "{kept:#?}");
}
