//! The docs as a tree: a folder is summarised by its README (its heading
//! names the folder, its first paragraph says what is in it), a document by its first heading and first
//! paragraph. Only an entrypoint's folder and the folders in it are pages, and only the files it
//! shows and the folders that hold some are listed.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use fancy_regex::Regex as Fancy;
use regex::Regex;
use serde::Serialize;

use crate::clock::now_stamp;
use crate::edit::{frontmatter_end, heading_anchor};
use crate::entrypoint::{HTML, MARKDOWN, join};
use crate::markup::normpath;
use crate::project::{Entrypoint, Project};
use crate::register::{Group, REGION_END, REGION_START, State, parse_decisions, supersede};
use crate::text::{read_text, splitlines, strip};

pub const SUMMARY_CHARS: usize = 220;
/// Fewer headings than this and a contents list is only noise.
pub const TOC_MIN: usize = 3;

static MARKUP: LazyLock<Fancy> = LazyLock::new(|| {
    Fancy::new(r"\[\[([^\]|#]+)(?:#[^\]|]*)?(?:\|([^\]]*))?\]\]|\[([^\]]*)\]\([^)]*\)|[*`]|(?<![A-Za-z0-9])_|_(?![A-Za-z0-9])")
        .expect("the markup pattern")
});
static TOC_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<hashes>#{2,3})\s+(?P<title>.+?)\s*#*\s*$").expect("the toc heading pattern")
});
static FENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(```|~~~)").expect("the fence pattern"));

/// A step of the way back up: `docs › Lamps › old › notes`. The page itself has no `href`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Crumb {
    pub name: String,
    pub href: Option<String>,
}

/// A heading for a contents list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Heading {
    pub level: u8,
    pub anchor: String,
    pub text: String,
}

fn body(text: &str) -> Vec<&str> {
    let lines = splitlines(text);
    match frontmatter_end(&lines) {
        Some(end) => lines[end + 1..].to_vec(),
        None => lines,
    }
}

/// The first `# ` heading outside the frontmatter.
pub(crate) fn first_heading(text: &str) -> Option<String> {
    body(text).into_iter().find_map(|line| {
        line.strip_prefix("# ")
            .map(|title| strip(title).to_string())
    })
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// A folder's name as a title: its words (`-` and `_` between them) with the first capitalised,
/// `board-design` as `Board design`.
fn folder_title(name: &str) -> String {
    let words = name.replace(['-', '_'], " ");
    let mut letters = words.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

/// The words of an HTML page's first `<title>`, its tags dropped, the common entities read and its
/// white space folded; none when it has none, or an empty one.
pub(crate) fn html_title(text: &str) -> Option<String> {
    static TITLE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)<title(?:\s[^>]*)?>(.*?)</title\s*>").expect("the title pattern")
    });
    static TAG: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<[^>]*>").expect("the tag pattern"));
    let inner = TITLE.captures(text)?.get(1)?.as_str();
    let words = TAG
        .replace_all(inner, "")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&");
    let words = words.split_whitespace().collect::<Vec<_>>().join(" ");
    (!words.is_empty()).then_some(words)
}

/// How much of an HTML page its title is read from: its head, never the whole page, so a folder of
/// large pages lists as fast as one of small ones.
const HTML_HEAD: u64 = 16 * 1024;

/// The first [`HTML_HEAD`] bytes of the HTML page at `path`, as text.
fn html_head(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(HTML_HEAD)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// A document's first heading (an HTML page's `<title>`, within its first [`HTML_HEAD`] bytes), or
/// its file name; a folder's README heading, or its name as a title ([`folder_title`]).
pub fn title(path: &Path) -> String {
    let doc = if path.is_dir() {
        path.join("README.md")
    } else {
        path.to_path_buf()
    };
    let found = if !doc.is_file() {
        None
    } else if crate::entrypoint::is_html(&doc.to_string_lossy()) {
        html_head(&doc).and_then(|head| html_title(&head))
    } else {
        read_text(&doc).ok().and_then(|text| first_heading(&text))
    };
    if let Some(found) = found {
        return found;
    }
    if path.is_dir() {
        folder_title(&file_name(path))
    } else {
        file_stem(path)
    }
}

/// The slugs of the cards that wait on the owner, in page order: unanswered, or an agent asks them something.
pub fn pending(text: &str) -> Vec<String> {
    // a replaced card is already out of the queue, before anyone writes it down
    let (text, _) = supersede(text, &now_stamp());
    parse_decisions(&splitlines(&text))
        .into_iter()
        .filter(|card| {
            // An agent's question keeps a card the owner's until it is resolved or dismissed;
            // no card kind changes that.
            let settled = matches!(card.state, State::Resolved | State::Dismissed);
            card.state == State::Pending || (card.asks_owner() && !settled)
        })
        .map(|card| card.slug)
        .collect()
}

/// Markup as the words it shows: links by their text, emphasis and code marks dropped.
pub(crate) fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for found in MARKUP.captures_iter(text).flatten() {
        let whole = found.get(0).expect("a whole match");
        out.push_str(&text[last..whole.start()]);
        let word = |n: usize| found.get(n).map(|m| m.as_str());
        out.push_str(
            word(2)
                .filter(|s| !s.is_empty())
                .or(word(1))
                .filter(|s| !s.is_empty())
                .or(word(3))
                .unwrap_or(""),
        );
        last = whole.end();
    }
    out.push_str(&text[last..]);
    strip(&out).to_string()
}

/// A folder of an entrypoint: the project, the entrypoint and the folder's path inside it, which
/// is what a folder page lists.
pub(crate) struct Place<'a> {
    pub root: &'a Path,
    pub project: &'a Project,
    pub entry: &'a Entrypoint,
    pub inside: String,
}

impl Place<'_> {
    /// The folder page at the route `rel`, when an entrypoint serves it as a folder its walk
    /// reaches ([`Project::reachable`]): never one an exclude pattern or a dot name passes over,
    /// nor one only a link leads to.
    pub(crate) fn of<'a>(root: &'a Path, project: &'a Project, rel: &str) -> Option<Place<'a>> {
        let (entry, inside) = project.serving(rel.trim_matches('/'))?;
        project
            .reachable(root, entry, inside)
            .is_some_and(|path| path.is_dir())
            .then(|| Place {
                root,
                project,
                entry,
                inside: inside.to_string(),
            })
    }

    pub(crate) fn path(&self) -> PathBuf {
        self.root.join(self.entry.file(&self.inside))
    }

    pub(crate) fn route(&self) -> String {
        self.entry.route(&self.inside)
    }

    /// The folder `name` in this one.
    pub(crate) fn sub(&self, name: &str) -> Place<'_> {
        Place {
            inside: join(&self.inside, name),
            ..*self
        }
    }

    /// The paths inside the entrypoint of the files it shows under this folder.
    fn shown(&self) -> Vec<String> {
        let mut found = Vec::new();
        self.project
            .walk(self.root, self.entry, &self.inside, &mut |inside| {
                if self.entry.shows(inside) {
                    found.push(inside.to_string());
                }
            });
        found
    }

    /// The documents under it, at any depth: the one count of documents its page, its card in
    /// the folder above and, for an entrypoint's folder, the home say. A README is its folder's
    /// navigation, not a document. A page in a history folder is history ([`Entrypoint::document`]),
    /// left out of every count above that folder; a history folder, and one in it, counts the
    /// pages it lists.
    pub(crate) fn count_docs(&self) -> usize {
        let history = self.entry.in_history(&self.inside);
        self.shown()
            .iter()
            .filter(|inside| {
                self.entry
                    .renderer(inside)
                    .is_some_and(|renderer| renderer == MARKDOWN || renderer == HTML)
                    && (history || !self.entry.in_history(inside))
                    && file_name(Path::new(inside)) != "README.md"
            })
            .count()
    }

    /// The subfolders that hold a file the entrypoint shows, by lowercase name.
    pub(crate) fn subfolders(&self) -> Vec<String> {
        let mut subs: Vec<String> = Vec::new();
        self.project
            .walk_folders(self.root, self.entry, &self.inside, &mut |name| {
                subs.push(name.to_string());
            });
        subs.retain(|name| !self.sub(name).shown().is_empty());
        symphony_board::order::sort_by_key(&mut subs, &|name| name.to_lowercase());
        subs
    }

    /// The folder's own files the entrypoint shows with a renderer `wanted` takes, by name.
    fn files(&self, wanted: impl Fn(crate::entrypoint::Renderer) -> bool) -> Vec<String> {
        let mut found = Vec::new();
        self.project
            .walk_files(self.root, self.entry, &self.inside, &mut |name| {
                if self
                    .entry
                    .renderer(&join(&self.inside, name))
                    .is_some_and(&wanted)
                {
                    found.push(name.to_string());
                }
            });
        found
    }

    /// The folder's own Markdown documents but the README, by lowercase title: those that can hold
    /// cards, in the order the queue takes them ([`index_order`]).
    pub(crate) fn documents(&self) -> Vec<String> {
        self.titled(|renderer| renderer == MARKDOWN)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    /// The folder's own documents but the README, Markdown and HTML together, each with its title
    /// ([`title`], read once), by lowercase title: what its page lists as documents.
    pub(crate) fn listed(&self) -> Vec<(String, String)> {
        self.titled(|renderer| renderer == MARKDOWN || renderer == HTML)
    }

    fn titled(
        &self,
        wanted: impl Fn(crate::entrypoint::Renderer) -> bool,
    ) -> Vec<(String, String)> {
        let mut docs: Vec<(String, String)> = self
            .files(wanted)
            .into_iter()
            .filter(|name| name != "README.md")
            .map(|name| {
                let title = title(&self.path().join(&name));
                (name, title)
            })
            .collect();
        symphony_board::order::sort_by_key(&mut docs, &|(_, title)| title.to_lowercase());
        docs
    }

    /// The folder's own files shown as they are (pictures, mockups and the files a plugin's
    /// renderer draws), by lowercase name.
    pub(crate) fn media(&self) -> Vec<String> {
        let mut media = self.files(|renderer| renderer != MARKDOWN && renderer != HTML);
        symphony_board::order::sort_by_key(&mut media, &|name| name.to_lowercase());
        media
    }
}

/// The first paragraph of prose, as plain text, cut to [`SUMMARY_CHARS`].
pub fn summary(path: &Path) -> String {
    let doc = if path.is_dir() {
        path.join("README.md")
    } else {
        path.to_path_buf()
    };
    let Some(text) = read_text(&doc).ok().filter(|_| doc.is_file()) else {
        return String::new();
    };
    const NOT_PROSE: [&str; 10] = ["#", "- ", "* ", "---", "|", ">", "<", "[[", "```", "1."];
    let mut para: Vec<&str> = Vec::new();
    for line in body(&text) {
        let s = strip(line);
        if s.is_empty() {
            if para.is_empty() {
                continue;
            }
            break;
        }
        if para.is_empty() && NOT_PROSE.iter().any(|mark| s.starts_with(mark)) {
            continue; // a list, not bold prose
        }
        para.push(s);
    }
    let text = plain(&para.join(" "));
    if text.chars().count() <= SUMMARY_CHARS {
        return text;
    }
    let cut: String = text.chars().take(SUMMARY_CHARS - 1).collect();
    let words = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{words}…")
}

/// How many documents the folder page `rel` holds, at any depth, as its page says: none for a
/// route that is no folder page.
pub fn count_documents(root: &Path, rel: &str) -> usize {
    let project = Project::of(root);
    Place::of(root, &project, rel).map_or(0, |place| place.count_docs())
}

/// A folder page: an entrypoint's folder or a folder in it, once `..` is resolved, that the
/// entrypoint serving the route `rel` lists.
pub fn is_folder(root: &Path, rel: &str) -> bool {
    Place::of(root, &Project::of(root), rel).is_some()
}

/// Every document under the folder page `rel` in the order the folder pages list them, by route:
/// the README, then each subfolder (by lowercase name) in turn, then the folder's documents by
/// lowercase title. A route that is no folder page holds none.
pub fn index_order(root: &Path, rel: &str) -> Vec<String> {
    let project = Project::of(root);
    let Some(place) = Place::of(root, &project, rel) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    order(&place, &mut found);
    found
}

fn order(place: &Place<'_>, found: &mut Vec<String>) {
    let readme = join(&place.inside, "README.md");
    if place.entry.page(&readme) && place.root.join(place.entry.file(&readme)).is_file() {
        found.push(place.entry.route(&readme));
    }
    for sub in place.subfolders() {
        order(&place.sub(&sub), found);
    }
    found.extend(
        place
            .documents()
            .iter()
            .map(|doc| place.entry.route(&join(&place.inside, doc))),
    );
}

/// `docs › Lamps › old › notes`: the entrypoint's own route first, then every folder above the page,
/// each linking to its folder page; the page itself is plain. An entrypoint at `/` is named by its
/// folder's title.
pub fn crumbs(root: &Path, rel: &str) -> Vec<Crumb> {
    let project = Project::of(root);
    let Some((entry, inside)) = project.serving(rel.trim_matches('/')) else {
        return Vec::new();
    };
    let parts: Vec<&str> = inside.split('/').filter(|p| !p.is_empty()).collect();
    let top = Crumb {
        name: if entry.url.is_empty() {
            title(&root.join(&entry.folder))
        } else {
            entry.url.clone()
        },
        href: (!parts.is_empty()).then(|| entry.label()),
    };
    std::iter::once(top)
        .chain((0..parts.len()).map(|i| {
            let here = normpath(&parts[..=i].join("/"));
            Crumb {
                name: title(&root.join(entry.file(&here))),
                href: (i + 1 < parts.len()).then(|| format!("/{}/", entry.route(&here))),
            }
        }))
        .collect()
}

/// The second- and third-level headings outside code, each anchor once. Inside a register every
/// group is listed, with the topics only of the groups that wait on the owner.
pub fn headings(text: &str) -> Vec<Heading> {
    let waiting: Vec<&str> = [Group::Decide, Group::Auto, Group::Confirm]
        .iter()
        .map(|g| g.title())
        .collect();
    let (mut out, mut seen): (Vec<Heading>, Vec<String>) = (Vec::new(), Vec::new());
    let (mut fenced, mut in_register, mut listed) = (false, false, false);
    for line in body(text) {
        if strip(line) == REGION_START || strip(line) == REGION_END {
            in_register = strip(line) == REGION_START;
            listed = true;
            continue;
        }
        if FENCE.is_match(line) {
            fenced = !fenced;
            continue;
        }
        let Some(found) = TOC_HEADING.captures(line).filter(|_| !fenced) else {
            continue;
        };
        // In a register every group is listed, and the topics only of a group that waits on the
        // owner: a settled group's topics repeat earlier groups' names (Releases, Page loading),
        // and a repeated heading's anchor leads to its first.
        if in_register {
            if found["hashes"].len() == 2 {
                listed = waiting.contains(&strip(&found["title"]));
            } else if !listed {
                continue;
            }
        }
        let anchor = heading_anchor(&found["title"]);
        if !anchor.is_empty() && !seen.contains(&anchor) {
            seen.push(anchor.clone());
            out.push(Heading {
                level: u8::try_from(found["hashes"].len()).unwrap_or(2),
                anchor,
                text: plain(&found["title"]),
            });
        }
    }
    out
}

#[cfg(test)]
mod ordering_tests {
    use super::*;
    use crate::test_tree::Tree;

    #[test]
    fn folder_and_media_outputs_match_the_previous_walk_and_stable_order() {
        let tree = Tree::new();
        tree.write(
            "symphony.toml",
            "[[entrypoints]]\nurl=\"/docs/\"\nfolder=\"docs\"\ntypes=[\"markdown\",\"media\"]\n",
        );
        for name in ["beta", "Alpha", "Éclair", "東京"] {
            tree.write(&format!("docs/{name}/A.md"), "# A\n");
            tree.write(&format!("docs/{name}.png"), "fixture");
        }
        tree.write("docs/empty/.keep", "");
        let project = Project::of(&tree.root);
        let place = Place::of(&tree.root, &project, "docs").unwrap();
        let mut folders = Vec::new();
        project.walk_folders(&tree.root, place.entry, &place.inside, &mut |name| {
            folders.push(name.to_string())
        });
        folders.retain(|name| !place.sub(name).shown().is_empty());
        symphony_board::order::sort_by_key(&mut folders, &|name| name.to_lowercase());
        assert_eq!(place.subfolders(), folders);
        let mut media = place.files(|renderer| {
            renderer != crate::entrypoint::MARKDOWN && renderer != crate::entrypoint::HTML
        });
        symphony_board::order::sort_by_key(&mut media, &|name| name.to_lowercase());
        assert_eq!(place.media(), media);
        assert_eq!(media.len(), 4);
        let empty = place.sub("empty");
        assert!(empty.subfolders().is_empty());
        assert!(empty.media().is_empty());
    }
}
