//! HistoryInProseTest and VerdictsOnPagesTest: documents describe the current design; why and when
//! live in the cards and the ledger, and which picture was approved is a register's record.

use symphony::lint::{Finding, history_in_prose, rule_findings};

use crate::common::{Tree, status};

fn finding(path: &str, line: usize, word: &str) -> Finding {
    (path.to_string(), line, word.to_string())
}

fn prose_tree() -> Tree {
    let tree = Tree::new();
    tree.write(
        "docs/DECISIONS.md",
        "# D\n\nIntro naming no card.\n\n<!-- register:start -->\n- [x] **star-lanes** — Travel is by lanes.\n\
         \x20 [2026-09-24] agent — see **ship-count**\n<!-- register:end -->\n",
    );
    tree.write(
        "docs/factions/x.md",
        "# X\n\nThey travel by lanes (`star-lanes`, approved).\n\n```\nstar-lanes in a code block\n```\n\n\
         <!-- register:start -->\n- [ ] **ship-count** — How many ships?\n<!-- register:end -->\n",
    );
    tree.write(
        "docs/history/old.md",
        "star-lanes was decided on a Tuesday.\n",
    );
    tree.write("docs/analysis/notes.md", "star-lanes evidence.\n");
    tree
}

#[test]
fn a_slug_in_prose_is_reported_with_its_place() {
    let tree = prose_tree();
    assert_eq!(
        history_in_prose(&tree.root).unwrap(),
        vec![finding("docs/factions/x.md", 3, "star-lanes")]
    );
}

#[test]
fn a_slug_after_a_space_or_opening_a_line_is_a_mention() {
    let tree = prose_tree();
    tree.write(
        "docs/factions/x.md",
        "# X\n\nLanes, as star-lanes says.\nstar-lanes opens this line; star-lanes. ends it\n",
    );
    assert_eq!(
        history_in_prose(&tree.root).unwrap(),
        vec![
            finding("docs/factions/x.md", 3, "star-lanes"),
            finding("docs/factions/x.md", 4, "star-lanes"),
            finding("docs/factions/x.md", 4, "star-lanes"),
        ]
    );
}

#[test]
fn registers_code_blocks_history_and_analysis_are_exempt() {
    let tree = prose_tree();
    let found = history_in_prose(&tree.root).unwrap();
    assert!(
        !found
            .iter()
            .any(|(path, _, _)| path == "docs/history/old.md")
    );
    assert!(
        !found
            .iter()
            .any(|(path, _, _)| path == "docs/analysis/notes.md")
    );
    assert!(!found.iter().any(|(_, line, _)| *line == 5));
}

/// One prose line per boundary, each with what the lint finds on it. A mention follows `(`, `[`, `*`, a
/// quote, a space or the line's start, and ends at `)`, `,`, `.`, `;`, `:` or the line's end; it never
/// follows `/`, `#`, `-` or a word character, nor runs on into `-` or a word character. Of two slugs
/// where one opens the other, the longer is the mention.
const BOUNDARIES: [(&str, &[&str]); 26] = [
    ("(star-lanes", &["star-lanes"]),
    ("[star-lanes", &["star-lanes"]),
    ("*star-lanes", &["star-lanes"]),
    ("\"star-lanes", &["star-lanes"]),
    ("'star-lanes", &["star-lanes"]),
    ("see star-lanes", &["star-lanes"]),
    ("star-lanes opens", &["star-lanes"]),
    ("star-lanes)", &["star-lanes"]),
    ("star-lanes,", &["star-lanes"]),
    ("star-lanes.", &["star-lanes"]),
    ("star-lanes;", &["star-lanes"]),
    ("star-lanes:", &["star-lanes"]),
    ("/star-lanes", &[]),
    ("#star-lanes", &[]),
    ("-star-lanes", &[]),
    ("xstar-lanes", &[]),
    ("_star-lanes", &[]),
    ("9star-lanes", &[]),
    ("star-lanes-", &[]),
    ("star-lanesx", &[]),
    ("star-lanes_", &[]),
    ("star-lanes9", &[]),
    ("quay-b", &["quay-b"]),
    ("quay", &["quay"]),
    // The shorter slug alone would match here too (a `.` may follow a mention): only longest first
    // finds the longer one.
    ("dock.two", &["dock.two"]),
    ("dock", &["dock"]),
];

#[test]
fn each_boundary_character_starts_or_ends_a_mention_or_does_not() {
    let tree = prose_tree();
    tree.write(
        "docs/DECISIONS.md",
        "# D\n\n<!-- register:start -->\n- [x] **star-lanes** — Lanes.\n- [ ] **quay** — q.\n\
         - [ ] **quay-b** — q.\n- [ ] **dock** — d.\n- [ ] **dock.two** — d.\n<!-- register:end -->\n",
    );
    let lines: Vec<&str> = BOUNDARIES.iter().map(|(line, _)| *line).collect();
    tree.write(
        "docs/factions/x.md",
        &format!("# X\n\n{}\n", lines.join("\n")),
    );
    let found = history_in_prose(&tree.root).unwrap();
    for (at, (line, expected)) in BOUNDARIES.iter().enumerate() {
        let on_line: Vec<&str> = found
            .iter()
            .filter(|(path, number, _)| path == "docs/factions/x.md" && *number == at + 3)
            .map(|(_, _, slug)| slug.as_str())
            .collect();
        assert_eq!(on_line, *expected, "in {line:?}");
    }
    assert!(
        found
            .iter()
            .all(|(path, _, _)| path == "docs/factions/x.md"),
        "{found:?}"
    );
}

#[test]
fn a_slug_inside_a_longer_word_is_not_a_mention() {
    let tree = prose_tree();
    tree.write(
        "docs/factions/x.md",
        "# X\n\nstar-lanes-v2 and my-star-lanes are other words.\n",
    );
    assert_eq!(history_in_prose(&tree.root).unwrap(), vec![]);
}

#[test]
fn a_heading_anchor_that_shares_a_slug_is_not_a_mention() {
    let tree = prose_tree();
    tree.write(
        "docs/factions/x.md",
        "# X\n\nSee [lanes](other.md#star-lanes) and [[other#star-lanes]].\n",
    );
    assert_eq!(history_in_prose(&tree.root).unwrap(), vec![]);
}

#[test]
fn the_lint_flag_fails_and_names_each_mention() {
    let tree = prose_tree();
    let (code, _, err) = status(&tree.root, &["--lint"]);
    assert_eq!(code, 1);
    assert!(err.contains("docs/factions/x.md:3 star-lanes"), "{err}");
}

#[test]
fn the_lint_flag_passes_on_clean_prose() {
    let tree = prose_tree();
    tree.write("docs/factions/x.md", "# X\n\nThey travel by lanes.\n");
    let (code, out, err) = status(&tree.root, &["--lint"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("✅"), "{out}");
}

fn verdict_tree() -> Tree {
    let tree = Tree::star_foundry();
    tree.write(
        "docs/factions/x.md",
        "# X\n\n**Look:** Black glass.\n\n**Also approved:** [people](a.png)\n\nThe rejected hulls are gone.\n\n\
         ```\napproved in a code block\n```\n\n\
         <!-- register:start -->\n- [x] **x-art** — A picture.\n  [2026-09-25 10:00] owner · approved\n<!-- register:end -->\n",
    );
    tree.write("docs/REVIEW.md", "# R\n\n| `[x]` | approved |\n");
    tree
}

#[test]
fn verdict_words_in_a_faction_page_are_reported_with_their_place() {
    let tree = verdict_tree();
    assert_eq!(
        rule_findings(&tree.root).0,
        vec![
            finding("docs/factions/x.md", 5, "approved"),
            finding("docs/factions/x.md", 7, "rejected")
        ]
    );
}

#[test]
fn registers_code_blocks_and_other_documents_are_exempt() {
    let tree = verdict_tree();
    tree.write(
        "docs/factions/y.md",
        "# Y\n\nNo verdicts here; an unapproved word is another word.\n",
    );
    let paths: Vec<String> = rule_findings(&tree.root)
        .0
        .into_iter()
        .map(|(path, _, _)| path)
        .collect();
    assert_eq!(paths, vec!["docs/factions/x.md"; 2]);
}

#[test]
fn the_folders_readme_and_its_uppercase_hub_documents_are_not_faction_pages() {
    let tree = verdict_tree();
    tree.write(
        "docs/factions/README.md",
        "# Factions\n\nWhat an approved faction page holds.\n",
    );
    tree.write(
        "docs/factions/FACTIONS.md",
        "# Factions\n\nHow the approved design fits together.\n",
    );
    let paths: Vec<String> = rule_findings(&tree.root)
        .0
        .into_iter()
        .map(|(path, _, _)| path)
        .collect();
    assert_eq!(paths, vec!["docs/factions/x.md"; 2]);
}

#[test]
fn the_lint_flag_fails_on_a_verdict_in_a_page() {
    let tree = verdict_tree();
    let (code, _, err) = status(&tree.root, &["--lint"]);
    assert_eq!(code, 1);
    assert!(err.contains("docs/factions/x.md:5 approved"), "{err}");
}
