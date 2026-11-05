//! A renderer: a file type a plugin draws, named by the extensions of its files. A project's
//! entrypoint shows such files when its `types` names the renderer, and a page that links or
//! pictures one draws it as the renderer says: as the generic blocks components render to, or as a
//! viewer of the plugin's own (a custom element its script defines) over the file served as it is.

use crate::Page;
use crate::vocabulary::{Block, Viewer};

/// The file a page links or pictures, as the renderer is handed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct File<'a> {
    /// Its URL, resolved against the page (`/docs/a.item`).
    pub src: &'a str,
    /// The link's text or the picture's alt text; empty when it has none.
    pub label: &'a str,
}

/// How a renderer draws a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drawing {
    /// Generic blocks, drawn where the link or picture was.
    Blocks(Vec<Block>),
    /// The file in the viewer, under its caption.
    Viewer(Viewer),
}

/// A file type a plugin draws.
pub trait Renderer: Send + Sync {
    /// The name an entrypoint's `types` gives it.
    fn name(&self) -> &str;
    /// The extensions of its files, lower case, without the dot.
    fn extensions(&self) -> &[&str];
    /// The content type its files are served with.
    fn content_type(&self) -> &str {
        "application/octet-stream"
    }
    /// How `file` is drawn on `page`, or why it cannot be, shown in its place.
    fn draw(&self, page: &Page<'_>, file: &File<'_>) -> Result<Drawing, String>;
}
