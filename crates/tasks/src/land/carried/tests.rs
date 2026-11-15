//! What the carried approve reads without a repository: which paths are documents, and what the
//! land says of the merge. The merges themselves are `tests/sweep/land_carried.rs`'s.

use super::*;

#[test]
fn a_document_is_a_markdown_file_by_its_extension() {
    for path in ["docs/DECISIONS.md", "README.MD", "a b/notes.md", "RULES.md"] {
        assert!(document(path), "{path}");
    }
    for path in [
        "src/land.rs",
        "docs/md",
        "docs/notes.md.rs",
        "notes.mdx",
        "md",
        ".md/config",
        "symphony.toml",
    ] {
        assert!(!document(path), "{path}");
    }
}

fn carried(settled: &[&str]) -> Carried {
    Carried {
        merge: "c".repeat(40),
        approved: "a".repeat(40),
        from: "b".repeat(40),
        settled: settled.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn the_land_names_the_approved_commit_the_merge_and_what_it_settles() {
    assert_eq!(
        carried(&["docs/A.md", "docs/B.md"]).said("main"),
        "naming aaaaaaa, which stands over the one merge ccccccc of main at bbbbbbb into it, \
         which differs from git's own merge of the two in documents alone (docs/A.md, \
         docs/B.md); its tests are read at ccccccc"
    );
    assert_eq!(
        carried(&[]).said("trunk"),
        "naming aaaaaaa, which stands over the one merge ccccccc of trunk at bbbbbbb into it, \
         which is git's own merge of the two; its tests are read at ccccccc"
    );
}
