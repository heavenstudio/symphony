//! A checkout that holds none of its entrypoints' folders: the server reads it as a project with no
//! documents, its pages say how to add them, and the CLI's board still refuses it by name.

use symphony::board::{documents, scan};
use symphony::docview;
use symphony::project::Project;
use symphony::render::Render;

use super::Tree;

const SAID: &str = "This project has no documents yet: create a docs/ folder with a Markdown file \
                    in it, or name the folder that holds them in symphony.toml's [[entrypoints]].";

#[test]
fn a_checkout_without_the_docs_folder_has_no_documents_and_its_pages_say_how_to_add_them() {
    let tree = Tree::new();
    tree.write(
        "README.md",
        "# Not a document\n\n- [ ] **outside** — No entrypoint's.\n",
    );
    let project = Project::of(&tree.root);
    assert_eq!(project.missing_documents(&tree.root), Some("docs"));
    assert_eq!(project.no_documents(&tree.root).as_deref(), Some(SAID));

    assert!(documents(&tree.root).unwrap().is_empty());
    let refused = scan(&tree.root).unwrap_err().0;
    assert_eq!(
        refused,
        format!("no docs/ directory under {}", tree.root.display())
    );

    let snapshot = symphony::dashboard::snapshot(&tree.root, &[], "test").unwrap();
    assert_eq!(snapshot.board.counts.waiting, 0);
    assert_eq!(snapshot.board.progress.docs, 0);
    assert!(snapshot.queue.waiting.is_empty() && snapshot.queue.agent.is_empty());

    let render = Render::new(&tree.root);
    assert_eq!(
        docview::folder(&render, "docs/", "", &[]).unwrap_err(),
        SAID
    );
    assert_eq!(docview::folder(&render, "docs", "", &[]).unwrap_err(), SAID);
    assert_eq!(
        docview::document(&render, "docs/README.md", "", &[]).unwrap_err(),
        SAID
    );
    // A route no entrypoint serves is not found, documents or none.
    assert_eq!(
        docview::target(&tree.root, "README.md", true).unwrap_err(),
        "not found: README.md"
    );
}

#[test]
fn once_the_folder_is_there_its_documents_are_read_and_a_missing_one_is_not_found() {
    let tree = Tree::new();
    tree.write(
        "docs/TOPIC.md",
        "# Topic\n\n- [ ] **first** — A question.\n",
    );
    let project = Project::of(&tree.root);
    assert_eq!(project.missing_documents(&tree.root), None);
    assert_eq!(project.no_documents(&tree.root), None);
    let read = documents(&tree.root).unwrap();
    assert_eq!(read, scan(&tree.root).unwrap());
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].path, "docs/TOPIC.md");

    let render = Render::new(&tree.root);
    assert_eq!(
        docview::document(&render, "docs/GONE.md", "", &[]).unwrap_err(),
        "not found: docs/GONE.md"
    );
    assert_eq!(docview::folder(&render, "docs/", "", &[]).unwrap().docs, 1);
}

#[test]
fn a_project_names_the_first_of_its_entrypoints_folders_and_has_documents_while_any_is_there() {
    let tree = Tree::new();
    tree.write(
        "symphony.toml",
        "[[entrypoints]]\nurl = \"/spec/\"\nfolder = \"spec\"\n\n\
         [[entrypoints]]\nurl = \"/notes/\"\nfolder = \"notes\"\n",
    );
    let project = Project::of(&tree.root);
    assert_eq!(project.missing_documents(&tree.root), Some("spec"));
    assert_eq!(
        project.no_documents(&tree.root).as_deref(),
        Some(
            "This project has no documents yet: create a spec/ folder with a Markdown file in \
             it, or name the folder that holds them in symphony.toml's [[entrypoints]]."
        )
    );

    tree.write("notes/N.md", "# Note\n");
    let project = Project::of(&tree.root);
    assert_eq!(project.missing_documents(&tree.root), None);
    assert_eq!(project.no_documents(&tree.root), None);
    assert_eq!(
        documents(&tree.root)
            .unwrap()
            .iter()
            .map(|doc| doc.path.as_str())
            .collect::<Vec<_>>(),
        ["notes/N.md"]
    );
}
