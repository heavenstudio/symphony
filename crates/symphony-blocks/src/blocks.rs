//! Document content as data. No field contains rendered HTML. Inline components and media stay
//! inline even inside a table cell; a client can render the same tree on web or desktop.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Media;

macro_rules! vocabulary {
    ($name:ident { $($variant:ident => $word:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
        #[non_exhaustive]
        pub enum $name { $(#[serde(rename = $word)] $variant),+ }
        impl $name {
            pub fn as_str(self) -> &'static str { match self { $(Self::$variant => $word),+ } }
        }
        impl std::str::FromStr for $name {
            type Err = String;
            fn from_str(word: &str) -> Result<Self, Self::Err> {
                match word { $($word => Ok(Self::$variant)),+, _ => Err(format!("unknown {}: {word}", stringify!($name))) }
            }
        }
    };
}

vocabulary!(CardState { Pending => "pending", Approved => "approved", AutoApproved => "auto-approved", Rejected => "rejected", Question => "question", Resolved => "resolved", Dismissed => "dismissed" });
vocabulary!(CardGroup { Decide => "decide", Auto => "auto", Confirm => "confirm", Agent => "agent", Rejected => "rejected", Resolved => "resolved" });
vocabulary!(ThreadAuthor { Owner => "owner", Agent => "agent", Unknown => "unknown" });
vocabulary!(CalloutKind { Comment => "comment", Like => "like", Dislike => "dislike", Question => "question", Decision => "decision", Resolved => "resolved" });
vocabulary!(FigureSize { Small => "small", Row => "row", Large => "large", Tile => "tile", Cell => "cell", Slot => "slot", Wide => "wide" });

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Block {
    Heading {
        level: u8,
        anchor: String,
        spans: Vec<Span>,
    },
    Paragraph {
        spans: Vec<Span>,
    },
    List {
        ordered: bool,
        start: u64,
        items: Vec<Vec<Block>>,
    },
    /// A table; a cell holds figures through `Span::Blocks`. A component's table names its look in
    /// `presentation` (a class a stylesheet selects), a document's table carries none.
    Table {
        head: Vec<Cell>,
        rows: Vec<Vec<Cell>>,
        #[serde(default, skip_serializing_if = "Presentation::is_empty")]
        presentation: Presentation,
    },
    Code {
        language: String,
        text: String,
    },
    Quote {
        blocks: Vec<Block>,
    },
    Rule,
    Callout {
        kind: CalloutKind,
        title: Vec<Span>,
        resolved: bool,
        blocks: Vec<Block>,
    },
    Card {
        slug: String,
        mark: CardState,
        title: String,
        body: Vec<Block>,
        verdict: Verdict,
        thread: Vec<ThreadEntry>,
        /// Source line or offset supplied by the section pipeline, never guessed by the client.
        line: Option<usize>,
        line_offset: Option<usize>,
        comparison: Vec<Block>,
    },
    /// A `[[kind:argument|option]]` component, as its plugin rendered it. `kind` is the component's
    /// name, kept on the wrapper (`data-component`) so a project's stylesheet can select it.
    Component {
        kind: String,
        blocks: Vec<Block>,
    },
    Media {
        media: Media,
    },
    Image {
        image: Image,
    },
    /// A picture with its caption: a picture slot, an item in a strip or a table cell, a capture.
    /// `href` links the picture, `badge` is the short state shown where the picture is missing,
    /// and `size` chooses the layout.
    Figure {
        picture: Option<Image>,
        caption: Vec<Span>,
        href: Option<String>,
        badge: Option<String>,
        size: FigureSize,
    },
    Caption {
        blocks: Vec<Block>,
    },
    Comparison {
        figures: Vec<Block>,
    },
    Group {
        blocks: Vec<Block>,
    },
    /// Structural grouping and presentation from the document and its live data. The tag is a
    /// closed enum; clients never interpret arbitrary element names or HTML.
    Container {
        tag: ContainerKind,
        presentation: Presentation,
        blocks: Vec<Block>,
    },
    Details {
        summary: Vec<Span>,
        open: bool,
        blocks: Vec<Block>,
    },
    SettledCard {
        summary: Vec<Span>,
        open: bool,
        blocks: Vec<Block>,
    },
    Thread {
        entries: Vec<ThreadEntry>,
    },
    Comment {
        text: String,
    },
    /// Unsupported markup is visible text, accompanied by a server warning naming the document.
    Unknown {
        tag: String,
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Span {
    Text {
        text: String,
    },
    Emphasis {
        spans: Vec<Span>,
    },
    Strong {
        spans: Vec<Span>,
    },
    Code {
        text: String,
    },
    Link {
        href: String,
        target: Option<LinkTarget>,
        title: String,
        spans: Vec<Span>,
    },
    WikiLink {
        href: String,
        target: Option<LinkTarget>,
        spans: Vec<Span>,
    },
    LineBreak,
    Image {
        image: Image,
    },
    Media {
        media: Media,
    },
    /// A component inside a line of text, as `Block::Component` is beside one.
    Component {
        kind: String,
        blocks: Vec<Block>,
    },
    /// A small vector symbol on the 24-unit stroke grid with its name: `label` is what it is called
    /// (its accessible name, and shown after it when `named`), `tip` what it means on hover.
    Icon {
        shape: Vec<VectorShape>,
        label: String,
        named: bool,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        tip: String,
    },
    Styled {
        tag: InlineKind,
        presentation: Presentation,
        spans: Vec<Span>,
    },
    Graphic {
        view_box: String,
        presentation: Presentation,
        shapes: Vec<VectorShape>,
    },
    /// Block content embedded in inline contexts (a component's figures in a table cell).
    Blocks {
        blocks: Vec<Block>,
    },
    Unknown {
        tag: String,
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct LinkTarget {
    /// Project-relative path, or the current document for a fragment-only link.
    pub path: String,
    pub anchor: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Presentation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub class: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Only attributes recognized by the server, including sizing, accessibility, and the
    /// document's existing CSS declarations. Unknown attributes are separate Unknown nodes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
}

impl Presentation {
    /// A presentation with only a class.
    pub fn class(class: impl Into<String>) -> Self {
        Self {
            class: class.into(),
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Image {
    pub src: String,
    pub alt: String,
    pub presentation: Presentation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Cell {
    pub header: bool,
    pub spans: Vec<Span>,
    pub presentation: Presentation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct Verdict {
    pub group: CardGroup,
    pub open: bool,
    pub posted: Option<String>,
    pub decided: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct ThreadEntry {
    pub who: ThreadAuthor,
    pub timestamp: Option<String>,
    pub verdict: Option<String>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ContainerKind {
    Div,
    Section,
    Figure,
    Figcaption,
    Aside,
    Header,
    Footer,
    Address,
    Center,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum InlineKind {
    Span,
    Small,
    Sub,
    Sup,
    Time,
    Mark,
    Kbd,
    Samp,
    Delete,
    Insert,
    Underline,
    Abbreviation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct VectorShape {
    pub kind: VectorKind,
    /// Geometry and paint attributes recognized for this SVG primitive, never markup.
    pub attributes: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum VectorKind {
    Path,
    Circle,
    Ellipse,
    Rect,
    Line,
    Polyline,
    Polygon,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A component's answer as `docs/engineering/PLUGIN_PROTOCOL.md` shows it on the line: each block and span
    /// tagged by its `type` in kebab case, an empty presentation left out of a table, and every
    /// other empty part of a presentation left out of it.
    #[test]
    fn blocks_write_tagged_by_their_type_and_read_back_the_same() {
        let blocks = vec![
            Block::Paragraph {
                spans: vec![Span::Text { text: "A-1".into() }],
            },
            Block::Table {
                head: Vec::new(),
                rows: Vec::new(),
                presentation: Presentation::default(),
            },
            Block::Container {
                tag: ContainerKind::Figcaption,
                presentation: Presentation::class("muted"),
                blocks: vec![Block::Rule],
            },
            Block::SettledCard {
                summary: vec![Span::LineBreak],
                open: false,
                blocks: Vec::new(),
            },
        ];
        let json = serde_json::to_string(&blocks).unwrap();
        assert_eq!(
            json,
            r#"[{"type":"paragraph","spans":[{"type":"text","text":"A-1"}]},{"type":"table","head":[],"rows":[]},{"type":"container","tag":"figcaption","presentation":{"class":"muted"},"blocks":[{"type":"rule"}]},{"type":"settled-card","summary":[{"type":"line-break"}],"open":false,"blocks":[]}]"#
        );
        assert_eq!(serde_json::from_str::<Vec<Block>>(&json).unwrap(), blocks);
    }

    #[test]
    fn a_vocabulary_word_reads_back_as_its_variant_and_an_unknown_one_is_refused() {
        for size in [
            FigureSize::Small,
            FigureSize::Row,
            FigureSize::Large,
            FigureSize::Tile,
            FigureSize::Cell,
            FigureSize::Slot,
            FigureSize::Wide,
        ] {
            assert_eq!(size.as_str().parse::<FigureSize>(), Ok(size));
            let json = serde_json::to_string(&size).unwrap();
            assert_eq!(json, format!("\"{}\"", size.as_str()));
        }
        assert_eq!(
            "auto-approved".parse::<CardState>(),
            Ok(CardState::AutoApproved)
        );
        assert_eq!(
            "huge".parse::<FigureSize>(),
            Err("unknown FigureSize: huge".to_string())
        );
        assert!(Presentation::default().is_empty());
        assert!(!Presentation::class("muted").is_empty());
    }
}
