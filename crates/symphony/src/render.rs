//! A document as data: text split into blocks the owner comments on, each
//! rendered to an HTML fragment with source lines, a key and a content hash.
//! comrak renders Markdown. links interprets cards (- [ ] **slug**), callouts
//! (> [!comment]), document/card links and [[name:arg]] components.
//! HTML mockups and files a plugin's renderer draws in a viewer are Media items
//! indexed by placeholders in the fragment, for the client to draw.

mod cards;
mod links;
mod markdown;
mod media;

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use regex::Regex;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::edit::{frontmatter_end, heading_anchor};
use crate::page::Document;
use crate::register::{
    CALLOUT, Group, KINDS, QUOTE, REGION_START, RESOLVED, card_end, card_line, is_fence,
    parse_decisions,
};
use crate::text::splitlines;

pub(crate) use cards::compare_pictures;
pub(crate) use links::{doc_paths, find_doc};
pub use media::{MOCKUP_NATIVE, MOCKUP_SANDBOX, is_mockup};

/// What the renderer cannot draw as markup: a picture that is an `.html` file, a file in a plugin's
/// viewer, a component's gallery. The fragment holds `<span class="media" data-media="N" data-kind="…">` where
/// it goes; `N` indexes the fragment's `media`.
pub use symphony_api::Media;

/// A piece of rendered markup and what it points at.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Fragment {
    pub html: String,
    pub media: Vec<Media>,
}

/// The fragment as the wire's, with no blocks drawn of it yet.
impl From<Fragment> for symphony_api::Fragment {
    fn from(fragment: Fragment) -> Self {
        symphony_api::Fragment {
            blocks: Vec::new(),
            html: fragment.html,
            media: fragment.media,
        }
    }
}

/// One block of a document, rendered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Section {
    /// 1-based source lines, inclusive.
    pub start: usize,
    pub end: usize,
    /// `section:<heading anchor>:<n>`: the heading it sits under and its place there, so text added
    /// under one heading leaves every other section's key as it was.
    pub key: String,
    /// The first 16 hex digits of the SHA-256 of `html` and `media`: a live update sends the sections
    /// whose hash moved.
    pub hash: String,
    /// The card of the page it belongs to (see [`crate::page`]); `None` for a title that heads the page.
    pub part: Option<usize>,
    pub html: String,
    pub media: Vec<Media>,
}

/// `text`, Markdown, as plain HTML: converted as a document is, its headings given the ids a
/// document's anchors name, with no card, callout, link or component of a document's.
pub fn plain_html(text: &str) -> String {
    markdown::convert(text)
}

/// A block of text between blank lines (a card with its whole thread, a fence whole).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// Blank-line-separated blocks with their source lines; fences stay whole, and a card keeps its thread.
pub fn split_blocks(text: &str) -> Vec<Block> {
    let lines = splitlines(text);
    let mut blocks = Vec::new();
    let mut start: Option<usize> = None;
    let mut in_fence = false;
    let mut index = frontmatter_end(&lines).map_or(0, |end| end + 1);
    while index < lines.len() {
        let line = lines[index];
        if start.is_none() && !in_fence && card_line(line).is_some() {
            let end = card_end(&lines, index);
            blocks.push(Block {
                start: index + 1,
                end,
                text: lines[index..end].join("\n"),
            });
            index = end;
            continue;
        }
        if is_fence(line) {
            in_fence = !in_fence;
        }
        if line.trim().is_empty() && !in_fence {
            if let Some(from) = start.take() {
                blocks.push(Block {
                    start: from + 1,
                    end: index,
                    text: lines[from..index].join("\n"),
                });
            }
        } else if start.is_none() {
            start = Some(index);
        }
        index += 1;
    }
    if let Some(from) = start {
        blocks.push(Block {
            start: from + 1,
            end: lines.len(),
            text: lines[from..].join("\n"),
        });
    }
    blocks
}

/// Which document holds each card, with the path and text of every document it was read from.
pub(crate) type CardCache = Mutex<Option<(Vec<(String, String)>, Arc<HashMap<String, String>>)>>;

/// What one render reads: the project, the page, and data loaded at most once.
pub(crate) struct Ctx<'a> {
    pub(crate) root: &'a std::path::Path,
    pub(crate) doc_path: &'a str,
    plugins: &'a crate::plugins::Plugins,
    page: symphony_plugin::Page<'a>,
    paths: OnceCell<Vec<String>>,
    cards: OnceCell<Arc<HashMap<String, String>>>,
    shared: &'a CardCache,
    media: RefCell<Vec<Media>>,
    detached_threads: bool,
    typed: bool,
}

impl Ctx<'_> {
    pub(crate) fn paths(&self) -> &[String] {
        self.paths.get_or_init(|| links::doc_paths(self.root))
    }

    fn cards(&self) -> &HashMap<String, String> {
        self.cards
            .get_or_init(|| links::card_docs(self.root, self.paths(), self.shared))
            .as_ref()
    }

    pub(crate) fn add_media(&self, item: Media) -> usize {
        let mut media = self.media.borrow_mut();
        media.push(item);
        media.len() - 1
    }

    fn take_media(&self) -> Vec<Media> {
        self.media.take()
    }

    /// The component `name` as the fragment holds it: its blocks written as HTML, in a typed render
    /// inside the `<symphony-component>` the parser reads back as [`symphony_api::Block::Component`].
    fn component_html(&self, name: &str, argument: &str, option: Option<&str>) -> String {
        let blocks = self.plugins.render(&self.page, name, argument, option);
        media::blocks_html(self, name, blocks)
    }

    /// `text` as the converter is handed it, components held out as tokens and document and card
    /// links written out, with the HTML each token stands for. With `hold_links`, a typed page's
    /// links are held out as tokens too, for a converter that escapes raw HTML (a note's).
    fn prepared(&self, text: &str, hold_links: bool) -> (String, Vec<String>) {
        let (expanded, mut stash) = links::expand_for(text, &mut |name, argument, option| {
            self.component_html(name, argument, option)
        });
        let mut held = hold_links.then_some(&mut stash);
        let wiki = links::wikilinks_for(
            &expanded,
            self.doc_path,
            self.paths(),
            self.typed,
            held.as_deref_mut(),
        );
        let linked = links::card_links(&wiki, self.cards(), self.typed, held);
        (linked, stash)
    }

    /// Markdown to HTML with its components and document links: the one path prose, cards and replies share.
    pub(crate) fn convert(&self, text: &str) -> String {
        let (linked, stash) = self.prepared(text, false);
        let body = markdown::convert(&linked);
        let restored = links::restore(body.trim_end_matches('\n'), &stash);
        media::embed(&restored, self)
    }

    /// A note under a card, the owner's or an agent's, through the same path as a document's body,
    /// with its line breaks kept, no heading and raw HTML shown as typed ([`markdown::convert_note`]).
    pub(crate) fn convert_note(&self, text: &str) -> String {
        let (linked, stash) = self.prepared(text, true);
        let body = markdown::convert_note(&linked);
        let restored = links::restore(body.trim_end_matches('\n'), &stash);
        media::embed(&restored, self)
    }

    /// An agent's post outside a document through the same path as a document's body, its
    /// components and links drawn, with raw HTML shown as typed ([`markdown::convert_post`]).
    pub(crate) fn convert_post(&self, text: &str) -> String {
        let (linked, stash) = self.prepared(text, true);
        let body = markdown::convert_post(&linked);
        let restored = links::restore(body.trim_end_matches('\n'), &stash);
        media::embed(&restored, self)
    }

    /// [`Ctx::convert_post`] of a part of `text`: the blocks from the one holding byte `from`, as
    /// many as `limit` bytes of `text` hold, each longer than `block` cut, from one parse of all of
    /// it ([`markdown::convert_part`]). The text the converter is handed has the lines of `text`, so
    /// a part begins at a line of both and is measured in `text`; a `[[…]]` written over lines is
    /// the exception, which moves where the later parts begin in `text`, never what they hold.
    fn convert_part(&self, text: &str, from: usize, limit: usize, block: usize) -> markdown::Part {
        let (linked, stash) = self.prepared(text, true);
        let starts = markdown::line_starts(text);
        let at = |line: usize| starts.get(line).copied().unwrap_or(text.len());
        let mut part = markdown::convert_part(&linked, &at, text.len(), from, limit, block);
        let restored = links::restore(part.html.trim_end_matches('\n'), &stash);
        part.html = media::embed(&restored, self);
        part
    }
}

static HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<hashes>#{1,6})\s+(?P<title>.+?)\s*#*\s*$").expect("the heading pattern")
});

/// A block's key: the heading it sits under and its place there. A heading that repeats (each register
/// group has its `### Cards`) is told apart by a count.
#[derive(Default)]
struct SectionKeys {
    anchor: String,
    place: usize,
    seen: HashMap<String, usize>,
}

impl SectionKeys {
    fn next(&mut self, first_line: &str) -> String {
        if let Some(heading) = HEADING.captures(first_line) {
            let anchor = heading_anchor(&heading["title"]);
            let count = self.seen.entry(anchor.clone()).or_insert(0);
            *count += 1;
            self.anchor = if *count == 1 {
                anchor
            } else {
                format!("{anchor}-{count}")
            };
            self.place = 0;
        } else {
            self.place += 1;
        }
        format!("section:{}:{}", self.anchor, self.place)
    }
}

fn hash(html: &str, media: &[Media]) -> String {
    let mut digest = Sha256::new();
    digest.update(html.as_bytes());
    digest.update(serde_json::to_vec(media).unwrap_or_default());
    digest
        .finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Renders documents of the project at `root`.
pub struct Render {
    root: PathBuf,
    /// The project's artifacts folder, relative to its root: what a component's page names.
    artifacts: String,
    plugins: crate::plugins::Plugins,
    /// Each document's last rendering and the version of its file it was made from.
    cache: Mutex<HashMap<String, (String, Arc<Document>)>>,
    /// Which document holds each card, as of the files it was read from: every render needs it, and
    /// reading every register for each would cost more than the render.
    card_cache: CardCache,
}

impl Render {
    pub(crate) fn component_version(&self) -> Option<String> {
        self.plugins.cache_version(
            &symphony_plugin::Page::new(&self.root, "").in_artifacts(&self.artifacts),
        )
    }

    pub fn new(root: impl Into<PathBuf>) -> Render {
        let root = root.into();
        Render {
            plugins: crate::plugins::Plugins::of(&root),
            artifacts: crate::project::Project::of(&root).artifacts.clone(),
            root,
            cache: Mutex::new(HashMap::new()),
            card_cache: Mutex::new(None),
        }
    }

    /// The same renderer with `plugins` filling `[[name:arg]]`.
    #[must_use]
    pub fn with_plugins(mut self, plugins: crate::plugins::Plugins) -> Render {
        self.plugins = plugins;
        self.cache
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        self
    }

    /// The plugins of the project it renders.
    pub fn plugins(&self) -> &crate::plugins::Plugins {
        &self.plugins
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// The component `name` on the page `doc_path`, as untyped fragment HTML.
    pub fn component(
        &self,
        doc_path: &str,
        name: &str,
        argument: &str,
        option: Option<&str>,
    ) -> String {
        self.ctx(doc_path).component_html(name, argument, option)
    }

    /// `text` with its components held out as tokens, and the untyped HTML each token stands for.
    pub fn expand_components(&self, text: &str, doc_path: &str) -> (String, Vec<String>) {
        let ctx = self.ctx(doc_path);
        links::expand_for(text, &mut |name, argument, option| {
            ctx.component_html(name, argument, option)
        })
    }

    fn ctx<'a>(&'a self, doc_path: &'a str) -> Ctx<'a> {
        Ctx {
            root: &self.root,
            doc_path,
            plugins: &self.plugins,
            page: symphony_plugin::Page::new(&self.root, doc_path).in_artifacts(&self.artifacts),
            paths: OnceCell::new(),
            cards: OnceCell::new(),
            shared: &self.card_cache,
            media: RefCell::new(Vec::new()),
            detached_threads: false,
            typed: false,
        }
    }

    /// The document as sections, one per block. Components read their data under the project root, each
    /// file at most once per call.
    pub fn sections(&self, text: &str, doc_path: &str) -> Vec<Section> {
        self.section_fragments(text, doc_path, false, false)
    }

    /// The native client draws card threads separately and derives source lines from section.start
    /// plus data-line-offset. Moving a block therefore leaves its content hash unchanged.
    pub fn client_sections(&self, text: &str, doc_path: &str) -> Vec<Section> {
        self.section_fragments(text, doc_path, true, false)
    }

    /// The same pipeline with semantic component/wiki markers retained for `blocks::parse`.
    /// These fragments are internal parser input, never a wire representation.
    pub fn typed_sections(&self, text: &str, doc_path: &str) -> Vec<Section> {
        self.section_fragments(text, doc_path, true, true)
    }

    fn section_fragments(
        &self,
        text: &str,
        doc_path: &str,
        detached: bool,
        typed: bool,
    ) -> Vec<Section> {
        let mut ctx = self.ctx(doc_path);
        ctx.detached_threads = detached;
        ctx.typed = typed;
        let mut keys = SectionKeys::default();
        let counts = if text.contains(REGION_START) {
            Some(group_counts(text, &self.plugins))
        } else {
            None
        };
        let mut sections = Vec::new();
        for block in split_blocks(text) {
            let lines = splitlines(&block.text);
            let first = lines.first().copied().unwrap_or("");
            let callout = CALLOUT.captures(first).filter(|c| {
                let kind = c["kind"].to_lowercase();
                KINDS.contains(&kind.as_str()) || kind == RESOLVED
            });
            let mut html = if callout.is_some() {
                let quoted: Vec<String> = lines
                    .iter()
                    .map(|line| match QUOTE.captures(line) {
                        Some(quote) if !quote["rest"].is_empty() => quote["rest"].to_string(),
                        _ => (*line).to_string(),
                    })
                    .collect();
                cards::callout(&quoted, &ctx)
            } else if card_line(first).is_some() {
                cards::decisions(&block, &ctx)
            } else {
                ctx.convert(&block.text)
            };
            if let Some(counts) = &counts {
                html = count_groups(&html, counts);
            }
            let media = ctx.take_media();
            sections.push(Section {
                start: block.start,
                end: block.end,
                key: keys.next(first),
                hash: hash(&html, &media),
                part: None,
                html,
                media,
            });
        }
        sections
    }

    /// The document body as HTML, one `<section data-start data-end>` per block.
    pub fn html(&self, text: &str, doc_path: &str) -> String {
        self.sections(text, doc_path)
            .iter()
            .map(|s| {
                format!(
                    "<section data-start=\"{}\" data-end=\"{}\">\n{}\n</section>",
                    s.start, s.end, s.html
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Each card's rendered thread by slug, so the page can refresh one without reloading.
    pub fn history(&self, text: &str, doc_path: &str) -> BTreeMap<String, Fragment> {
        self.history_for(text, doc_path, false)
    }

    pub fn typed_history(&self, text: &str, doc_path: &str) -> BTreeMap<String, Fragment> {
        self.history_for(text, doc_path, true)
    }

    fn history_for(&self, text: &str, doc_path: &str, typed: bool) -> BTreeMap<String, Fragment> {
        let mut ctx = self.ctx(doc_path);
        ctx.typed = typed;
        let mut history = BTreeMap::new();
        for card in parse_decisions(&splitlines(text)) {
            let html = cards::history_items(&card, &ctx);
            history.insert(
                card.slug.clone(),
                Fragment {
                    html,
                    media: ctx.take_media(),
                },
            );
        }
        history
    }

    /// An agent's Markdown outside a document (an ask's answer, a task's post or brief): its
    /// Markdown, components and links drawn as a document's, its raw HTML the words typed
    /// (`Vec<String>`, `<TokensPart key=x>`), as in a card's agent line.
    pub fn reply(&self, text: &str, doc_path: &str) -> Fragment {
        self.reply_for(text, doc_path, false)
    }

    pub fn typed_reply(&self, text: &str, doc_path: &str) -> Fragment {
        self.reply_for(text, doc_path, true)
    }

    /// A part of `text` rendered as [`Render::typed_reply`] renders the whole: the blocks from the
    /// one holding byte `from`, as many as `limit` bytes hold (one at least), and the byte the next
    /// part begins at, the text's length when this part reaches its end. One parse of the whole
    /// text decides every part, so a part means what it means in the whole: a reference defined in
    /// another part keeps its target, a fence of any kind holds its lines, a list keeps its
    /// numbering and a heading its id. A block longer than `block` bytes is cut between its items,
    /// blocks or lines, never inside a line, and at the same places whatever `from` and `limit`
    /// are: the parts of a text read with one `block` meet with no gap and no repeat.
    pub fn typed_reply_part(
        &self,
        text: &str,
        doc_path: &str,
        from: usize,
        limit: usize,
        block: usize,
    ) -> (Fragment, usize) {
        let mut ctx = self.ctx(doc_path);
        ctx.typed = true;
        let part = ctx.convert_part(text, from, limit, block);
        let html = if part.whole {
            cards::single_paragraph(part.html)
        } else {
            part.html
        };
        let fragment = Fragment {
            html,
            media: ctx.take_media(),
        };
        (fragment, part.next)
    }

    fn reply_for(&self, text: &str, doc_path: &str, typed: bool) -> Fragment {
        let mut ctx = self.ctx(doc_path);
        ctx.typed = typed;
        let html = cards::single_paragraph(ctx.convert_post(text));
        Fragment {
            html,
            media: ctx.take_media(),
        }
    }

    /// A document's own text on its own (a section an excerpt shows), rendered as the document
    /// renders it, its raw HTML and components its own, a lone paragraph unwrapped as a reply's.
    pub(crate) fn excerpt(&self, text: &str, doc_path: &str, typed: bool) -> Fragment {
        let mut ctx = self.ctx(doc_path);
        ctx.typed = typed;
        let html = cards::single_paragraph(ctx.convert(text));
        Fragment {
            html,
            media: ctx.take_media(),
        }
    }

    /// The owner's words outside a card (an ask's question or follow-up, a task message): literal
    /// text, with each line naming an upload drawn as a card's owner line draws it.
    pub fn owner(&self, text: &str, doc_path: &str) -> Fragment {
        self.owner_for(text, doc_path, false)
    }

    pub fn typed_owner(&self, text: &str, doc_path: &str) -> Fragment {
        self.owner_for(text, doc_path, true)
    }

    fn owner_for(&self, text: &str, doc_path: &str, typed: bool) -> Fragment {
        let mut ctx = self.ctx(doc_path);
        ctx.typed = typed;
        let html = cards::owner_html(text, &ctx);
        Fragment {
            html,
            media: ctx.take_media(),
        }
    }
}

/// Each `[[name:argument|option]]` of `text` outside code, in its order.
pub fn references(text: &str) -> Vec<(String, String, Option<String>)> {
    let mut found = Vec::new();
    links::expand_for(text, &mut |name, argument, option| {
        found.push((name.into(), argument.into(), option.map(str::to_string)));
        String::new()
    });
    found
}

/// How many cards belong to each register group, by where the cards belong, not where they sit.
fn group_counts(text: &str, plugins: &crate::plugins::Plugins) -> Vec<(Group, usize)> {
    let cards = parse_decisions(&splitlines(text));
    Group::ALL
        .iter()
        .map(|group| {
            (
                *group,
                cards
                    .iter()
                    .filter(|card| card.group(plugins) == *group)
                    .count(),
            )
        })
        .collect()
}

/// Put a live count on each register group heading.
fn count_groups(html: &str, counts: &[(Group, usize)]) -> String {
    let mut page = html.to_string();
    for (group, count) in counts {
        let title = group.title();
        let pattern = Regex::new(&format!(r">{}(?: \(\d+\))?</h2>", regex::escape(title)))
            .expect("a group heading pattern");
        page = pattern
            .replace_all(&page, format!(">{title} ({count})</h2>").as_str())
            .into_owned();
    }
    page
}

impl Render {
    /// The rendering of `rel` made from `version` of its file, if that is the last one.
    pub(crate) fn cached(&self, rel: &str, version: &str) -> Option<Arc<Document>> {
        let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache
            .get(rel)
            .filter(|(had, _)| had == version)
            .map(|(_, doc)| Arc::clone(doc))
    }

    pub(crate) fn remember(&self, rel: &str, version: String, doc: &Arc<Document>) {
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache.insert(rel.to_string(), (version, Arc::clone(doc)));
    }
}

impl Render {
    /// The text with its `[[NAME#Heading]]` links and card slugs written out as markdown links, before
    /// any conversion: what the converter is handed.
    pub fn linked(&self, text: &str, doc_path: &str) -> String {
        let ctx = self.ctx(doc_path);
        links::card_links(
            &links::wikilinks(text, doc_path, ctx.paths()),
            ctx.cards(),
            false,
            None,
        )
    }

    /// `text` through the markdown converter alone: no cards, callouts or components, only what
    /// [`Render::linked`] hands it. Headings carry their ids.
    pub fn markdown(&self, text: &str, doc_path: &str) -> String {
        markdown::convert(&self.linked(text, doc_path))
    }
}
