//! A long reference with no place to break, a commit's whole hash or a path, in a document's
//! ordinary paragraph, read in the release client at a phone's widths (`.doc-body`'s inline code,
//! `src/docs.css`): it wraps inside its paragraph rather than running under the document's edge,
//! the page no wider than its window, every character of it there to read and to copy, and the
//! ordinary prose beside it still wraps between its words alone.
use super::{SHOWN, chrome, common, dist, site::Site, visible};
use serde_json::{Value, json};

const DOC: &str = "docs/REFERENCE.md";
const HASH: &str = "36f1f088d1b9042f3f8c64d9f3b9133a98580c3d";
/// A path that breaks at its hyphens and never at its slashes: its last part alone is wider than
/// a phone's paragraph.
const PATH: &str = "apps/symphony-web-tests/tests/browser/document_references.rs";
const PROSE: &str = "An ordinary paragraph of prose around it reads in lines of the measure, each \
    word kept whole where it fits, and wraps between its words as prose always has.";

fn text(text: &str) -> Value {
    json!({"type":"text","text":text})
}

fn code(text: &str) -> Value {
    json!({"type":"code","text":text})
}

/// The document `docs/REFERENCE.md`: one section of three paragraphs, the hash's, the prose's
/// and the path's.
fn reference_site() -> Site {
    let site = Site::start(dist(), &common::fixture("board.json"));
    let paragraph = |spans: Vec<Value>| json!({"type":"paragraph","spans":spans});
    site.set_document(&json!({
        "version": 7, "path": DOC, "title": "Reference", "waiting": [], "crumbs": [],
        "headings": [], "tasks": [], "cards": [], "threads": {},
        "sections": [{"start":1,"end":6,"key":"references","hash":"references","part":0,"blocks":[
            paragraph(vec![text("The change landed at commit "), code(HASH), text(", which every later build names.")]),
            paragraph(vec![text(PROSE)]),
            paragraph(vec![text("The file "), code(PATH), text(" holds this test.")]),
        ]}],
    }));
    site.set_json("/api/asks", &json!({"asks": [], "recipient": null}));
    site.set_json("/api/postponed", &json!([]));
    site
}

/// Each reference as its paragraph lays it out (its text, what a copy of it takes, whether it
/// stands on more than one line and whether every box of it is inside its paragraph), the words
/// of the prose that do not stand whole on one line, and whether the page is no wider than its
/// window.
const LAID_OUT: &str = "(() => { \
    const references = [...document.querySelectorAll('.doc-body p > code')].map(code => { \
      const paragraph = code.closest('p').getBoundingClientRect(); \
      const boxes = [...code.getClientRects()]; \
      const selection = getSelection(); selection.selectAllChildren(code); \
      const copied = selection.toString(); selection.removeAllRanges(); \
      return { text: code.textContent, copied, wraps: boxes.length > 1, \
        inside: boxes.every(box => box.left >= paragraph.left - 1 && box.right <= paragraph.right + 1) }; }); \
    const prose = [...document.querySelectorAll('.doc-body p')].find(p => !p.querySelector('code')).firstChild; \
    let at = 0; const broken = []; \
    for (const word of prose.textContent.split(' ')) { \
      const range = document.createRange(); range.setStart(prose, at); range.setEnd(prose, at + word.length); \
      if (range.getClientRects().length !== 1) broken.push(word); at += word.length + 1; } \
    return { references, broken, page: document.documentElement.scrollWidth <= document.documentElement.clientWidth }; })()";

#[test]
fn document_paragraph_wraps_a_long_reference_inside_itself_at_a_phones_widths() {
    let site = reference_site();
    let Some(mut page) = chrome() else { return };
    // At both widths the hash is wider than its paragraph's whole line.
    for width in [320, 390] {
        page.viewport(width, 800, width < 600);
        page.open(&site.url(&format!("/{DOC}")));
        assert!(
            page.wait_for(&visible(".doc-body p:nth-of-type(3) > code"), SHOWN),
            "at {width}px the document's paragraphs are drawn: {}",
            page.eval("document.querySelector('main')?.innerText.slice(0, 400)")
        );
        // The monospace face's widths are the ones measured, not its fallback's.
        page.eval("document.fonts.ready.then(() => true)");
        let laid_out = page.eval(LAID_OUT);
        assert_eq!(
            laid_out,
            json!({
                "references": [
                    {"text":HASH,"copied":HASH,"wraps":true,"inside":true},
                    {"text":PATH,"copied":PATH,"wraps":true,"inside":true},
                ],
                "broken": [],
                "page": true,
            }),
            "at {width}px each reference wraps inside its paragraph, whole to read and copy, and the prose beside it wraps between its words alone"
        );
    }
}
