//! ComponentTest: `[[name:arg|option]]` is a hook here, rendered by the project's plugins. A
//! component renders the same in prose, in a table cell, in a card's text and in an agent's reply,
//! and never in owner words or code.

use symphony::plugins::Plugins;
use symphony::render::Render;
use symphony_plugin::vocabulary::{Block, ContainerKind, Image, Presentation, Span};
use symphony_plugin::{Component, Page, Plugin};

use super::Tree;

/// A plugin with one hull, REP-F01, as a picture of its size (`ship`) and in a strip (`ships`).
struct Fixture;

struct Ship;
struct Ships;

static FIXTURE: Fixture = Fixture;

impl Plugin for Fixture {
    fn name(&self) -> &str {
        "fixture"
    }

    fn components(&self) -> Vec<&dyn Component> {
        vec![&Ship, &Ships]
    }
}

fn kite(class: &str) -> Span {
    Span::Image {
        image: Image {
            src: "/k.png".into(),
            alt: "REP-F01 Kite".into(),
            presentation: Presentation::class(class),
        },
    }
}

impl Component for Ship {
    fn name(&self) -> &str {
        "ship"
    }

    fn render(
        &self,
        _page: &Page<'_>,
        arg: &str,
        option: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        if arg != "REP-F01" {
            return Err(format!("no hull {arg}"));
        }
        Ok(vec![Block::Paragraph {
            spans: vec![kite(&format!("ship-{}", option.unwrap_or("row")))],
        }])
    }
}

impl Component for Ships {
    fn name(&self) -> &str {
        "ships"
    }

    fn render(
        &self,
        _page: &Page<'_>,
        _arg: &str,
        _option: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        Ok(vec![Block::Container {
            tag: ContainerKind::Div,
            presentation: Presentation::class("ship-strip"),
            blocks: vec![Block::Paragraph {
                spans: vec![kite("")],
            }],
        }])
    }
}

fn fixture(tree: &Tree) -> Render {
    Render::new(&tree.root).with_plugins(Plugins::new(vec![&FIXTURE]))
}

fn shown(text: &str) -> String {
    let tree = Tree::new();
    fixture(&tree).html(text, "docs/x.md")
}

/// `text` rendered in a project whose `symphony.toml` names Star Foundry's plugin.
fn star_foundry(text: &str) -> String {
    let tree = Tree::star_foundry();
    Render::new(&tree.root).html(text, "docs/x.md")
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
fn every_catalogue_key_renders_through_star_foundrys_plugin() {
    let catalogue: std::collections::BTreeMap<String, Vec<serde_json::Value>> =
        serde_json::from_str(include_str!("../../src/star_foundry/icons.json")).unwrap();
    assert!(!catalogue.is_empty());
    for (family, icons) in catalogue {
        assert!(!icons.is_empty(), "{family}");
        for icon in icons {
            let key = icon["key"].as_str().unwrap();
            let html = star_foundry(&format!("[[{family}:{key}|name]]"));
            assert!(!html.contains("component-error"), "{family}:{key}: {html}");
            assert_eq!(html.matches("<svg ").count(), 1, "{family}:{key}: {html}");
            let label = icon["label"].as_str().unwrap();
            assert!(
                html.contains(&format!("data-icon=\"{label}\" data-named=\"true\"")),
                "{family}:{key}: {html}"
            );
        }
    }
}

#[test]
fn cloning_bay_gear_renders_with_its_icon_and_optional_name() {
    let html =
        star_foundry("| Gear |\n|---|\n| [[gear:cloning-bay]] |\n\n[[gear:cloning-bay|name]]\n");
    assert!(!html.contains("component-error"), "{html}");
    assert!(html.contains("<table>"), "{html}");
    assert_eq!(
        html.matches("data-icon=\"Cloning bay\"").count(),
        2,
        "{html}"
    );
    assert_eq!(html.matches("<svg ").count(), 2, "{html}");
    assert_eq!(html.matches("data-named=\"true\"").count(), 1, "{html}");
    assert_eq!(
        html.matches("data-tip=\"Cloning bay: ").count(),
        2,
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
    let render = fixture(&tree);
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
fn a_project_draws_only_the_components_of_the_plugins_it_names() {
    let plain = Tree::new();
    let star_foundry = Tree::star_foundry();
    for (tree, reference, problem) in [
        (&plain, "[[ship:REP-F01]]", "unknown component ship"),
        (&star_foundry, "[[ship:REP-F01]]", "unknown hull REP-F01"),
        (&star_foundry, "[[bogus:x]]", "unknown component bogus"),
    ] {
        let html = Render::new(&tree.root).html(reference, "docs/x.md");
        assert_eq!(
            html.trim(),
            format!(
                "<section data-start=\"1\" data-end=\"1\">\n<span class=\"component-error\" title=\"component error\">⚠ {problem}</span>\n</section>"
            ),
            "{reference}"
        );
    }
}

#[test]
fn the_built_in_task_link_needs_no_plugin() {
    let html = super::page("See [[task:sim/jump|the jump]].\n");
    assert!(
        html.contains(
            "<a class=\"tasklink\" data-task=\"sim/jump\" href=\"/task/sim/jump\">the jump</a>"
        ),
        "{html}"
    );
    assert!(html.contains(">the jump</a>"), "{html}");
}
