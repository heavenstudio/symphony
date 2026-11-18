//! `symphony release-notes` over a temporary machine folder holding installed releases' notes and
//! no repository at all.

use super::*;
use crate::test_tree::Tree;

/// The notes a release two after `release-2026-11-17` holds: both releases' steps.
const NOTES: &str = "# What a project changes to take each Symphony release\n\nOldest first.\n\n\
                     ## release-2026-11-17\n\nCommit: aaaaaaa111\nThe first release.\n\nNothing \
                     a project changes to take it.\n\n## release-2026-11-18\n\nCommit: \
                     bbbbbbb222\nFrom release-2026-11-17.\n\nBoard:\n- Board schema 20 to \
                     21.\n\n## release-2026-11-19\n\nCommit: ccccccc333\nFrom \
                     release-2026-11-18.\n\nRun once:\n- `just symphony-cli init --root .`\n";

/// A machine folder in `tree` whose newest release holds [`NOTES`], an older one stale notes.
fn machine(tree: &Tree) -> std::path::PathBuf {
    tree.write("machine/releases/release-2026-11-18/notes.md", "stale");
    tree.write("machine/releases/release-2026-11-19/notes.md", NOTES);
    tree.path("machine")
}

fn printed(machine: &Path, from: Option<&str>, to: Option<&str>) -> (i32, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = print(machine, from, to, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn a_project_two_releases_behind_reads_both_releases_steps_in_order_with_no_repository() {
    let tree = Tree::new();
    let (code, out, err) = printed(&machine(&tree), Some("release-2026-11-17"), None);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        out,
        "What a project changes to take each release after release-2026-11-17, oldest first, \
         from the notes of release-2026-11-19:\n\n## release-2026-11-18\n\nCommit: \
         bbbbbbb222\nFrom release-2026-11-17.\n\nBoard:\n- Board schema 20 to 21.\n\n## \
         release-2026-11-19\n\nCommit: ccccccc333\nFrom release-2026-11-18.\n\nRun once:\n- `just \
         symphony-cli init --root .`\n"
    );
    // The same list for a project pinned at that release's commit.
    let steps = |out: &str| out.split_once("\n\n").unwrap().1.to_string();
    let by_commit = printed(&machine(&tree), Some("aaaaaaa111"), None).1;
    assert!(
        by_commit.starts_with("What a project changes to take each release after aaaaaaa111, ")
    );
    assert_eq!(steps(&by_commit), steps(&out));
}

#[test]
fn the_span_is_bounded_by_to_and_a_project_at_the_newest_has_nothing_to_take() {
    let tree = Tree::new();
    let machine = machine(&tree);
    let (code, out, _) = printed(
        &machine,
        Some("release-2026-11-17"),
        Some("release-2026-11-18"),
    );
    assert_eq!(code, 0);
    assert!(out.contains("after release-2026-11-17 up to release-2026-11-18"));
    assert!(
        out.contains("## release-2026-11-18\n") && !out.contains("## release-2026-11-19"),
        "{out}"
    );
    let (code, out, _) = printed(&machine, None, None);
    assert_eq!(code, 0);
    assert!(out.starts_with("What a project changes to take each release the notes hold, "));
    assert!(out.contains("## release-2026-11-17\n"), "{out}");
    assert_eq!(
        printed(&machine, Some("release-2026-11-19"), None),
        (
            0,
            "No release after release-2026-11-19 in the notes of release-2026-11-19, the newest \
             installed here that holds any: nothing to take.\n"
                .into(),
            String::new()
        )
    );
}

#[test]
fn a_machine_with_no_notes_and_a_name_that_is_no_release_are_refused() {
    let tree = Tree::new();
    tree.write("empty/releases/release-2026-11-19/manifest.json", "{}");
    let (code, out, err) = printed(&tree.path("empty"), None, None);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(
        err.starts_with("symphony release-notes: no release installed in ")
            && err.contains("holds notes (notes.md)"),
        "{err}"
    );
    let machine = machine(&tree);
    for (from, why) in [
        ("main", "\"main\" names no release"),
        (
            "release-2026-11-16",
            "the notes say nothing of release-2026-11-16: they hold release-2026-11-17 to \
             release-2026-11-19",
        ),
        (
            "1b07a67d6be2",
            "1b07a67d6be2 is the commit of no release the notes hold",
        ),
    ] {
        let (code, out, err) = printed(&machine, Some(from), None);
        assert_eq!(
            (code, out.as_str()),
            (1, ""),
            "{from}: nothing printed as if whole"
        );
        assert!(
            err.starts_with("symphony release-notes: ")
                && err.contains(why)
                && err.contains("(the notes of release-2026-11-19, the newest installed"),
            "{err}"
        );
    }
}

#[test]
fn the_release_a_project_is_at_is_the_tag_its_manifest_names() {
    let tree = Tree::new();
    tree.write(
        "tagged/Cargo.toml",
        "[workspace.dependencies]\n# symphony = { git = \"u\", tag = \"release-2026-11-01\" }\n\
         serde = { version = \"1\", tag = \"v1\" }\nsymphony = { package = \"symphony-app\", git = \
         \"u\", tag = \"release-2026-11-17.3\" }\nsymphony-board = { git = \"u\", tag=\"release-2026-11-18\" }\n",
    );
    assert_eq!(
        pinned(&tree.path("tagged")),
        Some("release-2026-11-17.3".into())
    );
    tree.write(
        "by-commit/Cargo.toml",
        "[dependencies]\nsymphony = { git = \"u\", rev = \"1b07a67d6be2\" }\n",
    );
    assert_eq!(pinned(&tree.path("by-commit")), Some("1b07a67d6be2".into()));
    // A dependency whose name holds a key's letters is read past, to the key itself.
    tree.write(
        "named/Cargo.toml",
        "[dependencies]\nsymphony-review = { git = \"u\", rev = \"abcdef1234\" }\nmetag = { \
         git = \"u\", tag = \"release-2026-11-17\" }\n",
    );
    assert_eq!(
        pinned(&tree.path("named")),
        Some("release-2026-11-17".into())
    );
    tree.write(
        "elsewhere/Cargo.toml",
        "[dependencies]\nother = { git = \"u\", rev = \"1b07a67d6be2\" }\n",
    );
    assert_eq!(pinned(&tree.path("elsewhere")), None);
    assert_eq!(pinned(&tree.path("no-manifest")), None);
}

#[test]
fn help_names_both_arguments_and_a_third_is_refused() {
    let run = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = main(&args, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    };
    let (code, out, _) = run(&["--help"]);
    assert_eq!(code, 0);
    assert!(out.starts_with(USAGE) && out.contains("FROM") && out.contains("TO  "));
    let (code, _, err) = run(&["release-2026-11-17", "release-2026-11-18", "more"]);
    assert_eq!(code, 2);
    assert!(err.contains("at most FROM and TO"), "{err}");
}
