//! Blank lines before a card belong outside its source span. Verdict and tidy
//! operations must leave the following card in place exactly once.

use symphony::board::tidy_with;
use symphony::edit::set_decision;
use symphony::plugins::Plugins;
use symphony::register::{ReviewError, sort_register, supersede};

use crate::cards;
use crate::common::{Tree, plain, status_ok};

const NOW: &str = "2026-10-08 12:00";

/// Fixture: card a, blank line, card b, then card c directly after b.
const ONE_BLANK: &str = "<!-- register:start -->\n### T\n\n- [ ] **a** — A.\n\n- [ ] **b** — B.\n- [ ] **c** — C.\n<!-- register:end -->\n";
/// The same with two blank lines before b.
const TWO_BLANKS: &str = "<!-- register:start -->\n### T\n\n- [ ] **a** — A.\n\n\n- [ ] **b** — B.\n- [ ] **c** — C.\n<!-- register:end -->\n";
/// Either, once the owner approved b: c where it was, once.
const B_APPROVED: &str = "<!-- register:start -->\n## Needs your decision\n\n### T\n\n- [ ] **a** — A.\n- [ ] **c** — C.\n\n## Resolved\n\n### T\n\n- [x] **b** — B.\n  [2026-10-08 12:00] owner · approved\n<!-- register:end -->\n";

#[test]
fn a_blank_line_before_a_card_is_not_the_cards() {
    let said = "  [2026-10-08 09:00] owner · approved";
    let given: [&[&str]; 4] = [
        &[
            "- [ ] **a** — A.",
            "",
            "- [x] **b** — B.",
            said,
            "- [ ] **c** — C.",
        ],
        &[
            "- [ ] **a** — A.",
            "",
            "",
            "- [x] **b** — B.",
            said,
            "- [ ] **c** — C.",
        ],
        // b the last card, its line the last in the file
        &["- [ ] **a** — A.", "", "- [x] **b** — B.", said],
        // a topic between the blank and b
        &["- [ ] **a** — A.", "", "### U", "- [x] **b** — B.", said],
    ];
    let wrong: Vec<(&[&str], Vec<String>)> = given
        .into_iter()
        .map(|lines| (lines, cards(lines).swap_remove(1).raw))
        .filter(|(_, raw)| *raw != [said])
        .collect();
    assert_eq!(wrong, vec![]);
}

#[test]
fn a_verdict_on_the_card_after_a_blank_keeps_the_next_card_where_it_was() {
    let wrong: Vec<_> = [ONE_BLANK, TWO_BLANKS]
        .into_iter()
        .map(|register| {
            (
                register,
                set_decision(register, "b", "approved", "", NOW, &plain()),
            )
        })
        .filter(|(_, written)| *written != Ok(B_APPROVED.to_string()))
        .collect();
    assert_eq!(wrong, vec![]);
}

#[test]
fn the_write_after_that_verdict_still_lands() {
    let wrong: Vec<_> = [ONE_BLANK, TWO_BLANKS]
        .into_iter()
        .map(|register| {
            let once =
                set_decision(register, "b", "approved", "", NOW, &plain()).unwrap_or_default();
            (
                register,
                set_decision(&once, "c", "rejected", "", NOW, &plain()),
            )
        })
        .filter(|(_, next)| next.is_err())
        .collect();
    assert_eq!(wrong, vec![]);
}

/// Each register with b approved by the owner, as a tidy finds it, and what the tidy writes.
const TIDIED: [(&str, &str); 3] = [
    (
        "# D\n\n<!-- register:start -->\n### T\n\n- [ ] **a** — A.\n\n- [x] **b** — B.\n  [2026-10-08 09:00] owner · approved\n- [ ] **c** — C.\n<!-- register:end -->\n",
        "# D\n\n<!-- register:start -->\n## Needs your decision\n\n### T\n\n- [ ] **a** — A.\n- [ ] **c** — C.\n\n## Resolved\n\n### T\n\n- [r] **b** — B.\n  [2026-10-08 09:00] owner · approved\n  [2026-10-08 12:00] agent · resolved\n<!-- register:end -->\n",
    ),
    (
        "# D\n\n<!-- register:start -->\n### T\n\n- [ ] **a** — A.\n\n\n- [x] **b** — B.\n  [2026-10-08 09:00] owner · approved\n- [ ] **c** — C.\n<!-- register:end -->\n",
        "# D\n\n<!-- register:start -->\n## Needs your decision\n\n### T\n\n- [ ] **a** — A.\n- [ ] **c** — C.\n\n## Resolved\n\n### T\n\n- [r] **b** — B.\n  [2026-10-08 09:00] owner · approved\n  [2026-10-08 12:00] agent · resolved\n<!-- register:end -->\n",
    ),
    // b the register's last card, the region's end the file's last line
    (
        "# D\n\n<!-- register:start -->\n### T\n\n- [ ] **a** — A.\n\n- [x] **b** — B.\n  [2026-10-08 09:00] owner · approved\n<!-- register:end -->\n",
        "# D\n\n<!-- register:start -->\n## Needs your decision\n\n### T\n\n- [ ] **a** — A.\n\n## Resolved\n\n### T\n\n- [r] **b** — B.\n  [2026-10-08 09:00] owner · approved\n  [2026-10-08 12:00] agent · resolved\n<!-- register:end -->\n",
    ),
];

#[test]
fn a_tidy_writes_the_resolved_line_under_the_card_it_resolves() {
    let wrong: Vec<(&str, String)> = TIDIED
        .into_iter()
        .filter_map(|(given, tidied)| {
            let tree = Tree::new();
            tree.write("docs/DECISIONS.md", given);
            status_ok(&tree.root, &["--tidy", "--now", NOW]);
            let written = tree.read("docs/DECISIONS.md");
            (written != tidied).then_some((given, written))
        })
        .collect();
    assert_eq!(wrong, vec![]);
}

#[test]
fn supersede_dismisses_under_the_card_it_dismisses() {
    let given = "- [ ] **a** — A.\n\n- [ ] **b** — B.\n  [2026-10-08 09:00] agent — first draft\n- [ ] **c** — C. *Replaces:* b.\n";
    assert_eq!(
        supersede(given, NOW),
        (
            "- [ ] **a** — A.\n\n- [~] **b** — B.\n  [2026-10-08 09:00] agent — first draft\n  [2026-10-08 12:00] agent · dismissed — superseded by **c**\n- [ ] **c** — C. *Replaces:* b.\n"
                .to_string(),
            vec!["b".to_string()]
        )
    );
}

#[test]
fn a_regroup_refuses_rather_than_writes_a_card_twice() {
    // b approved, its owner line under it, not yet regrouped: what a verdict hands the sort
    let given = "<!-- register:start -->\n### T\n\n- [ ] **a** — A.\n\n- [x] **b** — B.\n  [2026-10-08 12:00] owner · approved\n- [ ] **c** — C.\n<!-- register:end -->\n";
    match sort_register(given, &plain()) {
        Ok(sorted) => assert_eq!(sorted.matches("**c**").count(), 1, "{sorted}"),
        Err(problem) => assert!(problem.0.contains("would repeat card(s) 'c'"), "{problem}"),
    }
}

/// A regroup that writes card c twice.
fn copy_c(text: &str, plugins: &Plugins) -> Result<String, ReviewError> {
    sort_register(text, plugins)
        .map(|sorted| sorted.replace("- [ ] **c** — C.\n", "- [ ] **c** — C.\n- [ ] **c** — C.\n"))
}

#[test]
fn a_tidy_whose_regroup_repeats_a_card_writes_nothing() {
    let tree = Tree::new();
    let text = "# D\n\n<!-- register:start -->\n### T\n\n- [ ] **a** — A.\n- [ ] **c** — C.\n<!-- register:end -->\n";
    tree.write("docs/DECISIONS.md", text);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = tidy_with(&tree.root, Some(NOW), copy_c, &mut out, &mut err);
    let err = String::from_utf8(err).unwrap();
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("would repeat card(s) 'c'"), "{err}");
    assert_eq!(tree.read("docs/DECISIONS.md"), text);
}
