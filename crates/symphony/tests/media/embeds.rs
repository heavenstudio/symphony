//! MockupEmbedTest: an `.html` mockup in a document is a media item the client draws, not markup;
//! every other picture and link stays what markdown made of it (`drawn.rs` has the files a plugin's
//! renderer draws).

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
        media("![c](mockups/c.html)\n", "docs/pictures/UI.md").1,
        [mockup("/docs/pictures/mockups/c.html", "c")]
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
