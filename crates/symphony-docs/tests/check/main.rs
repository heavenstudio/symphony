//! Broken-reference fixture trees exercise the docs checks every Symphony project's build runs, each
//! naming its file and line. A fixture check claims the one component the trees use.
//!
//! `DOCS_FIXTURES=DIR cargo test -p symphony-docs --test check` keeps each test's tree under DIR,
//! named after the test, so the same tree can be put before another checker.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use symphony_docs::{DocCheck, Docs, Problem, check};

mod links;
mod lists;
mod prose;

const ROOT_README: &str = "# Docs

## Folders

- [alpha/](alpha/README.md) — the alpha topic.
- [kappa/](kappa/README.md) — the kappa topic.
- [data/](data/) — the data.
- [history/](history/README.md) — the past.
";
const ALPHA_README: &str = "# Alpha

What alpha holds: [ONE](ONE.md) and [two](two.md).

- [ONE.md](ONE.md) — the first.
- [two.md](two.md) — the second.
";
const HISTORY_README: &str =
    "# History\n\nThe past.\n\n| file | what |\n|---|---|\n| [old.md](old.md) | an old page |\n";

fn baseline() -> BTreeMap<&'static str, String> {
    let files: [(&str, &str); 10] = [
        ("docs/README.md", ROOT_README),
        ("docs/alpha/README.md", ALPHA_README),
        (
            "docs/alpha/ONE.md",
            "# One\n\n## Part\n\nText, see [two](two.md#second) and [[two#Second]].\n",
        ),
        ("docs/alpha/two.md", "# Two\n\n## Second\n\nText.\n"),
        (
            "docs/kappa/README.md",
            "# Kappa\n\nThe pages: [lambda](lambda.md), [notes](NOTES.md).\n",
        ),
        (
            "docs/kappa/lambda.md",
            "# Lambda\n\n### Keepers\n\n[[item:A1]]\n",
        ),
        ("docs/kappa/NOTES.md", "# Notes\n\nText.\n"),
        ("docs/data/items.json", "{}\n"),
        ("docs/history/README.md", HISTORY_README),
        (
            "docs/history/old.md",
            "# Old\n\nA [[gone]] link nobody checks.\n",
        ),
    ];
    files
        .into_iter()
        .map(|(path, text)| (path, text.to_string()))
        .collect()
}

/// A project's check claiming `[[item:…]]`, so a page may use it; it finds nothing.
struct Items;

impl DocCheck for Items {
    fn name(&self) -> &str {
        "items"
    }

    fn components(&self, _docs: &Docs) -> Vec<String> {
        vec!["item".into()]
    }

    fn check(&self, _docs: &Docs) -> Vec<Problem> {
        Vec::new()
    }
}

/// A project tree on disk: the baseline with `edits` applied (`None` removes a file), removed when dropped.
struct Tree {
    root: PathBuf,
    keep: bool,
}

static TREES: AtomicUsize = AtomicUsize::new(0);

impl Tree {
    fn new(name: &str, edits: &[(&'static str, Option<&str>)]) -> Tree {
        let mut files = baseline();
        for (path, text) in edits {
            match text {
                Some(text) => files.insert(path, text.to_string()),
                None => files.remove(path),
            };
        }
        let (root, keep) = match std::env::var_os("DOCS_FIXTURES") {
            Some(dir) => (PathBuf::from(dir).join(name), true),
            None => {
                let n = TREES.fetch_add(1, Ordering::SeqCst);
                (
                    std::env::temp_dir()
                        .join(format!("symphony-docs-{}-{n}-{name}", std::process::id())),
                    false,
                )
            }
        };
        let _ = std::fs::remove_dir_all(&root);
        for (path, text) in files {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        Tree { root, keep }
    }

    fn problems(&self) -> Vec<String> {
        match check(&self.root, &[&Items]) {
            Ok(_) => Vec::new(),
            Err(problems) => problems.iter().map(ToString::to_string).collect(),
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

fn problems(name: &str, edits: &[(&'static str, Option<&str>)]) -> Vec<String> {
    Tree::new(name, edits).problems()
}

#[track_caller]
fn has(found: &[String], parts: &[&str]) {
    assert!(
        found
            .iter()
            .any(|p| parts.iter().all(|part| p.contains(part))),
        "no problem holds {parts:?}; found {found:#?}"
    );
}

#[track_caller]
fn none_holds(found: &[String], part: &str) {
    assert!(
        found.iter().all(|p| !p.contains(part)),
        "a problem holds {part:?}: {found:#?}"
    );
}

fn alpha_readme_plus(row: &str) -> String {
    format!("{ALPHA_README}{row}")
}

// --- the baseline and today's docs ---

// --- the baseline ---

#[test]
fn a_consistent_tree_passes() {
    assert_eq!(problems("consistent", &[]), Vec::<String>::new());
}

#[test]
fn the_index_holds_documents_anchors_and_cards() {
    let card = "# Two\n\n## Second\n\nText.\n\n<!-- register:start -->\n- [x] **quiet-card** — A card.\n<!-- register:end -->\n";
    let tree = Tree::new("index", &[("docs/alpha/two.md", Some(card))]);
    let index = check(&tree.root, &[&Items]).expect("the baseline passes");
    let two = index
        .docs
        .iter()
        .find(|d| d.path == "docs/alpha/two.md")
        .expect("two.md is indexed");
    assert_eq!(two.anchors, ["second", "two"]);
    assert!(index.docs.iter().any(|d| d.path == "docs/history/old.md"));
    let quiet = index
        .cards
        .iter()
        .find(|c| c.slug == "quiet-card")
        .expect("the card is indexed");
    assert_eq!(
        (index.docs[quiet.doc].path.as_str(), quiet.line),
        ("docs/alpha/two.md", 8)
    );
    assert!(index.media.contains(&"docs/data/items.json".to_string()));
}
