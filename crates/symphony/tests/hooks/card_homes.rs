//! The decision register links each document that holds a card (docs/workflow/REVIEW.md, "Where a card is
//! written"): the list is kept by hand, so this holds it to what the documents hold, both ways.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use symphony::register::carded;

const REGISTER: &str = "docs/DECISIONS.md";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// The Markdown files under `folder`, by their path from `root`, the history aside.
fn documents(root: &Path, folder: &Path, found: &mut Vec<String>) {
    for entry in std::fs::read_dir(folder).unwrap().flatten() {
        let path = entry.path();
        let name = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if path.is_dir() {
            if name != "docs/history" {
                documents(root, &path, found);
            }
        } else if name.ends_with(".md") {
            found.push(name);
        }
    }
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
    let mut all = Vec::new();
    documents(&root, &root.join("docs"), &mut all);
    let holding: BTreeSet<String> = (all.into_iter())
        .filter(|name| name != REGISTER)
        .filter(|name| holds_a_card(&std::fs::read_to_string(root.join(name)).unwrap()))
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
