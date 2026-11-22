//! InsertCommentTest: a typed comment becomes a callout after the line it was left on.

use symphony::edit::{insert_comment, split_message};

fn split(message: &str) -> (String, String) {
    split_message(message).unwrap()
}

#[test]
fn inserts_callout_after_the_line() {
    let out = insert_comment(
        "# T\n\npara\n\nnext\n",
        3,
        "dislike",
        "Too grim",
        "Lighten it.\nPlease.",
    )
    .unwrap();
    assert_eq!(
        out,
        "# T\n\npara\n\n> [!dislike] Too grim\n> Lighten it.\n> Please.\n\nnext\n"
    );
}

#[test]
fn body_optional() {
    assert_eq!(
        insert_comment("a\n", 1, "like", "keep", "").unwrap(),
        "a\n\n> [!like] keep\n"
    );
}

#[test]
fn a_plain_comment_is_a_kind() {
    assert_eq!(
        insert_comment("a\n", 1, "comment", "why this?", "").unwrap(),
        "a\n\n> [!comment] why this?\n"
    );
}

#[test]
fn one_message_splits_into_title_and_body() {
    assert_eq!(
        split("  Too grim  \nLighten it.\n\nPlease.\n"),
        ("Too grim".into(), "Lighten it.\n\nPlease.".into())
    );
    assert_eq!(
        split("just one line"),
        ("just one line".into(), String::new())
    );
    assert_eq!(
        split("\n\nafter blanks\nmore"),
        ("after blanks".into(), "more".into())
    );
}

#[test]
fn an_empty_message_is_refused() {
    assert!(split_message(" \n ").is_err());
}

#[test]
fn rejects_unknown_kind_and_bad_line() {
    assert!(insert_comment("a\n", 1, "note", "t", "").is_err());
    assert!(insert_comment("a\n", 5, "like", "t", "").is_err());
}

/// A register between a paragraph and a note: its start marker on line 5, its group heading on 6,
/// its topic on 8, its card on 10-11 and its end marker on 12.
const REGISTER: &str = "# Decisions\n\nIntro.\n\n<!-- register:start -->\n## Needs your decision\n\n### Topic\n\n- [ ] **alpha** — First question.\n  [2026-11-14 09:00] agent — Posted.\n<!-- register:end -->\n\nA note.\n";

#[test]
fn a_comment_inside_a_register_is_refused_saying_where_it_goes() {
    for line in 5..=11 {
        let refused = insert_comment(REGISTER, line, "comment", "About this", "").unwrap_err();
        assert_eq!(
            refused.0,
            format!(
                "line {line} is inside the decision register (lines 5 to 12), which holds cards only: a comment there would stop every later verdict on its cards. Say it on the card itself (its Verdict / message), or comment on a block outside the register"
            )
        );
    }
}

#[test]
fn a_comment_before_or_after_a_register_is_written() {
    let before = insert_comment(REGISTER, 3, "comment", "Before", "").unwrap();
    assert!(
        before.contains("Intro.\n\n> [!comment] Before\n\n<!-- register:start -->"),
        "{before}"
    );
    // After the end marker: outside, and the register is as it was.
    let after = insert_comment(REGISTER, 12, "comment", "After", "").unwrap();
    assert!(
        after.contains("<!-- register:end -->\n\n> [!comment] After\n\nA note."),
        "{after}"
    );
    for line in [1, 2, 4, 13, 14] {
        assert!(
            insert_comment(REGISTER, line, "comment", "Outside", "").is_ok(),
            "line {line}"
        );
    }
}

#[test]
fn every_register_of_a_document_refuses_a_comment_and_an_unclosed_one_does_not() {
    let two = format!("{REGISTER}\n<!-- register:start -->\n### Other\n<!-- register:end -->\n");
    // The second register's start marker is line 16, its topic 17 and its end marker 18.
    assert!(insert_comment(&two, 16, "comment", "t", "").is_err());
    assert!(insert_comment(&two, 17, "comment", "t", "").is_err());
    assert!(insert_comment(&two, 15, "comment", "t", "").is_ok());
    assert!(insert_comment(&two, 18, "comment", "t", "").is_ok());
    // A start marker no end marker follows opens no register: no regroup reads what is after it.
    let unclosed = "<!-- register:start -->\n## Needs your decision\n";
    assert!(insert_comment(unclosed, 2, "comment", "t", "").is_ok());
}

#[test]
fn at_end_of_file_without_trailing_newline() {
    assert_eq!(
        insert_comment("a", 1, "question", "why", "b").unwrap(),
        "a\n\n> [!question] why\n> b\n"
    );
}

// A card inline in its document, between two paragraphs: a comment may follow it, never part it.

const INLINE: &str = "# T\n\npara\n\n- [ ] **lamp** — A lamp.\n  [2026-11-21 10:00] agent — Posted.\n\n    More of it.\n\nnext\n";

#[test]
fn a_comment_between_an_inline_cards_lines_is_refused_naming_the_card() {
    for line in [5, 6, 7] {
        let refused = insert_comment(INLINE, line, "comment", "why?", "").unwrap_err();
        assert_eq!(
            refused.0,
            format!(
                "line {line} is inside the card 'lamp' (lines 5 to 8): a comment there would cut the lines under it from the card. Say it on the card itself (its Verdict / message), or comment on a block outside the card"
            )
        );
    }
}

#[test]
fn a_comment_after_an_inline_cards_last_line_follows_the_card() {
    let out = insert_comment(INLINE, 8, "comment", "why?", "").unwrap();
    assert_eq!(
        out,
        INLINE.replace(
            "    More of it.\n",
            "    More of it.\n\n> [!comment] why?\n"
        )
    );
    // The card keeps every line it had.
    let card = &symphony::register::parse_decisions(&crate::lines(&out))[0];
    assert_eq!(card.raw.len(), 3);
}

#[test]
fn a_comment_on_the_paragraph_above_an_inline_card_is_taken() {
    let out = insert_comment(INLINE, 3, "comment", "why?", "").unwrap();
    assert!(
        out.starts_with("# T\n\npara\n\n> [!comment] why?\n\n- [ ] **lamp**"),
        "{out}"
    );
}

#[test]
fn a_comment_between_two_adjacent_inline_cards_is_taken() {
    let text = "- [ ] **one** — First.\n- [ ] **two** — Second.\n";
    assert_eq!(
        insert_comment(text, 1, "comment", "why?", "").unwrap(),
        "- [ ] **one** — First.\n\n> [!comment] why?\n\n- [ ] **two** — Second.\n"
    );
}
