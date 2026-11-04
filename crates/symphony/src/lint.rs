//! The prose rules `symphony-status --lint` holds: a document says what the design is, not how it was
//! decided. The whole check is this file, which merge risk rates as a gate.

use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::board::scan;
use crate::register::{REGION_END, REGION_START, ReviewError, is_fence};
use crate::text::{is_word, os_error, read_text, splitlines, strip};

/// A word the lint found: (path, line, word).
pub type Finding = (String, usize, String);

/// The ledger and the evidence may say why and when: the first folder under `docs/` exempt.
const PROSE_EXEMPT: [&str; 2] = ["history", "analysis"];
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
    let project = crate::project::Project::of(root);
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
        let path = root.join(project.file(&doc.path));
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

/// Every verdict word in `text`'s prose, by 1-based line: what a page that shows the design as it
/// is never says (its register records which picture or trait was approved, rejected or replaced).
pub fn verdict_words(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (number, line) in prose(text) {
        for word in VERDICT.find_iter(line) {
            let before = line[..word.start()].chars().last();
            let after = line[word.end()..].chars().next();
            if before.is_none_or(|c| !is_word(c)) && after.is_none_or(|c| !is_word(c)) {
                found.push((number, word.as_str().to_lowercase()));
            }
        }
    }
    found
}

/// What the project's plugins' prose rules find in its documents: (path, line, what), with the
/// name of each rule that found something.
pub fn rule_findings(root: &Path) -> (Vec<Finding>, Vec<String>) {
    let plugins = crate::plugins::Plugins::of(root);
    let rules = plugins.prose_rules();
    let mut found = Vec::new();
    let mut names = Vec::new();
    if rules.is_empty() {
        return (found, names);
    }
    let project = crate::project::Project::of(root);
    for rel in crate::render::doc_paths(root) {
        let Ok(text) = read_text(&root.join(project.file(&rel))) else {
            continue;
        };
        for rule in &rules {
            let hits = rule.check(&rel, &text);
            if !hits.is_empty() && !names.iter().any(|name| name == rule.name()) {
                names.push(rule.name().to_string());
            }
            found.extend(
                hits.into_iter()
                    .map(|(line, what)| (rel.clone(), line, what)),
            );
        }
    }
    (found, names)
}

/// What the project's plugins' document checks find in it: each problem, naming its file, and the
/// name of each check that found one.
pub fn check_findings(root: &Path) -> (Vec<String>, Vec<String>) {
    let mut found = Vec::new();
    let mut names = Vec::new();
    for check in crate::plugins::Plugins::of(root).doc_checks() {
        let problems = check.check(root);
        if !problems.is_empty() {
            names.push(check.name().to_string());
        }
        found.extend(problems);
    }
    (found, names)
}

/// What is wrong with the project file at `root`: why it does not load, or each plugin it names
/// that this build does not hold.
pub fn project_problems(root: &Path) -> Vec<String> {
    match crate::project::Project::load(root) {
        Ok(project) => crate::plugins::problems(&project),
        Err(problem) => vec![problem],
    }
}

/// `symphony status --lint`: the project file, card slugs in prose, the plugins' prose rules and
/// their document checks, each finding named on `err`; exit 1 on any, 2 when the docs cannot be
/// read.
pub fn main(root: &Path, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let project = project_problems(root);
    if !project.is_empty() {
        for problem in &project {
            let _ = writeln!(err, "{problem}");
        }
        let _ = writeln!(
            err,
            "✖ the project file is refused, so the Symphony would use the defaults in its place."
        );
        return 1;
    }
    let slugs = match history_in_prose(root) {
        Ok(slugs) => slugs,
        Err(problem) => {
            let _ = writeln!(err, "✖ concept review board could not run: {problem}");
            return 2;
        }
    };
    let (verdicts, rules) = rule_findings(root);
    let (problems, checks) = check_findings(root);
    for (path, line, word) in slugs.iter().chain(&verdicts) {
        let _ = writeln!(err, "{path}:{line} {word}");
    }
    for problem in &problems {
        let _ = writeln!(err, "{problem}");
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
            "✖ {} prose problem(s): {}. A page shows the design; what was approved or rejected is its \
             register's record.",
            verdicts.len(),
            rules.join(", ")
        );
    }
    if !problems.is_empty() {
        let _ = writeln!(
            err,
            "✖ {} document check problem(s): {}.",
            problems.len(),
            checks.join(", ")
        );
    }
    if !slugs.is_empty() || !verdicts.is_empty() || !problems.is_empty() {
        return 1;
    }
    let _ = writeln!(
        out,
        "✅ docs describe the current design (no card slugs in prose, no verdicts on faction pages)"
    );
    0
}
