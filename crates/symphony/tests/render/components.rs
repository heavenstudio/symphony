//! ComponentTest: `[[name:arg|option]]` is a hook here, rendered by the project's plugins. A
//! component renders the same in prose, in a table cell, in a card's text and in a note under a
//! card, the owner's or an agent's, and never in code.

use symphony::plugins::Plugins;
use symphony::render::Render;
use symphony_plugin::vocabulary::{Block, ContainerKind, Image, Presentation, Span};
use symphony_plugin::{Component, Page, Plugin};

use super::Tree;

/// A plugin with one item, AB-12, as a picture of its size (`item`) and in a strip (`items`).
struct Fixture;

struct Item;
struct Items;

static FIXTURE: Fixture = Fixture;

impl Plugin for Fixture {
    fn name(&self) -> &str {
        "fixture"
    }

    fn components(&self) -> Vec<&dyn Component> {
        vec![&Item, &Items]
    }
}

fn lamp(class: &str) -> Span {
    Span::Image {
        image: Image {
            src: "/l.png".into(),
            alt: "AB-12 Lamp".into(),
            presentation: Presentation::class(class),
        },
    }
}

impl Component for Item {
    fn name(&self) -> &str {
        "item"
    }

    fn render(
        &self,
        _page: &Page<'_>,
        arg: &str,
        option: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        if arg != "AB-12" {
            return Err(format!("no item {arg}"));
        }
        Ok(vec![Block::Paragraph {
            spans: vec![lamp(&format!("item-{}", option.unwrap_or("row")))],
        }])
    }
}

impl Component for Items {
    fn name(&self) -> &str {
        "items"
    }

    fn render(
        &self,
        _page: &Page<'_>,
        _arg: &str,
        _option: Option<&str>,
    ) -> Result<Vec<Block>, String> {
        Ok(vec![Block::Container {
            tag: ContainerKind::Div,
            presentation: Presentation::class("item-strip"),
            blocks: vec![Block::Paragraph {
                spans: vec![lamp("")],
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

#[test]
fn a_component_in_prose_renders_its_live_picture() {
    let html = shown("The item [[item:AB-12|small]] leads.\n");
    assert!(
        html.contains("<img src=\"/l.png\" alt=\"AB-12 Lamp\" class=\"item-small\">"),
        "{html}"
    );
    assert!(html.contains("The item "));
    assert!(html.contains(" leads."));
}

#[test]
fn a_component_in_a_table_cell_keeps_the_table() {
    let html = shown("| Item | x |\n|---|---|\n| [[item:AB-12]] | y |\n");
    assert!(html.contains("<table>"), "{html}");
    assert!(html.contains("<img src=\"/l.png\""));
}

#[test]
fn a_component_alone_in_a_paragraph_loses_the_paragraph() {
    let html = shown("[[items:AB-12]]\n");
    assert!(html.contains("<div class=\"item-strip\">"), "{html}");
    assert!(!html.contains("<p><div"), "{html}");
}

#[test]
fn a_component_in_a_card_text_and_in_an_agent_reply_renders() {
    let html = shown(
        "- [ ] **a** — Covers [[items:AB-12]].\n  [2026-09-26 09:00] agent — Here: [[item:AB-12|large]]\n",
    );
    assert_eq!(html.matches("<img src=\"/l.png\"").count(), 2, "{html}");
    assert!(html.contains("item-large"));
    assert!(html.contains("item-strip"));
}

#[test]
fn a_component_in_an_agents_post_renders_beside_its_words_typed() {
    let tree = Tree::new();
    let fragment = fixture(&tree).typed_reply(
        "Here: [[item:AB-12|large]] inside <Lamp/>.\n\n[[items:AB-12]]\n",
        "docs/x.md",
    );
    let blocks = symphony::blocks::fragment(&fragment, "docs/x.md");
    let [Block::Paragraph { spans }, Block::Component { kind, .. }] = blocks.as_slice() else {
        panic!("{blocks:?}")
    };
    assert_eq!(kind, "items");
    assert!(
        matches!(&spans[..], [Span::Text { text: before }, Span::Component { kind, .. }, Span::Text { text: after }]
            if before == "Here: " && kind == "item" && after == " inside <Lamp/>."),
        "{spans:?}"
    );
}

#[test]
fn a_threads_refresh_renders_components_too() {
    let tree = Tree::new();
    let render = fixture(&tree);
    let threads = render.history(
        "- [ ] **a** — x.\n  [2026-09-26 09:00] agent — [[item:AB-12]]\n",
        "docs/DECISIONS.md",
    );
    assert!(threads["a"].html.contains("<img src=\"/l.png\""));
}

#[test]
fn an_owner_note_renders_components_as_an_agents_does() {
    let html = shown(
        "- [?] **a** — x.\n  [2026-09-26 09:00] owner · question — what is [[item:AB-12|small]]?\n    [[items:AB-12]]\n",
    );
    assert!(
        html.contains("what is <img src=\"/l.png\" alt=\"AB-12 Lamp\" class=\"item-small\">?"),
        "{html}"
    );
    assert!(html.contains("<div class=\"item-strip\">"), "{html}");
}

#[test]
fn a_callout_body_renders_components() {
    assert!(shown("> [!comment] Look\n> at [[item:AB-12]]\n").contains("<img src=\"/l.png\""));
}

#[test]
fn a_component_in_inline_code_stays_literal() {
    assert!(shown("write `[[item:AB-12]]`\n").contains("<code>[[item:AB-12]]</code>"));
}

#[test]
fn a_component_in_a_fence_stays_literal() {
    let html = shown("```\n[[item:AB-12]]\n```\n");
    assert!(
        html.contains("[[item:AB-12]]") && !html.contains("<img"),
        "{html}"
    );
}

#[test]
fn a_reference_that_names_nothing_is_an_error_the_reader_can_see() {
    let html = shown("[[item:NOPE]] and [[bogus:x]]\n");
    assert!(
        html.contains(
            "<span class=\"component-error\" title=\"component error\">⚠ no item NOPE</span>"
        ),
        "{html}"
    );
    assert!(html.contains("⚠ unknown component bogus"));
}

/// The plugins the render tests' process holds: the fixture alone.
static PLUGINS: &[&dyn Plugin] = &[&FIXTURE];

#[test]
fn a_project_draws_only_the_components_of_the_plugins_it_names() {
    symphony::plugins::install(PLUGINS).unwrap();
    let plain = Tree::new();
    let named = Tree::new();
    named.write(
        symphony::project::FILE,
        "[plugins]\ncrates = [\"fixture\"]\n",
    );
    for (tree, reference, problem) in [
        (&plain, "[[item:AB-12]]", "unknown component item"),
        (&named, "[[item:NOPE]]", "no item NOPE"),
        (&named, "[[bogus:x]]", "unknown component bogus"),
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
