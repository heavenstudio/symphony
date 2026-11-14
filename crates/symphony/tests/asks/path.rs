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

/// The store's place is the project's artifacts setting alone: the project is not loaded for it, so
/// no plugin's process starts, and one that cannot start leaves the store where the setting says.
#[test]
fn the_store_is_where_the_artifacts_setting_says_with_no_plugin_started() {
    let tree = Tree::new();
    tree.write(
        "symphony.toml",
        "[project]\nartifacts = \"out\"\n\n[plugins]\ncrates = [\"shelf\"]\n\n\
         [plugins.process]\nshelf = [\"./bin/shelf-plugin\"]\n\n\
         [[entrypoints]]\nurl = \"/models/\"\nfolder = \"models\"\ntypes = [\"shelf\"]\n",
    );
    assert_eq!(
        default_path(&tree.root, None),
        std::fs::canonicalize(&tree.root)
            .unwrap()
            .join("out/asks/asks.db")
    );
    assert!(!tree.path("out/plugins").exists());
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
