//! What the task board asks of each person: the questions waiting on the owner and the newest
//! reviewer's verdict, as the hub holds them for the project list and as its feed sends them, once
//! for each change.

use super::super::Hub;
use crate::testing::{
    Tree,
    board::{Fixture, Seed},
    repository,
};
use serde_json::{Value, json};

/// The hub of a fresh checkout, its main page read.
fn hub(tree: &Tree) -> Hub {
    let root = repository(tree);
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();
    hub
}

/// The task asks one observation of `store` sent on the main page's feed, each event's data.
fn observe(hub: &Hub, store: &symphony_board::Store) -> Vec<Value> {
    (changes(hub, store).into_iter())
        .filter(|change| change["region"] == "task-asks")
        .map(|change| change["data"].clone())
        .collect()
}

/// Every change one observation of `store` sent on the main page's feed.
fn changes(hub: &Hub, store: &symphony_board::Store) -> Vec<Value> {
    let last = |hub: &Hub| {
        hub.pages.read().unwrap()[""]
            .history
            .back()
            .map_or(0, |event| event.version)
    };
    let before = last(hub);
    hub.observe(store);
    let pages = hub.pages.read().unwrap();
    pages[""]
        .history
        .iter()
        .filter(|event| event.version > before)
        .flat_map(|event| {
            let value: Value = serde_json::from_str(&event.json).unwrap();
            value["changes"].as_array().unwrap().clone()
        })
        .collect()
}

fn waiting(questions: u32) -> Value {
    json!({"questions": {"owner": questions}, "verdicts": {}})
}

#[test]
fn a_question_to_the_owner_waits_until_the_owner_answers_or_its_task_is_cancelled() {
    let tree = Tree::new();
    let f = Fixture::new(
        Seed::default()
            .session("worker")
            .task("tools/ask", "in_progress")
            .task("tools/other", "in_progress")
            .role("tools/ask", "worker", "executor"),
    );
    let hub = hub(&tree);
    assert_eq!(
        hub.task_asks(),
        None,
        "before the board's first observation"
    );
    assert_eq!(observe(&hub, &f.store), [waiting(0)], "sent once");
    assert_eq!(observe(&hub, &f.store), Vec::<Value>::new(), "unchanged");
    f.store
        .say("tools/ask", "worker", "flat or falling?", Some("owner"))
        .unwrap();
    assert_eq!(observe(&hub, &f.store), [waiting(1)]);
    assert_eq!(hub.task_asks().unwrap().questions["owner"], 1);
    // The owner's word on another thread answers nothing here; a message to everyone asks nothing.
    f.store
        .say("tools/other", "owner", "carry on", None)
        .unwrap();
    f.store
        .say("tools/ask", "worker", "working on it", None)
        .unwrap();
    assert_eq!(observe(&hub, &f.store), Vec::<Value>::new());
    f.store.say("tools/ask", "owner", "flat", None).unwrap();
    assert_eq!(observe(&hub, &f.store), [waiting(0)], "answered");
    f.store
        .say("tools/ask", "worker", "and the cap?", Some("owner"))
        .unwrap();
    assert_eq!(observe(&hub, &f.store), [waiting(1)]);
    f.store.move_to("tools/ask", "cancelled", "worker").unwrap();
    assert_eq!(
        observe(&hub, &f.store),
        [waiting(0)],
        "a cancelled task's question waits no more"
    );
}

#[test]
fn the_newest_verdict_is_read_at_the_first_look_then_followed_and_moves_no_count() {
    let tree = Tree::new();
    let f = Fixture::new(
        Seed::default()
            .session("worker")
            .session("reviewer")
            .task("tools/review", "in_progress")
            .role("tools/review", "worker", "executor"),
    );
    f.store.move_to("tools/review", "review", "worker").unwrap();
    f.store
        .take_role("tools/review", "reviewer", "reviewer")
        .unwrap();
    let changes = f
        .store
        .verdict("tools/review", "reviewer", "changes", "the drift is off")
        .unwrap();
    assert_eq!(changes.status, "in_progress");
    let hub = hub(&tree);
    // On the board before the hub's first look: read then.
    let sent = observe(&hub, &f.store);
    assert_eq!(sent.len(), 1, "{sent:?}");
    let first = &sent[0]["verdicts"]["owner"];
    assert_eq!(
        [&first["task"], &first["verdict"], &first["reviewer"]],
        ["tools/review", "changes", "reviewer"]
    );
    assert_eq!(sent[0]["questions"], json!({"owner": 0}));
    let waiting_before = hub.pages.read().unwrap()[""].board.counts.waiting;

    // Given after it: followed from the lines the feed reads.
    f.store.move_to("tools/review", "review", "worker").unwrap();
    f.store
        .verdict("tools/review", "reviewer", "approve", "good")
        .unwrap();
    let sent = observe(&hub, &f.store);
    assert_eq!(sent.len(), 1, "{sent:?}");
    let newest = &sent[0]["verdicts"]["owner"];
    assert_eq!(newest["verdict"], "approve");
    assert!(
        newest["id"].as_i64() > first["id"].as_i64(),
        "{newest} after {first}"
    );
    assert_eq!(
        sent[0]["questions"],
        json!({"owner": 0}),
        "a verdict asks nothing of the owner"
    );
    assert_eq!(
        hub.pages.read().unwrap()[""].board.counts.waiting,
        waiting_before,
        "nor waits on them"
    );
    assert_eq!(
        serde_json::to_value(hub.task_asks().unwrap()).unwrap(),
        sent[0],
        "what the project list reads is what the feed sent"
    );
    // A line after it that is no reviewer's verdict changes nothing.
    f.store
        .say(
            "tools/review",
            "reviewer",
            "approve: said as a message",
            None,
        )
        .unwrap();
    assert_eq!(observe(&hub, &f.store), Vec::<Value>::new());
}

#[test]
fn a_board_with_no_verdict_and_no_question_says_so_for_the_owner() {
    let tree = Tree::new();
    let f = Fixture::new(Seed::default().session("worker"));
    let hub = hub(&tree);
    assert_eq!(observe(&hub, &f.store), [waiting(0)]);
    let asks = hub.task_asks().unwrap();
    assert_eq!(asks.questions["owner"], 0);
    assert!(asks.verdicts.is_empty());
}

#[test]
fn a_persons_line_goes_out_on_the_feed_marked_as_theirs_and_answers_as_the_owners_would() {
    let tree = Tree::new();
    let f = Fixture::new(
        Seed::default()
            .session("worker")
            .task("tools/ask", "in_progress")
            .role("tools/ask", "worker", "executor"),
    );
    let hub = hub(&tree);
    assert_eq!(observe(&hub, &f.store), [waiting(0)]);
    f.store
        .say("tools/ask", "worker", "flat or falling?", Some("owner"))
        .unwrap();
    assert_eq!(observe(&hub, &f.store), [waiting(1)]);
    let ana = f.other().for_person("ana");
    ana.say("tools/ask", "ana", "flat", None).unwrap();
    f.store.say("tools/ask", "worker", "on it", None).unwrap();
    let changes = changes(&hub, &f.store);
    let lines: Vec<_> = (changes.iter())
        .filter(|change| change["region"] == "message")
        .map(|change| {
            let line = &change["data"];
            (
                line["sender"].as_str(),
                line["person"].as_bool(),
                line["text"].as_str(),
            )
        })
        .collect();
    assert_eq!(
        lines,
        [
            (Some("ana"), Some(true), Some("flat")),
            (Some("worker"), Some(false), Some("on it"))
        ]
    );
    let asks: Vec<_> = (changes.iter())
        .filter(|change| change["region"] == "task-asks")
        .map(|change| change["data"].clone())
        .collect();
    assert_eq!(asks, [waiting(0)], "her word answers it");
}

#[test]
fn a_held_message_goes_out_with_the_boards_line_waits_on_the_owner_and_its_pass_answers_it() {
    let tree = Tree::new();
    let f = Fixture::new(
        Seed::default()
            .session("worker")
            .task("tools/ask", "in_progress")
            .role("tools/ask", "worker", "executor"),
    );
    let hub = hub(&tree);
    assert_eq!(observe(&hub, &f.store), [waiting(0)]);
    (f.store)
        .add_person("ana", &[], &[symphony_board::people::Power::Speak], "owner")
        .unwrap();
    let ana = f.other().for_person("ana");
    let words = "the header is cut off";
    let held = ana.hold_words("tools/ask", "ana", words, None).unwrap();
    let held_out = changes(&hub, &f.store);
    let heard = |changes: &[Value]| -> Vec<Value> {
        (changes.iter())
            .filter(|change| change["region"] == "message")
            .map(|change| change["data"].clone())
            .collect()
    };
    let lines = heard(&held_out);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["id"], held.id);
    assert_eq!(lines[0]["sender"], "board");
    assert!(!lines[0]["text"].as_str().unwrap().contains(words));
    assert_eq!(
        lines[0]["held"],
        json!({"id": held.id, "author": "ana", "text": words})
    );
    let asks: Vec<_> = (held_out.iter())
        .filter(|change| change["region"] == "task-asks")
        .map(|change| change["data"].clone())
        .collect();
    assert_eq!(asks, [waiting(1)], "it waits on the person at the machine");
    f.store.pass("tools/ask", held.id, "owner").unwrap();
    let passed_out = changes(&hub, &f.store);
    let lines = heard(&passed_out);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["sender"], "ana");
    assert_eq!(lines[0]["text"], format!("[passed on by owner] {words}"));
    assert_eq!(lines[0]["held"]["passed_by"], "owner");
    let asks: Vec<_> = (passed_out.iter())
        .filter(|change| change["region"] == "task-asks")
        .map(|change| change["data"].clone())
        .collect();
    assert_eq!(asks, [waiting(0)], "passing it on answers it");
}
