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
    "  [2026-10-05 12:17] agent — **COA-F01: exports.**\n",
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
