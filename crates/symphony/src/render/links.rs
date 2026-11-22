//! What the markdown text gets before it is converted: `[[NAME#Heading|label]]` document links, a card
//! slug that links to its card, and `[[name:arg]]` components held out as tokens, all outside code.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock};

use fancy_regex::Regex as Fancy;
use regex::{Captures, Regex};
use symphony_board::lock::Recovered as _;

use crate::edit::heading_anchor;
use crate::markup::{dirname, join, normpath};
use crate::register::parse_decisions;
use crate::text::{read_text, splitlines};

use super::CardCache;

static WIKILINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[\[(?P<target>[^\]|#]+)(?:#(?P<heading>[^\]|]+))?(?:\|(?P<label>[^\]]+))?\]\]")
        .expect("the wikilink pattern")
});
static MD_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[[^\]]*\]\([^)]*\)").expect("the link pattern"));
static CARD_REF: LazyLock<Fancy> = LazyLock::new(|| {
    Fancy::new(r"(?<=\*\*)(?P<bold>[a-z0-9][a-z0-9-]*)(?=\*\*)|(?<=\*Replaces:\* )(?P<tag>[a-z0-9][a-z0-9-]*)").expect("the card pattern")
});
static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[\[(?P<name>[a-z][a-z-]*):(?P<arg>[^\]|]*)(?:\|(?P<option>[^\]]*))?\]\]")
        .expect("the component pattern")
});
static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:<p>)?SF(?P<kind>COMPONENT|LINK)(?P<n>\d+)Z(?:</p>)?")
        .expect("the token pattern")
});

/// The length of the code span or fenced block that starts at `at`, if one does.
fn code_at(text: &str, at: usize) -> Option<usize> {
    let rest = &text[at..];
    let line_start = at == 0 || text.as_bytes()[at - 1] == b'\n';
    if line_start {
        for fence in ["```", "~~~"] {
            if !rest.starts_with(fence) {
                continue;
            }
            // up to the next line that starts with the same fence, then the rest of that line
            let mut from = fence.len();
            while let Some(newline) = rest[from..].find('\n') {
                let line = from + newline + 1;
                if rest[line..].starts_with(fence) {
                    let end = rest[line..].find('\n').map_or(rest.len(), |n| line + n);
                    return Some(end);
                }
                from = line;
            }
        }
    }
    if rest.starts_with('`') {
        let run = rest.len() - rest.trim_start_matches('`').len();
        for ticks in (1..=run).rev() {
            let mark = &rest[..ticks];
            let line_end = rest.find('\n').unwrap_or(rest.len());
            // lazily, the first `ticks` ticks after the opening ones on this line
            if let Some(close) = rest[ticks..line_end].find(mark) {
                return Some(ticks + close + ticks);
            }
        }
    }
    None
}

/// `text` with `swap` applied to every stretch outside inline code and fenced blocks.
pub(crate) fn outside_code(text: &str, swap: &mut dyn FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut last, mut at) = (0, 0);
    while at < text.len() {
        if let Some(length) = code_at(text, at) {
            out.push_str(&swap(&text[last..at]));
            out.push_str(&text[at..at + length]);
            at += length;
            last = at;
        } else {
            at += text[at..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out.push_str(&swap(&text[last..]));
    out
}

/// Every document page of the project's entrypoints, named by its route (`docs/…`), live ones
/// before those in their history folders.
pub(crate) fn doc_paths(root: &Path) -> Vec<String> {
    let project = crate::project::Project::of(root);
    let (mut live, mut history): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for (entry, file) in project.shown(root) {
        let Some(inside) = entry.inside_folder(&file) else {
            continue;
        };
        if entry.document(inside) {
            live.push(entry.route(inside));
        } else if entry.page(inside) {
            history.push(entry.route(inside));
        }
    }
    live.sort();
    history.sort();
    live.into_iter().chain(history).collect()
}

/// The document a wikilink names: the path it spells from the linking page if that exists, else the one
/// document whose path ends with it (a live one before one in history), so a document can move folders.
pub(crate) fn find_doc(target: &str, doc_path: &str, paths: &[String]) -> Option<String> {
    let mut name = target.trim().to_string();
    if !name.ends_with(".md") {
        name.push_str(".md");
    }
    let direct = normpath(&join(dirname(doc_path), &name));
    if paths.contains(&direct) {
        return Some(direct);
    }
    // A route's first name counts too: at an entrypoint on `/`, `index.md` is a whole route.
    let tail = format!("/{}", name.trim_start_matches('/'));
    paths
        .iter()
        .find(|p| format!("/{p}").ends_with(&tail))
        .cloned()
}

fn resolve_wikilink(target: &str, doc_path: &str, paths: &[String]) -> String {
    let found = find_doc(target, doc_path, paths).unwrap_or_else(|| {
        let mut name = target.trim().to_string();
        if !name.ends_with(".md") {
            name.push_str(".md");
        }
        normpath(&join(dirname(doc_path), &name))
    });
    format!("/{found}")
}

/// `[[NAME#Heading|label]]` as a markdown link to the document and the heading's anchor.
pub(crate) fn wikilinks(text: &str, doc_path: &str, paths: &[String]) -> String {
    wikilinks_for(text, doc_path, paths, false, None)
}

/// A typed link's HTML as the converter is handed it: the HTML itself, or, given a stash (for a
/// converter that escapes raw HTML, a note's), a token [`restore`] puts it back for.
fn anchor(html: String, held: &mut Option<&mut Vec<String>>) -> String {
    match held {
        Some(stash) => {
            stash.push(html);
            format!("SFLINK{}Z", stash.len() - 1)
        }
        None => html,
    }
}

/// [`wikilinks`], or with `typed` the typed page's anchors, held in `held` when one is given
/// ([`anchor`]).
pub(crate) fn wikilinks_for(
    text: &str,
    doc_path: &str,
    paths: &[String],
    typed: bool,
    mut held: Option<&mut Vec<String>>,
) -> String {
    outside_code(text, &mut |part| {
        WIKILINK
            .replace_all(part, |found: &Captures<'_>| {
                let mut href = resolve_wikilink(found["target"].trim(), doc_path, paths);
                if let Some(heading) = found.name("heading") {
                    href.push('#');
                    href.push_str(&heading_anchor(heading.as_str()));
                }
                let label = found.name("label").map_or(&found["target"], |m| m.as_str());
                if typed {
                    anchor(
                        format!(
                            "<a data-wiki=\"true\" href=\"{}\">{}</a>",
                            crate::markup::escape(&href),
                            crate::markup::escape(label)
                        ),
                        &mut held,
                    )
                } else {
                    format!("[{label}]({href})")
                }
            })
            .into_owned()
    })
}

/// Each card's slug to the project-relative path of the live document holding the first of it, as of the
/// files now: read again only when one of them changed.
pub(crate) fn card_docs(
    root: &Path,
    paths: &[String],
    cache: &CardCache,
) -> Arc<HashMap<String, String>> {
    let project = crate::project::Project::of(root);
    let live: Vec<(&String, String)> = paths
        .iter()
        .map(|rel| (rel, project.file(rel)))
        .filter(|(_, file)| project.is_document_path(file))
        .collect();
    let stamp: Vec<(String, String)> = live
        .iter()
        .map(|(rel, file)| {
            (
                (*rel).clone(),
                read_text(&root.join(file)).unwrap_or_default(),
            )
        })
        .collect();
    let mut cache = cache.lock().recover();
    if let Some((had, found)) = cache.as_ref()
        && *had == stamp
    {
        return Arc::clone(found);
    }
    let mut found = HashMap::new();
    for (rel, text) in &stamp {
        for card in parse_decisions(&splitlines(text)) {
            found.entry(card.slug).or_insert_with(|| rel.clone());
        }
    }
    let found = Arc::new(found);
    *cache = Some((stamp, Arc::clone(&found)));
    found
}

/// A card slug (in bold, or after *Replaces:*) links to its card. Typed references mark that their
/// fragment names a card rather than a heading, so the client does not offer a heading excerpt; with
/// `held`, their HTML is held there ([`anchor`]).
pub(crate) fn card_links(
    text: &str,
    cards: &HashMap<String, String>,
    typed: bool,
    mut held: Option<&mut Vec<String>>,
) -> String {
    let mut swap = |piece: &str| -> String {
        let mut out = String::with_capacity(piece.len());
        let mut last = 0;
        for found in CARD_REF.captures_iter(piece).flatten() {
            let whole = found.get(0).expect("a whole match");
            let slug = found
                .name("bold")
                .or_else(|| found.name("tag"))
                .map_or("", |m| m.as_str());
            out.push_str(&piece[last..whole.start()]);
            match cards.get(slug) {
                Some(doc) if typed => out.push_str(&anchor(
                    format!(
                        "<a data-card=\"true\" href=\"{}\">{slug}</a>",
                        crate::markup::escape(&format!("/{doc}#{slug}"))
                    ),
                    &mut held,
                )),
                Some(doc) => out.push_str(&format!("[{slug}](/{doc}#{slug})")),
                None => out.push_str(slug),
            }
            last = whole.end();
        }
        out.push_str(&piece[last..]);
        out
    };
    outside_code(text, &mut |part| {
        // a link already: its text is never linked again
        let mut out = String::with_capacity(part.len());
        let mut last = 0;
        for link in MD_LINK.find_iter(part) {
            out.push_str(&swap(&part[last..link.start()]));
            out.push_str(link.as_str());
            last = link.end();
        }
        out.push_str(&swap(&part[last..]));
        out
    })
}

fn token(n: usize) -> String {
    format!("SFCOMPONENT{n}Z")
}

/// `text` with each component reference replaced by a plain token, and the HTML each token stands for.
/// The tokens hold no markup, so markdown (tables included) cannot break or escape a component;
/// [`restore`] puts the HTML in after the conversion. `component` renders one reference.
pub(crate) fn expand_for(
    text: &str,
    component: &mut dyn FnMut(&str, &str, Option<&str>) -> String,
) -> (String, Vec<String>) {
    let mut stash: Vec<String> = Vec::new();
    let out = outside_code(text, &mut |part| {
        REFERENCE
            .replace_all(part, |found: &Captures<'_>| {
                stash.push(component(
                    &found["name"],
                    &found["arg"],
                    found.name("option").map(|m| m.as_str()),
                ));
                token(stash.len() - 1)
            })
            .into_owned()
    });
    (out, stash)
}

/// The converted HTML with each token swapped for its component or held link ([`anchor`]); a
/// component's token alone in a paragraph loses the `<p>`, a link's keeps it.
pub(crate) fn restore(converted: &str, stash: &[String]) -> String {
    if stash.is_empty() {
        return converted.to_string();
    }
    TOKEN
        .replace_all(converted, |found: &Captures<'_>| {
            let n: usize = found["n"].parse().unwrap_or(usize::MAX);
            let Some(body) = stash.get(n) else {
                return found[0].to_string();
            };
            let whole = &found[0];
            let component = &found["kind"] == "COMPONENT";
            if component && whole.starts_with("<p>") && whole.ends_with("</p>") {
                body.clone()
            } else {
                whole.replace(&format!("SF{}{n}Z", &found["kind"]), body)
            }
        })
        .into_owned()
}
