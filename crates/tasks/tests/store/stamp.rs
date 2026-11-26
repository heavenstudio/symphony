//! A task's stamp (`Store::task_stamp`): the same while nothing `get` and `thread` read of the task
//! has changed, another after each thing that changes them.

use crate::common::{Fixture, Seed, under};
use symphony_board::people::{Holder, Power};
use symphony_board::{NewLaunch, TaskStamp};

fn stamp(f: &Fixture, slug: &str) -> TaskStamp {
    f.store
        .task_stamp(slug)
        .unwrap()
        .expect("the task has a stamp")
}

/// The stamp carries who decides on the board, so a reader of the `owner` stamp asks no more of
/// the board while nothing changed: a change of who decides, by a person's own power or a
/// group's, moves every task's stamp, and a change of who is on the board that gives no one
/// decide moves it too, as the people read with it are another.
#[test]
fn a_tasks_stamp_names_who_decides_and_moves_when_that_changes() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    let by = f.session("worker");
    let first = stamp(&f, "sim/jump");
    assert!(
        first.deciders().is_empty(),
        "with nobody named, the owner alone"
    );
    f.store
        .add_person("ana", &[], &[Power::Decide], &by)
        .unwrap();
    let ana = stamp(&f, "sim/jump");
    assert_ne!(ana, first);
    assert_eq!(ana.deciders(), ["ana"]);
    f.store.add_group("staff", &[Power::Decide], &by).unwrap();
    f.store
        .add_person("bob", &["staff".into()], &[Power::Speak], &by)
        .unwrap();
    assert_eq!(
        stamp(&f, "sim/jump").deciders(),
        ["ana", "bob"],
        "by bob's group"
    );
    f.store
        .revoke(Holder::Person("ana"), &[Power::Decide], &by)
        .unwrap();
    let revoked = stamp(&f, "sim/jump");
    assert_eq!(
        revoked.deciders(),
        ["bob"],
        "a revoke holds at the next read"
    );
    f.store
        .add_person("cleo", &[], &[Power::Speak], &by)
        .unwrap();
    let cleo = stamp(&f, "sim/jump");
    assert_ne!(cleo, revoked);
    assert_eq!(cleo.deciders(), ["bob"]);
    assert_eq!(stamp(&f, "sim/jump"), cleo, "and it stands");
}

#[test]
fn a_slug_no_task_has_has_no_stamp() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    assert_eq!(f.store.task_stamp("sim/missing").unwrap(), None);
}

#[test]
fn a_task_made_under_a_subtask_leaves_the_stamp_of_the_task_above_it() {
    let f = Fixture::new(Seed::default());
    f.task("a");
    f.task_with(under("a/b", "a"));
    let (top, middle) = (stamp(&f, "a"), stamp(&f, "a/b"));
    f.task_with(under("a/b/c", "a/b"));
    assert_eq!(stamp(&f, "a"), top, "only the tasks right under it");
    assert_ne!(stamp(&f, "a/b"), middle);
}

#[test]
fn a_tasks_stamp_stands_until_the_task_or_its_thread_changes() {
    let f = Fixture::new(Seed::default());
    f.task("sim/jump");
    f.task("sim/other");
    let worker = f.session("worker");
    let first = stamp(&f, "sim/jump");
    assert_eq!(stamp(&f, "sim/jump"), first, "nothing changed");

    // Another task's thread and roles are not this one's.
    f.store
        .say("sim/other", &worker, "elsewhere", None)
        .unwrap();
    f.store.claim("sim/other", &worker).unwrap();
    assert_eq!(stamp(&f, "sim/jump"), first);

    // Each of these changes what `get` or `thread` gives: a post, a role, a status, a recorded
    // branch that writes no line, a task made under it.
    let mut last = first;
    let mut moved = |what: &str| {
        let now = stamp(&f, "sim/jump");
        assert_ne!(now, last, "{what} moves the stamp");
        assert_eq!(stamp(&f, "sim/jump"), now, "and it stands after {what}");
        last = now;
    };
    f.store.say("sim/jump", &worker, "a post", None).unwrap();
    moved("a post");
    let executor = f.session("worker");
    f.store.claim("sim/jump", &executor).unwrap();
    moved("a claim");
    f.store.move_to("sim/jump", "review", &executor).unwrap();
    moved("a move");
    let before = f.said("sim/jump");
    f.store
        .record_launch(&NewLaunch {
            branch: "sim/jump-2".into(),
            log: false,
            ..NewLaunch::new("sim/jump", &executor, "executor")
        })
        .unwrap();
    assert_eq!(f.said("sim/jump"), before, "the launch wrote no line");
    assert_eq!(
        f.store.get("sim/jump").unwrap().branch.as_deref(),
        Some("sim/jump-2")
    );
    moved("a branch recorded with no line");
    f.task_with(under("sim/jump/work", "sim/jump"));
    moved("a task made under it");
}
