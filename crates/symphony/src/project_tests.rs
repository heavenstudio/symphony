//! A project's read again for a plugin's process ([`Project::of`]): one read at a time, on a
//! thread of its own, its claim held until it ends, every reader keeping the plain project
//! meanwhile, and what it read kept only while the file is the one it was started for. A file's
//! route found with no process asked ([`Project::route_in`]).

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{AGAIN, FILE, Project, SEEN, lock};
use crate::test_tree::Tree;

/// A project whose plugin `slow` answers from the program `bin/slow`, an entrypoint over `models/`
/// at `/models/` showing its type, so the project loads only once it declares.
const SLOW: &str = "[plugins]\ncrates = [\"slow\"]\n\n\
                    [plugins.process]\nslow = [\"./bin/slow\"]\n\n\
                    [[entrypoints]]\nurl = \"/models/\"\nfolder = \"models\"\n\
                    types = [\"markdown\", \"slow\"]\n";

/// When the project at `root` is read again next, as its last read left it.
fn again(root: &Path) -> Option<Instant> {
    lock(&SEEN).get(root).and_then(|held| held.again)
}

/// What the project at `root` was last read from, as a read again started now is started for.
fn started(root: &Path) -> Vec<(PathBuf, super::Stamp)> {
    lock(&SEEN)[root].stamps.clone()
}

/// `text` written as the program at `path` under `tree`, which may run it.
fn program(tree: &Tree, path: &str, text: &str) {
    let program = tree.write(path, text);
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Waits until `holds`, with the backstop of ten seconds.
fn until(what: &str, holds: impl Fn() -> bool) {
    test_time::until_true(what, test_time::backstop(Duration::from_secs(10)), holds)
        .unwrap_or_else(|stall| panic!("{stall}"));
}

#[test]
fn a_read_again_holds_its_claim_until_it_ends_and_no_reader_waits_on_it() {
    let tree = Tree::new();
    tree.write(FILE, SLOW);
    tree.write("models/a.md", "# A\n");
    // Its program is missing at the first read: the plain project, read again a pause after.
    let first = Project::of(&tree.root);
    assert!(first.plugins.is_empty());
    let read_at = Instant::now();
    assert!(again(&tree.root).is_some_and(|at| at <= read_at + AGAIN));
    // Its program now: it marks its start and waits for `go`, then ends with nothing declared, so
    // a read of it lasts as long as the test says, past any pause.
    program(
        &tree,
        "bin/slow",
        "#!/bin/sh\necho started >> started\nwhile [ ! -e go ]; do sleep 0.05; done\n",
    );
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

/// A file's route is its entrypoint's URL and its path in the folder, which no declaration changes:
/// while no entrypoint names a type only a plugin's process handles, it is found with no process
/// started; once one does, the project is read as [`Project::of`] reads it, which asks the process.
#[test]
fn a_route_is_found_with_no_process_started_unless_an_entrypoint_names_its_type() {
    let tree = Tree::new();
    // Its program marks each start and ends at once, declaring nothing.
    program(&tree, "bin/gone", "#!/bin/sh\necho started >> started\n");
    let file = |types: &str| {
        format!(
            "[plugins]\ncrates = [\"gone\"]\n\n[plugins.process]\ngone = [\"./bin/gone\"]\n\n\
             [[entrypoints]]\nurl = \"/m/\"\nfolder = \"models\"\ntypes = [{types}]\n"
        )
    };
    tree.write(FILE, &file("\"markdown\""));
    tree.write("models/a.md", "# A\n");
    assert_eq!(
        Project::route_in(&tree.root, "models/a.md").as_deref(),
        Some("m/a.md")
    );
    assert_eq!(Project::route_in(&tree.root, "notes/b.md"), None);
    assert!(!tree.path("started").exists(), "no process was started");
    // Its type named: the process is asked, says nothing, and the project is refused, so the file
    // has the plain project's route, none outside docs/.
    tree.write(FILE, &file("\"markdown\", \"gone\""));
    assert_eq!(Project::route_in(&tree.root, "models/a.md"), None);
    assert_eq!(tree.read("started"), "started\n");
    // A project naming no process is read as Project::of reads it.
    tree.write(
        FILE,
        "[[entrypoints]]\nurl = \"/n/\"\nfolder = \"models\"\ntypes = [\"markdown\"]\n",
    );
    assert_eq!(
        Project::route_in(&tree.root, "models/a.md").as_deref(),
        Some("n/a.md")
    );
}
