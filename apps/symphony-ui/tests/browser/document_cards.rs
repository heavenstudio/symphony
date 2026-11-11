//! A card on a document page, read in the release client: its verdict form fits a phone's card
//! and its history keeps an agent's paragraphs.
use super::{SHOWN, chrome, chrome::Chrome, common, dist, site::Site};
use serde_json::{Value, json};

const DOC: &str = "docs/PROBE.md";

fn text(text: &str) -> Value {
    json!({"type":"text","text":text})
}

fn paragraph(spans: Vec<Value>) -> Value {
    json!({"type":"paragraph","spans":spans})
}

fn strong(words: &str) -> Value {
    json!({"type":"strong","spans":[text(words)]})
}

/// A card's section and the document's summary of it.
fn card(slug: &str, mark: &str, group: &str, start: usize, body: Vec<Value>) -> (Value, Value) {
    let block = json!({"type":"card","slug":slug,"mark":mark,"title":slug,"body":body,
        "verdict":{"group":group,"open":true,"posted":null,"decided":null},
        "thread":[],"line":start,"line_offset":null,"comparison":[]});
    (
        json!({"start":start,"end":start + 1,"key":format!("card:{slug}"),"hash":slug,"part":1,"blocks":[block]}),
        json!({"slug":slug,"state":mark,"text":slug,"group":group,"open":true}),
    )
}

/// The card `alpha`'s history: an agent's question in three paragraphs.
fn threads() -> Value {
    json!({"alpha":{"blocks":[{"type":"thread","entries":[{"who":"agent","timestamp":"2026-11-11 19:20","verdict":"question","blocks":[
        paragraph(vec![text("Five bands match the catalogue; three match what the eye tells apart.")]),
        paragraph(vec![strong("What five means."), text(" Every lamp keeps its band.")]),
        paragraph(vec![strong("What three means."), text(" Thirty lamps move band.")]),
    ]}]}]}})
}

/// The document `docs/PROBE.md`: a heading, a paragraph and a pending card waiting on the owner,
/// with its history.
fn probe_site() -> Site {
    let site = Site::start(dist(), &common::fixture("board.json"));
    let (alpha, summary) = card(
        "alpha",
        "pending",
        "decide",
        5,
        vec![paragraph(vec![text(
            "Keep all 141 lamps, or merge the near-duplicates down to about 90?",
        )])],
    );
    let sections = json!([
        {"start":1,"end":1,"key":"lamps","hash":"lamps","part":1,"blocks":[{"type":"heading","level":2,"anchor":"lamps","spans":[text("Lamps")]}]},
        {"start":3,"end":3,"key":"intro","hash":"intro","part":1,"blocks":[paragraph(vec![text("The catalogue holds 141 lamps.")])]},
        alpha,
    ]);
    site.set_json("/api/document", &json!({"document":{"version":7,"path":DOC,"title":"Probe","waiting":[],"crumbs":[],
        "headings":[{"level":2,"anchor":"lamps","text":"Lamps"}],"sections":sections,"cards":[summary],"threads":threads(),"tasks":[]},"anchors":{}}));
    site.set_json(
        "/api/section",
        &json!({"version":7,"change":{"path":DOC,"sections":sections,"threads":threads()}}),
    );
    site.set_json("/api/asks", &json!({"asks":[],"recipient":null}));
    site
}

fn viewport(page: &mut Chrome, width: u32, height: u32) {
    page.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":1,"mobile":width < 600}),
    );
}

/// Opens the card `slug`'s editor with its own button and waits for its form.
fn open_editor(page: &mut Chrome, slug: &str) {
    let toggle = format!("[data-doc-key=\"card:{slug}\"] .doc-actions button");
    assert!(
        page.wait_for(
            &format!("document.querySelector({}) !== null", json!(toggle)),
            SHOWN
        ),
        "the card {slug} is drawn"
    );
    page.eval(&format!(
        "document.querySelector({}).click()",
        json!(toggle)
    ));
    assert!(page.wait_for(
        &format!(
            "document.querySelector('[data-doc-key=\"card:{slug}\"] form.verdict:not([hidden])') !== null"
        ),
        SHOWN
    ));
}

#[test]
fn document_card_verdict_form_fits_a_phone_card() {
    let site = probe_site();
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 390, 844);
    page.open(&site.url(&format!("/{DOC}")));
    open_editor(&mut page, "alpha");
    // Every control of the form lies inside it, and the form inside its card.
    let outside = page.eval(
        r#"(() => {
        const card = document.querySelector('[data-doc-key="card:alpha"]'), form = card.querySelector('form.verdict');
        const edge = form.getBoundingClientRect(), outer = card.getBoundingClientRect();
        const out = [...form.querySelectorAll('button, textarea, .attach')]
            .filter(e => { const r = e.getBoundingClientRect(); return r.width > 0 && (r.left < edge.left - 0.5 || r.right > edge.right + 0.5); })
            .map(e => e.textContent.trim() || e.tagName);
        if (edge.right > outer.right + 0.5) out.push('the form, past its card');
        if (document.documentElement.scrollWidth > innerWidth) out.push('the page, sideways');
        return out;
    })()"#,
    );
    assert_eq!(
        outside,
        json!([]),
        "on a phone the verdict form and its controls stay inside the card"
    );
}

#[test]
fn document_card_history_keeps_an_agents_paragraphs() {
    let site = probe_site();
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1280, 900);
    page.open(&site.url(&format!("/{DOC}")));
    let entry = "[data-doc-key=\"card:alpha\"] .history .entry";
    assert!(page.wait_for(
        &format!(
            "document.querySelectorAll({}).length === 3",
            json!(format!("{entry} .entry-text > p"))
        ),
        SHOWN
    ));
    let layout = page.eval(&format!(
        r#"(() => {{
        const entry = document.querySelector({entry}), time = entry.querySelector('time').getBoundingClientRect();
        const paragraphs = [...entry.querySelectorAll('.entry-text > p')];
        const first = paragraphs[0].getClientRects()[0];
        return {{
            runsOn: first.top < time.bottom && first.bottom > time.top,
            ownLines: paragraphs.slice(1).map((p, i) => p.getBoundingClientRect().top >= paragraphs[i].getBoundingClientRect().bottom),
        }};
    }})()"#,
        entry = json!(entry)
    ));
    assert_eq!(
        layout,
        json!({"runsOn":true,"ownLines":[true,true]}),
        "the first paragraph runs on from the entry's time, and each later one starts on a line of its own"
    );
}
