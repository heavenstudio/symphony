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
        waits_on: None,
        review_rounds: None,
        from: None,
        workflow: None,
    };
    let board = Tasks {
        version: "fixture".into(),
        sessions: Vec::new(),
        orchestrators: Vec::new(),
        unstarted: 0,
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

/// Under its parent, a finished instruction to do or review the parent is no row of its own; a
/// slice, an errand and any open subtask are.
#[test]
fn only_a_finished_instruction_to_work_on_or_review_its_parent_is_left_out_under_it() {
    let staffing = [
        "feature/work",
        "feature/review",
        "feature/staff-review-1",
        "feature/staff-review-12",
        "feature/part/work",
        "work",
    ];
    for slug in staffing {
        for finished in ["done", "cancelled"] {
            assert!(
                !listed_under_parent(slug, true, finished),
                "{slug} {finished}"
            );
        }
        for open in [
            "concept",
            "design",
            "backlog",
            "ready",
            "in_progress",
            "review",
            "blocked",
        ] {
            assert!(listed_under_parent(slug, true, open), "{slug} {open}");
        }
        // A slice of that name is work on the parent's branch, listed whatever its status.
        assert!(listed_under_parent(slug, false, "done"), "slice {slug}");
    }
    let errands = [
        "feature/visual",
        "feature/merge-main",
        "feature/review-merge",
        "feature/rework",
        "feature/work-2",
        "feature/reviews",
        "feature/staff-review-",
        "feature/staff-review-x",
        "feature/staff-review-2b",
        "feature/work/notes",
    ];
    for slug in errands {
        for status in ["done", "cancelled", "ready"] {
            assert!(listed_under_parent(slug, true, status), "{slug} {status}");
        }
    }
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
        waits_on: None,
        review_rounds: None,
        from: None,
        workflow: None,
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

/// The activity list's `answered` and the questions still waiting on the owner
/// ([`symphony_board::Store::questions_waiting`]) are one rule, written in Rust here and in the
/// store's SQL: on one board, a line both read (an agent's message to the owner on an open
/// task) is answered in the list exactly when it is not among those waiting.
#[test]
fn the_activity_says_answered_of_exactly_the_owners_questions_no_longer_waiting() {
    use super::page_tests::store_fixture::{Fixture, NOW, Seed};
    use std::collections::BTreeSet;
    let line = |id: i32, slug: &str, sender: &str, to: Option<&str>, kind: &str| {
        symphony_board::rows::MessageRow {
            id: id.into(),
            slug: slug.into(),
            sender: sender.into(),
            recipient: to.map(str::to_owned),
            kind: kind.into(),
            text: format!("line {id}"),
            ts: NOW + f64::from(id),
        }
    };
    let (agent, owner, to_owner) = ("amber-heron", "owner", Some("owner"));
    let lines = vec![
        // One post of the owner's answers every question before it on its thread, none after it.
        line(1, "q/jump", agent, to_owner, "msg"),
        line(2, "q/jump", agent, to_owner, "msg"),
        line(3, "q/jump", owner, None, "msg"),
        line(4, "q/jump", agent, to_owner, "msg"),
        // A line of the owner's of any kind answers: a move they made.
        line(5, "q/dock", agent, to_owner, "msg"),
        line(6, "q/dock", owner, None, "system"),
        // One before the question, or after it on another thread, answers nothing.
        line(7, "q/cap", owner, None, "msg"),
        line(8, "q/cap", agent, to_owner, "msg"),
        line(9, "q/done", owner, None, "msg"),
        // Lines to the owner the questions leave out: a verdict, and a question on a done task.
        line(10, "q/cap", agent, to_owner, "verdict"),
        line(11, "q/done", agent, to_owner, "msg"),
    ];
    let mut seed = Seed::default()
        .task("q/jump", "in_progress")
        .task("q/dock", "review")
        .task("q/cap", "blocked")
        .task("q/done", "done");
    seed.dump.messages.clone_from(&lines);
    let f = Fixture::new(seed);
    let open: Vec<String> = ["q/jump", "q/dock", "q/cap"].map(String::from).into();

    let waiting: BTreeSet<i64> = (f.store.questions_waiting(owner, &open).unwrap().iter())
        .map(|question| question.id)
        .collect();
    assert_eq!(waiting, BTreeSet::from([4, 8]));
    let before = f.store.database().requests();
    assert_eq!(
        questions(&f.store, &open).unwrap(),
        BTreeMap::from([(owner.to_string(), 2)])
    );
    assert_eq!(f.store.database().requests() - before, PEOPLE.len());
    let before = f.store.database().requests();
    assert_eq!(
        questions(&f.store, &[]).unwrap(),
        BTreeMap::from([(owner.to_string(), 0)])
    );
    assert_eq!(f.store.database().requests(), before);
    let listed: BTreeMap<i64, bool> = (activity(&f.store).unwrap().iter())
        .map(|item| {
            let id = item.id.strip_prefix("task#").and_then(|id| id.parse().ok());
            (id.expect("a task message's item"), item.answered)
        })
        .collect();
    assert_eq!(
        listed.keys().copied().collect::<Vec<_>>(),
        [1, 2, 4, 5, 8, 10, 11],
        "the list holds every line to the owner"
    );
    let both: Vec<i64> = (lines.iter())
        .filter(|line| {
            line.recipient.as_deref() == to_owner
                && line.kind == "msg"
                && line.sender != owner
                && open.contains(&line.slug)
        })
        .map(|line| line.id)
        .collect();
    assert_eq!(both, [1, 2, 4, 5, 8]);
    for id in both {
        assert_eq!(
            listed[&id],
            !waiting.contains(&id),
            "line {id}: answered in the list, against waiting among the questions"
        );
    }
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

#[test]
fn stored_fields_keep_plugin_and_field_order_while_filtering_invalid_values() {
    use symphony_api::FieldValue;
    let stored = BTreeMap::from([
        ("z".into(), BTreeMap::new()),
        (
            "a".into(),
            BTreeMap::from([
                ("z".into(), serde_json::json!(2.5)),
                ("a".into(), serde_json::json!("text")),
                ("bad-object".into(), serde_json::json!({"nested": 1})),
                ("bad-null".into(), serde_json::Value::Null),
                ("bad-bool".into(), serde_json::json!(true)),
            ]),
        ),
    ]);
    let got = fields(&stored);
    assert_eq!(
        got.keys().map(String::as_str).collect::<Vec<_>>(),
        ["a", "z"]
    );
    assert_eq!(
        got["a"].keys().map(String::as_str).collect::<Vec<_>>(),
        ["a", "z"]
    );
    assert_eq!(got["a"]["a"], FieldValue::Text("text".into()));
    assert_eq!(got["a"]["z"], FieldValue::Number(2.5));
    assert!(got["z"].is_empty());
    assert!(fields(&BTreeMap::new()).is_empty());
}

/// A task's place in a custom workflow, as a page shows it: none for a task the board's own
/// writers keep or whose position this build cannot read, the resume pair a block saved, and the
/// person's recovery it waits on only where its action asks for one.
#[test]
fn a_tasks_place_is_its_position_as_a_page_shows_it() {
    use serde_json::json;
    use symphony_api::workflow::{ManualRecovery, Pair, TaskPlace};
    let fields = |board: serde_json::Value| {
        let mut stored = BTreeMap::new();
        stored.insert(
            "_board".to_string(),
            serde_json::from_value::<BTreeMap<String, serde_json::Value>>(board).unwrap(),
        );
        stored
    };
    assert_eq!(super::place(&BTreeMap::new()), None);
    assert_eq!(super::place(&fields(json!({"needs": "review"}))), None);
    let position = json!({"phase": "build", "state": "blocked", "generation": 2, "revision": 5,
        "digest": "d", "resume": {"phase": "build", "state": "working"}});
    assert_eq!(
        super::place(&fields(json!({"workflow": position}))),
        Some(TaskPlace {
            phase: "build".into(),
            state: "blocked".into(),
            resume: Some(Pair {
                phase: "build".into(),
                state: "working".into(),
            }),
            revision: 5,
            manual: None,
        })
    );
    let recovered = |manual: bool| {
        let mut position = position.clone();
        position["manual"] = json!({"trigger": "lost_holder", "holder": "amber", "role": "executor", "manual": manual});
        super::place(&fields(json!({"workflow": position}))).unwrap()
    };
    assert_eq!(
        recovered(true).manual,
        Some(ManualRecovery {
            trigger: "lost_holder".into(),
            holder: "amber".into(),
            role: "executor".into(),
        })
    );
    assert_eq!(recovered(false).manual, None);
    // A newer board's position, or a broken one, is no place: the page draws the board's own.
    let mut newer = position.clone();
    newer["attempt"] = json!("a-1");
    assert_eq!(super::place(&fields(json!({"workflow": newer}))), None);
    assert_eq!(super::place(&fields(json!({"workflow": "build"}))), None);
}

/// glacial-sparrow-263's post at 2026-11-20 00:54 on app/workflow-pages-follow-ups, as stored: two
/// JSX components named in angle brackets, which once drew a notice of unsupported content holding
/// the rest of the post, the rest again after it, and a notice for each attribute.
const TOKENS_POST: &str = "From app/token-cost-on-pages (glacial-sparrow-263), the hunks I hold in the task page and board files, so whoever lands second merges them. apps/symphony-web/src/task-page.tsx: useContext added to the react import; tokensPart added to the ./task-live import; a TokensPanel type and two components, TokensPart and TokensBody, just before WorkflowPart; one <TokensPart key=tokens-SLUG> line after the Changes section's NearViewport. apps/symphony-web/src/task-live.ts: TASK_TOKENS_PATH in the ./wire import, tokensPart() after timelinePart(). apps/symphony-web/src/task-diff.css: two .task-tokens rules at the end. Board: apps/symphony-web/src/boardwait.tsx (one import, one useCardCost line at WaitCard's top, a .card-cost span between the slug's code and .prio); apps/symphony-web/src/styles/boardwait.css (one .waiting-slug .card-cost rule before .waiting-slug .prio); apps/symphony-web/src/kinds/board.tsx (one import; TasksPage wrapped in <CardCostsOf key=scope scope>, the key moved onto it). board.tsx and boardlive.tsx: nothing. New: card-costs.ts. Say if any of it crosses what you hold.";

fn agent_post(text: &str) -> Message {
    Message {
        id: 16744,
        slug: "app/workflow-pages-follow-ups".into(),
        sender: "glacial-sparrow-263".into(),
        recipient: None,
        kind: "msg".into(),
        text: text.into(),
        ts: 1_795_132_459.79,
    }
}

#[test]
fn an_agents_post_naming_components_in_angle_brackets_is_drawn_as_typed_once() {
    let tree = Tree::new();
    let render = Render::new(&tree.root);
    assert_eq!(
        entry(&agent_post(TOKENS_POST), &render).blocks,
        [symphony_api::Block::Paragraph {
            spans: vec![symphony_api::Span::Text {
                text: TOKENS_POST.into()
            }]
        }]
    );
    // A verdict and a brief are an agent's Markdown too.
    let verdict = Message {
        kind: "verdict".into(),
        ..agent_post("changes: <Foo> and </Foo>")
    };
    assert_eq!(
        serde_json::to_value(entry(&verdict, &render).blocks).unwrap(),
        serde_json::json!([{ "type": "paragraph", "spans": [
            { "type": "text", "text": "changes: <Foo> and </Foo>" }
        ] }])
    );
    let brief = "Wrap it in <Foo>:\n\n<Foo/>\n\n- keep **this**\n";
    let (blocks, next) = brief_part(&render, brief, 0, BRIEF_LIMIT);
    assert_eq!(next, brief.len());
    let shown = serde_json::to_string(&blocks).unwrap();
    assert!(!shown.contains("unknown"), "{shown}");
    assert!(
        shown.contains(r#""text":"Wrap it in <Foo>:""#) && shown.contains(r#""text":"<Foo/>""#),
        "{shown}"
    );
    assert!(shown.contains(r#""type":"strong""#), "{shown}");
}
