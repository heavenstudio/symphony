//! A task whose work landed with another task's branch, closed as covered by that landing
//! (`close_covered`): by its executor or orchestrator in any open status, only by a task done and
//! landed whose landing names the commit, and never as an approve a landing or a move back to
//! review would go by.

use crate::common::{Fixture, Seed, refused, task};
use symphony_board::rows::TaskRow;
use symphony_board::{approved, covered_line};

/// The task whose branch landed.
const OTHER: &str = "tests/covering";
/// The task its landing covers.
const SLUG: &str = "app/covered";
/// The merge `OTHER`'s landing names, in full.
const COMMIT: &str = "75b4516a0123456789abcdef0123456789abcdef";
/// `OTHER`'s landing line, as `just land` writes it.
const LANDED: &str = "landed tests/covering on main at 75b4516a0123; pushed to origin";

/// `OTHER` in review and `SLUG` in `status`, which `ochre-owl` orchestrates and `amber-heron`
/// executes; `brisk-finch` reviews `SLUG`, `calm-vole` holds nothing and `dun-hare` filed it.
fn board(status: &str) -> Fixture {
    let filed = TaskRow {
        id: 2,
        created_by: "dun-hare".into(),
        ..task(SLUG, status)
    };
    Fixture::new(
        Seed::default()
            .session("ochre-owl")
            .session("amber-heron")
            .session("brisk-finch")
            .session("calm-vole")
            .session("dun-hare")
            .task(OTHER, "review")
            .task_row(filed)
            .role(OTHER, "ochre-owl", "orchestrator")
            .role(SLUG, "ochre-owl", "orchestrator")
            .role(SLUG, "amber-heron", "executor")
            .role(SLUG, "brisk-finch", "reviewer"),
    )
}

/// `OTHER` closed by its landing, whose line is `line`.
fn land(f: &Fixture, line: &str) {
    f.store
        .landed(OTHER, "ochre-owl", line, true, &["review"])
        .unwrap();
}

/// `board(status)` with `OTHER` landed.
fn landed(status: &str) -> Fixture {
    let f = board(status);
    land(&f, LANDED);
    f
}

#[test]
fn its_executor_closes_it_as_covered_and_the_thread_says_by_what() {
    let f = landed("in_progress");
    f.advance(5.0);
    let task = f
        .store
        .close_covered(
            SLUG,
            "amber-heron",
            OTHER,
            COMMIT,
            " its fix is 97e84883 \n",
        )
        .unwrap();
    assert_eq!(task.status, "done");
    let thread = f.store.thread(SLUG).unwrap();
    let last = thread.last().unwrap();
    assert_eq!(
        (
            last.sender.as_str(),
            last.kind.as_str(),
            last.text.as_str(),
            last.recipient.as_deref()
        ),
        (
            "amber-heron",
            "verdict",
            "covered by tests/covering, whose landing at 75b4516a0123 holds its work; closed by \
                 amber-heron: its fix is 97e84883",
            None
        )
    );
    assert_eq!(
        last.text,
        covered_line(OTHER, COMMIT, "amber-heron", "its fix is 97e84883")
    );
    // It is no approve: none stands on the task once it is closed.
    assert!(approved(&thread).is_none());
    // The commit the landing names, short as its line has it, is one it takes too.
    let f = landed("review");
    f.store
        .close_covered(SLUG, "amber-heron", OTHER, "75b4516a0123", "x")
        .unwrap();
}

#[test]
fn its_orchestrator_closes_it_in_any_open_status() {
    for status in [
        "backlog",
        "concept",
        "design",
        "ready",
        "in_progress",
        "review",
        "blocked",
    ] {
        let f = landed(status);
        let task = f
            .store
            .close_covered(SLUG, "ochre-owl", OTHER, COMMIT, "landed with it")
            .unwrap();
        assert_eq!(task.status, "done", "{status}");
        assert!(
            f.said(SLUG)
                .last()
                .unwrap()
                .ends_with("closed by ochre-owl: landed with it"),
            "{status}: {:?}",
            f.said(SLUG)
        );
    }
}

#[test]
fn no_other_session_closes_it_its_filer_and_reviewer_included() {
    let f = landed("in_progress");
    let before = f.rows();
    let board = f.store.board_command();
    for who in ["calm-vole", "dun-hare", "brisk-finch"] {
        assert_eq!(
            refused(f.store.close_covered(SLUG, who, OTHER, COMMIT, "x")),
            format!(
                "{who} is not {SLUG}'s executor or orchestrator, which alone close it as covered \
                     by another's landing; its orchestrator takes the role first: {board} role {SLUG} \
                     orchestrator"
            )
        );
    }
    assert_eq!(
        refused(f.store.close_covered(SLUG, "no-such", OTHER, COMMIT, "x")),
        f.no_session("no-such")
    );
    assert_eq!(f.rows(), before, "nothing is written");
}

#[test]
fn only_a_task_done_and_landed_covers_it() {
    let f = board("in_progress");
    assert_eq!(
        refused(
            f.store
                .close_covered(SLUG, "amber-heron", OTHER, COMMIT, "x")
        ),
        "tests/covering is review: a task is closed as covered only by a task done and landed"
    );
    assert_eq!(
        refused(
            f.store
                .close_covered(SLUG, "amber-heron", "no/task", COMMIT, "x")
        ),
        "no task 'no/task'"
    );
    // Approved and not landed: no landing closed it.
    f.store.take_role(OTHER, "brisk-finch", "reviewer").unwrap();
    f.store
        .verdict(OTHER, "brisk-finch", "approve", "abc1234: good")
        .unwrap();
    assert_eq!(
        refused(
            f.store
                .close_covered(SLUG, "amber-heron", OTHER, COMMIT, "x")
        ),
        "tests/covering is done, but no landing closed it: a task is closed as covered only by a \
             task done and landed"
    );
    // A landing whose line names no commit.
    let f = board("in_progress");
    land(&f, "landed tests/covering on main");
    assert_eq!(
        refused(
            f.store
                .close_covered(SLUG, "amber-heron", OTHER, COMMIT, "x")
        ),
        "tests/covering's landing names no commit (\"landed tests/covering on main\"): a task is \
             closed as covered only by a landing whose merge the default branch holds"
    );
    assert_eq!(f.store.get(SLUG).unwrap().status, "in_progress");
}

#[test]
fn the_commit_is_the_one_the_landing_names() {
    let f = landed("in_progress");
    assert_eq!(
        refused(f.store.close_covered(
            SLUG,
            "amber-heron",
            OTHER,
            "deadbeef0123456789abcdef0123456789abcdef",
            "x"
        )),
        "tests/covering's landing names 75b4516a0123, not deadbeef0123456789abcdef0123456789abcdef"
    );
    assert_eq!(f.store.get(SLUG).unwrap().status, "in_progress");
}

#[test]
fn a_finished_task_is_not_closed_again_nor_by_itself_nor_without_a_note() {
    for status in ["done", "cancelled"] {
        let f = landed(status);
        assert_eq!(
            refused(
                f.store
                    .close_covered(SLUG, "amber-heron", OTHER, COMMIT, "x")
            ),
            format!(
                "{SLUG} is {status}: only an open task is closed as covered by another's landing"
            )
        );
    }
    let f = landed("in_progress");
    assert_eq!(
        refused(
            f.store
                .close_covered(OTHER, "ochre-owl", OTHER, COMMIT, "x")
        ),
        "tests/covering is not covered by a landing of its own: it lands by a fresh reviewer's \
             approve"
    );
    assert_eq!(
        refused(
            f.store
                .close_covered(SLUG, "amber-heron", OTHER, COMMIT, " \n")
        ),
        "closing a task needs a note saying what came of it"
    );
    assert_eq!(f.store.get(SLUG).unwrap().status, "in_progress");
}

/// An approve given before, on a task sent back to review since, does not stand once the task is
/// closed as covered: the close is its last verdict, so neither a move back to review nor a
/// landing finds an approve to go by.
#[test]
fn an_approve_before_the_close_stands_no_more() {
    let f = landed("review");
    f.store
        .verdict(SLUG, "brisk-finch", "approve", "abc1234: good")
        .unwrap();
    f.store.move_to(SLUG, "review", "brisk-finch").unwrap();
    assert!(approved(&f.store.thread(SLUG).unwrap()).is_some());
    f.store
        .close_covered(
            SLUG,
            "amber-heron",
            OTHER,
            COMMIT,
            "its work landed with it",
        )
        .unwrap();
    let (task, thread) = (f.store.get(SLUG).unwrap(), f.store.thread(SLUG).unwrap());
    assert!(approved(&thread).is_none());
    assert_eq!(
        symphony_board::land::approval(&task, &thread).map(|a| a.by),
        Err(format!(
            "{SLUG} was closed after its approval: covered by tests/covering, whose landing at \
                 75b4516a0123 holds its work; closed by amber-heron: its work landed with it"
        ))
    );
    for who in ["amber-heron", "brisk-finch", "ochre-owl"] {
        assert_eq!(
            refused(f.store.move_to(SLUG, "review", who)),
            format!(
                "{SLUG} is done: only an approved task no landing has closed goes back to review, \
                     for a fresh approve of its new tip"
            )
        );
    }
}

/// With no approve before it, a covered task has none for a landing.
#[test]
fn a_landing_finds_no_approve_on_a_covered_task() {
    let f = landed("in_progress");
    f.store
        .close_covered(SLUG, "amber-heron", OTHER, COMMIT, "x")
        .unwrap();
    let (task, thread) = (f.store.get(SLUG).unwrap(), f.store.thread(SLUG).unwrap());
    assert_eq!(
        symphony_board::land::approval(&task, &thread).map(|a| a.by),
        Err(format!(
            "{SLUG} is done with no approving verdict from a reviewer"
        ))
    );
}
