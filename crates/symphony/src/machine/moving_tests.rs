//! What every move of a registered board shares: its snapshot's name, the snapshots kept and the
//! SurrealDB it never replaces.

use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use super::*;
use crate::test_tree::Tree;

#[test]
fn a_snapshot_is_named_for_the_schema_and_release_it_was_taken_before_in_its_engines_text() {
    let folder = Path::new("/repo/.git/symphony/tasks");
    assert_eq!(
        snapshot_file(folder, 23, "release-2", Engine::SurrealDb),
        Path::new("/repo/.git/symphony/tasks/snapshots/before-schema-23-release-2.surql")
    );
    assert_eq!(
        snapshot_file(folder, 24, "release-3", Engine::PostgreSql),
        Path::new("/repo/.git/symphony/tasks/snapshots/before-schema-24-release-3.sql")
    );
}

#[test]
fn the_snapshots_kept_are_the_newest_and_nothing_else_is_touched() {
    let tree = Tree::new();
    let folder = tree.path("snapshots");
    fs::create_dir_all(&folder).unwrap();
    for n in 0..7 {
        let file = folder.join(format!("before-schema-{n}-release.surql"));
        fs::write(&file, "x").unwrap();
        let when = UNIX_EPOCH + Duration::from_secs(1_000 + n);
        File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }
    fs::write(folder.join("someone-else.surql"), "y").unwrap();
    prune(&folder, KEPT_SNAPSHOTS);
    let mut left = tree.names("snapshots");
    left.sort();
    assert_eq!(
        left,
        [
            "before-schema-2-release.surql",
            "before-schema-3-release.surql",
            "before-schema-4-release.surql",
            "before-schema-5-release.surql",
            "before-schema-6-release.surql",
            "someone-else.surql",
        ]
    );
    prune(&tree.path("none"), 5);
}

#[test]
fn a_running_database_older_than_the_pins_is_one_the_move_would_replace() {
    assert!(older("3.2.9", "3.3.0"));
    assert!(older("v2.3.0", "3.0.0"));
    assert!(older("3.3", "3.3.1"));
    assert!(!older("3.3.0", "3.3.0"));
    assert!(!older("3.4.0", "3.3.9"));
    assert!(!older("3.3.0-beta", "3.3.0"), "a suffix is no older");
}

#[test]
fn a_snapshot_runs_the_surreal_beside_the_board_else_the_one_it_is_given() {
    let tree = Tree::new();
    let common = tree.path("repo/.git");
    let beside = Home::in_common(&common, None).surreal().to_path_buf();
    let given = tree.write("given/surreal", "");
    // SYMPHONY_SURREAL is the runner's own; only where it names none is the case read here.
    if symphony_env::SURREAL.os().is_none() {
        assert_eq!(surreal_for(&common, Some(&given)), given);
        assert_eq!(
            surreal_for(&common, None),
            beside,
            "none there: the pinned one's place"
        );
        fs::create_dir_all(beside.parent().unwrap()).unwrap();
        fs::write(&beside, "").unwrap();
        assert_eq!(surreal_for(&common, Some(&given)), beside);
    }
}
