//! Broken-reference fixture trees exercise the docs checks every Symphony project's build runs, each
//! naming its file and line. A fixture check claims the one component the trees use.
//!
//! `DOCS_FIXTURES=DIR cargo test -p symphony-docs --test check` keeps each test's tree under DIR,
//! named after the test, so the same tree can be put before another checker.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use symphony_docs::{DocCheck, Docs, Problem, check};

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

// --- reachable through READMEs ---

#[test]
fn a_page_nothing_links_to_is_unreachable() {
    let found = problems(
        "unreachable",
        &[("docs/alpha/three.md", Some("# Three\n\nAlone.\n"))],
    );
    has(
        &found,
        &[
            "docs/alpha/three.md",
            "not reachable",
            "docs/alpha/README.md must link it",
        ],
    );
}

#[test]
fn a_page_linked_only_by_a_sibling_wikilink_is_unreachable() {
    let found = problems(
        "sibling-wikilink",
        &[
            ("docs/alpha/three.md", Some("# Three\n\nAlone.\n")),
            (
                "docs/alpha/two.md",
                Some("# Two\n\n## Second\n\nSee [[three]].\n"),
            ),
        ],
    );
    has(&found, &["docs/alpha/three.md", "not reachable"]);
}

#[test]
fn a_page_linked_only_by_a_sibling_relative_link_is_unreachable() {
    let found = problems(
        "sibling-link",
        &[
            ("docs/alpha/three.md", Some("# Three\n\nAlone.\n")),
            (
                "docs/alpha/two.md",
                Some("# Two\n\n## Second\n\nSee [three](three.md).\n"),
            ),
        ],
    );
    has(&found, &["docs/alpha/three.md", "not reachable"]);
}

#[test]
fn a_page_a_prose_readme_links_is_reachable() {
    let readme = "# Alpha\n\nProse: [ONE](ONE.md), [two](two.md) and [three](three.md).\n";
    let found = problems(
        "prose-readme",
        &[
            ("docs/alpha/three.md", Some("# Three\n\nHere.\n")),
            ("docs/alpha/README.md", Some(readme)),
        ],
    );
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn a_page_a_prose_readme_only_names_is_unreachable() {
    let readme = "# Alpha\n\nProse: [ONE](ONE.md), [two](two.md); three.md is named, not linked.\n";
    let found = problems(
        "prose-names",
        &[
            ("docs/alpha/three.md", Some("# Three\n\nHere.\n")),
            ("docs/alpha/README.md", Some(readme)),
        ],
    );
    has(&found, &["docs/alpha/three.md", "docs/alpha/README.md"]);
}

#[test]
fn a_folder_reached_through_its_parent_readme_is_reachable() {
    let readme = alpha_readme_plus("- [deep/](deep/README.md) — below.\n");
    let found = problems(
        "deep-reached",
        &[
            ("docs/alpha/deep/README.md", Some("# Deep\n\nHere.\n")),
            ("docs/alpha/README.md", Some(&readme)),
        ],
    );
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn a_folder_the_parent_never_links_is_unreachable() {
    let found = problems(
        "deep-unlinked",
        &[("docs/alpha/deep/README.md", Some("# Deep\n\nHere.\n"))],
    );
    has(&found, &["docs/alpha/deep/README.md", "not reachable"]);
}

#[test]
fn a_page_in_an_unreached_folder_is_unreachable_even_if_a_reached_page_links_it() {
    let found = problems(
        "deep-page",
        &[
            ("docs/alpha/deep/README.md", Some("# Deep\n\n[x](x.md)\n")),
            ("docs/alpha/deep/x.md", Some("# X\n\nHere.\n")),
            (
                "docs/alpha/ONE.md",
                Some("# One\n\n## Part\n\n[x](deep/x.md)\n"),
            ),
        ],
    );
    has(&found, &["docs/alpha/deep/x.md", "not reachable"]);
}

#[test]
fn a_readme_reaches_only_its_own_folder_and_its_subfolders_readmes() {
    let root =
        format!("{ROOT_README}\nSee [x](alpha/deep/x.md) and [deep](alpha/deep/README.md).\n");
    let found = problems(
        "deep-skipped",
        &[
            ("docs/README.md", Some(&root)),
            ("docs/alpha/deep/README.md", Some("# Deep\n\n[x](x.md)\n")),
            ("docs/alpha/deep/x.md", Some("# X\n\nHere.\n")),
        ],
    );
    has(
        &found,
        &[
            "docs/alpha/deep/README.md",
            "not reachable",
            "docs/alpha/README.md must link it",
        ],
    );
    has(
        &found,
        &[
            "docs/alpha/deep/x.md",
            "not reachable",
            "docs/alpha/deep/README.md must link it",
        ],
    );
}

#[test]
fn a_readme_reaches_a_subfolders_readme_but_not_its_pages() {
    let root = format!("{ROOT_README}\nSee [three](alpha/three.md).\n");
    let found = problems(
        "page-skipped",
        &[
            ("docs/README.md", Some(&root)),
            ("docs/alpha/three.md", Some("# Three\n\nHere.\n")),
        ],
    );
    has(
        &found,
        &[
            "docs/alpha/three.md",
            "not reachable",
            "docs/alpha/README.md must link it",
        ],
    );
}

#[test]
fn a_missing_root_readme_reaches_nothing() {
    let found = problems("no-root-readme", &[("docs/README.md", None)]);
    has(&found, &["docs/README.md", "missing"]);
}

// --- the Folders list ---

#[test]
fn a_folder_missing_from_the_folders_list() {
    let found = problems(
        "folders-missing",
        &[("docs/beta/README.md", Some("# Beta\n\nNew.\n"))],
    );
    has(&found, &["docs/README.md", "beta/", "missing"]);
}

#[test]
fn a_stale_folders_entry() {
    let readme = format!("{ROOT_README}- [gamma/](gamma/README.md) — gone.\n");
    let found = problems("folders-stale", &[("docs/README.md", Some(&readme))]);
    has(&found, &["docs/README.md", "gamma/", "no such folder"]);
}

#[test]
fn a_folder_with_no_documents_still_must_be_listed() {
    let found = problems(
        "folders-data-only",
        &[("docs/zeta/schema.json", Some("{}"))],
    );
    has(&found, &["docs/README.md", "zeta/", "missing"]);
}

#[test]
fn hidden_folders_are_not_folders() {
    assert_eq!(
        problems("folders-hidden", &[("docs/.cache/x.json", Some("{}"))]),
        Vec::<String>::new()
    );
}

// --- list READMEs ---

#[test]
fn a_page_missing_from_its_list_readme() {
    let found = problems(
        "guide-missing-page",
        &[
            ("docs/alpha/three.md", Some("# Three\n\nSee [[ONE]].\n")),
            ("docs/alpha/ONE.md", Some("# One\n\n## Part\n\n[[three]]\n")),
        ],
    );
    has(&found, &["docs/alpha/README.md", "does not list three.md"]);
}

#[test]
fn a_dead_entry_in_a_list_readme() {
    let readme = alpha_readme_plus("- [ghost.md](ghost.md) — gone.\n");
    let found = problems("guide-dead", &[("docs/alpha/README.md", Some(&readme))]);
    has(
        &found,
        &[
            "docs/alpha/README.md:7",
            "lists ghost.md, which does not exist",
        ],
    );
}

#[test]
fn a_subfolder_missing_from_a_list_readme() {
    let found = problems(
        "guide-missing-folder",
        &[("docs/alpha/deep/README.md", Some("# Deep\n\nHere.\n"))],
    );
    has(&found, &["docs/alpha/README.md", "does not list deep/"]);
}

#[test]
fn a_table_readme_is_a_list() {
    let found = problems(
        "guide-table",
        &[("docs/history/new.md", Some("# New\n\nRecent.\n"))],
    );
    has(&found, &["docs/history/README.md", "does not list new.md"]);
}

#[test]
fn a_prose_readme_is_not_forced_into_a_list() {
    let readme = "# Alpha\n\nProse only: [ONE](ONE.md) and [two](two.md) say what is here.\n";
    assert_eq!(
        problems("guide-prose", &[("docs/alpha/README.md", Some(readme))]),
        Vec::<String>::new()
    );
}

#[test]
fn a_link_leaving_the_folder_is_not_an_entry() {
    let readme = alpha_readme_plus(
        "\n| you want | read |\n|---|---|\n| the past | [old](../history/old.md) |\n",
    );
    assert_eq!(
        problems("guide-leaving", &[("docs/alpha/README.md", Some(&readme))]),
        Vec::<String>::new()
    );
}

#[test]
fn only_a_rows_first_link_is_its_entry() {
    let readme = "# Alpha\n\n| what | read |\n|---|---|\n| the past | [old](../history/old.md), [ONE](ONE.md) |\n\nSee [two](two.md).\n";
    assert_eq!(
        problems(
            "guide-first-link",
            &[("docs/alpha/README.md", Some(readme))]
        ),
        Vec::<String>::new()
    );
}

#[test]
fn a_readme_whose_every_row_is_dead_is_still_a_list() {
    let readme = alpha_readme_plus("- [foo/](foo/README.md) — below.\n");
    let found = problems(
        "guide-all-dead",
        &[
            (
                "docs/alpha/foo/README.md",
                Some("# Foo\n\n- [a.md](a.md) — gone.\n"),
            ),
            ("docs/alpha/foo/b.md", Some("# B\n\nHere.\n")),
            ("docs/alpha/README.md", Some(&readme)),
        ],
    );
    has(
        &found,
        &[
            "docs/alpha/foo/README.md:3",
            "lists a.md, which does not exist",
        ],
    );
    has(&found, &["docs/alpha/foo/README.md", "does not list b.md"]);
}

// --- links ---

#[test]
fn a_document_link_to_a_removed_heading() {
    let found = problems(
        "wikilink-heading",
        &[(
            "docs/alpha/ONE.md",
            Some("# One\n\n## Part\n\nSee [[two#Nowhere]].\n"),
        )],
    );
    has(
        &found,
        &[
            "docs/alpha/ONE.md:5",
            "[[two#Nowhere]]",
            "no heading 'Nowhere' in docs/alpha/two.md",
        ],
    );
}

#[test]
fn a_document_link_to_no_document() {
    let found = problems(
        "wikilink-name",
        &[(
            "docs/alpha/ONE.md",
            Some("# One\n\n## Part\n\nSee [[nothing]] and [two](two.md).\n"),
        )],
    );
    has(
        &found,
        &["docs/alpha/ONE.md:5", "no document called nothing"],
    );
}

#[test]
fn a_document_link_finds_its_document_in_another_folder() {
    let one = "# One\n\n## Part\n\nSee [[lambda#Keepers]] and [[kappa/NOTES]].\n";
    assert_eq!(
        problems("wikilink-elsewhere", &[("docs/alpha/ONE.md", Some(one))]),
        Vec::<String>::new()
    );
}

#[test]
fn a_relative_link_to_a_missing_file() {
    let found = problems(
        "link-missing",
        &[(
            "docs/alpha/ONE.md",
            Some("# One\n\n## Part\n\nSee [x](missing.md) and [two](two.md).\n"),
        )],
    );
    has(
        &found,
        &["docs/alpha/ONE.md:5", "[missing.md]: no such file"],
    );
}

#[test]
fn a_relative_link_with_a_missing_heading() {
    let found = problems(
        "link-heading",
        &[(
            "docs/alpha/ONE.md",
            Some("# One\n\n## Part\n\nSee [x](two.md#third) and [two](two.md).\n"),
        )],
    );
    has(
        &found,
        &[
            "docs/alpha/ONE.md:5",
            "no heading 'third' in docs/alpha/two.md",
        ],
    );
}

#[test]
fn a_link_to_a_folder_resolves_through_its_readme() {
    let one = "# One\n\n## Part\n\nSee [home](../) and [two](two.md) and [h](../history/) and [d](../data/#x).\n";
    assert_eq!(
        problems("link-folder", &[("docs/alpha/ONE.md", Some(one))]),
        Vec::<String>::new()
    );
}

#[test]
fn links_in_code_are_not_links() {
    let one = "# One\n\n## Part\n\n`[x](missing.md)` and [two](two.md).\n\n```\n[y](nope.md) [[nope]]\n```\n";
    assert_eq!(
        problems("link-code", &[("docs/alpha/ONE.md", Some(one))]),
        Vec::<String>::new()
    );
}

#[test]
fn a_link_whose_text_is_code_is_a_link() {
    let found = problems(
        "link-code-text",
        &[(
            "docs/alpha/ONE.md",
            Some("# One\n\n## Part\n\nSee [`x`](missing.md) and [two](two.md).\n"),
        )],
    );
    has(&found, &["docs/alpha/ONE.md:5", "missing.md"]);
}

#[test]
fn external_links_are_left_alone() {
    let one =
        "# One\n\n## Part\n\n[web](https://example.com/a.md) [mail](mailto:x@y.z) [two](two.md)\n";
    assert_eq!(
        problems("link-external", &[("docs/alpha/ONE.md", Some(one))]),
        Vec::<String>::new()
    );
}

#[test]
fn history_links_are_not_checked() {
    let found = problems(
        "link-history",
        &[(
            "docs/history/old.md",
            Some("# Old\n\n[x](missing.md) [[gone#Nope]] [[item:GONE-01]]\n"),
        )],
    );
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn an_artifacts_link_is_not_the_builds_to_judge() {
    let one = "# One\n\n## Part\n\n![pic](/artifacts/a/b.png) [rel](../../artifacts/c.png) [two](two.md)\n";
    assert_eq!(
        problems("link-artifacts", &[("docs/alpha/ONE.md", Some(one))]),
        Vec::<String>::new()
    );
}

#[test]
fn a_root_link_to_a_missing_file() {
    let found = problems(
        "root-missing",
        &[(
            "docs/alpha/ONE.md",
            Some("# One\n\n## Part\n\nSee [x](/docs/alpha/NOPE.md) and [two](two.md).\n"),
        )],
    );
    has(&found, &["docs/alpha/ONE.md:5", "/docs/alpha/NOPE.md"]);
}

#[test]
fn a_root_link_to_a_present_file_and_heading_resolves() {
    let one = "# One\n\n## Part\n\nSee [x](/docs/alpha/two.md#second) and [two](two.md).\n";
    assert_eq!(
        problems("root-present", &[("docs/alpha/ONE.md", Some(one))]),
        Vec::<String>::new()
    );
}

#[test]
fn a_root_link_with_a_missing_heading() {
    let found = problems(
        "root-heading",
        &[(
            "docs/alpha/ONE.md",
            Some("# One\n\n## Part\n\nSee [x](/docs/alpha/two.md#third) and [two](two.md).\n"),
        )],
    );
    has(&found, &["docs/alpha/ONE.md:5", "third"]);
}

#[test]
fn a_root_link_cannot_climb_out_of_the_project() {
    let found = problems(
        "root-climb",
        &[(
            "docs/alpha/ONE.md",
            Some("# One\n\n## Part\n\nSee [x](/../etc/passwd) and [two](two.md).\n"),
        )],
    );
    has(&found, &["docs/alpha/ONE.md:5", "etc/passwd"]);
}

#[test]
fn a_relative_link_cannot_climb_out_of_the_project() {
    let tree = Tree::new("relative-climb", &[]);
    let name = tree
        .root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let beside = tree.root.with_file_name(format!("{name}-beside.md"));
    std::fs::write(&beside, "# Beside\n").unwrap();
    let one = format!(
        "# One\n\n## Part\n\nSee [h](../../../../../../../../../../../../etc/hosts), \
         [b](../../../{name}-beside.md) and [two](two.md).\n"
    );
    std::fs::write(tree.root.join("docs/alpha/ONE.md"), one).unwrap();
    let found = tree.problems();
    let _ = std::fs::remove_file(&beside);
    has(
        &found,
        &[
            "docs/alpha/ONE.md:5",
            "[../../../../../../../../../../../../etc/hosts]: no such file",
        ],
    );
    has(
        &found,
        &[
            "docs/alpha/ONE.md:5",
            &format!("[../../../{name}-beside.md]: no such file"),
        ],
    );
}

#[test]
fn a_link_spelt_in_another_case_is_no_such_file() {
    // macOS finds `Two.md` for `two.md`; Linux, where CI builds the crate, does not.
    let one = "# One\n\n## Part\n\nSee [a](Two.md), [b](../Alpha/two.md#second), \
               [c](/Docs/alpha/two.md) and [two](two.md).\n";
    let found = problems(
        "link-case",
        &[
            ("docs/alpha/ONE.md", Some(one)),
            (
                "docs/alpha/README.md",
                Some(&alpha_readme_plus("- [one.md](one.md) — again.\n")),
            ),
        ],
    );
    has(&found, &["docs/alpha/ONE.md:5", "[Two.md]: no such file"]);
    has(
        &found,
        &[
            "docs/alpha/ONE.md:5",
            "[../Alpha/two.md#second]: no such file",
        ],
    );
    has(
        &found,
        &["docs/alpha/ONE.md:5", "[/Docs/alpha/two.md]: no such file"],
    );
    has(
        &found,
        &[
            "docs/alpha/README.md:7",
            "lists one.md, which does not exist",
        ],
    );
}

#[test]
fn a_link_to_a_folder_outside_docs_is_refused_and_a_file_there_is_watched() {
    // Cargo watches docs/ and each file a document links: a folder outside docs/, moved away, would
    // leave the build green until some document changed.
    let one = "# One\n\n## Part\n\nSee [c](../../content/), [a](../../content/a.json), \
               [p](../../artifacts/pictures/) and [two](two.md).\n";
    let tree = Tree::new(
        "link-folder-outside",
        &[
            ("docs/alpha/ONE.md", Some(one)),
            ("content/a.json", Some("{}\n")),
            ("artifacts/pictures/p.png", Some("")),
        ],
    );
    let found = tree.problems();
    has(
        &found,
        &[
            "docs/alpha/ONE.md:5",
            "[../../content/]: a folder outside docs/",
        ],
    );
    assert_eq!(found.len(), 1, "{found:#?}");

    let one = "# One\n\n## Part\n\nSee [a](../../content/a.json) and [two](two.md).\n";
    std::fs::write(tree.root.join("docs/alpha/ONE.md"), one).unwrap();
    let index = check(&tree.root, &[&Items]).expect("the tree holds together");
    let root = tree.root.canonicalize().unwrap();
    assert!(
        index.watch.contains(&root.join("content/a.json")),
        "{:?}",
        index.watch
    );
    let symphony = root.join("symphony/docs/TASKS.md");
    std::fs::create_dir_all(symphony.parent().unwrap()).unwrap();
    std::fs::write(&symphony, "# Tasks\n\n## Claim\n").unwrap();
    std::fs::write(
        root.join("docs/alpha/ONE.md"),
        "# One\n\n## Part\n\nSee [board](../../symphony/docs/TASKS.md#claim) and [two](two.md).\n",
    )
    .unwrap();
    let index =
        check(&tree.root, &[&Items]).expect("project files can live in the nested workspace");
    assert!(index.watch.contains(&symphony), "{:?}", index.watch);
    std::fs::write(&symphony, "# Tasks\n\n## Assign\n").unwrap();
    has(
        &tree.problems(),
        &["no heading 'claim' in symphony/docs/TASKS.md"],
    );
}

#[test]
fn a_comment_in_a_code_fence_is_not_a_heading() {
    let two = "# Two\n\n```bash\n# not a heading\n```\n\n## Second\n";
    let one = "# One\n\n## Part\n\nSee [x](two.md#not-a-heading) and [[two#not a heading]].\n";
    let found = problems(
        "heading-fence",
        &[
            ("docs/alpha/two.md", Some(two)),
            ("docs/alpha/ONE.md", Some(one)),
        ],
    );
    has(&found, &["docs/alpha/ONE.md:5", "not-a-heading"]);
    has(&found, &["docs/alpha/ONE.md:5", "[[two#not a heading]]"]);
}

// --- card slugs in prose ---

const DECISIONS: &str = "# D\n\nIntro naming no card.\n\n<!-- register:start -->\n- [x] **quiet-roads** — Travel is by roads.\n  [2026-09-24] agent — see **item-count**\n<!-- register:end -->\n";
const ROADS: &str = "# X\n\nThey travel by roads (`quiet-roads`, settled).\n\n```\nquiet-roads in a code block\n```\n\n<!-- register:start -->\n- [ ] **item-count** — How many items?\n<!-- register:end -->\n";

fn lint_tree(name: &str, extra: &[(&'static str, Option<&str>)]) -> Vec<String> {
    let mut edits: Vec<(&'static str, Option<&str>)> = vec![
        ("docs/DECISIONS.md", Some(DECISIONS)),
        ("docs/kappa/x.md", Some(ROADS)),
        (
            "docs/kappa/README.md",
            Some("# Kappa\n\nThe pages: [lambda](lambda.md), [notes](NOTES.md), [x](x.md).\n"),
        ),
        (
            "docs/README.md",
            Some(
                "# Docs\n\nThe register: [decisions](DECISIONS.md).\n\n## Folders\n\n- [alpha/](alpha/README.md) — a.\n- [kappa/](kappa/README.md) — k.\n- [data/](data/) — d.\n- [history/](history/README.md) — h.\n- [analysis/](analysis/README.md) — n.\n",
            ),
        ),
        (
            "docs/analysis/README.md",
            Some("# Analysis\n\n[notes](notes.md)\n"),
        ),
        (
            "docs/analysis/notes.md",
            Some("# Notes\n\nquiet-roads evidence.\n"),
        ),
        (
            "docs/history/old.md",
            Some("# Old\n\nquiet-roads was decided on a Tuesday.\n"),
        ),
    ];
    edits.extend_from_slice(extra);
    problems(name, &edits)
}

#[test]
fn a_card_slug_in_prose_is_named_with_its_place() {
    let found = lint_tree("slug-in-prose", &[]);
    has(&found, &["docs/kappa/x.md:3", "quiet-roads"]);
    assert_eq!(
        found.iter().filter(|p| p.contains("card slug")).count(),
        1,
        "{found:#?}"
    );
}

#[test]
fn registers_code_blocks_history_and_analysis_may_name_a_card() {
    let found = lint_tree("slug-exempt", &[]);
    none_holds(&found, "docs/history/old.md");
    none_holds(&found, "docs/analysis/notes.md");
    none_holds(&found, "docs/kappa/x.md:6");
    none_holds(&found, "docs/DECISIONS.md");
}

#[test]
fn a_slug_inside_a_longer_word_is_not_a_mention() {
    let found = lint_tree(
        "slug-longer-word",
        &[(
            "docs/kappa/x.md",
            Some("# X\n\nquiet-roads-v2 and my-quiet-roads are other words.\n"),
        )],
    );
    none_holds(&found, "card slug");
}

#[test]
fn a_heading_anchor_that_shares_a_slug_is_not_a_mention() {
    let page = "# X\n\nSee [roads](../DECISIONS.md#quiet-roads) and [[DECISIONS#quiet-roads]].\n";
    let found = lint_tree("slug-anchor", &[("docs/kappa/x.md", Some(page))]);
    none_holds(&found, "card slug");
}

// --- components ---

fn two_with(body: &str) -> String {
    format!("# Two\n\n## Second\n\n{body}\n")
}

#[test]
fn an_unknown_component_name() {
    let found = problems(
        "component-name",
        &[("docs/alpha/two.md", Some(&two_with("[[itemz:A1]]")))],
    );
    has(
        &found,
        &[
            "docs/alpha/two.md:5",
            "[[itemz:A1]]: unknown component itemz",
        ],
    );
}

#[test]
fn a_claimed_component_and_the_task_link_are_known() {
    let found = problems(
        "component-known",
        &[(
            "docs/alpha/two.md",
            Some(&two_with("[[item:A2]] [[task:alpha/first]]")),
        )],
    );
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn a_component_no_check_claims_is_unknown_without_the_checks() {
    let tree = Tree::new("component-unclaimed", &[]);
    let found = match check(&tree.root, &[]) {
        Ok(_) => Vec::new(),
        Err(problems) => problems.iter().map(ToString::to_string).collect(),
    };
    assert_eq!(
        found,
        ["docs/kappa/lambda.md:5: [[item:A1]]: unknown component item"]
    );
}

#[test]
fn components_in_code_and_in_history_are_not_checked() {
    let found = problems(
        "component-code",
        &[
            ("docs/history/old.md", Some("# Old\n\n[[itemz:GONE-01]]\n")),
            (
                "docs/alpha/two.md",
                Some(&two_with(
                    "`[[itemz:GONE-01]]`\n\n```\n[[itemz:GONE-02]]\n```",
                )),
            ),
        ],
    );
    assert_eq!(found, Vec::<String>::new());
}

/// A check's problems and its inputs: what it finds fails the build after the generic checks, and
/// every file it reads is watched.
struct Reads;

impl DocCheck for Reads {
    fn name(&self) -> &str {
        "reads"
    }

    fn inputs(&self) -> Vec<String> {
        vec!["config/items.toml".into()]
    }

    fn check(&self, docs: &Docs) -> Vec<Problem> {
        if docs.has("docs/kappa/NOTES.md") {
            vec![Problem::new(
                "docs/kappa/NOTES.md",
                3,
                "notes refused".into(),
            )]
        } else {
            Vec::new()
        }
    }
}

#[test]
fn a_checks_problems_fail_the_build_naming_their_place() {
    let tree = Tree::new("check-problems", &[]);
    let found = check(&tree.root, &[&Items, &Reads]).expect_err("the check refuses the notes");
    let found: Vec<String> = found.iter().map(ToString::to_string).collect();
    assert_eq!(found, ["docs/kappa/NOTES.md:3: notes refused"]);
}

#[test]
fn a_checks_inputs_are_watched_under_the_root() {
    let tree = Tree::new("check-inputs", &[("docs/kappa/NOTES.md", None)]);
    let kappa = "# Kappa\n\nThe pages: [lambda](lambda.md).\n";
    std::fs::write(tree.root.join("docs/kappa/README.md"), kappa).unwrap();
    let index = check(&tree.root, &[&Items, &Reads]).expect("nothing to refuse");
    let input = tree.root.canonicalize().unwrap().join("config/items.toml");
    assert!(index.watch.contains(&input), "{:?}", index.watch);
}
