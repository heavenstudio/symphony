//! Task counts and brief rendering retain their task-page boundaries.

use super::*;
use crate::test_tree::Tree;

#[test]
fn project_task_counts_exclude_children_and_grandchildren() {
    let task = |slug: &str, status: &str, parent: Option<&str>| Task {
        slug: slug.into(),
        title: slug.into(),
        body: String::new(),
        status: status.into(),
        priority: 2,
        parent: parent.map(String::from),
        created_by: "owner".into(),
        created: 0.0,
        updated: 0.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: Vec::new(),
        instruction: false,
        fields: BTreeMap::new(),
    };
    let board = Tasks {
        version: "fixture".into(),
        sessions: Vec::new(),
        tasks: vec![
            task("feature", "in_progress", None),
            task("reviewed", "review", None),
            task("finished", "done", None),
            task("feature/implementation", "in_progress", Some("feature")),
            task("feature/check", "review", Some("feature")),
            task("feature/check/detail", "review", Some("feature/check")),
        ],
    };
    assert_eq!(
        counts(&board),
        TaskCounts {
            in_progress: 1,
            in_review: 1,
            agents_live: 0
        }
    );
}

#[test]
fn a_questions_task_is_open_until_it_is_done_or_cancelled() {
    let task = |slug: &str, status: &str| Task {
        slug: slug.into(),
        title: slug.into(),
        body: String::new(),
        status: status.into(),
        priority: 2,
        parent: None,
        created_by: "owner".into(),
        created: 0.0,
        updated: 0.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: Vec::new(),
        instruction: false,
        fields: BTreeMap::new(),
    };
    let tasks: Vec<Task> = [
        ("a", "backlog"),
        ("b", "in_progress"),
        ("c", "done"),
        ("d", "review"),
        ("e", "cancelled"),
        ("f", "blocked"),
    ]
    .iter()
    .map(|(slug, status)| task(slug, status))
    .collect();
    assert_eq!(open(&tasks), ["a", "b", "d", "f"]);
    assert_eq!(open(&[]), Vec::<String>::new());
}

#[test]
fn a_reviewers_verdict_is_news_for_every_person_and_any_other_line_is_none() {
    let line = |kind: &str, text: &str| symphony_board::Message {
        id: 6240,
        slug: "tools/x".into(),
        sender: "slate-wren".into(),
        recipient: Some("@executor".into()),
        kind: kind.into(),
        text: text.into(),
        ts: 1_763_213_045.25,
    };
    let given = verdict(&line("verdict", "changes: the hash moved")).unwrap();
    assert_eq!(
        given,
        TaskVerdict {
            id: 6240,
            task: "tools/x".into(),
            verdict: "changes".into(),
            reviewer: "slate-wren".into(),
            at: 1_763_213_045.25,
        }
    );
    assert_eq!(verdict(&line("verdict", "done: integrated")), None);
    assert_eq!(verdict(&line("msg", "approve")), None);
    let questions = BTreeMap::from([("owner".to_string(), 2)]);
    let asked = asks(questions.clone(), Some(&given));
    assert_eq!(asked.questions, questions);
    assert_eq!(
        asked.verdicts.keys().collect::<Vec<_>>(),
        PEOPLE.iter().collect::<Vec<_>>()
    );
    assert_eq!(asked.verdicts["owner"], given);
    assert!(
        asks(questions, None).verdicts.is_empty(),
        "no verdict yet, none for anyone"
    );
}

#[test]
fn a_brief_within_the_limit_is_its_whole_reply() {
    let tree = Tree::new();
    let render = Render::new(&tree.root);
    for body in [
        "One paragraph.",
        "# A heading\n\nA paragraph, and:\n\n- a list\n",
    ] {
        assert_eq!(
            brief_part(&render, body, 0, BRIEF_LIMIT),
            (reply_blocks(&render, body), body.len())
        );
    }
}
