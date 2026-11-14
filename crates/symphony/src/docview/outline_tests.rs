//! A document's outline (`/api/document?view=outline`, [`outline`]): each card's line, which a
//! folded card draws as its summary.

use super::{OUTLINE_CARD_CHARS, document, outline};
use crate::render::Render;
use crate::test_tree::Tree;

fn card_texts(text: &str) -> Vec<String> {
    let tree = Tree::new();
    tree.write("docs/R.md", text);
    let render = Render::new(&tree.root);
    let doc = document(&render, "docs/R.md", "", &[]).unwrap();
    outline(doc)
        .document
        .cards
        .into_iter()
        .map(|card| card.text)
        .collect()
}

#[test]
fn a_long_cards_line_ends_at_a_word_with_an_ellipsis_and_a_short_one_whole() {
    let long = "A task's page now shows what the task went through, built from the approved \
                pictures. It sits under the gates and subtasks and above the brief, one lane \
                each for its stages.";
    assert!(long.chars().count() > OUTLINE_CARD_CHARS);
    let texts = card_texts(&format!(
        "# R\n\n<!-- register:start -->\n## Resolved\n\n- [r] **long** — {long}\n- [r] **short** — \
         A short **card**.\n<!-- register:end -->\n"
    ));
    assert_eq!(texts.len(), 2, "{texts:?}");
    let cut = &texts[0];
    assert!(cut.ends_with('…'), "{cut}");
    assert!(cut.chars().count() <= OUTLINE_CARD_CHARS, "{cut}");
    let kept = cut.trim_end_matches('…');
    assert!(long.starts_with(kept), "{cut}");
    assert!(
        long[kept.len()..].starts_with(' '),
        "the cut falls between words, not inside one: {cut}"
    );
    assert_eq!(texts[1], "A short card.", "plain words, whole");
}
