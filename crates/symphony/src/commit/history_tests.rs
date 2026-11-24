//! The history merge: append-only card histories resolved by slug when a text merge cannot.

use super::card_count_mismatch;

/// The merge of a project without plugins: no card kind reads these cards.
fn merge_history(current: &str, base: &str, new: &str) -> Option<String> {
    super::merge_history(current, base, new, &super::Plugins::default())
}

#[test]
fn card_occurrences_are_compared_per_slug_and_fenced_examples_do_not_count() {
    let base = "- [ ] **a** — A.\n- [ ] **b** — B.\n";
    assert_eq!(card_count_mismatch(base, base, base, base), Some(false));
    assert_eq!(
        card_count_mismatch(base, base, base, &format!("{base}- [x] **a** — A.\n")),
        Some(true)
    );
    assert_eq!(
        card_count_mismatch(base, base, base, "- [x] **a** — A.\n- [x] **a** — A.\n"),
        Some(true)
    );
    assert_eq!(
        card_count_mismatch(
            base,
            base,
            base,
            &format!("{base}```\n- [ ] **a** — Example.\n```\n")
        ),
        Some(false)
    );
    let duplicate_base = format!("{base}{base}");
    assert_eq!(
        card_count_mismatch(
            &duplicate_base,
            &duplicate_base,
            &duplicate_base,
            &duplicate_base
        ),
        Some(false)
    );
    let one = "- [ ] **a** — A.\n";
    let two = format!("{one}{one}");
    for (current, base, new, merged, expected) in [
        (one, "", "", one, Some(false)),
        ("", "", one, one, Some(false)),
        (one, "", one, one, Some(false)),
        ("", one, one, one, Some(true)),
        ("", one, one, "", Some(false)),
        (one, one, "", "", Some(false)),
        (two.as_str(), one, two.as_str(), two.as_str(), Some(false)),
        ("", one, two.as_str(), one, None),
    ] {
        assert_eq!(card_count_mismatch(current, base, new, merged), expected);
    }
}

#[test]
fn many_added_cards_keep_separators_but_cannot_hide_changed_prose_or_topics() {
    let base = "# Original prose\n\n- [ ] **c** — Existing.\n";
    let additions = (0..32)
        .map(|i| format!("\n\n- [ ] **new{i}** — New card {i}.\n    Proposal details {i}.\n"))
        .collect::<String>();
    let current = format!("{base}{additions}");
    let saved = format!("{base}  [2026-10-30 08:00] owner — Exact owner words.\n");
    assert_eq!(
        merge_history(&current, base, &saved),
        Some(format!("{saved}{additions}"))
    );
    assert!(
        merge_history(
            &current.replace("Original prose", "Changed prose"),
            base,
            &saved
        )
        .is_none()
    );
    let base =
        format!("<!-- register:start -->\n### Original topic\n\n{base}<!-- register:end -->\n");
    // Authored prose inside a register is refused rather than treated as generated headings.
    assert!(merge_history(&base, &base, &base).is_none());
    let base = "<!-- register:start -->\n### Original topic\n\n- [ ] **c** — Existing.\n<!-- register:end -->\n";
    let saved = base.replace(
        "<!-- register:end -->",
        "  [2026-10-30 08:00] owner — Exact owner words.\n<!-- register:end -->",
    );
    let changed = base.replace("Original topic", "Changed topic").replace(
        "<!-- register:end -->",
        &format!("{additions}<!-- register:end -->"),
    );
    assert!(merge_history(&changed, base, &saved).is_none());
}

#[test]
fn shared_additions_anchor_later_saved_lines_and_conflicting_orders_are_refused() {
    let base = "- [ ] **c** — Card.\n";
    let first = "  [2026-10-30 07:50] owner — First.\n";
    let second = "  [2026-10-30 06:00] owner — Second.\n";
    let reply = "  [2026-10-30 07:55] agent — Reply.\n";
    let saved = format!("{base}{first}{second}");
    assert_eq!(
        merge_history(&format!("{base}{first}{reply}"), base, &saved),
        Some(format!("{base}{first}{second}{reply}"))
    );
    assert!(merge_history(&format!("{base}{second}{first}"), base, &saved).is_none());
}

#[test]
fn only_additions_are_inserted_and_unterminated_entries_remain_separate() {
    let base = "- [ ] **card** — Question.\n  [2026-10-30 07:50] owner — First.\n  [2026-10-30 07:40] owner — Second.";
    let current = format!("{base}\n  [2026-10-30 07:55] agent — Reply.");
    let new = format!("{base}\n  [2026-10-30 07:45] owner — Addition.\n");
    assert_eq!(merge_history(&current, base, &new), Some("- [ ] **card** — Question.\n  [2026-10-30 07:45] owner — Addition.\n  [2026-10-30 07:50] owner — First.\n  [2026-10-30 07:40] owner — Second.\n  [2026-10-30 07:55] agent — Reply.".into()));
    let header = "- [ ] **card** — Question.";
    assert_eq!(
        merge_history(
            header,
            header,
            &format!("{header}\n  [2026-10-30 07:45] owner — Addition.\n")
        ),
        Some(format!(
            "{header}\n  [2026-10-30 07:45] owner — Addition.\n"
        ))
    );
}

#[test]
fn identical_events_are_shared_and_equal_minutes_keep_each_sides_order() {
    let base = "- [ ] **card** — Question.\n";
    let shared = "  [2026-10-30 07:38] owner · question — Same event.\n";
    let reply = "  [2026-10-30 07:38] agent — Reply.\n";
    let verdict = "  [2026-10-30 07:38] owner · approved — Yes.\n";
    let current = format!("{base}{shared}{reply}");
    let new = format!("{base}{shared}{shared}{verdict}");
    assert_eq!(
        merge_history(&current, base, &new),
        Some(format!("{base}{shared}{reply}{shared}{verdict}"))
    );
}

#[test]
fn ambiguous_marks_and_rewritten_save_history_are_refused() {
    let base = "- [ ] **card** — Question.\n  [2026-10-30 07:37] owner — Original.\n";
    let current = base.replace("[ ]", "[x]");
    assert!(merge_history(&current, base, &base.replace("[ ]", "[-]")).is_none());
    assert!(merge_history(base, base, &base.replace("Original.", "Rewritten.")).is_none());
    assert!(merge_history(base, base, &base.replace("Question.", "Rewritten card.")).is_none());
    let duplicate = format!("{base}{base}");
    assert!(merge_history(&duplicate, &duplicate, &duplicate).is_none());
    let later = "  [2026-10-30 07:38] agent — Later.\n";
    let with_later = format!("{base}{later}");
    let reordered = base.replace("Question.\n", &format!("Question.\n{later}"));
    assert!(merge_history(&reordered, &with_later, &with_later).is_none());
    // An unchanged or matching saved mark keeps main's verdict while adding the note.
    for saved_header in [base, current.as_str()] {
        assert_eq!(
            merge_history(&current, base, &format!("{saved_header}{later}")),
            Some(format!("{current}{later}"))
        );
    }
}

#[test]
fn fenced_examples_are_not_cards_and_a_delayed_event_sorts_before_existing_history() {
    let example = "```text\n- [ ] **card** — Example.\n```\n";
    let base =
        format!("{example}- [ ] **card** — Question.\n  [2026-10-30 07:40] owner — Later.\n");
    let delayed = "  [2026-10-30 07:38] agent — Delayed.\n";
    assert_eq!(
        merge_history(&base, &base, &format!("{base}{delayed}")),
        Some(base.replace(
            "  [2026-10-30 07:40]",
            &format!("{delayed}  [2026-10-30 07:40]")
        ))
    );
}

const NOTE: &str = "  [2026-10-30 07:50] owner — Note.\n";
const REPLY: &str = "  [2026-10-30 07:55] agent — Reply.\n";

#[test]
fn an_indented_fence_under_a_card_hides_no_later_card_from_the_merge() {
    // Unclosed, and closed around an example card: both are the card's thread.
    for thread in ["  ~~~\n", "  ```\n  - [ ] **inner** — Code.\n  ```\n"] {
        let base = format!("- [ ] **a** — A.\n{thread}- [ ] **b** — B.\n");
        assert_eq!(
            merge_history(&format!("{base}{REPLY}"), &base, &format!("{base}{NOTE}")),
            Some(format!("{base}{NOTE}{REPLY}"))
        );
    }
}

#[test]
fn a_fence_no_card_contains_still_hides_its_cards_from_the_merge() {
    let example = "~~~\n- [ ] **c** — Example.\n  [2026-10-30 07:00] agent — Code.\n~~~\n";
    let base = format!("- [ ] **c** — Card.\n{example}");
    assert_eq!(
        merge_history(
            &base.replace(example, &format!("{REPLY}{example}")),
            &base,
            &base.replace(example, &format!("{NOTE}{example}"))
        ),
        Some(base.replace(example, &format!("{NOTE}{REPLY}{example}")))
    );
}

#[test]
fn a_hidden_marker_inside_an_entry_joins_the_metadata_and_keeps_the_continuation() {
    let marker = "  <!-- review:unseen-owner k1 k2 -->\n";
    let header = "- [ ] **c** — Card.\n";
    let entry = "  [2026-10-30 07:40] agent — A\n";
    let base = format!("{header}{entry}{marker}    B\n");
    assert_eq!(
        merge_history(&format!("{base}{REPLY}"), &base, &format!("{base}{NOTE}")),
        Some(format!("{header}{marker}{entry}    B\n{NOTE}{REPLY}"))
    );
    // Outside an entry it is metadata already, where it stays.
    let base = format!("{header}{marker}{entry}");
    assert_eq!(
        merge_history(&format!("{base}{REPLY}"), &base, &format!("{base}{NOTE}")),
        Some(format!("{base}{NOTE}{REPLY}"))
    );
}

/// The merge of NOTE and REPLY appended after `base`, and the app's entries of its one card.
fn appended_and_read(base: &str, eol: &str) -> (Option<String>, Vec<String>) {
    let (note, reply) = (NOTE.replace('\n', eol), REPLY.replace('\n', eol));
    let merged = merge_history(&format!("{base}{reply}"), base, &format!("{base}{note}"));
    let lines = crate::text::splitlines(base);
    let read = crate::register::parse_decisions(&lines)[0].entries();
    (merged, read.into_iter().map(|e| e.text).collect())
}

#[test]
fn a_tabbed_continuation_stays_with_its_entry_in_the_merge_as_in_the_app() {
    let base = "- [ ] **c** — Card.\n  [2026-10-30 07:40] agent — A\n\t  B\n";
    let (merged, read) = appended_and_read(base, "\n");
    assert_eq!(merged, Some(format!("{base}{NOTE}{REPLY}")));
    assert_eq!(read, ["A\nB"]);
}

#[test]
fn crlf_lines_are_classified_like_lf_ones_and_keep_their_bytes() {
    let header = "- [ ] **c** — Card.\r\n";
    let marker = "  <!-- review:unseen-owner k1 k2 -->\r\n";
    let entry = "  [2026-10-30 07:40] agent — A\r\n";
    let base = format!("{header}{entry}{marker}    B\r\n");
    let (merged, read) = appended_and_read(&base, "\r\n");
    let (note, reply) = (NOTE.replace('\n', "\r\n"), REPLY.replace('\n', "\r\n"));
    assert_eq!(
        merged,
        Some(format!("{header}{marker}{entry}    B\r\n{note}{reply}"))
    );
    assert_eq!(read, ["A\nB"]);
}

#[test]
fn a_deeper_dated_looking_line_continues_its_entry_in_the_merge_as_in_the_app() {
    let base = "- [ ] **c** — Card.\n  [2026-10-30 07:40] agent — Example:\n    [2026-10-30 07:00] owner — Literal.\n";
    let (merged, read) = appended_and_read(base, "\n");
    assert_eq!(merged, Some(format!("{base}{NOTE}{REPLY}")));
    assert_eq!(read, ["Example:\n[2026-10-30 07:00] owner — Literal."]);
}

#[test]
fn unseen_reply_lookup_uses_the_last_duplicate_card_on_each_side() {
    let current = "- [ ] **a** — A.\n  [2026-10-20 11:00] agent — Existing answer.\n";
    let new = "- [ ] **a** — A.\n  [2026-10-20 10:00] owner — New question.\n";
    let merged = format!("{new}  [2026-10-20 11:00] agent — Existing answer.\n");
    let plugins = super::Plugins::default();
    let expected = super::mark_unseen_replies(current, new, &merged, &plugins)
        .unwrap_or_else(|_| panic!("valid histories must merge"));
    assert!(expected.contains("review:unseen-owner"));
    let earlier = "- [ ] **a** — Earlier duplicate.\n";
    let actual = super::mark_unseen_replies(
        &format!("{earlier}{current}"),
        &format!("{earlier}{new}"),
        &merged,
        &plugins,
    )
    .unwrap_or_else(|_| panic!("valid histories must merge"));
    assert_eq!(actual, expected);
}

#[test]
fn a_persons_words_a_reply_had_not_seen_are_marked_and_stay_unanswered_as_the_owners() {
    let plugins = super::Plugins::default();
    let current = "- [ ] **a** — A.\n  [2026-10-20 11:00] agent — Existing answer.\n";
    let mut marks = Vec::new();
    for name in ["owner", "ana"] {
        let new = format!("- [ ] **a** — A.\n  [2026-10-20 10:00] {name} — New question.\n");
        let merged = format!("{new}  [2026-10-20 11:00] agent — Existing answer.\n");
        let marked = super::mark_unseen_replies(current, &new, &merged, &plugins)
            .unwrap_or_else(|_| panic!("valid histories must merge"));
        let mark = (marked.lines())
            .find(|line| line.contains("review:unseen-owner"))
            .unwrap_or_else(|| panic!("{name}: {marked}"));
        marks.push(mark.to_string());
        let card = crate::register::parse_decisions(&crate::text::splitlines(&marked)).remove(0);
        assert_eq!(card.unanswered(), ["New question."], "{name}: {marked}");
    }
    // Each person's line is its own: the mark names hers apart from the owner's same words.
    assert_ne!(marks[0], marks[1]);
}

#[test]
fn duplicate_card_histories_on_any_side_are_refused() {
    let one = "- [ ] **a** — A.\n  [2026-10-20 10:00] agent — Existing answer.\n";
    let duplicate = format!("{one}{one}");
    assert_eq!(merge_history(&duplicate, one, one), None);
    assert_eq!(merge_history(one, &duplicate, one), None);
    assert_eq!(merge_history(one, one, &duplicate), None);
}

#[test]
fn canonical_register_keeps_missing_rank_then_rank_then_slug_and_all_card_bytes() {
    let head = "Intro.\n<!-- register:start -->\n";
    let tail = "<!-- register:end -->\nTrailing.\n";
    let slugs = ["z", "b", "a", "c"];
    let body = slugs
        .iter()
        .map(|slug| format!("- [ ] **{slug}** — Card {slug}.\n"))
        .collect::<String>();
    let text = format!("{head}### Topic\n\n{body}{tail}");
    let order =
        std::collections::BTreeMap::from([("a".into(), 1), ("b".into(), 1), ("c".into(), 0)]);
    let actual = super::canonical_register(&text, &order, &super::Plugins::default()).unwrap();
    let expected = format!(
        "{head}{}{tail}",
        ["z", "c", "a", "b"]
            .iter()
            .map(|slug| { format!("\n### Topic\n\n- [ ] **{slug}** — Card {slug}.\n\n") })
            .collect::<String>()
    );
    assert_eq!(actual, expected);
    let empty = format!("{head}{tail}");
    assert_eq!(
        super::canonical_register(&empty, &order, &super::Plugins::default()).unwrap(),
        empty
    );
}
