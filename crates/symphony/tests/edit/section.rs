//! SectionTest and HeadingAnchorTest: the part of a document a card links to, by the anchor its
//! links use.

use symphony::edit::{heading_anchor, section_text};

const TEXT: &str = "---\nstatus: review\n---\n# Doc\n\nIntro.\n\n\
    ## Relationship matrix\n\nRows here.\n\n### Detail\n\nDeeper.\n\n\
    ## Capture, intrusion, and false identities\n\nOther.\n";

#[test]
fn a_section_runs_to_the_next_heading_of_its_level_and_keeps_subheadings() {
    let out = section_text(TEXT, "relationship-matrix").unwrap();
    assert!(out.starts_with("## Relationship matrix"), "{out}");
    assert!(out.contains("Deeper."));
    assert!(!out.contains("Other."));
}

#[test]
fn the_anchor_is_the_same_slug_the_links_use() {
    let anchor = heading_anchor("Capture, intrusion, and false identities");
    assert!(section_text(TEXT, &anchor).unwrap().contains("Other."));
}

#[test]
fn an_unknown_anchor_is_none() {
    assert_eq!(section_text(TEXT, "nowhere"), None);
}

#[test]
fn no_anchor_is_the_whole_document_without_frontmatter() {
    let out = section_text(TEXT, "").unwrap();
    assert!(out.starts_with("# Doc"), "{out}");
    assert!(out.contains("Other."));
}

#[test]
fn a_heading_inside_fenced_code_is_not_a_section() {
    let text = "# D\n\n```\n## Fake\n```\n\n## Real\n\nyes\n";
    assert_eq!(section_text(text, "fake"), None);
    assert!(section_text(text, "real").unwrap().contains("yes"));
}

#[test]
fn the_anchor_matches_the_rendered_heading_id() {
    // What Python-Markdown's `toc.slugify(heading, "-")` gives each.
    for (heading, anchor) in [
        ("Confirmed direction", "confirmed-direction"),
        (
            "Harbor Guild picture · Third pass: less brass, round their lamps",
            "harbor-guild-picture-third-pass-less-brass-round-their-lamps",
        ),
        (
            "Capture, intrusion, and false identities",
            "capture-intrusion-and-false-identities",
        ),
        ("Café — naïve  spacing", "cafe-naive-spacing"),
        ("NOR-L01 Lumen", "nor-l01-lumen"),
        ("x_y 2.5", "x_y-25"),
        ("Types of weapon", "types-of-weapon"),
    ] {
        assert_eq!(heading_anchor(heading), anchor, "{heading}");
    }
}
