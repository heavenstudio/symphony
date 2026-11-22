//! The prose rules `symphony-status --lint` holds: a document says what the design is, not how it was
//! decided. The whole check is this file, which merge risk rates as a gate.

use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::path::Path;

use crate::board::scan;
use crate::register::{REGION_END, REGION_START, ReviewError, is_fence};
use crate::text::{is_word, os_error, read_text, splitlines, strip};

/// A word the lint found: (path, line, word).
pub type Finding = (String, usize, String);

/// The ledger and the evidence may say why and when: a folder of either name is exempt, at any
/// depth under the entrypoint (`docs/workflow/analysis/`).
const PROSE_EXEMPT: [&str; 2] = ["history", "analysis"];

/// Whether the document at `path` is in a folder the prose rules pass over.
fn exempt(path: &str) -> bool {
    let mut folders = path.split('/').skip(1).collect::<Vec<_>>();
    folders.pop();
    folders.iter().any(|part| PROSE_EXEMPT.contains(part))
}

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

/// The lines of a document outside its code fences, its register region and its cards' own lines
/// wherever they sit ([`crate::register::carded`]), numbered from 1. A region marker inside a fence
/// still counts.
fn prose(text: &str) -> Vec<(usize, &str)> {
    let (mut in_register, mut in_fence) = (false, false);
    let mut found = Vec::new();
    let lines = splitlines(text);
    let carded = crate::register::carded(&lines);
    for (index, line) in lines.into_iter().enumerate() {
        if is_fence(line) {
            in_fence = !in_fence;
        } else if strip(line) == REGION_START || strip(line) == REGION_END {
            in_register = strip(line) == REGION_START;
        } else if !in_fence && !in_register && !carded[index] {
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

/// Every card slug named in a document's prose. Registers, a card's own lines, code blocks, `docs/history/` and a top
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
        if exempt(&doc.path) {
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

/// Each card slug more than one card of the project's documents carries, with where each is
/// (`path:line`), sorted by slug. One name is one card: a verdict saved on a page goes to the first
/// card of that name there, a task waiting on `decision:NAME` ends on either's answer, and a card
/// named in bold opens the first.
pub fn repeated_slugs(root: &Path) -> Result<Vec<(String, Vec<String>)>, ReviewError> {
    let mut places: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for doc in scan(root)? {
        for card in &doc.decisions {
            places
                .entry(card.slug.clone())
                .or_default()
                .push(format!("{}:{}", doc.path, card.line));
        }
    }
    places.retain(|_, held| held.len() > 1);
    Ok(places.into_iter().collect())
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
    let checks = crate::plugins::Plugins::of(root);
    let checks = checks.doc_checks();
    if checks.is_empty() {
        return (found, names);
    }
    let docs = symphony_plugin::Docs::read(root);
    for check in checks {
        let problems = check.check(&docs);
        if !problems.is_empty() {
            names.push(check.name().to_string());
        }
        found.extend(problems.iter().map(ToString::to_string));
    }
    (found, names)
}

/// What is wrong with the project file at `root`: why it does not load, or each plugin it names
/// that this build does not hold; and with its local settings file
/// ([`crate::project::Project::local_problems`]).
pub fn project_problems(root: &Path) -> Vec<String> {
    let mut problems = match crate::project::Project::load(root) {
        Ok(project) => crate::plugins::problems(&project),
        Err(problem) => vec![problem],
    };
    problems.extend(crate::project::Project::local_problems(root));
    problems
}

/// `symphony settings-check [DIR]`, the settings check: the project file at DIR (the current
/// folder) read as the Symphony reads it ([`crate::project::Project::load`]), judged on its shape
/// alone. A table or key the Symphony does not know, a value of the wrong type or one its key's
/// rule refuses (an agent no launch starts among them), or a required value left out is named on
/// `err`, exit 1; a file that loads is said on `out`, exit 0. It judges no model or recipe against a
/// list (either is any text) and, unlike the lint, reads no plugin this build holds and no document.
/// Recognizable cross-agent model names warn
/// without changing the exit code, judged on the settings in force, the local file's over the
/// project file's. The local settings file beside the main checkout's project
/// file is judged with it ([`crate::project::Project::local_problems`]): a setting it may not
/// hold, a value the board cannot read, or the file tracked by git, is exit 1. More than one
/// folder is exit 2.
pub fn settings(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    const USAGE: &str = "usage: symphony settings-check [DIR]";
    let root = match args {
        [] => Path::new("."),
        [dir] if !dir.starts_with('-') => Path::new(dir),
        _ => {
            return crate::cli::refuse(
                err,
                USAGE,
                "settings-check",
                &format!("unrecognized arguments: {}", args.join(" ")),
            );
        }
    };
    let loaded = crate::project::Project::load(root);
    let local = crate::project::Project::local_problems(root);
    if let Err(problem) = &loaded {
        let _ = writeln!(err, "{problem}");
        let _ = writeln!(err, "✖ the project file is refused.");
    } else {
        // The settings in force: the local file's over the project file's, unless it is refused,
        // when none of it is read.
        let file = root.join(crate::project::FILE);
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let over = crate::project::Project::local(root).ok().flatten();
        crate::project::warn_models(&text, &file, over.as_ref(), err);
    }
    for problem in &local {
        let _ = writeln!(err, "{problem}");
    }
    if !local.is_empty() {
        let _ = writeln!(err, "✖ the local settings file is refused.");
    }
    if loaded.is_err() || !local.is_empty() {
        return 1;
    }
    let _ = writeln!(
        out,
        "✔ {}: every table and key is a setting the Symphony knows, each value of its type",
        crate::project::FILE
    );
    if crate::project::Project::local_file(root).is_file() {
        let _ = writeln!(
            out,
            "✔ {}: every key is a preference a local file may override, each value of its type",
            crate::project::LOCAL_FILE
        );
    }
    0
}

/// `symphony status --lint`: the project file, card slugs in prose, a card name more than one card
/// carries, the plugins' prose rules and their document checks, each finding named on `err`; exit 1 on any, 2 when the docs cannot be
/// read.
pub fn main(root: &Path, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let project = project_problems(root);
    if !project.is_empty() {
        for problem in &project {
            let _ = writeln!(err, "{problem}");
        }
        let _ = writeln!(
            err,
            "✖ the project file is refused, so the Symphony runs with what it does not know left out, or with the defaults in its place when it does not load."
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
    // `history_in_prose` read these documents a moment ago: they read.
    let repeated = repeated_slugs(root).unwrap_or_default();
    for (slug, places) in &repeated {
        let _ = writeln!(err, "{}: card {slug} again", places.join(", "));
    }
    if !repeated.is_empty() {
        let _ = writeln!(
            err,
            "✖ {} card name(s) used by more than one card. One name is one card, wherever it sits: rename the newer.",
            repeated.len()
        );
    }
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
    if !slugs.is_empty() || !repeated.is_empty() || !verdicts.is_empty() || !problems.is_empty() {
        return 1;
    }
    let _ = writeln!(
        out,
        "✅ docs describe the current design (no card slugs in prose, each card name one card, nothing the plugins' rules and checks refuse)"
    );
    0
}
