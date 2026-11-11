//! The project's builds found by their links and the search path, in temporary checkouts.

use super::*;
use crate::test_tree::Tree;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;

fn program(tree: &Tree, rel: &str) -> PathBuf {
    let path = tree.write(rel, "#!/bin/sh\n");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A main checkout at `main/` and a linked worktree of it at `linked/`, with the markers git writes.
fn checkouts(tree: &Tree) -> (PathBuf, PathBuf) {
    std::fs::create_dir_all(tree.path("main/.git/worktrees/linked")).unwrap();
    tree.write("main/.git/worktrees/linked/commondir", "../..\n");
    tree.write(
        "linked/.git",
        &format!(
            "gitdir: {}\n",
            tree.path("main/.git/worktrees/linked").display()
        ),
    );
    (
        tree.path("main").canonicalize().unwrap(),
        tree.path("linked").canonicalize().unwrap(),
    )
}

#[test]
fn a_checkouts_runnable_link_is_its_build() {
    let tree = Tree::new();
    assert_eq!(link(&tree.root, "tasks"), None);
    tree.write(".target/bin/tasks", "not runnable\n");
    assert_eq!(link(&tree.root, "tasks"), None);
    let built = program(&tree, ".target/bin/tasks");
    assert_eq!(link(&tree.root, "tasks"), Some(built));
    std::fs::create_dir_all(tree.path(".target/bin/symphony")).unwrap();
    assert_eq!(link(&tree.root, "symphony"), None, "a folder never runs");
}

#[test]
fn a_link_to_a_build_is_followed_and_a_dangling_one_is_none() {
    let tree = Tree::new();
    let build = program(&tree, "out/tasks-1");
    std::fs::create_dir_all(tree.path(".target/bin")).unwrap();
    std::os::unix::fs::symlink(&build, tree.path(".target/bin/tasks")).unwrap();
    assert_eq!(
        link(&tree.root, "tasks"),
        Some(tree.path(".target/bin/tasks"))
    );
    std::fs::remove_file(&build).unwrap();
    assert_eq!(link(&tree.root, "tasks"), None);
}

#[test]
fn a_linked_worktree_without_a_link_runs_its_main_checkouts() {
    let tree = Tree::new();
    let (main, linked_tree) = checkouts(&tree);
    assert_eq!(linked(&linked_tree, "tasks"), None);
    program(&tree, "main/.target/bin/tasks");
    let mains = main.join(".target/bin/tasks");
    assert_eq!(linked(&linked_tree, "tasks"), Some(mains.clone()));
    assert_eq!(linked(&main, "tasks"), Some(mains));
    program(&tree, "linked/.target/bin/tasks");
    assert_eq!(
        linked(&linked_tree, "tasks"),
        Some(linked_tree.join(".target/bin/tasks")),
        "its own link wins"
    );
}

#[test]
fn a_main_checkout_or_a_folder_outside_git_has_only_its_own_link() {
    let tree = Tree::new();
    let (main, _) = checkouts(&tree);
    assert_eq!(main_checkout(&main), None);
    assert_eq!(main_checkout(&tree.root), None);
    tree.write("separate/.git", "gitdir: ../elsewhere\n");
    std::fs::create_dir_all(tree.path("elsewhere")).unwrap();
    assert_eq!(
        main_checkout(&tree.path("separate")),
        None,
        "a git dir not named .git names no main checkout"
    );
}

#[test]
fn the_search_path_is_read_in_order_and_skips_what_does_not_run() {
    let tree = Tree::new();
    tree.write("first/tasks", "not runnable\n");
    let second = program(&tree, "second/tasks");
    program(&tree, "third/tasks");
    let path = std::env::join_paths([
        PathBuf::new(),
        tree.path("first"),
        tree.path("second"),
        tree.path("third"),
    ])
    .unwrap();
    assert_eq!(on_path("tasks", Some(&path)), Some(second));
    assert_eq!(on_path("agent-send", Some(&path)), None);
    assert_eq!(on_path("tasks", None), None);
    assert_eq!(on_path("tasks", Some(&OsString::new())), None);
}

#[test]
fn the_link_wins_over_the_search_path() {
    let tree = Tree::new();
    let on = program(&tree, "path/tasks");
    let path = std::env::join_paths([tree.path("path")]).unwrap();
    assert_eq!(find(&tree.root, "tasks", Some(&path)), Some(on));
    let linked_build = program(&tree, ".target/bin/tasks");
    assert_eq!(find(&tree.root, "tasks", Some(&path)), Some(linked_build));
    assert_eq!(find(&tree.root, "symphony", Some(&path)), None);
}
