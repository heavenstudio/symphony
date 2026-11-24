//! The Markdown syntax every reader of the docs shares, against the rules the docs are read by.

use symphony_docs::syntax::{
    CARD_LINE, LineIndex, RESERVED_NAMES, SHORT_NAME_MAX, WikiLink, anchors, card_end, card_line,
    carded, cards, code_ranges, components, entries, entry_line, heading, heading_anchor,
    is_continuation, is_indented, is_short_name, line_links, line_of, lines, links,
    mask_code_spans, pictures, said, said_line, unseen_owner, unseen_owner_line, wikilinks,
};

#[test]
fn lines_split_like_python_splitlines() {
    assert_eq!(lines("a\nb"), ["a", "b"]);
    assert_eq!(lines("a\r\nb\rc\x0cd"), ["a", "b", "c", "d"]);
    assert_eq!(lines("a\n"), ["a"]);
    assert_eq!(lines("a\n\n"), ["a", ""]);
    assert_eq!(lines("\u{2028}x"), ["", "x"]);
    assert_eq!(lines(""), Vec::<&str>::new());
}

#[test]
fn a_heading_is_one_to_six_hashes_then_whitespace() {
    assert_eq!(heading("## Part"), Some((2, "Part")));
    assert_eq!(heading("###### Six"), Some((6, "Six")));
    assert_eq!(heading("####### Seven"), None);
    assert_eq!(heading("#x"), None);
    assert_eq!(heading(" # indented"), None);
    assert_eq!(heading("plain"), None);
}

#[test]
fn a_heading_title_drops_closing_hashes_and_spaces() {
    assert_eq!(heading("## Foo ##"), Some((2, "Foo")));
    assert_eq!(heading("## Foo #  "), Some((2, "Foo")));
    assert_eq!(heading("## C#"), Some((2, "C")));
    assert_eq!(heading("## a # #"), Some((2, "a #")));
    assert_eq!(heading("##   Spaced title   "), Some((2, "Spaced title")));
}

#[test]
fn a_heading_of_only_spaces_or_hashes_keeps_one_character_as_the_regex_does() {
    // `\s+` gives its last character back to `(.+?)`, which needs one.
    assert_eq!(heading("#   "), Some((1, " ")));
    // `(.+?)` takes the first `#`, the closing `#*` the rest.
    assert_eq!(heading("# ##"), Some((1, "#")));
}

#[test]
fn the_anchor_rule_is_markdowns_toc_slugify() {
    assert_eq!(heading_anchor("Second"), "second");
    assert_eq!(heading_anchor("not a heading"), "not-a-heading");
    assert_eq!(
        heading_anchor("Weapons — and their mounts"),
        "weapons-and-their-mounts"
    );
    assert_eq!(heading_anchor("A  --  B"), "a-b");
    assert_eq!(heading_anchor("100% (done)!"), "100-done");
    assert_eq!(heading_anchor("under_score kept"), "under_score-kept");
    assert_eq!(heading_anchor("  Trimmed  "), "trimmed");
    assert_eq!(heading_anchor("AB-12 Name"), "ab-12-name");
    assert_eq!(heading_anchor("trailing -"), "trailing-");
}

#[test]
fn the_anchor_rule_folds_accents_and_drops_what_has_no_ascii() {
    assert_eq!(heading_anchor("Café déjà vu"), "cafe-deja-vu");
    assert_eq!(heading_anchor("Ünïcödé Ŝtar"), "unicode-star");
    assert_eq!(heading_anchor("a\u{a0}b"), "a-b");
    assert_eq!(heading_anchor("æther ß"), "ther");
    assert_eq!(heading_anchor("wait…"), "wait");
}

#[test]
fn the_anchor_rule_folds_compatibility_characters_as_nfkd_does() {
    assert_eq!(heading_anchor("ﬁle ﬂow ﬀ ﬃ ﬄ"), "file-flow-ff-ffi-ffl");
    assert_eq!(heading_anchor("½ step, ¼ and ¾"), "12-step-14-and-34");
    assert_eq!(heading_anchor("x² y³ z¹"), "x2-y3-z1");
    assert_eq!(heading_anchor("Star™"), "startm");
    assert_eq!(heading_anchor("ＡＢＣ　ｄｅｆ"), "abc-def");
    assert_eq!(heading_anchor("1º 2ª"), "1o-2a");
    assert_eq!(heading_anchor("a´b¨c"), "a-b-c");
    assert_eq!(heading_anchor("a\u{2003}b\u{202f}c"), "a-b-c");
    assert_eq!(heading_anchor("one‥two․"), "onetwo");
}

#[test]
fn a_hash_line_inside_a_fence_is_not_a_heading() {
    let text = "# One\n\n```bash\n# not a heading\n```\n\n  ~~~\n## also not\n~~~\n## Two\n";
    assert_eq!(anchors(text), ["one", "two"]);
}

#[test]
fn code_spans_are_masked_with_one_x_per_character() {
    assert_eq!(mask_code_spans("a `[x](m.md)` b"), "a xxxxxxxxxxx b");
    assert_eq!(mask_code_spans("[`é`](m.md)"), "[xxx](m.md)");
    assert_eq!(mask_code_spans("one ` unclosed"), "one ` unclosed");
}

#[test]
fn a_link_target_runs_to_the_bracket_or_a_title() {
    assert_eq!(
        line_links("see [two](two.md#second) and ![p](/a.png)"),
        ["two.md#second", "/a.png"]
    );
    assert_eq!(line_links(r#"[a](b.md "The title")"#), ["b.md"]);
    assert_eq!(line_links("[a](b c)"), Vec::<&str>::new());
    assert_eq!(line_links("[a] (b.md)"), Vec::<&str>::new());
    assert_eq!(line_links("[a] [b](c.md)"), ["c.md"]);
    assert_eq!(line_links("[a [b](c.md)"), ["c.md"]);
    assert_eq!(line_links("[a]()"), Vec::<&str>::new());
}

#[test]
fn links_skip_fences_and_code_but_not_code_link_text() {
    let text = "# T\n\n`[x](missing.md)` and [two](two.md).\n\n```\n[y](nope.md)\n```\n\nSee [`x`](code.md).\n";
    assert_eq!(
        links(text),
        [(3, "two.md".to_string()), (9, "code.md".to_string())]
    );
}

#[test]
fn code_ranges_cover_fenced_blocks_and_inline_spans() {
    let text = "a `b` c\n```\n[[x]]\n```\nd ``e`f`` g\n";
    let ranges: Vec<&str> = code_ranges(text)
        .into_iter()
        .map(|r| text.get(r).unwrap())
        .collect();
    assert_eq!(ranges, ["`b`", "```\n[[x]]\n```", "``e`f``"]);
}

#[test]
fn an_indented_or_unclosed_fence_is_not_a_block() {
    let text = "  ```\n[[x]]\n  ```\n";
    let ranges: Vec<&str> = code_ranges(text)
        .into_iter()
        .map(|r| text.get(r).unwrap())
        .collect();
    assert_eq!(ranges, ["``", "``"]);
    let unclosed = "~~~\n[[y]]\n";
    assert_eq!(code_ranges(unclosed), Vec::<std::ops::Range<usize>>::new());
}

#[test]
fn the_line_index_counts_lines_as_line_of_does() {
    for text in ["", "a", "a\n", "\n\nb\n", "é\nü\r\nx", "one\ntwo\nthree"] {
        let index = LineIndex::new(text);
        for at in 0..=text.len() {
            assert_eq!(index.line(at), line_of(text, at), "offset {at} of {text:?}");
        }
    }
}

#[test]
fn a_document_link_has_a_target_a_heading_and_a_label() {
    let found = wikilinks("See [[two#Second]] and\n[[ONE|the first]], [[task:sim/jump]].");
    let parts: Vec<_> = found
        .iter()
        .map(|w| {
            (
                w.line,
                w.target.as_str(),
                w.heading.as_deref(),
                w.label.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        parts,
        [
            (1, "two", Some("Second"), None),
            (2, "ONE", None, Some("the first")),
            (2, "task:sim/jump", None, None)
        ]
    );
    assert_eq!(found[0].whole, "[[two#Second]]");
}

#[test]
fn a_document_link_in_code_or_malformed_is_none() {
    assert_eq!(
        wikilinks("`[[two]]` and\n```\n[[three]]\n```\n"),
        Vec::<WikiLink>::new()
    );
    assert_eq!(
        wikilinks("[[]] [[a#]] [[a|]] [[a#|b]]"),
        Vec::<WikiLink>::new()
    );
    assert_eq!(wikilinks("[[[two]]]").len(), 1);
}

#[test]
fn a_component_has_a_lowercase_name_an_argument_and_an_option() {
    let found = components(
        "x [[item:AB-12|small]]\n\n[[slot:Owners · people, tools]] [[Item:X]] `[[item:Y]]`",
    );
    let parts: Vec<_> = found
        .iter()
        .map(|c| (c.line, c.name.as_str(), c.arg.as_str(), c.option.as_deref()))
        .collect();
    assert_eq!(
        parts,
        [
            (1, "item", "AB-12", Some("small")),
            (3, "slot", "Owners · people, tools", None)
        ]
    );
    assert_eq!(found[0].whole, "[[item:AB-12|small]]");
}

#[test]
fn an_empty_component_argument_still_parses() {
    let found = components("[[item-matrix:]] [[x:a|]]");
    let parts: Vec<_> = found
        .iter()
        .map(|c| (c.name.as_str(), c.arg.as_str(), c.option.as_deref()))
        .collect();
    assert_eq!(parts, [("item-matrix", "", None), ("x", "a", Some(""))]);
}

#[test]
fn a_card_line_is_a_task_item_with_a_bold_slug() {
    assert_eq!(
        card_line("- [x] **five-civs** — Five of them."),
        Some(('x', "five-civs", "Five of them."))
    );
    assert_eq!(card_line("- [ ] **a**"), Some((' ', "a", "")));
    assert_eq!(
        card_line("-  [?]  **b** -5 points"),
        Some(('?', "b", "5 points"))
    );
    assert_eq!(card_line("-[x] **a**"), None);
    assert_eq!(card_line("- [z] **a**"), None);
    assert_eq!(card_line("- [x] plain task"), None);
    assert_eq!(card_line("  - [x] **nested**"), None);
}

#[test]
fn the_card_line_pattern_takes_each_mark_card_line_reads_and_no_other() {
    let class = (CARD_LINE.split_once(r"\[["))
        .and_then(|(_, rest)| rest.split_once(r"]\]"))
        .map(|(class, _)| class)
        .expect("a bracket of marks");
    for mark in (' '..='~').chain(['é', '✓', '\u{a0}']) {
        assert_eq!(
            class.contains(mark),
            card_line(&format!("- [{mark}] **a**")).is_some(),
            "{mark:?}"
        );
    }
}

#[test]
fn a_cards_own_lines_are_told_from_the_prose_around_it() {
    let text = "para\n\n- [ ] **one** — first\n  [2026-09-24] agent — a\n\n    more\n\nprose\n```\n- [x] **fenced** — no\n```\n- [-] **two** — second\n- [ ] **three**\n  last\n";
    assert_eq!(
        carded(&lines(text)),
        [
            false, false, // para and the blank line under it
            true, true, true,
            true, // the card, its entry, the blank line inside it, its last line
            false, false, // the blank line after the card is no line of it, nor the prose
            false, false, false, // a card in a code block is code
            true, true, true, // two cards with no line between them, to the text's end
        ]
    );
    assert_eq!(carded(&lines("")), Vec::<bool>::new());
    assert_eq!(carded(&lines("no card\n")), [false]);
}

#[test]
fn cards_keep_their_lines_topic_and_skip_fences() {
    let text = "### Items\n\n- [ ] **one** — first\n  [2026-09-24] agent — a\n\n  [2026-09-24] owner · approved\nprose\n\n```\n- [x] **fenced** — no\n```\n- [-] **two** — second\n";
    let found = cards(&lines(text));
    let parts: Vec<_> = found
        .iter()
        .map(|c| {
            (
                c.line,
                c.slug.as_str(),
                c.state(),
                c.topic.as_str(),
                c.raw.len(),
            )
        })
        .collect();
    assert_eq!(
        parts,
        [
            (3, "one", "pending", "Items", 3),
            (12, "two", "rejected", "Items", 0)
        ]
    );
}

/// Each card's slug and the lines under it.
fn slugs_and_raw(text: &str) -> Vec<(String, Vec<String>)> {
    cards(&lines(text))
        .into_iter()
        .map(|card| (card.slug, card.raw))
        .collect()
}

#[test]
fn an_indented_fence_belongs_to_its_card_and_hides_no_later_card() {
    let unclosed = slugs_and_raw("- [x] **a**\n  ~~~\n- [x] **b**\n");
    assert_eq!(
        unclosed,
        [
            ("a".to_string(), vec!["  ~~~".to_string()]),
            ("b".to_string(), vec![])
        ]
    );
    let closed =
        slugs_and_raw("- [ ] **a**\n  ```\n  - [x] **inner** — code\n  ```\n\n- [ ] **b**\n");
    assert_eq!(
        closed
            .iter()
            .map(|(slug, raw)| (slug.as_str(), raw.len()))
            .collect::<Vec<_>>(),
        [("a", 3), ("b", 0)]
    );
}

#[test]
fn a_fence_no_card_contains_hides_the_cards_in_it() {
    let found = slugs_and_raw(
        "- [x] **a**\n  [2026-11-04] agent — A\n~~~\n- [x] **example**\n  [2026-11-04] agent — code\n~~~\n- [x] **b**\n",
    );
    assert_eq!(
        found,
        [
            (
                "a".to_string(),
                vec!["  [2026-11-04] agent — A".to_string()]
            ),
            ("b".to_string(), vec![])
        ]
    );
}

#[test]
fn a_card_ends_at_its_last_indented_line_across_blank_lines() {
    let page = [
        "- [x] **a**",
        "  one",
        "",
        "    two",
        "",
        "prose",
        "  later",
    ];
    assert_eq!(card_end(&page, 0), 4);
    assert_eq!(card_end(&page, 5), 7);
    assert_eq!(card_end(&["- [x] **a**"], 0), 1);
    assert!(is_indented("\tx") && !is_indented("x") && !is_indented("   "));
}

#[test]
fn a_hidden_unseen_owner_line_is_no_entry_and_breaks_no_continuation() {
    let card = cards(&lines(
        "- [x] **a**\n  [2026-11-04] agent — A\n  <!-- review:unseen-owner a b -->\n    B\n",
    ))
    .remove(0);
    let found: Vec<_> = card
        .entries()
        .into_iter()
        .map(|e| (e.ts, e.who, e.text))
        .collect();
    assert_eq!(
        found,
        [(Some("2026-11-04".to_string()), "agent", "A\nB".to_string())]
    );
}

#[test]
fn a_continuation_is_any_indent_deeper_than_two_characters() {
    assert!(is_continuation("   x") && is_continuation("\t  x") && is_continuation("\t\t\tx"));
    assert!(!is_continuation("  x") && !is_continuation("\tx") && !is_continuation("      "));
    assert!(is_continuation("    [2026-10-30 07:00] owner — quoted"));
}

#[test]
fn a_deeper_line_continues_a_dated_entry_tabbed_or_dated_looking() {
    let raw = [
        "  [2026-11-04] agent — Example:",
        "\t  B",
        "    [2026-10-30 07:00] owner — Literal example.",
        "  [2026-11-05] owner · approved",
        "    [2026-11-01] owner — under a dated entry, a continuation",
    ];
    let found: Vec<_> = entries(&raw)
        .into_iter()
        .map(|e| (e.ts, e.who, e.text))
        .collect();
    assert_eq!(
        found,
        [
            (
                Some("2026-11-04".to_string()),
                "agent",
                "Example:\nB\n[2026-10-30 07:00] owner — Literal example.".to_string()
            ),
            (
                Some("2026-11-05".to_string()),
                "owner",
                "[2026-11-01] owner — under a dated entry, a continuation".to_string()
            )
        ]
    );
    // Under no dated entry, a deeper dated line still starts one.
    let first = entries(&["    [2026-11-01] owner — first"]);
    assert_eq!(first[0].ts.as_deref(), Some("2026-11-01"));
}

#[test]
fn crlf_lines_read_as_lf_ones_hidden_marker_included() {
    let text = "- [x] **a**\r\n  [2026-11-04] agent — A\r\n  <!-- review:unseen-owner a b -->\r\n    B\r\n";
    let found: Vec<_> = cards(&lines(text))
        .remove(0)
        .entries()
        .into_iter()
        .map(|e| e.text)
        .collect();
    assert_eq!(found, ["A\nB"]);
}

#[test]
fn a_hidden_unseen_owner_line_outside_an_entry_is_skipped() {
    let raw = [
        "  <!-- review:unseen-owner k1 k2 -->",
        "  [2026-11-04] owner · approved — fine",
    ];
    let found: Vec<_> = entries(&raw)
        .into_iter()
        .map(|e| (e.who, e.verdict, e.text))
        .collect();
    assert_eq!(
        found,
        [("owner", Some("approved".to_string()), "fine".to_string())]
    );
    // deeper than the two spaces it is written with, it is no marker but an entry's words
    assert_eq!(unseen_owner("    <!-- review:unseen-owner k1 k2 -->"), None);
}

#[test]
fn the_unseen_owner_line_reads_back_its_two_keys() {
    let line = unseen_owner_line("owner:1", "reply:2");
    assert_eq!(line, "  <!-- review:unseen-owner owner:1 reply:2 -->\n");
    assert_eq!(unseen_owner(&line), Some(("owner:1", "reply:2")));
    assert_eq!(unseen_owner(line.trim_end()), Some(("owner:1", "reply:2")));
    assert_eq!(unseen_owner("  <!-- review:unseen-owner lonely -->"), None);
    assert_eq!(unseen_owner("  <!-- review:other a b -->"), None);
}

#[test]
fn an_entry_line_is_dated_and_signed() {
    let owner = entry_line("  [2026-09-24 14:02] owner · rejected — needs better names").unwrap();
    assert_eq!(
        (
            owner.ts.as_deref(),
            owner.who,
            owner.verdict.as_deref(),
            owner.text.as_str()
        ),
        (
            Some("2026-09-24 14:02"),
            "owner",
            Some("rejected"),
            "needs better names"
        )
    );
    let agent = entry_line("  [2026-09-24] agent — see **x**").unwrap();
    assert_eq!(
        (agent.who, agent.verdict.as_deref(), agent.text.as_str()),
        ("agent", None, "see **x**")
    );
    assert_eq!(
        entry_line("  [2026-09-24] owner · approved").unwrap().text,
        ""
    );
    assert_eq!(entry_line("[2026-09-24] agent — not indented"), None);
    assert_eq!(entry_line("  [2026-9-24] agent — bad date"), None);
    assert_eq!(entry_line("  [2026-09-24] agent · approved2"), None);
}

#[test]
fn entries_continue_dated_lines_and_read_answers_and_notes() {
    let raw: Vec<String> = [
        "  [2026-09-24] agent — first",
        "    more",
        "",
        "    next paragraph",
        "  ↳ an answer",
        "  a note",
    ]
    .map(str::to_string)
    .to_vec();
    let found = entries(&raw);
    let parts: Vec<_> = found.iter().map(|e| (e.who, e.text.as_str())).collect();
    assert_eq!(
        parts,
        [
            ("agent", "first\nmore\n\nnext paragraph"),
            ("agent", "an answer"),
            ("owner", "a note")
        ]
    );
}

#[test]
fn pictures_are_image_sources() {
    assert_eq!(
        pictures("A ![H item A](/a.png) B ![](/b.png) [not](/c.png) ![bad](d e)"),
        ["/a.png", "/b.png"]
    );
}

#[test]
fn the_readers_keep_whole_characters_around_text_outside_ascii() {
    let link = wikilinks("é [[café#séance|légende]] ü");
    assert_eq!(link.len(), 1);
    assert_eq!(link[0].whole, "[[café#séance|légende]]");
    assert_eq!(
        (
            link[0].target.as_str(),
            link[0].heading.as_deref(),
            link[0].label.as_deref()
        ),
        ("café", Some("séance"), Some("légende"))
    );
    let component = components("ü [[slot:Öl · tür|Größe]] é");
    assert_eq!(component.len(), 1);
    assert_eq!(component[0].whole, "[[slot:Öl · tür|Größe]]");
    assert_eq!(
        (component[0].arg.as_str(), component[0].option.as_deref()),
        ("Öl · tür", Some("Größe"))
    );
    assert_eq!(mask_code_spans("à `é` ü"), "à xxx ü");
    assert_eq!(
        line_links("ë [à](é.md) [ö](ü.md \"tïtle\")"),
        ["é.md", "ü.md"]
    );
    assert_eq!(pictures("![é](à.png) ![ü](ö.png)"), ["à.png", "ö.png"]);
    assert_eq!(card_line("- [x] **é** — ü"), Some(('x', "é", "ü")));
}

#[test]
fn a_date_cut_through_a_character_is_no_stamp() {
    // Sixteen bytes, the tenth inside the `é`: no whole date comes before the time.
    assert_eq!(entry_line("  [2026-09-2é 1:00] owner"), None);
    let whole = entry_line("  [2026-09-25 10:00] owner").unwrap();
    assert_eq!(
        (whole.ts.as_deref(), whole.who, whole.text.as_str()),
        (Some("2026-09-25 10:00"), "owner", "")
    );
}

#[test]
fn a_short_name_is_lowercase_parts_joined_by_hyphens_and_never_a_handle_or_a_reserved_word() {
    for name in [
        "ana",
        "ana-l",
        "a",
        "a1",
        "ana-lima-2b",
        "x2-y",
        &"a".repeat(SHORT_NAME_MAX),
    ] {
        assert!(is_short_name(name), "{name}");
    }
    for name in [
        "",
        "Ana",
        "ana_l",
        "ana.l",
        "-ana",
        "ana-",
        "ana--l",
        "1ana",
        "anà",
        "ana l",
        // A session's handle ends in a number alone.
        "amber-heron-12",
        "ana-2",
        &"a".repeat(SHORT_NAME_MAX + 1),
    ] {
        assert!(!is_short_name(name), "{name:?}");
    }
    for word in RESERVED_NAMES {
        assert!(!is_short_name(word), "{word}");
    }
    assert_eq!(RESERVED_NAMES, ["agent", "owner", "board", "unknown"]);
}

#[test]
fn a_dated_line_by_a_short_name_is_a_persons_entry_and_keeps_its_name() {
    let said = said_line("  [2026-11-23 18:00] ana · approved — A").unwrap();
    assert_eq!(said.name, "ana");
    assert_eq!(
        (
            said.entry.ts.as_deref(),
            said.entry.who,
            said.entry.verdict.as_deref()
        ),
        (Some("2026-11-23 18:00"), "owner", Some("approved"))
    );
    assert_eq!(said.entry.text, "A");
    // The entry is a person's, as the owner's always was.
    assert_eq!(
        entry_line("  [2026-11-23 18:00] ana · approved — A"),
        Some(said.entry)
    );
    for (line, name, who) in [
        ("  [2026-11-23] ana-l — why?", "ana-l", "owner"),
        ("  [2026-11-23] owner · rejected", "owner", "owner"),
        ("  [2026-11-23] agent — Posted.", "agent", "agent"),
    ] {
        let said = said_line(line).unwrap();
        assert_eq!((said.name.as_str(), said.entry.who), (name, who), "{line}");
    }
}

#[test]
fn a_dated_line_whose_word_is_no_name_is_no_entry() {
    for line in [
        // Reserved for the board's own lines and the vocabulary's unread author.
        "  [2026-11-23] board — moved",
        "  [2026-11-23] unknown · approved",
        // A handle's shape, a capital, a word run on into the next.
        "  [2026-11-23] amber-heron-12 — hi",
        "  [2026-11-23] Ana · approved",
        "  [2026-11-23] ana·approved",
        "  [2026-11-23] ana: approved",
        "  [2026-11-23]  — nobody",
        // No space between the date and the name.
        "  [2026-11-23]ana · approved",
    ] {
        assert_eq!(said_line(line), None, "{line}");
        assert_eq!(entry_line(line), None, "{line}");
    }
    // Under a card it reads as an owner's undated note, as before names.
    let found = said(&["  [2026-11-23] Ana · approved"]);
    assert_eq!(
        (
            found[0].name.as_str(),
            found[0].entry.who,
            found[0].entry.ts.as_deref()
        ),
        ("owner", "owner", None)
    );
}

#[test]
fn said_names_every_entry_of_a_card_and_entries_reads_the_same_lines() {
    let raw = [
        "  [2026-11-23 10:00] agent — Posted.",
        "  [2026-11-23 10:05] ana · question — Which?",
        "    and when?",
        "  [2026-11-23 10:06] owner · approved — B",
        "  an undated note",
        "  ↳ an undated answer",
    ];
    let found = said(&raw);
    let names: Vec<(&str, &str)> = found
        .iter()
        .map(|said| (said.name.as_str(), said.entry.who))
        .collect();
    assert_eq!(
        names,
        [
            ("agent", "agent"),
            ("ana", "owner"),
            ("owner", "owner"),
            ("owner", "owner"),
            ("agent", "agent"),
        ]
    );
    assert_eq!(found[1].entry.text, "Which?\nand when?");
    let plain: Vec<_> = found.into_iter().map(|said| said.entry).collect();
    assert_eq!(entries(&raw), plain);
}
