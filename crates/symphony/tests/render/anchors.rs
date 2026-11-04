//! HeadingAnchorTest: the id a heading gets in the page is the anchor every link to it spells.

use symphony::edit::heading_anchor;

use super::page;

const HEADINGS: [&str; 6] = [
    "Confirmed direction",
    "Harbor Guild picture · Third pass: less brass, round their lamps",
    "Capture, intrusion, and false identities",
    "Café — naïve  spacing",
    "NOR-L01 Kite",
    "x_y 2.5",
];

#[test]
fn every_heading_gets_the_id_its_anchor_names() {
    for heading in HEADINGS {
        let html = page(&format!("## {heading}\n"));
        let id = format!("id=\"{}\"", heading_anchor(heading));
        assert!(html.contains(&id), "{heading}: {html}");
    }
}

#[test]
fn headings_get_ids_matching_wikilink_slugs() {
    assert!(page("## Confirmed direction\n").contains("id=\"confirmed-direction\""));
}

#[test]
fn the_id_is_made_from_the_heading_s_words_not_its_markup() {
    assert!(page("## Deeper `down` and *up*\n").contains("<h2 id=\"deeper-down-and-up\">"));
}

#[test]
fn a_heading_that_repeats_in_one_block_is_numbered_as_the_toc_numbered_it() {
    let html = page("## Cards\n## Cards\n## Cards\n");
    for id in ["cards", "cards_1", "cards_2"] {
        assert!(html.contains(&format!("id=\"{id}\"")), "{id}: {html}");
    }
}

#[test]
fn a_heading_with_no_letters_gets_a_numbered_id_rather_than_none() {
    assert!(page("## ★\n").contains("id=\"_1\""));
}

#[test]
fn a_heading_in_a_fence_is_code_and_has_no_id() {
    assert!(!page("```\n## not a heading\n```\n").contains("id="));
}
#[test]
fn headings_without_a_space_keep_their_existing_python_anchors() {
    let tree = super::Tree::new();
    let render = symphony::render::Render::new(&tree.root);
    let html = render.html("#3 does not touch:\n\n##Another heading\n", "docs/A.md");
    assert!(
        html.contains("<h1 id=\"3-does-not-touch\">3 does not touch:</h1>"),
        "{html}"
    );
    assert!(
        html.contains("<h2 id=\"another-heading\">Another heading</h2>"),
        "{html}"
    );
    for text in [
        "```\n#code\n```\n",
        "~~~\n#code\n~~~\n",
        "    #code\n",
        "`inline`#code\n",
        "<pre>\n#code\n</pre>\n",
    ] {
        let html = render.html(text, "docs/A.md");
        assert!(!html.contains("id=\"code\""), "{html}");
        assert!(html.contains("#code"), "{html}");
    }
}

#[test]
fn historical_arithmetic_keeps_its_plus_and_emphasis() {
    let tree = super::Tree::new();
    let render = symphony::render::Render::new(&tree.root);
    // The pre-commit gate compiles a miniature workspace without these documents.
    // Read the real regression inputs when this test runs, not while clippy builds it.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    for (path, words) in [
        (
            "docs/history/agents-guide.md",
            "1015 headless\n+ 67 render tests",
        ),
        (
            "docs/history/render.md",
            "CPU sim 3.8–4.7\n+ view sync 2.4–2.9 ≈ 7–8 ms</strong>",
        ),
    ] {
        let source = std::fs::read_to_string(root.join(path)).expect("history regression input");
        let html = render.html(&source, "docs/history/notes.md");
        assert!(
            html.contains(words),
            "the plus is prose, not a bullet: {words}"
        );
    }
}

#[test]
fn headings_without_a_space_between_many_code_and_html_blocks_keep_their_anchors() {
    let (_tree, render) = super::bare();
    let text = "#One\n\n```\n#fenced\n```\n\n#Two\n\n<div>\n#raw\n</div>\n\n    #indented\n\n\
                #Three\n\n~~~\n#tilde\n~~~\n#Four\n";
    let html = render.markdown(text, "docs/A.md");
    for id in ["one", "two", "three", "four"] {
        assert!(html.contains(&format!("id=\"{id}\"")), "{id}: {html}");
    }
    for code in ["#fenced", "#raw", "#indented", "#tilde"] {
        assert!(html.contains(code), "{code}: {html}");
    }
}
