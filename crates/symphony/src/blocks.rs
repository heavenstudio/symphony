//! The document pipeline's final fragments as a typed tree. HTML is only an internal input here;
//! callers prepare URLs/media as usual, then send the resulting blocks to a component renderer.
//! `Render::typed_sections`, `typed_reply` and `typed_history` retain live-component and wiki-link
//! identity. `parse` also accepts ordinary pipeline fragments (without those optional markers).

mod html;
pub mod write;

use std::collections::BTreeMap;

use symphony_api::{
    Block, CalloutKind, CardGroup, CardState, Cell, ContainerKind, FigureSize, Image, InlineKind,
    LinkTarget, Media, Presentation, Span, ThreadAuthor, ThreadEntry, VectorKind, VectorShape,
    Verdict,
};

use html::Node;

/// Parse a document, section, excerpt, or answer after the pipeline has prepared its local URLs.
/// Unknown elements/attributes stay visible as data and are warned about with `doc_path`.
pub fn parse(html: &str, media: &[Media], doc_path: &str) -> Vec<Block> {
    parse_mode(html, media, doc_path, true)
}

/// The shapes of an SVG's markup (`<path d="…"/>…`), the vocabulary's [`VectorShape`]s: what a
/// catalogue written as SVG text holds, for a [`Span::Icon`] or a [`Span::Graphic`].
pub fn vector_shapes(markup: &str) -> Vec<VectorShape> {
    let parser = Parser {
        media: &[],
        doc_path: "",
        semantic: false,
    };
    parser
        .shapes(&Node {
            tag: "svg".into(),
            children: html::parse(markup),
            ..Node::default()
        })
        .0
}

fn parse_mode(html: &str, media: &[Media], doc_path: &str, semantic: bool) -> Vec<Block> {
    let nodes = html::parse(html);
    let parser = Parser {
        media,
        doc_path,
        semantic,
    };
    let mut blocks = parser.blocks(&nodes);
    parser.attributes(&nodes, &mut blocks);
    blocks
}

/// Convenience for an unprepared pipeline fragment. URL preparation/scoping remains the caller's
/// job, so routes can prepare/cache once and parse precisely the content they would have sent.
pub fn fragment(fragment: &crate::render::Fragment, doc_path: &str) -> Vec<Block> {
    parse(&fragment.html, &fragment.media, doc_path)
}

pub fn section(section: &crate::render::Section, doc_path: &str) -> Vec<Block> {
    parse(&section.html, &section.media, doc_path)
}

struct Parser<'a> {
    media: &'a [Media],
    doc_path: &'a str,
    semantic: bool,
}

impl Parser<'_> {
    fn unknown(&self, tag: &str, text: String) -> Block {
        if self.semantic {
            eprintln!("blocks: {}: unsupported {tag}", self.doc_path);
        }
        Block::Unknown {
            tag: tag.into(),
            text,
        }
    }

    fn blocks(&self, nodes: &[Node]) -> Vec<Block> {
        let mut out = Vec::new();
        let mut inline = Vec::new();
        for node in nodes {
            if node.is_inline() {
                inline.push(node.clone());
                continue;
            }
            self.flush(&mut inline, &mut out);
            out.extend(self.block(node));
        }
        self.flush(&mut inline, &mut out);
        out
    }

    fn flush(&self, inline: &mut Vec<Node>, blocks: &mut Vec<Block>) {
        if inline
            .iter()
            .any(|n| !n.text.trim().is_empty() || !n.tag.is_empty())
        {
            blocks.push(paragraph(self.spans(inline)));
        }
        inline.clear();
    }

    fn block(&self, node: &Node) -> Vec<Block> {
        let children = &node.children;
        let one = match node.tag.as_str() {
            "#comment" => Block::Comment {
                text: node.text.clone(),
            },
            "#error" => self.unknown("malformed", node.text.clone()),
            "p" => paragraph(self.spans(children)),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Block::Heading {
                level: node.tag.as_bytes()[1] - b'0',
                anchor: node.attr("id").into(),
                spans: self.spans(children),
            },
            "hr" => Block::Rule,
            "blockquote" => Block::Quote {
                blocks: self.blocks(children),
            },
            "pre" => {
                let code = children.iter().position(|c| c.tag == "code");
                let one = Block::Code {
                    language: code
                        .map(|at| &children[at])
                        .unwrap_or(node)
                        .attr("class")
                        .split_whitespace()
                        .find_map(|c| c.strip_prefix("language-"))
                        .unwrap_or("")
                        .into(),
                    text: node.plain(),
                };
                let mut unrepresented = Vec::new();
                for (at, child) in children.iter().enumerate() {
                    self.pre_attributes(child, code == Some(at), &mut unrepresented);
                }
                if unrepresented.is_empty() {
                    one
                } else {
                    unrepresented.insert(0, one);
                    Block::Group {
                        blocks: unrepresented,
                    }
                }
            }
            "ul" | "ol" if node.class("history") => Block::Thread {
                entries: children
                    .iter()
                    .filter(|n| n.tag == "li")
                    .map(|n| self.entry(n))
                    .collect(),
            },
            "li" if node.class("entry") => Block::Thread {
                entries: vec![self.entry(node)],
            },
            "ul" | "ol" => Block::List {
                ordered: node.tag == "ol",
                start: node.attr("start").parse().unwrap_or(1),
                items: children
                    .iter()
                    .filter(|n| !n.blank())
                    .map(|n| {
                        if n.tag == "li" {
                            self.blocks(&n.children)
                        } else {
                            self.block(n)
                        }
                    })
                    .collect(),
            },
            "table" => self.table(node),
            "div" if node.class("decision") => self.card(node),
            "div" | "details" if node.class("callout") => {
                let head = children.iter().find(|n| n.class("callout-title"));
                let name = node
                    .attr("class")
                    .split_whitespace()
                    .find_map(|c| c.strip_prefix("callout-").filter(|c| *c != "done"))
                    .unwrap_or("comment");
                let Ok(kind) = name.parse::<CalloutKind>() else {
                    let mut blocks = vec![self.unknown("callout-kind", name.into())];
                    blocks.extend(self.blocks(children));
                    return vec![Block::Group { blocks }];
                };
                Block::Callout {
                    kind,
                    title: head.map(|n| self.spans(&n.children)).unwrap_or_default(),
                    resolved: node.class("callout-done"),
                    blocks: self.blocks(
                        &children
                            .iter()
                            .filter(|n| !n.class("callout-title"))
                            .cloned()
                            .collect::<Vec<_>>(),
                    ),
                }
            }
            "details" => Block::Details {
                summary: children
                    .iter()
                    .find(|n| n.tag == "summary")
                    .map(|n| self.spans(&n.children))
                    .unwrap_or_default(),
                open: node.attrs.contains_key("open"),
                blocks: self.blocks(
                    &children
                        .iter()
                        .filter(|n| n.tag != "summary")
                        .cloned()
                        .collect::<Vec<_>>(),
                ),
            },
            "symphony-component" => Block::Component {
                kind: node.attr("data-kind").into(),
                blocks: self.blocks(children),
            },
            "figure" if node.attrs.contains_key("data-figure") => match self.figure(node) {
                Some(figure) => figure,
                None => {
                    return vec![
                        self.unknown("figure-size", node.attr("data-figure").into()),
                        self.container(
                            ContainerKind::Figure,
                            Presentation::default(),
                            self.blocks(children),
                        ),
                    ];
                }
            },
            tag => {
                if let Some(tag) = container(tag) {
                    return vec![self.container(
                        tag,
                        presentation(node, &[]),
                        self.blocks(children),
                    )];
                }
                if node.is_inline() {
                    return vec![Block::Paragraph {
                        spans: self.spans(std::slice::from_ref(node)),
                    }];
                }
                // The element is said where it stands, by its name alone: what it holds is drawn
                // once, after it, never inside the notice as well.
                return vec![
                    self.unknown(tag, String::new()),
                    self.container(
                        ContainerKind::Div,
                        Presentation::default(),
                        self.blocks(children),
                    ),
                ];
            }
        };
        let consumed = match node.tag.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => &["id"][..],
            "ol" => &["start"][..],
            "details" => &["open"][..],
            _ => &[],
        };
        // Cards/callouts/components/figures carry their own semantic presentation, and a table
        // its own; ordinary Markdown retains any explicit styling via a structural wrapper.
        let semantic = node.class("decision")
            || node.class("callout")
            || node.class("history")
            || node.class("entry")
            || node.tag == "symphony-component"
            || node.tag == "table"
            || matches!(one, Block::Figure { .. });
        let p = presentation(node, consumed);
        if !semantic && p != Presentation::default() {
            vec![self.container(ContainerKind::Div, p, vec![one])]
        } else {
            vec![one]
        }
    }

    fn container(&self, tag: ContainerKind, p: Presentation, mut blocks: Vec<Block>) -> Block {
        if self.semantic {
            let class = |name| p.class.split_whitespace().any(|c| c == name);
            if class("settled")
                && promotable(&p, &["settled"], &[])
                && blocks.len() == 1
                && matches!(blocks[0], Block::Details { .. })
                && let Some(Block::Details {
                    summary,
                    open,
                    blocks,
                }) = blocks.pop()
            {
                return Block::SettledCard {
                    summary,
                    open,
                    blocks,
                };
            }
            if class("compare") && promotable(&p, &["compare"], &[]) {
                return Block::Comparison { figures: blocks };
            }
            if tag == ContainerKind::Figcaption && promotable(&p, &[], &[]) {
                return Block::Caption { blocks };
            }
            if tag == ContainerKind::Div && p == Presentation::default() {
                return Block::Group { blocks };
            }
        }
        Block::Container {
            tag,
            presentation: p,
            blocks,
        }
    }

    /// A preformatted block keeps plain text, not its children's element structure. Report any
    /// child attributes that flattening cannot carry; only the first code child's language is used.
    fn pre_attributes(&self, node: &Node, language: bool, out: &mut Vec<Block>) {
        for (key, value) in &node.attrs {
            if !known_attribute(&node.tag, key) {
                continue;
            } // The ordinary attribute scan reports it.
            let residual = if language && key == "class" {
                let mut consumed = false;
                value
                    .split_whitespace()
                    .filter(|class| {
                        if !consumed && class.starts_with("language-") {
                            consumed = true;
                            false
                        } else {
                            true
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                value.clone()
            };
            if !residual.is_empty() || value.is_empty() {
                out.push(self.unknown(&format!("pre/{}@{key}", node.tag), residual));
            }
        }
        for child in &node.children {
            self.pre_attributes(child, false, out);
        }
    }

    fn spans(&self, nodes: &[Node]) -> Vec<Span> {
        nodes.iter().flat_map(|node| self.span(node)).collect()
    }

    fn span(&self, node: &Node) -> Vec<Span> {
        let spans = || self.spans(&node.children);
        let one = match node.tag.as_str() {
            "" => Span::Text {
                text: node.text.clone(),
            },
            "em" | "i" => Span::Emphasis { spans: spans() },
            "strong" | "b" => Span::Strong { spans: spans() },
            "code" => Span::Code { text: node.plain() },
            "br" => Span::LineBreak,
            "a" if node.attr("data-wiki") == "true" => Span::WikiLink {
                href: node.attr("href").into(),
                target: self.link_target(node.attr("href")),
                spans: spans(),
            },
            "a" => Span::Link {
                href: node.attr("href").into(),
                target: (node.attr("data-card") != "true")
                    .then(|| self.link_target(node.attr("href")))
                    .flatten(),
                title: node.attr("title").into(),
                spans: spans(),
            },
            "img" => Span::Image {
                image: Image {
                    src: node.attr("src").into(),
                    alt: node.attr("alt").into(),
                    presentation: presentation(node, &["src", "alt"]),
                },
            },
            "span" if node.class("media") => {
                match node
                    .attr("data-media")
                    .parse::<usize>()
                    .ok()
                    .and_then(|i| self.media.get(i))
                {
                    Some(media) => Span::Media {
                        media: media.clone(),
                    },
                    None => {
                        let Block::Unknown { tag, text } =
                            self.unknown("media-index", node.attr("data-media").into())
                        else {
                            unreachable!()
                        };
                        Span::Unknown { tag, text }
                    }
                }
            }
            "symphony-component" => Span::Component {
                kind: node.attr("data-kind").into(),
                blocks: self.blocks(&node.children),
            },
            "span" if node.attrs.contains_key("data-icon") => Span::Icon {
                shape: node
                    .children
                    .iter()
                    .find(|child| child.tag == "svg")
                    .map(|svg| self.shapes(svg).0)
                    .unwrap_or_default(),
                label: node.attr("data-icon").into(),
                named: node.attr("data-named") == "true",
                tip: node.attr("data-tip").into(),
            },
            "svg" => return vec![self.graphic(node)],
            tag => {
                if let Some(tag) = inline(tag) {
                    return vec![Span::Styled {
                        tag,
                        presentation: presentation(node, &[]),
                        spans: spans(),
                    }];
                }
                if !node.is_inline() {
                    return vec![Span::Blocks {
                        blocks: self.block(node),
                    }];
                }
                let Block::Unknown { tag, text } = self.unknown(tag, node.plain()) else {
                    unreachable!()
                };
                return vec![
                    Span::Unknown { tag, text },
                    Span::Styled {
                        tag: InlineKind::Span,
                        presentation: Presentation::default(),
                        spans: spans(),
                    },
                ];
            }
        };
        let consumed = match node.tag.as_str() {
            "a" => &["href", "title", "data-wiki", "data-card"][..],
            "img" => return vec![one],
            "symphony-component" => return vec![one],
            "span" if node.class("media") || node.attrs.contains_key("data-icon") => {
                return vec![one];
            }
            _ => &[],
        };
        let p = presentation(node, consumed);
        if p == Presentation::default() {
            vec![one]
        } else {
            vec![Span::Styled {
                tag: InlineKind::Span,
                presentation: p,
                spans: vec![one],
            }]
        }
    }

    fn link_target(&self, href: &str) -> Option<LinkTarget> {
        if crate::markup::is_external(href) || href.is_empty() {
            return None;
        }
        let (path, anchor) = href
            .split_once('#')
            .map_or((href, None), |(p, a)| (p, Some(a.to_string())));
        let path = if path.is_empty() {
            self.doc_path.to_string()
        } else {
            crate::markup::target(path, self.doc_path)
                .trim_start_matches('/')
                .to_string()
        };
        Some(LinkTarget { path, anchor })
    }

    /// A `<figure data-figure="SIZE">` as the writer writes [`Block::Figure`]: its picture, linked by
    /// the anchor around it when it has `href`, and its caption.
    fn figure(&self, node: &Node) -> Option<Block> {
        let size = node.attr("data-figure").parse::<FigureSize>().ok()?;
        let link = node.children.iter().find(|child| child.tag == "a");
        let picture = link
            .map_or(node.children.as_slice(), |link| link.children.as_slice())
            .iter()
            .find(|child| child.tag == "img")
            .map(|img| Image {
                src: img.attr("src").into(),
                alt: img.attr("alt").into(),
                presentation: presentation(img, &["src", "alt"]),
            });
        Some(Block::Figure {
            picture,
            caption: node
                .children
                .iter()
                .find(|child| child.tag == "figcaption")
                .map(|caption| self.spans(&caption.children))
                .unwrap_or_default(),
            href: link.map(|link| link.attr("href").to_string()),
            badge: Some(node.attr("data-badge"))
                .filter(|badge| !badge.is_empty())
                .map(str::to_string),
            size,
        })
    }

    fn table(&self, node: &Node) -> Block {
        let mut head = Vec::new();
        let mut rows = Vec::new();
        let mut extra = Vec::new();
        for child in &node.children {
            let (in_head, candidates) = match child.tag.as_str() {
                "thead" => (true, child.children.as_slice()),
                "tbody" | "tfoot" => (false, child.children.as_slice()),
                "tr" => (false, std::slice::from_ref(child)),
                _ if child.blank() => continue,
                _ => {
                    extra.extend(self.block(child));
                    continue;
                }
            };
            for row in candidates.iter().filter(|n| !n.blank()) {
                if row.tag != "tr" {
                    extra.extend(self.block(row));
                    continue;
                }
                let cells: Vec<_> = row
                    .children
                    .iter()
                    .filter(|n| !n.blank())
                    .map(|cell| {
                        if cell.tag == "td" || cell.tag == "th" {
                            Cell {
                                header: cell.tag == "th",
                                spans: self.spans(&cell.children),
                                presentation: presentation(cell, &[]),
                            }
                        } else {
                            Cell {
                                header: false,
                                spans: vec![Span::Blocks {
                                    blocks: self.block(cell),
                                }],
                                presentation: Presentation::default(),
                            }
                        }
                    })
                    .collect();
                if in_head && head.is_empty() {
                    head = cells;
                } else {
                    rows.push(cells);
                }
            }
        }
        let table = Block::Table {
            head,
            rows,
            presentation: presentation(node, &[]),
        };
        if extra.is_empty() {
            table
        } else {
            extra.insert(0, table);
            Block::Container {
                tag: ContainerKind::Div,
                presentation: Presentation::default(),
                blocks: extra,
            }
        }
    }

    fn card(&self, node: &Node) -> Block {
        let mark = node.attr("data-state").parse::<CardState>();
        let group = node.attr("data-group").parse::<CardGroup>();
        let (Ok(mark), Ok(group)) = (mark, group) else {
            let mut blocks = vec![self.unknown(
                "card-state/group",
                format!("{} / {}", node.attr("data-state"), node.attr("data-group")),
            )];
            blocks.extend(self.blocks(&node.children));
            return Block::Group { blocks };
        };
        let content = node.children.iter().find(|n| n.class("decision-text"));
        let body_nodes: Vec<_> = content
            .map(|n| {
                n.children
                    .iter()
                    .filter(|c| c.tag != "code" && !c.class("posted"))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let title = body_nodes
            .iter()
            .map(Node::plain)
            .collect::<String>()
            .trim()
            .to_string();
        let history = node.children.iter().find(|n| n.class("history"));
        let posted = content
            .and_then(|n| n.children.iter().find(|n| n.class("posted")))
            .map(Node::plain);
        let mut body = self.blocks(&body_nodes);
        body.extend(
            self.blocks(
                &node
                    .children
                    .iter()
                    .filter(|n| {
                        !n.blank()
                            && !n.class("decision-text")
                            && !n.class("history")
                            && !n.class("compare")
                    })
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
        );
        Block::Card {
            slug: node.attr("data-slug").into(),
            mark,
            title,
            body,
            verdict: Verdict {
                group,
                open: node.attr("data-open") == "true",
                posted,
                decided: (!node.attr("data-decided").is_empty())
                    .then(|| node.attr("data-decided").to_string()),
            },
            thread: history
                .map(|n| {
                    n.children
                        .iter()
                        .filter(|n| n.tag == "li")
                        .map(|n| self.entry(n))
                        .collect()
                })
                .unwrap_or_default(),
            line: node.attr("data-line").parse().ok(),
            line_offset: node.attr("data-line-offset").parse().ok(),
            comparison: self.blocks(
                &node
                    .children
                    .iter()
                    .filter(|n| n.class("compare"))
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
        }
    }

    fn entry(&self, node: &Node) -> ThreadEntry {
        let mut blocks = Vec::new();
        for child in node
            .children
            .iter()
            .filter(|n| !n.blank() && n.tag != "time" && n.tag != "b")
        {
            if child.class("entry-text") {
                blocks.extend(self.blocks(&child.children));
            } else if child.class(crate::render::BY_CLASS) {
                // A person's name, first of the entry's blocks: the kit's thread entry has no
                // field for it, and the client draws it before the verdict.
                blocks.insert(0, by_block(&child.plain()));
            } else {
                blocks.extend(self.block(child));
            }
        }
        let author = node
            .attr("class")
            .split_whitespace()
            .find(|c| *c != "entry")
            .unwrap_or("owner");
        let who = author.parse::<ThreadAuthor>().unwrap_or_else(|_| {
            blocks.insert(0, self.unknown("thread-author", author.into()));
            ThreadAuthor::Unknown
        });
        ThreadEntry {
            who,
            timestamp: node
                .children
                .iter()
                .find(|n| n.tag == "time")
                .map(Node::plain),
            verdict: node.children.iter().find(|n| n.tag == "b").map(Node::plain),
            blocks,
        }
    }

    fn graphic(&self, node: &Node) -> Span {
        let (shapes, unknown) = self.shapes(node);
        let graphic = Span::Graphic {
            view_box: node.attr("viewbox").into(),
            presentation: presentation(node, &["viewbox"]),
            shapes,
        };
        if unknown.is_empty() {
            graphic
        } else {
            Span::Styled {
                tag: InlineKind::Span,
                presentation: Presentation::default(),
                spans: vec![graphic, Span::Blocks { blocks: unknown }],
            }
        }
    }

    /// An `<svg>`'s shapes, and what it holds that is not one.
    fn shapes(&self, node: &Node) -> (Vec<VectorShape>, Vec<Block>) {
        let mut shapes = Vec::new();
        let mut unknown = Vec::new();
        for shape in &node.children {
            let kind = match shape.tag.as_str() {
                "path" => VectorKind::Path,
                "circle" => VectorKind::Circle,
                "ellipse" => VectorKind::Ellipse,
                "rect" => VectorKind::Rect,
                "line" => VectorKind::Line,
                "polyline" => VectorKind::Polyline,
                "polygon" => VectorKind::Polygon,
                "" if shape.text.trim().is_empty() => continue,
                tag => {
                    unknown.push(self.unknown(tag, shape.plain()));
                    continue;
                }
            };
            shapes.push(VectorShape {
                kind,
                attributes: {
                    let mut attributes = BTreeMap::new();
                    for (key, value) in &shape.attrs {
                        if known_attribute(&shape.tag, key) {
                            attributes.insert(key.clone(), value.clone());
                        }
                    }
                    attributes
                },
            });
        }
        (shapes, unknown)
    }

    fn attributes(&self, nodes: &[Node], out: &mut Vec<Block>) {
        for node in nodes {
            for (key, value) in &node.attrs {
                if !known_attribute(&node.tag, key) {
                    out.push(self.unknown(&format!("{}@{key}", node.tag), value.clone()));
                }
            }
            self.attributes(&node.children, out);
        }
    }
}

/// A semantic variant may consume only the classes/attributes its fields represent. Otherwise
/// retain the original explicit container, including anchor IDs and every presentation field.
fn promotable(p: &Presentation, classes: &[&str], attributes: &[&str]) -> bool {
    p.id.is_empty()
        && p.title.is_empty()
        && p.class
            .split_whitespace()
            .all(|class| classes.contains(&class))
        && p.attributes
            .keys()
            .all(|key| attributes.contains(&key.as_str()))
}

fn paragraph(spans: Vec<Span>) -> Block {
    let visible: Vec<_> = spans
        .iter()
        .filter(|s| !matches!(s, Span::Text { text } if text.trim().is_empty()))
        .collect();
    if let [one] = visible.as_slice() {
        match one {
            Span::Media { media } => {
                return Block::Media {
                    media: media.clone(),
                };
            }
            Span::Image { image } => {
                return Block::Image {
                    image: image.clone(),
                };
            }
            Span::Component { kind, blocks } => {
                return Block::Component {
                    kind: kind.clone(),
                    blocks: blocks.clone(),
                };
            }
            _ => (),
        }
    }
    Block::Paragraph { spans }
}

fn presentation(node: &Node, consumed: &[&str]) -> Presentation {
    let get = |key| {
        if consumed.contains(&key) {
            String::new()
        } else {
            node.attr(key).into()
        }
    };
    Presentation {
        class: get("class"),
        id: get("id"),
        title: get("title"),
        attributes: {
            let mut attributes = BTreeMap::new();
            for (key, value) in &node.attrs {
                if !["class", "id", "title"].contains(&key.as_str())
                    && !consumed.contains(&key.as_str())
                    && known_attribute(&node.tag, key)
                {
                    attributes.insert(key.clone(), value.clone());
                }
            }
            attributes
        },
    }
}

fn known_attribute(tag: &str, key: &str) -> bool {
    // The table model has cells rather than arbitrary row/group nodes. Attributes on those
    // omitted structural elements cannot be represented and must therefore be explicit Unknown.
    if ["thead", "tbody", "tfoot", "tr"].contains(&tag) {
        return false;
    }
    if [
        "class",
        "id",
        "title",
        "style",
        "role",
        "tabindex",
        "aria-label",
        "aria-hidden",
    ]
    .contains(&key)
    {
        return true;
    }
    match tag {
        "a" => [
            "href",
            "target",
            "rel",
            "download",
            "data-task",
            "data-wiki",
            "data-card",
        ]
        .contains(&key),
        "img" => [
            "src",
            "alt",
            "width",
            "height",
            "loading",
            "decoding",
            "data-full",
        ]
        .contains(&key),
        "ol" => key == "start",
        "details" => key == "open",
        "th" | "td" => ["scope", "align", "colspan", "rowspan"].contains(&key),
        "div" => [
            "data-slug",
            "data-state",
            "data-open",
            "data-group",
            "data-decided",
            "data-line",
            "data-line-offset",
        ]
        .contains(&key),
        "figure" => ["data-figure", "data-badge"].contains(&key),
        "span" => [
            "data-media",
            "data-kind",
            "data-tip",
            "data-icon",
            "data-named",
        ]
        .contains(&key),
        "symphony-component" => key == "data-kind",
        "svg" => [
            "viewbox",
            "width",
            "height",
            "fill",
            "stroke",
            "stroke-width",
            "stroke-linecap",
            "stroke-linejoin",
        ]
        .contains(&key),
        "path" => ["d", "fill", "stroke", "stroke-width", "stroke-dasharray"].contains(&key),
        "circle" => [
            "cx",
            "cy",
            "r",
            "fill",
            "stroke",
            "stroke-width",
            "stroke-dasharray",
        ]
        .contains(&key),
        "ellipse" => [
            "cx",
            "cy",
            "rx",
            "ry",
            "fill",
            "stroke",
            "stroke-width",
            "stroke-dasharray",
        ]
        .contains(&key),
        "rect" => [
            "x",
            "y",
            "width",
            "height",
            "rx",
            "ry",
            "fill",
            "stroke",
            "stroke-width",
            "stroke-dasharray",
        ]
        .contains(&key),
        "line" => [
            "x1",
            "x2",
            "y1",
            "y2",
            "fill",
            "stroke",
            "stroke-width",
            "stroke-dasharray",
        ]
        .contains(&key),
        "polyline" | "polygon" => [
            "points",
            "fill",
            "stroke",
            "stroke-width",
            "stroke-dasharray",
        ]
        .contains(&key),
        _ => false,
    }
}

fn container(tag: &str) -> Option<ContainerKind> {
    Some(match tag {
        "div" => ContainerKind::Div,
        "section" => ContainerKind::Section,
        "figure" => ContainerKind::Figure,
        "figcaption" => ContainerKind::Figcaption,
        "aside" => ContainerKind::Aside,
        "header" => ContainerKind::Header,
        "footer" => ContainerKind::Footer,
        "address" => ContainerKind::Address,
        "center" => ContainerKind::Center,
        _ => return None,
    })
}

fn inline(tag: &str) -> Option<InlineKind> {
    Some(match tag {
        "span" => InlineKind::Span,
        "small" => InlineKind::Small,
        "sub" => InlineKind::Sub,
        "sup" => InlineKind::Sup,
        "time" => InlineKind::Time,
        "mark" => InlineKind::Mark,
        "kbd" => InlineKind::Kbd,
        "samp" => InlineKind::Samp,
        "del" | "s" => InlineKind::Delete,
        "ins" => InlineKind::Insert,
        "u" => InlineKind::Underline,
        "abbr" => InlineKind::Abbreviation,
        _ => return None,
    })
}

/// A real-input census: useful while the document vocabulary migrates, without a test suite.
pub fn census(root: &std::path::Path, out: &mut dyn std::io::Write) -> Result<(), String> {
    let started = std::time::Instant::now();
    let render = crate::render::Render::new(root);
    let paths = crate::render::doc_paths(root);
    let mut counts = BTreeMap::<String, usize>::new();
    let mut raw_containers = BTreeMap::<String, usize>::new();
    let mut containers = BTreeMap::<String, usize>::new();
    let mut unknown = BTreeMap::<String, usize>::new();
    let project = crate::project::Project::of(root);
    for path in &paths {
        let text = crate::text::read_text(&root.join(project.file(path)))
            .map_err(|e| format!("{path}: {e}"))?;
        for part in render.typed_sections(&text, path) {
            census_fragment(
                &part.html,
                &part.media,
                path,
                &mut counts,
                &mut unknown,
                &mut raw_containers,
                &mut containers,
            )?;
        }
        for fragment in render.typed_history(&text, path).values() {
            census_fragment(
                &fragment.html,
                &fragment.media,
                path,
                &mut counts,
                &mut unknown,
                &mut raw_containers,
                &mut containers,
            )?;
        }
    }
    writeln!(out, "documents: {}", paths.len()).map_err(|e| e.to_string())?;
    for (kind, count) in counts {
        writeln!(out, "{kind}: {count}").map_err(|e| e.to_string())?;
    }
    for (label, pairs) in [
        ("raw-container", raw_containers),
        ("remaining-container", containers),
    ] {
        let mut pairs: Vec<_> = pairs.into_iter().collect();
        symphony_board::order::sort_by(&mut pairs, &|a, b| {
            b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))
        });
        for (pair, count) in pairs {
            writeln!(out, "{label}: {count} {pair}").map_err(|e| e.to_string())?;
        }
    }
    writeln!(
        out,
        "Unknown: {} {unknown:?}",
        unknown.values().sum::<usize>()
    )
    .map_err(|e| e.to_string())?;
    writeln!(
        out,
        "elapsed: {:.3}s; no panic",
        started.elapsed().as_secs_f64()
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn census_fragment(
    html: &str,
    media: &[Media],
    path: &str,
    counts: &mut BTreeMap<String, usize>,
    unknown: &mut BTreeMap<String, usize>,
    raw_containers: &mut BTreeMap<String, usize>,
    containers: &mut BTreeMap<String, usize>,
) -> Result<(), String> {
    let raw =
        serde_json::to_value(parse_mode(html, media, path, false)).map_err(|e| e.to_string())?;
    container_pairs(&raw, raw_containers);
    let typed = parse(html, media, path);
    let value = serde_json::to_value(&typed).map_err(|e| e.to_string())?;
    let restored: Vec<Block> = serde_json::from_value(value.clone())
        .map_err(|e| format!("{path}: typed data cannot be read: {e}"))?;
    if restored != typed {
        return Err(format!("{path}: typed data changed during serialization"));
    }
    container_pairs(&value, containers);
    count(&value, "block", counts, unknown);
    Ok(())
}

fn container_pairs(value: &serde_json::Value, counts: &mut BTreeMap<String, usize>) {
    match value {
        serde_json::Value::Object(fields) => {
            if fields.get("type").and_then(serde_json::Value::as_str) == Some("container") {
                let tag = fields
                    .get("tag")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let p = fields.get("presentation").cloned().unwrap_or_default();
                *counts.entry(format!("{tag} {p}")).or_default() += 1;
            }
            for value in fields.values() {
                container_pairs(value, counts);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                container_pairs(value, counts);
            }
        }
        _ => (),
    }
}

pub fn main(args: &[String], out: &mut dyn std::io::Write, err: &mut dyn std::io::Write) -> i32 {
    const USAGE: &str = "usage: symphony blocks [--root ROOT]";
    let parsed = match crate::cli::parse(
        args,
        &[crate::cli::Flag {
            name: "--root",
            value: true,
        }],
    ) {
        Ok(parsed) if parsed.help => {
            let _ = writeln!(
                out,
                "{USAGE}\n\nParse every document and its threads and print the typed-content census: \
                 how many of each\nkind of block they hold (a kind none holds is left out), and \
                 the content no kind names."
            );
            return 0;
        }
        Ok(parsed) if parsed.positionals.is_empty() => parsed,
        Ok(_) => {
            let _ = writeln!(err, "{USAGE}");
            return 2;
        }
        Err(error) => {
            let _ = writeln!(err, "{USAGE}\n{error}");
            return 2;
        }
    };
    match census(
        std::path::Path::new(parsed.value("--root").unwrap_or(".")),
        out,
    ) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(err, "blocks: {error}");
            1
        }
    }
}

fn count(
    value: &serde_json::Value,
    context: &str,
    counts: &mut BTreeMap<String, usize>,
    unknown: &mut BTreeMap<String, usize>,
) {
    match value {
        serde_json::Value::Object(fields) => {
            if let Some(kind) = fields.get("type").and_then(serde_json::Value::as_str) {
                *counts.entry(format!("{context}.{kind}")).or_default() += 1;
                if kind == "unknown" {
                    *unknown
                        .entry(
                            fields
                                .get("tag")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("")
                                .into(),
                        )
                        .or_default() += 1;
                }
            }
            for (key, field) in fields {
                let context = match key.as_str() {
                    "spans" | "summary" => "span",
                    "title" if field.is_array() => "span",
                    "blocks" | "body" | "comparison" | "items" => "block",
                    _ => context,
                };
                count(field, context, counts, unknown);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                count(item, context, counts, unknown);
            }
        }
        _ => (),
    }
}

/// The first block of a person's thread entry, which names them: a paragraph of one span classed
/// [`crate::render::BY_CLASS`] holding the short name their line carries.
pub(crate) fn by_block(name: &str) -> Block {
    Block::Paragraph {
        spans: vec![Span::Styled {
            tag: InlineKind::Span,
            presentation: Presentation::class(crate::render::BY_CLASS),
            spans: vec![Span::Text {
                text: name.to_string(),
            }],
        }],
    }
}

#[cfg(test)]
mod ordering_tests {
    use crate::test_tree::Tree;
    #[test]
    fn census_pairs_keep_complete_count_and_label_order_and_report_cost() {
        for count in [0, 1, 32, 512] {
            let mut pairs: Vec<_> = (0..count)
                .map(|i| (format!("div {{class:猫-{i}}}"), i % 11))
                .collect();
            let mut old = pairs.clone();
            let compare = |a: &(String, usize), b: &(String, usize)| {
                b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))
            };
            #[expect(
                clippy::disallowed_methods,
                reason = "the standard sort this test holds the shared sorter to"
            )]
            let (_, old_cpu) = test_time::cpu::spent(|| old.sort_by(&compare));
            let (_, new_cpu) =
                test_time::cpu::spent(|| symphony_board::order::sort_by(&mut pairs, &compare));
            assert_eq!(pairs, old);
            eprintln!(
                "census pairs n={count}: old_cpu={old_cpu:?},shared_cpu={new_cpu:?},positions={}bytes plus usize-sort scratch",
                count * std::mem::size_of::<usize>()
            );
        }
    }
    #[test]
    fn census_keeps_every_deterministic_output_byte_with_count_and_label_ties() {
        let tree = Tree::new();
        tree.write("docs/CENSUS.md", "# Census\n\n<div class=\"z\"><p>Alpha</p></div>\n\n<section><p>Beta</p></section>\n\n<div class=\"a\"><p>Gamma</p></div>\n\n<div class=\"z\"><p>Delta</p></div>\n\n<aside><p>Epsilon</p></aside>\n\n<section><p>Zeta</p></section>\n");
        let mut out = Vec::new();
        super::census(&tree.root, &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        let (stable, elapsed) = out.rsplit_once("elapsed: ").unwrap();
        // Golden captured with the unchanged native release census before the substitution.
        assert_eq!(
            stable,
            "documents: 1\nblock.container: 6\nblock.heading: 1\nblock.paragraph: 6\nspan.text: 7\nraw-container: 2 div {\"class\":\"z\"}\nraw-container: 2 section {}\nraw-container: 1 aside {}\nraw-container: 1 div {\"class\":\"a\"}\nremaining-container: 2 div {\"class\":\"z\"}\nremaining-container: 2 section {}\nremaining-container: 1 aside {}\nremaining-container: 1 div {\"class\":\"a\"}\nUnknown: 0 {}\n"
        );
        let seconds: f64 = elapsed
            .strip_suffix("s; no panic\n")
            .unwrap()
            .parse()
            .unwrap();
        assert!(seconds.is_finite() && seconds >= 0.0);
        let empty = Tree::new();
        let mut out = Vec::new();
        super::census(&empty.root, &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert_eq!(
            out.rsplit_once("elapsed: ").unwrap().0,
            "documents: 0\nUnknown: 0 {}\n"
        );
    }
}
