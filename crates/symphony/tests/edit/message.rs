//! MessageTest and AgentReplyTest: a chat line under a card, the owner's or an agent's, and
//! `just symphony-reply`.

use symphony::edit::add_message;
use symphony::register::{State, Who, parse_decisions};

use crate::common::{Tree, plain};
use crate::lines;

const TEXT: &str = "# D\n\n- [-] **one** — first.\n  [2026-09-24 10:00] owner · rejected — old note\n- [x] **two** — second.\n";
const NOW: &str = "2026-09-26 09:15";

fn owner(text: &str, slug: &str, message: &str) -> String {
    add_message(text, slug, message, NOW, Who::Owner, None).unwrap()
}

#[test]
fn appends_an_owner_line_without_a_verdict_and_keeps_the_mark() {
    let out = owner(TEXT, "one", "What if it were smaller?");
    assert_eq!(
        lines(&out)[2..6].to_vec(),
        vec![
            "- [-] **one** — first.",
            "  [2026-09-24 10:00] owner · rejected — old note",
            "  [2026-09-26 09:15] owner — What if it were smaller?",
            "- [x] **two** — second.",
        ]
    );
}

#[test]
fn a_multi_line_message_continues_indented() {
    assert!(
        owner(TEXT, "two", "First.\nSame paragraph.")
            .contains("  [2026-09-26 09:15] owner — First.\n    Same paragraph.\n")
    );
}

/// A single Enter is a line break and a blank line a paragraph gap: the file keeps both, blank lines
/// in a run kept as one and none before or after the words, and the entry reads them back.
#[test]
fn a_messages_line_breaks_and_paragraph_gaps_are_kept_in_the_file_and_its_entry() {
    for who in [Who::Owner, Who::Agent] {
        let out = add_message(
            TEXT,
            "two",
            "\n\n  First line.\nSecond line.  \n\n\n\nNext paragraph.\n\n",
            NOW,
            who,
            None,
        )
        .unwrap();
        assert!(
            out.ends_with(&format!(
                "- [x] **two** — second.\n  [2026-09-26 09:15] {} — First line.\n    Second line.\n\n    Next paragraph.\n",
                who.as_str()
            )),
            "{out}"
        );
        let card = parse_decisions(&lines(&out))
            .into_iter()
            .find(|d| d.slug == "two")
            .unwrap();
        assert_eq!(
            card.entries().last().unwrap().text,
            "First line.\nSecond line.\n\nNext paragraph."
        );
        // the card still ends where it did: nothing after it is drawn into it
        let more = add_message(&out, "one", "Later.", NOW, Who::Owner, None).unwrap();
        let cards = parse_decisions(&lines(&more));
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[1].entries().len(), 1, "{more}");
    }
}

/// A review callout cannot live in a note: the page's comment reader would take it for a comment
/// on the document and archiving it would cut it from the card. In fenced code it is code.
#[test]
fn a_note_opening_a_review_callout_is_refused_and_one_in_code_is_kept() {
    for message in ["Look:\n> [!comment] this", "> [!Question] why"] {
        let refused = add_message(TEXT, "two", message, NOW, Who::Owner, None).unwrap_err();
        assert!(
            refused.0.starts_with("a note cannot hold a review callout"),
            "{refused}"
        );
        let verdict = symphony::edit::set_decision(TEXT, "two", "question", message, NOW, &plain());
        assert_eq!(verdict.unwrap_err(), refused);
    }
    let fenced = owner(
        TEXT,
        "two",
        "Like this:\n```\n> [!comment] this\n```\n> a quote",
    );
    assert!(
        fenced.contains("    > [!comment] this\n    ```\n    > a quote\n"),
        "{fenced}"
    );
}

#[test]
fn a_dated_looking_line_in_a_message_stays_in_that_message() {
    let out = owner(TEXT, "two", "Like this:\n[2026-09-20 08:00] agent — quoted");
    let card = parse_decisions(&lines(&out))
        .into_iter()
        .find(|d| d.slug == "two")
        .unwrap();
    let said: Vec<_> = card
        .entries()
        .into_iter()
        .map(|e| (e.ts, e.who, e.text))
        .collect();
    assert_eq!(
        said,
        vec![(
            Some(NOW.to_string()),
            Who::Owner,
            "Like this:\n[2026-09-20 08:00] agent — quoted".to_string()
        )]
    );
}

#[test]
fn the_message_is_waiting_for_an_agent_answer() {
    let out = owner(TEXT, "two", "Why this one?");
    let card = parse_decisions(&lines(&out))
        .into_iter()
        .find(|d| d.slug == "two")
        .unwrap();
    assert_eq!(card.state, State::Approved);
    assert_eq!(card.entries().last().unwrap().verdict, None);
    assert_eq!(card.unanswered(), vec!["Why this one?"]);
    assert!(card.open(&plain()));
}

#[test]
fn an_empty_message_or_unknown_slug_is_an_error() {
    assert!(add_message(TEXT, "one", "  \n ", NOW, Who::Owner, None).is_err());
    assert!(add_message(TEXT, "nine", "hello", NOW, Who::Owner, None).is_err());
}

// AgentReplyTest: an agent's answer on a card, an agent line appended after the card's last line.

const REPLY_TEXT: &str = "# D\n\n- [-] **one** — first.\n  [2026-09-24 10:00] owner · rejected — why\n- [x] **two** — second.\n";

fn reply(args: &[&str]) -> (i32, String, String) {
    let argv: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony::cli::reply_main(&argv, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn appends_an_agent_line_under_the_named_card() {
    let out = add_message(
        REPLY_TEXT,
        "one",
        "Redrawing it.\nSecond line.",
        "2026-09-26 09:15",
        Who::Agent,
        None,
    )
    .unwrap();
    assert_eq!(
        lines(&out)[2..7].to_vec(),
        vec![
            "- [-] **one** — first.",
            "  [2026-09-24 10:00] owner · rejected — why",
            "  [2026-09-26 09:15] agent — Redrawing it.",
            "    Second line.",
            "- [x] **two** — second.",
        ]
    );
}

#[test]
fn the_command_line_writes_the_reply_into_the_file() {
    let tree = Tree::new();
    let page = tree.write("x.md", REPLY_TEXT);
    let (code, out, err) = reply(&[
        page.to_str().unwrap(),
        "two",
        "Noted.",
        "--now",
        "2026-09-26 09:20",
    ]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, format!("replied on two in {}\n", page.display()));
    assert!(
        tree.read("x.md")
            .ends_with("- [x] **two** — second.\n  [2026-09-26 09:20] agent — Noted.\n")
    );
}

#[test]
fn an_agent_question_is_written_with_the_question_verdict() {
    let out = add_message(
        "- [x] **two** — second.\n",
        "two",
        "Which letter?",
        "2026-10-03 08:21",
        Who::Agent,
        Some("question"),
    )
    .unwrap();
    assert_eq!(
        out,
        "- [x] **two** — second.\n  [2026-10-03 08:21] agent · question — Which letter?\n"
    );
}

#[test]
fn the_command_line_asks_a_question_with_a_flag() {
    let tree = Tree::new();
    let page = tree.write("x.md", REPLY_TEXT);
    let (code, _, err) = reply(&[
        page.to_str().unwrap(),
        "two",
        "Which?",
        "--question",
        "--now",
        "2026-10-03 08:21",
    ]);
    assert_eq!(code, 0, "{err}");
    assert!(
        tree.read("x.md")
            .ends_with("  [2026-10-03 08:21] agent · question — Which?\n")
    );
}

#[test]
fn a_reply_to_a_missing_card_fails() {
    let tree = Tree::new();
    let page = tree.write("x.md", REPLY_TEXT);
    let (code, _, err) = reply(&[page.to_str().unwrap(), "nope", "Hi."]);
    assert_eq!(code, 1);
    assert_eq!(err, "✖ no decision with slug 'nope'\n");
    assert_eq!(tree.read("x.md"), REPLY_TEXT);
}

#[test]
fn a_reply_to_a_missing_file_fails_naming_it() {
    let tree = Tree::new();
    let page = tree.path("gone.md");
    let (code, _, err) = reply(&[page.to_str().unwrap(), "two", "Hi."]);
    assert_eq!(code, 1);
    assert_eq!(
        err,
        format!(
            "✖ [Errno 2] No such file or directory: '{}'\n",
            page.display()
        )
    );
}
