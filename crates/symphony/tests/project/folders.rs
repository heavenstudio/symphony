//! A project whose documents are in `manual/`, its history in `manual/past/` and its working output
//! in `output/`: every walk reads those folders, and each document is still named `docs/…`.

use symphony::board::{scan, tidy};
use symphony::lint::history_in_prose;
use symphony::project::{Project, ROUTE};

use super::Tree;

const MANUAL: &str = "[project]\ndocs = \"manual\"\nhistory = [\"manual/past\"]\n\
                      register = \"manual/DECISIONS.md\"\nartifacts = \"output\"\n";

/// The project on `manual/`, with a stray `docs/` folder its walks must not read.
fn manual() -> Tree {
    let tree = Tree::new();
    tree.write("symphony.toml", MANUAL);
    tree.write(
        "manual/DECISIONS.md",
        "# Decisions\n\n<!-- register:start -->\n### Topic\n\n- [ ] **pick** — Choose one.\n\
         <!-- register:end -->\n",
    );
    tree.write("manual/guide/START.md", "# Start\n\nAs pick says.\n");
    tree.write(
        "manual/past/OLD.md",
        "# Old\n\npick was decided.\n\n- [ ] **old** — Past.\n",
    );
    tree.write(
        "docs/STRAY.md",
        "# Stray\n\npick again.\n\n- [ ] **stray** — Not a document.\n",
    );
    tree
}

#[test]
fn a_document_is_named_by_its_route_and_its_route_by_the_projects_folder() {
    let project = Project::parse(std::path::Path::new("/p"), MANUAL).unwrap();
    assert_eq!(
        project.route("manual/guide/START.md").as_deref(),
        Some("docs/guide/START.md")
    );
    assert_eq!(project.route("manual").as_deref(), Some(ROUTE));
    for outside in ["docs/STRAY.md", "manualx/A.md", "view/manual/A.md"] {
        assert_eq!(project.route(outside), None, "{outside}");
    }
    assert_eq!(project.file("docs/guide/START.md"), "manual/guide/START.md");
    assert_eq!(project.file(ROUTE), "manual");
    for other in ["view/models/A.glb", "docsx/A.md", "artifacts/a.png"] {
        assert_eq!(project.file(other), other);
    }
    // the default project's folder is its route
    let plain = Project::parse(std::path::Path::new("/p"), "").unwrap();
    assert_eq!(plain.route("docs/A.md").as_deref(), Some("docs/A.md"));
    assert_eq!(plain.file("docs/A.md"), "docs/A.md");
}

#[test]
fn the_board_reads_the_documents_folder_and_skips_its_history() {
    let tree = manual();
    let docs = scan(&tree.root).unwrap();
    assert_eq!(
        docs.iter().map(|doc| doc.path.as_str()).collect::<Vec<_>>(),
        ["docs/DECISIONS.md", "docs/guide/START.md"]
    );
    assert_eq!(
        docs.iter()
            .flat_map(|doc| doc.decisions.iter().map(|card| card.slug.as_str()))
            .collect::<Vec<_>>(),
        ["pick"]
    );
}

#[test]
fn the_prose_lint_reads_each_document_from_the_projects_folder() {
    let tree = manual();
    assert_eq!(
        history_in_prose(&tree.root).unwrap(),
        [("docs/guide/START.md".to_string(), 3, "pick".to_string())]
    );
}

#[test]
fn a_tidy_files_answered_callouts_in_the_first_history_folder() {
    let tree = manual();
    tree.write(
        "manual/NOTES.md",
        "# N\n\n> [!comment] Odd\n> > [!resolved] Fixed.\n\nrest\n",
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = tidy(&tree.root, Some("2026-09-26 08:00"), &mut out, &mut err);
    let (out, err) = (
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    );
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("archived: manual/NOTES.md (1)"), "{out}");
    assert_eq!(tree.read("manual/NOTES.md"), "# N\n\nrest\n");
    let ledger = tree.read("manual/past/review-ledger.md");
    assert!(
        ledger.contains("## 2026-09-26 08:00 · manual/NOTES.md"),
        "{ledger}"
    );
    assert!(!tree.path("docs/history").exists());
    // a project with no history folder keeps its callouts where they are
    tree.write("symphony.toml", &MANUAL.replace("[\"manual/past\"]", "[]"));
    tree.write(
        "manual/MORE.md",
        "# M\n\n> [!comment] Odd\n> > [!resolved] Fixed.\n",
    );
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        tidy(&tree.root, Some("2026-09-26 09:00"), &mut out, &mut err),
        0
    );
    assert!(tree.read("manual/MORE.md").contains("[!comment] Odd"));
}

#[test]
fn uploads_are_stored_under_the_projects_artifacts_folder() {
    let tree = manual();
    assert_eq!(
        symphony::uploads::dir(&tree.root),
        tree.path("output/uploads")
    );
    let plain = Tree::new();
    assert_eq!(
        symphony::uploads::dir(&plain.root),
        plain.path("artifacts/uploads")
    );
}
