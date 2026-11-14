//! A project's read again for a plugin's process ([`Project::of`]): one read at a time, on a
//! thread of its own, its claim held until it ends, every reader keeping the plain project
//! meanwhile.

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{AGAIN, Project, SEEN, lock};
use crate::test_tree::Tree;

/// When the project at `root` is read again next, as its last read left it.
fn again(root: &std::path::Path) -> Option<Instant> {
    lock(&SEEN).get(root).and_then(|held| held.again)
}

/// Waits until `holds`, with the backstop of ten seconds.
fn until(what: &str, holds: impl Fn() -> bool) {
    test_time::until_true(what, test_time::backstop(Duration::from_secs(10)), holds)
        .unwrap_or_else(|stall| panic!("{stall}"));
}

#[test]
fn a_read_again_holds_its_claim_until_it_ends_and_no_reader_waits_on_it() {
    let tree = Tree::new();
    tree.write(
        super::FILE,
        "[plugins]\ncrates = [\"slow\"]\n\n[plugins.process]\nslow = [\"./bin/slow\"]\n\n\
         [[entrypoints]]\nurl = \"/models/\"\nfolder = \"models\"\ntypes = [\"markdown\", \"slow\"]\n",
    );
    tree.write("models/a.md", "# A\n");
    // Its program is missing at the first read: the plain project, read again a pause after.
    let first = Project::of(&tree.root);
    assert!(first.plugins.is_empty());
    let read_at = Instant::now();
    assert!(again(&tree.root).is_some_and(|at| at <= read_at + AGAIN));
    // Its program now: it marks its start and waits for `go`, then ends with nothing declared, so
    // a read of it lasts as long as the test says, past any pause.
    let program = tree.write(
        "bin/slow",
        "#!/bin/sh\necho started >> started\nwhile [ ! -e go ]; do sleep 0.05; done\n",
    );
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    // A pause after the first read, the read again is due, and the process may start again.
    until("a pause passed since the first read", || {
        Instant::now() >= read_at + AGAIN
    });
    // The reader that finds it due keeps the plain project: the read is on a thread of its own.
    let claimed = Instant::now();
    assert!(Arc::ptr_eq(&Project::of(&tree.root), &first));
    until("the read again started the plugin's program", || {
        tree.path("started").exists()
    });
    // While it waits on the program, its claim reaches past any pause, so no reader reads again or
    // waits: each keeps the plain project, and the program is started once.
    assert!(
        again(&tree.root).is_some_and(|at| at >= claimed + 10 * AGAIN),
        "the claim lasts the read, not a pause"
    );
    for _ in 0..10 {
        assert!(Arc::ptr_eq(&Project::of(&tree.root), &first));
    }
    assert_eq!(tree.read("started"), "started\n");
    // Its end sets the next read again, a pause after it, with its own plain project.
    tree.write("go", "");
    until("the read again ended", || {
        again(&tree.root).is_some_and(|at| at < claimed + 10 * AGAIN)
    });
    let after = Project::of(&tree.root);
    assert!(!Arc::ptr_eq(&after, &first));
    assert!(
        Project::problem(&tree.root)
            .unwrap()
            .contains("[plugins.process] slow cannot say its renderers: its process ")
    );
}
