//! MockupEmbedTest and ModelEmbedTest: an `.html` mockup or a `.glb` model in a document is a media
//! item the client draws, not markup; every other picture and link stays what markdown made of it.

use symphony::render::{MOCKUP_NATIVE, MOCKUP_SANDBOX, Media, Render, is_mockup};

use super::Tree;

fn media(text: &str, doc_path: &str) -> (String, Vec<Media>) {
    let tree = Tree::new();
    let sections = Render::new(&tree.root).sections(text, doc_path);
    (
        sections.iter().map(|s| s.html.clone()).collect(),
        sections.into_iter().flat_map(|s| s.media).collect(),
    )
}

fn mockup(src: &str, label: &str) -> Media {
    Media::Mockup {
        src: src.into(),
        label: label.into(),
    }
}

fn model(src: &str, label: &str) -> Media {
    Media::Model {
        src: src.into(),
        label: label.into(),
        poster: None,
    }
}

#[test]
fn an_image_to_an_html_file_is_a_mockup_not_an_img() {
    let (html, items) = media("![Flight HUD A](/ui/mockups/hud.html)\n", "docs/A.md");
    assert_eq!(items, [mockup("/ui/mockups/hud.html", "Flight HUD A")]);
    assert!(!html.contains("<img"), "{html}");
    assert!(
        html.contains("data-media=\"0\" data-kind=\"mockup\""),
        "{html}"
    );
}

#[test]
fn the_frame_is_the_native_size_in_a_sandbox_of_scripts_only() {
    assert_eq!(MOCKUP_NATIVE, (1280, 800));
    assert_eq!(MOCKUP_SANDBOX, "allow-scripts");
}

#[test]
fn a_mockup_without_alt_text_is_named_by_its_file() {
    assert_eq!(
        media("![](/ui/mockups/hud.html)\n", "docs/A.md").1,
        [mockup("/ui/mockups/hud.html", "hud.html")]
    );
}

#[test]
fn a_relative_mockup_resolves_against_its_page() {
    assert_eq!(
        media("![c](mockups/c.html)\n", "docs/art/UI.md").1,
        [mockup("/docs/art/mockups/c.html", "c")]
    );
    assert_eq!(
        media("![u](../ui/hud.html)\n", "docs/A.md").1,
        [mockup("/ui/hud.html", "u")]
    );
}

#[test]
fn a_png_image_is_untouched() {
    let (html, items) = media("![x](../artifacts/x.png)\n", "docs/A.md");
    assert!(html.contains("src=\"../artifacts/x.png\""), "{html}");
    assert!(items.is_empty());
}

#[test]
fn a_link_to_an_html_file_stays_a_link() {
    let (html, items) = media("[the mockup](/ui/mockups/hud.html)\n", "docs/A.md");
    assert!(html.contains("href=\"/ui/mockups/hud.html\""), "{html}");
    assert!(items.is_empty());
}

#[test]
fn an_html_image_in_a_code_span_stays_text() {
    assert!(
        media("write `![f](a.html)` to embed\n", "docs/A.md")
            .1
            .is_empty()
    );
}

#[test]
fn a_remote_html_target_is_not_framed() {
    assert!(
        media("![x](https://example.com/a.html)\n", "docs/A.md")
            .1
            .is_empty()
    );
}

#[test]
fn a_mockup_in_a_card_thread_message_is_media_of_the_threads_fragment() {
    let tree = Tree::new();
    let text =
        "- [ ] **hud** — x\n  [2026-10-06 10:00] agent — ![Flight HUD A](/ui/mockups/hud.html)\n";
    let threads = Render::new(&tree.root).history(text, "docs/A.md");
    assert_eq!(
        threads["hud"].media,
        [mockup("/ui/mockups/hud.html", "Flight HUD A")]
    );
}

#[test]
fn which_pictures_are_mockups() {
    for yes in ["/a/b.html", "x.HTM", "/a.html?v=3", "/a.html#top"] {
        assert!(is_mockup(yes), "{yes}");
    }
    for no in ["/a.png", "/a.html.png", "/htmlx", "https://x/a.js"] {
        assert!(!is_mockup(no), "{no}");
    }
}

#[test]
fn an_image_to_a_glb_is_a_model_not_an_img() {
    let (html, items) = media("![Frigate](/view/models/hulls/REP-F01.glb)\n", "docs/A.md");
    assert_eq!(items, [model("/view/models/hulls/REP-F01.glb", "Frigate")]);
    assert!(!html.contains("<img"), "{html}");
    assert!(html.contains("data-kind=\"model\""));
}

#[test]
fn an_image_is_one_model_and_a_link_is_one_model() {
    assert_eq!(media("![F](/a.glb)\n", "docs/A.md").1.len(), 1);
    assert_eq!(
        media("[F](/a.glb)\n", "docs/A.md").1,
        [model("/a.glb", "F")]
    );
}

#[test]
fn a_link_to_a_glb_takes_its_text_as_the_caption_without_markup() {
    assert_eq!(
        media("[the **Kite** model](/a.glb)\n", "docs/A.md").1,
        [model("/a.glb", "the Kite model")]
    );
}

#[test]
fn a_model_without_a_caption_is_named_by_its_file() {
    assert_eq!(
        media("![](/view/a%20b.glb)\n", "docs/A.md").1,
        [model("/view/a%20b.glb", "a b.glb")]
    );
}

#[test]
fn a_relative_and_a_worktree_model_resolve_like_an_image() {
    assert_eq!(
        media("![f](../a.glb)\n", "docs/ships/x.md").1,
        [model("/docs/a.glb", "f")]
    );
    let worktree = ".claude/worktrees/art-x/view/models/hulls/REP-F01.glb";
    assert_eq!(
        media(&format!("![f](/{worktree})\n"), "docs/A.md").1,
        [model(&format!("/{worktree}"), "f")]
    );
}

#[test]
fn a_glb_in_a_code_span_stays_text() {
    assert!(
        media("write `![f](a.glb)` to embed\n", "docs/A.md")
            .1
            .is_empty()
    );
}

#[test]
fn a_model_in_a_card_thread_message_is_media_of_the_threads_fragment() {
    let tree = Tree::new();
    let text =
        "- [ ] **hull** — x\n  [2026-10-06 10:00] agent — ![f](/view/models/hulls/REP-F01.glb)\n";
    let threads = Render::new(&tree.root).history(text, "docs/A.md");
    assert_eq!(
        threads["hull"].media,
        [model("/view/models/hulls/REP-F01.glb", "f")]
    );
}

#[test]
fn a_models_and_a_mockups_indexes_are_the_fragments_own() {
    let (html, items) = media("![m](/a.glb) and ![h](/b.html)\n", "docs/A.md");
    assert_eq!(items, [model("/a.glb", "m"), mockup("/b.html", "h")]);
    assert!(html.contains("data-media=\"0\" data-kind=\"model\""));
    assert!(html.contains("data-media=\"1\" data-kind=\"mockup\""));
}
