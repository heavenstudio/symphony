//! The guides and the documents state their rules in plain words, never as the owner's words, dated
//! or quoted (RULES.md `docs/no-ledger-in-documents`): the words are kept in
//! `docs/history/owner-words.md` and the decision register, the records this check passes over. The
//! pattern is the one the published packages are read against (`tests/common/owner_words.rs`),
//! read across a line's break, as prose wraps an attribution onto the next line. It finds the
//! shapes that pattern names; any other is a reviewer's to read for.

#[path = "../common/owner_words.rs"]
mod words;

use std::path::{Path, PathBuf};
use std::process::Command;

/// What a guide is called, at the root or in a package.
const GUIDES: [&str; 3] = ["AGENTS.md", "README.md", "RULES.md"];

/// The records under `docs/` that keep the owner's words: the decision register and the history.
const RECORDS: [&str; 2] = ["docs/DECISIONS.md", "docs/history/"];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Whether the tracked file `path` is a guide or a document the rule holds: a guide anywhere, or
/// Markdown under `docs/`, the records aside, whatever their files are called.
fn held(path: &str) -> bool {
    if RECORDS.iter().any(|record| path.starts_with(record)) {
        return false;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    GUIDES.contains(&name) || (path.starts_with("docs/") && path.ends_with(".md"))
}

/// The tracked files of `root` the rule holds.
fn texts(root: &Path) -> Vec<String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "-z"])
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .split('\0')
        .filter(|path| held(path))
        .map(str::to_string)
        .collect()
}

/// Each line of `text` the owner's words start on, by its number from 1, with the words: a line is
/// read with the next one joined by a space, and a match is told at the line it starts on.
fn found(text: &str) -> Vec<(usize, String)> {
    let pattern = words::pattern();
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let here = line.trim_end();
        let next = lines.get(at + 1).map_or("", |next| next.trim_start());
        let read = format!("{here} {next}");
        if let Some(words) = pattern
            .find_iter(&read)
            .find(|words| words.start() < here.len())
        {
            found.push((at + 1, words.as_str().to_string()));
        }
    }
    found
}

#[test]
fn the_guides_and_documents_give_no_rule_as_the_owners_words() {
    let root = workspace();
    let texts = texts(&root);
    assert!(
        texts.iter().any(|path| path == "RULES.md") && texts.iter().any(|path| path == "AGENTS.md"),
        "the guides are read: {texts:?}"
    );
    let strays: Vec<String> = texts
        .iter()
        .flat_map(|path| {
            let text = std::fs::read_to_string(root.join(path)).unwrap_or_default();
            found(&text)
                .into_iter()
                .map(move |(line, words)| format!("{path}:{line}: {words}"))
        })
        .collect();
    assert!(
        strays.is_empty(),
        "the owner's words, dated or quoted, to say as the rule they set (the words themselves go \
         to docs/history/owner-words.md):\n{}",
        strays.join("\n")
    );
}

#[test]
fn the_guides_and_the_documents_are_held_and_the_records_are_not() {
    for path in [
        "RULES.md",
        "AGENTS.md",
        "README.md",
        "crates/tasks/AGENTS.md",
        "apps/symphony-web/AGENTS.md",
        "crates/symphony-api/README.md",
        "docs/README.md",
        "docs/workflow/TASKS.md",
        "docs/product/analysis/desktop-client.md",
    ] {
        assert!(held(path), "{path}");
    }
    for path in [
        "docs/DECISIONS.md",
        "docs/history/owner-words.md",
        "docs/history/README.md",
        "docs/product/analysis/owner-asks-board-asks.png",
        "crates/symphony/shipped/skills/work/SKILL.md",
        "crates/tasks/src/slots.rs",
        "apps/symphony-server/CRATE.md",
    ] {
        assert!(!held(path), "{path}");
    }
}

#[test]
fn an_attribution_is_found_dated_or_quoted_and_across_a_line_break() {
    for (text, line) in [
        ("The owner, 2026-11-11: \"do it\".", 1),
        ("a rule (the owner,\n2026-11-08: \"keep it\")", 1),
        ("the cap is the owner's of 2026-11-11: \"300\"", 1),
        ("reported beside the total (the owner's, 2026-11-11).", 1),
        (
            "one Dioxus app (the owner: \"switch to it\"), the browser now",
            1,
        ),
        ("That is the owner's \"stamps\" line.", 1),
        ("by the owner's word of 2026-11-12,", 1),
        ("prose first\nThe owner, 2026-11-13: \"a rerun\"", 2),
    ] {
        let got = found(text);
        assert_eq!(got.len(), 1, "{text:?}: {got:?}");
        assert_eq!(got[0].0, line, "{text:?}");
    }
}

#[test]
fn a_rule_in_plain_words_or_a_dated_measurement_is_not_the_owners_words() {
    for text in [
        "Only the owner approves a design document or a card.",
        "Read 2026-11-11: the owner's Mac is Apple silicon.",
        "Running it by hand is the owner's call, and an orchestrator does it only on the owner's word.",
        "`owner: \"o\"` names the recipient",
        "[2026-09-24] owner · approved — at least 21.",
        "the owner's asks landed on main at 09:28 on 2026-11-12",
        "the owner\n\n2026-11-11 is another paragraph's date",
    ] {
        assert_eq!(found(text), Vec::<(usize, String)>::new(), "{text:?}");
    }
}
