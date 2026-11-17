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

/// Each section carries the kinds of its first and last blocks into the outline, which reads no
/// bodies, so a page spaces a section not read yet as it will draw: the title, a paragraph, code, a
/// quote, a list and a card each by what they draw.
#[test]
fn the_outline_names_each_sections_first_and_last_blocks() {
    let tree = Tree::new();
    tree.write(
        "docs/R.md",
        "# R\n\nThe intro.\n\n## Lamps\n\nWarm white.\n\n```sh\nlamp on\n```\n\n> Said once.\n\n- \
         one\n- two\n\n<!-- register:start -->\n## Open\n\n- [ ] **tile** — Pick the tile.\n<!-- \
         register:end -->\n",
    );
    let render = Render::new(&tree.root);
    let doc = document(&render, "docs/R.md", "", &[]).unwrap();
    let ends: Vec<(String, Option<[String; 2]>)> = outline(doc)
        .document
        .sections
        .into_iter()
        .map(|section| {
            assert!(section.blocks.is_empty(), "the outline reads no bodies");
            (section.key, section.ends)
        })
        .collect();
    let has = |first: &str, last: &str| {
        ends.iter()
            .any(|(_, ends)| ends.as_ref().is_some_and(|[a, b]| a == first && b == last))
    };
    for (first, last) in [
        ("heading-1", "heading-1"),
        ("paragraph", "paragraph"),
        ("code", "code"),
        ("quote", "quote"),
        ("list", "list"),
    ] {
        assert!(has(first, last), "{first} to {last}: {ends:?}");
    }
    let card = ends.iter().find(|(key, _)| key == "card:tile");
    assert_eq!(
        card.and_then(|(_, ends)| ends.clone()),
        Some(["card".to_string(), "card".to_string()]),
        "{ends:?}"
    );
    // The register's closing comment is a section of its own that draws nothing.
    let (last, nothing) = ends.split_last().unwrap();
    assert_eq!(last.1, None, "{ends:?}");
    assert!(nothing.iter().all(|(_, ends)| ends.is_some()), "{ends:?}");
}

#[test]
fn outline_anchor_maps_keep_lexical_keys_and_last_duplicate_sections() {
    let tree = Tree::new();
    tree.write("docs/R.md", "# R\n\n## Zed\n\nText.\n\n## Alpha\n\nMore.\n- [ ] **thread-card** — A card.\n  [2026-10-20 10:00] agent — Noted.\n");
    let render = Render::new(&tree.root);
    let mut doc = document(&render, "docs/R.md", "", &[]).unwrap();
    let mut duplicate = doc.sections[1].clone();
    duplicate.html = "<h2 id=\"last-copy\">Latest</h2>".into();
    let duplicate_key = duplicate.key.clone();
    doc.sections.push(duplicate);
    doc.sections[0].start = 0;
    let header_key = doc.sections[0].key.clone();
    let expected_hash = super::thread_hash(&doc.threads["thread-card"]);
    let result = outline(doc);
    assert_eq!(result.anchors[&duplicate_key], ["last-copy"]);
    assert!(result.anchors[&header_key].is_empty());
    let keys = result.anchors.keys().collect::<Vec<_>>();
    assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(
        result
            .document
            .sections
            .iter()
            .all(|section| section.html.is_empty()
                && section.media.is_empty()
                && section.blocks.is_empty())
    );
    assert_eq!(result.thread_hashes["thread-card"], expected_hash);
    assert!(result.document.threads.is_empty());
}
