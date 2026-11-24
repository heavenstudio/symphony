//! A slice closes with its integration note; its parent owns delivery.

use crate::common::{Fixture, Seed, refused, task};
use symphony_board::rows::TaskRow;

/// `a`, and under it `a/m`, an instruction subtask in progress that `amber-heron` executes.
fn instruction() -> Fixture {
    subtask(true, "in_progress", Some("a"))
}

fn subtask(instruction: bool, status: &str, parent: Option<&str>) -> Fixture {
    let row = TaskRow {
        id: 2,
        parent: parent.map(String::from),
        instruction,
        ..task("a/m", status)
    };
    Fixture::new(
        Seed::default()
            .session("amber-heron")
            .session("brisk-finch")
            .task("a", "ready")
            .task_row(row)
            .role("a/m", "amber-heron", "executor"),
    )
}

#[test]
fn an_instruction_subtask_closes_itself_with_a_note() {
    let f = instruction();
    let task = f
        .store
        .done("a/m", "amber-heron", " median 4.1 ms\n")
        .unwrap();
    assert_eq!(task.status, "done");
    let last = f.store.thread("a/m").unwrap().pop().unwrap();
    assert_eq!(
        (last.kind.as_str(), last.sender.as_str(), last.text.as_str()),
        ("verdict", "amber-heron", "done: median 4.1 ms")
    );
}

#[test]
fn done_needs_a_note() {
    let f = instruction();
    assert_eq!(
        refused(f.store.done("a/m", "amber-heron", "  \n")),
        "closing a task needs a note saying what came of it"
    );
    assert_eq!(f.store.get("a/m").unwrap().status, "in_progress");
}

#[test]
fn an_ordinary_subtask_closes_without_an_independent_review() {
    let f = subtask(false, "in_progress", Some("a"));
    let result = f
        .store
        .done("a/m", "amber-heron", "integrated commit abc123 into a")
        .unwrap();
    assert_eq!(result.status, "done");
    assert_eq!(f.store.get("a").unwrap().status, "ready");
}

#[test]
fn neither_kind_of_top_level_task_closes_as_a_slice() {
    for instruction in [false, true] {
        let f = subtask(instruction, "in_progress", None);
        assert_eq!(
            refused(f.store.done("a/m", "amber-heron", "finished")),
            format!(
                "a/m is not a subtask; it closes by a verdict or a landing, or, when its work \
                     landed with another task's, as covered by that landing: {} done a/m NOTE \
                     --covered-by OTHER",
                f.store.board_command()
            )
        );
    }
}

#[test]
fn only_the_executor_closes_it() {
    let f = instruction();
    assert_eq!(
        refused(f.store.done("a/m", "brisk-finch", "x")),
        "brisk-finch is not the executor of a/m"
    );
}

#[test]
fn only_work_in_progress_is_closed() {
    for status in ["ready", "done", "blocked"] {
        let f = subtask(true, status, Some("a"));
        assert_eq!(
            refused(f.store.done("a/m", "amber-heron", "x")),
            format!("a/m is {status}, not in progress")
        );
    }
}
