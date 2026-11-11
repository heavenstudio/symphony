//! The built-in reader: Symphony's run records, read at each tick and attributed by the board.

use super::*;
use symphony_board::cli::machine_work::{Kind, State};
use symphony_board::sys::{Command, Stdio};

/// A process of the test's own standing for a run's wrapper, ended when dropped.
struct Wrapper(symphony_board::sys::Child);

impl Wrapper {
    fn start() -> Self {
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Self(child)
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }

    fn end(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Wrapper {
    fn drop(&mut self) {
        self.end();
    }
}

/// A record in `common`'s run folder as `tools/cargo.sh` writes it, ended with `end` when given.
fn write(
    common: &Path,
    name: &str,
    kind: &str,
    pid: u32,
    checkout: &Path,
    branch: &str,
    end: Option<i32>,
) -> PathBuf {
    let dir = common.join(runs::DIR);
    std::fs::create_dir_all(&dir).unwrap();
    let mut text = json!({"v":1,"kind":kind,"pid":pid,"checkout":checkout,"branch":branch,
        "tip":"abc123","handle":"","command":format!("cargo {kind} --locked"),"launcher":""})
    .to_string();
    text.push('\n');
    if let Some(exit) = end {
        text.push_str(&format!("{{\"exit\":{exit},\"seconds\":3}}\n"));
    }
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn task(slug: &str, status: &str, roles: &[(&str, &str)]) -> symphony_board::Task {
    symphony_board::Task {
        branch: None,
        id: 0,
        slug: slug.into(),
        title: String::new(),
        body: String::new(),
        status: status.into(),
        priority: 2,
        parent: None,
        created_by: String::new(),
        created: 0.0,
        updated: 0.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: (roles.iter())
            .map(|(handle, role)| symphony_board::Role {
                handle: (*handle).into(),
                role: (*role).into(),
                since: 0.0,
                expires: None,
                health: symphony_board::Health::Live,
            })
            .collect(),
        instruction: false,
        fields: Default::default(),
    }
}

/// A sampler with no reader of the project's own, its identity given.
fn runs_sampler(root: &Path) -> Sampler {
    let mut sampler = Sampler::new(root, None);
    sampler.identity = Some(Ok("machine-a".into()));
    sampler
}

fn scan(sampler: &mut Sampler, root: &Path, tasks: &[symphony_board::Task]) -> Snapshot {
    let context = Context {
        records: &[],
        tasks,
        bindings: &no_bindings,
    };
    sampler
        .scan(root, Some(&root.join(".git")), &context)
        .snapshot
}

#[test]
fn a_live_run_is_its_tasks_operation_until_its_process_ends() {
    let tree = Tree::new();
    let root = repository(&tree);
    let common = root.join(".git");
    let tasks = [task("app/x", "in_progress", &[])];
    let mut sampler = runs_sampler(&root);
    let mut wrapper = Wrapper::start();
    let record = write(
        &common,
        "1-1.run",
        "test",
        wrapper.pid(),
        &root,
        "app/x",
        None,
    );
    let going = scan(&mut sampler, &root, &tasks);
    assert_eq!(going.machines.len(), 1);
    let machine = &going.machines[0];
    assert_eq!(machine.machine_id, "machine-a");
    assert_eq!(machine.freshness, Freshness::Incomplete);
    assert_eq!(machine.counts, None);
    let operation = &machine.operations[0];
    assert_eq!(operation.task.as_deref(), Some("app/x"));
    assert_eq!(operation.machine_id.as_deref(), Some("machine-a"));
    assert_eq!(
        (&operation.kind, &operation.state),
        (&Kind::Test, &State::Running)
    );
    assert_eq!(operation.recipe, "cargo test --locked");
    // The wire's placement: an in-progress task with a running test sits in Test.
    let wire: symphony_api::MachineWork =
        serde_json::from_value(serde_json::to_value(&going).unwrap()).unwrap();
    let board_task: symphony_api::Task = serde_json::from_value(json!({
        "slug":"app/x","title":"X","body":"","status":"in_progress","priority":2,"parent":null,
        "created_by":"fixture","created":1.0,"updated":1.0,"links":[],"depends":[],"roles":[],
        "instruction":false}))
    .unwrap();
    assert_eq!(
        wire.task_stage(&board_task, true),
        symphony_api::TaskStage::Test
    );
    // An unchanged tick is the same observation, so the feed hears nothing new.
    let again = scan(&mut sampler, &root, &[]);
    assert_eq!(again, going, "a later board must not rebind a run");
    wrapper.end();
    let ended = scan(&mut sampler, &root, &tasks);
    assert!(ended.machines[0].operations.is_empty());
    assert!(ended.machines[0].sequence > going.machines[0].sequence);
    assert!(!record.exists(), "a run cut short left its record");
    let wire: symphony_api::MachineWork =
        serde_json::from_value(serde_json::to_value(&ended).unwrap()).unwrap();
    assert_eq!(
        wire.task_stage(&board_task, true),
        symphony_api::TaskStage::Lifecycle("in_progress")
    );
}

#[test]
fn a_project_that_never_recorded_a_run_has_no_publisher_and_a_prepared_reader_wins() {
    let tree = Tree::new();
    let root = repository(&tree);
    let mut sampler = runs_sampler(&root);
    let snapshot = scan(&mut sampler, &root, &[]);
    assert!(snapshot.machines.is_empty());
    assert_eq!(sampler.reason, "");
    // Its first record makes it one, even with nothing going.
    let wrapper = Wrapper::start();
    write(
        &root.join(".git"),
        "1-1.run",
        "build",
        999_999,
        &root,
        "",
        Some(0),
    );
    let snapshot = scan(&mut sampler, &root, &[]);
    assert_eq!(snapshot.machines.len(), 1);
    assert!(snapshot.machines[0].operations.is_empty());
    // A reader the project prepared answers instead.
    let reader = root.join(".target/bin/sf-slots");
    std::fs::create_dir_all(reader.parent().unwrap()).unwrap();
    crate::fixture_script::write(&reader, "#!/bin/sh\n/bin/cat \"$3/native.json\"\n", true);
    std::fs::write(root.join(".git/native.json"), input("running")).unwrap();
    write(
        &root.join(".git"),
        "2-2.run",
        "test",
        wrapper.pid(),
        &root,
        "",
        None,
    );
    let snapshot = scan(&mut sampler, &root, &[]);
    assert_eq!(snapshot.machines.len(), 1);
    let operations = &snapshot.machines[0].operations;
    assert_eq!(operations.len(), 1);
    assert_eq!(
        operations[0].recipe, "cargo build",
        "the run records answered"
    );
    assert!(sampler.records.is_none());
}

#[test]
fn each_task_keeps_its_last_finished_run_and_older_ones_go() {
    let tree = Tree::new();
    let root = repository(&tree);
    let common = root.join(".git");
    let tasks = [task("app/x", "review", &[])];
    let older = write(&common, "1-1.run", "test", 1, &root, "app/x", Some(101));
    let mut sampler = runs_sampler(&root);
    scan(&mut sampler, &root, &tasks);
    assert_eq!(sampler.last_run("app/x").unwrap().id, "1-1.run");
    // A later run of the task is its last; the earlier one is removed.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let newer = write(&common, "2-2.run", "test", 2, &root, "app/x", Some(0));
    let unclaimed = write(&common, "3-3.run", "test", 3, &root, "main", Some(0));
    scan(&mut sampler, &root, &tasks);
    let last = sampler.last_run("app/x").unwrap();
    assert_eq!((last.id.as_str(), last.tip.as_str()), ("2-2.run", "abc123"));
    assert_eq!(last.ended.as_ref().map(|ended| ended.exit), Some(0));
    assert_eq!(last.command, "cargo test --locked");
    assert!(!older.exists());
    assert!(newer.exists());
    assert!(unclaimed.exists(), "an unclaimed run is kept a day");
    assert_eq!(sampler.last_run("main"), None);
}

#[test]
fn without_a_machine_identity_a_run_stays_visible_and_unassigned() {
    let tree = Tree::new();
    let root = repository(&tree);
    let wrapper = Wrapper::start();
    write(
        &root.join(".git"),
        "1-1.run",
        "build",
        wrapper.pid(),
        &root,
        "app/x",
        None,
    );
    let mut sampler = Sampler::new(&root, None);
    sampler.identity = Some(Err("corrupt installation identity".into()));
    let snapshot = scan(&mut sampler, &root, &[task("app/x", "in_progress", &[])]);
    assert!(snapshot.machines.is_empty());
    assert_eq!(snapshot.unassigned.len(), 1);
    assert_eq!(snapshot.unassigned[0].machine_id, None);
    assert_eq!(snapshot.unassigned[0].task.as_deref(), Some("app/x"));
    assert!(sampler.reason.contains("corrupt installation identity"));
}
