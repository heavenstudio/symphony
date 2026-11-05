//! CommentParseTest: the owner's callouts, and taking the answered ones out to the ledger.

use symphony::callouts::{archive_callouts, ledger_entry, parse_comments};

#[test]
fn finds_a_callout_with_title_and_body() {
    let found = parse_comments(&[
        "# Doc",
        "",
        "> [!dislike] Too many lamps",
        "> 141 is a lot. Cut to 60.",
        "",
        "text",
    ]);
    assert_eq!(found.len(), 1);
    let comment = &found[0];
    assert_eq!(comment.kind, "dislike");
    assert_eq!(comment.line, 3);
    assert_eq!(comment.title, "Too many lamps");
    assert_eq!(comment.body, vec!["141 is a lot. Cut to 60."]);
    assert!(comment.open());
}

#[test]
fn kind_is_case_insensitive() {
    assert_eq!(parse_comments(&["> [!Question] why?"])[0].kind, "question");
}

#[test]
fn untitled_callout() {
    let found = parse_comments(&["> [!like]", "> keep this"]);
    assert_eq!(found[0].title, "");
    assert_eq!(found[0].body, vec!["keep this"]);
}

#[test]
fn other_callout_types_are_ignored() {
    assert!(parse_comments(&["> [!note] just a note", "> [!warning] careful"]).is_empty());
}

#[test]
fn plain_blockquote_is_ignored() {
    assert!(parse_comments(&["> the owner said:", "> something"]).is_empty());
}

#[test]
fn callout_inside_code_fence_is_ignored() {
    let found = parse_comments(&[
        "```",
        "> [!dislike] example syntax",
        "```",
        "> [!dislike] real",
    ]);
    assert_eq!(found.iter().map(|c| c.line).collect::<Vec<_>>(), vec![4]);
}

#[test]
fn answered_callouts_leave_the_text_and_open_ones_stay() {
    let text = "# T\n\npara\n\n> [!comment] Too long\n> Cut it.\n> > [!resolved] Cut to half.\n\n> [!question] Why?\n\nend\n";
    let (out, gone) = archive_callouts(text);
    assert_eq!(out, "# T\n\npara\n\n> [!question] Why?\n\nend\n");
    let gone: Vec<_> = gone
        .iter()
        .map(|c| {
            (
                c.kind.as_str(),
                c.title.as_str(),
                c.body.clone(),
                c.resolution.clone(),
            )
        })
        .collect();
    assert_eq!(
        gone,
        vec![(
            "comment",
            "Too long",
            vec!["Cut it.".to_string()],
            Some("Cut to half.".to_string())
        )]
    );
}

#[test]
fn a_like_is_kept_as_a_record_until_answered() {
    let text = "a\n\n> [!like] keep this\n";
    assert_eq!(archive_callouts(text), (text.to_string(), vec![]));
}

#[test]
fn callouts_in_code_fences_are_left_alone() {
    let text = "```markdown\n> [!comment] x\n> > [!resolved] y\n```\n";
    assert_eq!(archive_callouts(text), (text.to_string(), vec![]));
}

#[test]
fn ledger_entries_keep_where_and_what() {
    let (_, gone) =
        archive_callouts("> [!dislike] Grim\n> Lighten it.\n> > [!resolved] Lightened.\n");
    let entry = ledger_entry("docs/VISION.md", &gone[0], "2026-09-26 08:00");
    assert_eq!(
        entry,
        "## 2026-09-26 08:00 · docs/VISION.md\n\n> [!dislike] Grim\n> Lighten it.\n> > [!resolved] Lightened.\n"
    );
}

#[test]
fn nested_resolved_closes_the_comment() {
    let found = parse_comments(&[
        "> [!decision] Lamp 60 lamps, not 141",
        "> Trim the lineup.",
        "> > [!resolved] LAMP_CATALOGUE.md now lists 60.",
    ]);
    assert_eq!(found.len(), 1);
    assert!(!found[0].open());
    assert_eq!(
        found[0].resolution.as_deref(),
        Some("LAMP_CATALOGUE.md now lists 60.")
    );
}

#[test]
fn a_plain_comment_is_open_until_answered() {
    assert!(parse_comments(&["> [!comment] why this?"])[0].open());
    assert!(!parse_comments(&["> [!comment] why this?", "> > [!resolved] because"])[0].open());
}

#[test]
fn like_is_never_open() {
    assert!(!parse_comments(&["> [!like] nice"])[0].open());
}

#[test]
fn two_adjacent_callouts_stay_separate() {
    let found = parse_comments(&["> [!question] one", "> [!question] two"]);
    assert_eq!(
        found.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
        vec!["one", "two"]
    );
}

#[test]
fn body_stops_at_first_non_quote_line() {
    let found = parse_comments(&["> [!question] q", "> a", "b", "> not part of it"]);
    assert_eq!(found[0].body, vec!["a"]);
}

#[test]
fn indented_quote_marker_counts() {
    let found = parse_comments(&["  > [!dislike] indented", "  > body"]);
    assert_eq!(found[0].body, vec!["body"]);
}
