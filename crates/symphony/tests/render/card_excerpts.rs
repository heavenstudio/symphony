//! Card navigation and heading excerpts have different fragment targets. Exercise the typed wire
//! data the client uses, then its server lookup, without a server or the shared board.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use docs_index::{Doc, syntax};
use serde_json::Value;
use symphony::{blocks, docview, register, render::Render, text::splitlines};
use symphony_api::{Block, LinkTarget};

use super::Tree;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}

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
fn shield_field_v2_offers_the_heading_excerpt_and_keeps_its_replacement_as_navigation() {
    let root = root();
    let rel = "docs/combat/CAPABILITIES.md";
    let text = std::fs::read_to_string(root.join(rel)).unwrap();
    let render = Render::new(&root);
    let section = render
        .typed_sections(&text, rel)
        .into_iter()
        .find(|part| part.html.contains("data-slug=\"shield-field-v2\""))
        .expect("the reported card must be exercised");
    let links = links(&blocks::section(&section, rel));
    let heading = links
        .iter()
        .find(|(href, _)| href == "/docs/combat/CAPABILITIES.md#shield-projector")
        .expect("the card's heading link");
    let target = heading.1.as_ref().expect("the heading has a show action");
    let fragment = docview::section(&render, &target.path, target.anchor.as_deref().unwrap(), "")
        .expect("show opens the shield projector section");
    assert!(
        matches!(&fragment.blocks[0], Block::Heading { anchor, .. } if anchor == "shield-projector")
    );
    assert!(fragment.html.contains("+50% shield regeneration"));
    assert!(!fragment.html.contains("Needs your decision"));
    assert_eq!(
        links
            .iter()
            .find(|(href, _)| href == "/docs/combat/CAPABILITIES.md#shield-field"),
        Some(&("/docs/combat/CAPABILITIES.md#shield-field".into(), None)),
        "Replaces navigates to its card; a card slug is not a heading excerpt"
    );
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

/// Resolve the source spelling independently of the renderer, against the compiled docs index:
/// an existing relative file wins, otherwise the live-before-history suffix match wins.
fn indexed_target(root: &Path, from: &str, name: &str) -> Option<Doc> {
    let name = if name.trim().ends_with(".md") {
        name.trim().to_string()
    } else {
        format!("{}.md", name.trim())
    };
    if let Some(doc) = root
        .join(from)
        .parent()
        .unwrap()
        .join(&name)
        .canonicalize()
        .ok()
        .and_then(|path| {
            path.strip_prefix(root)
                .ok()
                .and_then(|p| p.to_str())
                .and_then(Doc::find)
        })
    {
        return Some(doc);
    }
    let tail = format!("/{}", name.trim_start_matches('/'));
    [false, true].into_iter().find_map(|history| {
        Doc::all()
            .find(|doc| doc.path().contains("/history/") == history && doc.path().ends_with(&tail))
    })
}

#[test]
fn every_index_resolved_card_heading_link_opens_the_same_heading_in_the_server() {
    let root = root();
    let render = Render::new(&root);
    let mut checked = 0;
    let mut shield_checked = false;
    let mut opened = BTreeSet::new();
    for doc in Doc::all() {
        let text = std::fs::read_to_string(root.join(doc.path())).unwrap();
        let cards = register::parse_decisions(&splitlines(&text));
        if cards.is_empty() {
            continue;
        }
        let sections = render.typed_sections(&text, doc.path());
        let mut history = None;
        for card in cards {
            let mut source_links = syntax::wikilinks(&card.text);
            for entry in card
                .entries()
                .iter()
                .filter(|entry| entry.who == register::Who::Agent)
            {
                source_links.extend(syntax::wikilinks(&entry.text));
            }
            let mut rendered = None;
            for link in source_links {
                let Some(heading) = link.heading.as_ref().filter(|_| !link.target.contains(':'))
                else {
                    continue;
                };
                let Some(target) = indexed_target(&root, doc.path(), &link.target) else {
                    continue;
                };
                let anchor = syntax::heading_anchor(heading);
                if !target.has_anchor(&anchor) {
                    continue; // The docs build does not validate historical documents' links.
                }
                let context = format!("{}:{} {}: {}", doc.path(), card.line, card.slug, link.whole);
                let rendered = rendered.get_or_insert_with(|| {
                    let section = sections
                        .iter()
                        .find(|part| part.html.contains(&format!("data-slug=\"{}\"", card.slug)))
                        .unwrap_or_else(|| panic!("missing card: {context}"));
                    let mut found = links(&blocks::section(section, doc.path()));
                    let history = history
                        .get_or_insert_with(|| docview::history(&render, doc.path(), "").unwrap());
                    if let Some(thread) = history.get(&card.slug) {
                        found.extend(links(&thread.blocks));
                    }
                    found
                });
                let href = format!("/{}#{anchor}", target.path());
                let actual = rendered
                    .iter()
                    .find(|(url, _)| url == &href)
                    .unwrap_or_else(|| panic!("missing {href}: {context}"))
                    .1
                    .as_ref()
                    .unwrap_or_else(|| panic!("no show action: {context}"));
                assert_eq!(actual.path, target.path(), "{context}");
                assert_eq!(actual.anchor.as_deref(), Some(anchor.as_str()), "{context}");
                if opened.insert((actual.path.clone(), anchor.clone())) {
                    let fragment = docview::section(
                        &render,
                        &actual.path,
                        actual.anchor.as_deref().unwrap(),
                        "",
                    )
                    .unwrap_or_else(|why| panic!("{context}: {why}"));
                    assert!(
                        matches!(fragment.blocks.first(), Some(Block::Heading { anchor: found, .. }) if found == &anchor),
                        "wrong heading: {context}"
                    );
                }
                checked += 1;
                shield_checked |= card.slug == "shield-field-v2" && anchor == "shield-projector";
            }
        }
    }
    assert!(
        shield_checked,
        "the reported card must be included in the sweep"
    );
    assert!(checked > 0, "the sweep must exercise heading excerpts");
    eprintln!(
        "checked {checked} index-resolved heading links in real decision cards, opening {} distinct headings",
        opened.len()
    );
}
