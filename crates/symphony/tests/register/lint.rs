//! HistoryInProseTest: documents describe the current design; why and when live in the cards and the
//! ledger.

use symphony::lint::{Finding, history_in_prose};

use crate::common::{Tree, status};

fn finding(path: &str, line: usize, word: &str) -> Finding {
    (path.to_string(), line, word.to_string())
}

fn prose_tree() -> Tree {
    let tree = Tree::new();
    tree.write(
        "docs/DECISIONS.md",
        "# D\n\nIntro naming no card.\n\n<!-- register:start -->\n- [x] **quiet-roads** — Travel is by roads.\n\
         \x20 [2026-09-24] agent — see **item-count**\n<!-- register:end -->\n",
    );
    tree.write(
        "docs/kappa/x.md",
        "# X\n\nThey travel by roads (`quiet-roads`, approved).\n\n```\nquiet-roads in a code block\n```\n\n\
         <!-- register:start -->\n- [ ] **item-count** — How many items?\n<!-- register:end -->\n",
    );
    tree.write(
        "docs/history/old.md",
        "quiet-roads was decided on a Tuesday.\n",
    );
    tree.write("docs/analysis/notes.md", "quiet-roads evidence.\n");
    tree.write("docs/kappa/analysis/deep.md", "quiet-roads evidence.\n");
    tree.write("docs/kappa/history/older.md", "quiet-roads, long ago.\n");
    tree
}

#[test]
fn a_slug_in_prose_is_reported_with_its_place() {
    let tree = prose_tree();
    assert_eq!(
        history_in_prose(&tree.root).unwrap(),
        vec![finding("docs/kappa/x.md", 3, "quiet-roads")]
    );
}

#[test]
fn a_slug_after_a_space_or_opening_a_line_is_a_mention() {
    let tree = prose_tree();
    tree.write(
        "docs/kappa/x.md",
        "# X\n\nRoads, as quiet-roads says.\nquiet-roads opens this line; quiet-roads. ends it\n",
    );
    assert_eq!(
        history_in_prose(&tree.root).unwrap(),
        vec![
            finding("docs/kappa/x.md", 3, "quiet-roads"),
            finding("docs/kappa/x.md", 4, "quiet-roads"),
            finding("docs/kappa/x.md", 4, "quiet-roads"),
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
    // An area's own analyses and history are passed over as the first folder's are.
    assert!(!found.iter().any(|(path, _, _)| {
        path == "docs/kappa/analysis/deep.md" || path == "docs/kappa/history/older.md"
    }));
    assert!(!found.iter().any(|(_, line, _)| *line == 5));
}

#[test]
fn a_document_named_as_an_exempt_folder_is_judged() {
    let tree = prose_tree();
    tree.write("docs/kappa/analysis.md", "# A\n\nquiet-roads here.\n");
    tree.write("docs/kappa/history.md", "# H\n\nquiet-roads here.\n");
    let found = history_in_prose(&tree.root).unwrap();
    for path in ["docs/kappa/analysis.md", "docs/kappa/history.md"] {
        assert!(
            found.contains(&finding(path, 3, "quiet-roads")),
            "{path}: {found:?}"
        );
    }
}

/// One prose line per boundary, each with what the lint finds on it. A mention follows `(`, `[`, `*`, a
/// quote, a space or the line's start, and ends at `)`, `,`, `.`, `;`, `:` or the line's end; it never
/// follows `/`, `#`, `-` or a word character, nor runs on into `-` or a word character. Of two slugs
/// where one opens the other, the longer is the mention.
const BOUNDARIES: [(&str, &[&str]); 26] = [
    ("(quiet-roads", &["quiet-roads"]),
    ("[quiet-roads", &["quiet-roads"]),
    ("*quiet-roads", &["quiet-roads"]),
    ("\"quiet-roads", &["quiet-roads"]),
    ("'quiet-roads", &["quiet-roads"]),
    ("see quiet-roads", &["quiet-roads"]),
    ("quiet-roads opens", &["quiet-roads"]),
    ("quiet-roads)", &["quiet-roads"]),
    ("quiet-roads,", &["quiet-roads"]),
    ("quiet-roads.", &["quiet-roads"]),
    ("quiet-roads;", &["quiet-roads"]),
    ("quiet-roads:", &["quiet-roads"]),
    ("/quiet-roads", &[]),
    ("#quiet-roads", &[]),
    ("-quiet-roads", &[]),
    ("xquiet-roads", &[]),
    ("_quiet-roads", &[]),
    ("9quiet-roads", &[]),
    ("quiet-roads-", &[]),
    ("quiet-roadsx", &[]),
    ("quiet-roads_", &[]),
    ("quiet-roads9", &[]),
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
        "# D\n\n<!-- register:start -->\n- [x] **quiet-roads** — Roads.\n- [ ] **quay** — q.\n\
         - [ ] **quay-b** — q.\n- [ ] **dock** — d.\n- [ ] **dock.two** — d.\n<!-- register:end -->\n",
    );
    let lines: Vec<&str> = BOUNDARIES.iter().map(|(line, _)| *line).collect();
    tree.write("docs/kappa/x.md", &format!("# X\n\n{}\n", lines.join("\n")));
    let found = history_in_prose(&tree.root).unwrap();
    for (at, (line, expected)) in BOUNDARIES.iter().enumerate() {
        let on_line: Vec<&str> = found
            .iter()
            .filter(|(path, number, _)| path == "docs/kappa/x.md" && *number == at + 3)
            .map(|(_, _, slug)| slug.as_str())
            .collect();
        assert_eq!(on_line, *expected, "in {line:?}");
    }
    assert!(
        found.iter().all(|(path, _, _)| path == "docs/kappa/x.md"),
        "{found:?}"
    );
}

#[test]
fn a_slug_inside_a_longer_word_is_not_a_mention() {
    let tree = prose_tree();
    tree.write(
        "docs/kappa/x.md",
        "# X\n\nquiet-roads-v2 and my-quiet-roads are other words.\n",
    );
    assert_eq!(history_in_prose(&tree.root).unwrap(), vec![]);
}

#[test]
fn a_heading_anchor_that_shares_a_slug_is_not_a_mention() {
    let tree = prose_tree();
    tree.write(
        "docs/kappa/x.md",
        "# X\n\nSee [roads](other.md#quiet-roads) and [[other#quiet-roads]].\n",
    );
    assert_eq!(history_in_prose(&tree.root).unwrap(), vec![]);
}

#[test]
fn the_lint_flag_fails_and_names_each_mention() {
    let tree = prose_tree();
    let (code, _, err) = status(&tree.root, &["--lint"]);
    assert_eq!(code, 1);
    assert!(err.contains("docs/kappa/x.md:3 quiet-roads"), "{err}");
}

#[test]
fn the_lint_flag_passes_on_clean_prose() {
    let tree = prose_tree();
    tree.write("docs/kappa/x.md", "# X\n\nThey travel by roads.\n");
    let (code, out, err) = status(&tree.root, &["--lint"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("✅"), "{out}");
}
