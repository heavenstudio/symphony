//! The checks a project's docs build runs over its `docs/`, each naming the `file:line` of a
//! reference that does not resolve:
//!
//! - **Reachable through READMEs.** From `docs/README.md`, a folder's README links each document in its
//!   folder and each subfolder's README; a page only another page links is not reached.
//! - **Links.** Outside `docs/history/`, every inline link (relative, or from the project root) and every
//!   `[[NAME#Heading]]` resolves, heading included: inside the project, and spelt as the file is (Linux,
//!   where CI builds, tells `two.md` from `Two.md`). A folder outside `docs/` is refused, since cargo
//!   could not watch it; a file there is watched. A missing `/artifacts/` file is not the build's to
//!   judge: that folder is not in the repository.
//! - **The Folders list** in `docs/README.md` names every folder under `docs/` and no other.
//! - **List READMEs.** A README with a row (a bullet or a table row) whose first link is a file or
//!   subfolder of its own folder lists every document and subfolder there, and nothing that is gone.
//! - **Prose.** A design document names no card slug outside its register (history and the top
//!   `analysis/` folder aside).
//! - **Components.** Every `[[name:…]]` outside `docs/history/` is the built-in `task` or one a
//!   [`DocCheck`] resolves; what its argument may name is that check's.
//!
//! A project's own rules over its documents and data are [`DocCheck`]s, run in the same pass over
//! the same reading of the tree ([`Docs`]).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::syntax;

mod links;
mod lists;
mod prose;

use links::{links, reachable, wikilinks};
use lists::{folders_list, guides};
use prose::{prose, unknown_components};
pub use prose::{prose_lines, runs};

/// One reference that does not resolve, at its file and line (0 when the whole file is meant).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub file: String,
    pub line: usize,
    pub message: String,
}

impl Problem {
    pub fn new(file: &str, line: usize, message: String) -> Self {
        Self {
            file: file.to_string(),
            line,
            message,
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}: {}", self.file, self.message)
        } else {
            write!(f, "{}:{}: {}", self.file, self.line, self.message)
        }
    }
}

/// A document and its heading anchors, sorted.
#[derive(Clone, Debug, Default)]
pub struct DocInfo {
    pub path: String,
    pub anchors: Vec<String>,
}

/// A card on a live document: its slug, its document (an index into [`Index::docs`]) and its line.
#[derive(Clone, Debug, Default)]
pub struct CardInfo {
    pub slug: String,
    pub doc: usize,
    pub line: usize,
}

/// What the docs hold, when they hold together.
#[derive(Clone, Debug, Default)]
pub struct Index {
    /// Every document under `docs/`, history included, sorted by path.
    pub docs: Vec<DocInfo>,
    /// Every card on a live document, sorted by slug; a slug on two pages is the first page's.
    pub cards: Vec<CardInfo>,
    /// Every other file under `docs/`, sorted.
    pub media: Vec<String>,
    /// Files outside `docs/` a link reaches or a check reads: the build reruns when one changes or
    /// goes.
    pub watch: Vec<PathBuf>,
}

/// A project's own check over its documents and data, run by its docs build beside the checks
/// above, and by `symphony-status --lint`.
pub trait DocCheck: Send + Sync {
    fn name(&self) -> &str;
    /// The components whose arguments it checks, by name: a `[[name:…]]` no check names is an
    /// unknown component.
    fn components(&self, _docs: &Docs) -> Vec<String> {
        Vec::new()
    }
    /// The files outside `docs/` it reads, project-relative: the docs build reruns when one changes.
    fn inputs(&self) -> Vec<String> {
        Vec::new()
    }
    /// Each problem in the project's documents and data.
    fn check(&self, docs: &Docs) -> Vec<Problem>;
}

const DOCS: &str = "docs";
const ROOT_README: &str = "docs/README.md";
const LEDGER: &str = "docs/history/review-ledger.md";
/// The component every project has: a link to a task on its board.
const TASK: &str = "task";

/// Every reference under `root/docs` checked, with `checks`' own rules; the index when they all
/// resolve, else every problem.
pub fn check(root: &Path, checks: &[&dyn DocCheck]) -> Result<Index, Vec<Problem>> {
    let tree = Docs::read(root);
    let mut problems = tree.unreadable.clone();
    let mut watch = BTreeSet::new();
    reachable(&tree, &mut problems);
    links(&tree, &mut problems, &mut watch);
    wikilinks(&tree, &mut problems);
    folders_list(&tree, &mut problems);
    guides(&tree, &mut problems);
    prose(&tree, &mut problems);
    unknown_components(&tree, checks, &mut problems);
    for check in checks {
        problems.extend(check.check(&tree));
        watch.extend(check.inputs().iter().map(|input| tree.root.join(input)));
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(tree.index(watch.into_iter().collect()))
}

// --- the tree ---

/// The files under a project's `docs/`, read and parsed once for every check: the documents' text,
/// their anchors and cards, the other files.
pub struct Docs {
    root: PathBuf,
    /// Every document, sorted by its project-relative path.
    documents: Vec<String>,
    texts: HashMap<String, String>,
    /// Each document's heading anchors, sorted; each live document's cards.
    anchors: HashMap<String, Vec<String>>,
    cards: HashMap<String, Vec<syntax::Card>>,
    media: Vec<String>,
    unreadable: Vec<Problem>,
    /// Each folder's entries as spelt on disk, read once, for the case-exact `exists`.
    listings: RefCell<HashMap<PathBuf, HashSet<OsString>>>,
}

impl Docs {
    /// The documents under `root/docs`, `root` canonical where it can be.
    pub fn read(root: &Path) -> Docs {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let mut tree = Docs {
            root: root.clone(),
            documents: Vec::new(),
            texts: HashMap::new(),
            anchors: HashMap::new(),
            cards: HashMap::new(),
            media: Vec::new(),
            unreadable: Vec::new(),
            listings: RefCell::new(HashMap::new()),
        };
        tree.walk(&root.join(DOCS), DOCS, false);
        tree.documents.sort();
        tree.media.sort();
        for rel in &tree.documents {
            let Some(text) = tree.texts.get(rel) else {
                continue;
            };
            tree.anchors.insert(rel.clone(), syntax::anchors(text));
            if !is_history(rel) {
                tree.cards
                    .insert(rel.clone(), syntax::cards(&syntax::lines(text)));
            }
        }
        tree
    }

    /// The project's root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every document, history included, by its project-relative path, sorted.
    pub fn paths(&self) -> &[String] {
        &self.documents
    }

    /// A document's text; empty for one the tree does not hold.
    pub fn text(&self, rel: &str) -> &str {
        self.texts.get(rel).map_or("", String::as_str)
    }

    /// Whether the tree holds the document `rel`.
    pub fn has(&self, rel: &str) -> bool {
        self.texts.contains_key(rel)
    }

    /// A live document's cards (none for a history document).
    pub fn cards(&self, rel: &str) -> &[syntax::Card] {
        self.cards.get(rel).map_or(&[], Vec::as_slice)
    }

    /// Every component the docs build checks, with its document: those outside `docs/history/`.
    pub fn components(&self) -> Vec<(&str, syntax::Component)> {
        self.documents
            .iter()
            .filter(|rel| !rel.starts_with("docs/history/"))
            .flat_map(|rel| {
                syntax::components(self.text(rel))
                    .into_iter()
                    .map(move |component| (rel.as_str(), component))
            })
            .collect()
    }

    fn doc_anchors(&self, rel: &str) -> &[String] {
        self.anchors.get(rel).map_or(&[], Vec::as_slice)
    }

    fn doc_cards(&self, rel: &str) -> &[syntax::Card] {
        self.cards(rel)
    }

    #[expect(
        clippy::case_sensitive_file_extension_comparisons,
        reason = "a document is a `.md` file in lower case, as the Symphony serves one"
    )]
    fn walk(&mut self, dir: &Path, rel: &str, hidden: bool) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut names: Vec<(String, bool)> = entries
            .flatten()
            .map(|e| {
                (
                    e.file_name().to_string_lossy().into_owned(),
                    e.file_type().is_ok_and(|t| t.is_dir()),
                )
            })
            .collect();
        names.sort();
        for (name, is_dir) in names {
            let child = format!("{rel}/{name}");
            let quiet = hidden || name.starts_with('.');
            if is_dir {
                self.walk(&dir.join(&name), &child, quiet);
            } else if name.ends_with(".md") {
                match std::fs::read_to_string(dir.join(&name)) {
                    Ok(text) => {
                        self.texts.insert(child.clone(), text);
                        self.documents.push(child);
                    }
                    Err(error) => self.unreadable.push(Problem {
                        file: child,
                        line: 0,
                        message: format!("cannot be read as UTF-8: {error}"),
                    }),
                }
            } else if !quiet {
                self.media.push(child);
            }
        }
    }

    fn has_doc(&self, rel: &str) -> bool {
        self.has(rel)
    }

    fn live(&self) -> impl Iterator<Item = &String> {
        self.documents.iter().filter(|rel| !is_history(rel))
    }

    /// The documents in the order a document link searches them: live ones, then history.
    fn search_order(&self) -> Vec<&str> {
        let live = self.documents.iter().filter(|rel| !is_history(rel));
        let past = self.documents.iter().filter(|rel| is_history(rel));
        live.chain(past).map(String::as_str).collect()
    }

    /// A path's project-relative spelling; the absolute path when it lies outside the project.
    fn rel(&self, path: &Path) -> String {
        match path.strip_prefix(&self.root) {
            Ok(inside) => inside.to_string_lossy().replace('\\', "/"),
            Err(_) => path.to_string_lossy().into_owned(),
        }
    }

    /// Whether `path`, inside the project, exists spelt exactly so: each part as its folder lists it.
    /// macOS finds `Two.md` for `two.md`; Linux, where CI builds, does not, so neither does this.
    fn exists(&self, path: &Path) -> bool {
        let Ok(inside) = path.strip_prefix(&self.root) else {
            return false;
        };
        let mut folder = self.root.clone();
        for part in inside.components() {
            let Component::Normal(name) = part else {
                return false;
            };
            let mut listings = self.listings.borrow_mut();
            let listing = listings.entry(folder.clone()).or_insert_with(|| {
                std::fs::read_dir(&folder)
                    .map(|entries| entries.flatten().map(|e| e.file_name()).collect())
                    .unwrap_or_default()
            });
            if !listing.contains(name) {
                return false;
            }
            folder.push(name);
        }
        true
    }

    fn index(&self, watch: Vec<PathBuf>) -> Index {
        let docs: Vec<DocInfo> = self
            .documents
            .iter()
            .map(|rel| DocInfo {
                path: rel.clone(),
                anchors: self.doc_anchors(rel).to_vec(),
            })
            .collect();
        let place: HashMap<&str, usize> = self
            .documents
            .iter()
            .enumerate()
            .map(|(at, rel)| (rel.as_str(), at))
            .collect();
        let mut cards: BTreeMap<String, CardInfo> = BTreeMap::new();
        for rel in self.live() {
            for card in self.doc_cards(rel) {
                cards.entry(card.slug.clone()).or_insert(CardInfo {
                    slug: card.slug.clone(),
                    doc: place[rel.as_str()],
                    line: card.line,
                });
            }
        }
        Index {
            docs,
            cards: cards.into_values().collect(),
            media: self.media.clone(),
            watch,
        }
    }
}

/// Whether a document is under a `history/` folder: it describes a past state, and its links rot by design.
pub fn is_history(rel: &str) -> bool {
    let mut parts: Vec<&str> = rel.split('/').collect();
    parts.pop();
    parts.contains(&"history")
}

fn problem(problems: &mut Vec<Problem>, file: &str, line: usize, message: String) {
    problems.push(Problem::new(file, line, message));
}

// --- paths ---

/// `path` with `.` and `..` taken out by spelling alone (the docs hold no links through symlinks).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// POSIX `normpath` on a slash path: `.` dropped, `..` taking the part before it.
fn normpath(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            ".." if absolute => {}
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

fn is_external(target: &str) -> bool {
    let Some((scheme, _)) = target.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
}

/// The file or folder a link points at, and its anchor; `None` for a link to another site. A link from
/// the root (`/docs/...`) is read from the project root, as the app serves it, and one that leaves the
/// project points at nothing.
fn resolve(tree: &Docs, rel: &str, target: &str) -> Option<(PathBuf, String)> {
    if is_external(target) {
        return None;
    }
    let (path, anchor) = target.split_once('#').unwrap_or((target, ""));
    let path = path.split('?').next().unwrap_or("");
    let found = if let Some(rooted) = target.strip_prefix('/') {
        let rooted = rooted
            .split('#')
            .next()
            .unwrap_or("")
            .split('?')
            .next()
            .unwrap_or("")
            .trim_start_matches('/');
        let joined = normalize(&tree.root.join(rooted));
        if joined.starts_with(&tree.root) {
            joined
        } else {
            tree.root.join(".outside-project")
        }
    } else if path.is_empty() {
        tree.root.join(rel)
    } else {
        let joined = normalize(
            &tree
                .root
                .join(rel)
                .parent()
                .unwrap_or(&tree.root)
                .join(path),
        );
        if joined.starts_with(&tree.root) {
            joined
        } else {
            tree.root.join(".outside-project")
        }
    };
    Some((found, anchor.to_string()))
}

fn is_dir(path: &Path) -> bool {
    path.is_dir()
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default()
}

/// The document a link stands for: a document under `docs/` itself, or a folder's README.
fn page_for(tree: &Docs, path: &Path) -> Option<String> {
    let page = if tree.has_doc(&tree.rel(&path.join("README.md"))) && is_dir(path) {
        path.join("README.md")
    } else {
        path.to_path_buf()
    };
    let rel = tree.rel(&page);
    tree.has_doc(&rel).then_some(rel)
}

struct Anchors<'t> {
    tree: &'t Docs,
    cache: HashMap<PathBuf, Vec<String>>,
}

impl Anchors<'_> {
    /// A document's anchors: one under `docs/` from the tree, any other read once.
    fn of(&mut self, doc: &Path) -> &[String] {
        let rel = self.tree.rel(doc);
        if self.tree.has_doc(&rel) {
            return self.tree.doc_anchors(&rel);
        }
        self.cache.entry(doc.to_path_buf()).or_insert_with(|| {
            std::fs::read_to_string(doc)
                .map(|text| syntax::anchors(&text))
                .unwrap_or_default()
        })
    }
}
