//! A card handed on, as its save edits the file before the board records it ([`super::handed`]):
//! its first line written for the new name and nothing else, and each refusal before anything is
//! written.

use super::*;

const PAGE: &str = "# Topic\n\n- [ ] **one** — proposal.\n  [2026-11-24 10:00] owner — Why?\n- [x] **two** — *For:* design. Done.\n- [?] **three** — Which?\n";

fn hand(slug: &str, to: &str) -> (String, Handing) {
    tried(slug, to).unwrap_or_else(|failure| panic!("refused: {}", failure.error))
}

fn tried(slug: &str, to: &str) -> Result<(String, Handing), Failure> {
    handed("docs/design/TOPIC.md", PAGE, slug, &json!({"to": to}))
}

fn refused(slug: &str, to: &str) -> String {
    match tried(slug, to) {
        Ok((text, _)) => panic!("handed: {text}"),
        Err(failure) => failure.error,
    }
}

#[test]
fn a_waiting_card_is_written_for_its_new_name_and_only_its_first_line_changes() {
    let (text, hand) = hand("one", " engineering ");
    assert_eq!(
        text,
        PAGE.replace(
            "**one** — proposal.",
            "**one** — *For:* engineering. proposal."
        )
    );
    assert_eq!(hand.to, "engineering");
    assert_eq!(hand.written, None);
    assert_eq!(
        hand.thing,
        symphony_board::people::ownership::Thing::Card {
            slug: "one".into(),
            route: "docs/design/TOPIC.md".into()
        }
    );
    let (text, hand) = super::hand_tests::hand("three", "owner");
    assert!(
        text.contains("- [?] **three** — *For:* owner. Which?"),
        "{text}"
    );
    assert_eq!(
        hand.written, None,
        "a question waits on its answer, so is handed"
    );
}

#[test]
fn a_card_missing_answered_or_handed_to_no_name_is_refused() {
    assert_eq!(refused("four", "design"), "no decision with slug 'four'");
    assert_eq!(
        refused("two", "engineering"),
        "the card two is approved: only a card that waits on an answer is handed on"
    );
    for name in ["", "Design", "agent", "two words"] {
        assert!(
            refused("one", name)
                .starts_with("a card is handed to a group's or a person's short name"),
            "{name:?}"
        );
    }
}
