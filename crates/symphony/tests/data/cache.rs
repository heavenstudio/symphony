use crate::common::Tree;
use serde_json::json;
use std::fs::{File, FileTimes};
use std::os::unix::fs::MetadataExt;
use symphony::{State, board, schema, text};

#[test]
fn idle_file_caches_follow_same_size_rewrites_with_a_restored_mtime() {
    let before = "# Topic\n\n- [ ] **cache-card** — alpha.\n";
    let after = "# Topic\n\n- [x] **cache-card** — bravo.\n";
    assert_eq!(before.len(), after.len());
    for atomic in [false, true] {
        let tree = Tree::new();
        let path = tree.write("docs/topic.md", before);
        assert_eq!(text::read_text(&path).unwrap(), before);
        assert_eq!(
            board::scan(&tree.root).unwrap()[0].decisions[0].state,
            State::Pending
        );
        let original = std::fs::metadata(&path).unwrap();
        let replacement = if atomic {
            tree.write("replacement", after)
        } else {
            std::fs::write(&path, after).unwrap();
            path.clone()
        };
        File::open(&replacement)
            .unwrap()
            .set_times(FileTimes::new().set_modified(original.modified().unwrap()))
            .unwrap();
        if atomic {
            std::fs::rename(replacement, &path).unwrap();
        }
        let changed = std::fs::metadata(&path).unwrap();
        assert_eq!(changed.modified().unwrap(), original.modified().unwrap());
        assert_eq!(changed.len(), original.len());
        assert_eq!(changed.ino() == original.ino(), !atomic);
        assert_eq!(text::read_text(&path).unwrap(), after);
        let docs = board::scan(&tree.root).unwrap();
        assert_eq!(docs[0].decisions[0].state, State::Approved);
        assert_eq!(docs[0].decisions[0].text, "bravo.");
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            text::read_text(&path).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        assert!(board::scan(&tree.root).unwrap().is_empty());
    }
}

#[test]
fn idle_cached_text_does_not_hide_a_later_decoding_error() {
    let tree = Tree::new();
    let path = tree.write("docs/topic.md", "readable");
    assert_eq!(text::read_text(&path).unwrap(), "readable");
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, [0xff; 8]).unwrap();
    File::open(&path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(
        text::read_text(&path).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn idle_schema_cache_distinguishes_revisions_with_the_same_id() {
    let mut rule = json!({"$id":"urn:idle:rule", "type":"integer", "minimum":2});
    assert!(schema::validate(&json!(3), &rule).unwrap().is_empty());
    rule["minimum"] = json!(4);
    assert_eq!(
        schema::validate(&json!(3), &rule).unwrap(),
        ["$: must be at least 4"]
    );
    rule["minimum"] = json!(2);
    assert!(schema::validate(&json!(3), &rule).unwrap().is_empty());
    rule["minimumm"] = json!(5);
    assert!(schema::validate(&json!(3), &rule).is_err());
}
