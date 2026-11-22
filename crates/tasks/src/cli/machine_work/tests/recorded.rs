//! The recorder's operations as a reader finds them, the history it keeps, and how a sweep finishes
//! an operation whose recorder is gone.

use super::*;
use crate::cli::machine_work::native::Inventory;
use crate::cli::machine_work::recorded::{
    self, CpuReading, Found, HISTORY, OPERATIONS, Phase, RETAIN, ROTATE_BYTES, ROTATED, Record,
    Step,
};
use crate::sweep::trigger::now;
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

mod kept;

const ONE: &str = "11111111111111111111111111111111";
const TWO: &str = "22222222222222222222222222222222";
const THREE: &str = "33333333333333333333333333333333";

/// A folder of this test's own, emptied first.
fn folder(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "symphony-recorded-{}-{}-{name}",
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
        run: None,
        reading: None,
    }
}

/// A reading a recorder that watches its command writes with a beat.
fn reading() -> CpuReading {
    CpuReading {
        at: 1002.5,
        cpu_s: 1.25,
        partial: false,
        processes: 2,
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

/// The backquoted words of `text`, in order.
fn quoted(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// A reading is read back as its beat wrote it; a record without one (its recorder watches no
/// command's processes, or is older) reads as before and writes none. The record's table in
/// docs/MACHINE_WORK.md names each field a record holds, and its `reading` row each of a reading's.
#[test]
fn a_reading_reads_back_as_written_is_left_out_when_there_is_none_and_the_doc_names_its_fields() {
    let dir = folder("reading");
    let mut read = record(ONE, Step::Running);
    read.reading = Some(reading());
    let _held = running(&dir, &read);
    let _also = running(&dir, &record(TWO, Step::Running));
    let found = recorded::read(&dir);
    assert!(!found.uncertain, "{found:?}");
    let mut readings: Vec<_> = (found.records.iter())
        .map(|(record, _)| (record.operation_id.as_str(), record.reading))
        .collect();
    readings.sort_by_key(|(id, _)| *id);
    assert_eq!(readings, [(ONE, Some(reading())), (TWO, None)]);
    let text = fs::read_to_string(path(&dir, TWO)).unwrap();
    assert!(!text.contains("reading"), "{text}");
    fs::remove_dir_all(&dir).unwrap();

    let doc = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/MACHINE_WORK.md"),
    )
    .unwrap();
    let table = doc.split("**What.**").nth(1).unwrap();
    let rows: Vec<&str> = (table.lines())
        .skip_while(|line| !line.starts_with("| `"))
        .take_while(|line| line.starts_with('|'))
        .collect();
    let named: BTreeSet<String> = (rows.iter())
        .flat_map(|row| quoted(row.split('|').nth(1).unwrap()))
        .collect();
    let mut every = read.clone();
    every.run = Some(
        serde_json::from_value(serde_json::json!({
            "pipeline": "verify", "trigger": "owner"
        }))
        .unwrap(),
    );
    let written = serde_json::to_value(&every).unwrap();
    let fields: BTreeSet<String> = written.as_object().unwrap().keys().cloned().collect();
    assert_eq!(named, fields);
    let row = (rows.iter())
        .find(|row| row.starts_with("| `reading` |"))
        .unwrap();
    let said: BTreeSet<String> = quoted(row.split('|').nth(2).unwrap()).into_iter().collect();
    let keys = written["reading"].as_object().unwrap().keys().cloned();
    for key in keys {
        assert!(
            said.contains(&key),
            "the doc's reading row names no `{key}`: {row}"
        );
    }
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
    // A reading of CPU below none, of no process, or of a command not started yet.
    let mut below = record(TWO, Step::Running);
    below.reading = Some(CpuReading {
        cpu_s: -0.5,
        ..reading()
    });
    wrong.push(below);
    let mut none = record(TWO, Step::Running);
    none.reading = Some(CpuReading {
        processes: 0,
        ..reading()
    });
    wrong.push(none);
    let mut early = record(TWO, Step::Queued);
    early.reading = Some(reading());
    wrong.push(early);
    for bad in wrong {
        recorded::write(&path(&dir, TWO), &bad).unwrap();
        let found = recorded::read(&dir);
        assert_eq!(ids(&found), [(ONE, Step::Running)], "{bad:?}");
        assert!(found.uncertain, "{bad:?}");
    }
    fs::write(path(&dir, TWO), "{").unwrap();
    assert!(recorded::read(&dir).uncertain);
    // A field added later, under the same version, is passed over: the record is read.
    let mut later = serde_json::to_value(record(TWO, Step::Finished)).unwrap();
    later["added_later"] = serde_json::json!({"any": ["shape"]});
    fs::write(path(&dir, TWO), serde_json::to_vec(&later).unwrap()).unwrap();
    let found = recorded::read(&dir);
    assert_eq!(
        ids(&found),
        [(ONE, Step::Running), (TWO, Step::Finished)],
        "{later}"
    );
    assert!(!found.uncertain);
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

/// One read finding `records`, certain of them.
fn found(records: &[Record]) -> Found {
    Found {
        records: records
            .iter()
            .map(|record| (record.clone(), serde_json::to_vec(record).unwrap()))
            .collect(),
        uncertain: false,
    }
}

#[test]
fn finished_recorded_nonces_gone_from_the_folder_free_their_ownership_and_a_readers_never_do() {
    // Room for three nonces a machine's lifetime; ten recorded operations run, finish and go.
    let mut receiver = Projection::new(1, 3, 100);
    let sequence = std::cell::Cell::new(0);
    let observe = |receiver: &mut Projection, read: Found| {
        sequence.set(sequence.get() + 1);
        let inventory = Inventory::recorded(&read, Some("m")).unwrap();
        let at = sequence.get();
        receiver.observe_native(sample("m", at, vec![]), inventory, at)
    };
    for n in 0..10 {
        let id = format!("{n:032x}");
        observe(&mut receiver, found(&[record(&id, Step::Running)])).unwrap();
        observe(&mut receiver, found(&[record(&id, Step::Finished)])).unwrap();
        observe(&mut receiver, found(&[])).unwrap();
    }
    observe(&mut receiver, found(&[record(ONE, Step::Finished)])).unwrap();
    let running = record(TWO, Step::Running);
    observe(
        &mut receiver,
        found(&[record(ONE, Step::Finished), running.clone()]),
    )
    .unwrap();
    let machine = receiver.snapshot(sequence.get()).machines.remove(0);
    assert!(
        matches!(machine.freshness, Freshness::Fresh | Freshness::Incomplete),
        "{machine:?}"
    );
    let ids = machine
        .operations
        .iter()
        .map(|operation| operation.operation_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(ids, [Some(running.operation_id)]);
    // Still read, a finished one keeps its nonce: it cannot come back running.
    assert_eq!(
        observe(&mut receiver, found(&[record(ONE, Step::Running)])),
        Err(Rejected::ConflictingOperation)
    );

    // A project reader's finished nonces stay owned: its records may come back.
    let mut reader = Projection::new(1, 3, 100);
    let slots = |id: &str, state: &str| {
        let finished = state == "finished";
        let bytes = serde_json::to_vec(&serde_json::json!({"version":1,"complete":false,
            "uncertain":false,"truncated":false,"records":[{
            "evidence":if finished { "finished" } else { "active" },"operation_id":id,
            "parent_id":null,"timing_run_id":null,"handle":"worker","checkout":"/same/path",
            "recipe":"cargo build","kind":"build","state":state,"resource":"","reason":"",
            "phase_since_epoch_ms":1,"owner_pid":9,"child_pid":null,
            "exit_code":if finished { Some(0) } else { None }}]}))
        .unwrap();
        Inventory::decode(&bytes, Some("m")).unwrap()
    };
    let mut sequence = 0;
    let mut refused = None;
    for n in 0..4 {
        let id = format!("{n:032x}");
        for state in ["running", "finished"] {
            sequence += 1;
            if let Err(error) =
                reader.observe_native(sample("m", sequence, vec![]), slots(&id, state), sequence)
            {
                refused.get_or_insert((n, error));
            }
        }
    }
    assert_eq!(refused, Some((3, Rejected::Limit)));
    assert_eq!(
        reader.snapshot(sequence).machines[0].freshness,
        Freshness::Conflict
    );
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

/// `file` last changed `seconds` ago.
fn aged(file: &Path, seconds: u64) {
    let at = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds);
    OpenOptions::new()
        .append(true)
        .open(file)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

#[test]
fn a_sweep_finishes_lost_work_without_an_exit_keeps_it_once_and_drops_what_is_an_hour_done() {
    let dir = folder("sweep");
    let _held = running(&dir, &record(ONE, Step::Running));
    gone(running(&dir, &record(TWO, Step::Running)));
    recorded::write(&path(&dir, THREE), &record(THREE, Step::Finished)).unwrap();
    aged(&path(&dir, THREE), RETAIN as u64 + 1);
    let now = now();
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
    recorded::sweep(&dir, now + RETAIN + 1.0).unwrap();
    assert_eq!(ids(&recorded::read(&dir)), [(ONE, Step::Running)]);
    assert_eq!(recorded::history(&dir).unwrap().len(), 1);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_sweep_clears_a_record_no_live_recorder_holds_and_no_reader_reads_at_once_and_the_rest_once_an_hour_old()
 {
    let dir = folder("tidy");
    let operations = dir.join(OPERATIONS);
    let partial = operations.join(format!(".{ONE}.json.1.0"));
    // A history write of an older recorder's own, cut short in the work folder itself.
    let history_partial = dir.join(format!(".{HISTORY}.1.0"));
    let orphan = path(&dir, TWO).with_extension("lease");
    for (file, text) in [(&partial, "{"), (&history_partial, "{"), (&orphan, "")] {
        fs::write(file, text).unwrap();
    }
    // Records no reader here reads, whose recorders are gone: one cut short with no lease, and one
    // of another version beside the lease its recorder let go.
    let unreadable = path(&dir, THREE);
    fs::write(&unreadable, "{").unwrap();
    let six = "6".repeat(32);
    let mut newer = record(&six, Step::Running);
    newer.version = 2;
    gone(running(&dir, &newer));
    let unread = [
        unreadable,
        path(&dir, &six),
        path(&dir, &six).with_extension("lease"),
    ];
    // A live recorder's lease before its first record, and beside one that cannot be read.
    let (four, five) = ("4".repeat(32), "5".repeat(32));
    let lease = |id: &str| {
        let file = File::create(path(&dir, id).with_extension("lease")).unwrap();
        file.lock().unwrap();
        file
    };
    let held = [lease(&four), lease(&five)];
    fs::write(path(&dir, &five), "{").unwrap();
    let now = now();
    recorded::sweep(&dir, now).unwrap();
    for file in &unread {
        assert!(!file.exists(), "{} stayed unread", file.display());
    }
    for file in [&partial, &history_partial, &orphan] {
        assert!(file.exists(), "{} went within the hour", file.display());
    }
    recorded::sweep(&dir, now + RETAIN + 60.0).unwrap();
    for file in [&partial, &history_partial, &orphan] {
        assert!(!file.exists(), "{} stayed", file.display());
    }
    for kept in [
        path(&dir, &four).with_extension("lease"),
        path(&dir, &five).with_extension("lease"),
        path(&dir, &five),
    ] {
        assert!(
            kept.exists(),
            "{} went with its recorder alive",
            kept.display()
        );
    }
    assert!(recorded::history(&dir).unwrap().is_empty());
    for file in held {
        gone(file);
    }
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_sweep_leaves_a_record_it_could_not_read_to_a_sweep_that_can() {
    use std::os::unix::fs::PermissionsExt as _;
    // SAFETY: geteuid cannot fail.
    if unsafe { libc::geteuid() } == 0 {
        // Every file opens for root: none is unreadable.
        return;
    }
    let dir = folder("unreadable-now");
    // A lost recorder's record, and one that is no record, neither held by a recorder.
    gone(running(&dir, &record(ONE, Step::Running)));
    let malformed = path(&dir, TWO);
    fs::write(&malformed, "{").unwrap();
    let records = [path(&dir, ONE), malformed.clone()];
    let mode = |mode| {
        for file in &records {
            fs::set_permissions(file, fs::Permissions::from_mode(mode)).unwrap();
        }
    };
    mode(0o000);
    assert!(recorded::sweep(&dir, now()).unwrap().is_empty());
    for file in [&records[0], &records[0].with_extension("lease"), &malformed] {
        assert!(file.exists(), "{} went unread", file.display());
    }
    assert!(recorded::history(&dir).unwrap().is_empty());
    assert!(recorded::read(&dir).uncertain);
    mode(0o644);
    let finished = recorded::sweep(&dir, now()).unwrap();
    assert_eq!(
        finished
            .iter()
            .map(|done| done.operation_id.as_str())
            .collect::<Vec<_>>(),
        [ONE]
    );
    assert!(!malformed.exists(), "no record stayed once read");
    let found = recorded::read(&dir);
    assert_eq!(ids(&found), [(ONE, Step::Finished)]);
    assert!(!found.uncertain);
    assert_eq!(recorded::history(&dir).unwrap().len(), 1);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_sweep_lets_go_the_lease_of_an_operation_its_recorder_finished_without_keeping_it_twice() {
    let dir = folder("finished-lease");
    let done = record(ONE, Step::Finished);
    recorded::keep(&dir, &done).unwrap();
    // Its recorder ended between saying it finished and letting its lease go.
    gone(running(&dir, &done));
    assert!(recorded::sweep(&dir, now()).unwrap().is_empty());
    assert!(!path(&dir, ONE).with_extension("lease").exists());
    assert_eq!(ids(&recorded::read(&dir)), [(ONE, Step::Finished)]);
    assert_eq!(recorded::history(&dir).unwrap(), [done]);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_sweep_another_recorder_is_making_is_left_to_it() {
    let dir = folder("busy");
    gone(running(&dir, &record(ONE, Step::Running)));
    let lock = File::create(dir.join(recorded::LOCK)).unwrap();
    lock.lock().unwrap();
    assert!(recorded::sweep(&dir, now()).unwrap().is_empty());
    assert!(recorded::read(&dir).uncertain, "still lost, not finished");
    lock.unlock().unwrap();
    assert_eq!(recorded::sweep(&dir, now()).unwrap().len(), 1);
    // A folder no recorder made yet has nothing to sweep and is not made by one.
    let never = dir.join("never");
    assert!(recorded::sweep(&never, now()).unwrap().is_empty());
    assert!(!never.exists());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_earlier_run_is_waited_for_until_it_ends_unless_its_command_still_runs() {
    let dir = folder("pending");
    let before = crate::store::now() + 10.0;
    let mut dead = crate::sys::command("true").spawn().unwrap();
    let gone = dead.id();
    dead.wait().unwrap();
    let pending =
        |label: &str| recorded::pending(&dir, ONE, before, |found: &Record| found.label == label);
    // An earlier lease, held, its record not written yet: it may yet be the run repeated.
    let lease = path(&dir, TWO).with_extension("lease");
    let held = File::create(&lease).unwrap();
    held.lock().unwrap();
    assert!(pending("cargo test --locked"));
    // Its command still runs: it had not ended when this run was asked for.
    fs::write(&lease, format!("{}\n", std::process::id())).unwrap();
    assert!(!pending("cargo test --locked"));
    // Its command is gone: its end line is still to come.
    fs::write(&lease, format!("{gone}\n")).unwrap();
    assert!(pending("cargo test --locked"));
    // Its record says it runs: waited for when it is of the same command.
    let mut found = record(TWO, Step::Running);
    found.child_pid = Some(gone);
    recorded::write(&path(&dir, TWO), &found).unwrap();
    assert!(pending("cargo test --locked"));
    assert!(!pending("another command"));
    // Its lease naming none, the process its record names still runs.
    fs::write(&lease, "").unwrap();
    found.child_pid = Some(std::process::id());
    recorded::write(&path(&dir, TWO), &found).unwrap();
    assert!(!pending("cargo test --locked"));
    found.child_pid = Some(gone);
    recorded::write(&path(&dir, TWO), &found).unwrap();
    assert!(pending("cargo test --locked"));
    // Finished, or let go by its recorder, it is waited for no more.
    recorded::write(&path(&dir, TWO), &record(TWO, Step::Finished)).unwrap();
    assert!(!pending("cargo test --locked"));
    recorded::write(&path(&dir, TWO), &found).unwrap();
    held.unlock().unwrap();
    assert!(!pending("cargo test --locked"));
    // Its own lease, and one made after it was asked for, are no earlier run's.
    held.lock().unwrap();
    assert!(!recorded::pending(&dir, TWO, before, |_| true));
    assert!(!recorded::pending(&dir, ONE, before - 20.0, |_| true));
    fs::remove_dir_all(&dir).unwrap();
}

/// A finished record of `n`, its label long enough that a few hundred fill a history.
fn padded(n: usize) -> Record {
    let mut done = record(&format!("{n:032x}"), Step::Finished);
    done.label = format!("run {n} {}", "x".repeat(4000));
    done
}

/// Records kept from `from` until the current history reaches its bound, the last one rotating it.
fn fill(dir: &Path, from: usize) -> usize {
    let mut n = from;
    while fs::metadata(dir.join(HISTORY)).map_or(0, |meta| meta.len()) < ROTATE_BYTES {
        recorded::keep(dir, &padded(n)).unwrap();
        n += 1;
        if !dir.join(HISTORY).exists() {
            break;
        }
    }
    n
}

#[test]
fn the_history_appends_each_operation_and_passes_over_a_line_that_is_not_a_record() {
    let dir = folder("history");
    fs::write(dir.join(HISTORY), "not a record\n{\"version\":1,\n").unwrap();
    let first = record(ONE, Step::Finished);
    let mut second = record(TWO, Step::Finished);
    second.exit_code = Some(101);
    recorded::keep(&dir, &first).unwrap();
    recorded::keep(&dir, &second).unwrap();
    let history = recorded::history(&dir).unwrap();
    assert_eq!(history, [first.clone(), second]);
    assert_eq!(history[0].duration(), Some(30.0));
    // One operation kept twice reads once, as first kept.
    let mut again = first.clone();
    again.label = "again".into();
    recorded::keep(&dir, &again).unwrap();
    assert_eq!(recorded::history(&dir).unwrap()[0], first);
    assert_eq!(recorded::history(&dir).unwrap().len(), 2);
    // A history not made yet holds nothing.
    assert!(recorded::history(&dir.join("never")).unwrap().is_empty());
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_history_rotates_at_its_bound_keeping_the_one_before_and_dropping_older() {
    let dir = folder("rotation");
    let first = fill(&dir, 0);
    assert!(!dir.join(HISTORY).exists(), "the full history rotated");
    let rotated = fs::metadata(dir.join(ROTATED)).unwrap().len();
    assert!(rotated >= ROTATE_BYTES, "{rotated}");
    assert!(rotated < ROTATE_BYTES + 8192, "{rotated}");
    // The next one starts a history of its own; both read, oldest first.
    recorded::keep(&dir, &padded(first)).unwrap();
    let history = recorded::history(&dir).unwrap();
    assert_eq!(history.len(), first + 1);
    assert_eq!(history[0], padded(0));
    assert_eq!(history.last(), Some(&padded(first)));
    // A second rotation replaces the first generation: the two files stay bounded.
    let second = fill(&dir, first + 1);
    let history = recorded::history(&dir).unwrap();
    assert_eq!(history.len(), second - first);
    assert_eq!(history[0], padded(first));
    assert_eq!(history.last(), Some(&padded(second - 1)));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_history_waits_to_rotate_while_another_recorder_sweeps() {
    let dir = folder("rotation-held");
    let lock = File::create(dir.join(recorded::LOCK)).unwrap();
    lock.lock().unwrap();
    let mut n = 0;
    while fs::metadata(dir.join(HISTORY)).map_or(0, |meta| meta.len()) < ROTATE_BYTES {
        recorded::keep(&dir, &padded(n)).unwrap();
        n += 1;
    }
    assert!(!dir.join(ROTATED).exists(), "rotated under another's lock");
    lock.unlock().unwrap();
    recorded::keep(&dir, &padded(n)).unwrap();
    assert!(!dir.join(HISTORY).exists());
    assert_eq!(recorded::history(&dir).unwrap().len(), n + 1);
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
        Some(common.join("symphony/tasks/work.scratch"))
    );
    fs::remove_dir_all(&repository).unwrap();
}
