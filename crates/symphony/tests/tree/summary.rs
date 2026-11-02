//! TreeSummaryTest: what a folder page says under each entry, the first paragraph of prose as plain text.

use symphony::tree::{pending, summary, title};

use super::Tree;

fn summary_of(text: &str) -> String {
    let tree = Tree::new();
    summary(&tree.write("X.md", text))
}

#[test]
fn an_underscore_inside_a_name_is_kept() {
    assert_eq!(
        summary_of("# Ships\n\nSHIP_ROSTER sets the lengths; `ship_art.py` draws _the_ pages.\n"),
        "SHIP_ROSTER sets the lengths; ship_art.py draws the pages."
    );
}

#[test]
fn a_paragraph_that_opens_in_bold_is_prose_not_a_list() {
    assert_eq!(
        summary_of("# Deck\n\n**Not required reading.** Evidence only.\n\nLater.\n"),
        "Not required reading. Evidence only."
    );
}

#[test]
fn a_list_before_the_first_paragraph_is_skipped() {
    assert_eq!(
        summary_of("# X\n\n* one\n- two\n\nThe prose.\n"),
        "The prose."
    );
}

#[test]
fn headings_tables_quotes_and_html_are_not_prose() {
    assert_eq!(
        summary_of("# X\n\n| a | b |\n\n> quote\n\n<div>\n\nThe prose.\n"),
        "The prose."
    );
}

#[test]
fn links_and_wikilinks_read_as_their_words() {
    assert_eq!(
        summary_of("# X\n\nSee [the plan](PLAN.md) and [[SPEC#Top|the spec]] and [[GAME#Part]].\n"),
        "See the plan and the spec and GAME."
    );
}

#[test]
fn frontmatter_is_not_prose() {
    assert_eq!(
        summary_of("---\nstatus: review\n---\n# X\n\nBody.\n"),
        "Body."
    );
}

#[test]
fn long_prose_is_cut_at_a_word_with_an_ellipsis() {
    let long = "word ".repeat(80);
    let out = summary_of(&format!("# X\n\n{long}\n"));
    assert!(out.ends_with("word…"), "{out}");
    assert!(out.chars().count() <= 220, "{}", out.chars().count());
}

#[test]
fn a_missing_file_has_no_summary() {
    let tree = Tree::new();
    assert_eq!(summary(&tree.path("none.md")), "");
}

#[test]
fn a_title_is_the_first_heading_or_the_file_name() {
    let tree = Tree::new();
    assert_eq!(
        title(&tree.write("a/B.md", "intro\n\n# The *real* one\n")),
        "The *real* one"
    );
    assert_eq!(title(&tree.write("a/C.md", "no heading\n")), "C");
    assert_eq!(title(&tree.path("a")), "a");
    tree.write("a/README.md", "# Folder name\n");
    assert_eq!(title(&tree.path("a")), "Folder name");
}

#[test]
fn pending_lists_the_cards_that_wait_on_the_owner_in_page_order() {
    let text = "- [ ] **one** — new\n- [x] **two** — done\n- [?] **three** — q\n  [2026-09-24 10:00] owner · question — what?\n  [2026-09-24 11:00] agent · question — and you?\n- [ ] **four** — also new\n";
    assert_eq!(pending(text), ["one", "three", "four"]);
}
