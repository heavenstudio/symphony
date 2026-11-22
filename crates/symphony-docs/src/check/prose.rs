//! Prose and components: a design document names no card slug outside its register, and every
//! `[[name:…]]` is the built-in `task` or one a [`DocCheck`] resolves.

use std::collections::HashSet;

use super::{DocCheck, Docs, LEDGER, Problem, TASK, problem};
use crate::syntax;

// --- prose: card slugs ---

/// The lines of a page outside its fences, its register regions and its cards' own lines
/// ([`syntax::carded`]), numbered from 1: the page's prose. A card inline in a document is its
/// record there, as a register's cards are.
pub fn prose_lines(text: &str) -> Vec<(usize, &str)> {
    let mut found = Vec::new();
    let (mut fenced, mut register) = (false, false);
    let lines = syntax::lines(text);
    let carded = syntax::carded(&lines);
    for (index, line) in lines.into_iter().enumerate() {
        let trimmed = line.trim_matches(syntax::is_space);
        if syntax::is_fence(line) {
            fenced = !fenced;
        } else if trimmed == syntax::REGION_START || trimmed == syntax::REGION_END {
            register = trimmed == syntax::REGION_START;
        } else if !fenced && !register && !carded[index] {
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
                found.extend(line.get(from..at).map(|run| (before, run)));
                start = None;
            }
            _ => {}
        }
        if start.is_none() {
            before = Some(c);
        }
    }
    if let Some(from) = start {
        found.extend(line.get(from..).map(|run| (before, run)));
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
        for (at, matched) in line.match_indices(slug) {
            let before = line.get(..at).and_then(|head| head.chars().next_back());
            let after = line
                .get(at + matched.len()..)
                .and_then(|tail| tail.chars().next());
            if !before.is_some_and(|c| word(c) || c == '/' || c == '#') && !after.is_some_and(word)
            {
                found.push((at, matched));
            }
        }
    }
    found.sort_unstable();
    found.into_iter().map(|(_, slug)| slug).collect()
}

pub(super) fn prose(tree: &Docs, problems: &mut Vec<Problem>) {
    let scanned: Vec<&String> = tree
        .documents
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
        let mut folders = rel.split('/').skip(1).collect::<Vec<_>>();
        folders.pop();
        if folders.iter().any(|p| matches!(*p, "history" | "analysis")) {
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
pub(super) fn unknown_components(
    tree: &Docs,
    checks: &[&dyn DocCheck],
    problems: &mut Vec<Problem>,
) {
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
