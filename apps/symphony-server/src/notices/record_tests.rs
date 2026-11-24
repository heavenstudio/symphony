//! The record written whole and read back, none before it is made, and a place that cannot hold it
//! said by its path, leaving no part written.

use super::*;
use crate::testing::Tree;

fn pending(handing: bool) -> Pending {
    Pending {
        id: "a1".into(),
        kind: "card".into(),
        link: "http://studio:8765/symphony/docs/a.md#one".into(),
        person: "ana".into(),
        way: "slack".into(),
        digest: "d".repeat(64),
        hands: 1,
        due: 60.0,
        handing,
    }
}

#[test]
fn a_record_is_written_whole_its_folder_made_and_read_back() {
    let tree = Tree::new();
    let path = tree.path("artifacts/deep/notices.json");
    assert_eq!(Record::read(&path), Ok(None), "none before it is made");
    let record = Record {
        line: 12,
        cards: [("docs/a.md#one".to_string(), ["ana".to_string()].into())].into(),
        pending: vec![pending(false), pending(true)],
    };
    record.write(&path).unwrap();
    assert_eq!(Record::read(&path), Ok(Some(record)));
    let text = tree.read("artifacts/deep/notices.json");
    assert_eq!(
        text.matches("\"handing\"").count(),
        1,
        "kept only while true: {text}"
    );
    let left: Vec<_> = std::fs::read_dir(tree.path("artifacts/deep"))
        .unwrap()
        .collect();
    assert_eq!(left.len(), 1, "no temporary file stays");
    // A record of before `cards` and `pending` reads with neither.
    tree.write("old.json", "{\"line\": 3}");
    assert_eq!(
        Record::read(&tree.path("old.json")).unwrap().unwrap().line,
        3
    );
}

#[test]
fn a_place_that_cannot_hold_the_record_is_said_by_its_path_and_leaves_nothing() {
    let tree = Tree::new();
    std::fs::create_dir_all(tree.path("artifacts/notices.json")).unwrap();
    let path = tree.path("artifacts/notices.json");
    let why = Record::read(&path).unwrap_err();
    assert!(why.contains("notices.json"), "{why}");
    let why = Record::default().write(&path).unwrap_err();
    assert!(why.contains("notices.json"), "{why}");
    let left: Vec<_> = std::fs::read_dir(tree.path("artifacts")).unwrap().collect();
    assert_eq!(left.len(), 1, "its temporary file is removed");
    tree.write("file", "");
    let why = Record::default()
        .write(&tree.path("file/notices.json"))
        .unwrap_err();
    assert!(why.contains("file"), "{why}");
    tree.write("bad.json", "{\"line\": \"three\"}");
    assert!(
        Record::read(&tree.path("bad.json"))
            .unwrap_err()
            .contains("bad.json")
    );
}

#[test]
fn every_id_is_new() {
    let ids: std::collections::BTreeSet<_> = (0..200).map(|_| new_id()).collect();
    assert_eq!(ids.len(), 200);
    assert!(
        ids.iter()
            .all(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
    );
}
