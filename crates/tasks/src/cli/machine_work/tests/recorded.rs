//! The recorder's operations as a reader finds them, the history it keeps, and how a sweep finishes
//! an operation whose recorder is gone.

use super::*;
use crate::cli::machine_work::native::Inventory;
use crate::cli::machine_work::recorded::{
    self, Found, HISTORY, KEEP, OPERATIONS, Phase, RETAIN, Record, Step,
};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const ONE: &str = "11111111111111111111111111111111";
const TWO: &str = "22222222222222222222222222222222";
const THREE: &str = "33333333333333333333333333333333";

/// A folder of this test's own, emptied first.
fn folder(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "sf-recorded-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(OPERATIONS)).unwrap();
    dir
}

fn record(id: &str, state: Step) -> Record {
    Record {
        version: 1,
        operation_id: id.into(),
        parent_id: None,
        phase: Phase::Test,
        label: "cargo test --locked".into(),
        state,
        handle: Some("worker".into()),
        task: Some("tools/x".into()),
        checkout: "/same/path".into(),
        branch: Some("tools/x".into()),
        commit: None,
        machine_id: None,
        host: "this-host".into(),
        pid: 123,
        child_pid: (state != Step::Queued).then_some(124),
        queued_at: 1000.0,
        started_at: (state != Step::Queued).then_some(1001.0),
        ended_at: (state == Step::Finished).then_some(1031.0),
        beat_at: 1003.0,
        exit_code: (state == Step::Finished).then_some(0),
        signal: None,
        reason: String::new(),
    }
}

fn path(folder: &Path, id: &str) -> PathBuf {
    folder.join(OPERATIONS).join(format!("{id}.json"))
}

/// `record` written in `folder`, its lease held by the file returned.
fn running(folder: &Path, record: &Record) -> File {
    let at = path(folder, &record.operation_id);
    let lease = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(at.with_extension("lease"))
        .unwrap();
    lease.lock().unwrap();
    recorded::write(&at, record).unwrap();
    lease
}

/// A recorder gone: its lock let go, as its process ending lets it go.
fn gone(lease: File) {
    lease.unlock().unwrap();
}

fn ids(found: &Found) -> Vec<(&str, Step)> {
    found
        .records
        .iter()
        .map(|(record, _)| (record.operation_id.as_str(), record.state))
        .collect()
}

#[test]
fn a_folder_not_made_yet_holds_nothing_for_certain_and_an_unreadable_one_is_uncertain() {
    let dir = folder("absent");
    let found = recorded::read(&dir.join("never-made"));
    assert!(found.is_empty(), "{found:?}");
    fs::remove_dir_all(dir.join(OPERATIONS)).unwrap();
    fs::write(dir.join(OPERATIONS), "not a folder").unwrap();
    let found = recorded::read(&dir);
    assert!(found.records.is_empty());
    assert!(found.uncertain);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn work_whose_recorder_holds_its_lease_is_read_and_work_whose_recorder_is_gone_is_uncertainty() {
    let dir = folder("leases");
    let _held = running(&dir, &record(ONE, Step::Running));
    let queued = running(&dir, &record(TWO, Step::Queued));
    recorded::write(&path(&dir, THREE), &record(THREE, Step::Finished)).unwrap();
    let found = recorded::read(&dir);
    assert_eq!(
        ids(&found),
        [
            (ONE, Step::Running),
            (TWO, Step::Queued),
            (THREE, Step::Finished)
        ]
    );
    assert!(!found.uncertain);
    // Its recorder ended without finishing it: neither work nor done.
    gone(queued);
    let found = recorded::read(&dir);
    assert_eq!(ids(&found), [(ONE, Step::Running), (THREE, Step::Finished)]);
    assert!(found.uncertain);
    // Nor is one whose lease is gone.
    fs::remove_file(path(&dir, TWO).with_extension("lease")).unwrap();
    assert!(recorded::read(&dir).uncertain);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_record_that_is_not_one_a_recorder_writes_is_uncertainty_and_a_partial_write_is_passed_over() {
    let dir = folder("invalid");
    let _held = running(&dir, &record(ONE, Step::Running));
    let mut wrong = Vec::new();
    // Named for another operation; finished without an end; running before it started; both an
    // exit and a signal; another version; a handle no session could have.
    let mut renamed = record(TWO, Step::Finished);
    renamed.operation_id = THREE.into();
    wrong.push(renamed);
    let mut unended = record(TWO, Step::Finished);
    unended.ended_at = None;
    wrong.push(unended);
    let mut unstarted = record(TWO, Step::Running);
    unstarted.started_at = None;
    wrong.push(unstarted);
    let mut both = record(TWO, Step::Finished);
    both.signal = Some(9);
    wrong.push(both);
    let mut version = record(TWO, Step::Finished);
    version.version = 2;
    wrong.push(version);
    let mut handle = record(TWO, Step::Finished);
    handle.handle = Some("line\nbreak".into());
    wrong.push(handle);
    for bad in wrong {
        recorded::write(&path(&dir, TWO), &bad).unwrap();
        let found = recorded::read(&dir);
        assert_eq!(ids(&found), [(ONE, Step::Running)], "{bad:?}");
        assert!(found.uncertain, "{bad:?}");
    }
    fs::write(path(&dir, TWO), "{").unwrap();
    assert!(recorded::read(&dir).uncertain);
    fs::remove_file(path(&dir, TWO)).unwrap();
    // A write's own file before its rename is no record.
    fs::write(dir.join(OPERATIONS).join(format!(".{TWO}.json.1.0")), "{").unwrap();
    let found = recorded::read(&dir);
    assert_eq!(ids(&found), [(ONE, Step::Running)]);
    assert!(!found.uncertain);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn recorded_operations_keep_the_task_their_recorder_bound_and_their_phase_and_label() {
    let dir = folder("inventory");
    let _held = running(&dir, &record(ONE, Step::Running));
    let mut untasked = record(TWO, Step::Queued);
    untasked.task = None;
    untasked.phase = Phase::Build;
    let _queued = running(&dir, &untasked);
    recorded::write(&path(&dir, THREE), &record(THREE, Step::Finished)).unwrap();
    let mut inventory = Inventory::recorded(&recorded::read(&dir), Some("machine-a")).unwrap();
    // A receiver's attribution never rewrites what a recorder bound, its "none" included.
    inventory.attribute(|_| Some("tools/guessed".into()));
    assert_eq!(inventory.operations.len(), 2);
    assert_eq!(inventory.finished.len(), 1);
    let one = &inventory.operations[0];
    assert_eq!(
        (one.kind.clone(), one.state.clone(), one.task.as_deref()),
        (Kind::Test, State::Running, Some("tools/x"))
    );
    assert_eq!(one.recipe, "cargo test --locked");
    assert_eq!(one.machine_id.as_deref(), Some("machine-a"));
    assert_eq!(one.phase_since, Some(1001.0));
    let two = &inventory.operations[1];
    assert_eq!(
        (two.kind.clone(), two.state.clone(), two.task.as_deref()),
        (Kind::Build, State::Queued, None)
    );
    assert_eq!(two.phase_since, Some(1000.0));
    assert_eq!(inventory.finished[0].task.as_deref(), Some("tools/x"));
    assert_eq!(inventory.revisions().len(), 3);
    assert!(Inventory::recorded(&recorded::read(&dir), Some("\n")).is_err());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_beat_is_a_new_publication_and_a_reread_of_the_same_bytes_is_not() {
    let dir = folder("beat");
    let mut beating = record(ONE, Step::Running);
    let _held = running(&dir, &beating);
    let first = Inventory::recorded(&recorded::read(&dir), Some("m")).unwrap();
    let again = Inventory::recorded(&recorded::read(&dir), Some("m")).unwrap();
    assert_eq!(first.revisions(), again.revisions());
    assert_eq!(first.operations, again.operations);
    beating.beat_at += 2.0;
    recorded::write(&path(&dir, ONE), &beating).unwrap();
    let beaten = Inventory::recorded(&recorded::read(&dir), Some("m")).unwrap();
    assert_ne!(first.revisions(), beaten.revisions());
    // The operation itself changes, so the projection renews its receipt from this read.
    assert_ne!(first.operations, beaten.operations);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_recorded_and_a_project_inventory_merge_into_one_and_never_share_a_nonce() {
    let dir = folder("merge");
    let _held = running(&dir, &record(ONE, Step::Running));
    let slots = |id: &str| {
        serde_json::to_vec(
            &serde_json::json!({"version":1,"complete":false,"uncertain":true,
            "truncated":false,"records":[{"evidence":"active","operation_id":id,"parent_id":null,
            "timing_run_id":null,"handle":"worker","checkout":"/same/path","recipe":"cargo build",
            "kind":"build","state":"running","resource":"","reason":"","phase_since_epoch_ms":1,
            "owner_pid":9,"child_pid":null,"exit_code":null}]}),
        )
        .unwrap()
    };
    let found = recorded::read(&dir);
    let mut merged = Inventory::recorded(&found, Some("m"))
        .unwrap()
        .merge(Inventory::decode(&slots(TWO), Some("m")).unwrap())
        .unwrap();
    assert_eq!(merged.operations.len(), 2);
    assert!(
        merged.uncertain,
        "the project reader's uncertainty is the merge's"
    );
    merged.attribute(|_| Some("tools/from-launch".into()));
    let task = |id: &str| {
        merged
            .operations
            .iter()
            .find(|operation| operation.operation_id.as_deref() == Some(id))
            .and_then(|operation| operation.task.clone())
    };
    assert_eq!(task(ONE).as_deref(), Some("tools/x"));
    assert_eq!(task(TWO).as_deref(), Some("tools/from-launch"));
    let repeated = Inventory::recorded(&found, Some("m"))
        .unwrap()
        .merge(Inventory::decode(&slots(ONE), Some("m")).unwrap());
    assert!(repeated.is_err());
    let elsewhere = Inventory::recorded(&found, Some("m"))
        .unwrap()
        .merge(Inventory::decode(&slots(TWO), Some("other")).unwrap());
    assert!(elsewhere.is_err());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_sweep_finishes_lost_work_without_an_exit_keeps_it_once_and_drops_what_is_an_hour_done() {
    let dir = folder("sweep");
    let _held = running(&dir, &record(ONE, Step::Running));
    gone(running(&dir, &record(TWO, Step::Running)));
    let mut old = record(THREE, Step::Finished);
    old.ended_at = Some(1004.0 - RETAIN - 1.0);
    recorded::write(&path(&dir, THREE), &old).unwrap();
    let now = 1003.0 + 1.0;
    let lost = recorded::sweep(&dir, now).unwrap();
    assert_eq!(lost.len(), 1);
    let kept = &lost[0];
    assert_eq!(kept.operation_id, TWO);
    assert_eq!(kept.state, Step::Finished);
    assert_eq!((kept.exit_code, kept.signal), (None, None));
    assert_eq!(
        kept.ended_at,
        Some(kept.beat_at),
        "it ran as long as it was last said to"
    );
    assert!(!path(&dir, TWO).with_extension("lease").exists());
    // The old one went; the one still running and the one just finished stay.
    let found = recorded::read(&dir);
    assert_eq!(ids(&found), [(ONE, Step::Running), (TWO, Step::Finished)]);
    assert!(!found.uncertain);
    let history = recorded::history(&dir).unwrap();
    assert_eq!(
        history
            .iter()
            .map(|record| record.operation_id.as_str())
            .collect::<Vec<_>>(),
        [TWO]
    );
    // Again: nothing more to finish, and nothing kept twice.
    assert!(recorded::sweep(&dir, now).unwrap().is_empty());
    assert_eq!(recorded::history(&dir).unwrap().len(), 1);
    // Past the hour, it goes too, but stays kept.
    recorded::sweep(&dir, 1003.0 + RETAIN + 1.0).unwrap();
    assert_eq!(ids(&recorded::read(&dir)), [(ONE, Step::Running)]);
    assert_eq!(recorded::history(&dir).unwrap().len(), 1);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_sweep_keeps_a_finished_operation_its_recorder_ended_before_keeping() {
    let dir = folder("unkept");
    recorded::write(&path(&dir, ONE), &record(ONE, Step::Finished)).unwrap();
    assert!(recorded::sweep(&dir, 1100.0).unwrap().is_empty());
    let history = recorded::history(&dir).unwrap();
    assert_eq!(history, [record(ONE, Step::Finished)]);
    recorded::keep(&dir, &record(ONE, Step::Finished)).unwrap();
    assert_eq!(recorded::history(&dir).unwrap().len(), 1);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_history_keeps_the_newest_and_passes_over_a_line_that_is_not_a_record() {
    let dir = folder("history");
    let mut text = String::from("not a record\n");
    for n in 0..KEEP {
        let mut old = record(&format!("{n:032x}"), Step::Finished);
        old.label = format!("run {n}");
        text.push_str(&serde_json::to_string(&old).unwrap());
        text.push('\n');
    }
    fs::write(dir.join(HISTORY), text).unwrap();
    let newest = record(&"f".repeat(32), Step::Finished);
    recorded::keep(&dir, &newest).unwrap();
    let history = recorded::history(&dir).unwrap();
    assert_eq!(history.len(), KEEP);
    assert_eq!(history[0].label, "run 1");
    assert_eq!(history.last(), Some(&newest));
    assert_eq!(history.last().unwrap().duration(), Some(30.0));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_work_folder_is_named_else_the_live_boards_else_none_for_a_test_board() {
    let env = |vars: &[(&str, &str)]| crate::cli::Env {
        vars: vars
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect(),
        cwd: std::env::temp_dir(),
    };
    assert_eq!(
        recorded::folder(&env(&[
            ("TASKS_WORK", "/w"),
            ("TASKS_SERVER", "127.0.0.1:1")
        ])),
        Some(PathBuf::from("/w"))
    );
    assert_eq!(
        recorded::folder(&env(&[("TASKS_SERVER", "127.0.0.1:1")])),
        None
    );
    let repository = folder("repository");
    crate::sys::output(
        crate::sys::command("git")
            .args(["init", "-q"])
            .current_dir(&repository),
    )
    .unwrap();
    let here = |vars: &[(&str, &str)]| crate::cli::Env {
        cwd: repository.clone(),
        ..env(vars)
    };
    let common = crate::git::common_dir(&repository).unwrap();
    assert_eq!(
        recorded::folder(&here(&[])),
        Some(recorded::in_common(&common))
    );
    assert_eq!(
        recorded::folder(&here(&[("TASKS_DATABASE", "scratch")])),
        Some(common.join("sf-tasks/work.scratch"))
    );
    fs::remove_dir_all(&repository).unwrap();
}
