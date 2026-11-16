//! Which documents name a file from outside the entrypoints' folders ([`names_files_outside`]):
//! one whose render holds the validator of a picture no entrypoint serves.

use super::{document, names_files_outside};
use crate::project::Project;
use crate::render::Render;
use crate::test_tree::Tree;

fn outside(tree: &Tree, text: &str, scope: &str) -> bool {
    tree.write("docs/R.md", text);
    let doc = document(&Render::new(&tree.root), "docs/R.md", scope, &[]).unwrap();
    names_files_outside(&doc, &Project::of(&tree.root), scope)
}

#[test]
fn a_document_with_no_picture_or_only_its_entrypoints_pictures_names_none_outside() {
    let tree = Tree::new();
    tree.write("docs/shots/lamp.png", "png");
    assert!(!outside(
        &tree,
        "# R\n\nWords, and [a link](OTHER.md).\n",
        ""
    ));
    assert!(!outside(
        &tree,
        "# R\n\n![The lamp](shots/lamp.png)\n\n## More\n\n![Again](shots/lamp.png)\n",
        ""
    ));
    let doc = document(&Render::new(&tree.root), "docs/R.md", "", &[]).unwrap();
    let html: String = doc.sections.iter().map(|s| s.html.as_str()).collect();
    assert!(
        html.contains("/docs/shots/lamp.png?v="),
        "the picture is named by its validator, so the case can tell: {html}"
    );
}

#[test]
fn a_picture_from_outside_the_entrypoints_folders_is_named_outside() {
    let tree = Tree::new();
    tree.write("shots/lamp.png", "png");
    tree.write("docs/shots/lamp.png", "png");
    assert!(outside(
        &tree,
        "# R\n\n![Inside](shots/lamp.png)\n\n## More\n\n![Outside](../shots/lamp.png)\n",
        ""
    ));
}

#[test]
fn a_missing_picture_has_no_validator_and_names_nothing() {
    let tree = Tree::new();
    assert!(!outside(&tree, "# R\n\n![Gone](../shots/gone.png)\n", ""));
}

#[test]
fn a_branch_pages_pictures_are_read_under_its_scope() {
    let tree = Tree::new();
    tree.write("shots/lamp.png", "png");
    tree.write("docs/shots/lamp.png", "png");
    assert!(!outside(
        &tree,
        "# R\n\n![Inside](shots/lamp.png)\n",
        "tools~lamp"
    ));
    assert!(outside(
        &tree,
        "# R\n\n![Outside](../shots/lamp.png)\n",
        "tools~lamp"
    ));
}

#[test]
fn a_media_items_file_is_read_as_a_pictures_is() {
    let tree = Tree::new();
    tree.write("mock/lamp.html", "<p>The lamp.</p>");
    tree.write("docs/mock/lamp.html", "<p>The lamp.</p>");
    assert!(!outside(&tree, "# R\n\n![The lamp](mock/lamp.html)\n", ""));
    let doc = document(&Render::new(&tree.root), "docs/R.md", "", &[]).unwrap();
    let media = serde_json::to_string(&doc.sections).unwrap();
    assert!(
        media.contains("/docs/mock/lamp.html?v="),
        "the mockup is a media item named by its validator, so the case can tell: {media}"
    );
    assert!(outside(
        &tree,
        "# R\n\n![The lamp](../mock/lamp.html)\n",
        ""
    ));
}

#[test]
fn a_url_with_a_query_of_its_own_or_another_sites_carries_no_validator_and_names_nothing() {
    let tree = Tree::new();
    tree.write("shots/lamp.png", "png");
    assert!(!outside(
        &tree,
        "# R\n\n[Sized](../shots/lamp.png?size=2) and [elsewhere](https://example.invalid/watch?v=1) and [again](//example.invalid/watch?v=1).\n",
        ""
    ));
}
