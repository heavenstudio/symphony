//! Reachable through READMEs, and links: trees holding a page no README reaches, and a link or
//! `[[NAME#Heading]]` that does not resolve.

use symphony_docs::check;

use super::{Items, ROOT_README, Tree, alpha_readme_plus, has, problems};

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
