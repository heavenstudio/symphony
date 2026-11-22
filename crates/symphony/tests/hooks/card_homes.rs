//! The decision register links each document that holds a card (docs/REVIEW.md, "Where a card is
//! written"): the list is kept by hand, so this holds it to what the documents hold, both ways.

use crate::support::repo as workspace;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use symphony::register::carded;

const REGISTER: &str = "docs/DECISIONS.md";

/// The Markdown files git tracks under `docs/` in `root`, by their path from it, the history
/// aside: a file not added yet is no document of the project's, as the other checks here read it.
fn documents(root: &Path) -> Vec<String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "-z", "--", "docs"])
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("git lists paths in UTF-8")
        .split('\0')
        .filter(|path| path.ends_with(".md") && !path.starts_with("docs/history/"))
        .map(str::to_string)
        .collect()
}

/// Whether `text` holds a card, wherever it sits, a fenced example aside.
fn holds_a_card(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    carded(&lines).contains(&true)
}

/// The documents the list under `heading` of `register` links, by their path from the root.
fn listed(register: &str, heading: &str) -> BTreeSet<String> {
    (register.lines())
        .skip_while(|line| *line != heading)
        .skip(1)
        .take_while(|line| !line.starts_with("## "))
        .filter_map(|line| line.strip_prefix("- ["))
        .filter_map(|line| Some(line.rsplit_once("](")?.1.strip_suffix(')')?.to_string()))
        .map(|target| format!("docs/{target}"))
        .collect()
}

#[test]
fn the_register_lists_each_document_that_holds_a_card_and_no_other() {
    let root = workspace();
    // A tracked file gone from the checkout holds no card there.
    let holding: BTreeSet<String> = (documents(&root).into_iter())
        .filter(|name| name != REGISTER)
        .filter(|name| holds_a_card(&std::fs::read_to_string(root.join(name)).unwrap_or_default()))
        .collect();
    let register = std::fs::read_to_string(root.join(REGISTER)).unwrap();
    let listed = listed(&register, "## Documents that hold cards");
    let unlisted: Vec<&String> = holding.difference(&listed).collect();
    let stale: Vec<&String> = listed.difference(&holding).collect();
    assert!(
        unlisted.is_empty() && stale.is_empty(),
        "{REGISTER}, \"Documents that hold cards\": add a line for {unlisted:?}, take out {stale:?}"
    );
}

#[test]
fn a_list_is_read_under_its_heading_alone_and_a_fenced_card_is_no_card() {
    let register = "# R\n\n- [Out](OUT.md)\n\n## Documents that hold cards\n\nWords.\n\n\
                    - [A](A.md)\n- [B (old)](analysis/b.md)\n\n## Next\n\n- [C](C.md)\n";
    assert_eq!(
        listed(register, "## Documents that hold cards")
            .into_iter()
            .collect::<Vec<_>>(),
        ["docs/A.md", "docs/analysis/b.md"]
    );
    assert!(holds_a_card(
        "Prose.\n\n- [ ] **lamp** — Twelve or twenty?\n"
    ));
    assert!(!holds_a_card(
        "```\n- [ ] **lamp** — An example.\n```\n- [ ] a task, no card\n"
    ));
}

#[test]
fn a_document_is_a_tracked_markdown_file_under_docs_the_history_aside() {
    let root = crate::scratch("card-homes");
    let git = |args: &[&str]| crate::beat_fixture::git(&root, args);
    git(&["init", "-q"]);
    for (path, text) in [
        ("docs/a.md", "- [ ] **lamp** — Twelve or twenty?\n"),
        ("docs/analysis/b.md", "Prose.\n"),
        ("docs/history/old.md", "- [x] **old** — Kept.\n"),
        ("docs/notes.txt", "Not Markdown.\n"),
        ("README.md", "Outside docs.\n"),
        ("docs/draft.md", "- [ ] **draft** — Not added.\n"),
    ] {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    git(&[
        "add",
        "docs/a.md",
        "docs/analysis/b.md",
        "docs/history/old.md",
        "docs/notes.txt",
        "README.md",
    ]);
    let mut found = documents(&root);
    found.sort();
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(found, ["docs/a.md", "docs/analysis/b.md"]);
}
