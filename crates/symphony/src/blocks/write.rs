//! Blocks back to the pipeline's HTML: how a plugin component's blocks travel through the same URL,
//! thumbnail, version and branch preparation a document's markup gets, and come out of [`super::parse`]
//! as the blocks they went in as. A media item is handed to `media`, which returns the index its
//! placeholder names.

use symphony_api::{Block, Cell, ContainerKind, InlineKind, Media, Presentation, Span, VectorKind};

use crate::markup::escape;

/// `blocks` as fragment HTML.
pub fn html(blocks: &[Block], media: &mut dyn FnMut(Media) -> usize) -> String {
    let mut writer = Writer {
        out: String::new(),
        media,
    };
    writer.blocks(blocks);
    writer.out
}

/// `spans` as fragment HTML: a paragraph's content without the paragraph.
pub fn spans_html(spans: &[Span], media: &mut dyn FnMut(Media) -> usize) -> String {
    let mut writer = Writer {
        out: String::new(),
        media,
    };
    writer.spans(spans);
    writer.out
}

struct Writer<'a> {
    out: String,
    media: &'a mut dyn FnMut(Media) -> usize,
}

impl Writer<'_> {
    fn open(&mut self, tag: &str, presentation: &Presentation, extra: &[(&str, &str)]) {
        self.out.push('<');
        self.out.push_str(tag);
        for (name, value) in extra {
            self.attribute(name, value);
        }
        for (name, value) in [
            ("class", &presentation.class),
            ("id", &presentation.id),
            ("title", &presentation.title),
        ] {
            if !value.is_empty() {
                self.attribute(name, value);
            }
        }
        for (name, value) in &presentation.attributes {
            self.attribute(name, value);
        }
        self.out.push('>');
    }

    fn attribute(&mut self, name: &str, value: &str) {
        self.out.push_str(&format!(" {name}=\"{}\"", escape(value)));
    }

    fn close(&mut self, tag: &str) {
        self.out.push_str(&format!("</{tag}>"));
    }

    fn wrapped(&mut self, tag: &str, presentation: &Presentation, blocks: &[Block]) {
        self.open(tag, presentation, &[]);
        self.blocks(blocks);
        self.close(tag);
    }

    fn blocks(&mut self, blocks: &[Block]) {
        for block in blocks {
            self.block(block);
        }
    }

    fn block(&mut self, block: &Block) {
        let plain = Presentation::default();
        match block {
            Block::Heading {
                level,
                anchor,
                spans,
            } => {
                let tag = format!("h{level}");
                let id = Presentation {
                    id: anchor.clone(),
                    ..Presentation::default()
                };
                self.open(&tag, &id, &[]);
                self.spans(spans);
                self.close(&tag);
            }
            Block::Paragraph { spans } => {
                self.open("p", &plain, &[]);
                self.spans(spans);
                self.close("p");
            }
            Block::List {
                ordered,
                start,
                items,
            } => {
                let tag = if *ordered { "ol" } else { "ul" };
                let start = start.to_string();
                let extra: &[(&str, &str)] = if *ordered && start != "1" {
                    &[("start", &start)]
                } else {
                    &[]
                };
                self.open(tag, &plain, extra);
                for item in items {
                    self.wrapped("li", &plain, item);
                }
                self.close(tag);
            }
            Block::Table {
                head,
                rows,
                presentation,
            } => {
                self.open("table", presentation, &[]);
                if !head.is_empty() {
                    self.out.push_str("<thead><tr>");
                    self.cells(head);
                    self.out.push_str("</tr></thead>");
                }
                self.out.push_str("<tbody>");
                for row in rows {
                    self.out.push_str("<tr>");
                    self.cells(row);
                    self.out.push_str("</tr>");
                }
                self.out.push_str("</tbody></table>");
            }
            Block::Code { language, text } => {
                let class = format!("language-{language}");
                let extra: &[(&str, &str)] = if language.is_empty() {
                    &[]
                } else {
                    &[("class", &class)]
                };
                self.out.push_str("<pre>");
                self.open("code", &plain, extra);
                self.out.push_str(&escape(text));
                self.out.push_str("</code></pre>");
            }
            Block::Quote { blocks } => self.wrapped("blockquote", &plain, blocks),
            Block::Rule => self.out.push_str("<hr>"),
            Block::Component { kind, blocks } => {
                self.open("symphony-component", &plain, &[("data-kind", kind)]);
                self.blocks(blocks);
                self.close("symphony-component");
            }
            Block::Media { media } => self.media(media),
            Block::Image { image } => self.image(image),
            Block::Figure {
                picture,
                caption,
                href,
                badge,
                size,
            } => {
                let mut extra = vec![("data-figure", size.as_str())];
                if let Some(badge) = badge {
                    extra.push(("data-badge", badge));
                }
                self.open("figure", &plain, &extra);
                if let Some(href) = href {
                    self.open("a", &plain, &[("href", href)]);
                }
                if let Some(picture) = picture {
                    self.image(picture);
                }
                if href.is_some() {
                    self.close("a");
                }
                if !caption.is_empty() {
                    self.out.push_str("<figcaption>");
                    self.spans(caption);
                    self.out.push_str("</figcaption>");
                }
                self.close("figure");
            }
            Block::Caption { blocks } => self.wrapped("figcaption", &plain, blocks),
            Block::Comparison { figures } => {
                self.wrapped("div", &Presentation::class("compare"), figures);
            }
            Block::Group { blocks } => self.wrapped("div", &plain, blocks),
            Block::Container {
                tag,
                presentation,
                blocks,
            } => self.wrapped(container_tag(*tag), presentation, blocks),
            Block::Details {
                summary,
                open,
                blocks,
            } => self.details(summary, *open, blocks),
            Block::SettledCard {
                summary,
                open,
                blocks,
            } => {
                self.open("div", &Presentation::class("settled"), &[]);
                self.details(summary, *open, blocks);
                self.close("div");
            }
            Block::Callout {
                kind,
                title,
                resolved,
                blocks,
            } => {
                let class = if *resolved {
                    format!("callout callout-{} callout-done", kind.as_str())
                } else {
                    format!("callout callout-{}", kind.as_str())
                };
                self.open("div", &Presentation::class(class), &[]);
                self.open("div", &Presentation::class("callout-title"), &[]);
                self.spans(title);
                self.close("div");
                self.blocks(blocks);
                self.close("div");
            }
            Block::Thread { entries } => {
                self.open("ul", &Presentation::class("history"), &[]);
                for entry in entries {
                    let class = format!("entry {}", entry.who.as_str());
                    self.open("li", &Presentation::class(class), &[]);
                    if let Some(at) = &entry.timestamp {
                        self.out.push_str(&format!("<time>{}</time>", escape(at)));
                    }
                    if let Some(verdict) = &entry.verdict {
                        self.out.push_str(&format!("<b>{}</b>", escape(verdict)));
                    }
                    self.wrapped("span", &Presentation::class("entry-text"), &entry.blocks);
                    self.close("li");
                }
                self.close("ul");
            }
            // A card belongs to its document's register, never to a component's output: its body
            // is kept, its verdict controls are not.
            Block::Card { body, .. } => self.wrapped("div", &plain, body),
            Block::Comment { text } => self.out.push_str(&format!("<!--{text}-->")),
            Block::Unknown { text, .. } => {
                self.out.push_str(&format!("<p>{}</p>", escape(text)));
            }
        }
    }

    fn details(&mut self, summary: &[Span], open: bool, blocks: &[Block]) {
        let extra: &[(&str, &str)] = if open { &[("open", "")] } else { &[] };
        self.open("details", &Presentation::default(), extra);
        self.out.push_str("<summary>");
        self.spans(summary);
        self.out.push_str("</summary>");
        self.blocks(blocks);
        self.close("details");
    }

    fn cells(&mut self, cells: &[Cell]) {
        for cell in cells {
            let tag = if cell.header { "th" } else { "td" };
            self.open(tag, &cell.presentation, &[]);
            self.spans(&cell.spans);
            self.close(tag);
        }
    }

    fn image(&mut self, image: &symphony_api::Image) {
        self.open(
            "img",
            &image.presentation,
            &[("src", &image.src), ("alt", &image.alt)],
        );
    }

    fn media(&mut self, media: &Media) {
        let kind = match media {
            Media::Mockup { .. } => "mockup",
            Media::Viewer { .. } => "viewer",
            Media::Gallery { .. } => "gallery",
        };
        let index = (self.media)(media.clone());
        self.out.push_str(&format!(
            "<span class=\"media\" data-media=\"{index}\" data-kind=\"{kind}\"></span>"
        ));
    }

    fn spans(&mut self, spans: &[Span]) {
        for span in spans {
            self.span(span);
        }
    }

    fn span(&mut self, span: &Span) {
        let plain = Presentation::default();
        match span {
            Span::Text { text } => self.out.push_str(&escape(text)),
            Span::Emphasis { spans } => self.inline("em", &plain, spans),
            Span::Strong { spans } => self.inline("strong", &plain, spans),
            Span::Code { text } => self.out.push_str(&format!("<code>{}</code>", escape(text))),
            Span::Link { .. } => self.link(&plain, span),
            Span::WikiLink { href, spans, .. } => {
                self.open("a", &plain, &[("href", href), ("data-wiki", "true")]);
                self.spans(spans);
                self.close("a");
            }
            Span::LineBreak => self.out.push_str("<br>"),
            Span::Image { image } => self.image(image),
            Span::Media { media } => self.media(media),
            Span::Component { kind, blocks } => {
                self.open("symphony-component", &plain, &[("data-kind", kind)]);
                self.blocks(blocks);
                self.close("symphony-component");
            }
            Span::Icon {
                shape,
                label,
                named,
                tip,
            } => {
                let mut extra = vec![("data-icon", label.as_str())];
                if *named {
                    extra.push(("data-named", "true"));
                }
                if !tip.is_empty() {
                    extra.push(("data-tip", tip));
                }
                self.open("span", &plain, &extra);
                self.out.push_str("<svg viewBox=\"0 0 24 24\">");
                self.shapes(shape);
                self.out.push_str("</svg></span>");
            }
            Span::Styled {
                tag,
                presentation,
                spans,
            } => match spans.as_slice() {
                [link @ Span::Link { .. }] if *tag == InlineKind::Span && links(presentation) => {
                    self.link(presentation, link);
                }
                _ => self.inline(inline_tag(*tag), presentation, spans),
            },
            Span::Graphic {
                view_box,
                presentation,
                shapes,
            } => {
                self.open("svg", presentation, &[("viewBox", view_box)]);
                self.shapes(shapes);
                self.close("svg");
            }
            Span::Blocks { blocks } => self.blocks(blocks),
            Span::Unknown { text, .. } => self.out.push_str(&escape(text)),
        }
    }

    /// `link` (a [`Span::Link`]) as one `<a>` carrying `presentation`, written before the link's
    /// own attributes.
    fn link(&mut self, presentation: &Presentation, link: &Span) {
        let Span::Link {
            href,
            target,
            title,
            spans,
        } = link
        else {
            return;
        };
        self.out.push_str("<a");
        for (name, value) in [
            ("class", &presentation.class),
            ("id", &presentation.id),
            ("title", &presentation.title),
        ] {
            if !value.is_empty() {
                self.attribute(name, value);
            }
        }
        for (name, value) in &presentation.attributes {
            self.attribute(name, value);
        }
        self.attribute("href", href);
        // The parser gives every local link a target; a link written without one is a card's,
        // which it marks.
        if target.is_none() && !href.is_empty() && !crate::markup::is_external(href) {
            self.attribute("data-card", "true");
        }
        if !title.is_empty() {
            self.attribute("title", title);
        }
        self.out.push('>');
        self.spans(spans);
        self.close("a");
    }

    fn inline(&mut self, tag: &str, presentation: &Presentation, spans: &[Span]) {
        self.open(tag, presentation, &[]);
        self.spans(spans);
        self.close(tag);
    }

    fn shapes(&mut self, shapes: &[symphony_api::VectorShape]) {
        for shape in shapes {
            let tag = match shape.kind {
                VectorKind::Path => "path",
                VectorKind::Circle => "circle",
                VectorKind::Ellipse => "ellipse",
                VectorKind::Rect => "rect",
                VectorKind::Line => "line",
                VectorKind::Polyline => "polyline",
                VectorKind::Polygon => "polygon",
            };
            self.out.push('<');
            self.out.push_str(tag);
            for (name, value) in &shape.attributes {
                self.attribute(name, value);
            }
            self.out.push_str("/>");
        }
    }
}

fn container_tag(tag: ContainerKind) -> &'static str {
    match tag {
        ContainerKind::Div => "div",
        ContainerKind::Section => "section",
        ContainerKind::Figure => "figure",
        ContainerKind::Figcaption => "figcaption",
        ContainerKind::Aside => "aside",
        ContainerKind::Header => "header",
        ContainerKind::Footer => "footer",
        ContainerKind::Address => "address",
        ContainerKind::Center => "center",
    }
}

fn inline_tag(tag: InlineKind) -> &'static str {
    match tag {
        InlineKind::Span => "span",
        InlineKind::Small => "small",
        InlineKind::Sub => "sub",
        InlineKind::Sup => "sup",
        InlineKind::Time => "time",
        InlineKind::Mark => "mark",
        InlineKind::Kbd => "kbd",
        InlineKind::Samp => "samp",
        InlineKind::Delete => "del",
        InlineKind::Insert => "ins",
        InlineKind::Underline => "u",
        InlineKind::Abbreviation => "abbr",
    }
}

/// Whether `presentation`, around a lone link, belongs to the link: the attributes the parser
/// moves off an `<a>` (the client draws them on the link the same way).
fn links(presentation: &Presentation) -> bool {
    presentation
        .attributes
        .keys()
        .any(|key| matches!(key.as_str(), "target" | "rel" | "download" | "data-task"))
        || presentation
            .class
            .split_whitespace()
            .any(|class| class == "tasklink")
}
