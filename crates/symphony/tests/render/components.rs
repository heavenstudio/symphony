//! ComponentTest: `[[name:arg|option]]` is a hook here, rendered by whoever fills it
//! (tools/python-zero/review-components). A component renders the same in prose, in a table cell, in a
//! card's text and in an agent's reply, and never in owner words or code.

use symphony::render::{Components, Render};

use super::Tree;

struct Ship;

impl Components for Ship {
    fn render_one(&self, name: &str, arg: &str, option: Option<&str>) -> Result<String, String> {
        match name {
            "ship" if arg == "REP-F01" => Ok(format!(
                "<img src=\"/k.png\" alt=\"REP-F01 Kite\" class=\"ship-{}\">",
                option.unwrap_or("row")
            )),
            "ships" => Ok(
                "<div class=\"ship-strip\"><img src=\"/k.png\" alt=\"REP-F01 Kite\"></div>".into(),
            ),
            "ship" => Err(format!("no hull {arg}")),
            other => Err(format!("unknown component {other}")),
        }
    }
}

fn shown(text: &str) -> String {
    let tree = Tree::new();
    Render::new(&tree.root)
        .with_components(Box::new(Ship))
        .html(text, "docs/x.md")
}

#[test]
fn a_component_in_prose_renders_its_live_picture() {
    let html = shown("The hull [[ship:REP-F01|small]] leads.\n");
    assert!(
        html.contains("<img src=\"/k.png\" alt=\"REP-F01 Kite\" class=\"ship-small\">"),
        "{html}"
    );
    assert!(html.contains("The hull "));
    assert!(html.contains(" leads."));
}

#[test]
fn a_component_in_a_table_cell_keeps_the_table() {
    let html = shown("| Hull | x |\n|---|---|\n| [[ship:REP-F01]] | y |\n");
    assert!(html.contains("<table>"), "{html}");
    assert!(html.contains("<img src=\"/k.png\""));
}

#[test]
fn every_catalogue_key_renders_through_the_default_provider() {
    let catalogue: std::collections::BTreeMap<String, Vec<serde_json::Value>> =
        serde_json::from_str(include_str!("../../src/component_icons.json")).unwrap();
    assert!(!catalogue.is_empty());
    for (family, icons) in catalogue {
        assert!(!icons.is_empty(), "{family}");
        for icon in icons {
            let key = icon["key"].as_str().unwrap();
            let html = super::page(&format!("[[{family}:{key}|name]]"));
            assert!(!html.contains("component-error"), "{family}:{key}: {html}");
            assert_eq!(html.matches("<svg ").count(), 1, "{family}:{key}: {html}");
            assert!(html.contains("role=\"img\""), "{family}:{key}: {html}");
        }
    }
}

#[test]
fn cloning_bay_gear_renders_with_its_icon_and_optional_name() {
    let html =
        super::page("| Gear |\n|---|\n| [[gear:cloning-bay]] |\n\n[[gear:cloning-bay|name]]\n");
    assert!(!html.contains("component-error"), "{html}");
    assert!(html.contains("<table>"), "{html}");
    assert_eq!(
        html.matches("class=\"gear gear-cloning-bay\"").count(),
        2,
        "{html}"
    );
    assert_eq!(html.matches("<svg ").count(), 2, "{html}");
    assert_eq!(
        html.matches("aria-label=\"Cloning bay\"").count(),
        2,
        "{html}"
    );
    assert!(
        html.contains("<span class=\"gear-name\">Cloning bay</span>"),
        "{html}"
    );
}

#[test]
fn a_component_alone_in_a_paragraph_loses_the_paragraph() {
    let html = shown("[[ships:REP-F01]]\n");
    assert!(html.contains("<div class=\"ship-strip\">"), "{html}");
    assert!(!html.contains("<p><div"), "{html}");
}

#[test]
fn a_component_in_a_card_text_and_in_an_agent_reply_renders() {
    let html = shown(
        "- [ ] **a** — Covers [[ships:REP-F01]].\n  [2026-09-26 09:00] agent — Here: [[ship:REP-F01|large]]\n",
    );
    assert_eq!(html.matches("<img src=\"/k.png\"").count(), 2, "{html}");
    assert!(html.contains("ship-large"));
    assert!(html.contains("ship-strip"));
}

#[test]
fn a_threads_refresh_renders_components_too() {
    let tree = Tree::new();
    let render = Render::new(&tree.root).with_components(Box::new(Ship));
    let threads = render.history(
        "- [ ] **a** — x.\n  [2026-09-26 09:00] agent — [[ship:REP-F01]]\n",
        "docs/DECISIONS.md",
    );
    assert!(threads["a"].html.contains("<img src=\"/k.png\""));
}

#[test]
fn owner_words_stay_plain_text() {
    let html = shown(
        "- [?] **a** — x.\n  [2026-09-26 09:00] owner · question — what is [[ship:REP-F01]]?\n",
    );
    assert!(!html.contains("<img"), "{html}");
}

#[test]
fn a_callout_body_renders_components() {
    assert!(shown("> [!comment] Look\n> at [[ship:REP-F01]]\n").contains("<img src=\"/k.png\""));
}

#[test]
fn a_component_in_inline_code_stays_literal() {
    assert!(shown("write `[[ship:REP-F01]]`\n").contains("<code>[[ship:REP-F01]]</code>"));
}

#[test]
fn a_component_in_a_fence_stays_literal() {
    let html = shown("```\n[[ship:REP-F01]]\n```\n");
    assert!(
        html.contains("[[ship:REP-F01]]") && !html.contains("<img"),
        "{html}"
    );
}

#[test]
fn a_reference_that_names_nothing_is_an_error_the_reader_can_see() {
    let html = shown("[[ship:NOPE]] and [[bogus:x]]\n");
    assert!(
        html.contains(
            "<span class=\"component-error\" title=\"component error\">⚠ no hull NOPE</span>"
        ),
        "{html}"
    );
    assert!(html.contains("⚠ unknown component bogus"));
}

#[test]
fn the_default_provider_distinguishes_unknown_hulls_from_unknown_components() {
    let tree = Tree::new();
    let render = Render::new(&tree.root);
    for (reference, problem) in [
        ("[[ship:REP-F01]]", "unknown hull REP-F01"),
        ("[[bogus:x]]", "unknown component bogus"),
    ] {
        let html = render.html(reference, "docs/x.md");
        assert_eq!(
            html.trim(),
            format!(
                "<section data-start=\"1\" data-end=\"1\">\n<span class=\"component-error\" title=\"component error\">⚠ {problem}</span>\n</section>"
            )
        );
    }
}
