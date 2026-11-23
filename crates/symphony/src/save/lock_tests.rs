//! The write lock a document save shares with the lander, as the save takes it
//! ([`held_write_lock`]): free for every copy of its descriptor once the save drops it.

use std::fs::OpenOptions;

use super::*;

/// A temporary folder that is a repository on `main`, with nothing committed: a checkout whose
/// saves take the write lock.
fn repository() -> crate::test_tree::Tree {
    let tree = crate::test_tree::Tree::new();
    let done = symphony_board::sys::output(
        symphony_board::sys::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&tree.root),
    )
    .unwrap();
    assert!(done.status.success(), "{done:?}");
    tree
}

/// Whether a descriptor of its own takes the lock at `path` now.
fn free(path: &Path) -> bool {
    let file = OpenOptions::new().append(true).open(path).unwrap();
    file.try_lock().is_ok()
}

/// A copy of the write lock's descriptor kept past the save's drop, as a child another thread
/// spawned keeps one until its exec, holds nothing: the drop unlocks it, where a close alone would
/// release it only with its last copy.
#[test]
fn copies_of_the_write_locks_descriptor_kept_past_its_drop_hold_nothing() {
    let tree = repository();
    let lock =
        (tree.path(".git").join(symphony_board::folders::SERVER)).join("verdict-writes.lock");
    let held = held_write_lock(&tree.root)
        .unwrap()
        .expect("a repository's saves take the write lock");
    assert!(!free(&lock), "the save holds the write lock");
    let copy = held.0.try_clone().unwrap();
    drop(held);
    assert!(
        free(&lock),
        "a copy of its descriptor held it past the drop"
    );
    drop(copy);
    // Taken again once released: nothing of the first take is left held.
    drop(held_write_lock(&tree.root).unwrap());
}
