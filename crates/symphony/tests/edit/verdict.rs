//! SetDecisionTest, RepeatDecisionTest and AutoApprovedRenderTest's writes: a verdict sets the mark
//! and appends a dated owner line; a tap sent twice lands once.

use symphony::edit::{add_message, is_repeat_decision, set_decision};
use symphony::register::{Group, Who, parse_decisions};

use crate::common::plain;
use crate::lines;

// RepeatDecisionTest: a tap whose answer was lost is sent again; the file itself says whether it
// already landed.

const BASE: &str = "# D\n\n- [ ] **one** — first.\n- [ ] **two** — second.\n";

fn written(slug: &str, state: &str, note: &str, at: &str) -> String {
    set_decision(BASE, slug, state, note, at, &plain()).unwrap()
}

fn repeat(text: &str, slug: &str, state: &str, note: &str, at: &str) -> bool {
    is_repeat_decision(text, slug, state, note, at).unwrap()
}

#[test]
fn the_same_verdict_and_note_just_written_is_a_repeat() {
    let text = written("one", "approved", "Fine.\nReally.", "2026-10-06 09:00");
    assert!(repeat(
        &text,
        "one",
        "approved",
        "Fine.\nReally.",
        "2026-10-06 09:03"
    ));
}

#[test]
fn an_empty_note_repeats_too() {
    let text = written("one", "approved", "", "2026-10-06 09:00");
    assert!(repeat(&text, "one", "approved", "", "2026-10-06 09:00"));
}

#[test]
fn a_different_state_or_note_is_not_a_repeat() {
    let text = written("one", "approved", "Fine.", "2026-10-06 09:00");
    assert!(!repeat(
        &text,
        "one",
        "rejected",
        "Fine.",
        "2026-10-06 09:01"
    ));
    assert!(!repeat(
        &text,
        "one",
        "approved",
        "Fine!",
        "2026-10-06 09:01"
    ));
    assert!(!repeat(
        &text,
        "one",
        "approved",
        "Fine.\nMore.",
        "2026-10-06 09:01"
    ));
}

#[test]
fn an_old_entry_is_not_a_repeat() {
    let text = written("one", "approved", "Fine.", "2026-10-06 09:00");
    assert!(!repeat(
        &text,
        "one",
        "approved",
        "Fine.",
        "2026-10-06 09:06"
    ));
}

#[test]
fn only_the_newest_line_of_the_card_counts() {
    let text = written("one", "approved", "Fine.", "2026-10-06 09:00");
    let text = add_message(&text, "one", "Wait.", "2026-10-06 09:01", Who::Owner, None).unwrap();
    assert!(!repeat(
        &text,
        "one",
        "approved",
        "Fine.",
        "2026-10-06 09:02"
    ));
}

#[test]
fn an_agents_line_or_another_card_is_not_a_repeat() {
    let text = written("one", "approved", "Fine.", "2026-10-06 09:00");
    assert!(!repeat(
        &text,
        "two",
        "approved",
        "Fine.",
        "2026-10-06 09:01"
    ));
    let agent = add_message(BASE, "one", "Fine.", "2026-10-06 09:00", Who::Agent, None).unwrap();
    assert!(!repeat(
        &agent,
        "one",
        "approved",
        "Fine.",
        "2026-10-06 09:01"
    ));
    assert!(!repeat(BASE, "one", "approved", "", "2026-10-06 09:01"));
    assert!(!repeat(BASE, "nine", "approved", "", "2026-10-06 09:01"));
}

#[test]
fn a_time_that_does_not_read_is_an_error_not_an_answer() {
    let text = written("one", "approved", "Fine.", "2026-10-06 09:00");
    assert!(is_repeat_decision(&text, "one", "approved", "Fine.", "soon").is_err());
}

// SetDecisionTest

const TEXT: &str = "# D\n\n- [ ] **one** — first.\n- [-] **two** — second.\n  [2026-09-24 10:00] owner · rejected — old note\n- [x] **three** — third.\n";
const NOW: &str = "2026-09-26 09:15";

#[test]
fn sets_marker_and_appends_a_dated_owner_entry() {
    let out = set_decision(TEXT, "one", "rejected", "Too many.\nCut it.", NOW, &plain()).unwrap();
    assert_eq!(
        lines(&out)[2..6].to_vec(),
        vec![
            "- [-] **one** — first.",
            "  [2026-09-26 09:15] owner · rejected — Too many.",
            "    Cut it.",
            "- [-] **two** — second."
        ]
    );
}

#[test]
fn history_is_appended_never_replaced() {
    let out = set_decision(TEXT, "two", "approved", "", NOW, &plain()).unwrap();
    assert_eq!(
        lines(&out)[3..6].to_vec(),
        vec![
            "- [x] **two** — second.",
            "  [2026-09-24 10:00] owner · rejected — old note",
            "  [2026-09-26 09:15] owner · approved"
        ]
    );
}

#[test]
fn pending_is_a_valid_state() {
    let out = set_decision(TEXT, "three", "pending", "", NOW, &plain()).unwrap();
    assert!(
        out.contains("- [ ] **three** — third.\n  [2026-09-26 09:15] owner · pending"),
        "{out}"
    );
}

#[test]
fn dismissed_is_an_owner_state_with_its_own_mark() {
    let out = set_decision(TEXT, "one", "dismissed", "", NOW, &plain()).unwrap();
    assert!(
        out.contains("- [~] **one** — first.\n  [2026-09-26 09:15] owner · dismissed"),
        "{out}"
    );
}

#[test]
fn owner_cannot_mark_resolved() {
    assert!(set_decision(TEXT, "one", "resolved", "", NOW, &plain()).is_err());
}

#[test]
fn unknown_slug_or_state_is_an_error() {
    assert!(set_decision(TEXT, "nine", "approved", "", NOW, &plain()).is_err());
    assert!(set_decision(TEXT, "one", "maybe", "", NOW, &plain()).is_err());
}

#[test]
fn regroups_when_the_file_has_a_register_region() {
    let text = "<!-- register:start -->\n### T\n\n- [ ] **a** — 1\n- [ ] **b** — 2\n<!-- register:end -->\n";
    let out = set_decision(text, "a", "approved", "", NOW, &plain()).unwrap();
    assert!(
        out.find("**b**").unwrap() < out.find("**a**").unwrap(),
        "{out}"
    );
    assert!(out.contains("## Needs your decision\n"), "{out}");
}

/// A note may hold lines that read as the register's own structure, a `## heading` or a `- [ ]`
/// card line: indented under its entry they are the note's words, so the card keeps them, every card
/// is still read, the register regroups as before, and a later write sees the same cards.
#[test]
fn a_note_with_a_heading_and_a_card_line_saves_reloads_and_regroups_as_typed() {
    let text = "<!-- register:start -->\n## Needs your decision\n\n### T\n\n- [ ] **a** — 1\n- [ ] **b** — 2\n<!-- register:end -->\n";
    let note = "## title\n- [ ] **b** — not a card\n\nAnd a paragraph.";
    let out = set_decision(text, "a", "question", note, NOW, &plain()).unwrap();
    assert!(
        out.contains("- [?] **a** — 1\n  [2026-09-26 09:15] owner · question — ## title\n    - [ ] **b** — not a card\n\n    And a paragraph.\n"),
        "{out}"
    );
    let cards = parse_decisions(&lines(&out));
    let read: Vec<_> = cards
        .iter()
        .map(|c| (c.slug.as_str(), c.group(&plain())))
        .collect();
    assert_eq!(read, [("b", Group::Decide), ("a", Group::Agent)], "{out}");
    let entry = cards[1].entries().pop().unwrap();
    assert_eq!(entry.text, note);
    assert_eq!(out.matches("## Needs your decision").count(), 1, "{out}");
    assert!(
        out.find("## Waiting on an agent").unwrap() > out.find("**b** — 2").unwrap(),
        "{out}"
    );
    // The same note again is a repeat, and the next verdict on the real b lands on b.
    assert!(repeat(&out, "a", "question", note, "2026-09-26 09:16"));
    let again = set_decision(&out, "b", "approved", "", NOW, &plain()).unwrap();
    assert!(
        again.contains("- [x] **b** — 2\n  [2026-09-26 09:15] owner · approved\n"),
        "{again}"
    );
    assert!(again.contains("    - [ ] **b** — not a card\n"), "{again}");
}

// AutoApprovedRenderTest: an auto-approved card is the owner's to overturn, never to set.

const AUTO: &str = "<!-- register:start -->\n## Auto-approved: spot-check\n\n### T\n\n- [a] **pic** — A lamp. *Fills:* X · lamp\n\
    \x20 [2026-09-27 10:00] agent — ![x](/x.png)\n  [2026-09-27 11:00] agent · auto-approved — A: bold outline\n\
    <!-- register:end -->\n";

#[test]
fn the_owner_cannot_set_it_from_the_app() {
    assert!(
        set_decision(
            AUTO,
            "pic",
            "auto-approved",
            "",
            "2026-09-27 12:00",
            &plain()
        )
        .is_err()
    );
}

#[test]
fn the_owner_can_overturn_it() {
    let out = set_decision(
        AUTO,
        "pic",
        "rejected",
        "turrets too small",
        "2026-09-27 12:00",
        &plain(),
    )
    .unwrap();
    assert!(out.contains("- [-] **pic**"), "{out}");
    assert!(
        out.contains("  [2026-09-27 12:00] owner · rejected — turrets too small"),
        "{out}"
    );
}

// A card inline in its document: a verdict is written where the card sits, and the register
// region the document also holds is regrouped without it.

#[test]
fn a_verdict_on_an_inline_card_is_written_in_place_and_leaves_the_prose_around_it() {
    let text = "# Lamps\n\n## How many\n\nThere are 141.\n\n- [ ] **lamp-count** — 141 lamps.\n  [2026-11-21 10:00] agent — Posted.\n\nThree sizes.\n\n## Finishes\n\nBrass.\n";
    let out = set_decision(
        text,
        "lamp-count",
        "approved",
        "B",
        "2026-11-21 11:00",
        &plain(),
    )
    .unwrap();
    assert_eq!(
        out,
        text.replace("- [ ] **lamp-count**", "- [x] **lamp-count**")
            .replace(
                "agent — Posted.\n",
                "agent — Posted.\n  [2026-11-21 11:00] owner · approved — B\n"
            )
    );
}

#[test]
fn a_verdict_on_an_inline_card_keeps_it_out_of_the_documents_register_region() {
    let text = "# M\n\n- [ ] **inline** — Above.\n\nWords.\n\n<!-- register:start -->\n### T\n\n- [ ] **held** — In the region.\n<!-- register:end -->\n";
    let out = set_decision(
        text,
        "inline",
        "rejected",
        "no",
        "2026-11-21 11:00",
        &plain(),
    )
    .unwrap();
    assert!(
        out.starts_with("# M\n\n- [-] **inline** — Above.\n  [2026-11-21 11:00] owner · rejected — no\n\nWords.\n\n<!-- register:start -->\n## Needs your decision\n"),
        "{out}"
    );
    assert_eq!(out.matches("**inline**").count(), 1);
    let cards = parse_decisions(&lines(&out));
    assert_eq!(cards[0].group(&plain()), Group::Agent);
    assert_eq!(cards[1].group(&plain()), Group::Decide);
}
