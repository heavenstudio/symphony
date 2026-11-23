//! The board's own recorder (`tasks run`) as a scan reads it from the common dir: on a scratch
//! board, with no project reader, beside one, and as its beats keep long work fresh.

use super::*;
use std::time::Duration;
use symphony_board::cli::Env;
use symphony_board::cli::machine_work::recorded::{self as work, Phase, Record, Step};

/// What the scan of `sampler` places `task` in, as the board shows it.
fn stage(sampler: &mut Sampler, root: &Path, common: &Path, task: &symphony_api::Task) -> String {
    let wire = wire(sampler.scan(root, Some(common), &[]).snapshot);
    wire.task_stage(task, true).as_str().to_string()
}

fn wire(snapshot: Snapshot) -> symphony_api::MachineWork {
    serde_json::from_value(serde_json::to_value(snapshot).unwrap()).unwrap()
}

/// `task`'s slug and status as the board's API carries them, which are what place it.
fn shown(task: &symphony_board::Task) -> symphony_api::Task {
    serde_json::from_value(json!({
        "slug": task.slug, "title": task.title, "body": "", "status": task.status, "priority": 1,
        "parent": null, "created_by": task.created_by, "created": task.created,
        "updated": task.updated, "links": [], "depends": [], "roles": [], "instruction": false,
    }))
    .unwrap()
}

/// The one record in `folder` once it is in `state`: written by the recorder once the board
/// answered which task it is for, so waited for on its condition, with a backstop only a stall
/// reaches (30 s was its bound on the wall). A wait that gives up says what the folder holds.
fn once(folder: &Path, state: Step) -> Record {
    test_time::until(
        format!("one operation {state:?}"),
        test_time::backstop(Duration::from_secs(30)),
        || match work::read(folder).records.as_slice() {
            [(one, _)] if one.state == state => Some(one.clone()),
            _ => None,
        },
    )
    .unwrap_or_else(|stall| {
        let records: Vec<_> = work::read(folder)
            .records
            .into_iter()
            .map(|(record, _)| record)
            .collect();
        panic!("{stall}: the folder holds {records:?}")
    })
}

#[test]
fn a_recorded_test_puts_the_task_its_session_holds_in_test_while_it_runs_and_back_after() {
    let tree = Tree::new();
    let root = repository(&tree);
    let common = root.join(".git");
    let f = Fixture::empty();
    let worker = f.session("worker");
    f.task("tools/recorded");
    let claimed = shown(&f.store.claim("tools/recorded", &worker).unwrap());
    assert_eq!(claimed.status, "in_progress");
    let release = tree.path("release");
    let vars: BTreeMap<String, String> = [
        ("TASKS_SERVER", f.store.database().addr()),
        ("TASKS_DATABASE", f.store.database().database().to_string()),
        ("TASKS_SESSION", worker.clone()),
        (
            work::FOLDER_VAR,
            work::in_common(&common).display().to_string(),
        ),
        (
            "SYMPHONY_MACHINE_DIR",
            tree.path("machine").display().to_string(),
        ),
        // Its board's answer and git's waited for twenty times as long, which the recorder grants
        // on this scratch board alone (`cli::recorder`'s scale), so a loaded machine never decides
        // which task its record names.
        ("TASKS_RUN_WAIT_SCALE", "20".to_string()),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), value))
    .collect();
    let env = Env {
        vars,
        cwd: root.clone(),
    };
    let args: Vec<String> = [
        "run",
        "--phase",
        "test",
        "--label",
        "verify",
        "--",
        "/bin/sh",
        "-c",
        "while [ ! -e \"$1\" ]; do sleep 0.02; done",
        "sh",
        release.to_str().unwrap(),
    ]
    .map(str::to_string)
    .to_vec();
    let recorder = std::thread::spawn(move || {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code =
            symphony_board::cli::main(&args, &env, &mut std::io::empty(), &mut out, &mut err);
        (code, out, err)
    });
    // No project reader: the recorder's own records are the machine's work.
    let mut sampler = Sampler::new(&root, None);
    sampler.identity = Some(Ok("machine-a".into()));
    let folder = work::in_common(&common);
    let running = once(&folder, Step::Running);
    assert_eq!(running.task.as_deref(), Some("tools/recorded"));
    assert_eq!(stage(&mut sampler, &root, &common, &claimed), "test");
    let view = sampler.snapshot(sampler.receipt()).machines.remove(0);
    assert_eq!(view.operations.len(), 1, "{view:?}");
    assert_eq!(view.operations[0].recipe, "verify");
    assert_eq!(view.operations[0].handle.as_deref(), Some(worker.as_str()));
    std::fs::write(&release, "").unwrap();
    let (code, out, err) = recorder.join().unwrap();
    assert_eq!((code, out, err), (0, Vec::new(), Vec::new()));
    assert_eq!(
        stage(&mut sampler, &root, &common, &claimed),
        "in_progress",
        "a finished operation leaves its task where its lifecycle has it"
    );
    let history = work::history(&folder).unwrap();
    let [finished] = history.as_slice() else {
        panic!("{history:?}");
    };
    assert_eq!(finished.operation_id, running.operation_id);
    assert_eq!((finished.exit_code, finished.signal), (Some(0), None));
    assert!(finished.duration().unwrap() >= 0.0, "{finished:?}");
    assert_eq!(finished.task.as_deref(), Some("tools/recorded"));
}

/// A running record of `id`'s, written to `folder` with its lease held by the file returned.
fn running(folder: &Path, id: &str, task: &str) -> std::fs::File {
    let operations = folder.join(work::OPERATIONS);
    std::fs::create_dir_all(&operations).unwrap();
    beat(folder, id, task, 1000.0);
    let lease = std::fs::File::create(operations.join(format!("{id}.lease"))).unwrap();
    lease.lock().unwrap();
    lease
}

/// `id`'s record in `state` for `task`, as its recorder writes it when it beats at `beat_at`.
fn made(id: &str, task: &str, state: Step, beat_at: f64) -> Record {
    let finished = state == Step::Finished;
    Record {
        version: 1,
        operation_id: id.into(),
        parent_id: None,
        phase: Phase::Build,
        label: "cargo build".into(),
        state,
        handle: Some("worker".into()),
        task: Some(task.into()),
        checkout: "/private/checkout".into(),
        branch: None,
        commit: None,
        machine_id: None,
        host: "host".into(),
        pid: 1,
        child_pid: Some(2),
        queued_at: 1000.0,
        started_at: Some(1000.0),
        ended_at: finished.then_some(beat_at),
        beat_at,
        exit_code: finished.then_some(0),
        signal: None,
        reason: String::new(),
        run: None,
        reading: None,
    }
}

/// `id`'s running record as its recorder writes it when it beats at `beat_at`.
fn beat(folder: &Path, id: &str, task: &str, beat_at: f64) {
    let record = made(id, task, Step::Running, beat_at);
    let path = folder.join(work::OPERATIONS).join(format!("{id}.json"));
    work::write(&path, &record).unwrap();
}

#[test]
fn recorded_work_and_a_project_readers_work_are_one_machines_and_either_malformed_is_unknown() {
    let tree = Tree::new();
    let root = repository(&tree);
    let common = root.join(".git");
    let reader = super::prepared_reader(&root);
    crate::fixture_script::write(&reader, "#!/bin/sh\n/bin/cat \"$3/native.json\"\n", true);
    std::fs::write(common.join("native.json"), input("running")).unwrap();
    let folder = work::in_common(&common);
    let _held = running(&folder, &"a".repeat(32), "tools/recorded");
    let mut sampler = super::reading(&root, None);
    sampler.identity = Some(Ok("machine-a".into()));
    let snapshot = sampler
        .scan(&root, Some(&common), &[record("task-a")])
        .snapshot;
    let view = &snapshot.machines[0];
    assert_eq!(view.freshness, Freshness::Incomplete, "{view:?}");
    let mut tasks: Vec<_> = view
        .operations
        .iter()
        .map(|operation| (operation.resource.as_str(), operation.task.as_deref()))
        .collect();
    tasks.sort_unstable();
    // The recorder's binding is its own; the project reader's is the receiver's.
    assert_eq!(
        tasks,
        [
            ("cargo-room", Some("task-a")),
            ("tasks run", Some("tools/recorded"))
        ]
    );
    std::fs::write(
        folder
            .join(work::OPERATIONS)
            .join(format!("{}.json", "b".repeat(32))),
        "{",
    )
    .unwrap();
    let unknown = sampler.scan(&root, Some(&common), &[]).snapshot;
    assert_eq!(unknown.machines[0].freshness, Freshness::Unknown);
    assert_eq!(unknown.machines[0].operations.len(), 2);
}

#[test]
fn a_machine_where_nothing_was_recorded_and_no_reader_answers_is_never_claimed() {
    let tree = Tree::new();
    let root = repository(&tree);
    let common = root.join(".git");
    let mut sampler = Sampler::new(&root, None);
    let evidence = sampler.scan(&root, Some(&common), &[]);
    assert!(sampler.identity.is_none());
    assert!(evidence.snapshot.machines.is_empty());
    assert!(
        sampler.reason.starts_with("no build or test recorded yet"),
        "{}",
        sampler.reason
    );
}

#[test]
fn a_recorders_beat_keeps_long_work_fresh_where_an_unchanged_reread_lets_it_go_stale() {
    let tree = Tree::new();
    let folder = tree.path("work");
    let id = "c".repeat(32);
    let _held = running(&folder, &id, "tools/recorded");
    let mut sampler = sampler();
    let read = |sampler: &Sampler| {
        Inventory::recorded(&work::read(&folder), Some(&sampler.trusted().unwrap())).unwrap()
    };
    sampler.observe(read(&sampler), &[], 10, "host").unwrap();
    assert_eq!(
        sampler.snapshot(10).machines[0].freshness,
        Freshness::Incomplete
    );
    sampler
        .observe(read(&sampler), &[], 10_012, "host")
        .unwrap();
    assert_eq!(
        sampler.snapshot(10_012).machines[0].freshness,
        Freshness::Stale,
        "the same record read again is not news"
    );
    // Its recorder says it runs, two seconds on.
    beat(&folder, &id, "tools/recorded", 1002.0);
    sampler
        .observe(read(&sampler), &[], 10_013, "host")
        .unwrap();
    let view = sampler.snapshot(10_013).machines.remove(0);
    assert_eq!(view.freshness, Freshness::Incomplete, "{view:?}");
    assert_eq!(view.age_ms, Some(0));
    assert_eq!(
        view.operations[0].reason,
        "running; its recorder said so 2 s after it started"
    );
}

#[test]
fn five_thousand_finished_recorded_operations_leave_a_running_one_placed_in_build() {
    let tree = Tree::new();
    let root = repository(&tree);
    let common = root.join(".git");
    let mut sampler = Sampler::new(&root, None);
    sampler.identity = Some(Ok("machine-a".into()));
    // Each read holds the ten newest finished operations; a sweep took the older ones.
    let mut window = std::collections::VecDeque::new();
    for n in 0..5_000_u32 {
        window.push_back(made(
            &format!("{n:032x}"),
            "tools/done",
            Step::Finished,
            1001.0,
        ));
        if window.len() > 10 {
            window.pop_front();
        }
        let read = work::Found {
            records: window
                .iter()
                .map(|record| (record.clone(), serde_json::to_vec(record).unwrap()))
                .collect(),
            uncertain: false,
        };
        let inventory = Inventory::recorded(&read, Some("machine-a")).unwrap();
        let now = sampler.receipt();
        sampler.observe(inventory, &[], now, "host").unwrap();
    }
    assert!(sampler.reason.is_empty(), "{}", sampler.reason);
    assert!(sampler.revisions.len() <= 10, "{}", sampler.revisions.len());
    let folder = work::in_common(&common);
    let _held = running(&folder, &"d".repeat(32), "tools/recorded");
    let task: symphony_api::Task = serde_json::from_value(json!({
        "slug": "tools/recorded", "title": "recorded", "body": "", "status": "in_progress",
        "priority": 1, "parent": null, "created_by": "orchestrator", "created": 0.0,
        "updated": 0.0, "links": [], "depends": [], "roles": [], "instruction": false,
    }))
    .unwrap();
    assert_eq!(stage(&mut sampler, &root, &common, &task), "build");
    let view = sampler.snapshot(sampler.receipt()).machines.remove(0);
    assert_eq!(view.freshness, Freshness::Incomplete, "{view:?}");
}
