//! A comment on a document held for people, and its pass (docs/product/analysis/users-and-identity.md,
//! "Words without agent reach"; the board's row is `symphony_board::held::OnDocument`).
//!
//! A callout or a card's message from a person a board named who may not direct agents is checked
//! as the save would write it, against the document as its page shows it, and then kept as the
//! board's row alone: no file, no commit, no delivery, no ask and no chat callback carries it. A
//! callout keeps the block it was left after, by its key and its lines then.
//!
//! Its pass, by someone who may direct agents, writes it as its author's own comment would be
//! written, their name on it and the passer's first in its words
//! ([`symphony_board::held::passed_text`]): a callout after its block found again in the file as
//! it is now (the block of its key holding the same lines, else the one block anywhere that does),
//! refused in plain words with nothing written where none or several do; a message on its card.
//! The save's ask and callback follow as the author's, and the row is marked passed once the file
//! holds the words.

use serde_json::{Value, json};
use symphony_board::held::{Block, MESSAGE, OnDocument, Place};
use symphony_board::people::Power;

use super::{Failure, string};

/// Whether the comment or card message `name`, a person a board named, writes on the board `store`
/// is held for people: the board names someone and does not give them direct agents. A board that
/// names nobody holds nothing, and every save reads as before.
pub(super) fn holds(store: &symphony_board::Store, name: &str) -> Result<bool, Failure> {
    let people = store.people()?;
    Ok(!people.members.is_empty() && !people.powers_of(name).holds(Power::DirectAgents))
}

/// The words a held save of `route` keeps: a message's text, a comment's as typed, its title the
/// first line.
pub(super) fn words(route: &str, payload: &Value) -> String {
    if route == "/api/comment" && payload.get("text").is_none() {
        let (title, body) = (string(payload, "title"), string(payload, "body"));
        return if body.trim().is_empty() {
            title.to_string()
        } else {
            format!("{title}\n{body}")
        };
    }
    string(payload, "text").to_string()
}

/// Where a held save of `route` on the document `rel` of the checkout `scope` (empty: the
/// project's own) was left: a card's message on its card, a callout after the block of `text` (the
/// document as its page shows it) that ends on, or holds, the line `after` it went after.
pub(super) fn place(
    route: &str,
    scope: &str,
    rel: &str,
    text: &str,
    payload: &Value,
    after: i64,
) -> Result<Place, Failure> {
    let scope = Some(scope.to_string()).filter(|scope| !scope.is_empty());
    if route == "/api/message" {
        return Ok(Place {
            path: rel.into(),
            scope,
            kind: MESSAGE.into(),
            card: Some(string(payload, "slug").into()),
            block: None,
        });
    }
    let at = usize::try_from(after).unwrap_or(0);
    let blocks = crate::render::keyed_blocks(text);
    let left = (blocks.iter().find(|(_, block)| block.end == at))
        .or_else(|| (blocks.iter()).find(|(_, block)| (block.start..=block.end).contains(&at)))
        .ok_or_else(|| format!("no block of {rel} ends on line {after}: nothing was written"))?;
    Ok(Place {
        path: rel.into(),
        scope,
        kind: string(payload, "kind").into(),
        card: None,
        block: Some(Block {
            key: left.0.clone(),
            line: i64::try_from(left.1.end).unwrap_or(i64::MAX),
            text: left.1.text.clone(),
        }),
    })
}

/// The line a passed callout left after `block` goes after in `text`, the document as it is now:
/// the end of the block of its key that holds the same lines, else of the one block that does.
/// Refused, with nothing written, where no block does or several do and none has its key.
pub(super) fn found_again(text: &str, block: &Block) -> Result<i64, Failure> {
    let blocks = crate::render::keyed_blocks(text);
    let same: Vec<&(String, crate::render::Block)> = (blocks.iter())
        .filter(|(_, found)| found.text == block.text)
        .collect();
    let found = match (same.iter()).find(|(key, _)| *key == block.key) {
        Some(found) => found,
        None if same.len() == 1 => same[0],
        None if same.is_empty() => {
            return Err(Failure {
                status: 409,
                error: "the block this comment was left after has changed or is gone: nothing \
                        was written"
                    .into(),
            });
        }
        None => {
            return Err(Failure {
                status: 409,
                error: "the block this comment was left after now stands in several places: \
                        nothing was written"
                    .into(),
            });
        }
    };
    Ok(i64::try_from(found.1.end).unwrap_or(i64::MAX))
}

/// The save a pass of `held` by `by` makes: the route its words take and its payload, as its
/// author's own save would send it, the passer first in its words.
pub(super) fn passed_save(held: &OnDocument, by: &str) -> (&'static str, Value) {
    let text = symphony_board::held::passed_text(by, &held.text);
    let place = &held.document;
    match &place.card {
        Some(card) => (
            "/api/message",
            json!({"path": place.path, "slug": card, "text": text}),
        ),
        None => (
            "/api/comment",
            json!({"path": place.path, "kind": place.kind, "text": text,
                "after_line": place.block.as_ref().map(|block| block.line)}),
        ),
    }
}

/// A refused pass of the comment `id`, naming why: none held by it, or passed already.
pub(super) fn not_held(id: i64, held: Option<&OnDocument>) -> Option<Failure> {
    match held {
        None => Some(Failure {
            status: 404,
            error: format!("no comment #{id} is held on a document: nothing was written"),
        }),
        Some(OnDocument {
            passed: Some(passed),
            ..
        }) => Some(Failure {
            status: 409,
            error: format!(
                "comment #{id} was passed on already, by {}: nothing was written",
                passed.by
            ),
        }),
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_board::held::PassedOn;

    const PAGE: &str = "# Topic\n\nSome prose.\n\n## Lamps\n\nSome prose.\n\nOther prose.\n";

    fn block(key: &str, line: i64, text: &str) -> Block {
        Block {
            key: key.into(),
            line,
            text: text.into(),
        }
    }

    /// `answer`, or the test's failure saying why it was refused.
    fn ok<T>(answer: Result<T, Failure>) -> T {
        answer.unwrap_or_else(|failure| panic!("refused ({}): {}", failure.status, failure.error))
    }

    /// The refusal `answer` is.
    fn refused<T>(answer: Result<T, Failure>) -> Failure {
        match answer {
            Ok(_) => panic!("not refused"),
            Err(failure) => failure,
        }
    }

    fn comment(payload: Value, after: i64) -> Result<Place, Failure> {
        place("/api/comment", "", "docs/a.md", PAGE, &payload, after)
    }

    #[test]
    fn a_callout_is_left_after_the_block_that_ends_on_its_line_or_holds_it() {
        let kind = json!({"kind": "question"});
        let after = |line: i64| ok(comment(kind.clone(), line)).block.unwrap();
        assert_eq!(after(3), block("section:topic:1", 3, "Some prose."));
        assert_eq!(after(7), block("section:lamps:1", 7, "Some prose."));
        // A heading's line, and a blank one inside no block, find what ends or holds them.
        assert_eq!(after(5), block("section:lamps:0", 5, "## Lamps"));
        let missing = refused(comment(kind.clone(), 4));
        assert_eq!(
            missing.error,
            "no block of docs/a.md ends on line 4: nothing was written"
        );
        assert!(comment(kind.clone(), 0).is_err() && comment(kind, 99).is_err());
        let placed = ok(comment(json!({"kind": "decision"}), 9));
        assert_eq!(
            (placed.kind.as_str(), placed.card, placed.scope),
            ("decision", None, None)
        );
    }

    #[test]
    fn a_message_is_left_on_its_card_and_a_branchs_page_keeps_its_scope() {
        let payload = json!({"slug": "one", "text": "And why?"});
        let placed = ok(place(
            "/api/message",
            "feat~x",
            "docs/a.md",
            PAGE,
            &payload,
            0,
        ));
        assert_eq!(
            placed,
            Place {
                path: "docs/a.md".into(),
                scope: Some("feat~x".into()),
                kind: MESSAGE.into(),
                card: Some("one".into()),
                block: None,
            }
        );
    }

    #[test]
    fn a_block_is_found_again_by_its_key_and_lines_or_alone_by_its_lines() {
        let prose = |key: &str| block(key, 3, "Some prose.");
        // Two blocks hold the same lines: the key decides.
        assert_eq!(ok(found_again(PAGE, &prose("section:topic:1"))), 3);
        assert_eq!(ok(found_again(PAGE, &prose("section:lamps:1"))), 7);
        // A block came above the one under Lamps' heading: its key stands, found where it is now.
        let grown = PAGE.replace("# Topic\n\n", "# Topic\n\nNew.\n\n");
        assert_eq!(ok(found_again(&grown, &prose("section:lamps:1"))), 9);
        // Under its own heading the key moved with it, and its lines stand twice: refused rather
        // than put after the other.
        let moved = refused(found_again(&grown, &prose("section:topic:1")));
        assert!(moved.error.contains("several places"), "{}", moved.error);
        // Lines that stand once are found by them alone, whatever the key.
        let other = block("section:gone:1", 9, "Other prose.");
        assert_eq!(
            ok(found_again(&grown, &other)),
            11,
            "the one block holding them"
        );
        // Its lines changed, or stand twice under no block of its key: refused.
        let changed = refused(found_again(
            PAGE,
            &block("section:topic:1", 3, "Some proze."),
        ));
        assert_eq!(
            (changed.status, changed.error.as_str()),
            (
                409,
                "the block this comment was left after has changed or is gone: nothing was written"
            )
        );
        let twice = refused(found_again(PAGE, &prose("section:elsewhere:4")));
        assert_eq!(
            twice.error,
            "the block this comment was left after now stands in several places: nothing was \
             written"
        );
    }

    #[test]
    fn a_held_comments_words_are_its_text_or_its_title_and_body() {
        let text = json!({"text": "Title\nBody"});
        assert_eq!(words("/api/comment", &text), "Title\nBody");
        assert_eq!(words("/api/message", &json!({"text": "Hi"})), "Hi");
        let parts = json!({"title": "Title", "body": "Body"});
        assert_eq!(words("/api/comment", &parts), "Title\nBody");
        assert_eq!(
            words("/api/comment", &json!({"title": "T", "body": " "})),
            "T"
        );
    }

    fn held(card: Option<&str>, passed: Option<&str>) -> OnDocument {
        OnDocument {
            id: 4,
            document: Place {
                path: "docs/a.md".into(),
                scope: None,
                kind: if card.is_some() { MESSAGE } else { "comment" }.into(),
                card: card.map(String::from),
                block: card
                    .is_none()
                    .then(|| block("section:topic:1", 3, "Some prose.")),
            },
            author: "ana".into(),
            text: "Lamps\nToo many".into(),
            at: 1.0,
            passed: passed.map(|by| PassedOn {
                by: by.into(),
                at: 2.0,
            }),
        }
    }

    #[test]
    fn a_pass_saves_as_its_authors_comment_would_the_passer_first() {
        assert_eq!(
            passed_save(&held(None, None), "bo"),
            (
                "/api/comment",
                json!({"path": "docs/a.md", "kind": "comment", "after_line": 3,
                    "text": "[passed on by bo] Lamps\nToo many"})
            )
        );
        assert_eq!(
            passed_save(&held(Some("one"), None), "owner"),
            (
                "/api/message",
                json!({"path": "docs/a.md", "slug": "one",
                    "text": "[passed on by owner] Lamps\nToo many"})
            )
        );
    }

    #[test]
    fn a_pass_of_nothing_held_or_of_one_passed_already_is_refused() {
        let none = not_held(9, None).unwrap();
        assert_eq!(
            (none.status, none.error.as_str()),
            (
                404,
                "no comment #9 is held on a document: nothing was written"
            )
        );
        let passed = not_held(4, Some(&held(None, Some("bo")))).unwrap();
        assert_eq!(
            (passed.status, passed.error.as_str()),
            (
                409,
                "comment #4 was passed on already, by bo: nothing was written"
            )
        );
        assert!(not_held(4, Some(&held(None, None))).is_none());
    }
}
