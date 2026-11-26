//! Native document read models. Threads and source positions are independent of section markup so
//! a reply or a moved block never retransmits the rest of a long register.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use symphony_api::{
    Document, DocumentChange, DocumentTask, Folder, Fragment, Media, SectionPosition,
};

use crate::render::Render;

mod comparison;
#[cfg(test)]
mod first_tests;
mod legacy;
#[cfg(test)]
mod linked_tests;
#[cfg(test)]
mod outline_tests;
#[cfg(test)]
mod outside_tests;
mod regroup;

pub use legacy::legacy_html;

/// Resolve only routes an entrypoint serves (`docs/…`) to the file the entrypoint's walk would
/// list there, links resolved ([`crate::project::Project::serving`],
/// [`crate::project::Project::reachable`]), whatever its folder is called; a `document` is a Markdown
/// file the entrypoint shows.
pub fn target(root: &Path, rel: &str, document: bool) -> Result<PathBuf, String> {
    let not_found = || {
        if rel.is_empty() {
            "not found: the path is empty".to_string()
        } else {
            format!("not found: {rel}")
        }
    };
    if Path::new(rel)
        .components()
        .any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err(not_found());
    }
    let project = crate::project::Project::of(root);
    let (entry, inside) = project.serving(rel).ok_or_else(not_found)?;
    // In a checkout with none of the entrypoints' folders, a page says how to add documents.
    let path = project
        .reachable(root, entry, inside)
        .ok_or_else(|| project.no_documents(root).unwrap_or_else(not_found))?;
    if document && (!path.is_file() || !entry.page(inside)) {
        return Err(not_found());
    }
    Ok(path)
}

fn wire<T: Serialize, U: DeserializeOwned>(value: T) -> U {
    serde_json::from_value(serde_json::to_value(value).expect("review data serializes"))
        .expect("native and wire review shapes agree")
}

/// Where the route `rel` of the checkout at `root` may have moved, when no file has it: the
/// documents a wikilink to its file name from its own folder finds ([`crate::render::find_docs`]),
/// so a link filed before its document moved folders still finds it. One is the document it names;
/// more is a name documents share, which a page offers as a choice rather than open the first. None
/// for a route a file has, one with no file name, or a name no document has.
pub fn moved(root: &Path, rel: &str) -> Vec<String> {
    let project = crate::project::Project::of(root);
    moved_among(root, &project, rel, || crate::render::doc_paths(root))
}

/// [`moved`] in `project`, among its document routes, which `paths` walks
/// ([`crate::render::doc_paths`]) only for a route with a file name that no file has.
fn moved_among<P: AsRef<[String]>>(
    root: &Path,
    project: &crate::project::Project,
    rel: &str,
    paths: impl FnOnce() -> P,
) -> Vec<String> {
    let Some(name) = Path::new(rel).file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };
    if root.join(project.file(rel)).is_file() {
        return Vec::new();
    }
    let paths = paths();
    crate::render::find_docs(name, rel, paths.as_ref())
        .cloned()
        .collect()
}

/// What the task links of the checkout at `root` name: a document by its route, or by a route no
/// file has any longer that [`moved`] finds that document alone for, as its page opens it. A name
/// documents share names none of them. The project is read once, and the documents walked at most
/// once, for the first link only its name can settle: only a link whose file name is the
/// document's is looked for on disk. The document page's tasks ([`linked`]) and the cards a task
/// may have posted ([`crate::gates::linked_cards`]) read their links through one.
pub struct LinkNames<'a> {
    root: &'a Path,
    project: std::sync::Arc<crate::project::Project>,
    paths: std::cell::OnceCell<Vec<String>>,
}

impl<'a> LinkNames<'a> {
    /// The links of the checkout at `root`, nothing read yet but its project.
    pub fn of(root: &'a Path) -> Self {
        Self {
            root,
            project: crate::project::Project::of(root),
            paths: std::cell::OnceCell::new(),
        }
    }

    /// Whether the task link `link` (a route, a `#heading` after it or not) names the document
    /// `rel`.
    pub fn names(&self, link: &str, rel: &str) -> bool {
        let path = link.split('#').next().unwrap_or_default();
        if path == rel {
            return true;
        }
        let linked = Path::new(path).file_name().and_then(|name| name.to_str());
        let (Some(linked), Some(own)) = (linked, Path::new(rel).file_name()) else {
            return false;
        };
        own == crate::render::doc_name(linked).as_str()
            && moved_among(self.root, &self.project, path, || {
                self.paths
                    .get_or_init(|| crate::render::doc_paths(self.root))
            }) == [rel]
    }
}

/// The tasks of `board` that link the document `rel` of the checkout at `root`, as [`LinkNames`]
/// reads their links. Reads no board: `board` is the caller's copy of it.
pub fn linked(root: &Path, rel: &str, board: &[symphony_api::Task]) -> Vec<DocumentTask> {
    let links = LinkNames::of(root);
    board
        .iter()
        .filter(|task| task.links.iter().any(|link| links.names(link, rel)))
        .map(|task| DocumentTask {
            slug: task.slug.clone(),
            title: task.title.clone(),
            status: task.status.clone(),
        })
        .collect()
}

/// The document `rel` as a page shows it, its tasks the ones of `board` that link it ([`linked`]).
pub fn document(
    render: &Render,
    rel: &str,
    scope: &str,
    board: &[symphony_api::Task],
) -> Result<Document, String> {
    document_showing(render, rel, scope, board, &[])
}

/// [`document`], each card of `cards` shown as that one card, its copies' lines together
/// ([`crate::branches::one_card`]), rather than as the file holds it: its section, thread and state.
/// Each section keeps the file's own lines as its place, so a save from the page lands where it did.
pub fn document_showing(
    render: &Render,
    rel: &str,
    scope: &str,
    board: &[symphony_api::Task],
    cards: &[crate::Decision],
) -> Result<Document, String> {
    document_marked(render, rel, scope, board, cards, &BTreeSet::new())
}

/// What a draft says beside its slug on a branch's page: the agent has not committed it, so it is
/// in no queue or count until the owner speaks on it ([`crate::branches::PageMarks::drafts`]).
pub const DRAFT: &str = "draft, not posted";

/// The card `slug` in a section's `html` marked as a draft: [`DRAFT`] where its posted time goes
/// (which the page draws beside its slug), before that time when it has one.
fn mark_draft(html: &mut String, slug: &str) {
    let slug = crate::markup::escape(slug);
    let Some(card) = html.find(&format!("data-slug=\"{slug}\"")) else {
        return;
    };
    let code = format!("<code>{slug}</code> ");
    let Some(after) = html[card..].find(&code).map(|at| card + at + code.len()) else {
        return;
    };
    const POSTED: &str = "<time class=\"posted\">";
    if html[after..].starts_with(POSTED) {
        html.insert_str(after + POSTED.len(), &format!("{DRAFT} · "));
    } else {
        html.insert_str(after, &format!("{POSTED}{DRAFT}</time> "));
    }
}

/// What a branch's page says above its document while a merge stopped on its file
/// ([`crate::merging`]): the page shows the branch's last committed copy instead of the markers.
pub const MERGING: &str = "> **Merging.** This worktree is in the middle of a merge, and its copy of this document holds conflict markers. This is the branch's last committed copy. An answer given on a card here is saved on the branch's side of the merge.";

/// The file `rel` of the checkout at `root`, and the text its page shows: on a branch's page
/// (`scope`) while a merge stopped on the file, its last committed copy, and `true`
/// ([`crate::merging::committed`]).
pub fn source(root: &Path, rel: &str, scope: &str) -> Result<(PathBuf, String, bool), String> {
    let path = target(root, rel, true)?;
    let text = crate::text::read_text(&path).map_err(|e| e.to_string())?;
    let file = crate::project::Project::of(root).file(rel);
    if scope.is_empty() {
        // The owner's saves a project that delivers by pull request still keeps are shown.
        let text = crate::commit::pending::shown(root, &file, text);
        return Ok((path, text, false));
    }
    Ok(match crate::merging::committed(root, &file, &text) {
        Some(committed) => (path, committed, true),
        None => (path, text, false),
    })
}

/// A section's markup made ready for the page: its links scoped, its blocks parsed and its hash
/// taken over both.
fn finish(section: &mut symphony_api::Section, root: &Path, rel: &str, scope: &str) {
    prepare_fragment(&mut section.html, &mut section.media, root, rel, scope);
    section.blocks = crate::blocks::parse(&section.html, &section.media, &scoped_path(rel, scope));
    let mut digest = Sha256::new();
    digest.update(section.html.as_bytes());
    digest.update(serde_json::to_vec(&section.media).expect("media serializes"));
    digest.update(serde_json::to_vec(&section.blocks).expect("blocks serialize"));
    section.hash = digest
        .finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect();
}

/// [`document_showing`], each card of `drafts` marked as a draft ([`DRAFT`]) where its posted time
/// goes. A branch's page whose file a merge stopped on shows its last committed copy under a note
/// that says so ([`MERGING`]), never the conflict markers.
pub fn document_marked(
    render: &Render,
    rel: &str,
    scope: &str,
    board: &[symphony_api::Task],
    cards: &[crate::Decision],
    drafts: &BTreeSet<String>,
) -> Result<Document, String> {
    let (path, text, merging) = source(render.root(), rel, scope)?;
    let (text, _) = crate::register::supersede(&text, &crate::clock::now_stamp());
    // A card shown in another group than its file copy's is drawn in its group: the page draws the
    // register regrouped, each line standing for the file's it was. Its waiting cards stay the
    // file's.
    let regrouped = if cards.is_empty() {
        None
    } else {
        let parsed = crate::register::parse_decisions(&crate::text::splitlines(&text));
        regroup::drawn(&text, &parsed, cards, render.plugins())
    };
    let waiting = crate::tree::pending(&text);
    let (text, places) = match regrouped {
        Some((drawn, places)) => (drawn, Some(places)),
        None => (text, None),
    };
    let mut sections = render.typed_sections(&text, rel);
    for section in &mut sections {
        if let Some(slug) = section
            .html
            .split("data-slug=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
        {
            section.key = format!("card:{slug}");
        }
    }
    let mut threads = render.typed_history(&text, rel);
    let lines = crate::text::splitlines(&text);
    let parsed = crate::register::parse_decisions(&lines);
    let mut shown: BTreeMap<&str, &crate::Decision> = BTreeMap::new();
    if let Some(places) = &places {
        // Each card is written as its one card already; each section's place is the file's.
        for card in &parsed {
            if let Some(one) = cards.iter().find(|one| one.slug == card.slug) {
                shown.insert(one.slug.as_str(), one);
            }
        }
        for section in &mut sections {
            (section.start, section.end) = regroup::place(places, section.start, section.end);
        }
    } else if !cards.is_empty() {
        for section in &mut sections {
            // The file's cards in this section that a one card replaces, in their order.
            let inside: Vec<(&crate::Decision, &crate::Decision)> = parsed
                .iter()
                .filter(|card| (section.start..=section.end).contains(&card.line))
                .filter_map(|card| {
                    cards
                        .iter()
                        .find(|one| one.slug == card.slug)
                        .map(|one| (card, one))
                })
                .collect();
            if inside.is_empty() {
                continue;
            }
            // The section's own lines, each such card written as its one card.
            let mut block = String::new();
            let mut at = section.start;
            for (card, one) in &inside {
                for line in lines.iter().take(card.line - 1).skip(at - 1) {
                    block.push_str(line);
                    block.push('\n');
                }
                block.push_str(&crate::branches::markdown(one));
                at = card.line + 1 + card.raw.len();
            }
            for line in lines.iter().take(section.end).skip(at - 1) {
                block.push_str(line);
                block.push('\n');
            }
            let mut fresh = render.typed_sections(&block, rel);
            if fresh.len() != 1 {
                continue; // drawn otherwise than the file's block: the file's stands
            }
            let fresh = fresh.remove(0);
            section.html = fresh.html;
            section.media = fresh.media;
            section.hash = fresh.hash;
            let mut drawn = render.typed_history(&block, rel);
            for (_, one) in inside {
                if let Some(thread) = drawn.remove(&one.slug) {
                    threads.insert(one.slug.clone(), thread);
                }
                shown.insert(one.slug.as_str(), one);
            }
        }
    }
    for slug in drafts {
        for section in &mut sections {
            mark_draft(&mut section.html, slug);
        }
    }
    crate::page::assign_parts(&mut sections);
    let cards = parsed
        .into_iter()
        .map(|card| {
            shown
                .get(card.slug.as_str())
                .map_or(card, |one| (*one).clone())
        })
        .map(|card| {
            let thread = card.thread(render.plugins());
            symphony_api::DocumentCard {
                slug: card.slug.clone(),
                state: card.state.as_str().into(),
                text: card.text.clone(),
                group: thread.group.key().into(),
                open: thread.open,
            }
        })
        .collect();
    let mut doc = Document {
        version: 0,
        path: rel.into(),
        title: crate::tree::first_heading(&text)
            .map(|title| crate::tree::plain(&title))
            .unwrap_or_else(|| {
                path.file_stem()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
            }),
        waiting,
        crumbs: wire(crate::tree::crumbs(render.root(), rel)),
        headings: wire(crate::tree::headings(&text)),
        sections: wire(sections),
        cards,
        threads: wire(threads),
        tasks: linked(render.root(), rel, board),
        comparison: None,
    };
    for section in &mut doc.sections {
        finish(section, render.root(), rel, scope);
    }
    for fragment in doc.threads.values_mut() {
        prepare_fragment(
            &mut fragment.html,
            &mut fragment.media,
            render.root(),
            rel,
            scope,
        );
        fragment.blocks =
            crate::blocks::parse(&fragment.html, &fragment.media, &scoped_path(rel, scope));
    }
    comparison::apply(render, rel, scope, &text, &mut doc);
    // A notice's sections are put before the document's, keyed `key`, at no place in its text.
    let mut notice = |text: &str, key: &str| {
        let mut notice: Vec<symphony_api::Section> = wire(render.typed_sections(text, rel));
        for section in &mut notice {
            section.key = key.into();
            (section.start, section.end, section.part) = (0, 0, None);
            finish(section, render.root(), rel, scope);
        }
        doc.sections.splice(0..0, notice);
    };
    if merging {
        // After the comparison, which reads the sections as the text's blocks in order.
        notice(MERGING, "merging");
    }
    if scope.is_empty()
        && let Some(note) = crate::commit::pending::note(
            render.root(),
            &crate::project::Project::of(render.root()).file(rel),
        )
    {
        notice(&note, crate::commit::pending::NOTE);
    }
    for section in &mut doc.sections {
        section.ends = symphony_api::Section::ends_of(&section.blocks);
    }
    if !scope.is_empty() {
        doc.path = format!("@{scope}/{rel}");
        for crumb in &mut doc.crumbs {
            if let Some(href) = &mut crumb.href {
                *href = scoped(href, scope);
            }
        }
    }
    Ok(doc)
}

pub fn anchors(html: &str) -> Vec<String> {
    html.split(" id=\"")
        .skip(1)
        .filter_map(|rest| rest.split_once('"').map(|(id, _)| id.to_string()))
        .collect()
}

/// The most characters of a card's text the outline carries, its ellipsis among them.
pub const OUTLINE_CARD_CHARS: usize = 140;

/// A card's line as the outline and the feed carry it for a card whose body is not read (a folded
/// card's summary): its plain words, cut at a word's end with an ellipsis, never inside one.
pub fn card_line(text: &str) -> String {
    crate::dashboard::cut(&crate::tree::plain(text), OUTLINE_CARD_CHARS)
}

pub fn outline(mut document: Document) -> symphony_api::DocumentOutline {
    let mut section_anchors = BTreeMap::new();
    for section in &document.sections {
        section_anchors.insert(
            section.key.clone(),
            if section.start == 0 {
                Vec::new()
            } else {
                anchors(&section.html)
            },
        );
    }
    for section in &mut document.sections {
        section.html.clear();
        section.media.clear();
        section.blocks.clear();
        if let Some(comparison) = &mut section.comparison {
            comparison.before.clear();
        }
    }
    for card in &mut document.cards {
        card.text = card_line(&card.text);
    }
    let mut thread_hashes = BTreeMap::new();
    for (slug, thread) in std::mem::take(&mut document.threads) {
        thread_hashes.insert(slug, thread_hash(&thread));
    }
    symphony_api::DocumentOutline {
        document,
        anchors: section_anchors,
        thread_hashes,
    }
}

/// What a page reads ahead of `document` (`/api/document?view=first`): its outline's head and its
/// leading sections whole, within [`symphony_api::AHEAD_BYTES`] of JSON
/// ([`symphony_api::DocumentOutline::first`]); `None` when its head alone is over them.
pub fn first(document: Document) -> Option<symphony_api::DocumentOutline> {
    outline(document.clone()).first(&document, |part| {
        serde_json::to_vec(part).is_ok_and(|json| json.len() <= symphony_api::AHEAD_BYTES)
    })
}

/// A card thread's hash, as the outline names it: the first 16 hex digits of the SHA-256 of the
/// thread as the client reads it.
fn thread_hash(thread: &Fragment) -> String {
    Sha256::digest(serde_json::to_vec(thread).expect("a thread serializes"))
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn selected(
    document: &Document,
    keys: &[String],
    version: u64,
) -> symphony_api::DocumentSections {
    let mut change = symphony_api::DocumentChange {
        path: document.path.clone(),
        ..Default::default()
    };
    change.sections = document
        .sections
        .iter()
        .filter(|section| keys.contains(&section.key))
        .cloned()
        .collect();
    for card in document
        .cards
        .iter()
        .filter(|card| keys.contains(&format!("card:{}", card.slug)))
    {
        change.cards.push(symphony_api::ItemChange {
            key: card.slug.clone(),
            value: Some(card.clone()),
            removed: false,
        });
        change
            .threads
            .insert(card.slug.clone(), document.threads.get(&card.slug).cloned());
    }
    symphony_api::DocumentSections { version, change }
}

/// The folder `rel` as a page shows it, its introduction's tasks the ones of `board` that link it.
pub fn folder(
    render: &Render,
    rel: &str,
    scope: &str,
    board: &[symphony_api::Task],
) -> Result<Folder, String> {
    target(render.root(), rel.trim_end_matches('/'), false)?;
    let page = render.folder(rel).map_err(|e| e.to_string())?;
    let mut folder = Folder {
        path: page.path.clone(),
        title: page.title,
        crumbs: wire(page.crumbs),
        docs: page.docs,
        intro: page
            .intro
            .map(|p| document(render, &p.path, scope, board))
            .transpose()?,
        entries: wire(page.entries),
    };
    if !scope.is_empty() {
        folder.path = format!("@{scope}/{}", folder.path);
        for entry in &mut folder.entries {
            entry.path = format!("@{scope}/{}", entry.path);
        }
        for crumb in &mut folder.crumbs {
            if let Some(href) = &mut crumb.href {
                *href = scoped(href, scope);
            }
        }
    }
    Ok(folder)
}

/// Count each entry of `folder` by the cards of `waiting`, the queue's: a document's own, and a
/// folder's those of every document under it, a branch's proposal among them. The home, the
/// queue and each document count those same cards, so a folder says the sum of its documents'.
pub fn count_waiting(folder: &mut Folder, waiting: &[symphony_api::Card]) {
    use crate::dashboard::document_path;
    for entry in &mut folder.entries {
        let path = document_path(&entry.path);
        let holds = |doc: &str| match entry.kind.as_str() {
            "doc" => doc == path,
            _ => doc
                .strip_prefix(path)
                .is_some_and(|rest| rest.starts_with('/')),
        };
        if matches!(entry.kind.as_str(), "doc" | "folder") {
            entry.waiting = waiting
                .iter()
                .filter(|card| holds(document_path(&card.doc)))
                .count();
        }
    }
}

pub fn section(render: &Render, rel: &str, anchor: &str, scope: &str) -> Result<Fragment, String> {
    section_with(render, rel, anchor, scope, true)
}

/// The still-served legacy card excerpt needs ordinary markup rather than typed-parser markers.
pub fn legacy_section(
    render: &Render,
    rel: &str,
    anchor: &str,
    scope: &str,
) -> Result<Fragment, String> {
    section_with(render, rel, anchor, scope, false)
}

fn section_with(
    render: &Render,
    rel: &str,
    anchor: &str,
    scope: &str,
    typed: bool,
) -> Result<Fragment, String> {
    let (_, text, _) = source(render.root(), rel, scope)?;
    let part = crate::edit::section_text(&text, anchor)
        .ok_or_else(|| format!("no such heading in {rel}"))?;
    let mut fragment: Fragment = wire(render.excerpt(&part, rel, typed));
    prepare_fragment(
        &mut fragment.html,
        &mut fragment.media,
        render.root(),
        rel,
        scope,
    );
    if typed {
        fragment.blocks =
            crate::blocks::parse(&fragment.html, &fragment.media, &scoped_path(rel, scope));
    }
    Ok(fragment)
}

pub fn history(
    render: &Render,
    rel: &str,
    scope: &str,
) -> Result<BTreeMap<String, Fragment>, String> {
    let (_, text, _) = source(render.root(), rel, scope)?;
    let mut threads: BTreeMap<String, Fragment> = wire(render.typed_history(&text, rel));
    for fragment in threads.values_mut() {
        prepare_fragment(
            &mut fragment.html,
            &mut fragment.media,
            render.root(),
            rel,
            scope,
        );
        fragment.blocks =
            crate::blocks::parse(&fragment.html, &fragment.media, &scoped_path(rel, scope));
    }
    Ok(threads)
}

fn scoped_path(rel: &str, scope: &str) -> String {
    if scope.is_empty() {
        rel.into()
    } else {
        format!("@{scope}/{rel}")
    }
}

pub fn changes(old: &Document, new: &Document) -> Option<DocumentChange> {
    let mut delta = DocumentChange {
        path: new.path.clone(),
        ..DocumentChange::default()
    };
    for section in &new.sections {
        match old.sections.iter().find(|s| s.key == section.key) {
            Some(before)
                if before.hash == section.hash && before.comparison == section.comparison =>
            {
                if (before.start, before.end, before.part)
                    != (section.start, section.end, section.part)
                {
                    delta.positions.push(SectionPosition {
                        key: section.key.clone(),
                        start: section.start,
                        end: section.end,
                        part: section.part,
                    });
                }
            }
            _ => delta.sections.push(section.clone()),
        }
    }
    delta.deleted = old
        .sections
        .iter()
        .filter(|s| !new.sections.iter().any(|n| n.key == s.key))
        .map(|s| s.key.clone())
        .collect();
    let keys = |doc: &Document| {
        doc.sections
            .iter()
            .map(|s| s.key.clone())
            .collect::<Vec<_>>()
    };
    if keys(old) != keys(new) {
        delta.order = Some(keys(new));
    }
    for (slug, fragment) in &new.threads {
        if old.threads.get(slug) != Some(fragment) {
            delta.threads.insert(slug.clone(), Some(fragment.clone()));
        }
    }
    for slug in old.threads.keys().filter(|s| !new.threads.contains_key(*s)) {
        delta.threads.insert(slug.clone(), None);
    }
    macro_rules! field {
        ($name:ident) => {
            if old.$name != new.$name {
                delta.$name = Some(new.$name.clone());
            }
        };
    }
    for card in &new.cards {
        if old.cards.iter().find(|before| before.slug == card.slug) != Some(card) {
            delta.cards.push(symphony_api::ItemChange {
                key: card.slug.clone(),
                value: Some(card.clone()),
                removed: false,
            });
        }
    }
    for card in old
        .cards
        .iter()
        .filter(|card| !new.cards.iter().any(|after| after.slug == card.slug))
    {
        delta.cards.push(symphony_api::ItemChange {
            key: card.slug.clone(),
            value: None,
            removed: true,
        });
    }
    field!(title);
    field!(waiting);
    field!(crumbs);
    field!(headings);
    field!(tasks);
    if old.comparison != new.comparison {
        delta.comparison = Some(new.comparison.clone().into_iter().collect());
    }
    let empty = DocumentChange {
        path: new.path.clone(),
        ..DocumentChange::default()
    };
    (delta != empty).then_some(delta)
}

pub fn scoped(path: &str, scope: &str) -> String {
    if !scope.is_empty() && path.starts_with('/') && !path.starts_with("//") {
        format!("/@{scope}{path}")
    } else {
        path.into()
    }
}

/// Resolve URLs against the document rather than the SPA shell, reserve picture sizes/thumbnails,
/// and name assets by their current bytes before computing a section's hash.
pub fn prepare_fragment(
    html: &mut String,
    media: &mut [Media],
    root: &Path,
    rel: &str,
    scope: &str,
) {
    static URL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"((?:href|src|data-full)=")([^"]*)"#).expect("URL attribute pattern")
    });
    *html = URL
        .replace_all(html, |c: &regex::Captures<'_>| {
            let value = if c[2].starts_with('#') {
                format!("/{rel}{}", &c[2])
            } else {
                crate::markup::target(&c[2], rel)
            };
            format!("{}{}", &c[1], crate::markup::escape(&value))
        })
        .into_owned();
    *html = crate::images::reserve(html, root, false, crate::images::sips_available());
    *html = crate::assets::bust(html, root, &root.join("tools/static"), &format!("/{rel}")).0;
    for src in media.iter_mut().flat_map(media_urls) {
        if let Some(path) =
            crate::assets::local_file(src, root, &root.join("tools/static"), &format!("/{rel}"))
        {
            let token = crate::assets::validator(&path);
            let (path, anchor) = src.split_once('#').unwrap_or((src, ""));
            *src = format!(
                "{}{}v={token}{}",
                path,
                if path.contains('?') { "&" } else { "?" },
                if anchor.is_empty() {
                    String::new()
                } else {
                    format!("#{anchor}")
                }
            );
        }
    }
    scope_fragment(html, media, scope);
}

pub fn scope_fragment(html: &mut String, media: &mut [Media], scope: &str) {
    if scope.is_empty() {
        return;
    }
    static LINK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"((?:href|src|data-full)=")/([^/])"#).expect("local URL pattern")
    });
    *html = LINK
        .replace_all(html, |c: &regex::Captures<'_>| {
            format!("{}/@{scope}/{}", &c[1], &c[2])
        })
        .into_owned();
    for src in media.iter_mut().flat_map(media_urls) {
        *src = scoped(src, scope);
    }
}

/// Whether `document`, rendered for `scope`'s page, names by its validator
/// ([`crate::assets::validator`]) a file no entrypoint of `project` serves: a picture or a script
/// from outside the documents' folders. Such a file changes what the document renders to while no
/// file under an entrypoint does, so a reader that keeps a render until one of those changes
/// renders this document each time instead.
pub fn names_files_outside(
    document: &Document,
    project: &crate::project::Project,
    scope: &str,
) -> bool {
    static LOADED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\s(?:src|data-full|data-module|poster|href)="([^"]*)""#)
            .expect("loaded URL pattern")
    });
    let own = format!("/@{scope}/");
    let outside = |url: &str| {
        let url = crate::markup::unescape(url);
        let url = url.split('#').next().unwrap_or_default();
        let Some((path, query)) = url.split_once('?') else {
            return false;
        };
        // A validator is written on a file of this project alone: another site's `v=` is its own.
        let local = path.starts_with('/') && !path.starts_with("//");
        if !local || !query.split('&').any(|pair| pair.starts_with("v=")) {
            return false;
        }
        let route = match path.strip_prefix(&own) {
            Some(inside) if !scope.is_empty() => inside,
            _ => path.trim_start_matches('/'),
        };
        project.serving(route).is_none()
    };
    let fragments = document
        .sections
        .iter()
        .map(|section| (&section.html, &section.media))
        .chain(
            document
                .threads
                .values()
                .map(|thread| (&thread.html, &thread.media)),
        );
    for (html, media) in fragments {
        if LOADED.captures_iter(html).any(|found| outside(&found[1])) {
            return true;
        }
        let mut media = media.clone();
        if media
            .iter_mut()
            .flat_map(media_urls)
            .any(|url| outside(url))
        {
            return true;
        }
    }
    false
}

/// Every URL a media item loads: its files, their posters and its viewer's script, which a branch's
/// page loads from that branch's checkout as it does the files.
fn media_urls(media: &mut Media) -> Vec<&mut String> {
    match media {
        Media::Mockup { src, .. } => vec![src],
        Media::Viewer {
            src,
            poster,
            viewer,
            ..
        } => [src, &mut viewer.script]
            .into_iter()
            .chain(poster.iter_mut())
            .collect(),
        Media::Gallery { items, viewer } => std::iter::once(&mut viewer.script)
            .chain(
                items
                    .iter_mut()
                    .flat_map(|item| std::iter::once(&mut item.src).chain(item.poster.iter_mut())),
            )
            .collect(),
        // A kind a later minor of the kit adds names its URLs here before a branch's page shows it
        // (`crates/symphony-api/tests/vocabulary_drawn.rs` holds every kind to an arm).
        _ => Vec::new(),
    }
}
