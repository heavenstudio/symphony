//! The words of a person who may speak and may not direct agents, held for people until someone
//! who may passes them on (docs/product/analysis/users-and-identity.md, "Words without agent
//! reach", and step 5 of "The way from one owner").
//!
//! A held message is a row of [`crate::schema::HELD`]'s table, never a line of a thread: no read of
//! `message` (an agent's `show`, `inbox` and `wait`, the dispatcher's callbacks, `tasks api`, a
//! search, the timeline, the server's thread and feed, an older build) reads its words. The write
//! that holds it puts a line of the board's on the task's thread to `owner` in the same
//! transaction ([`MARKER`]), which says a message is held and nothing of it: an agent reading the
//! thread is shown it, the person at the machine is told by it as by any line to them, and a
//! person's line after it answers it. The row's key is that line's number, which passing names.
//!
//! Passing it on ([`crate::Store::pass`]) marks the row passed and, in the same transaction, puts
//! its words on the thread under its author's name, marked a person's, to whom it was written,
//! opening or joining the task's ask, as a person's message does: the passer goes first in its
//! text ([`passed_text`]), so the line carries both names and its first word is never `approve`.

use serde::{Deserialize, Serialize};

/// What the board's line for a held message says before its number (`#N`): no words and no
/// author of it.
pub const MARKER: &str = "a message is held for people: ";

/// A message held for people.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Held {
    /// The number of the board's line that says it is held, its key.
    pub id: i64,
    pub slug: String,
    /// The person who wrote it, by their short name.
    pub author: String,
    pub text: String,
    /// Whom it was written to: a role (`@executor`), a session's handle, or nobody.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub at: f64,
    /// Who passed it on, when, and the line it became: none while it is held.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passed: Option<Passed>,
}

/// A held message passed on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Passed {
    /// Who passed it: the owner, a person a board named, or a session.
    pub by: String,
    pub at: f64,
    /// The number of the line its words became.
    pub line: i64,
}

/// What a person's message on a task's thread became (`Store::say_or_hold_as`).
#[derive(Clone, Debug, PartialEq)]
pub enum Said {
    /// A line of the thread, as the owner's is.
    Line(crate::Message),
    /// Held for people: its writer may not direct agents.
    Held(Held),
}

/// Whether `row` may be a line a held message is of, the board's that says one is held or the
/// person's its words became: what a reader that draws them for people reads them for.
pub fn may_be_of_one(row: &crate::rows::MessageRow) -> bool {
    row.person || (row.recipient.as_deref() == Some("owner") && row.text.starts_with(MARKER))
}

/// The text of the line `text`, passed on by `by`, becomes: the passer first, so the line carries
/// both names, and so its first word is never `approve`, which an approve's reading takes
/// (`crate::stamps::read::approves_by`).
pub fn passed_text(by: &str, text: &str) -> String {
    format!("[passed on by {by}] {text}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_passed_line_starts_with_its_passer_never_with_the_words() {
        assert_eq!(
            passed_text("stefano", "approve 1a2b3c4"),
            "[passed on by stefano] approve 1a2b3c4"
        );
    }

    #[test]
    fn a_held_row_reads_back_as_written_and_refuses_a_field_it_does_not_know() {
        let held = Held {
            id: 7,
            slug: "t/a".into(),
            author: "ana".into(),
            text: "a picture, please".into(),
            to: None,
            at: 10.0,
            passed: Some(Passed {
                by: "owner".into(),
                at: 11.0,
                line: 9,
            }),
        };
        let json = serde_json::to_value(&held).expect("a held row serializes");
        assert!(json.get("to").is_none(), "{json}");
        assert_eq!(serde_json::from_value::<Held>(json).ok(), Some(held));
        let unknown = serde_json::json!({
            "id": 1, "slug": "t/a", "author": "ana", "text": "x", "at": 1.0, "more": true
        });
        assert!(serde_json::from_value::<Held>(unknown).is_err());
    }

    #[test]
    fn a_line_may_be_of_a_held_message_only_as_a_persons_or_the_boards_line_to_owner_saying_so() {
        let line = |recipient: Option<&str>, text: &str, person: bool| crate::rows::MessageRow {
            id: 1,
            slug: "t/a".into(),
            sender: "board".into(),
            recipient: recipient.map(String::from),
            kind: "msg".into(),
            text: text.into(),
            ts: 1.0,
            person,
        };
        let marker = format!("{MARKER}#4");
        for (row, may) in [
            (line(Some("owner"), &marker, false), true),
            (line(None, "[passed on by owner] Cut", true), true),
            (line(Some("@executor"), &marker, false), false),
            (line(None, &marker, false), false),
            (
                line(Some("owner"), &format!("said: {marker}"), false),
                false,
            ),
            (line(Some("owner"), "Ready", false), false),
        ] {
            assert_eq!(may_be_of_one(&row), may, "{row:?}");
        }
    }
}
