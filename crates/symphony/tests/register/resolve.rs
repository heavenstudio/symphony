//! ResolveTest and SupersedeTest: what a tidy writes under a card before it regroups.

use symphony::register::resolve as resolve_text;
use symphony::register::{Group, State, supersede};

use crate::common::{Tree, status_ok};
use crate::{card, lines};

// ResolveTest

#[test]
fn marks_resolvable_cards_and_records_when() {
    let text = "<!-- register:start -->\n### T\n\n- [x] **plain** — 1\n- [x] **noted** — 2\n\
                \x20 [2026-09-24] owner · approved — tweak\n<!-- register:end -->\n";
    let (out, slugs) = resolve_text(text, "2026-09-26 08:00");
    assert_eq!(slugs, vec!["plain"]);
    assert!(
        out.contains("- [r] **plain** — 1\n  [2026-09-26 08:00] agent · resolved\n"),
        "{out}"
    );
    assert!(
        out.contains("- [x] **noted** — 2\n  [2026-09-24] owner · approved — tweak\n"),
        "{out}"
    );
}

#[test]
fn tidy_leaves_a_dismissed_card_alone() {
    let text = "- [~] **a** — 1\n  [2026-09-25 15:40] owner · dismissed\n";
    assert_eq!(
        resolve_text(text, "2026-09-26 08:00"),
        (text.to_string(), vec![])
    );
}

#[test]
fn resolving_twice_changes_nothing() {
    let (once, _) = resolve_text("- [x] **a** — 1\n", "2026-09-26 08:00");
    assert_eq!(
        resolve_text(&once, "2026-09-27 08:00"),
        (once.clone(), vec![])
    );
}

// SupersedeTest

#[test]
fn a_pending_card_a_newer_card_replaces_is_dismissed() {
    let text =
        "- [ ] **old** — 1\n  [2026-09-25 15:34] agent — pic\n- [ ] **new** — 2 *Replaces:* old.\n";
    let (out, slugs) = supersede(text, "2026-09-26 08:00");
    assert_eq!(slugs, vec!["old"]);
    assert!(
        out.contains(
            "- [~] **old** — 1\n  [2026-09-25 15:34] agent — pic\n  [2026-09-26 08:00] agent · dismissed — superseded by **new**\n"
        ),
        "{out}"
    );
    assert!(out.contains("- [ ] **new** — 2"), "{out}");
}

#[test]
fn an_answered_question_card_is_dismissed_too() {
    let text = "- [?] **old** — 1\n  [2026-09-25 15:34] owner · question — why\n  [2026-09-25 16:00] agent — because\n\
                - [ ] **new** — 2 *Replaces:* old.\n";
    let (out, slugs) = supersede(text, "2026-09-26 08:00");
    assert_eq!(slugs, vec!["old"]);
    assert!(out.contains("- [~] **old** — 1\n"), "{out}");
    assert!(
        out.contains("  [2026-09-25 16:00] agent — because\n  [2026-09-26 08:00] agent · dismissed — superseded by **new**\n"),
        "{out}"
    );
    assert_eq!(
        card(&lines(&out).iter().map(String::as_str).collect::<Vec<_>>()).group(),
        Group::Resolved
    );
}

#[test]
fn a_decided_card_keeps_the_owner_verdict() {
    let text = "- [-] **old** — 1\n  [2026-09-25] owner · rejected — no\n\
                - [x] **kept** — 1\n- [ ] **new** — 2 *Replaces:* old.\n- [ ] **n2** — *Replaces:* kept.\n";
    assert_eq!(
        supersede(text, "2026-09-26 08:00"),
        (text.to_string(), vec![])
    );
}

#[test]
fn extends_does_not_supersede() {
    let text = "- [ ] **old** — 1\n- [ ] **new** — 2 *Extends:* old.\n";
    assert_eq!(
        supersede(text, "2026-09-26 08:00"),
        (text.to_string(), vec![])
    );
}

#[test]
fn a_card_does_not_supersede_itself_and_prose_mentions_do_not_count() {
    let text = "- [ ] **old** — replaces nothing; **old** again *Replaces:* old.\n- [ ] **b** — like old\n";
    assert_eq!(
        supersede(text, "2026-09-26 08:00"),
        (text.to_string(), vec![])
    );
}

#[test]
fn superseded_card_is_resolved_on_the_board() {
    let (out, _) = supersede(
        "- [ ] **old** — 1\n- [ ] **new** — *Replaces:* old.\n",
        "2026-09-26 08:00",
    );
    let found = card(&lines(&out).iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(
        (found.state, found.group()),
        (State::Dismissed, Group::Resolved)
    );
}

#[test]
fn tidy_supersedes_before_sorting() {
    let tree = Tree::new();
    tree.write(
        "docs/DECISIONS.md",
        "---\nstatus: review\n---\n\n<!-- register:start -->\n### T\n\n\
         - [ ] **old** — 1\n- [ ] **new** — *Replaces:* old.\n<!-- register:end -->\n",
    );
    status_ok(&tree.root, &["--tidy", "--now", "2026-09-26 08:00"]);
    let text = tree.read("docs/DECISIONS.md");
    assert!(
        text.contains(
            "- [~] **old** — 1\n  [2026-09-26 08:00] agent · dismissed — superseded by **new**"
        ),
        "{text}"
    );
}
