//! `finish`, and the cleanup after a task finishes, on the launch commands' private board and
//! throwaway repository.

use crate::launch::Panes;
use symphony_board::{NewLaunch, NewSession, NewTask};

/// A task `sim/a` with a session launched on `on` (it, or an instruction subtask under it), in the
/// idle pane `w1:p1`, and claimed there: the worker that runs it.
fn launched_on(p: &Panes, on: &str) -> String {
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
            pane: "w1:p1".into(),
            ..NewLaunch::default()
        })
        .unwrap();
    board.claim(on, &worker).unwrap();
    p.idle_pane(&worker);
    worker
}

fn closed_line(on: &str, worker: &str) -> String {
    format!(
        "→ closed pane w1:p1 of {worker} (its tasks are finished); restore: just tasks restore {on} {worker}\n"
    )
}

#[test]
fn a_task_that_finishes_with_a_session_launched_on_it_is_cleaned_up_after() {
    for (on, args, status) in [
        ("sim/a", &["move", "sim/a", "cancelled"][..], "cancelled"),
        ("sim/a/do", &["done", "sim/a/do", "it is"][..], "done"),
    ] {
        let p = Panes::new("finish");
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
        assert_eq!(p.pane_ids(), Vec::<String>::new());
    }
}

#[test]
fn a_verdict_that_finishes_a_task_is_cleaned_up_after() {
    let p = Panes::new("verdict");
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
    assert_eq!(
        ran.out,
        format!("sim/a is done\n{}", closed_line("sim/a", &worker))
    );
}

#[test]
fn a_session_launched_on_a_task_below_counts_for_the_one_that_finishes() {
    let p = Panes::new("below");
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
fn nothing_is_asked_of_herdr_for_a_task_no_session_was_launched_on_or_one_still_open() {
    let p = Panes::new("quiet");
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
fn a_finish_that_cannot_read_the_process_table_keeps_the_pane_and_names_the_failure() {
    for (on, args, status) in [
        ("sim/a", &["move", "sim/a", "cancelled"][..], "cancelled"),
        ("sim/a/do", &["done", "sim/a/do", "it is"][..], "done"),
    ] {
        let p = Panes::new("finish-no-table");
        let worker = launched_on(&p, on);
        let why = p.unreadable_process_table();
        let ran = p.as_(&worker, args);
        assert_eq!(
            (ran.code, ran.out, ran.err.as_str()),
            (
                0,
                format!(
                    "{on} is {status}\n→ kept pane w1:p1 (never closed on no information: {why})\n"
                ),
                ""
            ),
            "{args:?}"
        );
        assert_eq!(p.pane_ids(), ["w1:p1"]);
        assert_eq!(p.board().get(on).unwrap().status, status);
    }
}

#[test]
fn a_cleanup_that_fails_is_said_and_the_status_change_stands() {
    for (key, value, why) in [
        ("FAKE_HERDR_FAIL", "pane list", "herdr pane list failed: "),
        ("HERDR_BIN", "/no/such/herdr", "herdr pane list: "),
    ] {
        let mut p = Panes::new("fails");
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
    let p = Panes::new("finish-command");
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
