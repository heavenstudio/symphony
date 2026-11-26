//! What every move of a registered board shares: its snapshot's name and the snapshots kept.

use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use super::*;
use crate::test_tree::Tree;

#[test]
fn a_snapshot_is_named_for_the_schema_and_release_it_was_taken_before_as_sql() {
    let folder = Path::new("/repo/.git/symphony/tasks");
    assert_eq!(
        snapshot_file(folder, 24, "release-3"),
        Path::new("/repo/.git/symphony/tasks/snapshots/before-schema-24-release-3.sql")
    );
}

#[test]
fn the_snapshots_kept_are_the_newest_and_nothing_else_is_touched() {
    let tree = Tree::new();
    let folder = tree.path("snapshots");
    fs::create_dir_all(&folder).unwrap();
    for n in 0..7 {
        let file = folder.join(format!("before-schema-{n}-release.sql"));
        fs::write(&file, "x").unwrap();
        let when = UNIX_EPOCH + Duration::from_secs(1_000 + n);
        File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }
    fs::write(folder.join("someone-else.sql"), "y").unwrap();
    prune(&folder, KEPT_SNAPSHOTS);
    let mut left = tree.names("snapshots");
    symphony_board::order::sort_by(&mut left, &Ord::cmp);
    assert_eq!(
        left,
        [
            "before-schema-2-release.sql",
            "before-schema-3-release.sql",
            "before-schema-4-release.sql",
            "before-schema-5-release.sql",
            "before-schema-6-release.sql",
            "someone-else.sql",
        ]
    );
    prune(&tree.path("none"), 5);
}
