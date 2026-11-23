//! A project's read again is kept only while its file is the one it was started for.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use symphony_board::lock::Recovered as _;

use super::{FILE, Project, SEEN};
use crate::test_tree::Tree;

/// A project whose plugin `slow` answers from the program `bin/slow`, an entrypoint over `models/`
/// at `/models/` showing its type, so the project loads only once it declares.
const SLOW: &str = "[plugins]\ncrates = [\"slow\"]\n\n\
                    [plugins.process]\nslow = [\"./bin/slow\"]\n\n\
                    [[entrypoints]]\nurl = \"/models/\"\nfolder = \"models\"\n\
                    types = [\"markdown\", \"slow\"]\n";

/// What the project at `root` was last read from, as a read again started now is started for.
fn started(root: &Path) -> Vec<(PathBuf, super::Stamp)> {
    SEEN.lock().recover()[root].stamps.clone()
}

/// A read again that ends after a reader found the project's file changed, and read it afresh,
/// leaves that newer read in place, so no reader loads the project once more for it; one that ends
/// with the file still the one it was started for is kept.
#[test]
fn a_read_again_keeps_what_it_read_only_while_the_file_is_the_one_it_was_started_for() {
    let tree = Tree::new();
    tree.write(FILE, SLOW);
    tree.write("models/a.md", "# A\n");
    // Its program missing: the plain project, to be read again.
    let first = Project::of(&tree.root);
    assert!(Project::problem(&tree.root).is_some());
    // A read again started for that file reads it, still refused...
    let for_first = started(&tree.root);
    let late = Project::load_seen(&tree.root, None);
    assert!(late.problem.is_some());
    // ...and before it ends the file changes, and the next reader reads it afresh.
    tree.write(
        FILE,
        "[[entrypoints]]\nurl = \"/models/\"\nfolder = \"models\"\ntypes = [\"markdown\"]\n",
    );
    let changed = Project::of(&tree.root);
    assert!(!Arc::ptr_eq(&changed, &first));
    assert_eq!(Project::problem(&tree.root), None);
    // Its end leaves that read in place: every later reader is given it, with no load of its own.
    assert!(
        !Project::keep(&tree.root, &for_first, late),
        "a read of the file before is not kept"
    );
    assert!(Arc::ptr_eq(&Project::of(&tree.root), &changed));
    assert_eq!(Project::problem(&tree.root), None);
    // A read again that ends with the file as it was started for is kept, and given from then on.
    let for_changed = started(&tree.root);
    let current = Project::load_seen(&tree.root, None);
    let read = current.project.clone();
    assert!(Project::keep(&tree.root, &for_changed, current));
    assert!(Arc::ptr_eq(&Project::of(&tree.root), &read));
}
