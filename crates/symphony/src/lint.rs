//! The prose rules `symphony-status --lint` holds: a document says what the design is, not how it was
//! decided. The whole check is this file, which merge risk rates as a gate.

use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::board::{relative, scan};
use crate::register::{DOCS_DIR, REGION_END, REGION_START, ReviewError, is_fence};
use crate::text::{is_word, os_error, read_text, splitlines, strip};

/// A word the lint found: (path, line, word).
pub type Finding = (String, usize, String);

/// The ledger and the evidence may say why and when: the first folder under `docs/` exempt.
const PROSE_EXEMPT: [&str; 2] = ["history", "analysis"];
const VERDICT_PAGES: &str = "factions";
static VERDICT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)approved|rejected|superseded|dismissed").expect("the verdict words")
});

/// Python's `re.escape`: the characters a pattern could read as syntax, each behind a backslash.
fn re_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "()[]{}?*+-|^$\\.&~# \t\n\r\x0b\x0c".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The lines of a document outside its code fences and its register region, numbered from 1. A
/// region marker inside a fence still counts.
fn prose(text: &str) -> Vec<(usize, &str)> {
    let (mut in_register, mut in_fence) = (false, false);
    let mut found = Vec::new();
    for (index, line) in splitlines(text).into_iter().enumerate() {
        if is_fence(line) {
            in_fence = !in_fence;
        } else if strip(line) == REGION_START || strip(line) == REGION_END {
            in_register = strip(line) == REGION_START;
        } else if !in_fence && !in_register {
            found.push((index + 1, line));
        }
    }
    found
}

/// The slugs named in `line`, as Python's `findall` of `(?<![\w/#-])(slug|…)(?![\w-])` with the
/// longer slugs tried first: `slugs` is keyed by first character, each list in that order.
fn named_in<'a>(line: &str, slugs: &HashMap<char, Vec<&'a str>>) -> Vec<&'a str> {
    let mut found = Vec::new();
    let mut before: Option<char> = None;
    let mut at = 0;
    while let Some(c) = line[at..].chars().next() {
        let free = before.is_none_or(|b| !(is_word(b) || matches!(b, '/' | '#' | '-')));
        let hit = free
            .then(|| slugs.get(&c))
            .flatten()
            .and_then(|candidates| {
                candidates.iter().find(|slug| {
                    line[at..].starts_with(**slug)
                        && line[at + slug.len()..]
                            .chars()
                            .next()
                            .is_none_or(|n| !(is_word(n) || n == '-'))
                })
            });
        match hit {
            Some(slug) => {
                found.push(*slug);
                at += slug.len();
                before = slug.chars().last();
            }
            None => {
                at += c.len_utf8();
                before = Some(c);
            }
        }
    }
    found
}

/// Every card slug named in a document's prose. Registers, code blocks, `docs/history/` and a top
/// `docs/analysis/` are exempt: documents describe the design as it is now; why and when it was
/// decided live on the cards and in the ledger.
pub fn history_in_prose(root: &Path) -> Result<Vec<Finding>, ReviewError> {
    let docs = scan(root)?;
    let slugs: BTreeSet<&str> = docs
        .iter()
        .flat_map(|doc| doc.decisions.iter().map(|d| d.slug.as_str()))
        .collect();
    if slugs.is_empty() {
        return Ok(Vec::new());
    }
    // Python's order: sorted by the escaped slug, then the longest escaped slug first.
    let mut ordered: Vec<(String, &str)> = slugs
        .into_iter()
        .map(|slug| (re_escape(slug), slug))
        .collect();
    ordered.sort();
    ordered.sort_by_key(|(escaped, _)| std::cmp::Reverse(escaped.chars().count()));
    let mut by_first: HashMap<char, Vec<&str>> = HashMap::new();
    for (_, slug) in &ordered {
        if let Some(first) = slug.chars().next() {
            by_first.entry(first).or_default().push(slug);
        }
    }
    let mut found = Vec::new();
    for doc in &docs {
        if doc
            .path
            .split('/')
            .nth(1)
            .is_some_and(|part| PROSE_EXEMPT.contains(&part))
        {
            continue;
        }
        let path = root.join(&doc.path);
        let text = read_text(&path).map_err(|e| ReviewError(os_error(&e, &path)))?;
        for (number, line) in prose(&text) {
            found.extend(
                named_in(line, &by_first)
                    .into_iter()
                    .map(|slug| (doc.path.clone(), number, slug.to_string())),
            );
        }
    }
    Ok(found)
}

/// Every verdict word in a faction page's prose (a lowercase page; not the README or a hub): a
/// faction page shows the design as it is; which picture or trait was approved, rejected or
/// replaced is what its register records. Every other document may use these words for the review
/// process itself.
pub fn verdicts_on_pages(root: &Path) -> Vec<Finding> {
    let folder = root.join(DOCS_DIR).join(VERDICT_PAGES);
    let mut pages: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(&folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| {
                    (
                        entry.file_name().to_string_lossy().to_string(),
                        entry.path(),
                    )
                })
                .filter(|(name, path)| {
                    name.ends_with(".md")
                        && *name == name.to_lowercase()
                        && name != "README.md"
                        && path.is_file()
                })
                .collect()
        })
        .unwrap_or_default();
    pages.sort();
    let mut found = Vec::new();
    for (_, page) in pages {
        let Ok(text) = read_text(&page) else { continue };
        let path = relative(&page, root);
        for (number, line) in prose(&text) {
            for word in VERDICT.find_iter(line) {
                let before = line[..word.start()].chars().last();
                let after = line[word.end()..].chars().next();
                if before.is_none_or(|c| !is_word(c)) && after.is_none_or(|c| !is_word(c)) {
                    found.push((path.clone(), number, word.as_str().to_lowercase()));
                }
            }
        }
    }
    found
}

/// `symphony status --lint`: card slugs in prose and verdicts on faction pages, each named on `err`;
/// exit 1 on any, 2 when the docs cannot be read.
pub fn main(root: &Path, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let slugs = match history_in_prose(root) {
        Ok(slugs) => slugs,
        Err(problem) => {
            let _ = writeln!(err, "✖ concept review board could not run: {problem}");
            return 2;
        }
    };
    let verdicts = verdicts_on_pages(root);
    for (path, line, word) in slugs.iter().chain(&verdicts) {
        let _ = writeln!(err, "{path}:{line} {word}");
    }
    if !slugs.is_empty() {
        let _ = writeln!(
            err,
            "✖ {} card slug(s) in document prose. Say what the design IS; why and when belong on the card and in \
             docs/history/review-ledger.md.",
            slugs.len()
        );
    }
    if !verdicts.is_empty() {
        let _ = writeln!(
            err,
            "✖ {} verdict word(s) on a faction page. The page shows the design; what was approved or rejected is \
             its register's record.",
            verdicts.len()
        );
    }
    if !slugs.is_empty() || !verdicts.is_empty() {
        return 1;
    }
    let _ = writeln!(
        out,
        "✅ docs describe the current design (no card slugs in prose, no verdicts on faction pages)"
    );
    0
}
