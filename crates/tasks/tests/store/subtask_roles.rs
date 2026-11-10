//! Neither implementation slices nor instructions start a separate review lifecycle.

use crate::common::{Fixture, refused};
use symphony_board::NewTask;

#[test]
fn neither_kind_of_subtask_starts_an_independent_review() {
    for instruction in [false, true] {
        let f = Fixture::empty();
        let agent = f.session("worker");
        f.task("a");
        f.task_with(NewTask {
            parent: Some("a".into()),
            instruction,
            ..NewTask::new("a/m", "Measure", "owner")
        });
        f.store.claim("a/m", &agent).unwrap();
        assert_eq!(
            refused(f.store.move_to("a/m", "review", &agent)),
            "a/m is a subtask: integrate into its parent and close it with `done`, not review"
        );
        assert_eq!(
            refused(f.store.move_to("a/m", "done", &agent)),
            "a/m cannot go from in_progress to done"
        );
        assert_eq!(f.store.get("a/m").unwrap().status, "in_progress");
    }
}
