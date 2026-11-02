//! Document content as data. No field contains rendered HTML. Inline components and media stay
//! inline even inside a table cell; a client can render the same tree on web or desktop.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Media;

macro_rules! vocabulary {
    ($name:ident { $($variant:ident => $word:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
vocabulary!(ShipSize { Small => "small", Row => "row", Large => "large" });
vocabulary!(ShapePivot { Class => "class" });
vocabulary!(RelationshipTable { Destroy => "destroy", Help => "help" });

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
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
    Table {
        head: Vec<Cell>,
        rows: Vec<Vec<Cell>>,
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
    Component {
        component: LiveComponent,
    },
    Media {
        media: Media,
    },
    Image {
        image: Image,
    },
    /// The pictures filling an art slot, in their display order.
    ArtSlot {
        figures: Vec<Block>,
    },
    /// A hull's picture, navigation and facts in a roster/matrix cell.
    Hull {
        blocks: Vec<Block>,
    },
    Figure {
        blocks: Vec<Block>,
        caption: Vec<Block>,
        missing: bool,
    },
    Caption {
        blocks: Vec<Block>,
    },
    Comparison {
        figures: Vec<Block>,
    },
    Matrix {
        matrix: MatrixKind,
        head: Vec<Cell>,
        rows: Vec<Vec<Cell>>,
    },
    MatrixNote {
        blocks: Vec<Block>,
    },
    EmptyRoster {
        blocks: Vec<Block>,
    },
    Group {
        blocks: Vec<Block>,
    },
    ScreenCapture {
        id: String,
        missing: bool,
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
#[serde(tag = "type", rename_all = "kebab-case")]
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
    Component {
        component: LiveComponent,
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
    /// Block content embedded in inline contexts (notably live ship matrices in table cells).
    Blocks {
        blocks: Vec<Block>,
    },
    Unknown {
        tag: String,
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkTarget {
    /// Project-relative path, or the current document for a fragment-only link.
    pub path: String,
    pub anchor: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub src: String,
    pub alt: String,
    pub presentation: Presentation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    pub header: bool,
    pub spans: Vec<Span>,
    pub presentation: Presentation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub group: CardGroup,
    pub open: bool,
    pub posted: Option<String>,
    pub decided: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadEntry {
    pub who: ThreadAuthor,
    pub timestamp: Option<String>,
    pub verdict: Option<String>,
    pub blocks: Vec<Block>,
}

/// Each live component has its own selector and the fully expanded, typed display data. This
/// keeps the current component output (pictures, captions, links, tables and icons) without
/// duplicating its business rules in a client or sending markup. The client renders `blocks`
/// with its document components; the selector provides identity for updates and navigation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LiveComponent {
    Task {
        slug: String,
        label: Option<String>,
        blocks: Vec<Block>,
    },
    Ship {
        id: String,
        size: ShipSize,
        blocks: Vec<Block>,
    },
    Ships {
        ids: Vec<String>,
        size: ShipSize,
        blocks: Vec<Block>,
    },
    Art {
        slot: String,
        blocks: Vec<Block>,
    },
    Screen {
        id: String,
        blocks: Vec<Block>,
    },
    ShipMatrix {
        faction: String,
        blocks: Vec<Block>,
    },
    ShapeMatrix {
        faction: String,
        pivot: Option<ShapePivot>,
        blocks: Vec<Block>,
    },
    SubfactionShips {
        name: String,
        size: ShipSize,
        blocks: Vec<Block>,
    },
    Relationships {
        table: RelationshipTable,
        blocks: Vec<Block>,
    },
    Capability {
        id: String,
        named: bool,
        blocks: Vec<Block>,
    },
    Weapon {
        id: String,
        named: bool,
        blocks: Vec<Block>,
    },
    Gear {
        id: String,
        named: bool,
        blocks: Vec<Block>,
    },
    Status {
        id: String,
        named: bool,
        blocks: Vec<Block>,
    },
    Defence {
        id: String,
        named: bool,
        blocks: Vec<Block>,
    },
}

impl LiveComponent {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Task { .. } => "task",
            Self::Ship { .. } => "ship",
            Self::Ships { .. } => "ships",
            Self::Art { .. } => "art",
            Self::Screen { .. } => "screen",
            Self::ShipMatrix { .. } => "ship-matrix",
            Self::ShapeMatrix { .. } => "shape-matrix",
            Self::SubfactionShips { .. } => "subfaction-ships",
            Self::Relationships { .. } => "relationships",
            Self::Capability { .. } => "capability",
            Self::Weapon { .. } => "weapon",
            Self::Gear { .. } => "gear",
            Self::Status { .. } => "status",
            Self::Defence { .. } => "defence",
        }
    }
    pub fn blocks(&self) -> &[Block] {
        match self {
            Self::Task { blocks, .. }
            | Self::Ship { blocks, .. }
            | Self::Ships { blocks, .. }
            | Self::Art { blocks, .. }
            | Self::Screen { blocks, .. }
            | Self::ShipMatrix { blocks, .. }
            | Self::ShapeMatrix { blocks, .. }
            | Self::SubfactionShips { blocks, .. }
            | Self::Relationships { blocks, .. }
            | Self::Capability { blocks, .. }
            | Self::Weapon { blocks, .. }
            | Self::Gear { blocks, .. }
            | Self::Status { blocks, .. }
            | Self::Defence { blocks, .. } => blocks,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MatrixKind {
    Ships,
    Shapes,
    Relationships,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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
#[serde(rename_all = "kebab-case")]
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
pub struct VectorShape {
    pub kind: VectorKind,
    /// Geometry and paint attributes recognized for this SVG primitive, never markup.
    pub attributes: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VectorKind {
    Path,
    Circle,
    Ellipse,
    Rect,
    Line,
    Polyline,
    Polygon,
}
