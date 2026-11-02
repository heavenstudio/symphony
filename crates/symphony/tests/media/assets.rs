//! AssetUrlTest and PictureCachingTest: every local file a page loads is named by a validator of the file
//! as it is now, so a browser never shows a stale copy.

use std::path::Path;

use symphony::assets::{KEEP, REVALIDATE, bust, cache_headers, local_file, validator};

use super::{Tree, png};

fn project() -> Tree {
    let tree = Tree::new();
    tree.write("docs/A.md", "# Alpha\n");
    tree.write("docs/sub/B.md", "# Beta\n");
    std::fs::create_dir_all(tree.path("artifacts")).unwrap();
    std::fs::write(tree.path("artifacts/x.png"), png(8, 8)).unwrap();
    tree.write("ui/hud.html", "<!doctype html><p>hud</p>\n");
    tree.write("static/live.js", "console.log(1)\n");
    tree
}

fn v(tree: &Tree, rel: &str) -> String {
    validator(&tree.path(rel))
}

#[test]
fn a_validator_names_the_file_as_it_is_now() {
    let tree = project();
    let before = v(&tree, "artifacts/x.png");
    assert!(
        before.len() == 12 && before.chars().all(|c| c.is_ascii_hexdigit()),
        "{before}"
    );
    assert_eq!(before, v(&tree, "artifacts/x.png"));
    let mut more = png(8, 8);
    more.extend(b"more");
    std::fs::write(tree.path("artifacts/x.png"), more).unwrap();
    assert_ne!(before, v(&tree, "artifacts/x.png"));
}

#[test]
fn a_file_that_is_not_there_has_no_validator() {
    assert_eq!(validator(Path::new("/nowhere/none.png")), "");
}

#[test]
fn a_documents_pictures_and_mockups_are_named_by_their_validator() {
    let tree = project();
    let markup = "<img src=\"../artifacts/x.png\" alt=\"pic\"><iframe src=\"/ui/hud.html\" sandbox=\"allow-scripts\"></iframe>";
    let (out, files) = bust(markup, &tree.root, &tree.path("static"), "/docs/A.md");
    // as written, relative to the document, so a branch's page still loads its own worktree's file
    assert!(
        out.contains(&format!(
            "src=\"../artifacts/x.png?v={}\"",
            v(&tree, "artifacts/x.png")
        )),
        "{out}"
    );
    assert!(
        out.contains(&format!(
            "src=\"/ui/hud.html?v={}\"",
            v(&tree, "ui/hud.html")
        )),
        "{out}"
    );
    assert_eq!(files.len(), 2);
}

#[test]
fn the_apps_own_scripts_and_icons_are_found_under_the_static_folder() {
    let tree = project();
    let markup =
        "<script src=\"/static/live.js\"></script><link rel=\"icon\" href=\"/static/live.js\">";
    let (out, _) = bust(markup, &tree.root, &tree.path("static"), "/");
    assert_eq!(
        out.matches(&format!("?v={}", v(&tree, "static/live.js")))
            .count(),
        2,
        "{out}"
    );
}

#[test]
fn the_other_attributes_that_load_a_file_are_named_too() {
    let tree = project();
    let validator = v(&tree, "artifacts/x.png");
    for attr in ["data-full", "data-module", "poster"] {
        let (out, _) = bust(
            &format!("<video {attr}=\"/artifacts/x.png\"></video>"),
            &tree.root,
            &tree.path("static"),
            "/",
        );
        assert!(
            out.contains(&format!("{attr}=\"/artifacts/x.png?v={validator}\"")),
            "{out}"
        );
    }
}

#[test]
fn links_and_outside_urls_are_left_as_they_are() {
    let tree = project();
    let markup = "<a href=\"sub/B.md\">B</a> <a href=\"../artifacts/x.png\">the picture</a> <img src=\"https://example.com/e.png\"> <img src=\"//cdn/x.png\">";
    assert_eq!(
        bust(markup, &tree.root, &tree.path("static"), "/docs/A.md"),
        (markup.to_string(), Default::default())
    );
}

#[test]
fn a_thumbnail_or_a_missing_file_keeps_its_url() {
    let tree = project();
    let markup = "<img src=\"/thumb/540/abc/artifacts/x.png.jpg\"><img src=\"/artifacts/gone.png\"><img src=\"../x.png?w=2\">";
    assert_eq!(
        bust(markup, &tree.root, &tree.path("static"), "/docs/A.md"),
        (markup.to_string(), Default::default())
    );
}

#[test]
fn a_fragment_stays_after_the_validator() {
    let tree = project();
    let (out, _) = bust(
        "<img src=\"/artifacts/x.png#a\">",
        &tree.root,
        &tree.path("static"),
        "/",
    );
    assert!(
        out.contains(&format!("x.png?v={}#a\"", v(&tree, "artifacts/x.png"))),
        "{out}"
    );
}

#[test]
fn nothing_outside_the_project_is_found() {
    let tree = project();
    let static_dir = tree.path("static");
    assert!(local_file("../../etc/passwd", &tree.root, &static_dir, "/docs/A.md").is_none());
    assert!(local_file("/../secret.html", &tree.root, &static_dir, "/").is_none());
    assert!(local_file("/%2e%2e/secret.html", &tree.root, &static_dir, "/").is_none());
    assert!(
        local_file("/docs", &tree.root, &static_dir, "/").is_none(),
        "a folder is not a file"
    );
    assert!(local_file("/artifacts/x.png", &tree.root, &static_dir, "/").is_some());
}

#[test]
fn a_request_naming_the_current_validator_may_be_kept_for_good() {
    let tree = project();
    let current = v(&tree, "artifacts/x.png");
    let (etag, control) = cache_headers(&tree.path("artifacts/x.png"), &format!("v={current}"));
    assert_eq!(etag, format!("\"{current}\""));
    assert!(
        control == KEEP && control.contains("immutable"),
        "{control}"
    );
}

#[test]
fn any_other_request_is_revalidated() {
    let tree = project();
    for query in ["", "v=stale", "w=2"] {
        let (etag, control) = cache_headers(&tree.path("artifacts/x.png"), query);
        assert_eq!(control, REVALIDATE, "{query}");
        assert!(etag.starts_with('"') && etag.len() > 2);
    }
}
