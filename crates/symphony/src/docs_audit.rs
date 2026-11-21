//! The render audit: every document and folder page uses the app's own renderer, each pending
//! picture card still shows the candidates it asks the owner to choose, and every open card's
//! pictures load ([`missing_pictures`]) and are files git tracks, so a clone shows them
//! (`card_pictures`). A link into the ignored artifacts folder, or to an owner's upload, that a
//! clone or this machine lacks is warned of (`artifacts`). In the Symphony's own checkout, every
//! `just NAME` its documents, what it delivers and its guides name is one the rule of its lists
//! allows (`recipes_named`, by the reader the hooks test shares); a consuming project's texts name
//! its own recipes and are not judged for them. Reference and structure checks are compilation
//! failures in docs-index, not duplicated here.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::cli::{Flag, parse, refuse};
use crate::markup::{dirname, join, normpath};
use crate::register::{
    CALLOUT, Decision, KINDS, RESOLVED, Who, card_line, is_fence, parse_decisions,
};
use crate::render::{Render, doc_paths, split_blocks};
use crate::text::{read_text, splitlines};

mod pictures;
mod recipes;

#[cfg(test)]
mod paths_tests;
#[cfg(test)]
mod recipes_tests;

static COMPONENT_ERROR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<span class="component-error"[^>]*>⚠ ([^<]*)</span>"#).unwrap());
static ASKED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?i:say) (?P<letters>[A-Z](?:, [A-Z])*,? or [A-Z])\b").unwrap()
});
static IMAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"!\[([^\]]*)\]\(([^)\s]+)\)").unwrap());
static BEFORE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[^A-Za-z])\*{0,2}(?P<letter>[A-Z])[:.)]?\*{0,2}\s*[:.)—–-]?\s*!\[").unwrap()
});
static LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"!?\[[^\]]*\]\((?P<target>[^)\s]+)(?:\s+"[^"]*")?\)"#).unwrap());
static CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`]*`").unwrap());
static SCHEME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[a-z][a-z0-9+.-]*:").unwrap());

const WARNINGS_WHY: &str = "/artifacts/ is gitignored, so a file there shows only on a machine that holds it, and only what git tracks is in a clone; each is a warning, not a failure";

#[derive(Default)]
pub struct Result {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

fn component_errors(body: &str, label: &str, result: &mut Result) {
    result.errors.extend(
        COMPONENT_ERROR
            .captures_iter(body)
            .map(|c| format!("{label}: does not render cleanly: {}", &c[1])),
    );
}

fn letters_a_picture(card: &Decision, wanted: &[String]) -> bool {
    let mut pictures = 0;
    for entry in card.entries() {
        if entry.who != Who::Agent || entry.text.starts_with("Base") {
            continue;
        }
        let found: Vec<_> = IMAGE.captures_iter(&entry.text).collect();
        pictures += found.len();
        if found.iter().any(|c| {
            wanted
                .iter()
                .any(|w| c[1].rsplit(' ').next() == Some(w.as_str()))
        }) || BEFORE
            .captures_iter(&entry.text)
            .any(|c| wanted.iter().any(|w| *w == c["letter"]))
        {
            return true;
        }
    }
    pictures >= 2
}

fn picture_cards(text: &str, rel: &str, result: &mut Result) {
    for card in parse_decisions(&splitlines(text))
        .iter()
        .filter(|c| c.state.as_str() == "pending")
    {
        let Some(asked) = ASKED.captures(&card.text) else {
            continue;
        };
        let wanted: Vec<String> = asked["letters"]
            .chars()
            .filter(char::is_ascii_uppercase)
            .map(|c| c.to_string())
            .collect();
        if !letters_a_picture(card, &wanted) {
            continue;
        }
        let (_, candidates) = crate::render::compare_pictures(card);
        let got: Vec<_> = candidates
            .into_iter()
            .map(|picture| picture.label)
            .collect();
        if got != wanted {
            let shown = got
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            result.errors.push(format!("{rel}:{}: picture card **{}** asks for {} but the Symphony shows {}: put one picture per letter on one agent line, each alt ending in its letter", card.line, card.slug, wanted.join(", "), if shown.is_empty() { "no lettered row" } else { &shown }));
        }
    }
}

fn undrawn_cards(text: &str, rel: &str, result: &mut Result) {
    for block in split_blocks(text) {
        let lines = splitlines(&block.text);
        let first = lines.first().copied().unwrap_or("");
        if card_line(first).is_some()
            || CALLOUT.captures(first).is_some_and(|c| {
                let kind = c["kind"].to_lowercase();
                KINDS.contains(&kind.as_str()) || kind == RESOLVED
            })
        {
            continue;
        }
        let mut fenced = false;
        for (offset, line) in lines.iter().enumerate() {
            if is_fence(line) {
                fenced = !fenced;
            }
            if !fenced && let Some(card) = card_line(line) {
                result.errors.push(format!("{rel}:{}: card **{}** is not drawn as a card: its block starts with another line; put a blank line above it", block.start + offset, card.slug.trim()));
            }
        }
    }
}

/// The path from the checkout's root that a link `target` in the document `rel` names, before the
/// project maps it to a file, its `%XX` escapes read; none for an outside URL.
fn resolved(rel: &str, target: &str) -> Option<String> {
    if SCHEME.is_match(target) {
        return None;
    }
    let path = target
        .split('#')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("");
    let path = unescaped(path);
    Some(normpath(&if target.starts_with('/') {
        path.trim_start_matches('/').into()
    } else {
        join(dirname(rel), &path)
    }))
}

/// `path` with its `%XX` escapes read, as the browser asks for the file; as written where they do
/// not read as UTF-8.
fn unescaped(path: &str) -> String {
    if !path.contains('%') {
        return path.to_string();
    }
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes
            .get(at + 1..at + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes[at], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                at += 3;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| path.to_string())
}

/// The path from the checkout's root that a picture `target` in the document `rel` names
/// ([`resolved`]), this server's own URL read as its route ([`pictures::local_route`]); none for an
/// outside URL, another project's, or a target that names no file (a bare `#anchor`).
fn picture_path(project: &crate::project::Project, rel: &str, target: &str) -> Option<String> {
    let routed = pictures::local_route(target, &project.id, |route| {
        project.serving(route).is_some()
            || names_upload(route)
            || project
                .file(route)
                .starts_with(&format!("{}/", project.artifacts))
    });
    let target = match routed {
        None => target.to_string(),
        Some(None) => return None,
        Some(Some(route)) => route,
    };
    let bare = target.split(['#', '?']).next().unwrap_or("");
    if bare.is_empty() {
        return None;
    }
    resolved(rel, &target)
}

/// Whether `resolved`, a link's path from the root ([`resolved`]), names an owner's upload: under
/// [`crate::uploads::ROUTE`], whatever the project's artifacts folder is called. It is also the
/// path a verdict commits the upload at, so git tracks it there or nowhere.
fn names_upload(resolved: &str) -> bool {
    resolved.starts_with(&crate::uploads::ROUTE[1..])
}

/// The files git tracks in the checkout at `root` (its index, so a picture added and not yet
/// committed counts), by their paths from `root`; none where git cannot say.
fn tracked_files(root: &Path) -> Option<HashSet<String>> {
    let listed = crate::branches::git(root, &["ls-files", "-z"])?;
    Some(
        listed
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// The pictures `card` shows in its text and its agents' lines, in every form
/// ([`pictures::pictures`], a reference read against the document's `definitions`), as written
/// there: what agents put on it, never the owner's words.
fn agent_pictures(card: &Decision, definitions: &pictures::Definitions) -> Vec<String> {
    let agents = card
        .entries()
        .into_iter()
        .filter(|entry| entry.who == Who::Agent)
        .map(|entry| entry.text);
    std::iter::once(card.text.clone())
        .chain(agents)
        .flat_map(|text| {
            pictures::pictures(&text, definitions)
                .into_iter()
                .map(|picture| picture.target)
        })
        .collect()
}

/// The pictures `card` in the document `rel` shows (its text and its agents' lines) that are not
/// on disk under the checkout at `root`: a picture the owner would see broken. Files the owner
/// attached are left out: they live in the main checkout's uploads, which every branch's page
/// shows. A reference picture counts only in the audit, which reads the whole document's
/// definitions.
pub fn missing_pictures(root: &Path, rel: &str, card: &Decision) -> Vec<String> {
    let project = crate::project::Project::of(root);
    missing_in(&project, root, rel, card, &pictures::Definitions::new())
        .into_iter()
        .map(|(target, _)| target)
        .collect()
}

/// The files of [`missing_pictures`], under `root`: what a card held back for them waits on.
pub fn missing_files(root: &Path, rel: &str, card: &Decision) -> Vec<std::path::PathBuf> {
    let project = crate::project::Project::of(root);
    missing_in(&project, root, rel, card, &pictures::Definitions::new())
        .into_iter()
        .map(|(_, file)| root.join(file))
        .collect()
}

/// [`missing_pictures`] with the checkout's project and the document's reference `definitions`
/// already read: each picture's link, and the file it names under the checkout.
fn missing_in(
    project: &crate::project::Project,
    root: &Path,
    rel: &str,
    card: &Decision,
    definitions: &pictures::Definitions,
) -> Vec<(String, String)> {
    let uploads = format!("{}/uploads/", project.artifacts);
    agent_pictures(card, definitions)
        .into_iter()
        .filter_map(|target| {
            let path = picture_path(project, rel, &target)?;
            let file = project.file(&path);
            (!names_upload(&path) && !file.starts_with(&uploads) && !root.join(&file).exists())
                .then_some((target, file))
        })
        .collect()
}

/// Whether the picture at `path` ([`picture_path`]) is not in a clone: not a file git tracks
/// (`tracked`) at the file the project maps it to, or an owner's upload, which a picture an agent
/// shows never is (a picture is committed beside its document, and attaching is for threads).
fn unclonable(project: &crate::project::Project, path: &str, tracked: &HashSet<String>) -> bool {
    names_upload(path) || !tracked.contains(&project.file(path))
}

/// The pictures `card` in the document `rel` shows (its text and its agents' lines) that a clone
/// lacks ([`unclonable`]), each once. Where they are refused (`on_disk`), only those on disk under
/// the checkout at `root`: one not on disk is [`missing_in`]'s.
fn untracked_in(
    project: &crate::project::Project,
    root: &Path,
    rel: &str,
    card: &Decision,
    tracked: &HashSet<String>,
    definitions: &pictures::Definitions,
    on_disk: bool,
) -> Vec<String> {
    let mut seen = HashSet::new();
    agent_pictures(card, definitions)
        .into_iter()
        .filter(|target| {
            seen.insert(target.clone())
                && picture_path(project, rel, target).is_some_and(|path| {
                    unclonable(project, &path, tracked)
                        && (!on_disk
                            || names_upload(&path)
                            || root.join(project.file(&path)).exists())
                })
        })
        .collect()
}

/// The line of the document that shows `target` on `card` (whose own line is `card.line`), in what
/// `written` holds of it: the card's own line for its text, else the first of its lines that
/// `written` kept and that shows `target` (a reference by its `definitions`).
fn line_of(
    card: &Decision,
    written: &Decision,
    target: &str,
    definitions: &pictures::Definitions,
) -> usize {
    let shows = |text: &str| {
        pictures::pictures(text, definitions)
            .iter()
            .any(|picture| picture.target == target)
    };
    if shows(&written.text) {
        return card.line;
    }
    card.raw
        .iter()
        .position(|line| shows(line) && written.raw.contains(line))
        .map_or(card.line, |at| card.line + 1 + at)
}

/// `first`, and how many more there are of `all`.
fn and_more(first: &str, all: usize) -> String {
    if all > 1 {
        format!("{first} and {} more", all - 1)
    } else {
        first.to_string()
    }
}

/// Each document of `docs` (by its route) as it stood where the checkout at `root` forked from
/// main, "" for one the fork did not hold; none where git could not say.
fn forked_documents(
    root: &Path,
    project: &crate::project::Project,
    docs: &[String],
) -> Option<std::collections::HashMap<String, String>> {
    let base = crate::branches::fork_base(root)?;
    let specs: Vec<String> = docs
        .iter()
        .map(|rel| format!("{base}:{}", project.file(rel)))
        .collect();
    let texts = crate::branches::blobs(root, &specs)?;
    Some(docs.iter().cloned().zip(texts).collect())
}

/// Whether the checkout at `root` is main itself: on the project's default branch, or detached at
/// its tip, as the release run's clean checkout is. Main has no fork of its own, so what its cards
/// show is warned of, never refused ([`card_pictures`]).
fn main_itself(root: &Path) -> bool {
    if crate::commit::on_main(root) {
        return true;
    }
    let git = |args: &[&str]| crate::branches::git(root, args);
    if git(&["symbolic-ref", "--quiet", "HEAD"]).is_some() {
        return false;
    }
    let main = format!(
        "refs/heads/{}^{{commit}}",
        crate::project::Project::of(root).default_branch
    );
    let head = git(&["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]);
    head.is_some() && head == git(&["rev-parse", "--verify", "--quiet", &main])
}

/// Whether the link resolved to `path` points into the checkout's ignored artifacts folder, an
/// owner's upload included: what [`artifacts`] warns of on every page that links it.
fn into_artifacts(project: &crate::project::Project, path: &str) -> bool {
    names_upload(path)
        || project
            .file(path)
            .starts_with(&format!("{}/", project.artifacts))
}

/// What the audit judges each document of the checkout at `root` by.
struct Judging<'a> {
    project: &'a crate::project::Project,
    root: &'a Path,
    /// What the registers held where the checkout forked from main.
    fork: Option<&'a crate::branches::Fork>,
    /// Whether what the checkout wrote that a clone lacks is refused: a branch with a fork from
    /// main. Main itself, or a checkout with no fork, is only warned.
    refuses: bool,
    /// The files git tracks; none where git cannot say, and nothing is judged for a clone.
    tracked: Option<&'a HashSet<String>>,
    /// Each document's text at the fork, by its route, for the lines a branch wrote of it; none
    /// where git could not say, and a document's own pictures are then only warned of.
    forked: Option<&'a std::collections::HashMap<String, String>>,
}

/// Whether the document `rel` is in a history folder, which the audit passes over.
fn in_history(project: &crate::project::Project, rel: &str) -> bool {
    project
        .serving(rel)
        .is_some_and(|(entry, inside)| entry.in_history(inside))
}

/// Each card in `text` whose pictures do not load (an open card's), or are not in a clone
/// ([`untracked_in`]; a closed card's too, as every past decision is shown), judged on what this
/// checkout wrote: the card's text and lines its fork from main did not hold
/// ([`crate::branches::Fork::written`]). A branch answering a card of main's is never refused for
/// main's pictures, which `/artifacts/` (each checkout's own) may not hold in its worktree, and a
/// verdict's branch, which writes only the owner's lines, never for an agent's. On main itself or
/// with no fork, the whole card is judged for what a clone lacks, on disk or not, and a picture is
/// warned of rather than refused, the ones in the artifacts folder left to the page's own warning
/// ([`artifacts`]).
fn card_pictures(
    judging: &Judging<'_>,
    rel: &str,
    text: &str,
    definitions: &pictures::Definitions,
    result: &mut Result,
) {
    let (project, root) = (judging.project, judging.root);
    if in_history(project, rel) {
        return;
    }
    let plugins = crate::plugins::Plugins::of(root);
    for card in parse_decisions(&splitlines(text)) {
        let closed = crate::branches::closed(&plugins, &card);
        let written = judging
            .fork
            .map_or_else(|| card.clone(), |fork| fork.written(&card));
        let missing = if closed {
            Vec::new()
        } else {
            missing_in(project, root, rel, &written, definitions)
        };
        if let Some((first, _)) = missing.first() {
            result.errors.push(format!(
                "{rel}:{}: card **{}** shows a picture this checkout wrote that does not load: {} is not on disk; add the file, or take the picture out, before committing",
                card.line,
                card.slug,
                and_more(first, missing.len())
            ));
        }
        let refused = judging.refuses;
        let judged = if refused { &written } else { &card };
        // A closed card's picture not on disk is no one else's to refuse.
        let on_disk = refused && !closed;
        let mut untracked = judging
            .tracked
            .map(|tracked| untracked_in(project, root, rel, judged, tracked, definitions, on_disk))
            .unwrap_or_default();
        if !refused {
            untracked.retain(|target| {
                !missing.iter().any(|(shown, _)| shown == target)
                    && picture_path(project, rel, target)
                        .is_some_and(|path| !into_artifacts(project, &path))
            });
        }
        if let Some(first) = untracked.first() {
            let line = line_of(&card, judged, first, definitions);
            let shown = and_more(first, untracked.len());
            if refused {
                result.errors.push(format!(
                    "{rel}:{line}: card **{}** shows a picture this checkout wrote that git does not track: {shown}; commit it beside its document, as a clone shows only what git tracks (an upload is for a thread, never a card)",
                    card.slug
                ));
            } else {
                result.warnings.push(format!(
                    "{rel}:{line}: card **{}** shows a picture git does not track: {shown}: not in a clone",
                    card.slug
                ));
            }
        }
    }
}

/// The lines of `lines` (from 0) that are not a document's own: each card's (its pictures are
/// [`card_pictures`]'), and each review callout the owner left with its body, an agent's nested
/// resolution aside.
fn not_the_documents(lines: &[&str]) -> HashSet<usize> {
    let mut skipped = HashSet::new();
    for card in parse_decisions(lines) {
        let first = card.line.saturating_sub(1);
        skipped.extend(first..=first + card.raw.len());
    }
    let (mut current, mut fenced) = (false, false);
    for (index, line) in lines.iter().enumerate() {
        if is_fence(line) {
            fenced = !fenced;
            current = false;
            continue;
        }
        if fenced {
            continue;
        }
        if crate::callouts::review_kind(line).is_some() {
            skipped.insert(index);
            current = true;
            continue;
        }
        if !current {
            continue;
        }
        match crate::register::QUOTE.captures(line) {
            None => current = false,
            Some(quote) if crate::register::NESTED_RESOLVED.is_match(&quote["rest"]) => {}
            Some(_) if CALLOUT.is_match(line) => current = false,
            Some(_) => {
                skipped.insert(index);
            }
        }
    }
    skipped
}

/// The lines of `lines` (from 0) that `base`, the document at the fork, did not hold as often:
/// what a branch wrote of it.
fn written_lines(base: &str, lines: &[&str]) -> HashSet<usize> {
    let mut held: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for line in splitlines(base) {
        *held.entry(line).or_default() += 1;
    }
    lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| match held.get_mut(line) {
            Some(count) if *count > 0 => {
                *count -= 1;
                None
            }
            _ => Some(index),
        })
        .collect()
}

/// The pictures the document `rel` shows of its own (outside its cards and the owner's callouts,
/// [`not_the_documents`]), in every form ([`pictures::pictures`]), that a clone lacks
/// ([`unclonable`]), on disk or not: a git clone shows a document's pictures, those the docs
/// reference above all. On a branch, those on lines it wrote (or
/// shown by a definition it wrote) are refused; on main itself or with no fork every one is warned
/// of, the ones in the artifacts folder left to the page's own warning ([`artifacts`]).
fn document_pictures(
    judging: &Judging<'_>,
    rel: &str,
    text: &str,
    definitions: &pictures::Definitions,
    result: &mut Result,
) {
    let project = judging.project;
    let Some(tracked) = judging.tracked else {
        return;
    };
    if in_history(project, rel) {
        return;
    }
    let lines = splitlines(text);
    let skipped = not_the_documents(&lines);
    let written = judging
        .refuses
        .then_some(judging.forked)
        .flatten()
        .map(|forked| written_lines(forked.get(rel).map_or("", String::as_str), &lines));
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for picture in pictures::pictures(text, definitions) {
        if skipped.contains(&picture.line) {
            continue;
        }
        if let Some(written) = &written
            && !written.contains(&picture.line)
            && !picture.defined.is_some_and(|at| written.contains(&at))
        {
            continue;
        }
        let Some(path) = picture_path(project, rel, &picture.target) else {
            continue;
        };
        if !unclonable(project, &path, tracked)
            || (written.is_none() && into_artifacts(project, &path))
        {
            continue;
        }
        if seen.insert(picture.target.clone()) {
            found.push((picture.line + 1, picture.target));
        }
    }
    let Some((line, first)) = found.first() else {
        return;
    };
    let shown = and_more(first, found.len());
    if written.is_some() {
        result.errors.push(format!(
            "{rel}:{line}: shows a picture this checkout wrote that git does not track: {shown}; commit it beside the document, as a clone shows only what git tracks (an upload is for a thread, never a document)"
        ));
    } else {
        result.warnings.push(format!(
            "{rel}:{line}: shows a picture git does not track: {shown}: not in a clone"
        ));
    }
}

/// Each link of `text`, the document `rel`, into the checkout's ignored artifacts folder that a
/// machine or a clone lacks, warned of and never refused: a file there that is not on disk ("not
/// on this machine"), or that is but git does not track (`tracked`), and an owner's upload git
/// does not track at the path its link names, wherever its store is ("not in a clone").
fn artifacts(
    root: &Path,
    rel: &str,
    text: &str,
    tracked: Option<&HashSet<String>>,
    result: &mut Result,
) {
    let project = crate::project::Project::of(root);
    if project
        .serving(rel)
        .is_some_and(|(entry, inside)| entry.in_history(inside))
    {
        return;
    }
    let untracked = |path: &str| tracked.is_some_and(|tracked| !tracked.contains(path));
    let folder = format!("{}/", project.artifacts);
    let mut absent = Vec::new();
    let mut unclonable = Vec::new();
    let mut fenced = false;
    for (index, line) in splitlines(text).iter().enumerate() {
        if is_fence(line) {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let masked = CODE.replace_all(line, |c: &regex::Captures<'_>| {
            "x".repeat(c[0].chars().count())
        });
        for link in LINK.captures_iter(&masked) {
            let target = &link["target"];
            let Some(path) = resolved(rel, target) else {
                continue;
            };
            let found = (index + 1, target.to_string());
            if names_upload(&path) {
                if untracked(&path) {
                    unclonable.push(found);
                }
                continue;
            }
            let file = project.file(&path);
            if !file.starts_with(&folder) {
                continue;
            }
            if !root.join(&file).exists() {
                absent.push(found);
            } else if untracked(&file) {
                unclonable.push(found);
            }
        }
    }
    for (found, why) in [
        (absent, "not on this machine"),
        (unclonable, "not in a clone, as git does not track it"),
    ] {
        if let Some((number, target)) = found.first() {
            result.warnings.push(format!(
                "{rel}:{number}: {}: {why}",
                and_more(target, found.len())
            ));
        }
    }
}

pub fn audit(root: &Path) -> Result {
    let mut result = Result::default();
    let renderer = Render::new(root);
    let mut docs = doc_paths(root);
    docs.sort();
    let project = crate::project::Project::of(root);
    let mut folders: Vec<String> = project.entrypoints.iter().map(|e| e.url.clone()).collect();
    for rel in &docs {
        let Some((entry, inside)) = project.serving(rel) else {
            continue;
        };
        let mut folder = Path::new(inside).parent();
        while let Some(path) = folder.filter(|p| !p.as_os_str().is_empty()) {
            folders.push(entry.route(&path.to_string_lossy()));
            folder = path.parent();
        }
    }
    folders.sort();
    folders.dedup();
    let fork = crate::branches::Fork::of(root);
    let refuses = fork.is_some() && !main_itself(root);
    let tracked = tracked_files(root);
    let forked = refuses
        .then(|| forked_documents(root, &project, &docs))
        .flatten();
    let judging = Judging {
        project: &project,
        root,
        fork: fork.as_ref(),
        refuses,
        tracked: tracked.as_ref(),
        forked: forked.as_ref(),
    };
    // The Symphony's own checkout, whose texts name only the recipes its lists allow: the documents
    // of them the walk reads, by their files.
    let symphony = project.id == recipes::PROJECT && root.join(recipes::LISTS).is_file();
    let mut naming = Vec::new();
    for folder in &folders {
        match renderer.folder(folder) {
            Ok(page) => {
                if let Some(intro) = page.intro {
                    for section in intro.sections {
                        component_errors(
                            &section.html,
                            &format!("{folder}: folder page"),
                            &mut result,
                        );
                    }
                }
            }
            Err(error) => result
                .errors
                .push(format!("{folder}: folder page: does not render: {error}")),
        }
    }
    for rel in &docs {
        let text = match read_text(&root.join(project.file(rel))) {
            Ok(text) => text,
            Err(error) => {
                result
                    .errors
                    .push(format!("{rel}: does not render: {error}"));
                continue;
            }
        };
        let readme = Path::new(rel)
            .file_name()
            .is_some_and(|name| name == "README.md");
        if !readme || !folders.iter().any(|folder| *folder == dirname(rel)) {
            match renderer.document(rel) {
                Ok(page) => {
                    for section in &page.sections {
                        component_errors(&section.html, rel, &mut result);
                    }
                }
                Err(error) => result
                    .errors
                    .push(format!("{rel}: does not render: {error}")),
            }
        }
        artifacts(root, rel, &text, tracked.as_ref(), &mut result);
        let definitions = pictures::definitions(&text);
        card_pictures(&judging, rel, &text, &definitions, &mut result);
        document_pictures(&judging, rel, &text, &definitions, &mut result);
        picture_cards(&text, rel, &mut result);
        undrawn_cards(&text, rel, &mut result);
        let file = project.file(rel);
        if symphony && recipes::audited(&file) {
            naming.push((file, 1, prose_alone(&text)));
        }
    }
    if symphony {
        result
            .errors
            .extend(recipes_named(root, tracked.as_ref(), naming));
    }
    result
}

/// `text` with each line of a record emptied, every other line where it was: a card's own lines,
/// wherever the card sits ([`crate::register::carded`]), and a register region's. A card says what
/// was proposed and decided, as the records under `docs/` do, never what to run now.
fn prose_alone(text: &str) -> String {
    use crate::register::{REGION_END, REGION_START, carded};
    let lines: Vec<&str> = text.split('\n').collect();
    let carded = carded(&lines);
    let mut region = false;
    let mut kept = Vec::with_capacity(lines.len());
    for (line, card) in lines.iter().zip(carded) {
        region |= line.trim() == REGION_START;
        kept.push(if region || card { "" } else { line });
        region &= line.trim() != REGION_END;
    }
    kept.join("\n")
}

/// The errors of the recipes the Symphony's own checkout at `root` names: in its documents (`texts`,
/// each a file, its first line and its text, as the walk read them) and, of the files git tracks
/// (`tracked`), what it delivers and its guides ([`recipes::beside_documents`]), judged by the rule
/// of its lists, with each entry of the lists for those files kept current
/// ([`recipes::Rule::errors`]).
fn recipes_named(
    root: &Path,
    tracked: Option<&HashSet<String>>,
    mut texts: Vec<(String, usize, String)>,
) -> Vec<String> {
    let read = |file: &str| {
        std::fs::read_to_string(root.join(file))
            .map_err(|error| vec![format!("{file}: does not read: {error}")])
    };
    let (lists, installed) = match (read(recipes::LISTS), read(recipes::INSTALLED)) {
        (Ok(lists), Ok(installed)) => (lists, installed),
        (Err(error), _) | (_, Err(error)) => return error,
    };
    let justfile = read("justfile").unwrap_or_default();
    let rule = match recipes::Rule::read(&lists, &installed, &justfile) {
        Ok(rule) => rule,
        Err(error) => return vec![error],
    };
    let mut beside: Vec<String> = (tracked.into_iter().flatten())
        .filter(|file| recipes::beside_documents(file) && !texts.iter().any(|(at, ..)| at == *file))
        .cloned()
        .collect();
    beside.sort();
    for file in beside {
        if let Ok(text) = read(&file) {
            texts.push((file, 1, text));
        }
    }
    rule.errors(&texts, recipes::audited)
}

pub fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    const USAGE: &str = "usage: symphony docs-audit [-h] [--root ROOT] [--check]";
    let flags = [
        Flag {
            name: "--root",
            value: true,
        },
        Flag {
            name: "--check",
            value: false,
        },
    ];
    let parsed = match parse(args, &flags) {
        Ok(p) if p.help => {
            let _ = writeln!(
                out,
                "{USAGE}\n\nCheck that every document renders and every waiting card can be answered; in the \
                 Symphony's own checkout, that every recipe its texts name is one tools/named-recipes.txt \
                 allows.\n\n\
                 options:\n  \
                 -h, --help   show this help message and exit\n  \
                 --root ROOT  the project's folder (default: here)\n  \
                 --check      the check, which runs without it too"
            );
            return 0;
        }
        Ok(p) if p.positionals.is_empty() => p,
        Ok(p) => {
            return refuse(
                err,
                USAGE,
                "docs-audit",
                &format!("unrecognized arguments: {}", p.positionals.join(" ")),
            );
        }
        Err(error) => return refuse(err, USAGE, "docs-audit", &error),
    };
    let root = PathBuf::from(parsed.value("--root").unwrap_or("."));
    let result = audit(&root);
    for warning in &result.warnings {
        let _ = writeln!(out, "⚠ {warning}");
    }
    if !result.warnings.is_empty() {
        let count = result.warnings.len();
        let plural = if count == 1 { "" } else { "s" };
        let _ = writeln!(
            out,
            "⚠ {count} warning{plural} of a file this machine or a clone lacks; {WARNINGS_WHY}"
        );
    }
    for error in &result.errors {
        let _ = writeln!(err, "✖ {error}");
    }
    if !result.errors.is_empty() {
        return 1;
    }
    let _ = writeln!(
        out,
        "docs audit: every document renders, and every card waiting on the owner can be answered"
    );
    0
}
