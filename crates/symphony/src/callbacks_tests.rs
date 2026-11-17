//! Quick-chat publication counts, legacy migration and durable write failures.

use super::*;
use crate::asks::SQLITE_RUNS;
use crate::test_tree::Tree;
use serde_json::{Value, json};

#[test]
fn chat_persists_its_question_and_delivery_intent_in_three_sqlite_processes() {
    let tree = Tree::new();
    let path = tree.root.join("asks.db");
    for words in ["First question", "Follow up in another thread"] {
        SQLITE_RUNS.with(|runs| runs.set(0));
        let store = AskStore::open(&path).unwrap();
        let id = store.ask("docs/README.md", words, "", "").unwrap();
        let ask = symphony_board::NewAsk {
            kind: "chat".into(),
            thread: Some(id.to_string()),
            words: words.into(),
            ..symphony_board::NewAsk::default()
        };
        publish_at(&path, words, Some(&ask)).unwrap();
        assert_eq!(SQLITE_RUNS.with(|runs| runs.get()), 3);
        assert_eq!(store.thread(id).unwrap().question, words);
    }
    let rows: Vec<Value> = AskStore::existing(&path)
        .unwrap()
        .rows("SELECT id, text, ask FROM callbacks ORDER BY id;")
        .unwrap();
    assert_eq!(
        rows.iter().map(|r| r["text"].clone()).collect::<Vec<_>>(),
        [
            json!("First question"),
            json!("Follow up in another thread")
        ]
    );
    let ask: Value = serde_json::from_str(rows[1]["ask"].as_str().unwrap()).unwrap();
    assert_eq!(ask["thread"], "2");
}

#[test]
fn callback_publication_initializes_missing_and_legacy_outboxes_without_losing_records() {
    let tree = Tree::new();
    for legacy in [false, true] {
        let path = tree.root.join(if legacy { "legacy.db" } else { "new.db" });
        if legacy {
            let store = AskStore::open(&path).unwrap();
            store.run("DROP TABLE callbacks; CREATE TABLE callbacks (id INTEGER PRIMARY KEY AUTOINCREMENT, text TEXT NOT NULL); INSERT INTO callbacks(text) VALUES ('old');").unwrap();
        }
        publish_at(&path, "new", None).unwrap();
        let rows: Vec<Value> = AskStore::existing(&path)
            .unwrap()
            .rows("SELECT text, ask FROM callbacks ORDER BY id;")
            .unwrap();
        assert_eq!(
            rows,
            if legacy {
                vec![
                    json!({"text":"old","ask":null}),
                    json!({"text":"new","ask":null}),
                ]
            } else {
                vec![json!({"text":"new","ask":null})]
            }
        );
    }
}

#[test]
fn real_journal_failures_are_not_retried_as_schema_setup() {
    let tree = Tree::new();
    let path = tree.root.join("asks.db");
    std::fs::write(&path, "not a SQLite database").unwrap();
    SQLITE_RUNS.with(|runs| runs.set(0));
    let error = publish_at(&path, "keep this", None).unwrap_err();
    assert!(error.contains("not a database"), "{error}");
    assert_eq!(SQLITE_RUNS.with(|runs| runs.get()), 1);

    let path = tree.root.join("refused.db");
    let store = AskStore::open(&path).unwrap();
    store.run("CREATE TRIGGER refuse_callback BEFORE INSERT ON callbacks BEGIN SELECT RAISE(ABORT, 'journal write refused'); END;").unwrap();
    SQLITE_RUNS.with(|runs| runs.set(0));
    let error = publish_at(&path, "keep this too", None).unwrap_err();
    assert!(error.contains("journal write refused"), "{error}");
    assert_eq!(SQLITE_RUNS.with(|runs| runs.get()), 1);
    let rows: Vec<Value> = store.rows("SELECT text FROM callbacks;").unwrap();
    assert!(rows.is_empty());
    store.run("DROP TRIGGER refuse_callback;").unwrap();
    publish_at(&path, "keep this too", None).unwrap();
    let rows: Vec<Value> = store.rows("SELECT text FROM callbacks;").unwrap();
    assert_eq!(rows, [json!({"text":"keep this too"})]);
}

#[test]
fn concurrent_legacy_publishers_keep_each_durable_intent() {
    let tree = Tree::new();
    let path = tree.root.join("asks.db");
    let store = AskStore::open(&path).unwrap();
    store.run("DROP TABLE callbacks; CREATE TABLE callbacks (id INTEGER PRIMARY KEY AUTOINCREMENT, text TEXT NOT NULL);").unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let one = scope.spawn(|| {
            barrier.wait();
            publish_at(&path, "one", None)
        });
        let two = scope.spawn(|| {
            barrier.wait();
            publish_at(&path, "two", None)
        });
        one.join().unwrap().unwrap();
        two.join().unwrap().unwrap();
    });
    let rows: Vec<Value> = store
        .rows("SELECT text, ask FROM callbacks ORDER BY text;")
        .unwrap();
    assert_eq!(
        rows,
        [
            json!({"text":"one","ask":null}),
            json!({"text":"two","ask":null})
        ]
    );
}

#[test]
fn insertable_but_malformed_outboxes_never_acknowledge_an_intent() {
    let tree = Tree::new();
    for (case, columns) in [
        ("missing-id", "text TEXT NOT NULL, ask TEXT"),
        (
            "reused-id",
            "id INTEGER PRIMARY KEY, text TEXT NOT NULL, ask TEXT",
        ),
        (
            "nullable-text",
            "id INTEGER PRIMARY KEY AUTOINCREMENT, text TEXT, ask TEXT",
        ),
        (
            "wrong-ask",
            "id INTEGER PRIMARY KEY AUTOINCREMENT, text TEXT NOT NULL, ask INTEGER",
        ),
        ("legacy-bad-id", "id TEXT PRIMARY KEY, text TEXT NOT NULL"),
    ] {
        let path = tree.root.join(format!("{case}.db"));
        let store = AskStore::open(&path).unwrap();
        store.run(&format!("DROP TABLE callbacks; CREATE TABLE callbacks ({columns}); INSERT INTO callbacks(text) VALUES ('existing');")).unwrap();
        let error = publish_at(&path, "must remain unacknowledged", None).unwrap_err();
        assert!(error.contains("callbacks needs"), "{case}: {error}");
        assert!(error.contains(path.to_str().unwrap()), "{case}: {error}");
        let rows: Vec<Value> = store.rows("SELECT text FROM callbacks;").unwrap();
        assert_eq!(rows, [json!({"text":"existing"})], "{case}");
    }
}

#[test]
fn an_outbox_with_extra_columns_remains_compatible_and_receipts_do_not_reuse_ids() {
    let tree = Tree::new();
    let path = tree.root.join("extended.db");
    let store = AskStore::open(&path).unwrap();
    store
        .run("ALTER TABLE callbacks ADD COLUMN future TEXT;")
        .unwrap();
    publish_at(&path, "first", None).unwrap();
    let first: Vec<Value> = store.rows("SELECT id FROM callbacks;").unwrap();
    store.run("DELETE FROM callbacks;").unwrap();
    publish_at(&path, "second", None).unwrap();
    let second: Vec<Value> = store.rows("SELECT id FROM callbacks;").unwrap();
    assert!(second[0]["id"].as_i64().unwrap() > first[0]["id"].as_i64().unwrap());

    let blocked = tree.root.join("blocked");
    std::fs::write(&blocked, "not a directory").unwrap();
    let error = publish_at(&blocked.join("asks.db"), "retain this", None).unwrap_err();
    assert!(
        !error.contains("callbacks needs"),
        "I/O must not become schema readiness: {error}"
    );
}
