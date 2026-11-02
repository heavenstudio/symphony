//! `DefaultPathTest`: where the store is, shared by every worktree like the task board.

use std::path::{Path, PathBuf};

use symphony::asks::default_path;

use super::common::Tree;

#[test]
fn the_variable_overrides() {
    assert_eq!(
        default_path(Path::new("."), Some("/x/y.db")),
        PathBuf::from("/x/y.db")
    );
}

#[test]
fn outside_a_repository_it_is_under_the_roots_artifacts() {
    let tree = Tree::new();
    assert_eq!(
        default_path(&tree.root, None),
        std::fs::canonicalize(&tree.root)
            .unwrap()
            .join("artifacts/asks/asks.db")
    );
}

#[test]
fn in_this_repo_it_is_under_the_main_checkouts_ignored_artifacts() {
    let path = default_path(Path::new(env!("CARGO_MANIFEST_DIR")), None);
    assert!(path.ends_with("artifacts/asks/asks.db"), "{path:?}");
    // the main checkout, which every worktree shares: its `.git` is the folder, where a linked
    // worktree's own is a file naming it
    let checkout = path.parent().unwrap().parent().unwrap().parent().unwrap();
    assert!(checkout.join(".git").is_dir(), "{path:?}");
}
