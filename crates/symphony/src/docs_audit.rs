//! The render audit: every document and folder page uses the app's own renderer, each pending
//! picture card still shows the candidates it asks the owner to choose, and every open card's
//! pictures load ([`missing_pictures`]) and are files git tracks, so a clone shows them
//! (`card_pictures`). A link into the ignored artifacts folder, or to an owner's upload, that a
//! clone or this machine lacks is warned of (`artifacts`). Reference and structure checks are
//! compilation failures in docs-index, not duplicated here.

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
        let got: Vec<_> = candidates.into_iter().map(|(label, _)| label).collect();
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
/// project maps it to a file; none for an outside URL.
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
    Some(normpath(&if target.starts_with('/') {
        path.trim_start_matches('/').into()
    } else {
        join(dirname(rel), path)
    }))
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

/// The pictures `card` shows in its text and its agents' lines, as written there, code left out:
/// what agents put on it, never the owner's words.
fn agent_pictures(card: &Decision) -> Vec<String> {
    let agents = card
        .entries()
        .into_iter()
        .filter(|entry| entry.who == Who::Agent)
        .map(|entry| entry.text);
    std::iter::once(card.text.clone())
        .chain(agents)
        .flat_map(|text| {
            let prose = CODE.replace_all(&text, "");
            IMAGE
                .captures_iter(&prose)
                .map(|c| c[2].to_string())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The pictures `card` in the document `rel` shows (its text and its agents' lines) that are not
/// on disk under the checkout at `root`: a picture the owner would see broken. Files the owner
/// attached are left out: they live in the main checkout's uploads, which every branch's page
/// shows.
pub fn missing_pictures(root: &Path, rel: &str, card: &Decision) -> Vec<String> {
    missing_in(&crate::project::Project::of(root), root, rel, card)
        .into_iter()
        .map(|(target, _)| target)
        .collect()
}

/// The files of [`missing_pictures`], under `root`: what a card held back for them waits on.
pub fn missing_files(root: &Path, rel: &str, card: &Decision) -> Vec<std::path::PathBuf> {
    missing_in(&crate::project::Project::of(root), root, rel, card)
        .into_iter()
        .map(|(_, file)| root.join(file))
        .collect()
}

/// [`missing_pictures`] with the checkout's project already read: each picture's link, and the file
/// it names under the checkout.
fn missing_in(
    project: &crate::project::Project,
    root: &Path,
    rel: &str,
    card: &Decision,
) -> Vec<(String, String)> {
    let uploads = format!("{}/uploads/", project.artifacts);
    agent_pictures(card)
        .into_iter()
        .filter_map(|target| {
            let path = resolved(rel, &target)?;
            let file = project.file(&path);
            (!names_upload(&path) && !file.starts_with(&uploads) && !root.join(&file).exists())
                .then_some((target, file))
        })
        .collect()
}

/// The pictures `card` in the document `rel` shows (its text and its agents' lines) that are on
/// disk under the checkout at `root` but are not files git tracks (`tracked`), and every one that
/// names an owner's upload: a card's picture is committed beside its document, and attaching is
/// for threads. A clone shows none of them. A picture not on disk is [`missing_in`]'s.
fn untracked_in(
    project: &crate::project::Project,
    root: &Path,
    rel: &str,
    card: &Decision,
    tracked: &HashSet<String>,
) -> Vec<String> {
    let mut seen = HashSet::new();
    agent_pictures(card)
        .into_iter()
        .filter(|target| {
            seen.insert(target.clone())
                && resolved(rel, target).is_some_and(|path| {
                    let file = project.file(&path);
                    names_upload(&path) || (root.join(&file).exists() && !tracked.contains(&file))
                })
        })
        .collect()
}

/// The line of the document that shows `target` on `card` (whose own line is `card.line`), in what
/// `written` holds of it: the card's own line for its text, else the first of its lines that
/// `written` kept and that names `target`.
fn line_of(card: &Decision, written: &Decision, target: &str) -> usize {
    if written.text.contains(target) {
        return card.line;
    }
    card.raw
        .iter()
        .position(|line| line.contains(target) && written.raw.contains(line))
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

/// Whether the checkout at `root` is main itself: on the project's default branch, or detached at
/// its tip, as the nightly's clean checkout is. Main has no fork of its own, so what its cards show
/// is warned of, never refused ([`card_pictures`]).
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

/// Each open card in `text` (not resolved or rejected) whose pictures do not load, or are not files
/// git tracks (`tracked`, [`untracked_in`]), judged on what this checkout wrote: the card's text
/// and lines its fork from main did not hold ([`crate::branches::Fork::written`]). A branch
/// answering a card of main's is never refused for main's pictures, which `/artifacts/` (each
/// checkout's own) may not hold in its worktree, and a verdict's branch, which writes only the
/// owner's lines, never for an agent's. On main itself (`main`) or with no fork, the whole card is
/// judged for what git does not track, and a picture is warned of rather than refused, the ones
/// in the artifacts folder left to the page's own warning ([`artifacts`]).
fn card_pictures(
    root: &Path,
    rel: &str,
    text: &str,
    fork: Option<&crate::branches::Fork>,
    main: bool,
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
    let plugins = crate::plugins::Plugins::of(root);
    for card in parse_decisions(&splitlines(text)) {
        if crate::branches::closed(&plugins, &card) {
            continue;
        }
        let written = fork.map_or_else(|| card.clone(), |fork| fork.written(&card));
        let missing = missing_in(&project, root, rel, &written);
        if let Some((first, _)) = missing.first() {
            result.errors.push(format!(
                "{rel}:{}: card **{}** shows a picture this checkout wrote that does not load: {} is not on disk; add the file, or take the picture out, before committing",
                card.line,
                card.slug,
                and_more(first, missing.len())
            ));
        }
        let refused = fork.is_some() && !main;
        let judged = if refused { &written } else { &card };
        let mut untracked = tracked
            .map(|tracked| untracked_in(&project, root, rel, judged, tracked))
            .unwrap_or_default();
        if !refused {
            untracked.retain(|target| {
                resolved(rel, target).is_some_and(|path| !into_artifacts(&project, &path))
            });
        }
        if let Some(first) = untracked.first() {
            let line = line_of(&card, judged, first);
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
    let main = main_itself(root);
    let tracked = tracked_files(root);
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
        card_pictures(
            root,
            rel,
            &text,
            fork.as_ref(),
            main,
            tracked.as_ref(),
            &mut result,
        );
        picture_cards(&text, rel, &mut result);
        undrawn_cards(&text, rel, &mut result);
    }
    result
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
                "{USAGE}\n\nCheck that every document renders and every waiting card can be answered.\n\n\
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
