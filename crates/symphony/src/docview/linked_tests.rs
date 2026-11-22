//! The tasks a document lists: those of the board the caller holds that link it, by its route or
//! by a route no file has that finds it by name ([`moved`]).

use super::{LinkNames, document, folder, linked, moved};
use crate::render::Render;
use crate::test_tree::Tree;
use symphony_api::Task;

fn task(slug: &str, links: &[&str]) -> Task {
    Task {
        slug: slug.into(),
        title: format!("The {slug} task"),
        body: String::new(),
        status: "ready".into(),
        priority: 0,
        parent: None,
        created_by: "owner".into(),
        created: 0.0,
        updated: 0.0,
        links: links.iter().map(|link| (*link).into()).collect(),
        depends: Vec::new(),
        roles: Vec::new(),
        instruction: false,
        fields: Default::default(),
        waits_on: None,
        review_rounds: None,
        from: None,
        workflow: None,
    }
}

fn project() -> Tree {
    let tree = Tree::new();
    tree.write("docs/README.md", "# Docs\n\nThe introduction.\n");
    tree.write("docs/TOPIC.md", "# Topic\n\nA topic.\n");
    tree.write("docs/OTHER.md", "# Other\n\nAnother topic.\n");
    tree.write(
        "docs/guide/TOPIC.md",
        "# Guide topic\n\nSame name, elsewhere.\n",
    );
    tree
}

fn slugs(tasks: &[symphony_api::DocumentTask]) -> Vec<&str> {
    tasks.iter().map(|task| task.slug.as_str()).collect()
}

#[test]
fn a_document_lists_the_tasks_that_link_it_by_its_route_or_a_heading_in_it() {
    let tree = project();
    let board = [
        task("tools/route", &["docs/TOPIC.md"]),
        task(
            "tools/heading",
            &["docs/OTHER.md", "docs/TOPIC.md#a-heading"],
        ),
        task("tools/other", &["docs/OTHER.md"]),
        task("tools/none", &[]),
        task("tools/elsewhere", &["docs/guide/TOPIC.md"]),
    ];
    let listed = linked(&tree.root, "docs/TOPIC.md", &board);
    assert_eq!(slugs(&listed), ["tools/route", "tools/heading"]);
    assert_eq!(listed[0].title, "The tools/route task");
    assert_eq!(listed[0].status, "ready");
    assert_eq!(
        slugs(&linked(&tree.root, "docs/guide/TOPIC.md", &board)),
        ["tools/elsewhere"],
        "a document of the same name in another folder is another document"
    );
    assert!(linked(&tree.root, "docs/TOPIC.md", &[]).is_empty());
}

#[test]
fn a_link_to_a_document_that_moved_finds_it_by_its_name() {
    let tree = project();
    let board = [
        task("tools/moved", &["docs/old/OTHER.md#a-heading"]),
        task("tools/named", &["docs/old/OTHER"]),
        task("tools/gone", &["docs/old/GONE.md"]),
        task("tools/exists", &["docs/OTHER.md"]),
        task("tools/nameless", &["#a-heading", "docs/.."]),
    ];
    assert_eq!(
        slugs(&linked(&tree.root, "docs/OTHER.md", &board)),
        ["tools/moved", "tools/named", "tools/exists"],
        "a link with no file name finds no document by name"
    );
    assert!(
        linked(&tree.root, "docs/TOPIC.md", &board).is_empty(),
        "a moved link finds only the document of its name"
    );
}

#[test]
fn a_moved_link_to_a_name_documents_share_is_listed_by_none_of_them() {
    let tree = project();
    let board = [task("tools/shared", &["docs/old/TOPIC.md#a-heading"])];
    for rel in ["docs/TOPIC.md", "docs/guide/TOPIC.md"] {
        assert!(
            linked(&tree.root, rel, &board).is_empty(),
            "{rel}: the page offers both, so neither claims it"
        );
    }
}

#[test]
fn a_route_no_file_has_finds_the_document_of_its_name_as_a_wikilink_does() {
    let tree = project();
    let found = |rel: &str| moved(&tree.root, rel);
    assert_eq!(found("docs/old/OTHER.md"), ["docs/OTHER.md"]);
    assert_eq!(
        found("docs/deep/old/OTHER"),
        ["docs/OTHER.md"],
        "`.md` added"
    );
    assert_eq!(
        found("docs/old/TOPIC.md"),
        ["docs/TOPIC.md", "docs/guide/TOPIC.md"],
        "a shared name finds each document of it, in the order find_doc tries them"
    );
    assert_eq!(
        found("docs/TOPIC"),
        ["docs/TOPIC.md"],
        "the route it spells from its own folder wins over the name's other documents"
    );
}

#[test]
fn a_route_a_file_has_a_name_no_document_has_or_no_name_finds_nothing() {
    let tree = project();
    tree.write("docs/pictures/lamp.png", "not a document");
    for rel in [
        "docs/OTHER.md",
        "docs/guide/TOPIC.md",
        "docs/pictures/lamp.png",
        "docs/old/GONE.md",
        "docs/..",
        "",
    ] {
        assert!(moved(&tree.root, rel).is_empty(), "{rel:?}");
    }
}

#[test]
fn the_documents_are_walked_only_for_a_link_only_its_name_can_settle() {
    let tree = project();
    let links = LinkNames::of(&tree.root);
    assert!(
        !links.names("docs/TOPIC.md#a-heading", "docs/guide/TOPIC.md"),
        "a route a file has names its own document"
    );
    assert!(!links.names("docs/old/README.md", "docs/TOPIC.md"));
    assert!(
        links.paths.get().is_none(),
        "neither a route a file has nor another name walks the documents"
    );
    assert!(links.names("docs/old/OTHER.md", "docs/OTHER.md"));
    assert!(
        links.paths.get().is_some(),
        "a route no file has, of the document's name, walks them"
    );
}

#[test]
fn a_name_only_history_keeps_is_found_there() {
    let tree = project();
    tree.write("docs/history/PLAN.md", "# Plan\n\nAs it was.\n");
    assert_eq!(moved(&tree.root, "docs/PLAN.md"), ["docs/history/PLAN.md"]);
    tree.write("docs/game/PLAN.md", "# Plan\n\nAs it is.\n");
    assert_eq!(
        moved(&tree.root, "docs/PLAN.md"),
        ["docs/game/PLAN.md", "docs/history/PLAN.md"],
        "a live document first, then history's"
    );
}

#[test]
fn a_rendered_document_and_a_folders_introduction_carry_the_boards_tasks_that_link_them() {
    let tree = project();
    let render = Render::new(&tree.root);
    let board = [
        task("tools/topic", &["docs/TOPIC.md"]),
        task("tools/intro", &["docs/README.md"]),
    ];
    let doc = document(&render, "docs/TOPIC.md", "", &board).unwrap();
    assert_eq!(slugs(&doc.tasks), ["tools/topic"]);
    assert!(
        document(&render, "docs/TOPIC.md", "", &[])
            .unwrap()
            .tasks
            .is_empty(),
        "with no board, no tasks"
    );
    let intro = folder(&render, "docs", "", &board).unwrap().intro.unwrap();
    assert_eq!(slugs(&intro.tasks), ["tools/intro"]);
}
