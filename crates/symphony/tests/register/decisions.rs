//! DecisionParseTest: a task item with a bold slug is a card; its mark is the verdict.

use symphony::register::State;

use crate::common::plain;
use crate::{card, cards};

#[test]
fn each_checkbox_state() {
    let found = cards(&[
        "- [ ] **pending-one** — Five civilizations.",
        "- [x] **approved-one** — Star lanes.",
        "- [X] **approved-two** — Capital X too.",
        "- [-] **rejected-one** — 141 lamps.",
        "- [?] **question-one** — Five station bands.",
    ]);
    assert_eq!(
        found.iter().map(|d| d.state).collect::<Vec<_>>(),
        vec![
            State::Pending,
            State::Approved,
            State::Approved,
            State::Rejected,
            State::Question
        ]
    );
    assert_eq!(found[0].slug, "pending-one");
    assert_eq!(found[0].text, "Five civilizations.");
    assert_eq!(found[0].line, 1);
}

#[test]
fn plain_tasks_without_a_bold_slug_are_not_decisions() {
    assert!(cards(&["- [ ] buy milk", "- [x] done thing"]).is_empty());
}

#[test]
fn unknown_marker_is_not_a_decision() {
    assert!(cards(&["- [/] **half** — in progress"]).is_empty());
}

#[test]
fn indented_lines_under_an_item_are_the_note() {
    let found = cards(&[
        "- [-] **too-many** — 141 lamps.",
        "  Show me 30 first.",
        "  Then we talk.",
        "- [ ] **next** — x",
    ]);
    assert_eq!(found[0].note, vec!["Show me 30 first.", "Then we talk."]);
    assert!(found[1].note.is_empty());
}

#[test]
fn note_stops_at_unindented_line() {
    assert_eq!(
        card(&["- [?] **q** — why", "  because", "prose", "  not a note"]).note,
        vec!["because"]
    );
}

#[test]
fn code_fence_is_ignored() {
    let found = cards(&[
        "```",
        "- [ ] **example** — syntax",
        "```",
        "- [ ] **real** — one",
    ]);
    assert_eq!(
        found.iter().map(|d| d.slug.as_str()).collect::<Vec<_>>(),
        vec!["real"]
    );
}

#[test]
fn open_is_rejected_or_question() {
    let found = cards(&[
        "- [ ] **a** — 1",
        "- [x] **b** — 2",
        "- [-] **c** — 3",
        "- [?] **d** — 4",
    ]);
    assert_eq!(
        found.iter().map(|d| d.open(&plain())).collect::<Vec<_>>(),
        vec![false, false, true, true]
    );
}

#[test]
fn an_owner_note_is_open_whatever_the_verdict() {
    let found = cards(&[
        "- [x] **a** — 1",
        "  at least 21, could vary",
        "- [ ] **b** — 2",
        "  thinking about it",
    ]);
    assert_eq!(
        found.iter().map(|d| d.open(&plain())).collect::<Vec<_>>(),
        vec![true, true]
    );
}

#[test]
fn an_agent_reply_after_the_last_note_closes_it() {
    let found = cards(&[
        "- [x] **a** — 1",
        "  at least 21",
        "  ↳ Lineup now says at least 21 per maker.",
        "- [-] **b** — 2",
        "  ↳ Replaced by **c**.",
        "- [?] **d** — 3",
        "  why?",
        "  ↳ Because.",
        "  still unclear",
    ]);
    assert_eq!(
        found.iter().map(|d| d.open(&plain())).collect::<Vec<_>>(),
        vec![false, false, true]
    );
}

#[test]
fn reply_is_reported_separately_from_the_note() {
    let found = card(&["- [x] **a** — 1", "  owner words", "  ↳ agent words"]);
    assert_eq!(found.note, vec!["owner words", "↳ agent words"]);
    assert_eq!(found.reply().as_deref(), Some("agent words"));
}
