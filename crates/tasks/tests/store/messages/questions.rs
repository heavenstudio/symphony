//! What the board asks of the owner beyond the cards: the questions agents asked them on tasks that
//! still wait on them (`questions_waiting`), and the newest reviewer's verdict (`newest_verdict`).

use super::texts;
use crate::common::{Fixture, Seed, task};
use symphony_board::rows::TaskRow;

fn open(slugs: &[&str]) -> Vec<String> {
    slugs.iter().map(|slug| (*slug).to_string()).collect()
}

fn waiting(f: &Fixture, slugs: &[&str]) -> Vec<String> {
    texts(f.store.questions_waiting("owner", &open(slugs)).unwrap())
}

#[test]
fn a_question_waits_until_the_owner_posts_on_its_thread_after_it() {
    let f = Fixture::new(Seed::default());
    let worker = f.session("worker");
    f.task("sim/jump");
    for text in ["which yield curve?", "and the drift?"] {
        f.store
            .say("sim/jump", &worker, text, Some("owner"))
            .unwrap();
    }
    assert_eq!(
        waiting(&f, &["sim/jump"]),
        ["which yield curve?", "and the drift?"],
        "oldest first"
    );
    f.store
        .say("sim/jump", "owner", "flat until spent", None)
        .unwrap();
    assert_eq!(
        waiting(&f, &["sim/jump"]),
        Vec::<String>::new(),
        "one post answers every question before it"
    );
    f.store
        .say("sim/jump", &worker, "one more: the cap?", Some("owner"))
        .unwrap();
    assert_eq!(waiting(&f, &["sim/jump"]), ["one more: the cap?"]);
}

#[test]
fn only_an_agents_lines_to_the_person_on_the_tasks_named_wait_on_them() {
    let f = Fixture::new(Seed::default());
    let worker = f.session("worker");
    for slug in ["sim/jump", "sim/dock"] {
        f.task(slug);
    }
    f.store
        .say("sim/jump", &worker, "to everyone", None)
        .unwrap();
    f.store
        .say(
            "sim/jump",
            &worker,
            "to the orchestrator",
            Some("@orchestrator"),
        )
        .unwrap();
    f.store
        .say("sim/jump", "owner", "the owner's own note", Some("owner"))
        .unwrap();
    f.store
        .say("sim/dock", &worker, "on the other task", Some("owner"))
        .unwrap();
    assert_eq!(waiting(&f, &["sim/jump"]), Vec::<String>::new());
    assert_eq!(
        waiting(&f, &["sim/jump", "sim/dock"]),
        ["on the other task"]
    );
    assert_eq!(
        waiting(&f, &[]),
        Vec::<String>::new(),
        "no open task, no question"
    );
    // The owner's post on one thread answers nothing on another.
    f.store.say("sim/jump", "owner", "noted", None).unwrap();
    assert_eq!(
        waiting(&f, &["sim/jump", "sim/dock"]),
        ["on the other task"]
    );
    assert_eq!(
        texts(
            f.store
                .questions_waiting("someone-else", &open(&["sim/dock"]))
                .unwrap()
        ),
        Vec::<String>::new(),
        "each person's own"
    );
}

#[test]
fn the_newest_verdict_is_the_newest_reviewers_approve_or_changes() {
    // `sim/jump`, and `sim/jump/m`, a slice of it in progress that `amber-heron` executes.
    let f = Fixture::new(
        Seed::default()
            .session("amber-heron")
            .task("sim/jump", "ready")
            .task_row(TaskRow {
                id: 2,
                parent: Some("sim/jump".into()),
                ..task("sim/jump/m", "in_progress")
            })
            .role("sim/jump/m", "amber-heron", "executor"),
    );
    assert_eq!(f.store.newest_verdict().unwrap(), None, "a board with none");
    let executor = f.session("worker");
    f.store.claim("sim/jump", &executor).unwrap();
    f.store.move_to("sim/jump", "review", &executor).unwrap();
    let reviewer = f.session("reviewer");
    f.store
        .take_role("sim/jump", &reviewer, "reviewer")
        .unwrap();
    f.store
        .verdict("sim/jump", &reviewer, "changes", "the drift is off")
        .unwrap();
    let changes = f.store.newest_verdict().unwrap().unwrap();
    assert_eq!(
        (
            changes.slug.as_str(),
            changes.sender.as_str(),
            changes.text.as_str()
        ),
        ("sim/jump", reviewer.as_str(), "changes: the drift is off")
    );
    assert_eq!(symphony_board::reviewer_verdict(&changes), Some("changes"));
    f.store.move_to("sim/jump", "review", &executor).unwrap();
    f.store
        .verdict("sim/jump", &reviewer, "approve", "")
        .unwrap();
    let approve = f.store.newest_verdict().unwrap().unwrap();
    assert_eq!(approve.text, "approve");
    assert!(approve.id > changes.id);
    // Lines after it that are no reviewer's verdict leave it the newest: a slice's `done:`, a verdict
    // line but no review's, and a message that reads like one.
    f.store
        .done("sim/jump/m", "amber-heron", "integrated in 3e0c661")
        .unwrap();
    f.store
        .say("sim/jump", &reviewer, "approve: said as a message", None)
        .unwrap();
    assert_eq!(f.store.newest_verdict().unwrap(), Some(approve));
}

#[test]
fn a_persons_post_answers_the_question_to_the_owner_and_a_sessions_unmarked_line_does_not() {
    let f = Fixture::new(Seed::default());
    let worker = f.session("worker");
    f.task("sim/jump");
    f.task("sim/dock");
    for slug in ["sim/jump", "sim/dock"] {
        f.store
            .say(
                slug,
                &worker,
                &format!("which curve on {slug}?"),
                Some("owner"),
            )
            .unwrap();
    }
    f.other()
        .for_person("ana")
        .say("sim/jump", "ana", "flat", None)
        .unwrap();
    assert_eq!(
        waiting(&f, &["sim/jump", "sim/dock"]),
        ["which curve on sim/dock?"],
        "her word answers it as the owner's would"
    );
    // A line the board did not mark is a session's, whatever its handle's shape: it answers
    // nothing.
    let other = f.session("worker");
    f.store.say("sim/dock", &other, "flat", None).unwrap();
    assert_eq!(waiting(&f, &["sim/dock"]), ["which curve on sim/dock?"]);
}
