//! Card navigation and heading excerpts have different fragment targets: a card's bold name and its
//! `*Replaces:*` link navigate to the card, in the typed wire data the client uses.

use serde_json::Value;
use symphony::{blocks, render::Render};
use symphony_api::{Block, LinkTarget};

use super::Tree;

/// Walk the actual wire vocabulary, including links nested in settled cards or styled spans.
fn links(blocks: &[Block]) -> Vec<(String, Option<LinkTarget>)> {
    fn visit(value: &Value, found: &mut Vec<(String, Option<LinkTarget>)>) {
        match value {
            Value::Array(items) => items.iter().for_each(|item| visit(item, found)),
            Value::Object(object) => {
                if matches!(
                    object.get("type").and_then(Value::as_str),
                    Some("link" | "wiki-link")
                ) {
                    found.push((
                        object["href"].as_str().unwrap().into(),
                        serde_json::from_value(object["target"].clone()).unwrap(),
                    ));
                }
                object.values().for_each(|value| visit(value, found));
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    visit(&serde_json::to_value(blocks).unwrap(), &mut found);
    found
}

#[test]
fn typed_card_references_preserve_bold_and_replaces_links_without_heading_actions() {
    let tree = Tree::new();
    tree.write("docs/old.md", "- [-] **old-card** — old\n");
    let render = Render::new(&tree.root);
    let fragment = render.typed_reply(
        "See **old-card** and *Replaces:* old-card. Also [details](#details), `**old-card**`, and **unknown-card**.",
        "docs/new.md",
    );
    let blocks = blocks::parse(&fragment.html, &[], "docs/new.md");
    assert_eq!(
        links(&blocks),
        vec![
            ("/docs/old.md#old-card".into(), None),
            ("/docs/old.md#old-card".into(), None),
            (
                "#details".into(),
                Some(LinkTarget {
                    path: "docs/new.md".into(),
                    anchor: Some("details".into())
                })
            ),
        ]
    );
    assert!(fragment.html.contains("<strong><a data-card="));
    assert!(fragment.html.contains("<code>**old-card**</code>"));
    assert!(fragment.html.contains("<strong>unknown-card</strong>"));
    assert!(
        !serde_json::to_string(&blocks)
            .unwrap()
            .contains("unknown\"")
    );
}
