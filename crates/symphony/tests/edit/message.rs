//! MessageTest and AgentReplyTest: a chat line under a card, the owner's or an agent's, and
//! `just symphony-reply`.

use symphony::edit::{add_message, resolve_comment};
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

/// A note's code that is never closed would make the file's readers take every later line for
/// code, a review comment below the card among them; one opened on the note's first line is not
/// a fence in the file, which holds that line on the entry's own, so its closing line would open
/// one. Both are refused, by the message and the verdict alike, and the file is left as it was.
#[test]
fn a_note_whose_code_is_left_open_or_opens_on_its_first_line_is_refused() {
    for (message, refusal) in [
        (
            "Look:\n```\nlet x = 1;",
            "a note's code fence is not closed",
        ),
        (
            "```\ncode\n> [!comment] sneaky",
            "a note cannot open with a code fence",
        ),
        ("```\ncode\n```", "a note cannot open with a code fence"),
        (
            "Two blocks:\n```\none\n```\n~~~\ntwo",
            "a note's code fence is not closed",
        ),
    ] {
        let refused = add_message(TEXT, "two", message, NOW, Who::Owner, None).unwrap_err();
        assert!(refused.0.starts_with(refusal), "{message:?}: {refused}");
        let verdict = symphony::edit::set_decision(TEXT, "two", "question", message, NOW, &plain());
        assert_eq!(verdict.unwrap_err(), refused, "{message:?}");
    }
    let closed = owner(TEXT, "two", "Look:\n```\nlet x = 1;\n```");
    assert!(
        closed.contains(" — Look:\n    ```\n    let x = 1;\n    ```\n"),
        "{closed}"
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

// A comment's answer: `L` and the line its callout opens on, the answer nested under its quote.

const COMMENTED: &str =
    "# D\n\n> [!dislike] Too many lamps\n> Fewer.\n\nAfter.\n\n  > [!question] Why blue?\n";

#[test]
fn a_comment_is_resolved_on_one_line_under_its_quote() {
    let (out, title) = resolve_comment(COMMENTED, 3, "Two now,\n  on the shelf.").unwrap();
    assert_eq!(title, "Too many lamps");
    assert_eq!(
        out,
        "# D\n\n> [!dislike] Too many lamps\n> Fewer.\n> > [!resolved] Two now, on the shelf.\n\nAfter.\n\n  > [!question] Why blue?\n"
    );
    // An indented callout keeps its indent; a callout right under another is not its quote.
    let (out, title) = resolve_comment(COMMENTED, 8, "Brand.").unwrap();
    assert_eq!(title, "Why blue?");
    assert!(
        out.ends_with("  > [!question] Why blue?\n  > > [!resolved] Brand.\n"),
        "{out}"
    );
    let stacked = "> [!dislike] A\n> [!dislike] B\n";
    let (out, _) = resolve_comment(stacked, 1, "Done.").unwrap();
    assert_eq!(
        out,
        "> [!dislike] A\n> > [!resolved] Done.\n> [!dislike] B\n"
    );
}

#[test]
fn a_line_with_no_open_comment_or_an_empty_answer_is_refused() {
    let resolved = resolve_comment(COMMENTED, 3, "Two.").unwrap().0;
    for (text, line, answer, said) in [
        (
            COMMENTED,
            4,
            "x",
            "line 4 opens no comment: name one by the line its `> [!kind]` callout is on (its open comments open on L3, L8)",
        ),
        (COMMENTED, 3, " \n ", "an answer needs some text"),
        (
            resolved.as_str(),
            3,
            "x",
            "the comment on line 3 is already resolved",
        ),
        (
            "> [!like] Nice\n",
            1,
            "x",
            "the comment on line 1 is a like: it waits on no answer",
        ),
        (
            "# D\n",
            1,
            "x",
            "line 1 opens no comment: name one by the line its `> [!kind]` callout is on (it has no open comment)",
        ),
    ] {
        assert_eq!(resolve_comment(text, line, answer).unwrap_err().0, said);
    }
}

#[test]
fn the_command_line_resolves_a_comment_by_its_line_and_refuses_a_question_on_one() {
    let tree = Tree::new();
    let page = tree.write("x.md", COMMENTED);
    let path = page.to_str().unwrap();
    let (code, _, err) = reply(&[path, "L3", "Which?", "--question"]);
    assert_eq!(
        (code, err.as_str()),
        (
            1,
            "✖ --question is for a card: an answer on a comment resolves it, so ask the owner on a card\n"
        )
    );
    assert_eq!(tree.read("x.md"), COMMENTED);
    let (code, out, err) = reply(&[path, "L3", "Two now."]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, format!("replied on L3 in {path}\n"));
    assert!(
        tree.read("x.md")
            .contains("> Fewer.\n> > [!resolved] Two now.\n")
    );
    let (code, _, err) = reply(&[path, "L3", "Again."]);
    assert_eq!(
        (code, err.as_str()),
        (1, "✖ the comment on line 3 is already resolved\n")
    );
}

/// `git ARGS` in `at`, which must succeed.
fn git(at: &std::path::Path, args: &[&str]) {
    let done = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(at)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap();
    assert!(
        done.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
}

#[test]
fn a_reply_in_a_checkout_records_who_wrote_it_for_the_owners_ask() {
    // The ask store is the one `ASKS_DB` names when it is set: a test writes only its own.
    if std::env::var_os("ASKS_DB").is_some() {
        return;
    }
    let tree = Tree::new();
    tree.write("docs/D.md", REPLY_TEXT);
    tree.write("docs/N.md", COMMENTED);
    git(&tree.root, &["init", "-q"]);
    git(&tree.root, &["add", "."]);
    git(&tree.root, &["commit", "-qm", "docs"]);
    let card = tree.path("docs/D.md");
    let (code, _, err) = reply(&[
        card.to_str().unwrap(),
        "one",
        "Redrawn.",
        "--as",
        "amber-heron",
    ]);
    assert_eq!((code, err.as_str()), (0, ""));
    let note = tree.path("docs/N.md");
    let (code, _, err) = reply(&[note.to_str().unwrap(), "L3", "Two.", "--as", "brisk-lark"]);
    assert_eq!((code, err.as_str()), (0, ""));
    let store =
        symphony::asks::AskStore::open(&symphony::asks::default_path(&tree.root, None)).unwrap();
    let said: Vec<_> = store
        .replies()
        .unwrap()
        .into_iter()
        .map(|r| (r.tree, r.path, r.kind, r.target, r.by, r.at > 0.0))
        .collect();
    let row = |path: &str, kind: &str, target: &str, by: &str| {
        (
            String::new(),
            path.to_string(),
            kind.to_string(),
            target.to_string(),
            by.to_string(),
            true,
        )
    };
    assert_eq!(
        said,
        [
            row("docs/D.md", "card", "one", "amber-heron"),
            row("docs/N.md", "comment", "Too many lamps", "brisk-lark"),
        ]
    );
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
