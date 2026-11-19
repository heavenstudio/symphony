//! [`super`] where no repository is needed: what a journal entry's name and a kept failure's
//! words become, and what a folder that is no checkout answers.

use super::{Delivery, QueuedPublication, flat, kept, note, saves, sequence_of, shown};
use crate::test_tree::Tree;
use std::path::Path;

#[test]
fn a_journal_entrys_number_is_its_file_name() {
    let name = format!("{:020}.json", 42);
    assert_eq!(
        sequence_of(Path::new("journal").join(name).as_path()),
        Some(42)
    );
    assert_eq!(sequence_of(Path::new("journal/notes.json")), None);
    assert_eq!(sequence_of(Path::new("")), None);
}

#[test]
fn a_failures_words_stay_on_the_notes_one_line() {
    assert_eq!(
        flat("git exited\n  status 1\n\n> quoted"),
        "git exited status 1 > quoted"
    );
    assert_eq!(flat(""), "");
    // Cut at a word, within the limit, and said as cut.
    let long = flat(&"word ".repeat(100));
    assert_eq!(long, format!("{}…", "word ".repeat(39).trim_end()));
    assert!(long.chars().count() <= super::WHY_CHARS);
}

#[test]
fn an_entry_without_a_word_of_delivery_reads_as_none_and_writes_none() {
    let plain = r#"{"edit":{"rel":"docs/a.md","base":"a","new":"b"},"publication":null}"#;
    let saved: QueuedPublication = serde_json::from_str(plain).unwrap();
    assert!(saved.delivery.is_none());
    assert!(!serde_json::to_string(&saved).unwrap().contains("delivery"));
    // One a pass has not reached yet: every field at its default.
    let kept = format!(r#"{},"delivery":{{}}}}"#, plain.strip_suffix('}').unwrap());
    let saved: QueuedPublication = serde_json::from_str(&kept).unwrap();
    assert_eq!(saved.delivery, Some(Delivery::default()));
}

#[test]
fn a_folder_that_is_no_checkout_keeps_and_shows_nothing() {
    let tree = Tree::new();
    let root = tree.path("");
    assert!(!kept(&root));
    assert!(saves(&root).is_empty());
    assert_eq!(note(&root, "docs/a.md"), None);
    assert_eq!(shown(&root, "docs/a.md", "text".into()), "text");
}
