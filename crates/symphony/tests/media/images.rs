//! SizeTest, ReserveTest and the picture cases of ThumbnailTest: every picture a page draws reserves its
//! space, and a big one is drawn from a small copy.

use std::cell::RefCell;

use symphony::images::{THUMB_MIN_BYTES, file_size, png_size, reserve, thumbnail, token};

use super::{Tree, pictures, png};

#[test]
fn png_size_comes_from_the_header() {
    assert_eq!(png_size(&png(30, 20)), Some((30, 20)));
}

#[test]
fn a_file_that_is_not_a_picture_has_no_size() {
    assert_eq!(png_size(b"not a png at all"), None);
    assert_eq!(png_size(&png(0, 0)), None); // a placeholder whose header says 0 x 0
    assert_eq!(png_size(&png(9, 9)[..20]), None);
}

#[test]
fn a_file_s_size_is_read_again_when_it_changes() {
    let tree = pictures();
    assert_eq!(file_size(&tree.path("a/wide.png")), Some((40, 20)));
    let mut bigger = png(80, 60);
    bigger.extend([0; 5]);
    std::fs::write(tree.path("a/wide.png"), bigger).unwrap();
    assert_eq!(file_size(&tree.path("a/wide.png")), Some((80, 60)));
    assert_eq!(file_size(&tree.path("a/none.png")), None);
}

#[test]
fn a_local_picture_gets_its_real_size() {
    let tree = pictures();
    let out = reserve(
        "<img src=\"/a/wide.png\" alt=\"w\">",
        &tree.root,
        false,
        false,
    );
    assert!(
        out.contains("width=\"40\"") && out.contains("height=\"20\""),
        "{out}"
    );
    assert!(out.starts_with("<img src=\"/a/wide.png\" alt=\"w\""));
}

#[test]
fn a_width_the_markup_chose_keeps_its_ratio() {
    let tree = pictures();
    let out = reserve(
        "<img src=\"/a/wide.png\" alt=\"w\" width=\"100\" style=\"width:100px;max-width:none\">",
        &tree.root,
        false,
        false,
    );
    assert!(
        out.contains("width=\"100\"") && out.contains("height=\"50\""),
        "{out}"
    );
    assert_eq!(out.matches("width=").count(), 1);
}

#[test]
fn sizes_it_cannot_know_are_left_alone() {
    let tree = pictures();
    for tag in [
        "<img src=\"/a/missing.png\" alt=\"\">",
        "<img src=\"/a/fake.png\" alt=\"\">",
        "<img src=\"https://x/y.png\" alt=\"\">",
        "<img alt=\"\">",
        "<img src=\"/../outside.png\" alt=\"\">",
    ] {
        assert_eq!(reserve(tag, &tree.root, false, false), tag);
    }
}

#[test]
fn a_picture_already_sized_is_untouched() {
    let tree = pictures();
    let tag = "<img src=\"/a/wide.png\" width=\"9\" height=\"9\">";
    assert_eq!(reserve(tag, &tree.root, false, false), tag);
}

#[test]
fn a_query_string_does_not_hide_the_file() {
    let tree = pictures();
    assert!(
        reserve("<img src=\"/a/wide.png?v=3\">", &tree.root, false, false)
            .contains("height=\"20\"")
    );
}

#[test]
fn other_markup_is_not_touched() {
    let tree = pictures();
    let text = "<p>an <imgx src=\"/a/wide.png\"> and <a href=\"/a/wide.png\">link</a></p>";
    assert_eq!(reserve(text, &tree.root, false, false), text);
}

#[test]
fn the_first_picture_of_a_page_is_fetched_first_and_the_rest_wait() {
    let tree = pictures();
    let out = reserve(
        &"<img src=\"/a/wide.png\"> ".repeat(3),
        &tree.root,
        true,
        false,
    );
    let tags: Vec<&str> = out.split("<img").skip(1).collect();
    assert!(
        tags[0].contains("fetchpriority=\"high\"") && !tags[0].contains("loading=\"lazy\""),
        "{}",
        tags[0]
    );
    for tag in &tags[1..] {
        assert!(
            !tag.contains("fetchpriority") && tag.contains("loading=\"lazy\""),
            "{tag}"
        );
    }
    assert!(tags.iter().all(|t| t.contains("decoding=\"async\"")));
}

#[test]
fn a_picture_that_chose_its_loading_keeps_it() {
    let tree = pictures();
    let out = reserve(
        "<img src=\"/a/wide.png\" loading=\"eager\" decoding=\"sync\">",
        &tree.root,
        false,
        false,
    );
    assert_eq!(
        out.matches("loading=").count() + out.matches("decoding=").count(),
        2,
        "{out}"
    );
}

fn big(tree: &Tree, name: &str, width: u32, height: u32) {
    let mut bytes = png(width, height);
    bytes.resize(THUMB_MIN_BYTES + 10, 0); // a picture over the size worth shrinking
    std::fs::create_dir_all(tree.path("artifacts")).unwrap();
    std::fs::write(tree.path(&format!("artifacts/{name}")), bytes).unwrap();
}

#[test]
fn a_big_picture_is_drawn_from_a_thumbnail_and_keeps_its_original() {
    let tree = Tree::new();
    big(&tree, "big.png", 64, 48);
    let out = reserve(
        "<img src=\"/artifacts/big.png\" alt=\"big\">",
        &tree.root,
        false,
        true,
    );
    let src = out
        .split("src=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let rest = src.strip_prefix("/thumb/540/").expect(&out);
    let (tok, path) = rest.split_once('/').unwrap();
    assert!(
        tok.len() == 16 && tok.chars().all(|c| c.is_ascii_hexdigit()),
        "{tok}"
    );
    assert_eq!(path, "artifacts/big.png.jpg");
    assert!(out.contains("data-full=\"/artifacts/big.png\""), "{out}");
    assert!(
        out.contains("width=\"64\" height=\"48\""),
        "the space is the original's, not the thumbnail's: {out}"
    );
}

#[test]
fn a_picture_drawn_small_gets_a_small_thumbnail() {
    let tree = Tree::new();
    big(&tree, "big.png", 64, 48);
    let out = reserve(
        "<img src=\"/artifacts/big.png\" alt=\"tiny\" width=\"40\">",
        &tree.root,
        false,
        true,
    );
    assert!(out.contains("src=\"/thumb/160/"), "{out}");
}

#[test]
fn a_small_picture_is_left_as_it_is() {
    let tree = pictures();
    let out = reserve("<img src=\"/a/wide.png\">", &tree.root, false, true);
    assert!(
        out.contains("src=\"/a/wide.png\"") && !out.contains("data-full"),
        "{out}"
    );
}

#[test]
fn without_a_converter_the_original_is_drawn() {
    let tree = Tree::new();
    big(&tree, "big.png", 64, 48);
    let out = reserve("<img src=\"/artifacts/big.png\">", &tree.root, false, false);
    assert!(
        out.contains("src=\"/artifacts/big.png\"") && !out.contains("/thumb/"),
        "{out}"
    );
}

#[test]
fn the_thumbnail_is_made_once_and_is_current_by_its_token() {
    let tree = Tree::new();
    big(&tree, "big.png", 64, 48);
    let tok = token(&tree.root, &tree.path("artifacts/big.png"), 540);
    let calls = RefCell::new(Vec::new());
    let convert = |src: &std::path::Path, dest: &std::path::Path, width: u32| {
        calls.borrow_mut().push((src.to_path_buf(), width));
        std::fs::write(dest, b"\xff\xd8thumb").is_ok()
    };
    for _ in 0..2 {
        let (made, current) =
            thumbnail(&tree.root, 540, &tok, "artifacts/big.png", &convert, true).unwrap();
        assert!(current);
        assert_eq!(std::fs::read(made).unwrap(), b"\xff\xd8thumb");
    }
    assert_eq!(calls.borrow().len(), 1);
    assert_eq!(calls.borrow()[0].1, 540);
}

#[test]
fn a_thumbnail_whose_original_changed_is_not_current() {
    let tree = Tree::new();
    big(&tree, "big.png", 64, 48);
    let stale = token(&tree.root, &tree.path("artifacts/big.png"), 540);
    let convert =
        |_: &std::path::Path, dest: &std::path::Path, _: u32| std::fs::write(dest, b"j").is_ok();
    big(&tree, "big.png", 32, 24);
    let mut bytes = std::fs::read(tree.path("artifacts/big.png")).unwrap();
    bytes.extend([0; 99]);
    std::fs::write(tree.path("artifacts/big.png"), bytes).unwrap();
    let (_, current) =
        thumbnail(&tree.root, 540, &stale, "artifacts/big.png", &convert, true).unwrap();
    assert!(!current);
}

#[test]
fn only_pictures_under_the_project_have_thumbnails() {
    let tree = Tree::new();
    big(&tree, "big.png", 64, 48);
    tree.write("docs/A.md", "# A\n");
    let convert =
        |_: &std::path::Path, dest: &std::path::Path, _: u32| std::fs::write(dest, b"j").is_ok();
    for rel in ["../secret.png", "docs/A.md", "artifacts/none.png"] {
        assert!(
            thumbnail(&tree.root, 540, "0123456789abcdef", rel, &convert, true).is_none(),
            "{rel}"
        );
    }
    assert!(
        thumbnail(
            &tree.root,
            100,
            "0123456789abcdef",
            "artifacts/big.png",
            &convert,
            true
        )
        .is_none(),
        "a width nobody asks for"
    );
}

#[test]
fn a_failed_conversion_or_no_converter_makes_no_thumbnail() {
    let tree = Tree::new();
    big(&tree, "big.png", 64, 48);
    let fails = |_: &std::path::Path, _: &std::path::Path, _: u32| false;
    assert!(thumbnail(&tree.root, 540, "x", "artifacts/big.png", &fails, true).is_none());
    assert!(thumbnail(&tree.root, 540, "x", "artifacts/big.png", &fails, false).is_none());
}
