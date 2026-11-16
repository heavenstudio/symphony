//! A renderer: a file type a plugin draws, named by the extensions of its files. A project's
//! entrypoint shows such files when its `types` names the renderer, and a page that links or
//! pictures one draws it as the renderer says: as the generic blocks components render to, or as a
//! viewer of the plugin's own (a custom element its script defines) over the file served as it is.

use serde::{Deserialize, Serialize};

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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Plain;

    impl Renderer for Plain {
        fn name(&self) -> &str {
            "plain"
        }
        fn extensions(&self) -> &[&str] {
            &["plain"]
        }
        fn draw(&self, _: &Page<'_>, file: &File<'_>) -> Result<Drawing, String> {
            Err(format!("{} is plain", file.src))
        }
    }

    #[test]
    fn a_renderer_naming_no_type_serves_its_files_as_bytes() {
        assert_eq!(Plain.content_type(), "application/octet-stream");
        let page = Page::new(std::path::Path::new("/project"), "docs/A.md");
        let file = File {
            src: "/a.plain",
            label: "",
        };
        assert_eq!(
            Plain.draw(&page, &file),
            Err("/a.plain is plain".to_string())
        );
    }
}
