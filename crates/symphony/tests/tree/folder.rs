//! TreeTest: a folder is a page listing what is under it, and every page shows its way back up.

use symphony::page::EntryKind;
use symphony::render::Render;
use symphony::tree::{Crumb, count_documents, crumbs, index_order, is_folder};

use super::docs;

fn names(entries: &[symphony::page::Entry]) -> Vec<&str> {
    entries.iter().map(|e| e.path.as_str()).collect()
}

#[test]
fn a_folder_lists_its_subfolders_then_its_documents_by_title_and_summary() {
    let tree = docs();
    let folder = Render::new(&tree.root).folder("docs").unwrap();
    assert_eq!(names(&folder.entries), ["docs/lamps", "docs/SPEC.md"]);
    assert_eq!(folder.entries[0].kind, EntryKind::Folder);
    assert_eq!(folder.entries[0].summary, "Every lamp, by maker."); // a subfolder is summarised by its README
    assert_eq!(folder.entries[1].kind, EntryKind::Doc);
    assert_eq!(folder.entries[1].title, "The game");
    assert_eq!(folder.entries[1].summary, "What the game is, in one place.");
}

#[test]
fn a_folder_shows_its_readme_as_the_intro_and_does_not_list_it_again() {
    let tree = docs();
    let folder = Render::new(&tree.root).folder("docs").unwrap();
    let intro = folder.intro.expect("the README is the intro");
    assert!(
        intro
            .sections
            .iter()
            .any(|s| s.html.contains("Start here."))
    );
    assert!(!names(&folder.entries).contains(&"docs/README.md"));
}

#[test]
fn a_folder_lists_only_documents_and_folders_that_hold_documents() {
    let tree = docs();
    let folder = Render::new(&tree.root).folder("docs").unwrap();
    assert!(
        !names(&folder.entries)
            .iter()
            .any(|n| n.contains("data.json") || n.ends_with("/empty"))
    );
}

#[test]
fn a_listed_document_shows_what_waits_on_the_owner() {
    let tree = docs();
    let folder = Render::new(&tree.root).folder("docs/lamps").unwrap();
    let east = folder
        .entries
        .iter()
        .find(|e| e.path == "docs/lamps/east.md")
        .unwrap();
    assert_eq!(east.waiting, 1);
    assert_eq!(east.title, "East lamps");
}

#[test]
fn a_document_without_a_heading_is_listed_by_its_file_name() {
    let tree = docs();
    let folder = Render::new(&tree.root).folder("docs/lamps/old").unwrap();
    assert_eq!(folder.entries[0].path, "docs/lamps/old/notes.md");
    assert_eq!(folder.entries[0].title, "notes");
    assert_eq!(
        folder.title, "Old",
        "a folder without a README: its name, as a title"
    );
}

#[test]
fn folders_outside_docs_are_not_listed() {
    let tree = docs();
    assert!(is_folder(&tree.root, "docs/lamps"));
    assert!(is_folder(&tree.root, "docs"));
    assert!(!is_folder(&tree.root, "artifacts"));
    assert!(!is_folder(&tree.root, "docs/../artifacts"));
    assert!(Render::new(&tree.root).folder("artifacts").is_err());
}

#[test]
fn a_document_shows_the_breadcrumbs_back_to_the_root() {
    let tree = docs();
    let found = crumbs(&tree.root, "docs/lamps/old/notes.md");
    let crumb = |name: &str, href: Option<&str>| Crumb {
        name: name.into(),
        href: href.map(Into::into),
    };
    assert_eq!(
        found,
        [
            crumb("docs", Some("/docs/")),
            crumb("Lamps", Some("/docs/lamps/")), // a folder crumb is named by its README
            crumb("Old", Some("/docs/lamps/old/")),
            crumb("notes", None),
        ]
    );
}

#[test]
fn a_folder_shows_the_breadcrumbs_to_itself() {
    let tree = docs();
    let found = crumbs(&tree.root, "docs/lamps/");
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].href.as_deref(), Some("/docs/"));
    assert_eq!(
        (found[1].name.as_str(), found[1].href.as_deref()),
        ("Lamps", None)
    );
}

#[test]
fn a_path_outside_docs_has_no_crumbs() {
    assert!(crumbs(&docs().root, "artifacts/x.png").is_empty());
}

#[test]
fn a_folders_children_are_the_cards_of_a_grid() {
    let tree = docs();
    assert_eq!(
        Render::new(&tree.root)
            .folder("docs")
            .unwrap()
            .entries
            .len(),
        2
    );
}

#[test]
fn a_listed_folder_counts_its_documents_but_not_its_readme() {
    let tree = docs();
    let render = Render::new(&tree.root);
    assert_eq!(render.folder("docs").unwrap().entries[0].docs, 2); // east.md and old/notes.md
    assert_eq!(render.folder("docs/lamps").unwrap().docs, 2);
    assert_eq!(render.folder("docs/lamps/old").unwrap().docs, 1);
}

#[test]
fn a_folder_page_says_one_count_of_documents_its_entry_and_the_home_say_too() {
    let tree = docs();
    tree.write("docs/history/old-spec.md", "# The old game\n");
    let render = Render::new(&tree.root);
    let docs = render.folder("docs").unwrap();
    // SPEC.md, lamps/east.md and lamps/old/notes.md: not the READMEs, nor a page of history.
    assert_eq!(docs.docs, 3);
    assert_eq!(count_documents(&tree.root, "docs"), 3);
    let home = symphony::dashboard::snapshot(&tree.root, &[], "test").unwrap();
    assert_eq!(home.board.progress.docs, 3, "the home's browse all");
    let lamps = docs
        .entries
        .iter()
        .find(|e| e.path == "docs/lamps")
        .unwrap();
    assert_eq!(
        (lamps.docs, render.folder("docs/lamps").unwrap().docs),
        (2, 2),
        "a folder's entry and its own page"
    );
    let history = docs
        .entries
        .iter()
        .find(|e| e.path == "docs/history")
        .unwrap();
    assert_eq!(history.docs, 0);
    assert_eq!(
        count_documents(&tree.root, "artifacts"),
        0,
        "no folder page"
    );
}

#[test]
fn a_listed_folder_shows_the_cards_that_wait_in_the_documents_under_it() {
    let tree = docs();
    tree.write(
        "docs/lamps/old/west.md",
        "# West\n\n- [ ] **a** — first\n- [x] **b** — decided\n- [ ] **c** — second\n",
    );
    let render = Render::new(&tree.root);
    let lamps = &render.folder("docs").unwrap().entries[0];
    assert_eq!(lamps.path, "docs/lamps");
    assert_eq!(lamps.waiting, 3, "east.md's one and west.md's two");
    let old = &render.folder("docs/lamps").unwrap().entries[0];
    assert_eq!((old.path.as_str(), old.waiting), ("docs/lamps/old", 2));
}

#[test]
fn a_folder_without_a_readme_is_named_by_its_name_as_a_title() {
    let tree = docs();
    tree.write("docs/board-design/notes.md", "# Notes\n");
    tree.write("docs/machine_work/notes.md", "# Notes\n");
    let render = Render::new(&tree.root);
    let titles: Vec<_> = render
        .folder("docs")
        .unwrap()
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Folder)
        .map(|e| e.title.clone())
        .collect();
    assert_eq!(titles, ["Board design", "Lamps", "Machine work"]);
    assert_eq!(
        render.folder("docs/board-design").unwrap().title,
        "Board design"
    );
}

#[test]
fn a_folders_readme_sits_in_cards_like_a_document() {
    let tree = docs();
    tree.write(
        "docs/README.md",
        "# Lamp Works docs\n\nStart here.\n\n## Folders\n\nGame.\n",
    );
    let intro = Render::new(&tree.root)
        .folder("docs")
        .unwrap()
        .intro
        .unwrap();
    let parts: Vec<_> = intro.sections.iter().map(|s| s.part).collect();
    assert_eq!(
        parts.iter().flatten().max(),
        Some(&1),
        "two cards: {parts:?}"
    );
    let second = intro.sections.iter().find(|s| s.part == Some(1)).unwrap();
    assert!(second.html.contains("id=\"folders\""));
}

#[test]
fn the_index_order_is_the_readme_then_each_folder_then_the_documents_by_lowercase_title() {
    let tree = docs();
    tree.write("docs/Alpha.md", "# zeta first\n");
    tree.write("docs/B.md", "# Beta\n");
    tree.write("docs/Zoo/README.md", "# Zoo\n");
    assert_eq!(
        index_order(&tree.root, "docs"),
        [
            "docs/README.md",
            "docs/lamps/README.md",
            "docs/lamps/old/notes.md",
            "docs/lamps/east.md",
            "docs/Zoo/README.md",
            "docs/B.md",
            "docs/SPEC.md",
            "docs/Alpha.md",
        ]
    );
}
