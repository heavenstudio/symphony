//! A document, and a folder, as the data a client draws: for a document its title, what waits on the
//! owner, its way back up, its headings and its sections grouped into cards; for a folder its intro and
//! what it holds, without the page around them.

use std::io;
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::entrypoint::join;
use crate::render::{Render, Section};
use crate::text::read_text;
use crate::tree::{self, Crumb, Heading};

/// A document ready to draw.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Document {
    /// Project-relative, `docs/…`.
    pub path: String,
    /// The first heading as plain words, or the file name.
    pub title: String,
    /// The slugs of the cards that wait on the owner, in page order.
    pub waiting: Vec<String>,
    pub crumbs: Vec<Crumb>,
    /// The second- and third-level headings, for a contents list (shown from [`tree::TOC_MIN`] of them).
    pub headings: Vec<Heading>,
    pub sections: Vec<Section>,
}

impl Document {
    /// Whether the page has enough headings for a contents list.
    pub fn has_toc(&self) -> bool {
        self.headings.len() >= tree::TOC_MIN
    }

    /// The sections of `self` whose fragment is not one `before` had: a live update sends these.
    pub fn changed_since(&self, before: &Document) -> Vec<&Section> {
        self.sections
            .iter()
            .filter(|now| {
                !before
                    .sections
                    .iter()
                    .any(|then| then.key == now.key && then.hash == now.hash)
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Folder,
    Doc,
    /// A file a media renderer shows, opened as it is.
    Media,
}

/// One thing a folder holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub kind: EntryKind,
    /// Project-relative, no trailing slash.
    pub path: String,
    /// A document's first heading as written, or its file name; a folder's README heading, or its name.
    pub title: String,
    /// For a folder, the documents under it; a README is its navigation, not a document.
    pub docs: usize,
    /// For a document, how many decisions wait on the owner.
    pub waiting: usize,
    pub summary: String,
}

/// A folder page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Folder {
    pub path: String,
    pub title: String,
    pub crumbs: Vec<Crumb>,
    /// The documents directly in it.
    pub docs: usize,
    /// Its README, rendered as the intro.
    pub intro: Option<Document>,
    /// Its subfolders by lowercase name, then its documents by lowercase title.
    pub entries: Vec<Entry>,
}

fn opens_with(html: &str, tag: &str) -> bool {
    html.trim_start()
        .strip_prefix(tag)
        .is_some_and(|rest| rest.starts_with([' ', '>']))
}

/// A title with nothing else in it: one `<h1>` and no more.
fn title_only(html: &str) -> bool {
    let html = html.trim();
    opens_with(html, "<h1") && html.ends_with("</h1>") && html.matches("</h1>").count() == 1
}

/// Group the sections into the cards of the page: a new card at each second-level heading. A title with
/// nothing under it stays outside them, rather than leave an empty card.
pub(crate) fn assign_parts(sections: &mut [Section]) {
    let skip = usize::from(
        sections.len() > 1 && title_only(&sections[0].html) && opens_with(&sections[1].html, "<h2"),
    );
    let mut part = None;
    for (i, section) in sections.iter_mut().enumerate().skip(skip) {
        if i == skip || opens_with(&section.html, "<h2") {
            part = Some(part.map_or(0, |n| n + 1));
        }
        section.part = part;
    }
}

fn fingerprint(root: &Path, text: &str, components: &str) -> String {
    let mut digest = Sha256::new();
    let mut field = |value: &str| {
        digest.update(value.len().to_le_bytes());
        digest.update(value.as_bytes());
    };
    field(text);
    field(components);
    // Card links read all live documents, wikilinks read their paths, and breadcrumbs read READMEs.
    // Hash content rather than metadata: an equal-length rewrite with a preserved mtime is an edit too.
    let project = crate::project::Project::of(root);
    for path in crate::render::doc_paths(root) {
        field(&path);
        field(&read_text(&root.join(project.file(&path))).unwrap_or_default());
    }
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn not_found(rel: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, format!("no such page: {rel}"))
}

impl Render {
    /// The file the document name `rel` stands for under the project's root.
    fn file(&self, rel: &str) -> std::path::PathBuf {
        self.root()
            .join(crate::project::Project::of(self.root()).file(rel))
    }

    /// The document named by its route `rel` (`docs/…`, in its entrypoint's folder), cached by its text,
    /// all document inputs (card locations, wikilinks and breadcrumb titles), and the component
    /// provider's version. Unversioned providers render afresh.
    pub fn document(&self, rel: &str) -> io::Result<Arc<Document>> {
        let text = read_text(&self.file(rel))?;
        let version = self
            .component_version()
            .map(|version| fingerprint(self.root(), &text, &version));
        if let Some(found) = version
            .as_ref()
            .and_then(|version| self.cached(rel, version))
        {
            return Ok(found);
        }
        let mut sections = self.sections(&text, rel);
        assign_parts(&mut sections);
        let heading = tree::first_heading(&text);
        let doc = Arc::new(Document {
            path: rel.to_string(),
            title: heading.map_or_else(
                || {
                    Path::new(rel)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default()
                },
                |name| tree::plain(&name),
            ),
            waiting: tree::pending(&text),
            crumbs: tree::crumbs(self.root(), rel),
            headings: tree::headings(&text),
            sections,
        });
        if let Some(version) = version {
            self.remember(rel, version, &doc);
        }
        Ok(doc)
    }

    /// The folder page at `rel`: its README as the intro, then its subfolders, its documents and
    /// the media its entrypoint shows.
    pub fn folder(&self, rel: &str) -> io::Result<Folder> {
        let rel = rel.trim_matches('/');
        let project = crate::project::Project::of(self.root());
        let Some(place) = tree::Place::of(self.root(), &project, rel) else {
            return Err(not_found(rel));
        };
        let folder = place.path();
        let route = |name: &str| place.entry.route(&join(&place.inside, name));
        let intro = match place.entry.page(&join(&place.inside, "README.md"))
            && folder.join("README.md").is_file()
        {
            true => Some(self.document(&route("README.md"))?.as_ref().clone()),
            false => None,
        };
        let mut entries = Vec::new();
        for name in place.subfolders() {
            let sub = folder.join(&name);
            entries.push(Entry {
                kind: EntryKind::Folder,
                path: route(&name),
                title: tree::title(&sub),
                docs: place.sub(&name).count_docs(),
                waiting: 0,
                summary: tree::summary(&sub),
            });
        }
        let documents = place.documents();
        for name in &documents {
            let doc = folder.join(name);
            let text = read_text(&doc)?;
            entries.push(Entry {
                kind: EntryKind::Doc,
                path: route(name),
                title: tree::title(&doc),
                docs: 0,
                waiting: tree::pending(&text).len(),
                summary: tree::summary(&doc),
            });
        }
        for name in place.media() {
            entries.push(Entry {
                kind: EntryKind::Media,
                path: route(&name),
                title: name,
                docs: 0,
                waiting: 0,
                summary: String::new(),
            });
        }
        Ok(Folder {
            path: place.route(),
            title: tree::title(&folder),
            crumbs: tree::crumbs(self.root(), &format!("{rel}/")),
            docs: documents.len(),
            intro,
            entries,
        })
    }
}
