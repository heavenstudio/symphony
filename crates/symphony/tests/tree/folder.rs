//! TreeTest: a folder is a page listing what is under it, and every page shows its way back up.

use symphony::docview;
use symphony::page::EntryKind;
use symphony::render::Render;
use symphony::tree::{Crumb, count_documents, crumbs, index_order, is_folder, pending};

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

/// An HTML page in a documents folder is a document like any other under the convention: listed
/// beside the Markdown ones by its `<title>` (its file name's stem with none), counted among its
/// folder's documents, with no summary and nothing waiting, and named by its title on its way back
/// up; never in the queue's document order, which holds the pages that can carry cards.
#[test]
fn an_html_page_is_listed_beside_the_markdown_documents_by_its_title() {
    let tree = docs();
    tree.write(
        "docs/readout.html",
        "<!doctype html><html><head><TITLE lang=\"en\">\n  Client  <b>readout</b> &amp; more \
         </TITLE></head><body><h1>Not the title</h1><p>Not a summary.</p></body></html>",
    );
    tree.write("docs/lamps/bare.HTM", "<p>No title here.</p>");
    tree.write("docs/lamps/empty.html", "<title> </title>");
    // The title is read from the page's head alone: a large page's at its top is found, and one
    // that comes only after 16 KiB is not read for.
    tree.write(
        "docs/lamps/big.html",
        &format!("<title>Big page</title><p>{}</p>", "words ".repeat(50_000)),
    );
    tree.write(
        "docs/lamps/late.html",
        &format!("<!-- {} --><title>Too late</title>", "x".repeat(16 * 1024)),
    );
    tree.write("docs/lamps/picture.png", "not shown");
    tree.write("docs/only-html/page.html", "<title>Alone</title>");
    let before = index_order(&tree.root, "docs");
    let render = Render::new(&tree.root);
    let folder = render.folder("docs").unwrap();
    assert_eq!(
        names(&folder.entries),
        [
            "docs/lamps",
            "docs/only-html",
            "docs/readout.html",
            "docs/SPEC.md"
        ]
    );
    let readout = entry(&folder, "docs/readout.html");
    assert_eq!(
        (
            readout.kind,
            readout.title.as_str(),
            readout.summary.as_str(),
            readout.waiting
        ),
        (EntryKind::Doc, "Client readout & more", "", 0)
    );
    // SPEC.md, east.md, old/notes.md, readout.html, bare.HTM, empty.html, big.html, late.html and
    // page.html.
    assert_eq!((folder.docs, count_documents(&tree.root, "docs")), (9, 9));
    assert_eq!(entry(&folder, "docs/lamps").docs, 6);
    assert_eq!(entry(&folder, "docs/only-html").docs, 1);
    let lamps = render.folder("docs/lamps").unwrap();
    assert_eq!(
        lamps
            .entries
            .iter()
            .map(|e| (e.path.as_str(), e.title.as_str(), e.kind))
            .collect::<Vec<_>>(),
        [
            ("docs/lamps/old", "Old", EntryKind::Folder),
            ("docs/lamps/bare.HTM", "bare", EntryKind::Doc),
            ("docs/lamps/big.html", "Big page", EntryKind::Doc),
            ("docs/lamps/east.md", "East lamps", EntryKind::Doc),
            ("docs/lamps/empty.html", "empty", EntryKind::Doc),
            ("docs/lamps/late.html", "late", EntryKind::Doc),
        ],
        "titled by file name without one, and no picture under the convention"
    );
    assert_eq!(
        crumbs(&tree.root, "docs/readout.html").last().unwrap().name,
        "Client readout & more"
    );
    assert_eq!(index_order(&tree.root, "docs"), before, "no card is in one");
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

fn entry<'a>(folder: &'a symphony::page::Folder, path: &str) -> &'a symphony::page::Entry {
    folder.entries.iter().find(|e| e.path == path).unwrap()
}

#[test]
fn a_folder_page_says_one_count_of_documents_its_entry_and_the_home_say_too() {
    let tree = docs();
    tree.write("docs/history/README.md", "# History\n\nWhat was.\n");
    tree.write("docs/history/old-spec.md", "# The old game\n");
    tree.write("docs/history/2025/older-spec.md", "# The older game\n");
    let render = Render::new(&tree.root);
    let docs = render.folder("docs").unwrap();
    // SPEC.md, lamps/east.md and lamps/old/notes.md: not the READMEs, nor a page of history.
    assert_eq!(docs.docs, 3);
    assert_eq!(count_documents(&tree.root, "docs"), 3);
    let home = symphony::dashboard::snapshot(&tree.root, &[], "test").unwrap();
    assert_eq!(home.board.progress.docs, 3, "the home's browse all");
    assert_eq!(
        (
            entry(&docs, "docs/lamps").docs,
            render.folder("docs/lamps").unwrap().docs
        ),
        (2, 2),
        "a folder's entry and its own page"
    );
    // A history folder counts the pages it lists, on its card and on its own page alike.
    assert_eq!(
        (
            entry(&docs, "docs/history").docs,
            render.folder("docs/history").unwrap().docs
        ),
        (2, 2),
        "old-spec.md and 2025/older-spec.md, not its README"
    );
    assert_eq!(render.folder("docs/history/2025").unwrap().docs, 1);
    assert_eq!(
        count_documents(&tree.root, "artifacts"),
        0,
        "no folder page"
    );
}

/// A card of a queue, as a server holds it.
fn queued(doc: &str, slug: &str) -> symphony_api::Card {
    serde_json::from_value(
        serde_json::json!({"slug":slug,"doc":doc,"doc_title":"","title":"","why":""}),
    )
    .unwrap()
}

#[test]
fn a_listed_folder_says_the_queues_cards_under_it_the_sum_of_its_documents() {
    let tree = docs();
    // A card that waits, a question the owner asked that an agent answered (the owner's again,
    // though no longer pending on the page), and a card the owner commented on (the agent's,
    // though still unticked).
    tree.write(
        "docs/lamps/old/west.md",
        "# West\n\n- [ ] **a** — First.\n\
         - [?] **b** — A question.\n  [2026-10-20 10:00] owner · question — Why?\n  [2026-10-20 11:00] agent — Because.\n\
         - [ ] **c** — Third.\n  [2026-10-20 10:00] owner — Not this way.\n",
    );
    let page = std::fs::read_to_string(tree.path("docs/lamps/old/west.md")).unwrap();
    assert_eq!(
        pending(&page),
        ["a", "c"],
        "the file's own count is another"
    );
    let mut queue = symphony::dashboard::snapshot(&tree.root, &[], "test")
        .unwrap()
        .queue
        .waiting;
    let mut slugs: Vec<_> = queue.iter().map(|card| card.slug.clone()).collect();
    symphony_board::order::sort_by(&mut slugs, &Ord::cmp);
    assert_eq!(slugs, ["a", "b", "x"]);
    // A branch's proposal in a document main does not have yet, and a card in a folder whose
    // name only starts like lamps.
    queue.push(queued("@design~west/docs/lamps/old/north.md", "d"));
    queue.push(queued("docs/lampshades/shade.md", "e"));
    let render = Render::new(&tree.root);
    let waiting = |rel: &str, scope: &str, path: &str| {
        let mut folder = docview::folder(&render, rel, scope, &[]).unwrap();
        docview::count_waiting(&mut folder, &queue);
        let found = folder.entries.iter().find(|e| e.path.ends_with(path));
        found.unwrap().waiting
    };
    assert_eq!(
        waiting("docs/lamps/old", "", "docs/lamps/old/west.md"),
        2,
        "a and b: c waits on an agent"
    );
    assert_eq!(
        waiting("docs/lamps", "", "docs/lamps/old"),
        3,
        "west.md's two and the branch's d"
    );
    assert_eq!(waiting("docs/lamps", "", "docs/lamps/east.md"), 1);
    assert_eq!(
        waiting("docs", "", "docs/lamps"),
        4,
        "east.md's x, west.md's a and b, the branch's d; not e"
    );
    assert_eq!(
        waiting("docs", "design~west", "docs/lamps"),
        4,
        "a branch's page counts the same cards"
    );
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
