//! The Folders list and the list READMEs: a folder missing from the list or a stale entry, and a
//! list README missing a document or still naming one that is gone.

use super::{ROOT_README, alpha_readme_plus, has, problems};

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
