//! What the gates are read from that needs no board: the catalogue kept by the main checkout's
//! head, whether a tip holds work of its own by the two tips, and a checkout's runs. The task and
//! its thread, kept by the task's stamp, are the server's `live::gates_tests`, over a board.

use super::*;
use crate::gates::tests::{git, run};
use crate::test_tree::Tree;
use symphony_board::cli::machine_work::recorded::Step;

/// A repository with one commit on `main`: that commit.
fn repository(tree: &Tree) -> String {
    git(&tree.root, &["init", "-q", "-b", "main"]);
    commit(tree, "a.txt", "a")
}

/// `text` committed as `file` on the branch checked out: the commit.
fn commit(tree: &Tree, file: &str, text: &str) -> String {
    tree.write(file, text);
    git(&tree.root, &["add", file]);
    git(&tree.root, &["commit", "-q", "-m", file]);
    git(&tree.root, &["rev-parse", "HEAD"])
}

#[test]
fn the_catalogue_is_read_once_a_commit_of_the_main_checkouts_head() {
    let tree = Tree::new();
    repository(&tree);
    let inputs = Inputs::default();
    let read = || (inputs.catalogue(&tree.root), inputs.reads().catalogues);
    assert_eq!(read(), (Ok(Catalogue::symphony()), 1));
    assert_eq!(read(), (Ok(Catalogue::symphony()), 1), "kept");

    // An edit not committed is not the catalogue, and moves no head: nothing is read.
    tree.write("symphony.toml", "[stamps.owner]\n");
    assert_eq!(read(), (Ok(Catalogue::symphony()), 1));

    // Committed, the head is another commit: read there, once.
    commit(&tree, "symphony.toml", "[stamps.owner]\n");
    let (written, reads) = read();
    assert_eq!(written.as_ref().map(Catalogue::names), Ok(vec!["owner"]));
    assert_eq!(reads, 2);
    assert_eq!(read(), (written, 2), "kept at the new head");

    // One that cannot be read is said so and not kept: the next read asks again.
    commit(&tree, "symphony.toml", "[stamps.lint]\n");
    let (refused, reads) = read();
    assert!(refused.unwrap_err().contains("lint"));
    assert_eq!(reads, 3);
    assert_eq!(read().1, 4);

    // Outside a repository the catalogue is the Symphony's own, and nothing is read.
    let outside = Tree::new();
    assert_eq!(inputs.catalogue(&outside.root), Ok(Catalogue::symphony()));
    assert_eq!(inputs.reads().catalogues, 4);
}

#[test]
fn whether_a_tip_holds_work_of_its_own_is_read_once_for_the_tip_and_the_default_branchs() {
    let tree = Tree::new();
    let made = repository(&tree);
    git(&tree.root, &["checkout", "-q", "-b", "tools/x"]);
    let own = commit(&tree, "x.txt", "x");
    git(&tree.root, &["checkout", "-q", "main"]);
    let inputs = Inputs::default();
    let holds = |slug: &str, tip: &str, default_tip: &str| {
        let own = inputs.own_commits(&tree.root, slug, tip, ("main", Some(default_tip)));
        (own, inputs.reads().merge_bases)
    };
    assert_eq!(holds("tools/x", &own, &made), (true, 1));
    assert_eq!(holds("tools/x", &own, &made), (true, 1), "kept");
    assert_eq!(
        holds("tools/y", &made, &made),
        (false, 2),
        "each task its own"
    );

    // The default branch moved: asked again, and it holds the tip now.
    git(&tree.root, &["merge", "-q", "--no-edit", "tools/x"]);
    let landed = git(&tree.root, &["rev-parse", "main"]);
    assert_eq!(holds("tools/x", &own, &landed), (false, 3));
    assert_eq!(holds("tools/x", &own, &landed), (false, 3));

    // A task that closed is forgotten; one still open is kept.
    inputs.keep_open(|slug| slug == "tools/y");
    assert_eq!(holds("tools/y", &made, &made), (false, 3));
    assert_eq!(holds("tools/x", &own, &landed), (false, 4));

    // With no default branch to compare, a tip holds its own, kept the same way.
    let unknown =
        |inputs: &Inputs| inputs.own_commits(&tree.root, "tools/z", &own, ("trunk", None));
    assert!(unknown(&inputs));
    assert!(unknown(&inputs));
    assert_eq!(inputs.reads().merge_bases, 5);
}

#[test]
fn a_checkouts_runs_are_its_work_folders_read_once_and_none_outside_a_repository() {
    let tree = Tree::new();
    let tip = repository(&tree);
    let inputs = Inputs::default();
    let kept = run(1, "just verify", "tools/x", &tip, Step::Finished, Some(0));
    let folder = crate::gates::work_folder(&tree.root).unwrap();
    recorded::keep(&folder, &kept).unwrap();
    let first = inputs.runs(&tree.root);
    assert_eq!(*first, [kept]);
    let again = inputs.runs(&tree.root);
    assert!(Arc::ptr_eq(&first, &again), "the list it gave");
    assert_eq!(
        inputs.reads(),
        Reads {
            histories: 1,
            ..Reads::default()
        }
    );
    assert!(inputs.runs(&Tree::new().root).is_empty());
    assert_eq!(inputs.reads().histories, 1, "no folder, nothing read");
}
