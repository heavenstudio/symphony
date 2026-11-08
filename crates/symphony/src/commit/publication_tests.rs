//! The publication journal's recovery: the locked file it adopts, and the ones it leaves.

use super::*;

#[test]
fn recovery_adopts_only_the_locked_inode_still_named_by_the_journal() {
    let tree = crate::test_tree::Tree::new();
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&tree.root),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    let path = tree.path("publication.json");
    let data = |new: &str| {
        serde_json::json!({"edit":{"rel":"docs/a.md","base":"","new":new},"publication":null})
            .to_string()
    };
    std::fs::write(&path, data("earlier")).unwrap();
    let old = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let next = tree.path("replacement");
    std::fs::write(&next, data("later")).unwrap();
    std::fs::rename(next, &path).unwrap();
    assert!(
        OutputJob::recover_opened(&tree.root, &path, old)
            .unwrap()
            .is_none()
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), data("later"));
    let current = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let adopted = OutputJob::recover_opened(&tree.root, &path, current)
        .unwrap()
        .unwrap();
    assert_eq!(adopted.saved.edit.new, "later");
    drop(adopted);
    let disappeared = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(
        OutputJob::recover_opened(&tree.root, &path, disappeared)
            .unwrap()
            .is_none()
    );
}
