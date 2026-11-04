//! ContinuationLinesTest and NothingLostTest: an agent post followed by a blank line and indented
//! table or image lines stays whole, and a post never drops a line it did not write.

use symphony::edit::{add_message, check_nothing_lost, set_decision};
use symphony::register::Who;

use crate::common::plain;
use crate::lines;

const NOW: &str = "2026-10-05 14:15";
const CARD: [&str; 10] = [
    "- [ ] **pilot** — Choose.",
    "  [2026-10-05 10:55] agent — Intro.",
    "  [2026-10-05 12:17] agent — **Exports.**",
    "",
    "    | A | B |",
    "    |---|---|",
    "    | 1 | 2 |",
    "",
    "    ![pic](/a.png)",
    "  [2026-10-05 12:17] agent — **Interpretation.** Text.",
];

fn text() -> String {
    format!(
        "# D\n\n<!-- register:start -->\n## Needs your decision\n\n{}\n<!-- register:end -->\n",
        CARD.join("\n")
    )
}

/// The lines from the card's own line on.
fn card_lines(out: &str) -> Vec<String> {
    let all = lines(out);
    let start = all.iter().position(|l| l.contains("**pilot**")).unwrap();
    all[start..].to_vec()
}

#[test]
fn a_verdict_keeps_every_line_in_order_and_appends_after() {
    let card = card_lines(&set_decision(&text(), "pilot", "pending", "", NOW, &plain()).unwrap());
    assert_eq!(card[1..CARD.len()].to_vec(), CARD[1..].to_vec());
    assert_eq!(card[CARD.len()], "  [2026-10-05 14:15] owner · pending");
}

#[test]
fn a_question_moves_the_card_and_keeps_every_line() {
    let card =
        card_lines(&set_decision(&text(), "pilot", "question", "Why?", NOW, &plain()).unwrap());
    assert_eq!(card[1..CARD.len()].to_vec(), CARD[1..].to_vec());
    assert_eq!(
        card[CARD.len()],
        "  [2026-10-05 14:15] owner · question — Why?"
    );
}

#[test]
fn a_chat_message_lands_after_the_continuation_lines() {
    let card = card_lines(&add_message(&text(), "pilot", "Hello", NOW, Who::Owner, None).unwrap());
    assert_eq!(card[..CARD.len()].to_vec(), CARD.to_vec());
    assert_eq!(card[CARD.len()], "  [2026-10-05 14:15] owner — Hello");
}

// NothingLostTest

const BEFORE: &str = "- [ ] **a** — x\n  [t] agent — one\n\n    | h |\n  [t] agent — two\n";

#[test]
fn a_mark_change_and_an_added_line_are_not_losses() {
    let after = BEFORE.replace("[ ]", "[x]") + "  [t2] owner · approved\n";
    check_nothing_lost(BEFORE, &after, "a").unwrap();
}

#[test]
fn a_dropped_continuation_line_is_refused_naming_the_card() {
    let refused =
        check_nothing_lost(BEFORE, "- [ ] **a** — x\n  [t] agent — one\n", "a").unwrap_err();
    assert!(
        regex::Regex::new(r"card 'a'.*\| h \|")
            .unwrap()
            .is_match(&refused.0),
        "{refused}"
    );
}

#[test]
fn a_write_that_drops_one_of_two_identical_lines_is_refused() {
    // Lines are counted, not only looked up: the one `+1` the owner left twice is two lines to keep.
    let before = "- [ ] **a** — x\n  [t] owner — +1\n  [t] owner — +1\n";
    let refused =
        check_nothing_lost(before, "- [x] **a** — x\n  [t] owner — +1\n", "a").unwrap_err();
    assert!(
        refused
            .0
            .contains("would drop 1 line(s) of card 'a' (first: '  [t] owner — +1')"),
        "{refused}"
    );
}

#[test]
fn a_dropped_card_is_refused() {
    assert!(check_nothing_lost(BEFORE, "# nothing\n", "a").is_err());
}
