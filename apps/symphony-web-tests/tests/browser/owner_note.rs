//! A note under a card keeps the owner's line breaks and paragraph gaps, and an agent's, on a wide
//! page and at phone width; a `#` or `- [ ]` line in it is the owner's words, never the register's
//! structure, before and after a reload.
use super::{SHOWN, chrome, chrome::Chrome, restored::Native};
use serde_json::{Value, json};

const REGISTER: &str = "# Decisions\n\n<!-- register:start -->\n## Needs your decision\n\n### Topic\n\n- [ ] **alpha** — First question.\n- [ ] **bravo** — Second question.\n  [2026-11-11 15:44] agent — First line.\n    Second line.\n\n    Next paragraph.\n<!-- register:end -->\n";
const NOTE: &str =
    "1. First question?\nIts answer.\n\nNext paragraph.\n## title\n- [ ] **bravo** — not a card";

/// How the newest `who` entry of the card `slug` is drawn, as a JS expression: its words (a line
/// break and a block's end read as a space), its blocks' tags, how far below the words before its
/// first line break the words after it start, and the gap between each block and the next; the
/// card's mark and the register group it sits under.
fn drawn(slug: &str, who: &str) -> String {
    format!(
        "(() => {{
            const card = document.querySelector('[data-doc-key=\"card:{slug}\"]');
            const entries = card ? [...card.querySelectorAll('.history .entry.{who} .entry-text')] : [];
            const text = entries[entries.length - 1];
            if (!text) return null;
            const copy = text.cloneNode(true);
            copy.querySelectorAll('br').forEach(br => br.replaceWith(' '));
            copy.querySelectorAll('p, li').forEach(block => block.append(' '));
            const words = [];
            const walker = document.createTreeWalker(text, NodeFilter.SHOW_TEXT);
            while (walker.nextNode()) if (walker.currentNode.data.trim()) words.push(walker.currentNode);
            const lines = node => {{ const range = document.createRange(); range.selectNodeContents(node); return [...range.getClientRects()]; }};
            const br = text.querySelector('br');
            const before = br && words.filter(t => br.compareDocumentPosition(t) & Node.DOCUMENT_POSITION_PRECEDING).pop();
            const after = br && words.find(t => br.compareDocumentPosition(t) & Node.DOCUMENT_POSITION_FOLLOWING);
            const blocks = [...text.children];
            const above = [...document.querySelectorAll('.doc-body h2')].filter(h => h.compareDocumentPosition(card) & Node.DOCUMENT_POSITION_FOLLOWING);
            return {{
                words: copy.textContent.replace(/\\s+/g, ' ').trim(),
                tags: blocks.map(b => b.tagName),
                line: before && after ? Math.round(lines(after)[0].top - lines(before).pop().top) : null,
                gaps: blocks.slice(1).map((b, i) => Math.round(b.getBoundingClientRect().top - blocks[i].getBoundingClientRect().bottom)),
                group: above.length ? above[above.length - 1].textContent.trim().replace(/ \\(\\d+\\)$/, '') : null,
                state: card.dataset.state,
            }};
        }})()"
    )
}

/// The owner's note as read by [`drawn`]: its list items' words too, which fill only once the card
/// is near the viewport.
const WORDS: &str = "First question? Its answer. Next paragraph. ## title [ ] bravo — not a card";
/// The register group a card answered with a question moves to.
const REGROUPED: &str = "Waiting on an agent";

/// The page's drawing of the owner's note on alpha, once the card, scrolled into view on each look,
/// shows the note's words and has moved to its new group, and of the agent's on bravo.
fn both(page: &mut Chrome) -> (Value, Value) {
    let owner = drawn("alpha", "owner");
    assert!(
        page.wait_for(
            &format!(
                "(() => {{ document.querySelector('[data-doc-key=\"card:alpha\"]')?.scrollIntoView({{block: 'center'}}); const drawn = {owner}; return drawn?.state === 'question' && drawn.group === {} && drawn.words === {}; }})()",
                json!(REGROUPED),
                json!(WORDS)
            ),
            SHOWN
        ),
        "{}",
        page.eval(&owner)
    );
    (page.eval(&owner), page.eval(&drawn("bravo", "agent")))
}

#[test]
fn a_notes_line_breaks_and_paragraph_gaps_show_wide_and_at_phone_width_and_after_a_reload() {
    let Some(mut page) = chrome() else {
        return;
    };
    let site = Native::start_committed(None, "main", &[("docs/DECISIONS.md", REGISTER)], None);
    let url = site.url("/docs/DECISIONS.md");
    let alpha = "[data-doc-key=\"card:alpha\"]";
    page.open(&url);
    assert!(page.wait_for(
        &format!(
            "document.querySelector({}) !== null && document.querySelector('.document-page')?.dataset.feed === 'live'",
            json!(format!("{alpha} .doc-actions button"))
        ),
        SHOWN
    ));
    page.eval(&format!(
        "document.querySelector({}).click()",
        json!(format!("{alpha} .doc-actions button"))
    ));
    let field = json!(format!("{alpha} form.verdict textarea"));
    assert!(page.wait_for(&format!("document.querySelector({field}) !== null"), SHOWN));
    page.eval(&format!(
        "(() => {{ const t = document.querySelector({field}); t.focus(); t.value = {}; t.dispatchEvent(new Event('input', {{bubbles: true}})); return true; }})()",
        json!(NOTE)
    ));
    page.eval(&format!(
        "[...document.querySelectorAll({})].find(b => b.textContent.includes('Question')).click()",
        json!(format!("{alpha} .verdict-choices button"))
    ));
    let file = || std::fs::read_to_string(site.root().join("docs/DECISIONS.md")).unwrap();
    for reload in [false, true] {
        if reload {
            page.open(&url);
        }
        for (width, phone) in [(1280, false), (390, true)] {
            page.viewport(width, 900, phone);
            page.frames();
            let (owner, agent) = both(&mut page);
            let at = format!("width {width}, reload {reload}");
            assert_eq!(
                (&owner["words"], &owner["tags"], &owner["group"]),
                (&json!(WORDS), &json!(["OL", "P", "UL"]), &json!(REGROUPED)),
                "{at}: the owner's note is drawn as typed and the card regrouped; the file:\n{}",
                file()
            );
            assert_eq!(
                (&agent["words"], &agent["tags"], &agent["group"]),
                (
                    &json!("First line. Second line. Next paragraph."),
                    &json!(["P", "P"]),
                    &json!("Needs your decision")
                ),
                "{at}"
            );
            for (who, drawn) in [("owner", &owner), ("agent", &agent)] {
                assert!(
                    drawn["line"].as_i64().is_some_and(|line| line >= 12),
                    "{at}: {who}'s line break starts a new line: {drawn}"
                );
                assert!(
                    drawn["gaps"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|gap| gap.as_i64().is_some_and(|gap| gap >= 6)),
                    "{at}: {who}'s blocks have a paragraph gap between them: {drawn}"
                );
            }
        }
    }
    let saved = file();
    assert!(
        saved.contains("owner · question — 1. First question?\n    Its answer.\n\n    Next paragraph.\n    ## title\n    - [ ] **bravo** — not a card\n"),
        "{saved}"
    );
}
