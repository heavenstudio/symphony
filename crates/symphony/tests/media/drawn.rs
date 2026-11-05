//! A file a renderer of the page's plugins draws: a picture of it or a link to it becomes the
//! renderer's viewer item, or its blocks, in the file's place; without such a renderer it stays what
//! markdown made of it. The fixture renderers ([`items`]) draw `.item` in a viewer, `.card` as a
//! paragraph and refuse `.broken`.

use symphony::render::{Media, Render};

use super::Tree;
use super::common::items::{self, viewer};

fn render(tree: &Tree) -> Render {
    Render::new(&tree.root).with_plugins(items::plugins())
}

fn media(text: &str, doc_path: &str) -> (String, Vec<Media>) {
    let tree = Tree::new();
    let sections = render(&tree).sections(text, doc_path);
    (
        sections.iter().map(|s| s.html.clone()).collect(),
        sections.into_iter().flat_map(|s| s.media).collect(),
    )
}

fn item(src: &str, label: &str) -> Media {
    Media::Viewer {
        src: src.into(),
        label: label.into(),
        poster: None,
        viewer: viewer(),
    }
}

#[test]
fn a_picture_of_an_item_is_its_viewer_not_an_img() {
    let (html, items) = media("![Pendant](/view/lamps/NOR.item)\n", "docs/A.md");
    assert_eq!(items, [item("/view/lamps/NOR.item", "Pendant")]);
    assert!(!html.contains("<img"), "{html}");
    assert!(html.contains("data-kind=\"viewer\""), "{html}");
}

#[test]
fn a_picture_and_a_link_are_one_viewer_each_and_a_link_takes_its_text_without_markup() {
    let (_, pictured) = media("![F](/a.item)\n", "docs/A.md");
    assert_eq!(pictured, [item("/a.item", "F")]);
    let (_, linked) = media("[the **Lumen** item](/a.ITEM)\n", "docs/A.md");
    assert_eq!(linked, [item("/a.ITEM", "the Lumen item")]);
}

#[test]
fn an_item_without_a_caption_is_named_by_its_file() {
    let (_, items) = media("![](/view/a%20b.item)\n", "docs/A.md");
    assert_eq!(items, [item("/view/a%20b.item", "a b.item")]);
}

#[test]
fn a_relative_and_a_worktree_item_resolve_like_a_picture() {
    let (_, relative) = media("![f](../a.item)\n", "docs/lamps/x.md");
    assert_eq!(relative, [item("/docs/a.item", "f")]);
    let worktree = ".claude/worktrees/picture-x/view/lamps/NOR.item";
    let (_, inside) = media(&format!("![f](/{worktree})\n"), "docs/A.md");
    assert_eq!(inside, [item(&format!("/{worktree}"), "f")]);
}

#[test]
fn an_item_in_a_code_span_stays_text() {
    let (_, items) = media("write `![f](a.item)` to embed\n", "docs/A.md");
    assert!(items.is_empty(), "{items:?}");
}

#[test]
fn without_a_renderer_for_it_a_picture_or_link_stays_what_markdown_made() {
    let tree = Tree::new();
    let text = "![f](/a.item) and [g](/b.item) and ![p](/c.png)\n";
    let sections = Render::new(&tree.root).sections(text, "docs/A.md");
    let html: String = sections.iter().map(|s| s.html.clone()).collect();
    assert!(sections.iter().all(|s| s.media.is_empty()), "{sections:?}");
    for kept in ["src=\"/a.item\"", "href=\"/b.item\"", "src=\"/c.png\""] {
        assert!(html.contains(kept), "{kept} missing: {html}");
    }
    let (with, items) = media("![p](/c.png) [m](/d.html)\n", "docs/A.md");
    assert!(items.is_empty(), "{items:?}");
    assert!(
        with.contains("<img") && with.contains("href=\"/d.html\""),
        "{with}"
    );
}

#[test]
fn a_blocks_drawing_stands_in_the_files_place_and_a_refusal_is_shown_there() {
    let (html, items) = media("[Deck](/a.card)\n", "docs/A.md");
    assert!(items.is_empty(), "{items:?}");
    assert!(html.contains("card Deck on docs/A.md"), "{html}");
    assert!(!html.contains("href=\"/a.card\""), "{html}");
    let (html, _) = media("![x](/a.broken)\n", "docs/A.md");
    assert!(
        html.contains("component-error") && html.contains("/a.broken cannot be read"),
        "{html}"
    );
}

#[test]
fn a_typed_render_keeps_the_renderers_name_on_its_blocks() {
    let tree = Tree::new();
    let sections = render(&tree).typed_sections("[Deck](/a.card)\n", "docs/A.md");
    let html: String = sections.iter().map(|s| s.html.clone()).collect();
    assert!(html.contains("<sf-component data-kind=\"card\">"), "{html}");
}

#[test]
fn an_item_in_a_card_thread_message_is_media_of_the_threads_fragment() {
    let tree = Tree::new();
    let text = "- [ ] **lamp** — x\n  [2026-10-06 10:00] agent — ![f](/view/lamps/NOR.item)\n";
    let threads = render(&tree).history(text, "docs/A.md");
    assert_eq!(threads["lamp"].media, [item("/view/lamps/NOR.item", "f")]);
}

#[test]
fn an_items_and_a_mockups_indexes_are_the_fragments_own() {
    let (html, items) = media("![m](/a.item) and ![h](/b.html)\n", "docs/A.md");
    assert_eq!(
        items,
        [
            item("/a.item", "m"),
            Media::Mockup {
                src: "/b.html".into(),
                label: "h".into()
            }
        ]
    );
    assert!(
        html.contains("data-media=\"0\" data-kind=\"viewer\""),
        "{html}"
    );
    assert!(
        html.contains("data-media=\"1\" data-kind=\"mockup\""),
        "{html}"
    );
}
