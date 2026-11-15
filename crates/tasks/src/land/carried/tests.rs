//! What the carried approve reads without a repository: which paths are documents, and what the
//! land says of the merge. The merges themselves are `tests/sweep/land_carried.rs`'s.

use super::*;

#[test]
fn a_document_is_a_markdown_file_by_its_extension() {
    for path in [
        "docs/DECISIONS.md",
        "README.MD",
        "a b/notes.md",
        "docs/skills.md",
        "docs/RULES-of-thumb.md",
    ] {
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
        // What instructs agents: a guide, the rules, a skill, wherever they lie.
        "RULES.md",
        "AGENTS.md",
        "crates/tasks/AGENTS.md",
        "CLAUDE.md",
        ".agents/skills/work/SKILL.md",
        "crates/symphony/shipped/skills/work/finish.md",
        "skills/notes.md",
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
         which settles conflicts in documents alone (docs/A.md, docs/B.md) and is git's own \
         merge of the two elsewhere; its tests are read at ccccccc"
    );
    assert_eq!(
        carried(&[]).said("trunk"),
        "naming aaaaaaa, which stands over the one merge ccccccc of trunk at bbbbbbb into it, \
         which is git's own merge of the two; its tests are read at ccccccc"
    );
}

#[test]
fn a_refusal_names_the_first_paths_and_how_many_more() {
    let paths: Vec<String> = (1..=7).map(|n| format!("src/{n}.rs")).collect();
    assert_eq!(listed(&paths[..2]), "src/1.rs, src/2.rs");
    assert_eq!(
        listed(&paths),
        "src/1.rs, src/2.rs, src/3.rs, src/4.rs, src/5.rs and 2 more"
    );
}
