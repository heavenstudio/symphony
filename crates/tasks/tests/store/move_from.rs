//! A move only from the statuses its caller names (`Store::move_from`), decided against the status
//! the move's own write reads: a move a caller decided on an earlier read never takes a task from
//! where another move put it since.

use crate::common::{Fixture, Seed, refused};

#[test]
fn a_move_from_review_sends_work_back_but_leaves_work_blocked_since_where_it_is() {
    let f = Fixture::new(Seed::default());
    let exe = f.session("worker");
    f.task("sim/jump");
    f.store.claim("sim/jump", &exe).unwrap();
    f.store.move_to("sim/jump", "review", &exe).unwrap();
    let back = f
        .store
        .move_from("sim/jump", &["review"], "in_progress", &exe);
    assert_eq!(back.unwrap().status, "in_progress");
    let last = f.store.thread("sim/jump").unwrap().pop().unwrap();
    assert_eq!(last.text, format!("{exe} moved it review → in_progress"));
    // Read in review by a caller, then blocked by its executor before the caller's move.
    f.store.move_to("sim/jump", "review", &exe).unwrap();
    f.store.move_to("sim/jump", "blocked", &exe).unwrap();
    let said = f.store.thread("sim/jump").unwrap().len();
    assert_eq!(
        refused(
            f.store
                .move_from("sim/jump", &["review"], "in_progress", &exe)
        ),
        "sim/jump is blocked, not review: it stays blocked"
    );
    assert_eq!(f.store.get("sim/jump").unwrap().status, "blocked");
    assert_eq!(
        f.store.thread("sim/jump").unwrap().len(),
        said,
        "a move refused says nothing on the thread"
    );
    // From any one of several; the pipeline and the roles still rule as for any move.
    let back = f
        .store
        .move_from("sim/jump", &["review", "blocked"], "in_progress", &exe);
    assert_eq!(back.unwrap().status, "in_progress");
    let other = f.session("worker");
    assert_eq!(
        refused(
            f.store
                .move_from("sim/jump", &["in_progress"], "review", &other)
        ),
        format!("{other} holds no role on sim/jump, which is in_progress")
    );
    assert_eq!(
        refused(
            f.store
                .move_from("sim/jump", &["in_progress"], "done", &exe)
        ),
        format!(
            "sim/jump cannot go from in_progress to done; it goes to review first: {board} move \
                 sim/jump review; or, when its work landed with another task's, {board} done \
                 sim/jump NOTE --covered-by OTHER",
            board = f.store.board_command()
        )
    );
}
