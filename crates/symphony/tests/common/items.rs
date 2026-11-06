//! A fixture plugin of renderers alone: it draws an `.item` file in the viewer [`viewer`], a `.card`
//! file as a paragraph naming its label and page, and refuses a `.broken` file.

use symphony::plugins::Plugins;
use symphony_plugin::vocabulary::{Block, Span, Viewer};
use symphony_plugin::{Drawing, File, Page, Plugin, Renderer};

pub struct Items;

pub static PLUGIN: Items = Items;

/// The fixture's renderers, each named by the one extension it draws.
struct Kind(&'static str);

static ITEM: Kind = Kind("item");
static CARD: Kind = Kind("card");
static BROKEN: Kind = Kind("broken");

impl Plugin for Items {
    fn name(&self) -> &str {
        "fixture-items"
    }

    fn renderers(&self) -> Vec<&dyn Renderer> {
        vec![&ITEM, &CARD, &BROKEN]
    }
}

/// The plugins of a page that draws the fixture's files.
pub fn plugins() -> Plugins {
    Plugins::new(vec![&PLUGIN])
}

/// The viewer an `.item` file is drawn in.
pub fn viewer() -> Viewer {
    Viewer {
        element: "item-view".into(),
        script: "/static/item-view.js".into(),
        ..Viewer::default()
    }
}

impl Renderer for Kind {
    fn name(&self) -> &str {
        self.0
    }

    fn extensions(&self) -> &[&str] {
        std::slice::from_ref(&self.0)
    }

    fn draw(&self, page: &Page<'_>, file: &File<'_>) -> Result<Drawing, String> {
        match self.0 {
            "item" => Ok(Drawing::Viewer(viewer())),
            "card" => Ok(Drawing::Blocks(vec![Block::Paragraph {
                spans: vec![Span::Text {
                    text: format!("card {} on {}", file.label, page.doc_path),
                }],
            }])),
            _ => Err(format!("{} cannot be read", file.src)),
        }
    }
}
