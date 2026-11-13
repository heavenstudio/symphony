//! A document's first part, what a page reads ahead of a document it links to
//! (`DocumentOutline::first`, `/api/document?view=first`): the outline's head and the leading
//! sections whole, as many as fit, with what belongs to them and nothing of the rest.

use std::collections::BTreeMap;

use serde_json::json;
use symphony_api::{AHEAD_BYTES, Document, DocumentOutline};

/// A document of an intro, the card `palette` (waiting, with a thread and a heading), a prose
/// section `lamps` of `text`, and the card `tiles` (with a thread), and its outline as the server
/// takes it: no bodies, no threads, each thread by its hash.
fn document(text: &str) -> (Document, DocumentOutline) {
    let paragraph =
        |text: &str| json!({"type": "paragraph", "spans": [{"type": "text", "text": text}]});
    let thread = |text: &str| json!({"blocks": [{"type": "thread", "entries": [{"who": "agent", "timestamp": null, "verdict": null, "blocks": [paragraph(text)]}]}]});
    let document: Document = serde_json::from_value(json!({
        "version": 7, "path": "docs/D.md", "title": "D", "waiting": ["palette", "elsewhere"],
        "crumbs": [{"name": "docs", "href": "/docs/"}],
        "headings": [{"level": 2, "anchor": "palette", "text": "Palette"}, {"level": 2, "anchor": "tiles", "text": "Tiles"}],
        "sections": [
            {"start": 0, "end": 2, "key": "intro", "hash": "i", "part": 0, "blocks": [paragraph("The intro.")]},
            {"start": 3, "end": 8, "key": "card:palette", "hash": "p", "part": 1, "blocks": [paragraph("Choose the palette.")]},
            {"start": 9, "end": 12, "key": "lamps", "hash": "l", "part": 2, "blocks": [paragraph(text)]},
            {"start": 13, "end": 18, "key": "card:tiles", "hash": "t", "part": 3, "blocks": [paragraph("Choose the tiles.")]},
        ],
        "cards": [
            {"slug": "palette", "state": "pending", "text": "Choose the palette, its whole text.", "group": "decide", "open": true},
            {"slug": "tiles", "state": "pending", "text": "Choose the tiles.", "group": "decide", "open": true},
        ],
        "threads": {"palette": thread("Posted."), "tiles": thread("Tiled.")},
        "tasks": [{"slug": "design/palette", "title": "Pick the palette", "status": "ready"}],
    }))
    .expect("the fixture is a whole document");
    let mut outline = DocumentOutline {
        document: document.clone(),
        anchors: BTreeMap::from([
            ("intro".to_string(), vec![]),
            ("card:palette".to_string(), vec!["palette".to_string()]),
            ("lamps".to_string(), vec!["L9".to_string()]),
            ("card:tiles".to_string(), vec!["tiles".to_string()]),
        ]),
        thread_hashes: BTreeMap::from([
            ("palette".to_string(), "pp".to_string()),
            ("tiles".to_string(), "tt".to_string()),
        ]),
    };
    for section in &mut outline.document.sections {
        section.blocks.clear();
    }
    for card in &mut outline.document.cards {
        card.text = "Short.".into();
    }
    outline.document.threads.clear();
    (document, outline)
}

fn keys(part: &DocumentOutline) -> Vec<&str> {
    part.document
        .sections
        .iter()
        .map(|section| section.key.as_str())
        .collect()
}

fn bytes(part: &DocumentOutline) -> usize {
    serde_json::to_vec(part)
        .expect("an outline serializes")
        .len()
}

#[test]
fn a_part_holds_the_heads_fields_and_its_sections_cards_threads_headings_and_anchors_alone() {
    let (document, outline) = document("Lamps.");
    let first = outline
        .first(&document, |part| part.document.sections.len() <= 2)
        .unwrap();
    assert_eq!(keys(&first), ["intro", "card:palette"]);
    let palette = &document.sections[1];
    assert_eq!(
        first.document.sections[1], *palette,
        "a section whole, its body with it"
    );
    assert_eq!(
        (
            &first.document.title,
            &first.document.crumbs,
            &first.document.tasks,
            first.document.version
        ),
        (&document.title, &document.crumbs, &document.tasks, 7),
        "the head as the outline has it"
    );
    assert_eq!(
        first.document.cards,
        [document.cards[0].clone()],
        "a card as its section's read gives it, its whole text"
    );
    assert_eq!(first.document.waiting, ["palette"]);
    assert_eq!(
        first.document.threads.keys().collect::<Vec<_>>(),
        ["palette"]
    );
    assert_eq!(
        first.document.threads["palette"],
        document.threads["palette"]
    );
    assert_eq!(
        first.thread_hashes,
        BTreeMap::from([("palette".to_string(), "pp".to_string())])
    );
    assert_eq!(
        first
            .document
            .headings
            .iter()
            .map(|heading| heading.anchor.as_str())
            .collect::<Vec<_>>(),
        ["palette"]
    );
    assert_eq!(
        first.anchors.keys().collect::<Vec<_>>(),
        ["card:palette", "intro"]
    );
}

#[test]
fn the_leading_sections_go_in_until_one_does_not_fit_and_none_after_it() {
    // What the leading `n` sections of a document with a short lamps take.
    let (short, short_outline) = document("Lamps.");
    let upto = |n: usize| {
        bytes(
            &short_outline
                .first(&short, |part| part.document.sections.len() <= n)
                .unwrap(),
        )
    };
    let tiles = upto(4) - upto(3);
    // Room for the intro, the card and tiles, not for a long lamps: tiles waits behind it.
    let room = upto(2) + tiles;
    let (document, outline) = document(&"Lamps, at length. ".repeat(40));
    let first = outline
        .first(&document, |part| bytes(part) <= room)
        .unwrap();
    assert_eq!(keys(&first), ["intro", "card:palette"]);
    assert!(first.document.cards.iter().all(|card| card.slug != "tiles"));
    assert!(!first.thread_hashes.contains_key("tiles"));
}

#[test]
fn a_head_alone_over_the_cap_is_no_part_and_a_head_alone_in_it_is_one_with_no_section() {
    let (document, outline) = document("Lamps.");
    assert_eq!(outline.first(&document, |_| false), None);
    let head = outline
        .first(&document, |part| part.document.sections.is_empty())
        .unwrap();
    assert_eq!(keys(&head), Vec::<&str>::new());
    assert!(head.document.cards.is_empty() && head.document.waiting.is_empty());
    assert!(head.document.headings.is_empty() && head.document.threads.is_empty());
    assert!(head.anchors.is_empty() && head.thread_hashes.is_empty());
    assert_eq!(head.document.tasks, document.tasks);
}

#[test]
fn under_the_cap_a_small_document_is_read_ahead_whole_and_a_large_one_in_its_cap() {
    let fits = |part: &DocumentOutline| bytes(part) <= AHEAD_BYTES;
    let (small, outline) = document("Lamps.");
    let whole = outline.first(&small, fits).unwrap();
    assert_eq!(
        keys(&whole),
        ["intro", "card:palette", "lamps", "card:tiles"]
    );
    assert_eq!(whole.document.threads, small.threads);
    let (large, outline) = document(&"Lamps, at length. ".repeat(2_000));
    let first = outline.first(&large, fits).unwrap();
    assert_eq!(keys(&first), ["intro", "card:palette"]);
    assert!(bytes(&first) <= AHEAD_BYTES, "{}", bytes(&first));
}
