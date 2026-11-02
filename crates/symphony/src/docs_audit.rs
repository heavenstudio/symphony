//! The render audit: every document and folder page uses the app's own renderer, and each pending
//! picture card still shows the candidates it asks the owner to choose. Reference and structure
//! checks are compilation failures in docs-index, not duplicated here.

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

const ARTIFACTS_WHY: &str = "/artifacts/ is gitignored and exists only on the owner's machine, so a missing file is a warning, not a failure";

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

fn artifacts(root: &Path, rel: &str, text: &str, result: &mut Result) {
    if rel.starts_with("docs/history/") {
        return;
    }
    let mut absent = Vec::new();
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
            if SCHEME.is_match(target) {
                continue;
            }
            let path = target
                .split('#')
                .next()
                .unwrap_or("")
                .split('?')
                .next()
                .unwrap_or("");
            let resolved = normpath(&if target.starts_with('/') {
                path.trim_start_matches('/').into()
            } else {
                join(dirname(rel), path)
            });
            if resolved.starts_with("artifacts/") && !root.join(&resolved).exists() {
                absent.push((index + 1, target.to_string()));
            }
        }
    }
    if let Some((number, target)) = absent.first() {
        let more = if absent.len() > 1 {
            format!(" and {} more", absent.len() - 1)
        } else {
            String::new()
        };
        result.warnings.push(format!(
            "{rel}:{number}: {target}{more}: not on this machine"
        ));
    }
}

pub fn audit(root: &Path) -> Result {
    let mut result = Result::default();
    let renderer = Render::new(root);
    let mut docs = doc_paths(root);
    docs.sort();
    let mut folders = vec!["docs".to_string()];
    for rel in &docs {
        let mut folder = Path::new(rel).parent();
        while let Some(path) = folder.filter(|p| p.starts_with("docs") && *p != Path::new("docs")) {
            if !path
                .components()
                .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
            {
                folders.push(path.to_string_lossy().into_owned());
            }
            folder = path.parent();
        }
    }
    folders.sort();
    folders.dedup();
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
        let text = match read_text(&root.join(rel)) {
            Ok(text) => text,
            Err(error) => {
                result
                    .errors
                    .push(format!("{rel}: does not render: {error}"));
                continue;
            }
        };
        if !rel.ends_with("/README.md") || !folders.iter().any(|folder| *folder == dirname(rel)) {
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
        artifacts(root, rel, &text, &mut result);
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
                "{USAGE}\n\nCheck that every document renders and every waiting card can be answered."
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
    if !parsed.flag("--check") {
        let _ = writeln!(out, "{USAGE}");
        return 2;
    }
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
            "⚠ {count} page{plural} cite an /artifacts/ file this machine lacks; {ARTIFACTS_WHY}"
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
