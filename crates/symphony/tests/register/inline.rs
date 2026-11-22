//! A card inline in a document, at the place it decides, with no register region around it: the
//! board, the lint and the tidy read it as they read a register's.

use symphony::lint::{history_in_prose, repeated_slugs};

use crate::common::{Tree, status, status_ok};

/// A reference document with a pending card under its first section and an approved one under
/// its second, prose after each.
const LAMPS: &str = "# Lamps\n\n## How many\n\nThere are 141 lamps.\n\n\
    - [ ] **lamp-count** — 141 lamps would be drawn. See **lamp-brass**.\n\
    \x20 [2026-11-21 10:00] agent — Posted, beside lamp-brass.\n\n\
    \x20   **Base.** 120 today.\n\n\
    They hang in three sizes.\n\n## Finishes\n\nBrass and steel.\n\n\
    - [x] **lamp-brass** — Brass stays.\n\
    \x20 [2026-11-21 10:05] owner · approved\n\nSteel is brushed.\n";

fn tree() -> Tree {
    let tree = Tree::new();
    tree.write("docs/LAMPS.md", LAMPS);
    tree
}

#[test]
fn the_board_counts_an_inline_card_and_names_its_line() {
    let tree = tree();
    let json: serde_json::Value =
        serde_json::from_str(&status_ok(&tree.root, &["--json"])).unwrap();
    let cards = &json["docs"][0]["decisions"];
    assert_eq!(json["docs"][0]["path"], "docs/LAMPS.md");
    assert_eq!(cards[0]["slug"], "lamp-count");
    assert_eq!(cards[0]["line"], 7);
    assert_eq!(cards[0]["group"], "decide");
    assert_eq!(cards[1]["slug"], "lamp-brass");
    assert_eq!(cards[1]["line"], 18);
    assert_eq!(cards[1]["group"], "resolved");
}

#[test]
fn a_cards_own_lines_are_not_prose_wherever_it_sits() {
    let tree = tree();
    assert_eq!(history_in_prose(&tree.root).unwrap(), vec![]);
    let (code, out, err) = status(&tree.root, &["--lint"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("✅"), "{out}");
}

#[test]
fn a_slug_in_the_prose_beside_an_inline_card_is_still_reported() {
    let tree = tree();
    tree.write(
        "docs/LAMPS.md",
        &LAMPS.replace(
            "Steel is brushed.",
            "Steel is brushed, as lamp-brass has it.",
        ),
    );
    assert_eq!(
        history_in_prose(&tree.root).unwrap(),
        vec![("docs/LAMPS.md".to_string(), 21, "lamp-brass".to_string())]
    );
}

#[test]
fn a_card_in_a_code_block_is_an_example_and_its_slug_no_card() {
    let tree = Tree::new();
    tree.write(
        "docs/GUIDE.md",
        "# Guide\n\n```markdown\n- [ ] **example** — A card.\n```\n\nAn example names example.\n",
    );
    assert_eq!(history_in_prose(&tree.root).unwrap(), vec![]);
    assert_eq!(repeated_slugs(&tree.root).unwrap(), vec![]);
}

#[test]
fn tidy_resolves_an_inline_card_where_it_sits_and_moves_nothing() {
    let tree = tree();
    let out = status_ok(&tree.root, &["--tidy", "--now", "2026-11-22 08:00"]);
    assert!(out.contains("resolved: lamp-brass"), "{out}");
    assert!(out.contains("tidied docs/LAMPS.md"), "{out}");
    assert!(!out.contains("regrouped"), "{out}");
    assert_eq!(
        tree.read("docs/LAMPS.md"),
        LAMPS
            .replace("- [x] **lamp-brass**", "- [r] **lamp-brass**")
            .replace(
                "  [2026-11-21 10:05] owner · approved\n",
                "  [2026-11-21 10:05] owner · approved\n  [2026-11-22 08:00] agent · resolved\n"
            )
    );
}

#[test]
fn tidy_supersedes_an_inline_card_a_newer_one_replaces() {
    let tree = Tree::new();
    tree.write(
        "docs/LAMPS.md",
        "# Lamps\n\n- [ ] **old** — Old.\n\nWords.\n\n- [ ] **new** — *Replaces:* old. New.\n",
    );
    let out = status_ok(&tree.root, &["--tidy", "--now", "2026-11-22 08:00"]);
    assert!(out.contains("superseded: old"), "{out}");
    assert!(
        tree.read("docs/LAMPS.md").contains(
            "- [~] **old** — Old.\n  [2026-11-22 08:00] agent · dismissed — superseded by **new**\n\nWords."
        ),
        "{}",
        tree.read("docs/LAMPS.md")
    );
}

#[test]
fn tidy_leaves_a_document_with_nothing_to_tidy_unwritten_and_unnamed() {
    let tree = Tree::new();
    // No newline at its end: a file the tidy wrote would gain one.
    let text = "# Lamps\n\n- [ ] **waiting** — Still asked.\n\nWords.";
    tree.write("docs/LAMPS.md", text);
    let out = status_ok(&tree.root, &["--tidy", "--now", "2026-11-22 08:00"]);
    assert_eq!(out, "");
    assert_eq!(tree.read("docs/LAMPS.md"), text);
}

#[test]
fn tidy_regroups_a_register_and_resolves_the_inline_cards_of_other_documents() {
    let tree = tree();
    tree.write(
        "docs/DECISIONS.md",
        "# D\n\n<!-- register:start -->\n### T\n\n- [x] **done** — plain.\n<!-- register:end -->\n",
    );
    let out = status_ok(&tree.root, &["--tidy", "--now", "2026-11-22 08:00"]);
    assert_eq!(
        out,
        "resolved: done\nregrouped docs/DECISIONS.md\nresolved: lamp-brass\ntidied docs/LAMPS.md\n"
    );
}

#[test]
fn a_document_with_a_region_and_an_inline_card_has_both_resolved() {
    let tree = Tree::new();
    tree.write(
        "docs/MIXED.md",
        "# M\n\n- [x] **inline** — Above.\n\nWords.\n\n<!-- register:start -->\n### T\n\n\
         - [x] **held** — In the region.\n<!-- register:end -->\n",
    );
    let out = status_ok(&tree.root, &["--tidy", "--now", "2026-11-22 08:00"]);
    assert_eq!(out, "resolved: inline, held\nregrouped docs/MIXED.md\n");
    let text = tree.read("docs/MIXED.md");
    assert!(
        text.starts_with("# M\n\n- [r] **inline** — Above.\n  [2026-11-22 08:00] agent · resolved\n\nWords.\n\n<!-- register:start -->\n## Resolved\n"),
        "{text}"
    );
}

#[test]
fn one_name_on_two_cards_is_refused_naming_each_place() {
    let tree = tree();
    tree.write(
        "docs/OTHER.md",
        "# Other\n\n- [ ] **lamp-brass** — Another.\n\n- [ ] **lamp-brass** — And again.\n",
    );
    assert_eq!(
        repeated_slugs(&tree.root).unwrap(),
        vec![(
            "lamp-brass".to_string(),
            vec![
                "docs/LAMPS.md:18".to_string(),
                "docs/OTHER.md:3".to_string(),
                "docs/OTHER.md:5".to_string()
            ]
        )]
    );
    let (code, _, err) = status(&tree.root, &["--lint"]);
    assert_eq!(code, 1);
    assert!(
        err.contains("docs/LAMPS.md:18, docs/OTHER.md:3, docs/OTHER.md:5: card lamp-brass again"),
        "{err}"
    );
    assert!(
        err.contains("✖ 1 card name(s) used by more than one card"),
        "{err}"
    );
}

#[test]
fn a_record_in_history_may_repeat_a_live_cards_name() {
    let tree = tree();
    tree.write(
        "docs/history/old.md",
        "# Old\n\n- [r] **lamp-brass** — As it was.\n",
    );
    assert_eq!(repeated_slugs(&tree.root).unwrap(), vec![]);
}
