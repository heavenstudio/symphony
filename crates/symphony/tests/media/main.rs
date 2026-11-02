//! Page media: model and mockup markup, image dimensions, thumbnails and asset
//! validators. HTTP encoding, cache and route cases belong to the server.

#[path = "../common/mod.rs"]
mod common;

mod assets;
mod embeds;
mod images;

use common::Tree;

/// The first bytes of a PNG that says it is `width` x `height`.
pub fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend(13u32.to_be_bytes());
    bytes.extend(b"IHDR");
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 2, 0, 0, 0]);
    bytes
}

/// A tree with picture files in it.
pub fn pictures() -> Tree {
    let tree = Tree::new();
    std::fs::create_dir_all(tree.path("a")).unwrap();
    std::fs::write(tree.path("a/wide.png"), png(40, 20)).unwrap();
    std::fs::write(tree.path("a/fake.png"), png(0, 0)).unwrap();
    tree
}
