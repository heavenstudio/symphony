//! `finish`, and the cleanup after a task finishes, on the launch commands' private board and
//! throwaway repository.

use crate::launch::Terminals;
use symphony_board::{NewLaunch, NewSession, NewTask};

/// A task `sim/a` with a session launched on `on` (it, or an instruction subtask under it), in the
/// idle terminal `t1`, and claimed there: the worker that runs it.
fn launched_on(p: &Terminals, on: &str) -> String {
    let board = p.board();
    let orch = board
        .register(&NewSession::new("orchestrator"))
        .unwrap()
        .row
        .handle;
    let worker = board
        .register(&NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    board.create(&NewTask::new("sim/a", "A", &orch)).unwrap();
    if on != "sim/a" {
        board
            .create(&NewTask {
                parent: Some("sim/a".into()),
                instruction: true,
                ..NewTask::new(on, "Do", &orch)
            })
            .unwrap();
    }
    board
        .record_launch(&NewLaunch {
            slug: on.into(),
            handle: worker.clone(),
            role: "executor".into(),
            claude_session: "c".into(),
            pane: "t1".into(),
            ..NewLaunch::default()
        })
        .unwrap();
    board.claim(on, &worker).unwrap();
    p.idle_terminal(&worker);
    worker
}

fn closed_line(on: &str, worker: &str) -> String {
    format!(
        "→ closed pane t1 of {worker} (its tasks are finished); restore: just tasks restore {on} {worker}\n"
    )
}

#[test]
fn a_task_that_finishes_with_a_session_launched_on_it_is_cleaned_up_after() {
    for (on, args, status) in [
        ("sim/a", &["move", "sim/a", "cancelled"][..], "cancelled"),
        ("sim/a/do", &["done", "sim/a/do", "it is"][..], "done"),
    ] {
        let p = Terminals::new("finish");
        let worker = launched_on(&p, on);
        let ran = p.as_(&worker, args);
        assert_eq!(
            (ran.code, ran.out, ran.err.as_str()),
            (
                0,
                format!("{on} is {status}\n{}", closed_line(on, &worker)),
                ""
            ),
            "{args:?}"
        );
        assert_eq!(p.addresses(), Vec::<String>::new());
    }
}

#[test]
fn an_approve_leaves_the_executors_terminal_to_the_land_that_finishes_the_task() {
    let p = Terminals::new("verdict");
    let worker = launched_on(&p, "sim/a");
    let board = p.board();
    board.move_to("sim/a", "review", &worker).unwrap();
    let reviewer = board
        .register(&NewSession::new("reviewer"))
        .unwrap()
        .row
        .handle;
    board.take_role("sim/a", &reviewer, "reviewer").unwrap();
    let ran = p.as_(&reviewer, &["verdict", "sim/a", "approve", "fine"]);
    // In a checkout whose justfile runs no land of its own: its default branch and the default
    // landing are named. Done is not landed: the land may still ask the executor for something.
    assert_eq!(
        ran.out,
        format!(
            "sim/a is approved: done on the board, not on main until it lands; `just land sim/a \
             --as HANDLE` lands it\n→ kept pane t1 of {worker} ({})\n",
            symphony_board::launch::NOT_LANDED
        )
    );
    assert_eq!(p.addresses(), ["t1"]);
    assert!(
        board.task_sessions("sim/a").unwrap()[0].closed.is_none(),
        "its record stays open"
    );
    // `finish` is the cleanup of a finished task whoever asks; a land asks once it closed the task.
    let ran = p.run(&["finish", "sim/a"]);
    assert_eq!(ran.out, closed_line("sim/a", &worker));
    assert_eq!(p.addresses(), Vec::<String>::new());
}

#[test]
fn a_session_launched_on_a_task_below_counts_for_the_one_that_finishes() {
    let p = Terminals::new("below");
    let worker = launched_on(&p, "sim/a/do");
    p.board().done("sim/a/do", &worker, "did it").unwrap(); // on the board itself: no cleanup yet
    let orch = p.board().get("sim/a").unwrap().created_by;
    let ran = p.as_(&orch, &["move", "sim/a", "cancelled"]);
    assert_eq!(
        ran.out,
        format!("sim/a is cancelled\n{}", closed_line("sim/a/do", &worker))
    );
}

#[test]
fn nothing_is_asked_of_hc_for_a_task_no_session_was_launched_on_or_one_still_open() {
    let p = Terminals::new("quiet");
    let worker = launched_on(&p, "sim/a");
    let board = p.board();
    board.create(&NewTask::new("sim/b", "B", &worker)).unwrap();
    board.claim("sim/b", &worker).unwrap();
    assert_eq!(
        p.as_(&worker, &["move", "sim/b", "cancelled"]).out,
        "sim/b is cancelled\n"
    );
    assert_eq!(
        p.as_(&worker, &["move", "sim/a", "review"]).out,
        "sim/a is review\n"
    );
    assert_eq!(p.calls(), Vec::<Vec<String>>::new());
}

#[test]
fn a_finish_that_cannot_read_the_process_table_keeps_the_terminal_and_names_the_failure() {
    for (on, args, status) in [
        ("sim/a", &["move", "sim/a", "cancelled"][..], "cancelled"),
        ("sim/a/do", &["done", "sim/a/do", "it is"][..], "done"),
    ] {
        for table in [None, Some("    1     0 /sbin/launchd\n")] {
            let p = Terminals::new("finish-no-table");
            let worker = launched_on(&p, on);
            let why = no_table(&p, table);
            let ran = p.as_(&worker, args);
            assert_eq!(
                (ran.code, ran.out, ran.err.as_str()),
                (
                    0,
                    format!(
                        "{on} is {status}\n→ kept pane t1 of {worker} (never closed on no information: {why})\n"
                    ),
                    ""
                ),
                "{args:?} {table:?}"
            );
            assert_eq!(p.addresses(), ["t1"]);
            assert_eq!(p.board().get(on).unwrap().status, status);
        }
    }
}

/// The process table the commands read made useless: none (`table` is None), or `table`, which
/// lists no process of the terminal's. What each kept terminal's line says of it.
fn no_table(p: &Terminals, table: Option<&str>) -> String {
    let file = p.ps_table();
    match table {
        None => {
            std::fs::remove_file(&file).unwrap();
            format!(
                "ps table {}: No such file or directory (os error 2)",
                file.display()
            )
        }
        Some(text) => {
            std::fs::write(&file, text).unwrap();
            format!("ps table {} lists no process 99999999", file.display())
        }
    }
}

#[test]
fn a_cleanup_that_cannot_read_the_process_table_keeps_the_terminal_and_names_the_failure() {
    for table in [None, Some("garbage only\n")] {
        let p = Terminals::new("cleanup-no-table");
        let worker = launched_on(&p, "sim/a");
        p.board().move_to("sim/a", "cancelled", &worker).unwrap(); // on the board itself
        let why = no_table(&p, table);
        for args in [&["cleanup", "--dry-run"][..], &["cleanup"]] {
            let ran = p.run(args);
            assert_eq!(
                (ran.code, ran.out, ran.err.as_str()),
                (
                    0,
                    format!("kept pane t1 of {worker} (never closed on no information: {why})\n"),
                    ""
                ),
                "{args:?} {table:?}"
            );
        }
        assert_eq!(p.addresses(), ["t1"]);
    }
}

#[test]
fn a_cleanup_that_fails_is_said_and_the_status_change_stands() {
    for (key, value, why) in [
        ("FAKE_HC_FAIL", "ls", "hc ls: fake_failure: ls failed"),
        ("HC_BIN", "/no/such/hc", "hc ls: "),
    ] {
        let mut p = Terminals::new("fails");
        let worker = launched_on(&p, "sim/a");
        p.set(key, value);
        let ran = p.as_(&worker, &["move", "sim/a", "cancelled"]);
        assert_eq!((ran.code, ran.err.as_str()), (0, ""));
        let start =
            format!("sim/a is cancelled\n→ could not clean up the sessions of sim/a: {why}");
        assert!(
            ran.out.starts_with(&start) && ran.out.matches('\n').count() == 2,
            "{}",
            ran.out
        );
        assert_eq!(p.board().get("sim/a").unwrap().status, "cancelled");
    }
}

#[test]
fn finish_closes_what_a_finished_task_left_and_needs_no_session() {
    let p = Terminals::new("finish-command");
    let worker = launched_on(&p, "sim/a");
    p.board().move_to("sim/a", "cancelled", &worker).unwrap(); // on the board itself, as `just land` does
    let ran = p.run(&["finish", "sim/a"]);
    assert_eq!(
        (ran.code, ran.out, ran.err.as_str()),
        (0, closed_line("sim/a", &worker), "")
    );
    let again = p.run(&["finish", "sim/a"]);
    assert_eq!(
        (again.code, again.out.as_str(), again.err.as_str()),
        (0, "", "")
    );
}
