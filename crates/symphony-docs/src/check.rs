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
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::syntax;

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
    docs: Vec<String>,
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
            docs: Vec::new(),
            texts: HashMap::new(),
            anchors: HashMap::new(),
            cards: HashMap::new(),
            media: Vec::new(),
            unreadable: Vec::new(),
            listings: RefCell::new(HashMap::new()),
        };
        tree.walk(&root.join(DOCS), DOCS.to_string(), false);
        tree.docs.sort();
        tree.media.sort();
        for (rel, text) in &tree.texts {
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
        &self.docs
    }

    /// A document's text; empty for one the tree does not hold.
    pub fn text(&self, rel: &str) -> &str {
        self.texts.get(rel).map(String::as_str).unwrap_or("")
    }

    /// Whether the tree holds the document `rel`.
    pub fn has(&self, rel: &str) -> bool {
        self.texts.contains_key(rel)
    }

    /// A live document's cards (none for a history document).
    pub fn cards(&self, rel: &str) -> &[syntax::Card] {
        self.cards.get(rel).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Every component the docs build checks, with its document: those outside `docs/history/`.
    pub fn components(&self) -> Vec<(&str, syntax::Component)> {
        self.docs
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
        self.anchors.get(rel).map(Vec::as_slice).unwrap_or(&[])
    }

    fn doc_cards(&self, rel: &str) -> &[syntax::Card] {
        self.cards(rel)
    }

    fn walk(&mut self, dir: &Path, rel: String, hidden: bool) {
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
                self.walk(&dir.join(&name), child, quiet);
            } else if name.ends_with(".md") {
                match std::fs::read_to_string(dir.join(&name)) {
                    Ok(text) => {
                        self.texts.insert(child.clone(), text);
                        self.docs.push(child);
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
        self.docs.iter().filter(|rel| !is_history(rel))
    }

    /// The documents in the order a document link searches them: live ones, then history.
    fn search_order(&self) -> Vec<&str> {
        let live = self.docs.iter().filter(|rel| !is_history(rel));
        let past = self.docs.iter().filter(|rel| is_history(rel));
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
            .docs
            .iter()
            .map(|rel| DocInfo {
                path: rel.clone(),
                anchors: self.doc_anchors(rel).to_vec(),
            })
            .collect();
        let place: HashMap<&str, usize> = self
            .docs
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
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
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

// --- reachable through READMEs ---

fn reachable(tree: &Docs, problems: &mut Vec<Problem>) {
    if !tree.has_doc(ROOT_README) {
        problem(
            problems,
            ROOT_README,
            0,
            "missing, nothing can be reached from it".to_string(),
        );
        return;
    }
    let mut seen: HashSet<String> = HashSet::from([ROOT_README.to_string()]);
    let mut queue = VecDeque::from([ROOT_README.to_string()]);
    while let Some(rel) = queue.pop_front() {
        let folder = tree
            .root
            .join(&rel)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        for (_, target) in syntax::links(tree.text(&rel)) {
            let Some((path, _)) = resolve(tree, &rel, &target) else {
                continue;
            };
            let Some(page) = page_for(tree, &path) else {
                continue;
            };
            if seen.contains(&page) {
                continue;
            }
            let path = tree.root.join(&page);
            let readme = path.file_name().is_some_and(|n| n == "README.md");
            let parent = path.parent().unwrap_or(&tree.root);
            if parent == folder || (readme && parent.parent() == Some(folder.as_path())) {
                seen.insert(page.clone());
                if readme {
                    queue.push_back(page);
                }
            }
        }
    }
    for rel in &tree.docs {
        if seen.contains(rel) {
            continue;
        }
        let mut parts: Vec<&str> = rel.split('/').collect();
        parts.pop();
        if rel.ends_with("/README.md") {
            parts.pop();
        }
        let owner = format!("{}/README.md", parts.join("/"));
        problem(
            problems,
            rel,
            0,
            format!(
                "not reachable from {ROOT_README}: {owner} must link it (a link in a README, not in another page)"
            ),
        );
    }
}

// --- links ---

fn links(tree: &Docs, problems: &mut Vec<Problem>, watch: &mut BTreeSet<PathBuf>) {
    let mut anchors = Anchors {
        tree,
        cache: HashMap::new(),
    };
    let artifacts = tree.root.join("artifacts");
    let docs = tree.root.join(DOCS);
    for rel in tree.live() {
        for (line, target) in syntax::links(tree.text(rel)) {
            let Some((path, anchor)) = resolve(tree, rel, &target) else {
                continue;
            };
            if !tree.exists(&path) {
                if !(path.starts_with(&artifacts) && path != artifacts) {
                    problem(problems, rel, line, format!("[{target}]: no such file"));
                }
                continue;
            }
            if !path.starts_with(&docs) && !path.starts_with(&artifacts) {
                if is_dir(&path) {
                    // Cargo watches docs/ and each file named here; a folder outside docs/ moved away
                    // would leave the build green until some document changed.
                    problem(
                        problems,
                        rel,
                        line,
                        format!(
                            "[{target}]: a folder outside docs/, which the build cannot watch; \
                             link a file in it (its README.md or AGENTS.md)"
                        ),
                    );
                    continue;
                }
                watch.insert(path.clone());
            }
            if anchor.is_empty() {
                continue;
            }
            let doc = if is_dir(&path) {
                path.join("README.md")
            } else {
                path
            };
            if extension(&doc) == ".md"
                && tree.exists(&doc)
                && doc.is_file()
                && !anchors.of(&doc).contains(&anchor)
            {
                problem(
                    problems,
                    rel,
                    line,
                    format!("[{target}]: no heading '{anchor}' in {}", tree.rel(&doc)),
                );
            }
        }
    }
}

/// The document a `[[NAME]]` names: the path it spells from the linking page if that exists, else the
/// first document whose path ends with it (live ones before history), so a document can move folders.
fn find_doc<'a>(target: &str, from: &str, order: &[&'a str]) -> Option<&'a str> {
    let mut name = target.trim_matches(syntax::is_space).to_string();
    if !name.ends_with(".md") {
        name.push_str(".md");
    }
    let folder = from.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let direct = if name.starts_with('/') {
        normpath(&name)
    } else {
        normpath(&format!("{folder}/{name}"))
    };
    if let Some(found) = order.iter().find(|p| **p == direct) {
        return Some(found);
    }
    let tail = format!("/{}", name.trim_start_matches('/'));
    order.iter().find(|p| p.ends_with(&tail)).copied()
}

fn wikilinks(tree: &Docs, problems: &mut Vec<Problem>) {
    let order = tree.search_order();
    for rel in tree.live() {
        for link in syntax::wikilinks(tree.text(rel)) {
            if link.target.contains(':') {
                continue; // a task or a component, not a document
            }
            let target = link.target.trim_matches(syntax::is_space);
            let Some(found) = find_doc(target, rel, &order) else {
                problem(
                    problems,
                    rel,
                    link.line,
                    format!("{}: no document called {target}", link.whole),
                );
                continue;
            };
            if let Some(heading) = &link.heading
                && !tree
                    .doc_anchors(found)
                    .contains(&syntax::heading_anchor(heading))
            {
                problem(
                    problems,
                    rel,
                    link.line,
                    format!("{}: no heading '{heading}' in {found}", link.whole),
                );
            }
        }
    }
}

// --- the Folders list and the list READMEs ---

/// A list row: a bullet (`-`, `*` or `+` and a space) or a table row (`|`), after any indent.
fn is_row(line: &str) -> bool {
    let body = line.trim_start_matches(syntax::is_space);
    body.starts_with('|')
        || (body.starts_with(['-', '*', '+']) && body[1..].starts_with(syntax::is_space))
}

fn subfolders(path: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| !name.starts_with('.'))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

fn folders_list(tree: &Docs, problems: &mut Vec<Problem>) {
    if !tree.has_doc(ROOT_README) {
        return;
    }
    let mut listed: BTreeSet<String> = BTreeSet::new();
    let mut inside = false;
    for line in syntax::lines(tree.text(ROOT_README)) {
        if let Some(rest) = line.strip_prefix("##")
            && rest.starts_with(syntax::is_space)
            && rest.trim_matches(syntax::is_space) == "Folders"
        {
            inside = true;
        } else if line.starts_with('#') {
            inside = false;
        } else if inside && is_row(line) {
            listed.extend(
                syntax::line_links(line)
                    .into_iter()
                    .map(|t| t.split('/').next().unwrap_or("").to_string()),
            );
        }
    }
    let real: BTreeSet<String> = subfolders(&tree.root.join(DOCS)).into_iter().collect();
    for name in real.difference(&listed) {
        problem(
            problems,
            ROOT_README,
            0,
            format!("the Folders list is missing {name}/ (a folder under docs/)"),
        );
    }
    for name in listed.difference(&real) {
        problem(
            problems,
            ROOT_README,
            0,
            format!("the Folders list names {name}/, but there is no such folder under docs/"),
        );
    }
}

fn holds_docs(tree: &Docs, folder: &Path) -> bool {
    let prefix = format!("{}/", tree.rel(folder));
    tree.docs.iter().any(|rel| rel.starts_with(&prefix))
}

fn guides(tree: &Docs, problems: &mut Vec<Problem>) {
    for rel in tree
        .docs
        .iter()
        .filter(|rel| rel.ends_with("/README.md") && rel.as_str() != ROOT_README)
    {
        let folder = tree
            .root
            .join(rel)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let mut listed: HashSet<PathBuf> = HashSet::new();
        let mut dead: Vec<(usize, &str)> = Vec::new();
        let text = tree.text(rel);
        let lines = syntax::lines(text);
        for (index, line) in syntax::outside_fences(&lines).into_iter().enumerate() {
            let Some(line) = line.filter(|l| is_row(l)) else {
                continue;
            };
            let Some(first) = syntax::line_links(line).into_iter().next() else {
                continue;
            };
            let Some((mut path, _)) = resolve(tree, rel, first) else {
                continue;
            };
            if path.file_name().is_some_and(|n| n == "README.md")
                && path.parent().and_then(Path::parent) == Some(folder.as_path())
            {
                path.pop();
            }
            let ext = extension(&path);
            if path.parent() != Some(folder.as_path()) || !(ext.is_empty() || ext == ".md") {
                continue;
            }
            if tree.exists(&path) {
                listed.insert(path);
            } else {
                dead.push((index + 1, first));
            }
        }
        if listed.is_empty() && dead.is_empty() {
            continue; // prose: it says what the folder holds without listing it
        }
        for (line, target) in dead {
            problem(
                problems,
                rel,
                line,
                format!("lists {target}, which does not exist"),
            );
        }
        let prefix = format!("{}/", tree.rel(&folder));
        let mut wanted: Vec<(PathBuf, bool)> = tree
            .docs
            .iter()
            .filter_map(|doc| doc.strip_prefix(&prefix))
            .filter(|name| !name.contains('/') && *name != "README.md")
            .map(|name| (folder.join(name), false))
            .collect();
        wanted.extend(
            subfolders(&folder)
                .into_iter()
                .map(|name| folder.join(name))
                .filter(|p| holds_docs(tree, p))
                .map(|p| (p, true)),
        );
        for (path, dir) in wanted {
            if !listed.contains(&path) {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                problem(
                    problems,
                    rel,
                    0,
                    format!(
                        "does not list {name}{} (add a row for it)",
                        if dir { "/" } else { "" }
                    ),
                );
            }
        }
    }
}

// --- prose: card slugs ---

/// The lines of a page outside its fences and register regions, numbered from 1: the page's prose.
pub fn prose_lines(text: &str) -> Vec<(usize, &str)> {
    let mut found = Vec::new();
    let (mut fenced, mut register) = (false, false);
    for (index, line) in syntax::lines(text).into_iter().enumerate() {
        let trimmed = line.trim_matches(syntax::is_space);
        if syntax::is_fence(line) {
            fenced = !fenced;
        } else if trimmed == syntax::REGION_START || trimmed == syntax::REGION_END {
            register = trimmed == syntax::REGION_START;
        } else if !fenced && !register {
            found.push((index + 1, line));
        }
    }
    found
}

/// The maximal runs of `keep` characters in `line`, each with the character before it.
pub fn runs(line: &str, keep: impl Fn(char) -> bool) -> Vec<(Option<char>, &str)> {
    let mut found = Vec::new();
    let mut before = None;
    let mut start = None;
    for (at, c) in line.char_indices() {
        match (keep(c), start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                found.push((before, &line[from..at]));
                start = None;
            }
            _ => {}
        }
        if start.is_none() {
            before = Some(c);
        }
    }
    if let Some(from) = start {
        found.push((before, &line[from..]));
    }
    found
}

/// The card slugs a line names: a slug standing alone, `(?<![\w/#-])slug(?![\w-])`, so an anchor
/// (`#slug`), a path part (`/slug`) and a longer word are not mentions.
fn mentions<'a>(line: &'a str, slugs: &HashSet<&str>, odd: &[&str]) -> Vec<&'a str> {
    let word = |c: char| syntax::is_word(c) || c == '-';
    let mut found: Vec<(usize, &str)> = runs(line, word)
        .into_iter()
        .filter(|(before, run)| !matches!(before, Some('/' | '#')) && slugs.contains(run))
        .map(|(_, run)| (run.as_ptr() as usize - line.as_ptr() as usize, run))
        .collect();
    for slug in odd {
        for (at, _) in line.match_indices(slug) {
            let before = line[..at].chars().next_back();
            let after = line[at + slug.len()..].chars().next();
            if !before.is_some_and(|c| word(c) || c == '/' || c == '#') && !after.is_some_and(word)
            {
                found.push((at, &line[at..at + slug.len()]));
            }
        }
    }
    found.sort();
    found.into_iter().map(|(_, slug)| slug).collect()
}

fn prose(tree: &Docs, problems: &mut Vec<Problem>) {
    let scanned: Vec<&String> = tree
        .docs
        .iter()
        .filter(|rel| !rel.split('/').any(|p| p == "history" || p.starts_with('.')))
        .collect();
    let slugs: HashSet<&str> = scanned
        .iter()
        .flat_map(|rel| tree.doc_cards(rel))
        .map(|card| card.slug.as_str())
        .collect();
    let word = |c: char| syntax::is_word(c) || c == '-';
    let plain: HashSet<&str> = slugs
        .iter()
        .copied()
        .filter(|s| !s.is_empty() && s.chars().all(word))
        .collect();
    let odd: Vec<&str> = slugs
        .iter()
        .copied()
        .filter(|s| !s.is_empty() && !s.chars().all(word))
        .collect();
    for rel in scanned {
        if matches!(rel.split('/').nth(1), Some("history" | "analysis")) {
            continue; // the ledger and the evidence may say why and when
        }
        for (line, text) in prose_lines(tree.text(rel)) {
            for slug in mentions(text, &plain, &odd) {
                problem(
                    problems,
                    rel,
                    line,
                    format!(
                        "card slug {slug} in prose: say what the design is; why and when belong on the card and in {LEDGER}"
                    ),
                );
            }
        }
    }
}

// --- components ---

/// A component neither built in nor named by a check.
fn unknown_components(tree: &Docs, checks: &[&dyn DocCheck], problems: &mut Vec<Problem>) {
    let known: HashSet<String> = checks
        .iter()
        .flat_map(|check| check.components(tree))
        .chain([TASK.to_string()])
        .collect();
    for (rel, component) in tree.components() {
        if !known.contains(&component.name) {
            problem(
                problems,
                rel,
                component.line,
                format!("{}: unknown component {}", component.whole, component.name),
            );
        }
    }
}
