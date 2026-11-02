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

#[test]
fn at_end_of_file_without_trailing_newline() {
    assert_eq!(
        insert_comment("a", 1, "question", "why", "b").unwrap(),
        "a\n\n> [!question] why\n> b\n"
    );
}
