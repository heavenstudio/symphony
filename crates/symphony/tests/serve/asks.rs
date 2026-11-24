//! The asks through `serve`: each call, its answer, and its refusals by kind.

use serde_json::{Value, json};

use super::common::Tree;
use super::{call, ok};

#[test]
fn an_ask_and_its_answer_come_back_as_the_app_reads_a_thread() {
    let tree = Tree::new();
    let db = tree.path("asks/asks.db");
    let id = ok(
        json!({"fn": "asks_ask", "db": db, "path": "docs/A.md", "question": "Why <five>?",
                       "anchor": "bands", "quote": "Five bands."}),
    );
    assert_eq!(id, json!(1));
    ok(
        json!({"fn": "asks_answer", "db": db, "id": 1, "text": "See **this**.", "by": "amber-heron"}),
    );
    let thread = ok(json!({"fn": "asks_thread", "db": db, "id": 1}));
    for (field, value) in [
        ("id", json!(1)),
        ("path", json!("docs/A.md")),
        ("anchor", json!("bands")),
        ("quote", json!("Five bands.")),
        ("question", json!("Why <five>?")),
        ("status", json!("answered")),
    ] {
        assert_eq!(thread[field], value, "{field}: {thread}");
    }
    assert!(
        thread["asked"].is_f64() && thread["updated"].is_f64(),
        "{thread}"
    );
    let messages = thread["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2, "{thread}");
    assert_eq!(
        (
            &messages[1]["kind"],
            &messages[1]["by"],
            &messages[1]["text"]
        ),
        (
            &json!("answer"),
            &json!("amber-heron"),
            &json!("See **this**.")
        )
    );
    assert!(
        messages[1]["seq"].is_i64() && messages[1]["at"].is_f64(),
        "{thread}"
    );
    assert_eq!(thread["answers"][0]["by"], "amber-heron", "{thread}");
}

#[test]
fn the_reads_the_app_makes_list_threads_and_questions() {
    let tree = Tree::new();
    let db = tree.path("asks.db");
    ok(
        json!({"fn": "asks_ask", "db": db, "path": "docs/A.md", "question": "here", "anchor": "", "quote": ""}),
    );
    ok(
        json!({"fn": "asks_ask", "db": db, "path": "docs/B.md", "question": "there", "anchor": "", "quote": ""}),
    );
    ok(json!({"fn": "asks_reply", "db": db, "id": 1, "text": "and again"}));
    let questions = |doc: &str| -> Vec<Value> {
        ok(json!({"fn": "asks_for_doc", "db": db, "path": doc}))
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["question"].clone())
            .collect()
    };
    assert_eq!(questions("docs/B.md"), [json!("there")]);
    assert_eq!(
        ok(json!({"fn": "asks_all", "db": db}))
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let since = ok(json!({"fn": "asks_questions_since", "db": db, "seq": 0}));
    let texts: Vec<&Value> = since
        .as_array()
        .unwrap()
        .iter()
        .map(|q| &q["text"])
        .collect();
    assert_eq!(
        texts,
        [&json!("here"), &json!("there"), &json!("and again")]
    );
    assert_eq!(
        ok(
            json!({"fn": "asks_watch_line", "ask": 1, "path": "docs/A.md", "anchor": "bands", "text": "ping\nnow"})
        ),
        json!("ASK 1 docs/A.md#bands — ping now")
    );
    assert_eq!(
        ok(json!({"fn": "asks_default_path", "root": tree.root, "asks_db": "/x/y.db"})),
        json!("/x/y.db")
    );
}

#[test]
fn a_refusal_is_an_ask_error_and_a_store_that_cannot_open_is_a_store_error() {
    let tree = Tree::new();
    let db = tree.path("asks.db");
    let refused = call(
        json!({"fn": "asks_ask", "db": db, "path": "../secret.md", "question": "hi", "anchor": "", "quote": ""}),
    );
    assert_eq!(refused["error"], "ask", "{refused}");
    let missing = call(json!({"fn": "asks_reply", "db": db, "id": 42, "text": "hi"}));
    assert_eq!(missing["error"], "ask", "{missing}");
    assert!(
        missing["message"]
            .as_str()
            .unwrap()
            .starts_with("no ask 42"),
        "{missing}"
    );
    tree.write("file", "");
    let broken = call(json!({"fn": "asks_all", "db": tree.path("file/asks.db")}));
    assert_eq!(broken["error"], "store", "{broken}");
}

#[test]
fn a_persons_ask_and_follow_up_are_theirs() {
    let tree = Tree::new();
    let db = tree.path("asks/asks.db");
    let id = ok(
        json!({"fn": "asks_ask", "db": db, "path": "docs/A.md", "question": "Why five?", "anchor": "", "quote": "", "by": "ana"}),
    );
    ok(json!({"fn": "asks_reply", "db": db, "id": id, "text": "And six?", "by": "ana"}));
    let thread = ok(json!({"fn": "asks_thread", "db": db, "id": id}));
    let said: Vec<(&str, &str)> = (thread["messages"].as_array().unwrap().iter())
        .map(|m| (m["kind"].as_str().unwrap(), m["by"].as_str().unwrap()))
        .collect();
    assert_eq!(said, [("question", "ana"), ("question", "ana")], "{thread}");
}
