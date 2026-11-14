//! DocPageTest: a document is its title, what waits on the owner, its way back up, its headings for a
//! contents list, and its sections grouped into the cards of the page.

use symphony::render::Render;
use symphony::tree::{Heading, headings};

use super::Tree;

fn project() -> Tree {
    let tree = Tree::new();
    tree.write(
        "docs/A.md",
        "---\nstatus: review\n---\n# Alpha *plan*\n\nIntro.\n\n## First part\n\nText.\n\n\
         ```\n## not a heading\n```\n\n## Second\n\n### Deeper `down`\n\nMore.\n\n\
         - [x] **done-one** — settled.\n- [ ] **open-one** — first open.\n- [ ] **open-two** — second open.\n",
    );
    tree.write("docs/B.md", "# Beta\n\n## Only one\n\nText.\n");
    tree.write("docs/C.md", "No heading.\n");
    tree
}

fn open(tree: &Tree, name: &str) -> std::sync::Arc<symphony::page::Document> {
    Render::new(&tree.root)
        .document(&format!("docs/{name}.md"))
        .unwrap()
}

#[test]
fn the_head_has_the_title_and_the_decisions_waiting() {
    let doc = open(&project(), "A");
    assert_eq!(doc.title, "Alpha plan");
    assert_eq!(doc.waiting, ["open-one", "open-two"]);
    assert_eq!(doc.path, "docs/A.md");
}

#[test]
fn a_document_without_open_cards_waits_on_nothing() {
    assert!(open(&project(), "B").waiting.is_empty());
}

#[test]
fn a_document_without_a_heading_is_titled_by_its_file_name() {
    assert_eq!(open(&project(), "C").title, "C");
}

#[test]
fn the_contents_list_the_second_and_third_level_headings_in_order() {
    let doc = open(&project(), "A");
    let want = |level: u8, anchor: &str, words: &str| Heading {
        level,
        anchor: anchor.into(),
        text: words.into(),
    };
    assert_eq!(
        doc.headings,
        [
            want(2, "first-part", "First part"),
            want(2, "second", "Second"),
            want(3, "deeper-down", "Deeper down")
        ]
    );
    for heading in &doc.headings {
        let id = format!("id=\"{}\"", heading.anchor);
        assert!(
            doc.sections.iter().any(|s| s.html.contains(&id)),
            "each entry leads to a heading on the page: {id}"
        );
    }
}

#[test]
fn a_register_lists_every_group_and_the_topics_only_of_the_groups_that_wait_on_the_owner() {
    let text = "<!-- register:start -->\n## Needs your decision\n\n### Topic one\n\n- [ ] **a** — x\n\n## Auto-approved: spot-check\n\n### Releases\n\n- [a] **c** — z\n\n## Rejected and replaced\n\n### Page loading\n\n- [x] **d** — w\n\n## Resolved\n\n### Old topic\n\n- [r] **b** — y\n\n### Releases\n\n- [r] **e** — v\n<!-- register:end -->\n\n## After\n\n### Its part\n";
    let found: Vec<(u8, String)> = headings(text)
        .into_iter()
        .map(|h| (h.level, h.text))
        .collect();
    let want = |level: u8, text: &str| (level, text.to_string());
    assert_eq!(
        found,
        [
            want(2, "Needs your decision"),
            want(3, "Topic one"),
            want(2, "Auto-approved: spot-check"),
            want(3, "Releases"),
            want(2, "Rejected and replaced"),
            want(2, "Resolved"),
            want(2, "After"),
            want(3, "Its part"),
        ]
    );
}

#[test]
fn a_heading_that_repeats_is_listed_once() {
    let found: Vec<String> = headings("## Cards\n\n## Cards\n\n## Other\n")
        .into_iter()
        .map(|h| h.text)
        .collect();
    assert_eq!(found, ["Cards", "Other"]);
}

#[test]
fn a_short_document_has_no_contents() {
    assert!(!open(&project(), "B").has_toc());
    assert!(open(&project(), "A").has_toc());
}

#[test]
fn each_second_level_part_sits_in_its_own_card() {
    let doc = open(&project(), "A");
    // the opening, "First part", and "Second" with its h3 and its cards
    assert_eq!(doc.sections.iter().filter_map(|s| s.part).max(), Some(2));
    let part = |n: usize| {
        doc.sections
            .iter()
            .filter(move |s| s.part == Some(n))
            .collect::<Vec<_>>()
    };
    assert!(part(0).iter().any(|s| s.html.contains("Intro.")));
    assert_eq!(part(1)[0].start, 8);
    assert!(part(1)[0].html.contains("id=\"first-part\""));
    assert!(part(1).iter().all(|s| !s.html.contains("id=\"second\"")));
    assert!(
        part(2)
            .iter()
            .any(|s| s.html.contains("id=\"deeper-down\""))
    );
    assert!(
        part(2)
            .iter()
            .any(|s| s.html.contains("data-slug=\"open-two\""))
    );
}

#[test]
fn a_title_with_nothing_under_it_stays_out_of_the_cards() {
    let doc = open(&project(), "B");
    assert_eq!(
        doc.sections[0].part, None,
        "the page head names the page; the heading is not in a card"
    );
    assert!(doc.sections[0].html.contains("id=\"beta\""));
    assert_eq!(doc.sections[1].part, Some(0));
    assert!(doc.sections[1].html.contains("id=\"only-one\""));
}

#[test]
fn a_title_followed_by_prose_is_in_the_first_card() {
    let tree = project();
    tree.write("docs/D.md", "# Title\n\nIntro words.\n\n## Next\n");
    let doc = open(&tree, "D");
    assert_eq!(
        doc.sections.iter().map(|s| s.part).collect::<Vec<_>>(),
        [Some(0), Some(0), Some(1)]
    );
}

#[test]
fn a_document_without_second_level_headings_is_one_card() {
    let doc = open(&project(), "C");
    assert!(doc.sections.iter().all(|s| s.part == Some(0)));
}

#[test]
fn a_document_is_rendered_once_per_file_version() {
    let tree = project();
    let render = Render::new(&tree.root);
    let first = render.document("docs/A.md").unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &first,
        &render.document("docs/A.md").unwrap()
    ));
    tree.write("docs/A.md", "# Alpha\n\nIntro changed.\n");
    let second = render.document("docs/A.md").unwrap();
    assert!(!std::sync::Arc::ptr_eq(&first, &second));
    assert!(
        second
            .sections
            .iter()
            .any(|s| s.html.contains("Intro changed."))
    );
}

#[test]
fn only_the_sections_whose_hash_moved_are_new() {
    let tree = project();
    let render = Render::new(&tree.root);
    let before = render.document("docs/B.md").unwrap();
    tree.write("docs/B.md", "# Beta\n\n## Only one\n\nText changed.\n");
    let after = render.document("docs/B.md").unwrap();
    let moved: Vec<&str> = after
        .changed_since(&before)
        .iter()
        .map(|s| s.key.as_str())
        .collect();
    assert_eq!(moved, ["section:only-one:1"]);
    assert!(after.changed_since(&after).is_empty());
}

#[test]
fn a_document_that_is_not_there_is_an_error() {
    assert!(
        Render::new(&project().root)
            .document("docs/none.md")
            .is_err()
    );
}
