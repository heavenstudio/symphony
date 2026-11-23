//! A task's thread in the release client draws an agent's post as the server sends it
//! (`crates/symphony/src/render/markdown.rs`, `convert_post`): words that look like markup, such
//! as the `<TokensPart key=tokens-SLUG>` an agent named in a post, are the text typed, drawn once
//! with no notice of unsupported content; and an element of a document the server cannot draw is
//! named alone, what it holds drawn once after it (`crates/symphony/src/blocks.rs`). The task is
//! the look-parity fixture's.
use super::{SHOWN, chrome, look_parity};
use serde_json::{Value, json};

const PAGE: &str = "/task/visual/in-progress";

/// The agents' entries of the thread, as a list.
const ENTRIES: &str = "[...document.querySelectorAll('#task-thread .entry.agent .entry-text')]";

/// Whether the browser renders the entry `text`, which has no `innerText` until it does: what a
/// failure prints for each entry, with the thread's `textContent`, as `task_attempts.rs`'s `SAID`.
const RENDERED: &str =
    "text.checkVisibility({visibilityProperty: true, contentVisibilityAuto: true})";

/// glacial-sparrow-263's post at 2026-11-20 00:54 on app/workflow-pages-follow-ups, as stored.
const POST: &str = "From app/token-cost-on-pages (glacial-sparrow-263), the hunks I hold in the task page and board files, so whoever lands second merges them. apps/symphony-web/src/task-page.tsx: useContext added to the react import; tokensPart added to the ./task-live import; a TokensPanel type and two components, TokensPart and TokensBody, just before WorkflowPart; one <TokensPart key=tokens-SLUG> line after the Changes section's NearViewport. apps/symphony-web/src/task-live.ts: TASK_TOKENS_PATH in the ./wire import, tokensPart() after timelinePart(). apps/symphony-web/src/task-diff.css: two .task-tokens rules at the end. Board: apps/symphony-web/src/boardwait.tsx (one import, one useCardCost line at WaitCard's top, a .card-cost span between the slug's code and .prio); apps/symphony-web/src/styles/boardwait.css (one .waiting-slug .card-cost rule before .waiting-slug .prio); apps/symphony-web/src/kinds/board.tsx (one import; TasksPage wrapped in <CardCostsOf key=scope scope>, the key moved onto it). board.tsx and boardlive.tsx: nothing. New: card-costs.ts. Say if any of it crosses what you hold.";

/// An agent's message `id` saying `text`, with `blocks` as the server draws it.
fn agent(id: i64, text: &str, blocks: Value) -> Value {
    json!({
        "message": {"id": id, "slug": "visual/in-progress", "sender": "glacial-sparrow-263",
            "recipient": null, "kind": "msg", "text": text, "ts": 1.0 + id as f64},
        "stamp": "2026-11-20 00:54",
        "blocks": blocks,
    })
}

fn paragraph(words: &str) -> Value {
    json!({"type": "paragraph", "spans": [{"type": "text", "text": words}]})
}

#[test]
fn a_post_naming_components_draws_its_words_once_and_an_unknown_element_its_content_once() {
    let site = crate::site();
    site.set_json(
        "/api/task-page",
        &look_parity::responses()["/api/task-page"],
    );
    site.set_json(
        "/api/task-history",
        &json!({
            "entries": [
                agent(2, POST, json!([paragraph(POST)])),
                agent(3, "<widget>visible</widget>", json!([
                    {"type": "unknown", "tag": "widget", "text": ""},
                    {"type": "group", "blocks": [paragraph("visible")]},
                ])),
            ],
            "before": null,
            "after": 3,
        }),
    );
    let Some(mut page) = chrome() else { return };
    page.open(&site.url(PAGE));
    page.reveal("[id=\"thread-region-visual/in-progress\"]");
    for id in [2, 3] {
        page.reveal(&format!("#message-{id}"));
    }
    // Rendered, not only mounted, and words not rendered have no `innerText`: the thread stays
    // hidden under its ghost until it settles, and its region is a slot of its own
    // (`content-visibility: auto`, viewport.tsx) once it settles and its last entry mounts, whose
    // content the browser renders only once a frame has found the slot on the screen. That frame
    // is the next one, so a reader never sees the region empty, but a read before it would.
    assert!(
        page.wait_for(
            &format!("(texts => texts.length === 2 && texts.every(text => {RENDERED}))({ENTRIES})"),
            SHOWN
        ),
        "the entries were never rendered: {}",
        page.eval(&format!(
            "({{text: document.querySelector('#task-thread')?.textContent, \
                rendered: {ENTRIES}.map(text => {RENDERED})}})"
        ))
    );
    let drawn = page.eval(
        "[...document.querySelectorAll('#task-thread .entry.agent .entry-text')].map(text => ({\
            words: text.innerText,\
            notices: [...text.querySelectorAll('.unknown-block')].map(notice => notice.innerText),\
            paragraphs: text.querySelectorAll('.unknown-block p').length,\
        }))",
    );
    assert_eq!(
        drawn,
        json!([
            {"words": POST, "notices": [], "paragraphs": 0},
            {"words": "Unsupported document content: widget\n\nvisible", "notices": ["Unsupported document content: widget"], "paragraphs": 0},
        ]),
    );
}
