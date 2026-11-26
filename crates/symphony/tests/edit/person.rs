//! A person a board named writes under their short name where the owner's lines say `owner`: a
//! card's verdict `ana · approved — A`, a card's message `ana — words`, and a comment named before
//! its title; anything that is no short name is refused, and nothing is written.

use symphony::callouts::parse_comments;
use symphony::edit::{
    add_message_by, insert_comment, insert_comment_by, is_repeat_decision_by, set_decision_by,
};
use symphony::register::{Decision, Who, parse_decisions};

use crate::common::plain;
use crate::lines;

const BASE: &str = "# D\n\n- [ ] **one** — first.\n- [ ] **two** — second.\n";
const AT: &str = "2026-11-23 18:00";

/// The words no person's line may carry: an agent's, the board's, the reader's unknown author, a
/// session's numbered handle, capitals, a space, nothing.
const NO_NAMES: [&str; 7] = ["agent", "board", "unknown", "fox-12", "Ana", "ana lima", ""];

fn one(text: &str) -> Decision {
    let decisions = parse_decisions(&lines(text));
    decisions.into_iter().find(|d| d.slug == "one").unwrap()
}

#[test]
fn a_persons_verdict_carries_their_name_and_reads_as_a_persons_line() {
    let text = set_decision_by(BASE, "one", "approved", "A", AT, &plain(), "ana").unwrap();
    assert!(
        text.contains("  [2026-11-23 18:00] ana · approved — A\n"),
        "{text}"
    );
    let card = one(&text);
    let entry = card.entries().pop().unwrap();
    assert_eq!((entry.who, entry.name.as_str()), (Who::Owner, "ana"));
    assert_eq!(entry.verdict.as_deref(), Some("approved"));
    // Hers again is a repeat; the owner's same words are not.
    assert!(is_repeat_decision_by(&text, "one", "approved", "A", AT, "ana").unwrap());
    assert!(!is_repeat_decision_by(&text, "one", "approved", "A", AT, "owner").unwrap());
    // The owner's verdict reads as it always has.
    let owner = set_decision_by(BASE, "one", "approved", "A", AT, &plain(), "owner").unwrap();
    assert!(owner.contains("] owner · approved — A\n"), "{owner}");
    assert_eq!(one(&owner).entries().pop().unwrap().name, "owner");
}

#[test]
fn a_persons_message_on_a_card_carries_their_name() {
    let text = add_message_by(BASE, "one", "Why this?", AT, "ana", None).unwrap();
    assert!(
        text.contains("  [2026-11-23 18:00] ana — Why this?\n"),
        "{text}"
    );
    let entry = one(&text).entries().pop().unwrap();
    assert_eq!((entry.who, entry.name.as_str()), (Who::Owner, "ana"));
    // An agent's stays an agent's.
    let agent = add_message_by(BASE, "one", "Done.", AT, "agent", None).unwrap();
    assert_eq!(one(&agent).entries().pop().unwrap().who, Who::Agent);
}

#[test]
fn a_word_that_is_no_persons_name_writes_no_line() {
    for by in NO_NAMES {
        let refused = set_decision_by(BASE, "one", "approved", "A", AT, &plain(), by).unwrap_err();
        assert!(
            refused.0.contains("is no person's short name"),
            "{by:?}: {}",
            refused.0
        );
        if by != "agent" {
            assert!(
                add_message_by(BASE, "one", "x", AT, by, None).is_err(),
                "{by:?}"
            );
        }
        assert!(
            insert_comment_by(BASE, 1, "comment", "T", "", by).is_err(),
            "{by:?}"
        );
    }
}

#[test]
fn a_persons_comment_names_them_before_its_title_and_the_owners_names_nobody() {
    let text = insert_comment_by("a\n", 1, "comment", "141 lamps", "Too many.", "ana").unwrap();
    assert_eq!(text, "a\n\n> [!comment] ana · 141 lamps\n> Too many.\n");
    let found = parse_comments(&text.lines().collect::<Vec<_>>());
    assert_eq!(
        (found[0].by.as_str(), found[0].title.as_str()),
        ("ana", "141 lamps")
    );
    let owner = insert_comment_by("a\n", 1, "comment", "141 lamps", "", "owner").unwrap();
    assert_eq!(
        owner,
        insert_comment("a\n", 1, "comment", "141 lamps", "").unwrap()
    );
    let found = parse_comments(&owner.lines().collect::<Vec<_>>());
    assert_eq!(
        (found[0].by.as_str(), found[0].title.as_str()),
        ("owner", "141 lamps")
    );
}
