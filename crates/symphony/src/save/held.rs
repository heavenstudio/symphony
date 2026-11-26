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
//! it is now (the one block under its heading holding the same lines, else the one block anywhere
//! that does), refused in plain words with nothing written where several do, and at the end of its
//! section where none does ([`goes_after`]); a message on its card. Its page draws a callout where
//! its pass would write it then ([`drawn_held`]).
//! The save's ask and callback follow as the author's, and the row is marked passed once the file
//! holds the words.

use serde_json::{Value, json};
use symphony_board::held::{Block, MESSAGE, OnDocument, Place, passed_text};
use symphony_board::people::Power;

use super::{Failure, string};
use crate::register::{self, Who};
use crate::{clock, edit};

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

/// The block of `text`, the document as it is now, a passed callout left after `block` goes after,
/// by its key and last line: the one block under its heading that holds the same lines, else the
/// one block anywhere that does. A key's place under its heading moves with every block above it
/// there, so it never tells two such blocks apart: refused, with nothing written, where its lines
/// stand twice under its heading, or twice elsewhere and not under it.
///
/// Where its lines stand nowhere, its block changed or is gone, and it goes at the end of its
/// section (docs/product/analysis/users-and-identity.md, `held-callout-block-changed`): after the
/// last block under the heading it was left under, the key's (`section:ANCHOR`), that a callout
/// may follow ([`edit::comment_refused_after`]), a heading under it starting a section of its own;
/// else, that heading gone, after the document's last such block. Refused, with nothing written,
/// where the document holds none.
pub fn goes_after(text: &str, block: &Block) -> Result<(String, i64), Failure> {
    let blocks = crate::render::keyed_blocks(text);
    let same: Vec<&(String, crate::render::Block)> = (blocks.iter())
        .filter(|(_, found)| found.text == block.text)
        .collect();
    let mine: Vec<&(String, crate::render::Block)> = (same.iter().copied())
        .filter(|(key, _)| heading(key) == heading(&block.key))
        .collect();
    let found = match (mine.as_slice(), same.as_slice()) {
        ([one], _) | ([], [one]) => *one,
        (_, []) => return section_end(text, &blocks, heading(&block.key)),
        _ => {
            return Err(Failure {
                status: 409,
                error: "the block this comment was left after now stands in several places: \
                        nothing was written"
                    .into(),
            });
        }
    };
    Ok(after(found))
}

/// The held comment `held` as its document's page draws it: a callout under the block its pass
/// would write it after in `text`, the document as that page shows it ([`goes_after`]), else,
/// that refused or the document not read, under the block it was left after, by its key and line
/// then; a card's message on its card.
pub fn drawn_held(text: Option<&str>, held: OnDocument) -> symphony_api::HeldComment {
    let left = held.document.block;
    let now = (left.as_ref().zip(text)).and_then(|(block, text)| goes_after(text, block).ok());
    let (key, line) = match (now, left) {
        (Some((key, line)), _) => (Some(key), Some(line)),
        (None, left) => (
            left.as_ref().map(|block| block.key.clone()),
            left.map(|block| block.line),
        ),
    };
    symphony_api::HeldComment {
        id: held.id,
        author: held.author,
        kind: held.document.kind,
        text: held.text,
        card: held.document.card,
        key,
        line,
    }
}

/// The line a passed callout left after `block` goes after in `text`: [`goes_after`]'s block's last.
pub(super) fn found_again(text: &str, block: &Block) -> Result<i64, Failure> {
    goes_after(text, block).map(|(_, line)| line)
}

/// The last of `blocks`, those of `text`, under the heading `under` (`section:ANCHOR`) that a
/// callout may follow, else the document's last such: its key and last line.
fn section_end(
    text: &str,
    blocks: &[(String, crate::render::Block)],
    under: &str,
) -> Result<(String, i64), Failure> {
    let lines = crate::text::splitlines(text);
    let takes = |(_, block): &&(String, crate::render::Block)| {
        let end = i64::try_from(block.end).unwrap_or(i64::MAX);
        edit::comment_refused_after(&lines, end).is_none()
    };
    let last = (blocks.iter().rev())
        .filter(|(key, _)| heading(key) == under)
        .find(takes)
        .or_else(|| blocks.iter().rev().find(takes));
    last.map(after).ok_or_else(|| Failure {
        status: 409,
        error: "the block this comment was left after is gone, and no block of the document may \
                take it: nothing was written"
            .into(),
    })
}

/// A block's key and its last line, which a callout after it goes after.
fn after((key, block): &(String, crate::render::Block)) -> (String, i64) {
    (key.clone(), i64::try_from(block.end).unwrap_or(i64::MAX))
}

/// The heading a block's key (`section:ANCHOR:N`) sits under: the key without its place there.
fn heading(key: &str) -> &str {
    key.rsplit_once(':').map_or(key, |(heading, _)| heading)
}

/// Whether what a pass of `held` writes stands in `text`, the document as its page shows it, more
/// often than the rows passed into that file already (`passed`, those kept under its checkout,
/// where a pass keeps the row it wrote, `Store::pass_on_document_into`, and where a sweep moves a
/// landed worktree's, `Store::held_landed`) put there: a pass whose words went in and whose mark
/// did not, so the next press writes nothing again and marks the row (what it misses: the save's
/// comment where it is called).
/// Only what a pass writes counts, its author's words with a passer first
/// ([`symphony_board::held::passed_text`]), whoever passed them: a callout of its kind and author
/// whose title and body are those words, a message of its author on its card. The author's own
/// words, written as hers once she may direct agents, never do.
pub(super) fn written_already(text: &str, held: &OnDocument, passed: &[OnDocument]) -> bool {
    let place = &held.document;
    let before = (passed.iter())
        .filter(|row| {
            row.id != held.id
                && row.passed.is_some()
                && (&row.author, &row.text) == (&held.author, &held.text)
                && (&row.document.kind, &row.document.card) == (&place.kind, &place.card)
        })
        .count();
    standing(text, held) > before
}

/// How many times what a pass of `held` writes stands in `text`, whoever passed it: each read as
/// the save would write it, so the two are told apart by its words alone.
fn standing(text: &str, held: &OnDocument) -> usize {
    let words = passed_text(Who::Owner.as_str(), &held.text);
    let lines = crate::text::splitlines(text);
    match &held.document.card {
        None => {
            let wanted = edit::split_message(&words).ok().and_then(|(title, body)| {
                let kind = &held.document.kind;
                let one = edit::insert_comment_by("x\n", 1, kind, &title, &body, &held.author);
                crate::callouts::parse_comments(&crate::text::splitlines(&one.ok()?)).pop()
            });
            let Some(wanted) = wanted else {
                return 0;
            };
            (crate::callouts::parse_comments(&lines).iter())
                .filter(|found| {
                    (&found.kind, &found.by, &found.body)
                        == (&wanted.kind, &wanted.by, &wanted.body)
                        && passed_words(&found.title).is_some()
                        && passed_words(&found.title) == passed_words(&wanted.title)
                })
                .count()
        }
        Some(card) => {
            let one = format!("- [ ] **{card}** — x\n");
            let now = clock::now_stamp();
            let one = edit::add_message_by(&one, card, &words, &now, &held.author, None);
            let wanted = (one.ok())
                .and_then(|one| register::parse_decisions(&crate::text::splitlines(&one)).pop())
                .and_then(|card| card.entries().pop());
            let Some(wanted) = wanted else {
                return 0;
            };
            (register::parse_decisions(&lines).iter())
                .filter(|found| found.slug == *card)
                .flat_map(register::Decision::entries)
                .filter(|found| {
                    (&found.name, &found.verdict) == (&wanted.name, &None)
                        && passed_words(&found.text).is_some()
                        && passed_words(&found.text) == passed_words(&wanted.text)
                })
                .count()
        }
    }
}

/// The words a passed line carries after its passer (`[passed on by NAME] WORDS`,
/// [`symphony_board::held::passed_text`]): none for a line no pass wrote.
fn passed_words(line: &str) -> Option<&str> {
    let (_, words) = line.strip_prefix("[passed on by ")?.split_once("] ")?;
    Some(words)
}

/// The save a pass of `held` by `by` makes: the route its words take and its payload, as its
/// author's own save would send it, the passer first in its words.
pub(super) fn passed_save(held: &OnDocument, by: &str) -> (&'static str, Value) {
    let text = passed_text(by, &held.text);
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
    fn a_block_is_found_again_by_its_heading_and_lines_or_alone_by_its_lines() {
        let prose = |key: &str| block(key, 3, "Some prose.");
        // Two blocks hold the same lines under two headings: its heading decides.
        assert_eq!(ok(found_again(PAGE, &prose("section:topic:1"))), 3);
        assert_eq!(ok(found_again(PAGE, &prose("section:lamps:1"))), 7);
        // A block came above it under its heading, or above the other heading: found where it is
        // now, though its place under its heading moved.
        let grown = PAGE.replace("# Topic\n\n", "# Topic\n\nNew.\n\n");
        assert_eq!(ok(found_again(&grown, &prose("section:lamps:1"))), 9);
        assert_eq!(ok(found_again(&grown, &prose("section:topic:1"))), 5);
        // Lines that stand once are found by them alone, whatever the key.
        let other = block("section:gone:1", 9, "Other prose.");
        assert_eq!(
            ok(found_again(&grown, &other)),
            11,
            "the one block holding them"
        );
        // Under one heading X, A and B, A and B alike: a comment left after A keeps A's place,
        // which is B's once X is gone. Refused while they stand twice there, never put after B.
        let twins = "# Topic\n\nX.\n\nSame.\n\nSame.\n";
        let after_a = block("section:topic:2", 5, "Same.");
        let without_x = twins.replace("X.\n\n", "");
        for page in [twins, without_x.as_str()] {
            let twice = refused(found_again(page, &after_a));
            assert!(
                twice.error.contains("several places"),
                "{page}: {}",
                twice.error
            );
        }
        // B gone instead: A stands alone under its heading and is found.
        let alone = "# Topic\n\nX.\n\nSame.\n";
        assert_eq!(ok(found_again(alone, &after_a)), 5);
        // Its lines changed: after the last block under its heading, a heading under it its own.
        assert_eq!(
            ok(goes_after(
                PAGE,
                &block("section:topic:1", 3, "Some proze.")
            )),
            ("section:topic:1".into(), 3),
            "Lamps' blocks are under a heading of their own"
        );
        assert_eq!(
            ok(goes_after(
                PAGE,
                &block("section:lamps:1", 7, "Some proze.")
            )),
            ("section:lamps:2".into(), 9)
        );
        // Its lines stand twice under no block of its heading: refused.
        let twice = refused(found_again(PAGE, &prose("section:elsewhere:4")));
        assert_eq!(
            twice.error,
            "the block this comment was left after now stands in several places: nothing was \
             written"
        );
    }

    /// Where a callout left after `left`, its key and lines, in a document that no longer holds
    /// those lines goes in `page`: the key and last line of the block it goes after.
    fn changed(page: &str, left: &str) -> (String, i64) {
        ok(goes_after(page, &block(left, 3, "Words gone.")))
    }

    #[test]
    fn a_changed_blocks_comment_goes_at_its_sections_end_else_the_documents() {
        // Its heading gone, or renamed: after the document's last block.
        assert_eq!(
            changed(PAGE, "section:gone:1"),
            ("section:lamps:2".into(), 9)
        );
        let renamed = PAGE.replace("## Lamps", "## Lights");
        assert_eq!(
            changed(&renamed, "section:lamps:1"),
            ("section:lights:2".into(), 9)
        );
        // A heading standing twice is told apart by its count, as the page's keys are.
        let twice = "# Topic\n\n## Cards\n\nA.\n\n## Cards\n\nB.\n\nC.\n\n## End\n\nZ.\n";
        assert_eq!(
            changed(twice, "section:cards:1"),
            ("section:cards:1".into(), 5)
        );
        assert_eq!(
            changed(twice, "section:cards-2:1"),
            ("section:cards-2:2".into(), 11)
        );
        // Above the first heading, the blocks there are its section.
        let preamble = "Intro.\n\nMore.\n\n# Topic\n\nX.\n";
        assert_eq!(changed(preamble, "section::1"), ("section::2".into(), 3));
        assert_eq!(
            changed("# Topic\n\nX.\n", "section::1"),
            ("section:topic:1".into(), 3)
        );
    }

    #[test]
    fn a_changed_blocks_comment_passes_over_the_blocks_no_callout_may_follow() {
        // A register opens under Cards, its headings under it each their own, and closes before
        // After: no callout may follow a line inside it.
        let register = "# Topic\n\nSome prose.\n\n## Cards\n\n<!-- register:start -->\n\
                        ## Group\n\n### One\n\n- [ ] **one** — x.\n\n### Two\n\n\
                        - [ ] **two** — y.\n<!-- register:end -->\n\nAfter.\n";
        // Cards' last block opens the register: its heading takes the callout.
        assert_eq!(
            changed(register, "section:cards:1"),
            ("section:cards:0".into(), 5)
        );
        // One's every block is inside it: after the document's last block.
        assert_eq!(
            changed(register, "section:one:1"),
            ("section:two:3".into(), 19)
        );
        // Two's section runs on past the register's end to After, its last block.
        assert_eq!(
            changed(register, "section:two:1"),
            ("section:two:3".into(), 19)
        );
        // A document of no block takes none.
        for page in ["", "---\ntitle: x\n---\n"] {
            let none = refused(goes_after(page, &block("section:topic:1", 3, "X.")));
            assert_eq!(
                (none.status, none.error.as_str()),
                (
                    409,
                    "the block this comment was left after is gone, and no block of the document \
                     may take it: nothing was written"
                ),
                "{page:?}"
            );
        }
    }

    #[test]
    fn a_held_callout_is_drawn_where_its_pass_writes_it_else_where_it_was_left() {
        let drawn = |page: Option<&str>, row: OnDocument| {
            let held = drawn_held(page, row);
            (held.key, held.line)
        };
        let row = || held(None, None);
        let at = |key: &str, line: i64| (Some(key.to_string()), Some(line));
        assert_eq!(drawn(Some(PAGE), row()), at("section:topic:1", 3));
        let grown = PAGE.replace("# Topic\n\n", "# Topic\n\nNew.\n\n");
        assert_eq!(drawn(Some(&grown), row()), at("section:topic:2", 5));
        // Its lines changed: at its section's end.
        let other = OnDocument {
            document: Place {
                block: Some(block("section:lamps:2", 9, "Other prose.")),
                ..row().document
            },
            ..row()
        };
        let edited = PAGE.replace("Other prose.", "Other proze.\n\nLast.");
        assert_eq!(drawn(Some(&edited), other), at("section:lamps:3", 11));
        // Standing twice under its heading, or the document unread: where it was left.
        let twice = PAGE.replace(
            "# Topic\n\nSome prose.",
            "# Topic\n\nSome prose.\n\nSome prose.",
        );
        for page in [Some(twice.as_str()), None] {
            assert_eq!(drawn(page, row()), at("section:topic:1", 3), "{page:?}");
        }
        let message = drawn_held(Some(PAGE), held(Some("one"), None));
        assert_eq!(
            message,
            symphony_api::HeldComment {
                id: 4,
                author: "ana".into(),
                kind: MESSAGE.into(),
                text: "Lamps\nToo many".into(),
                card: Some("one".into()),
                key: None,
                line: None,
            }
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

    /// [`PAGE`] with the callout of `kind` by `by`, its title `title`, after its first prose.
    fn commented(page: &str, kind: &str, by: &str, title: &str, body: &str) -> String {
        edit::insert_comment_by(page, 3, kind, title, body, by).unwrap()
    }

    #[test]
    fn a_callout_a_pass_wrote_stands_once_more_than_the_rows_passed_before_it_put_there() {
        let mine = held(None, None);
        let passed = |by: &str| format!("[passed on by {by}] Lamps");
        let wrote = |page: &str| commented(page, "comment", "ana", &passed("bo"), "Too many");
        assert!(!written_already(PAGE, &mine, &[]), "nothing written yet");
        assert!(
            written_already(&wrote(PAGE), &mine, &[]),
            "its mark never followed"
        );
        let by_owner = commented(PAGE, "comment", "ana", &passed("owner"), "Too many");
        assert!(written_already(&by_owner, &mine, &[]), "whoever passed it");
        // Its own row counts for nothing; another like it, passed, put one there.
        let itself = held(None, Some("bo"));
        assert!(written_already(&wrote(PAGE), &mine, &[itself]));
        let other = OnDocument {
            id: 5,
            ..held(None, Some("bo"))
        };
        let before = std::slice::from_ref(&other);
        assert!(!written_already(&wrote(PAGE), &mine, before));
        assert!(written_already(&wrote(&wrote(PAGE)), &mine, before));
        // A row unpassed, or of other words, author, kind or card, put none there.
        for row in [
            OnDocument {
                id: 5,
                ..held(None, None)
            },
            OnDocument {
                id: 5,
                text: "Lamps".into(),
                ..held(None, Some("bo"))
            },
            OnDocument {
                id: 5,
                author: "cy".into(),
                ..held(None, Some("bo"))
            },
            OnDocument {
                id: 5,
                ..held(Some("one"), Some("bo"))
            },
        ] {
            assert!(
                written_already(&wrote(PAGE), &mine, std::slice::from_ref(&row)),
                "{row:?}"
            );
        }
        // Her own words, written as hers, and lines no pass of hers wrote, are none of its.
        for (what, page) in [
            (
                "her own",
                commented(PAGE, "comment", "ana", "Lamps", "Too many"),
            ),
            (
                "its kind",
                commented(PAGE, "question", "ana", &passed("bo"), "Too many"),
            ),
            (
                "its author",
                commented(PAGE, "comment", "cy", &passed("bo"), "Too many"),
            ),
            (
                "its body",
                commented(PAGE, "comment", "ana", &passed("bo"), "Too few"),
            ),
            (
                "its title",
                commented(
                    PAGE,
                    "comment",
                    "ana",
                    "[passed on by bo] Lights",
                    "Too many",
                ),
            ),
            (
                "its body gone",
                commented(PAGE, "comment", "ana", &passed("bo"), ""),
            ),
        ] {
            assert!(!written_already(&page, &mine, &[]), "{what}: {page}");
        }
        // A row of a kind no save writes, or of no name, stands nowhere.
        let place = |kind: &str| Place {
            kind: kind.into(),
            ..mine.document.clone()
        };
        let unwritable = [
            OnDocument {
                document: place("nonsense"),
                ..mine.clone()
            },
            OnDocument {
                author: "Ana Lima".into(),
                ..mine.clone()
            },
        ];
        for row in unwritable {
            assert!(!written_already(&wrote(PAGE), &row, &[]), "{row:?}");
        }
    }

    #[test]
    fn a_cards_message_a_pass_wrote_stands_on_its_card_alone() {
        let mine = held(Some("one"), None);
        let cards = "# Topic\n\n- [ ] **one** — proposal.\n- [ ] **two** — another.\n";
        let said = |card: &str, by: &str, words: &str| {
            edit::add_message_by(cards, card, words, "2026-11-25 10:00", by, None).unwrap()
        };
        let passed = passed_text("owner", &mine.text);
        assert!(!written_already(cards, &mine, &[]));
        assert!(written_already(&said("one", "ana", &passed), &mine, &[]));
        let by_bo = passed_text("bo", &mine.text);
        assert!(written_already(&said("one", "ana", &by_bo), &mine, &[]));
        let before = OnDocument {
            id: 5,
            ..held(Some("one"), Some("bo"))
        };
        assert!(!written_already(
            &said("one", "ana", &passed),
            &mine,
            &[before]
        ));
        for (what, page) in [
            ("her own", said("one", "ana", &mine.text)),
            ("another card", said("two", "ana", &passed)),
            ("another author", said("one", "cy", &passed)),
        ] {
            assert!(!written_already(&page, &mine, &[]), "{what}: {page}");
        }
    }

    #[test]
    fn a_cards_message_of_an_author_no_line_can_carry_stands_nowhere() {
        let mine = held(Some("one"), None);
        let cards = "# Topic\n\n- [ ] **one** — proposal.\n";
        let words = passed_text("owner", &mine.text);
        let page = edit::add_message_by(cards, "one", &words, "2026-11-25 10:00", "ana", None);
        let page = page.unwrap();
        assert!(written_already(&page, &mine, &[]));
        // A name no line carries, as no save writes one: its pass would be refused, never marked.
        for author in ["Ana Lima", ""] {
            let nameless = OnDocument {
                author: author.into(),
                ..mine.clone()
            };
            assert!(!written_already(&page, &nameless, &[]), "{author:?}");
        }
    }

    #[test]
    fn a_passed_lines_words_follow_its_passer_and_no_other_line_has_any() {
        assert_eq!(passed_words("[passed on by bo] Lamps"), Some("Lamps"));
        assert_eq!(passed_words(&passed_text("owner", "a] b")), Some("a] b"));
        for line in [
            "Lamps",
            "[passed on by bo]Lamps",
            "passed on by bo] Lamps",
            "",
        ] {
            assert_eq!(passed_words(line), None, "{line}");
        }
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
