//! RenderTest, the prose half: sections with their lines, callouts, tables, and what a reply is.

use super::{bare, page};

#[test]
fn blocks_carry_source_lines() {
    let html = page("# T\n\npara **bold**\n");
    assert!(html.contains("data-start=\"1\" data-end=\"1\""), "{html}");
    assert!(html.contains("data-start=\"3\" data-end=\"3\""));
    assert!(html.contains("<strong>bold</strong>"));
}

#[test]
fn a_section_has_a_key_by_its_heading_and_its_place_and_a_hash_of_its_fragment() {
    let (_tree, render) = bare();
    let sections = render.sections(
        "# T\n\nintro\n\n## Cards\n\none\n\ntwo\n\n## Cards\n\nthree\n",
        "docs/x.md",
    );
    let keys: Vec<&str> = sections.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "section:t:0",
            "section:t:1",
            "section:cards:0",
            "section:cards:1",
            "section:cards:2",
            "section:cards-2:0",
            "section:cards-2:1"
        ]
    );
    assert!(sections.iter().all(|s| s.hash.len() == 16), "{sections:?}");
    assert_ne!(sections[3].hash, sections[4].hash);
}

#[test]
fn the_same_text_has_the_same_hashes_and_a_changed_section_has_another() {
    let (_tree, render) = bare();
    let before = render.sections("## A\n\none\n\n## B\n\ntwo\n", "docs/x.md");
    let again = render.sections("## A\n\none\n\n## B\n\ntwo\n", "docs/x.md");
    let edited = render.sections("## A\n\none\n\n## B\n\ntwo edited\n", "docs/x.md");
    assert_eq!(before, again);
    let moved: Vec<&str> = before
        .iter()
        .zip(&edited)
        .filter(|(a, b)| a.hash != b.hash)
        .map(|(a, _)| a.key.as_str())
        .collect();
    assert_eq!(moved, ["section:b:1"]);
}

#[test]
fn callout_becomes_a_div_with_kind_and_body() {
    let html = page("> [!dislike] Too grim\n> Lighten it.\n");
    assert!(html.contains("class=\"callout callout-dislike\""), "{html}");
    assert!(html.contains("Too grim"));
    assert!(html.contains("Lighten it."));
}

#[test]
fn nested_resolved_callout() {
    let html = page("> [!decision] Do X\n> > [!resolved] done\n");
    assert!(html.contains("callout-decision"), "{html}");
    assert!(html.contains("callout-resolved"));
}

#[test]
fn a_resolved_callout_is_collapsed() {
    let html = page("> [!dislike] Too grim\n> Lighten it.\n> > [!resolved] lightened\n");
    assert!(
        html.contains("<details class=\"callout callout-dislike callout-done\">"),
        "{html}"
    );
    assert!(html.contains(
        "<summary class=\"callout-title\"><span class=\"callout-kind\">dislike</span> Too grim"
    ));
    assert!(!html.contains("<details open"));
    assert!(html.contains("lightened"));
}

#[test]
fn an_open_callout_is_not_collapsed() {
    assert!(!page("> [!dislike] Too grim\n> Lighten it.\n").contains("<details"));
}

#[test]
fn other_callouts_stay_blockquotes() {
    assert!(page("> [!note] plain\n").contains("<blockquote>"));
}

#[test]
fn a_callout_title_is_escaped() {
    assert!(page("> [!comment] a <b> & c\n").contains("a &lt;b&gt; &amp; c"));
}

#[test]
fn tables_render() {
    assert!(page("| a | b |\n|---|---|\n| 1 | 2 |\n").contains("<table>"));
}

#[test]
fn fenced_code_keeps_its_language() {
    let html = page("```rust\nlet x = 1;\n```\n");
    assert!(
        html.contains("<pre><code class=\"language-rust\">let x = 1;"),
        "{html}"
    );
}

#[test]
fn raw_html_in_prose_passes_through_as_it_did() {
    assert!(page("a <kbd>Esc</kbd> key\n").contains("<kbd>Esc</kbd>"));
}

#[test]
fn reply_html_renders_an_agent_answer_like_a_card_reply() {
    let (_tree, render) = bare();
    let body = render.reply("See **this** and ![pic](/artifacts/x.png).", "docs/D.md");
    // comrak writes an image's src before its alt; the picture is the same
    assert_eq!(
        body.html,
        "See <strong>this</strong> and <img src=\"/artifacts/x.png\" alt=\"pic\" />."
    );
    assert!(
        render
            .reply("- one\n- two", "docs/D.md")
            .html
            .contains("<ul>")
    );
}

#[test]
fn relative_links_are_left_for_the_browser() {
    let html = super::page_at(
        "![a](../../artifacts/x.png) and [b](LAMP_CATALOGUE.md)\n",
        "docs/concepts/README.md",
    );
    assert!(html.contains("src=\"../../artifacts/x.png\""), "{html}");
    assert!(html.contains("href=\"LAMP_CATALOGUE.md\""));
}
