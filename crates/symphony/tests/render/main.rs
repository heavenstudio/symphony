//! Render documents with comrak into HTML sections carrying source lines,
//! stable keys and content hashes.

#[path = "../common/mod.rs"]
mod common;

mod anchors;
mod blocks;
mod card_excerpts;
mod cards;
mod components;
mod links;
mod parts;
mod posts;
mod prose;

use common::Tree;
use symphony::render::Render;

/// A renderer over an empty project, for text that reads no other file.
pub fn bare() -> (Tree, Render) {
    let tree = Tree::new();
    let render = Render::new(&tree.root);
    (tree, render)
}

/// `text` rendered as the document `docs/x.md` of an empty project.
pub fn page(text: &str) -> String {
    bare().1.html(text, "docs/x.md")
}

/// `text` rendered as `doc_path`.
pub fn page_at(text: &str, doc_path: &str) -> String {
    bare().1.html(text, doc_path)
}
