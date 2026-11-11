//! The run records `tools/cargo.sh` writes, as the built-in reader reads and attributes them.

use super::*;
use crate::health::Health;
use crate::{Binding, Role};
use std::fs;

/// A folder of its own under the temporary one, for test `name`: a stand-in git common dir.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sf-runs-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(DIR)).unwrap();
    dir
}

/// A record's first line, as `tools/cargo.sh` writes it.
fn start(kind: &str, pid: i64, checkout: &str, branch: &str) -> String {
    format!(
        "{{\"v\":1,\"kind\":\"{kind}\",\"pid\":{pid},\"checkout\":\"{checkout}\",\"branch\":\"{branch}\",\"tip\":\"abc123\",\"handle\":\"\",\"command\":\"cargo {kind} --locked\",\"launcher\":\"\"}}\n"
    )
}

fn end(exit: i32, seconds: u32) -> String {
    format!("{{\"exit\":{exit},\"seconds\":{seconds}}}\n")
}

fn write(common: &Path, name: &str, text: &str) -> PathBuf {
    let path = common.join(DIR).join(name);
    fs::write(&path, text).unwrap();
    path
}

/// A pid no process holds now: a child that ended and was reaped.
fn dead_pid() -> i64 {
    let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
    let pid = i64::from(child.id());
    child.wait().unwrap();
    pid
}

fn task(slug: &str, status: &str, roles: &[(&str, &str)]) -> Task {
    Task {
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
            .map(|(handle, role)| Role {
                handle: (*handle).into(),
                role: (*role).into(),
                since: 0.0,
                expires: None,
                health: Health::Live,
            })
            .collect(),
        instruction: false,
        fields: Default::default(),
    }
}

fn run(handle: Option<&str>, checkout: &str, branch: &str) -> Run {
    Run {
        id: "1-1.run".into(),
        kind: Kind::Test,
        pid: 1,
        checkout: checkout.into(),
        branch: branch.into(),
        tip: String::new(),
        handle: handle.map(str::to_owned),
        command: "cargo test".into(),
        launcher: None,
        started: 10.0,
        ended: None,
    }
}

fn bound(worktree: &str) -> Binding {
    Binding {
        in_process: true,
        worktree: worktree.into(),
        ..Binding::default()
    }
}

fn owner(tasks: &[Task], bindings: &[(&str, Binding)], run: &Run) -> Owner {
    let bindings = (bindings.iter())
        .map(|(handle, binding)| ((*handle).to_owned(), binding.clone()))
        .collect();
    Owners {
        tasks,
        bindings: &bindings,
        records: &[],
    }
    .of(run)
}

#[test]
fn a_record_reads_its_start_and_its_end_and_a_line_not_yet_whole_is_none() {
    let going = parse("7-1.run", &start("test", 7, "/w", "app/x"), 100.0).unwrap();
    assert_eq!(
        (going.kind, going.pid, going.started, going.ended),
        (Kind::Test, 7, 100.0, None)
    );
    assert_eq!(
        (going.branch.as_str(), going.tip.as_str()),
        ("app/x", "abc123")
    );
    assert_eq!(going.command, "cargo test --locked");
    assert_eq!((going.handle, going.launcher), (None, None));
    let text = format!("{}{}", start("build", 7, "/w", ""), end(101, 40));
    let finished = parse("7-1.run", &text, 140.0).unwrap();
    assert_eq!(finished.kind, Kind::Build);
    assert_eq!(finished.started, 100.0);
    assert_eq!(
        finished.ended,
        Some(Ended {
            exit: 101,
            seconds: 40.0,
            at: 140.0
        })
    );
    // An end still being written is no end; a start still being written is no run.
    let half = format!("{}{{\"exit\":0,", start("test", 7, "/w", ""));
    assert_eq!(parse("7-1.run", &half, 100.0).unwrap().ended, None);
    let line = start("test", 7, "/w", "");
    assert_eq!(parse("7-1.run", line.trim_end(), 100.0), None);
    for other in [
        start("fmt", 7, "/w", ""),
        start("test", 0, "/w", ""),
        start("test", 7, "relative/w", ""),
        start("test", 7, "/w", "").replace("\"v\":1", "\"v\":2"),
        "not json\n".into(),
        String::new(),
    ] {
        assert_eq!(parse("7-1.run", &other, 100.0), None, "{other}");
    }
    let named = start("test", 7, "/w", "")
        .replace("\"handle\":\"\"", "\"handle\":\"amber-heron\"")
        .replace("\"launcher\":\"\"", "\"launcher\":\"tasks\"");
    let named = parse("7-1.run", &named, 100.0).unwrap();
    assert_eq!(named.handle.as_deref(), Some("amber-heron"));
    assert_eq!(named.launcher.as_deref(), Some("tasks"));
}

#[test]
fn the_reader_lists_live_runs_keeps_finished_ones_and_removes_a_run_cut_short() {
    let common = scratch("reader");
    let me = i64::from(std::process::id());
    let dead = dead_pid();
    write(&common, "a-live.run", &start("test", me, "/w", "app/x"));
    write(
        &common,
        "b-live-too.run",
        &start("build", me, "/w", "app/x"),
    );
    let cut = write(&common, "c-cut.run", &start("test", dead, "/w", "app/x"));
    let ended = format!("{}{}", start("test", dead, "/w", "app/x"), end(0, 3));
    write(&common, "d-ended.run", &ended);
    let junk = write(&common, "e-junk.run", "no run\n");
    write(&common, "notes.txt", "not a record\n");
    write(&common, ".f-hidden.run", &start("test", me, "/w", "app/x"));
    let records = read(&common, &alive).unwrap();
    let ids = |runs: &[Run]| runs.iter().map(|run| run.id.clone()).collect::<Vec<_>>();
    // Two runs for one checkout and branch are two runs, each its own operation.
    assert_eq!(ids(&records.going), ["a-live.run", "b-live-too.run"]);
    assert_eq!(ids(&records.finished), ["d-ended.run"]);
    assert_eq!(records.unreadable, 1);
    assert!(
        !cut.exists(),
        "a run whose process is gone without an end stays"
    );
    assert!(junk.exists(), "a file that is no run was removed");
    // No folder is no run, and no failure.
    let empty = std::env::temp_dir().join(format!("sf-runs-none-{}", std::process::id()));
    let none = read(&empty, &alive).unwrap();
    assert!(none.going.is_empty() && none.finished.is_empty() && none.unreadable == 0);
}

#[test]
fn a_record_whose_run_ended_while_it_was_read_is_finished_not_removed() {
    let common = scratch("ended-meanwhile");
    let path = write(&common, "a.run", &start("test", 4242, "/w", ""));
    let records = read(&common, &|_, _| {
        // The wrapper appends its end just as the reader finds its process gone.
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        std::io::Write::write_all(&mut file, end(0, 1).as_bytes()).unwrap();
        false
    })
    .unwrap();
    assert!(records.going.is_empty());
    assert_eq!(records.finished.len(), 1);
    assert!(path.exists());
}

#[test]
fn a_pid_used_again_after_its_record_was_written_is_not_the_run() {
    let me = i64::from(std::process::id());
    let now = crate::sweep::trigger::now();
    assert!(alive(me, now));
    // This process began long after a record dated an hour ago: the pid names another process.
    assert!(!alive(me, now - 3600.0));
    assert!(!alive(dead_pid(), now));
    assert!(!alive(0, now));
}

#[test]
fn a_run_is_its_sessions_one_open_task_and_an_instruction_child_gives_way_to_its_parent() {
    let mut parent = task("app/feature", "in_progress", &[("sable-crane", "executor")]);
    let mut child = task(
        "app/feature/work",
        "in_progress",
        &[("sable-crane", "executor")],
    );
    child.parent = Some("app/feature".into());
    child.instruction = true;
    let done = task("app/old", "done", &[("sable-crane", "executor")]);
    let tasks = [done.clone(), parent.clone(), child.clone()];
    let mine = run(Some("sable-crane"), "/w/a", "app/feature");
    assert_eq!(
        owner(&tasks, &[], &mine),
        Owner {
            handle: Some("sable-crane".into()),
            task: Some("app/feature".into())
        }
    );
    // Off the branch named for it too: the session's one open task.
    let elsewhere = run(Some("sable-crane"), "/w/a", "scratch");
    assert_eq!(
        owner(&tasks, &[], &elsewhere).task.as_deref(),
        Some("app/feature")
    );
    // A reviewer's run is the task it reviews.
    parent.status = "review".into();
    parent.roles[0].role = "reviewer".into();
    let reviewing = [parent.clone()];
    assert_eq!(
        owner(&reviewing, &[], &elsewhere).task.as_deref(),
        Some("app/feature")
    );
    // An orchestrator's role is not work it runs.
    parent.roles[0].role = "orchestrator".into();
    assert_eq!(owner(&[parent], &[], &elsewhere).task, None);
}

#[test]
fn of_two_open_tasks_the_branch_decides_and_never_a_guess() {
    let one = task("app/one", "in_progress", &[("amber-heron", "executor")]);
    let two = task("app/two", "review", &[("amber-heron", "reviewer")]);
    let tasks = [one, two];
    let named = run(Some("amber-heron"), "/w", "app/two");
    assert_eq!(owner(&tasks, &[], &named).task.as_deref(), Some("app/two"));
    // A review task's branch is the one it was filed for.
    let review = [task(
        "review/app/three",
        "review",
        &[("amber-heron", "reviewer")],
    )];
    let filed = run(Some("amber-heron"), "/w", "app/three");
    assert_eq!(
        owner(&review, &[], &filed).task.as_deref(),
        Some("review/app/three")
    );
    let unnamed = run(Some("amber-heron"), "/w", "main");
    assert_eq!(
        owner(&tasks, &[], &unnamed),
        Owner {
            handle: Some("amber-heron".into()),
            task: None
        }
    );
}

#[test]
fn a_run_with_no_session_is_the_one_bound_in_its_checkout_else_its_branchs_task() {
    let work = std::env::temp_dir().join(format!("sf-runs-checkout-{}", std::process::id()));
    fs::create_dir_all(&work).unwrap();
    let checkout = work.to_str().unwrap();
    let tasks = [
        task("app/x", "in_progress", &[("amber-heron", "executor")]),
        task("app/y", "in_progress", &[]),
        task("app/gone", "done", &[]),
    ];
    let here = run(None, checkout, "detached-or-anything");
    assert_eq!(
        owner(&tasks, &[("amber-heron", bound(checkout))], &here),
        Owner {
            handle: Some("amber-heron".into()),
            task: Some("app/x".into())
        }
    );
    // A full session's binding records no checkout; another checkout's subagent is not this one's.
    let full = Binding {
        in_process: false,
        worktree: checkout.into(),
        ..Binding::default()
    };
    let others = [("amber-heron", full), ("cobalt-otter", bound("/elsewhere"))];
    assert_eq!(owner(&tasks, &others, &here), Owner::default());
    // Two sessions in one checkout name neither: the branch decides.
    let shared = [
        ("amber-heron", bound(checkout)),
        ("cobalt-otter", bound(checkout)),
    ];
    let on_y = run(None, checkout, "app/y");
    assert_eq!(
        owner(&tasks, &shared, &on_y),
        Owner {
            handle: None,
            task: Some("app/y".into())
        }
    );
    // A run with no task: main, or a branch whose task is finished.
    for branch in ["main", "app/gone", ""] {
        let alone = run(None, checkout, branch);
        assert_eq!(owner(&tasks, &shared, &alone), Owner::default(), "{branch}");
    }
    // An open launch there names its agent as a binding does; a closed one does not.
    let mut launched: TaskSessionRow = serde_json::from_value(serde_json::json!({
        "slug":"app/x","handle":"amber-heron","role":"executor","claude_session":"",
        "worktree":checkout,"started":1.0,"closed":null}))
    .unwrap();
    let bindings = BTreeMap::new();
    let by_launch = |records: &[TaskSessionRow]| {
        Owners {
            tasks: &tasks,
            bindings: &bindings,
            records,
        }
        .of(&here)
    };
    assert_eq!(
        by_launch(std::slice::from_ref(&launched)).task.as_deref(),
        Some("app/x")
    );
    launched.closed = Some(2.0);
    assert_eq!(by_launch(&[launched]), Owner::default());
}

#[test]
fn a_going_run_is_a_running_operation_of_its_machine_in_a_partial_inventory() {
    let mut going = run(Some("amber-heron"), "/w", "app/x");
    going.kind = Kind::Build;
    let owner = Owner {
        handle: Some("amber-heron".into()),
        task: Some("app/x".into()),
    };
    let operation = operation(&going, "machine-a", owner);
    assert_eq!(operation.machine_id.as_deref(), Some("machine-a"));
    assert_eq!(operation.operation_id.as_deref(), Some("1-1.run"));
    assert_eq!(operation.task.as_deref(), Some("app/x"));
    assert_eq!(
        (&operation.kind, &operation.state),
        (&Kind::Build, &State::Running)
    );
    assert_eq!(operation.recipe, "cargo test");
    assert_eq!(operation.phase_since, Some(10.0));
    let machine = machine("machine-a", "host", 3, vec![operation]);
    assert_eq!(machine.freshness, Freshness::Incomplete);
    assert!(!machine.complete);
    assert_eq!(machine.counts, None);
    assert_eq!(machine.sequence, 3);
    assert_eq!(machine.operations.len(), 1);
}

#[test]
fn each_task_keeps_its_last_finished_run_and_a_launchers_build_is_never_kept() {
    let common = scratch("keep");
    let finished = |id: &str, at: f64, launcher: Option<&str>| {
        write(&common, id, "kept until judged\n");
        Run {
            id: id.into(),
            launcher: launcher.map(str::to_owned),
            ended: Some(Ended {
                exit: 0,
                seconds: 5.0,
                at,
            }),
            started: at - 5.0,
            ..run(None, "/w", "app/x")
        }
    };
    let now = 1_000_000.0;
    let kept = keep_last(
        &common,
        vec![
            (finished("old.run", now - 60.0, None), Some("app/x".into())),
            (finished("new.run", now - 10.0, None), Some("app/x".into())),
            (
                finished("older.run", now - 90.0, None),
                Some("app/x".into()),
            ),
            (finished("other.run", now - 5.0, None), Some("app/y".into())),
            (
                finished("launch.run", now - 1.0, Some("tasks")),
                Some("app/x".into()),
            ),
            (finished("young.run", now - 60.0, None), None),
            (
                finished("stale.run", now - UNCLAIMED_SECONDS - 1.0, None),
                None,
            ),
        ],
        now,
    );
    assert_eq!(
        kept.iter()
            .map(|(task, run)| (task.as_str(), run.id.as_str()))
            .collect::<Vec<_>>(),
        [("app/x", "new.run"), ("app/y", "other.run")]
    );
    let left: Vec<String> = {
        let mut names: Vec<String> = fs::read_dir(common.join(DIR))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    };
    assert_eq!(left, ["new.run", "other.run", "young.run"]);
}
