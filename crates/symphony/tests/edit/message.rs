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
        owner(TEXT, "two", "First.\n\nSecond.")
            .contains("  [2026-09-26 09:15] owner — First.\n    Second.\n")
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
            "✖ no document {}: reply names the document by its path from here, such as \
             docs/DECISIONS.md\n",
            page.display()
        )
    );
}
