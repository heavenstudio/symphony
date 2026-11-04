//! The docs as a tree: a folder is summarised by its README (its heading
//! names the folder, its first paragraph says what is in it), a document by its first heading and first
//! paragraph. Only folders under `docs/` are pages, and only markdown and the folders that hold some are
//! listed.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use fancy_regex::Regex as Fancy;
use regex::Regex;
use serde::Serialize;

use crate::clock::now_stamp;
use crate::edit::{frontmatter_end, heading_anchor};
use crate::markup::normpath;
use crate::register::{Group, REGION_END, REGION_START, State, parse_decisions, supersede};
use crate::text::{read_text, splitlines, strip};

pub const ROOT: &str = "docs";
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

/// A step of the way back up: `docs › Ships › old › notes`. The page itself has no `href`.
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

/// A document's first heading, or its file name; a folder's README heading, or its name.
pub fn title(path: &Path) -> String {
    let doc = if path.is_dir() {
        path.join("README.md")
    } else {
        path.to_path_buf()
    };
    if let Some(found) = read_text(&doc)
        .ok()
        .filter(|_| doc.is_file())
        .and_then(|text| first_heading(&text))
    {
        return found;
    }
    if path.is_dir() {
        file_name(path)
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

/// The documents under a folder; a README is its navigation, not a document.
pub(crate) fn count_docs(folder: &Path) -> usize {
    markdown_under(folder)
        .iter()
        .filter(|p| file_name(p) != "README.md")
        .count()
}

fn markdown_under(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(markdown_under(&path));
            } else if file_name(&path).ends_with(".md") {
                found.push(path);
            }
        }
    }
    found
}

pub(crate) fn holds_docs(folder: &Path) -> bool {
    !markdown_under(folder).is_empty()
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

fn lowercase_name(path: &Path) -> String {
    file_name(path).to_lowercase()
}

/// The subfolders that hold documents, by lowercase name.
pub(crate) fn subfolders(folder: &Path) -> Vec<PathBuf> {
    let mut subs: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir() && holds_docs(p))
                .collect()
        })
        .unwrap_or_default();
    subs.sort_by_key(|p| lowercase_name(p));
    subs
}

/// The folder's own documents but the README, by lowercase title.
pub(crate) fn documents(folder: &Path) -> Vec<PathBuf> {
    let mut docs: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file() && file_name(p).ends_with(".md") && file_name(p) != "README.md"
                })
                .collect()
        })
        .unwrap_or_default();
    docs.sort_by_cached_key(|p| title(p).to_lowercase());
    docs
}

/// The file or folder under `root` the document name `rel` (`docs/…`) stands for.
fn file(root: &Path, rel: &str) -> PathBuf {
    root.join(crate::project::Project::of(root).file(rel))
}

/// A folder page: `docs/` or a folder under it, once `..` is resolved, in the project's documents
/// folder.
pub fn is_folder(root: &Path, rel: &str) -> bool {
    let Ok(docs) = file(root, ROOT).canonicalize() else {
        return false;
    };
    let Ok(target) = file(root, rel.trim_matches('/')).canonicalize() else {
        return false;
    };
    target.is_dir() && target.starts_with(&docs)
}

/// Every document under the folder named `rel` in the order the folder pages list them, by name: the
/// README, then each subfolder (by lowercase name) in turn, then the folder's documents by
/// lowercase title.
pub fn index_order(root: &Path, rel: &str) -> Vec<String> {
    let folder = file(root, rel);
    let mut found = Vec::new();
    if folder.join("README.md").is_file() {
        found.push(format!("{rel}/README.md"));
    }
    for sub in subfolders(&folder) {
        found.extend(index_order(root, &format!("{rel}/{}", file_name(&sub))));
    }
    found.extend(
        documents(&folder)
            .iter()
            .map(|doc| format!("{rel}/{}", file_name(doc))),
    );
    found
}

/// `docs › Ships › old › notes`: every folder above the page links to its folder page; the page itself is plain.
pub fn crumbs(root: &Path, rel: &str) -> Vec<Crumb> {
    let parts: Vec<&str> = rel
        .trim_matches('/')
        .split('/')
        .filter(|p| !p.is_empty())
        .collect();
    if parts.first() != Some(&ROOT) {
        return Vec::new();
    }
    (0..parts.len())
        .map(|i| {
            let here = parts[..=i].join("/");
            let name = if i == 0 {
                ROOT.to_string()
            } else {
                title(&file(root, &normpath(&here)))
            };
            let href = (i + 1 < parts.len()).then(|| format!("/{here}/"));
            Crumb { name, href }
        })
        .collect()
}

/// The second- and third-level headings outside code, each anchor once. Inside a register only the
/// groups that wait on the owner are listed, with their topics.
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
        if in_register {
            if found["hashes"].len() == 2 {
                listed = waiting.contains(&strip(&found["title"]));
            }
            if !listed {
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
