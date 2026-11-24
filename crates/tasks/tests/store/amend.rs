//! `amend`: a task's new title and further document links in one write, as the Symphony's verdict
//! roll-up keeps its count and pages current. The real store and a scratch database for each case.

use super::common::{Fixture, Seed, refused};
use symphony_board::NewTask;

#[test]
fn a_new_title_is_recorded_with_both_titles_and_links_are_added_once() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    let mut expected = f.task_with(NewTask {
        links: vec!["docs/A.md".into()],
        ..NewTask::new("verdicts/one", "Answer 1 owner verdict", "owner")
    });
    let before = f.said("verdicts/one").len();
    f.advance(1.0);
    expected.title = "Answer 2 owner verdicts".into();
    expected.links = vec!["@pic~x/docs/B.md".into(), "docs/A.md".into()];
    let links = ["docs/A.md".to_string(), "@pic~x/docs/B.md".to_string()];
    let amended = f
        .store
        .amend(
            "verdicts/one",
            " Answer 2 owner verdicts ",
            &links,
            "brisk-finch",
        )
        .unwrap();
    assert_eq!(amended, expected);
    assert_eq!(f.store.get("verdicts/one").unwrap(), expected);
    let thread = f.store.thread("verdicts/one").unwrap();
    assert_eq!(thread.len(), before + 1);
    let line = thread.last().unwrap();
    assert_eq!(
        (line.sender.as_str(), line.kind.as_str()),
        ("brisk-finch", "system")
    );
    assert_eq!(
        line.text,
        "brisk-finch retitled it: Answer 1 owner verdict → Answer 2 owner verdicts"
    );

    // The same title and links again: nothing more is said, and each link is still one.
    f.store
        .amend(
            "verdicts/one",
            "Answer 2 owner verdicts",
            &links,
            "brisk-finch",
        )
        .unwrap();
    assert_eq!(f.store.get("verdicts/one").unwrap(), expected);
    assert_eq!(f.said("verdicts/one").len(), before + 1);
}

#[test]
fn an_empty_title_a_missing_task_or_an_unknown_actor_changes_nothing() {
    let f = Fixture::new(Seed::default().session("brisk-finch"));
    f.task("verdicts/one");
    let before = f.rows();
    let links = ["docs/A.md".to_string()];
    let ghost = f.no_session("ghost-owl");
    for (slug, title, actor, why) in [
        ("verdicts/one", "  ", "brisk-finch", "a task needs a title"),
        (
            "verdicts/missing",
            "T",
            "brisk-finch",
            "no task 'verdicts/missing'",
        ),
        ("verdicts/one", "T", "ghost-owl", ghost.as_str()),
    ] {
        assert_eq!(refused(f.store.amend(slug, title, &links, actor)), why);
        assert_eq!(f.rows(), before);
    }
}
