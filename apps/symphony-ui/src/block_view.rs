//! Native document markup. The server sends Block/Span data; every element is selected here.

use dioxus::prelude::*;
use symphony_api::{
    Block, Cell, ContainerKind, FigureSize, Image, InlineKind, LinkTarget, Presentation, Span,
    VectorKind, VectorShape,
};

use crate::doc_components as view;

#[cfg(target_arch = "wasm32")]
use crate::viewport::NearViewport;

/// A card supplies its excerpt action. Ordinary document/task links remain ordinary links.
#[derive(Clone, Copy, Default)]
pub struct Excerpts(pub Option<EventHandler<LinkTarget>>);

/// A settled card's outer details owns its anchor while its body is fetched.
#[derive(Clone, Default)]
pub struct CardAnchor(pub Option<String>);

#[derive(Clone, Copy)]
struct Historical;

/// Previous card versions retain their names without owning the current card's anchor or excerpts.
#[component]
pub fn HistoricalBlocks(blocks: Vec<Block>) -> Element {
    use_context_provider(|| Historical);
    use_context_provider(|| Excerpts(None));
    rsx! { BlocksView { blocks } }
}

#[component]
pub fn BlocksView(blocks: Vec<Block>) -> Element {
    rsx! { SuspenseBoundary { fallback: |_| rsx! { p { "Loading content…" } },
        RenderedBlocks { blocks }
    } }
}

#[component(lazy)]
fn RenderedBlocks(blocks: Vec<Block>) -> Element {
    // Detached history comes as one Thread per entry. Join adjacent entries into one timeline.
    let mut joined: Vec<Block> = Vec::new();
    for block in blocks {
        if let Block::Thread { entries } = block {
            if let Some(Block::Thread { entries: previous }) = joined.last_mut() {
                previous.extend(entries);
            } else {
                joined.push(Block::Thread { entries });
            }
        } else {
            joined.push(block);
        }
    }
    rsx! { for (index, block) in joined.into_iter().enumerate() { BlockView { key: "{index}", block } } }
}

#[component]
pub fn SpansView(spans: Vec<Span>) -> Element {
    rsx! { for (index, span) in spans.into_iter().enumerate() { SpanView { key: "{index}", span } } }
}

#[component]
fn BlockView(block: Block) -> Element {
    match block {
        Block::Heading {
            level,
            anchor,
            spans,
        } => rsx! { view::Heading { level, anchor, SpansView { spans } } },
        Block::Paragraph { spans } => rsx! { p { SpansView { spans } } },
        Block::List {
            ordered,
            start,
            items,
        } => rsx! {
            view::DocumentList { ordered, start,
                for (index, blocks) in items.into_iter().enumerate() {
                    li { key: "{index}", Deferred { height: 32, BlocksView { blocks } } }
                }
            }
        },
        Block::Table {
            head,
            rows,
            presentation,
        } => rsx! { TableView { head, rows, presentation } },
        Block::Code { language, text } => rsx! { view::Code { language, text, block: true } },
        Block::Quote { blocks } => rsx! { blockquote { BlocksView { blocks } } },
        Block::Rule => rsx! { hr {} },
        Block::Callout {
            kind,
            title,
            resolved,
            blocks,
        } => {
            let title = title.into_iter().filter(|span| !matches!(span, Span::Styled { presentation, .. } if presentation.class.split_whitespace().any(|class| class == "callout-kind"))).collect();
            rsx! { view::Callout { tone: kind.as_str().to_string(), done: resolved,
                title: rsx! { SpansView { spans: title } }, BlocksView { blocks }
            } }
        }
        Block::Card {
            slug,
            mark,
            title,
            body,
            verdict,
            thread,
            comparison,
            ..
        } => {
            let anchor = try_consume_context::<Historical>().is_none()
                && try_consume_context::<CardAnchor>()
                    .is_none_or(|suppressed| suppressed.0.as_deref() != Some(slug.as_str()));
            rsx! { view::Decision { slug, state: mark.as_str().to_string(), group: verdict.group.as_str().to_string(), open: verdict.open,
                posted: verdict.posted.unwrap_or_default(), decided: verdict.decided.unwrap_or_default(), anchor,
                body: rsx! { if body.is_empty() { "{title}" } else { BlocksView { blocks: body } } },
                comparison: rsx! { BlocksView { blocks: comparison } if !thread.is_empty() { ThreadView { entries: thread } } }
            } }
        }
        Block::Component { kind, blocks } => rsx! { ComponentView { kind, blocks } },
        Block::Media { media } => rsx! { crate::media::MediaView { media } },
        Block::Image { image } => rsx! { crate::media::DocumentPicture { image } },
        Block::Figure {
            picture,
            caption,
            href,
            badge,
            size,
        } => rsx! { FigureView { picture, caption, href, badge, size } },
        Block::Caption { blocks } => rsx! { figcaption { BlocksView { blocks } } },
        Block::Comparison { figures } => {
            rsx! { div { class: "compare", BlocksView { blocks: figures } } }
        }
        Block::Group { blocks } => rsx! { BlocksView { blocks } },
        Block::Container {
            tag,
            presentation,
            blocks,
        } => rsx! { ContainerView { tag, presentation, blocks } },
        Block::Details {
            summary,
            open,
            blocks,
        }
        | Block::SettledCard {
            summary,
            open,
            blocks,
        } => rsx! {
            details { open, summary { SpansView { spans: summary } } Deferred { BlocksView { blocks } } }
        },
        Block::Thread { entries } => rsx! { ThreadView { entries } },
        // Comments have no visual content, as in Markdown. Their source remains in the DTO.
        Block::Comment { .. } => rsx! {},
        Block::Unknown { tag, text } => {
            rsx! { view::Unknown { description: tag, attributes: Vec::new(), p { "{text}" } } }
        }
    }
}

#[component]
fn SpanView(span: Span) -> Element {
    match span {
        Span::Text { text } => rsx! { "{text}" },
        Span::Emphasis { spans } => rsx! { em { SpansView { spans } } },
        Span::Strong { spans } => rsx! { strong { SpansView { spans } } },
        Span::Code { text } => rsx! { view::Code { text, language: String::new(), block: false } },
        Span::Link {
            href,
            target,
            title,
            spans,
        } => {
            rsx! { LinkView { href, target, title, spans, presentation: Presentation::default() } }
        }
        Span::WikiLink {
            href,
            target,
            spans,
        } => {
            rsx! { LinkView { href, target, title: String::new(), spans, presentation: Presentation::default() } }
        }
        Span::LineBreak => rsx! { br {} },
        Span::Image { image } => rsx! { crate::media::DocumentPicture { image } },
        Span::Media { media } => rsx! { crate::media::MediaView { media } },
        Span::Component { kind, blocks } => rsx! { ComponentView { kind, blocks, inline: true } },
        Span::Icon {
            shape,
            label,
            named,
            tip,
        } => rsx! { IconView { shape, label, named, tip } },
        Span::Styled {
            tag,
            presentation,
            spans,
        } => rsx! { InlineView { tag, presentation, spans } },
        Span::Graphic {
            view_box,
            presentation,
            shapes,
        } => rsx! { GraphicView { view_box, presentation, shapes } },
        Span::Blocks { blocks } => rsx! { BlocksView { blocks } },
        Span::Unknown { tag, text } => {
            rsx! { span { class: "unknown-block", role: "alert", "Unsupported {tag}: {text}" } }
        }
    }
}

#[component]
fn LinkView(
    href: String,
    target: Option<LinkTarget>,
    title: String,
    spans: Vec<Span>,
    presentation: Presentation,
) -> Element {
    let attrs = attributes(&presentation);
    let opener = try_consume_context::<Excerpts>().and_then(|context| context.0);
    rsx! {
        a { href: crate::project::href(&href), title, ..attrs, SpansView { spans } }
        if let (Some(open), Some(target)) = (opener, target.filter(|target| target.anchor.is_some())) {
            crate::components::button::Button { class: "show-link", r#type: "button", onclick: move |event: MouseEvent| { event.stop_propagation(); open.call(target.clone()); }, "▸ show" }
        }
        PresentationWarnings { presentation }
    }
}

/// A plugin's component: its blocks inside a wrapper naming its kind, which lays out nothing
/// itself and is there for a project's stylesheet to select. In a line of text, one paragraph's
/// spans stay in that line.
#[component]
fn ComponentView(kind: String, blocks: Vec<Block>, #[props(default)] inline: bool) -> Element {
    if inline {
        if let [Block::Paragraph { spans }] = blocks.as_slice() {
            let spans = spans.clone();
            return rsx! { span { "data-component": kind, SpansView { spans } } };
        }
        return rsx! { span { "data-component": kind, BlocksView { blocks } } };
    }
    rsx! { div { "data-component": kind, BlocksView { blocks } } }
}

/// A picture with its caption. An item's sizes (`small`, `row`, `large`, `tile`) draw a link
/// card, its badge standing in for a missing picture; the others draw a figure, `missing`
/// without one.
#[component]
fn FigureView(
    picture: Option<Image>,
    caption: Vec<Span>,
    href: Option<String>,
    badge: Option<String>,
    size: FigureSize,
) -> Element {
    let class = format!("figure figure-{}", size.as_str());
    if matches!(
        size,
        FigureSize::Small | FigureSize::Row | FigureSize::Large | FigureSize::Tile
    ) {
        let content = rsx! {
            if let Some(image) = picture { crate::media::DocumentPicture { image } }
            else if let Some(badge) = badge { span { class: "figure-badge", "{badge}" } }
            span { class: "figure-caption", SpansView { spans: caption } }
        };
        return match href {
            Some(href) => rsx! { a { class, href: crate::project::href(&href), {content} } },
            None => rsx! { span { class, {content} } },
        };
    }
    let class = if picture.is_none() {
        format!("{class} missing")
    } else {
        class
    };
    rsx! {
        figure { class,
            if let Some(image) = picture {
                Deferred {
                    if let Some(href) = href { a { href: crate::project::href(&href), crate::media::DocumentPicture { image } } }
                    else { crate::media::DocumentPicture { image } }
                }
            } else if let Some(badge) = badge { span { class: "figure-badge", "{badge}" } }
            if !caption.is_empty() { figcaption { SpansView { spans: caption } } }
        }
    }
}

/// An icon on the 24-unit stroke grid: its label is its accessible name, its tip shows on hover
/// and focus, and `named` shows the label after it.
#[component]
fn IconView(shape: Vec<VectorShape>, label: String, named: bool, tip: String) -> Element {
    let title = (!tip.is_empty()).then(|| tip.clone());
    let tip = (!tip.is_empty()).then_some(tip);
    rsx! {
        span { class: "icon", "data-tip": tip, title, tabindex: "0", role: "img", "aria-label": label.clone(),
            svg { view_box: "0 0 24 24", width: "18", height: "18", fill: "none", stroke: "currentColor",
                stroke_width: "1.8", stroke_linecap: "round", stroke_linejoin: "round", "aria-hidden": "true",
                for shape in &shape { ShapeView { shape: shape.clone() } }
            }
            if named { span { class: "icon-name", "{label}" } }
        }
        for shape in shape.clone() { AttributeWarnings { values: shape.attributes } }
    }
}

#[component]
fn ContainerView(tag: ContainerKind, presentation: Presentation, blocks: Vec<Block>) -> Element {
    let attrs = attributes(&presentation);
    let content = rsx! { BlocksView { blocks } };
    let shown = match tag {
        ContainerKind::Div => rsx! { div { ..attrs, {content} } },
        ContainerKind::Section => rsx! { section { ..attrs, {content} } },
        ContainerKind::Figure => rsx! { figure { ..attrs, {content} } },
        ContainerKind::Figcaption => rsx! { figcaption { ..attrs, {content} } },
        ContainerKind::Aside => rsx! { aside { ..attrs, {content} } },
        ContainerKind::Header => rsx! { header { ..attrs, {content} } },
        ContainerKind::Footer => rsx! { footer { ..attrs, {content} } },
        ContainerKind::Address => rsx! { address { ..attrs, {content} } },
        ContainerKind::Center => rsx! { div { style: "text-align:center", ..attrs, {content} } },
    };
    rsx! { {shown} PresentationWarnings { presentation } }
}

#[component]
fn InlineView(tag: InlineKind, presentation: Presentation, spans: Vec<Span>) -> Element {
    // The parser wraps attributes belonging to a link in a presentation span. Apply the
    // navigation/sizing attributes to the link itself (not its surrounding text).
    let link_attributes = presentation
        .attributes
        .keys()
        .any(|key| matches!(key.as_str(), "target" | "rel" | "download" | "data-task"))
        || presentation
            .class
            .split_whitespace()
            .any(|class| class == "tasklink");
    if tag == InlineKind::Span && link_attributes && spans.len() == 1 {
        match &spans[0] {
            Span::Link {
                href,
                target,
                title,
                spans,
            } => {
                return rsx! { LinkView { href: href.clone(), target: target.clone(), title: title.clone(), spans: spans.clone(), presentation } };
            }
            Span::WikiLink {
                href,
                target,
                spans,
            } => {
                return rsx! { LinkView { href: href.clone(), target: target.clone(), title: String::new(), spans: spans.clone(), presentation } };
            }
            _ => {}
        }
    }
    let attrs = attributes(&presentation);
    let content = rsx! { SpansView { spans } };
    let shown = match tag {
        InlineKind::Span => rsx! { span { ..attrs, {content} } },
        InlineKind::Small => rsx! { small { ..attrs, {content} } },
        InlineKind::Sub => rsx! { sub { ..attrs, {content} } },
        InlineKind::Sup => rsx! { sup { ..attrs, {content} } },
        InlineKind::Time => rsx! { time { ..attrs, {content} } },
        InlineKind::Mark => rsx! { mark { ..attrs, {content} } },
        InlineKind::Kbd => rsx! { kbd { ..attrs, {content} } },
        InlineKind::Samp => rsx! { samp { ..attrs, {content} } },
        InlineKind::Delete => rsx! { del { ..attrs, {content} } },
        InlineKind::Insert => rsx! { ins { ..attrs, {content} } },
        InlineKind::Underline => rsx! { u { ..attrs, {content} } },
        InlineKind::Abbreviation => rsx! { abbr { ..attrs, {content} } },
    };
    rsx! { {shown} PresentationWarnings { presentation } }
}

#[component]
fn TableView(head: Vec<Cell>, rows: Vec<Vec<Cell>>, presentation: Presentation) -> Element {
    let attrs = attributes(&presentation);
    rsx! {
        table { ..attrs,
            if !head.is_empty() { thead { tr { for cell in head { CellView { cell } } } } }
            tbody { for (index, cells) in rows.into_iter().enumerate() { TableRow { key: "{index}", cells } } }
        }
        PresentationWarnings { presentation }
    }
}

#[component]
fn TableRow(cells: Vec<Cell>) -> Element {
    let (shown, reveal) = crate::viewport::use_visibility(false);
    rsx! { tr { ..reveal,
        if shown() { for cell in cells { CellView { cell } } }
        else { td { colspan: "{cells.len()}", style: "height:48px", "aria-hidden": "true" } }
    } }
}

#[component]
fn CellView(cell: Cell) -> Element {
    let attrs = attributes(&cell.presentation);
    let content = rsx! { SpansView { spans: cell.spans } PresentationWarnings { presentation: cell.presentation } };
    if cell.header {
        rsx! { th { ..attrs, {content} } }
    } else {
        rsx! { td { ..attrs, {content} } }
    }
}

#[component]
fn ThreadView(entries: Vec<symphony_api::ThreadEntry>) -> Element {
    let awaiting = entries.last().is_some_and(|entry| {
        entry.who == symphony_api::ThreadAuthor::Owner && entry.verdict.is_none()
    });
    rsx! { ul { class: "history",
        for (index, entry) in entries.into_iter().enumerate() { li { key: "{index}", class: "entry {entry.who.as_str()}",
            Deferred { height: 60,
                if let Some(at) = entry.timestamp { time { "{at}" } " " }
                if let Some(verdict) = entry.verdict { b { class: "entry-verdict", "{verdict}" } " " }
                span { class: "entry-text", BlocksView { blocks: entry.blocks } }
                if entry.who == symphony_api::ThreadAuthor::Unknown { span { role: "alert", "Unknown thread author" } }
            }
        } }
    }
    if awaiting { p { class: "awaiting", "Waiting for an answer…" } }
    }
}

#[component]
fn GraphicView(view_box: String, presentation: Presentation, shapes: Vec<VectorShape>) -> Element {
    let attrs = attributes(&presentation);
    rsx! {
        svg { view_box, ..attrs, for shape in &shapes { ShapeView { shape: shape.clone() } } }
        PresentationWarnings { presentation }
        for shape in shapes.clone() { AttributeWarnings { values: shape.attributes } }
    }
}

#[component]
fn ShapeView(shape: VectorShape) -> Element {
    let attrs = map_attributes(&shape.attributes);
    match shape.kind {
        VectorKind::Path => rsx! { path { ..attrs } },
        VectorKind::Circle => rsx! { circle { ..attrs } },
        VectorKind::Ellipse => rsx! { ellipse { ..attrs } },
        VectorKind::Rect => rsx! { rect { ..attrs } },
        VectorKind::Line => rsx! { line { ..attrs } },
        VectorKind::Polyline => rsx! { polyline { ..attrs } },
        VectorKind::Polygon => rsx! { polygon { ..attrs } },
    }
}

#[component]
fn Deferred(children: Element, #[props(default = 160)] height: u32) -> Element {
    #[cfg(target_arch = "wasm32")]
    return rsx! { NearViewport { height, {children} } };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = height;
        children
    }
}

fn attribute_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "style" => "style",
        "role" => "role",
        "tabindex" => "tabindex",
        "aria-label" => "aria-label",
        "aria-hidden" => "aria-hidden",
        "target" => "target",
        "rel" => "rel",
        "download" => "download",
        "data-task" => "data-task",
        "data-wiki" => "data-wiki",
        "data-full" => "data-full",
        "data-tip" => "data-tip",
        "data-slug" => "data-slug",
        "data-state" => "data-state",
        "data-open" => "data-open",
        "data-group" => "data-group",
        "data-decided" => "data-decided",
        "data-line" => "data-line",
        "data-line-offset" => "data-line-offset",
        "data-screen" => "data-screen",
        "data-kind" => "data-kind",
        "data-media" => "data-media",
        "data-arg" => "data-arg",
        "data-option" => "data-option",
        "width" => "width",
        "height" => "height",
        "loading" => "loading",
        "decoding" => "decoding",
        "scope" => "scope",
        "align" => "align",
        "colspan" => "colspan",
        "rowspan" => "rowspan",
        "fill" => "fill",
        "stroke" => "stroke",
        "stroke-width" => "stroke-width",
        "stroke-dasharray" => "stroke-dasharray",
        "stroke-linecap" => "stroke-linecap",
        "stroke-linejoin" => "stroke-linejoin",
        "d" => "d",
        "cx" => "cx",
        "cy" => "cy",
        "r" => "r",
        "rx" => "rx",
        "ry" => "ry",
        "x" => "x",
        "y" => "y",
        "x1" => "x1",
        "x2" => "x2",
        "y1" => "y1",
        "y2" => "y2",
        "points" => "points",
        _ => return None,
    })
}

fn map_attributes(values: &std::collections::BTreeMap<String, String>) -> Vec<Attribute> {
    values
        .iter()
        .filter_map(|(name, value)| {
            attribute_name(name).map(|name| Attribute::new(name, value.clone(), None, false))
        })
        .collect()
}

fn attributes(presentation: &Presentation) -> Vec<Attribute> {
    let mut attrs = map_attributes(&presentation.attributes);
    if presentation.title.is_empty()
        && let Some(tip) = presentation.attributes.get("data-tip")
    {
        attrs.push(Attribute::new("title", tip.clone(), None, false));
    }
    for (name, value) in [
        ("class", &presentation.class),
        ("id", &presentation.id),
        ("title", &presentation.title),
    ] {
        if !value.is_empty() {
            attrs.push(Attribute::new(name, value.clone(), None, false));
        }
    }
    attrs
}

#[component]
fn PresentationWarnings(presentation: Presentation) -> Element {
    rsx! { AttributeWarnings { values: presentation.attributes } }
}

#[component]
fn AttributeWarnings(values: std::collections::BTreeMap<String, String>) -> Element {
    let unsupported = values
        .into_iter()
        .filter(|(name, _)| attribute_name(name).is_none())
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>();
    if unsupported.is_empty() {
        rsx! {}
    } else {
        rsx! { view::Unknown { description: "attributes".to_string(), attributes: unsupported } }
    }
}
