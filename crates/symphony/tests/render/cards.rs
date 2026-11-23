//! RenderTest's card half, MessageTest, AutoApprovedRenderTest and CardAnchorTest.

use symphony_api::{Block, Span};

use super::{bare, page, page_at};

const CANDIDATES: &str = "- [ ] **h** — three takes. Approve and say A, B or C in the note.\n  [2026-09-27 18:00] agent — A ![H lamp A](/a.png) B ![H lamp B](/b.png) C ![H lamp C](/c.png)\n";
const MOCKUPS: &str = concat!(
    "- [ ] **s** — three ways. Pick A, B or C.\n",
    "  [2026-10-06] agent — A ![Stat table A](/docs/pictures/mockups/a.html) [a.png](/artifacts/a.png)",
    " B ![Changed stats first B](/docs/pictures/mockups/b.html) [b.png](/artifacts/b.png)",
    " C ![Stats around the lamp C](mockups/c.html) [c.png](/artifacts/c.png)\n",
);

#[test]
fn decision_item_renders_as_control() {
    let html = page_at(
        "- [-] **lamp-count-141** — 141 lamps. *If no:* picture.\n  Thirty first.\n",
        "docs/DECISIONS.md",
    );
    for want in [
        "class=\"decision\"",
        "data-slug=\"lamp-count-141\"",
        "data-state=\"rejected\"",
        "data-line=\"1\"",
        "<em>If no:</em>",
        "Thirty first.",
        "data-group=\"agent\"",
    ] {
        assert!(html.contains(want), "{want}: {html}");
    }
}

#[test]
fn history_renders_as_dated_entries_by_author() {
    let html = page(
        "- [x] **a** — one.\n  [2026-09-24 10:00] owner · approved — at least 21\n  [2026-09-24 11:00] agent — Recorded as a floor.\n",
    );
    assert!(html.contains("<li class=\"entry owner\"><time>2026-09-24 10:00</time> <b>approved</b> <span class=\"entry-text\">at least 21</span></li>"), "{html}");
    assert!(html.contains("<li class=\"entry agent\"><time>2026-09-24 11:00</time> <span class=\"entry-text\">Recorded as a floor.</span></li>"));
    assert!(html.contains("data-group=\"resolved\""));
    assert!(html.contains("data-decided=\"2026-09-24 10:00\""));
}

#[test]
fn a_persons_line_carries_their_name_after_its_time_and_decides_as_the_owners() {
    let html = page(
        "- [x] **a** — one.\n  [2026-11-23 10:00] ana · approved — at least 21\n  [2026-11-23 11:00] agent — Recorded.\n",
    );
    assert!(html.contains("<li class=\"entry owner\"><time>2026-11-23 10:00</time> <span class=\"entry-by\">ana</span> <b>approved</b> <span class=\"entry-text\">at least 21</span></li>"), "{html}");
    assert!(html.contains("data-group=\"resolved\""), "{html}");
    assert!(html.contains("data-decided=\"2026-11-23 10:00\""), "{html}");
}

#[test]
fn a_card_shows_when_it_was_posted() {
    let html = page("- [ ] **a** — new\n  [2026-09-25 10:00] agent — ![x](/x.png)\n");
    assert!(html.contains("<div class=\"decision-text\"><code>a</code> <time class=\"posted\">2026-09-25 10:00</time>"), "{html}");
}

#[test]
fn an_undated_card_shows_no_posting_time() {
    assert!(!page("- [ ] **a** — new\n").contains("class=\"posted\""));
}

fn strip(html: &str) -> &str {
    let start = html.find("<div class=\"compare\">").expect("a compare row");
    let end = html[start..].find("</div>").unwrap() + start + 6;
    &html[start..end]
}

#[test]
fn a_picture_card_compares_its_candidates_in_one_labelled_row() {
    let html = page_at(CANDIDATES, "docs/lamps/x.md");
    let row = strip(&html);
    let at: Vec<usize> = ["/a.png", "/b.png", "/c.png"]
        .iter()
        .map(|s| row.find(s).unwrap())
        .collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{row}");
    for label in ["A", "B", "C"] {
        assert!(
            row.contains(&format!("<figcaption>{label}</figcaption>")),
            "{label}: {row}"
        );
    }
    assert!(html.find("class=\"compare\"").unwrap() < html.find("class=\"history\"").unwrap());
}

#[test]
fn the_base_picture_leads_the_row() {
    let text = format!(
        "{CANDIDATES}  [2026-09-27 19:00] agent — Base, the donor's approved picture: ![D donor](/base.png)\n"
    );
    let html = page_at(&text, "docs/lamps/x.md");
    let row = strip(&html);
    assert!(
        row.find("/base.png").unwrap() < row.find("/a.png").unwrap(),
        "{row}"
    );
    assert!(row.contains("<figcaption>Base</figcaption>"));
}

#[test]
fn an_html_candidate_in_the_row_is_a_mockup_item_not_a_broken_image() {
    // A browser cannot draw an .html file as an <img>: the row holds it as media, for the client to frame.
    let (_tree, render) = bare();
    let sections = render.sections(MOCKUPS, "docs/pictures/UI.md");
    let media = &sections[0].media;
    let mut srcs: Vec<&str> = media
        .iter()
        .map(|m| match m {
            symphony::render::Media::Mockup { src, .. } => src.as_str(),
            other => panic!("{other:?}"),
        })
        .collect();
    srcs.sort_unstable();
    // each of the line's three, once in the compare row and once in the thread
    assert_eq!(
        srcs,
        [
            "/docs/pictures/mockups/a.html",
            "/docs/pictures/mockups/a.html",
            "/docs/pictures/mockups/b.html",
            "/docs/pictures/mockups/b.html",
            "/docs/pictures/mockups/c.html",
            "/docs/pictures/mockups/c.html"
        ]
    );
    let html = &sections[0].html;
    let row = strip(html);
    assert!(!row.contains(".html"), "{row}");
    assert_eq!(row.matches("data-media=").count(), 3, "{row}");
    for label in ["A", "B", "C"] {
        assert!(
            row.contains(&format!("<figcaption>{label}</figcaption>")),
            "{label}: {row}"
        );
    }
}

#[test]
fn a_png_base_beside_mockup_candidates_stays_a_picture() {
    let text =
        format!("{MOCKUPS}  [2026-10-06] agent — Base, the approved screen: ![Old](/base.png)\n");
    let (_tree, render) = bare();
    let sections = render.sections(&text, "docs/pictures/UI.md");
    let row = strip(&sections[0].html);
    assert!(row.contains("<img src=\"/base.png\""), "{row}");
    assert!(row.find("/base.png").unwrap() < row.find("data-media=").unwrap());
    assert_eq!(
        sections[0].media.len(),
        3 + 3,
        "the row repeats the line's three, the line itself holds them too"
    );
}

#[test]
fn a_card_with_one_picture_has_no_row() {
    let html = page_at(
        "- [ ] **h** — one fix.\n  [2026-09-27 18:00] agent — ![H lamp](/a.png)\n",
        "docs/lamps/x.md",
    );
    assert!(!html.contains("class=\"compare\""));
}

#[test]
fn a_card_without_pictures_has_no_row() {
    assert!(!page("- [ ] **h** — words.\n").contains("class=\"compare\""));
}

#[test]
fn settled_cards_fold_into_a_one_line_summary() {
    for (mark, state) in [("r", "resolved"), ("~", "dismissed"), ("-", "rejected")] {
        let text = format!(
            "- [{mark}] **a** — A long card text that goes on.\n  [2026-09-24 10:00] owner · {state}\n  [2026-09-24 11:00] agent — done\n"
        );
        let html = page(&text);
        assert!(
            html.contains("<details class=\"settled\">"),
            "{state}: {html}"
        );
        assert!(
            html.contains(&format!("<summary><code>a</code> <span class=\"settled-state\">{state}</span> A long card text that goes on.</summary>")),
            "{state}: {html}"
        );
        assert!(html.contains("class=\"decision\""));
    }
}

#[test]
fn a_long_settled_summary_is_cut_and_markup_dropped() {
    let text = format!(
        "- [r] **a** — **Bold** [[PLAN|plan]] {}\n  [2026-09-24 10:00] owner · approved\n",
        "word ".repeat(40)
    );
    let html = page(&text);
    let summary = html
        .split("<summary>")
        .nth(1)
        .unwrap()
        .split("</summary>")
        .next()
        .unwrap();
    assert!(summary.contains("Bold plan word"), "{summary}");
    assert!(summary.ends_with('…'), "{summary}");
    assert!(!summary.contains("**"));
}

#[test]
fn open_cards_stay_unfolded() {
    for text in [
        "- [ ] **a** — new\n",
        "- [-] **a** — no\n  [2026-09-24 10:00] owner · rejected — why\n",
        "- [?] **a** — q\n  [2026-09-24 10:00] owner · question — what?\n  [2026-09-24 11:00] agent — this\n",
    ] {
        assert!(!page(text).contains("<details"), "{text}");
    }
}

#[test]
fn group_headings_show_a_live_count_of_the_cards_under_them() {
    let html = page(
        "<!-- register:start -->\n## Needs your decision\n\n### T\n\n- [ ] **a** — 1\n- [ ] **b** — 2\n\n## Resolved\n\n### T\n\n- [r] **c** — 3\n<!-- register:end -->\n",
    );
    assert!(html.contains(">Needs your decision (2)</h2>"), "{html}");
    assert!(html.contains(">Resolved (1)</h2>"));
}

#[test]
fn a_stale_file_is_counted_by_where_cards_belong_not_where_they_sit() {
    let html = page(
        "<!-- register:start -->\n## Needs your decision\n\n### T\n\n- [r] **c** — 3\n<!-- register:end -->\n",
    );
    assert!(html.contains(">Needs your decision (0)</h2>"), "{html}");
}

#[test]
fn legacy_undated_lines_still_render() {
    let html = page("- [x] **a** — one.\n  owner words\n  ↳ agent words\n");
    assert!(
        html.contains(
            "<li class=\"entry owner\"><span class=\"entry-text\">owner words</span></li>"
        ),
        "{html}"
    );
    assert!(
        html.contains(
            "<li class=\"entry agent\"><span class=\"entry-text\">agent words</span></li>"
        )
    );
}

/// The owner's note and an agent's are drawn by one renderer, a document's: markup, pictures and
/// document links alike.
#[test]
fn owner_and_agent_notes_render_markdown_alike() {
    let (tree, render) = bare();
    tree.write("docs/makers/MAKERS.md", "# Makers\n");
    for who in ["owner · question", "agent"] {
        let html = render.html(
            &format!("- [?] **a** — one.\n  [2026-09-24] {who} — **Bold**, *soft*, `code`: see ![scale](/artifacts/x.svg) and [[MAKERS#East]].\n"),
            "docs/DECISIONS.md",
        );
        let text = entry_text(&html, 0);
        assert!(
            text.starts_with("<strong>Bold</strong>, <em>soft</em>, <code>code</code>"),
            "{who}: {text}"
        );
        assert!(
            text.contains("<img src=\"/artifacts/x.svg\" alt=\"scale\""),
            "{who}: {text}"
        );
        assert!(
            text.contains("href=\"/docs/makers/MAKERS.md#east\""),
            "{who}: {text}"
        );
    }
}

/// Raw HTML in a note is the words typed, the owner's and an agent's alike, on the page and in the
/// typed thread the client draws: `Vec<String>` and `<b>raw</b>` are not markup. The document and
/// card links the note names are links all the same, one alone in its paragraph too.
#[test]
fn raw_html_in_a_note_stays_as_typed_and_its_links_stay_links() {
    let (tree, render) = bare();
    tree.write(
        "docs/makers/MAKERS.md",
        "# Makers\n\n- [ ] **elsewhere** — A card.\n",
    );
    let typed = "The call returns Result<Task, Error> and Vec<String>; a < b > c. <b>raw</b>";
    for who in ["owner · question", "agent"] {
        let card = format!(
            "- [?] **a** — one.\n  [2026-09-24] {who} — {typed}\n    See [[MAKERS#Makers]] and **elsewhere**.\n\n    [[MAKERS]]\n"
        );
        let text = entry_text(&render.html(&card, "docs/DECISIONS.md"), 0).to_string();
        assert!(
            text.starts_with("<p>The call returns Result&lt;Task, Error&gt; and Vec&lt;String&gt;; a &lt; b &gt; c. &lt;b&gt;raw&lt;/b&gt;<br />"),
            "{who}: {text}"
        );
        assert!(
            text.contains("<a href=\"/docs/makers/MAKERS.md#makers\">MAKERS</a>"),
            "{who}: {text}"
        );
        assert!(
            text.contains(
                "<strong><a href=\"/docs/makers/MAKERS.md#elsewhere\">elsewhere</a></strong>"
            ),
            "{who}: {text}"
        );
        let thread = render.typed_history(&card, "docs/DECISIONS.md");
        let blocks = symphony::blocks::fragment(&thread["a"], "docs/DECISIONS.md");
        let [Block::Thread { entries }] = blocks.as_slice() else {
            panic!("{blocks:?}")
        };
        let [
            Block::Paragraph { spans },
            Block::Paragraph { spans: alone },
        ] = entries[0].blocks.as_slice()
        else {
            panic!("{who}: {:?}", entries[0].blocks)
        };
        assert!(
            matches!(spans.first(), Some(Span::Text { text }) if text == typed),
            "{who}: {spans:?}"
        );
        let wiki = |span: &Span| matches!(span, Span::WikiLink { href, .. } if href == "/docs/makers/MAKERS.md#makers");
        assert!(spans.iter().any(wiki), "{who}: {spans:?}");
        assert!(
            spans.iter().any(|span| matches!(
                span,
                Span::Strong { spans } if matches!(
                    spans.as_slice(),
                    [Span::Link { href, target: None, .. }] if href == "/docs/makers/MAKERS.md#elsewhere"
                )
            )),
            "{who}: {spans:?}"
        );
        assert!(
            matches!(alone.as_slice(), [Span::WikiLink { href, .. }] if href == "/docs/makers/MAKERS.md"),
            "{who}: {alone:?}"
        );
    }
}

/// The `n`th entry's drawn words in `html`.
fn entry_text(html: &str, n: usize) -> &str {
    html.split("<span class=\"entry-text\">")
        .nth(n + 1)
        .and_then(|entry| entry.rsplit_once("</span></li>"))
        .map(|(text, _)| text)
        .unwrap_or_else(|| panic!("no entry {n}: {html}"))
}

/// A single Enter in a note is a line break and a blank line a paragraph gap, in the owner's note
/// and an agent's, on the page and in the thread the client draws.
#[test]
fn a_notes_line_break_and_paragraph_gap_are_drawn_for_owner_and_agent() {
    let (_tree, render) = bare();
    for who in ["owner · question", "agent"] {
        let card = format!(
            "- [?] **a** — one.\n  [2026-11-11 15:26] {who} — 1. First question?\n    Its answer.\n\n    Next paragraph.\n"
        );
        let html = render.html(&card, "docs/DECISIONS.md");
        assert_eq!(
            entry_text(&html, 0),
            "<ol>\n<li>First question?<br />\nIts answer.</li>\n</ol>\n<p>Next paragraph.</p>",
            "{who}"
        );
        let thread = render.typed_history(&card, "docs/DECISIONS.md");
        let blocks = symphony::blocks::fragment(&thread["a"], "docs/DECISIONS.md");
        let [Block::Thread { entries }] = blocks.as_slice() else {
            panic!("{blocks:?}")
        };
        let [
            Block::List {
                ordered: true,
                items,
                ..
            },
            Block::Paragraph { spans },
        ] = entries[0].blocks.as_slice()
        else {
            panic!("{who}: {:?}", entries[0].blocks)
        };
        let [Block::Paragraph { spans: item }] = items[0].as_slice() else {
            panic!("{who}: {items:?}")
        };
        assert!(item.contains(&Span::LineBreak), "{who}: {item:?}");
        assert_eq!(spans.len(), 1, "{who}: {spans:?}");
    }
}

/// Headings are the register's own structure: a `#` line in a note, or a line underlined into
/// one, stays the words it is; in fenced code it is code. A `- [ ]` line is a list item, not a card.
#[test]
fn a_heading_or_a_card_line_in_a_note_stays_as_typed() {
    let card = "- [?] **a** — one.\n  [2026-11-11 16:00] owner · question — ## title\n    #tag and words\n    Underlined\n    ---\n    - [ ] **b** — not a card\n\n    ```\n    # a comment\n    ```\n";
    let html = page_at(card, "docs/DECISIONS.md");
    let text = entry_text(&html, 0);
    assert!(!text.contains("<h"), "{text}");
    assert!(
        text.starts_with("<p>## title<br />\n#tag and words<br />\nUnderlined<br />\n---</p>"),
        "{text}"
    );
    assert!(
        text.contains("<ul>\n<li>[ ] <strong>b</strong> — not a card</li>\n</ul>"),
        "{text}"
    );
    assert!(
        text.contains("<pre><code># a comment\n</code></pre>"),
        "{text}"
    );
    assert_eq!(html.matches("class=\"decision\"").count(), 1, "{html}");
}

/// What a document's body can show, a note can: a table, fenced code, a card named in bold, a
/// mockup as a framed page and a picture.
#[test]
fn a_note_carries_what_a_document_body_shows() {
    let (tree, render) = bare();
    tree.write(
        "docs/OTHER.md",
        "# Other\n\n- [ ] **elsewhere** — A card.\n",
    );
    let card = "- [?] **a** — one.\n  [2026-11-11 16:00] owner · question — Compare:\n\n    | Key | Value |\n    |---|---|\n    | a | b |\n\n    ```rs\n    let x = 1;\n    ```\n\n    As **elsewhere** says, ![Mock A](/docs/mockups/a.html) or ![shot](/artifacts/a.png).\n";
    let sections = render.sections(card, "docs/DECISIONS.md");
    let text = entry_text(&sections[0].html, 0);
    for want in [
        "<table>",
        "<td>a</td>",
        "<code class=\"language-rs\">let x = 1;\n</code>",
        "href=\"/docs/OTHER.md#elsewhere\"",
        "<img src=\"/artifacts/a.png\" alt=\"shot\" />",
    ] {
        assert!(text.contains(want), "{want}: {text}");
    }
    assert!(
        !text.contains("a.html\""),
        "a mockup is held as media: {text}"
    );
    assert!(
        matches!(
            sections[0].media.as_slice(),
            [symphony::render::Media::Mockup { src, .. }] if src == "/docs/mockups/a.html"
        ),
        "{:?}",
        sections[0].media
    );
}

const PNG_5X7: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x05\0\0\0\x07";

/// The owner attached a picture and a PDF to a verdict note: each is drawn on its own line, the
/// picture as a picture and the file as its link, among the note's other lines. (Sizes and cache
/// tokens are added when a read prepares the fragment.)
#[test]
fn an_owner_line_naming_an_upload_is_drawn_on_its_own_line() {
    let (tree, render) = bare();
    let picture = symphony::uploads::store(&tree.root, "shot.png", PNG_5X7).unwrap();
    let document = symphony::uploads::store(&tree.root, "brief.pdf", b"%PDF-1.4").unwrap();
    let card = format!(
        "- [?] **a** — one.\n  [2026-10-28 10:00] owner · question — see\n    {}\n    {}\n    ![x](/artifacts/raw.png)\n",
        picture.markup(),
        document.markup()
    );
    let html = render.html(&card, "docs/DECISIONS.md");
    let lines: Vec<_> = entry_text(&html, 0).split("<br />\n").collect();
    assert_eq!(
        lines,
        [
            "see".to_string(),
            format!("<img src=\"{}\" alt=\"shot.png\" />", picture.url()),
            format!("<a href=\"{}\">brief.pdf</a>", document.url()),
            "<img src=\"/artifacts/raw.png\" alt=\"x\" />".to_string(),
        ]
    );
}

/// A task thread's owner message keeps its literal words; one that attached a file also carries
/// typed blocks, the picture among them, which the client draws instead of the bare text.
#[test]
fn an_owner_task_message_with_an_upload_has_picture_blocks_and_one_without_has_none() {
    let (tree, render) = bare();
    let picture = symphony::uploads::store(&tree.root, "shot.png", PNG_5X7).unwrap();
    let message = |sender: &str, kind: &str, text: String| symphony_api::Message {
        id: 1,
        slug: "fixture/task".into(),
        sender: sender.into(),
        recipient: None,
        kind: kind.into(),
        text,
        ts: 0.0,
        person: false,
    };
    let attached = format!("look <here>\n{}", picture.markup());
    // A person a board named writes as the owner does: literal words, and an upload's blocks.
    let ana = |text: String| symphony_api::Message {
        sender: "ana".into(),
        person: true,
        ..message("ana", "message", text)
    };
    let hers = symphony::taskview::entry(&ana(attached.clone()), &render);
    assert!(
        serde_json::to_string(&hers.blocks)
            .unwrap()
            .contains("\"type\":\"image\"")
    );
    let plain = symphony::taskview::entry(&ana("**not bold**".into()), &render);
    assert!(plain.blocks.is_empty(), "{:?}", plain.blocks);
    // The same words from a session are an agent's markdown.
    let agent =
        symphony::taskview::entry(&message("amber-heron", "msg", "**bold**".into()), &render);
    assert!(!agent.blocks.is_empty());
    let entry = symphony::taskview::entry(&message("owner", "message", attached.clone()), &render);
    assert_eq!(
        entry.message.text, attached,
        "the stored words are unchanged"
    );
    let json = serde_json::to_string(&entry.blocks).unwrap();
    assert!(json.contains("\"type\":\"image\""), "{json}");
    assert!(
        json.contains(&format!("\"src\":\"{}?v=", picture.url())),
        "{json}"
    );
    assert!(json.contains("\"alt\":\"shot.png\""), "{json}");
    assert!(
        json.contains("\"width\":\"5\"") && json.contains("\"height\":\"7\""),
        "the picture keeps its real size: {json}"
    );
    assert!(json.contains("\"text\":\"look <here>\""), "{json}");
    for (sender, kind, text) in [
        ("owner", "message", "no file, <b>literal</b>".to_string()),
        ("owner", "system", attached.clone()),
    ] {
        let entry = symphony::taskview::entry(&message(sender, kind, text), &render);
        assert!(
            entry.blocks.is_empty(),
            "{sender} {kind}: {:?}",
            entry.blocks
        );
    }
}

#[test]
fn history_html_gives_each_card_its_rendered_thread() {
    let text = "# D\n\n- [-] **one** — first.\n  [2026-09-24 10:00] owner · rejected — old note\n- [x] **two** — second.\n  [2026-09-26 09:20] agent — See **this**.\n";
    let (_tree, render) = bare();
    let threads = render.history(text, "docs/D.md");
    assert_eq!(
        threads.keys().map(String::as_str).collect::<Vec<_>>(),
        ["one", "two"]
    );
    assert!(threads["one"].html.contains("old note"));
    assert!(threads["two"].html.contains("<strong>this</strong>"));
    assert!(threads["two"].html.contains("class=\"entry agent\""));
}

const AUTO: &str = "<!-- register:start -->\n## Auto-approved: spot-check\n\n### T\n\n- [a] **pic** — A lamp. *Fills:* X · lamp\n  [2026-09-27 10:00] agent — ![x](/x.png)\n  [2026-09-27 11:00] agent · auto-approved — A: bold outline\n<!-- register:end -->\n";

#[test]
fn an_auto_approved_card_renders_unfolded_with_its_state_and_group() {
    let html = page_at(AUTO, "docs/lamps/x.md");
    assert!(html.contains("data-state=\"auto-approved\""), "{html}");
    assert!(html.contains("data-group=\"auto\""));
    assert!(!html.contains("<details"));
}

#[test]
fn an_auto_approved_card_is_counted_in_its_group_heading() {
    assert!(page_at(AUTO, "docs/lamps/x.md").contains(">Auto-approved: spot-check (1)</h2>"));
}

#[test]
fn the_auto_line_shows_its_verdict() {
    assert!(page_at(AUTO, "docs/lamps/x.md").contains("<b>auto-approved</b>"));
}

#[test]
fn an_open_card_has_its_slug_as_id() {
    assert!(
        page("- [ ] **one-card** — new\n")
            .contains("<div class=\"decision\" id=\"one-card\" data-slug=\"one-card\"")
    );
}

#[test]
fn a_settled_card_has_it_too() {
    assert!(
        page("- [r] **old** — done\n  [2026-09-24 10:00] owner · approved\n")
            .contains("id=\"old\"")
    );
}
