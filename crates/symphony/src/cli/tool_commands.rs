//! The two commands a project's art and landing tools consume. Their output contract and the
//! lock a lander shares with document saves are in docs/engineering/TOOL_COMMANDS.md.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{Flag, parse, refuse};
use crate::project::Project;

pub(super) fn main(name: &str, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let (usage, help, flags): (_, _, &[_]) = if name == "render-card" {
        (
            "usage: symphony render-card [-h] [--root ROOT] PATH SLUG",
            "Draw one card from PATH (a document route) with the project's plugins.\n\
             Prints JSON: path, slug, line, body and thread (typed blocks).",
            &[Flag {
                name: "--root",
                value: true,
            }],
        )
    } else {
        (
            "usage: symphony landing-checks [-h] [--root ROOT] [--base REF] BRANCH [PATH...]",
            "Check BRANCH using REF's committed project settings (default: main).\n\
             Prints JSON: base, tip, regenerates, documents, owner_lines_left, write_lock.\n\
             Documents classifies changed files and any extra PATHs (repository file paths).\n\
             Regeneration runs only the committed verdict generators, in a private worktree.",
            &[
                Flag {
                    name: "--root",
                    value: true,
                },
                Flag {
                    name: "--base",
                    value: true,
                },
            ],
        )
    };
    let parsed = match parse(args, flags) {
        Ok(parsed) if parsed.help => {
            let _ = writeln!(out, "{usage}\n\n{help}");
            return 0;
        }
        Ok(parsed) => parsed,
        Err(problem) => return refuse(err, usage, name, &problem),
    };
    let root = Path::new(parsed.value("--root").unwrap_or("."));
    let result = match (name, parsed.positionals.as_slice()) {
        ("render-card", [path, slug]) => render_card(root, path, slug),
        ("landing-checks", [branch, paths @ ..]) => landing_checks(
            root,
            parsed.value("--base").unwrap_or("main"),
            branch,
            paths,
        ),
        _ => return refuse(err, usage, name, "incorrect number of arguments"),
    };
    match result {
        Ok(value) => {
            let _ = writeln!(out, "{value}");
            0
        }
        Err(problem) => {
            let _ = writeln!(err, "symphony {name}: {problem}");
            1
        }
    }
}

fn render_card(root: &Path, path: &str, slug: &str) -> Result<Value, String> {
    let project = Project::load(root)?;
    let problems = crate::plugins::problems(&project);
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    let file = crate::docview::target(root, path, true)?;
    let text = crate::text::read_text(&file).map_err(|e| e.to_string())?;
    let (text, _) = crate::register::supersede(&text, &crate::clock::now_stamp());
    let lines = crate::text::splitlines(&text);
    let cards = crate::register::parse_decisions(&lines);
    let mut found = cards.iter().filter(|card| card.slug == slug);
    let card = found
        .next()
        .ok_or_else(|| format!("no card {slug:?} in {path}"))?;
    if found.next().is_some() {
        return Err(format!("more than one card {slug:?} in {path}"));
    }
    let end = crate::register::card_end(&lines, card.line - 1);
    let text = lines[card.line - 1..end].join("\n");
    let render = crate::render::Render::new(root);
    let mut sections = render.typed_sections(&text, path);
    let section = sections.pop().ok_or("card drew no section")?;
    let body = fragment(section.html, section.media, root, path);
    let thread = render
        .typed_history(&text, path)
        .remove(slug)
        .ok_or("card drew no thread")?;
    let thread = fragment(thread.html, thread.media, root, path);
    Ok(json!({"path": path, "slug": slug, "line": card.line, "body": body, "thread": thread}))
}

fn fragment(
    mut html: String,
    mut media: Vec<symphony_api::Media>,
    root: &Path,
    path: &str,
) -> Value {
    crate::docview::prepare_fragment(&mut html, &mut media, root, path, "");
    json!({"blocks": crate::blocks::parse(&html, &media, path)})
}

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let done = symphony_board::sys::output(crate::children::git().args(args).current_dir(root))
        .map_err(|e| e.to_string())?;
    if !done.status.success() {
        return Err(String::from_utf8_lossy(&done.stderr).trim().to_string());
    }
    Ok(done.stdout)
}

fn revision(root: &Path, name: &str) -> Result<String, String> {
    let bytes = git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{name}^{{commit}}"),
        ],
    )?;
    Ok(String::from_utf8_lossy(&bytes).trim().to_string())
}

fn landing_checks(
    root: &Path,
    base: &str,
    branch: &str,
    paths: &[String],
) -> Result<Value, String> {
    let base = revision(root, base)?;
    let tip = revision(root, branch)?;
    // The caller's checkout and the branch under judgment never widen the exemption. This is
    // the lander's existing plain-project fallback for absent or unreadable base settings.
    let mut project = git(root, &["show", &format!("{base}:{}", crate::project::FILE)])
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| Project::parse(root, &text).ok())
        .unwrap_or_else(|| Project::plain(root));
    project.default_branch.clone_from(&base);
    let common = git(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let common = PathBuf::from(String::from_utf8_lossy(&common).trim());
    let merge_base = git(root, &["merge-base", &base, &tip])?;
    let merge_base = String::from_utf8_lossy(&merge_base);
    let names = git(
        root,
        &[
            "diff",
            "--no-renames",
            "--name-only",
            "-z",
            merge_base.trim(),
            &tip,
            "--",
        ],
    )?;
    let mut documents = BTreeMap::new();
    for path in names.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let path = std::str::from_utf8(path).map_err(|_| "a changed path is not UTF-8")?;
        documents.insert(path.to_string(), crate::landing::is_doc(&project, path));
    }
    for path in paths {
        documents.insert(path.clone(), crate::landing::is_doc(&project, path));
    }
    let regenerates =
        crate::landing::regenerates(root, &tip, &project, &project.verdicts.generators);
    Ok(json!({
        "base": base,
        "tip": tip,
        "regenerates": regenerates,
        "documents": documents,
        "owner_lines_left": crate::landing::owner_lines_left(root),
        "write_lock": common.join(symphony_board::folders::SERVER).join("verdict-writes.lock"),
    }))
}
