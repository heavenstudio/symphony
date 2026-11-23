//! SplitBlocksTest and the thread cases of ThreadRenderTest: blocks, with their source lines.

use symphony::render::split_blocks;

use super::{bare, page_at};

fn spans(text: &str) -> Vec<(usize, usize)> {
    split_blocks(text)
        .iter()
        .map(|b| (b.start, b.end))
        .collect()
}

#[test]
fn blank_lines_separate_blocks_with_one_based_inclusive_lines() {
    let text = "# Title\n\npara one\nstill one\n\n\npara two\n";
    assert_eq!(spans(text), [(1, 1), (3, 4), (7, 7)]);
    assert_eq!(split_blocks(text)[1].text, "para one\nstill one");
}

#[test]
fn frontmatter_is_not_a_block() {
    assert_eq!(spans("---\nstatus: review\n---\n\n# Title\n"), [(5, 5)]);
}

#[test]
fn fenced_code_keeps_its_blank_lines() {
    assert_eq!(spans("```\na\n\nb\n```\n\nafter\n"), [(1, 5), (7, 7)]);
}

#[test]
fn an_empty_document_has_no_blocks() {
    assert!(split_blocks("").is_empty());
}

const CARD: &str = concat!(
    "- [?] **pilot** — Choose the method.\n",
    "  [2026-10-05 10:55] agent — Intro.\n",
    "  [2026-10-05 12:17] agent — **SOU-L03: exports.**\n",
    "\n",
    "    | Method | IoU |\n",
    "    |---|---|\n",
    "    | A | .32 |\n",
    "\n",
    "    ![pic](/a.png)\n",
    "  [2026-10-05 14:15] owner · question — How will the lamps work?\n",
    "\n",
    "    Second paragraph of the question.\n",
    "  [2026-10-05 14:20] agent — Answer.\n",
);

fn text() -> String {
    format!("# D\n\n{CARD}\nAfter the card.\n")
}

#[test]
fn a_card_and_its_whole_thread_are_one_block() {
    let blocks = split_blocks(&text());
    assert_eq!(blocks[1].text, CARD.trim_end_matches('\n'));
    assert_eq!(
        (blocks[1].start, blocks[1].end),
        (3, 2 + CARD.lines().count())
    );
    assert_eq!(blocks[2].text, "After the card.");
}

/// A register as a first one is written: its markers right above and below its one card.
const TIGHT: &str = "# D\n\n<!-- register:start -->\n- [ ] **a** — one.\n<!-- register:end -->\n";

#[test]
fn a_card_under_a_line_with_no_blank_between_is_a_block_of_its_own() {
    assert_eq!(spans(TIGHT), [(1, 1), (3, 3), (4, 4), (5, 5)]);
    assert_eq!(split_blocks(TIGHT)[2].text, "- [ ] **a** — one.");
    // Under a heading, a paragraph or a list item alike, and two cards in a row are two blocks.
    assert_eq!(
        spans("## Needs your decision\n- [ ] **a** — one.\n  its thread\n- [x] **b** — two.\n"),
        [(1, 1), (2, 3), (4, 4)]
    );
    assert_eq!(
        spans("words\nmore words\n- [ ] **a** — one.\n"),
        [(1, 2), (3, 3)]
    );
    assert_eq!(spans("- an item\n- [ ] **a** — one.\n"), [(1, 1), (2, 2)]);
}

#[test]
fn a_card_line_in_a_fence_stays_code_whatever_stands_above_it() {
    let text = "words\n```\n- [ ] **a** — one.\n```\n";
    assert_eq!(spans(text), [(1, 4)]);
    assert!(!page_at(text, "docs/x.md").contains("class=\"decision\""));
}

#[test]
fn a_card_right_under_its_registers_marker_is_drawn_as_the_queue_counts_it() {
    let html = page_at(TIGHT, "docs/DECISIONS.md");
    assert_eq!(html.matches("class=\"decision\"").count(), 1, "{html}");
    assert!(html.contains("data-slug=\"a\""), "{html}");
    assert!(html.contains("data-line=\"4\""), "{html}");
    assert!(html.contains("data-group=\"decide\""), "{html}");
    assert!(!html.contains("<li>"), "{html}");
    let lines: Vec<&str> = TIGHT.lines().collect();
    let counted = symphony::register::parse_decisions(&lines);
    assert_eq!(counted.len(), 1);
    assert_eq!((counted[0].line, counted[0].slug.as_str()), (4, "a"));
}

/// A register whose cards stand straight under its start marker and under a heading, no blank line
/// above either, as an agent writing its first cards into an empty register leaves it.
const CROWDED: &str = concat!(
    "# D\n",
    "\n",
    "<!-- register:start -->\n",
    "- [ ] **first** — One.\n",
    "- [ ] **second** — Two.\n",
    "### Topic\n",
    "- [ ] **third** — Three.\n",
    "<!-- register:end -->\n",
);

#[test]
fn every_card_of_a_crowded_register_is_drawn_as_a_card_and_its_heading_as_a_heading() {
    assert_eq!(
        spans(CROWDED),
        [(1, 1), (3, 3), (4, 4), (5, 5), (6, 6), (7, 7), (8, 8)]
    );
    let html = page_at(CROWDED, "docs/x.md");
    assert_eq!(html.matches("class=\"decision\"").count(), 3, "{html}");
    for (slug, line) in [("first", 4), ("second", 5), ("third", 7)] {
        assert!(
            html.contains(&format!("data-slug=\"{slug}\"")),
            "{slug}: {html}"
        );
        assert!(
            html.contains(&format!("data-line=\"{line}\"")),
            "{slug}: {html}"
        );
    }
    assert!(html.contains("<h3"), "{html}");
    assert!(!html.contains("<li>"), "{html}");
    // Each card the page draws is one the queue counts, at the same line.
    let lines: Vec<&str> = CROWDED.lines().collect();
    let counted = symphony::register::parse_decisions(&lines);
    let counted: Vec<(usize, &str, &str)> = (counted.iter())
        .map(|card| (card.line, card.slug.as_str(), card.topic.as_str()))
        .collect();
    assert_eq!(
        counted,
        [
            (4, "first", "General"),
            (5, "second", "General"),
            (7, "third", "Topic")
        ]
    );
}

#[test]
fn posts_render_inside_the_card_with_tables_as_tables() {
    let html = page_at(&text(), "docs/x.md");
    assert!(!html.contains("<pre"), "{html}");
    assert_eq!(html.matches("class=\"decision\"").count(), 1);
    let card = html
        .split("class=\"decision\"")
        .nth(1)
        .unwrap()
        .split("</section>")
        .next()
        .unwrap();
    assert!(card.contains("<table>"), "{card}");
    assert!(card.contains("<td>.32</td>"));
    assert!(card.contains("How will the lamps work?"));
    assert!(card.contains("Second paragraph of the question."));
    assert_eq!(card.matches("<li class=\"entry").count(), 4);
}

#[test]
fn the_thread_survives_the_card_html_for_each_slug() {
    let (_tree, render) = bare();
    let thread = &render.history(&text(), "docs/x.md")["pilot"];
    assert!(thread.html.contains("<table>"));
    assert_eq!(thread.html.matches("<li class=\"entry").count(), 4);
}
