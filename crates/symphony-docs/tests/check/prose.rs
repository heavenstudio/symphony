//! Card slugs in prose, and components: a design document naming a card outside its register, and
//! a component no check resolves.

use symphony_docs::{DocCheck, Docs, Problem, check};

use super::{Items, Tree, has, none_holds, problems};

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

/// A card inline in a document, under the paragraph it decides and outside any register: its own
/// lines are its record, and the prose around it is still read.
#[test]
fn an_inline_cards_own_lines_may_name_cards_and_the_prose_beside_it_may_not() {
    let page = "# X\n\nThey travel by roads.\n\n- [ ] **road-width** — How wide? See **quiet-roads**.\n  [2026-09-24] agent — Posted, after quiet-roads.\n\n    **Base.** quiet-roads as it stands.\n\nWide, as road-width has it.\n";
    let found = lint_tree("slug-inline", &[("docs/kappa/x.md", Some(page))]);
    has(&found, &["docs/kappa/x.md:10", "road-width"]);
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
fn an_areas_analysis_folder_may_name_a_card_and_a_file_of_that_name_may_not() {
    let found = lint_tree(
        "slug-area-analysis",
        &[
            (
                "docs/kappa/analysis/deep.md",
                Some("# Deep\n\nquiet-roads evidence.\n"),
            ),
            (
                "docs/kappa/analysis.md",
                Some("# Not a folder\n\nquiet-roads again.\n"),
            ),
        ],
    );
    let slugs = |path: &str| {
        found
            .iter()
            .filter(|p| p.contains("card slug") && p.contains(path))
            .count()
    };
    assert_eq!(slugs("docs/kappa/analysis/deep.md"), 0, "{found:#?}");
    assert_eq!(slugs("docs/kappa/analysis.md:3"), 1, "{found:#?}");
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
    fn name(&self) -> &'static str {
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
