//! The wikilink cases of RenderTest: `[[NAME#Heading|label]]` finds its document by name wherever it lives.

use super::{Tree, page, page_at};
use symphony::render::Render;

fn with_docs(files: &[&str], text: &str, doc_path: &str) -> String {
    let tree = Tree::new();
    for file in files {
        tree.write(file, "# Doc\n");
    }
    Render::new(&tree.root).html(text, doc_path)
}

#[test]
fn wikilink_resolves_to_doc_route_with_heading_anchor() {
    let html = with_docs(
        &["docs/factions/FACTIONS.md"],
        "see [[FACTIONS#Confirmed direction|the plan]]\n",
        "docs/DECISIONS.md",
    );
    assert!(
        html.contains("href=\"/docs/factions/FACTIONS.md#confirmed-direction\""),
        "{html}"
    );
    assert!(html.contains(">the plan</a>"));
}

#[test]
fn wikilink_without_label_or_heading() {
    let html = page_at("[[SHIP_ROSTER]]\n", "docs/concepts/README.md");
    assert!(
        html.contains("href=\"/docs/concepts/SHIP_ROSTER.md\""),
        "nothing by that name: left where it points: {html}"
    );
    assert!(html.contains(">SHIP_ROSTER</a>"));
}

#[test]
fn a_wikilink_finds_its_document_by_name_wherever_it_lives() {
    let html = with_docs(
        &["docs/combat/WEAPONS.md"],
        "see [[WEAPONS#Types]]\n",
        "docs/DECISIONS.md",
    );
    assert!(
        html.contains("href=\"/docs/combat/WEAPONS.md#types\""),
        "{html}"
    );
}

#[test]
fn a_wikilink_with_a_folder_matches_the_end_of_the_path() {
    let html = with_docs(
        &["docs/world/factions/empire.md", "docs/ships/empire.md"],
        "[[factions/empire#Look]]\n",
        "docs/DECISIONS.md",
    );
    assert!(
        html.contains("href=\"/docs/world/factions/empire.md#look\""),
        "{html}"
    );
}

#[test]
fn a_wikilink_prefers_a_live_document_to_one_in_history() {
    let html = with_docs(
        &["docs/history/PLAN.md", "docs/game/PLAN.md"],
        "[[PLAN]]\n",
        "docs/DECISIONS.md",
    );
    assert!(html.contains("href=\"/docs/game/PLAN.md\""), "{html}");
}

#[test]
fn a_wikilink_from_the_page_s_own_folder_wins_over_a_document_elsewhere() {
    let html = with_docs(
        &["docs/a/NOTES.md", "docs/b/NOTES.md"],
        "[[NOTES]]\n",
        "docs/b/X.md",
    );
    assert!(html.contains("href=\"/docs/b/NOTES.md\""), "{html}");
}

#[test]
fn a_document_link_inside_a_code_block_stays_literal() {
    let html = page("```markdown\n[[SHIP_ROSTER#Size distribution]]\n```\n");
    assert!(html.contains("[[SHIP_ROSTER#Size distribution]]"), "{html}");
}

#[test]
fn a_document_link_inside_inline_code_stays_literal() {
    assert!(page("write `[[PLAN#Top]]`\n").contains("<code>[[PLAN#Top]]</code>"));
}

#[test]
fn task_wikilink_goes_to_the_task_page() {
    let html = page_at(
        "see [[task:sim/jump-departure]] and [[task:view/hud|the HUD task]]\n",
        "docs/SPEC.md",
    );
    assert!(html.contains("<a class=\"tasklink\" data-task=\"sim/jump-departure\" href=\"/task/sim/jump-departure\">sim/jump-departure</a>"), "{html}");
    assert!(html.contains("href=\"/task/view/hud\">the HUD task</a>"));
}

#[test]
fn a_bold_card_slug_links_to_the_document_that_holds_the_card() {
    let tree = Tree::new();
    tree.write(
        "docs/ships/empire.md",
        "# Empire\n\n- [ ] **kite-art** — a card\n",
    );
    let html = Render::new(&tree.root).html(
        "See **kite-art** and **nothing-here**, and *Replaces:* kite-art.\n",
        "docs/DECISIONS.md",
    );
    assert!(
        html.contains("<a href=\"/docs/ships/empire.md#kite-art\">kite-art</a>"),
        "{html}"
    );
    assert!(html.contains("<strong>nothing-here</strong>"));
    assert_eq!(
        html.matches("href=\"/docs/ships/empire.md#kite-art\"")
            .count(),
        2,
        "{html}"
    );
}

#[test]
fn a_card_slug_inside_a_link_or_code_is_not_linked_again() {
    let tree = Tree::new();
    tree.write("docs/ships/empire.md", "- [ ] **kite-art** — a card\n");
    let html = Render::new(&tree.root).html(
        "[**kite-art**](/x) and `**kite-art**`\n",
        "docs/DECISIONS.md",
    );
    assert!(!html.contains("empire.md"), "{html}");
}
