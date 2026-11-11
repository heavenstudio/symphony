//! A card on a document page, read in the release client: its verdict form fits a phone's card,
//! its history keeps an agent's paragraphs, its buttons rank by what the owner does next, with
//! Escape closing the editor, and a link's excerpt opens beside the link's own button.
use super::components::key;
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

/// The excerpt the card `alpha`'s link opens: the card's own section.
const OWN_SECTION: &str = "/api/section?path=docs%2FPROBE.md&format=blocks&anchor=lamps";

/// The document `docs/PROBE.md`: a heading, a paragraph, a pending card waiting on the owner with
/// its history and a link to its own section, and an approved card waiting for the owner to
/// confirm it.
fn probe_site() -> Site {
    let site = Site::start(dist(), &common::fixture("board.json"));
    let link = json!({"type":"link","href":"/docs/PROBE.md#lamps","target":{"path":DOC,"anchor":"lamps"},"title":"","spans":[text("Lamps")]});
    let (alpha, pending) = card(
        "alpha",
        "pending",
        "decide",
        5,
        vec![paragraph(vec![
            text("Keep all 141 lamps, or merge the near-duplicates down to about 90? "),
            link,
        ])],
    );
    let (beta, approved) = card(
        "beta",
        "approved",
        "confirm",
        8,
        vec![paragraph(vec![text(
            "Drop the brass finish from the lamp matrix.",
        )])],
    );
    let sections = json!([
        {"start":1,"end":1,"key":"lamps","hash":"lamps","part":1,"blocks":[{"type":"heading","level":2,"anchor":"lamps","spans":[text("Lamps")]}]},
        {"start":3,"end":3,"key":"intro","hash":"intro","part":1,"blocks":[paragraph(vec![text("The catalogue holds 141 lamps.")])]},
        alpha,
        beta,
    ]);
    site.set_json("/api/document", &json!({"document":{"version":7,"path":DOC,"title":"Probe","waiting":[],"crumbs":[],
        "headings":[{"level":2,"anchor":"lamps","text":"Lamps"}],"sections":sections,"cards":[pending,approved],"threads":threads(),"tasks":[]},"anchors":{}}));
    site.set_json(
        "/api/section",
        &json!({"version":7,"change":{"path":DOC,"sections":sections,"threads":threads()}}),
    );
    site.set_json("/api/asks", &json!({"asks":[],"recipient":null}));
    let own: Vec<Value> = sections.as_array().unwrap()[0..3]
        .iter()
        .flat_map(|section| section["blocks"].as_array().unwrap().clone())
        .collect();
    site.set_json(OWN_SECTION, &json!({"blocks":own}));
    site
}

fn scheme(page: &mut Chrome, mode: &str) {
    page.call(
        "Emulation.setEmulatedMedia",
        json!({"features":[{"name":"prefers-color-scheme","value":mode}]}),
    );
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

/// The open verdict form of the card `slug`: the buttons it shows, in order; which are filled and
/// with what colour; whether the choices share one row; how Discard draft is drawn; which
/// button is marked as the card's own verdict.
fn verdict_form(page: &mut Chrome, slug: &str) -> Value {
    page.eval(&format!(
        r#"(() => {{
        const form = document.querySelector('[data-doc-key="card:{slug}"] form.verdict');
        const shown = [...form.querySelectorAll('button')].filter(b => b.getClientRects().length > 0);
        const choices = [...form.querySelectorAll('.verdict-choices button')];
        return {{
            buttons: shown.map(b => b.textContent.trim()),
            filled: shown.filter(b => b.classList.contains('primary')).map(b => [b.textContent.trim(), getComputedStyle(b).backgroundColor]),
            oneRow: new Set(choices.map(b => Math.round(b.getBoundingClientRect().top))).size === 1,
            discard: form.querySelector('.discard')?.dataset.style ?? null,
            current: shown.filter(b => b.getAttribute('aria-current') === 'true').map(b => b.textContent.trim()),
        }};
    }})()"#
    ))
}

#[test]
fn document_card_verdict_buttons_rank_by_what_the_owner_does_next() {
    let site = probe_site();
    let Some(mut page) = chrome() else { return };
    scheme(&mut page, "light");
    viewport(&mut page, 1280, 900);
    page.open(&site.url(&format!("/{DOC}")));
    open_editor(&mut page, "alpha");
    let approve = "[data-doc-key=\"card:alpha\"] form.verdict .v-approved";
    assert!(page.wait_for(
        &format!(
            "getComputedStyle(document.querySelector({})).backgroundColor === 'rgb(46, 125, 79)'",
            json!(approve)
        ),
        SHOWN
    ));
    assert_eq!(
        verdict_form(&mut page, "alpha"),
        json!({
            "buttons":["✓ Approve","✗ Reject","? Question","Later","Send message","⊘ Dismiss","Discard draft"],
            "filled":[["✓ Approve","rgb(46, 125, 79)"]],
            "oneRow":true,
            "discard":"ghost",
            "current":[],
        }),
        "a pending card fills Approve alone, offers no Reset, and puts the four choices on one row"
    );
    open_editor(&mut page, "beta");
    assert_eq!(
        verdict_form(&mut page, "beta"),
        json!({
            "buttons":["✓ Approve","✗ Reject","? Question","Later","Send message","⊘ Dismiss","↺ Reset to pending","Discard draft"],
            "filled":[["✓ Approve","rgb(46, 125, 79)"]],
            "oneRow":true,
            "discard":"ghost",
            "current":["✓ Approve"],
        }),
        "an approved card marks Approve as its own verdict and offers to reset it to pending"
    );
}

#[test]
fn document_card_escape_closes_the_editor_and_returns_focus_to_its_opener() {
    let site = probe_site();
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1280, 900);
    page.open(&site.url(&format!("/{DOC}")));
    open_editor(&mut page, "alpha");
    let card = "[data-doc-key=\"card:alpha\"]";
    assert!(page.wait_for(
        &format!(
            "document.activeElement === document.querySelector({})",
            json!(format!("{card} form.verdict textarea"))
        ),
        SHOWN
    ));
    page.call("Input.insertText", json!({"text":"Merge them."}));
    key(&mut page, "Escape", "Escape", 27);
    assert!(
        page.wait_for(
            &format!(
                "document.querySelector({form}).hidden && document.activeElement === document.querySelector({opener})",
                form = json!(format!("{card} form.verdict")),
                opener = json!(format!("{card} .doc-actions button"))
            ),
            SHOWN
        ),
        "Escape closes the card's editor and focus returns to the button that opened it: {}",
        page.eval("document.activeElement.outerHTML.slice(0, 200)")
    );
    assert_eq!(
        page.eval(&format!(
            "[document.querySelector({opener}).getAttribute('aria-expanded'), document.querySelector({text}).value]",
            opener = json!(format!("{card} .doc-actions button")),
            text = json!(format!("{card} form.verdict textarea"))
        )),
        json!(["false", "Merge them."]),
        "the editor reads as closed and keeps its draft"
    );

    // A paragraph's comment editor too; an open list in it takes the first Escape itself.
    let add = "[data-doc-key=\"intro\"] > .add";
    page.eval(&format!("document.querySelector({}).click()", json!(add)));
    assert!(page.wait_for(
        "document.activeElement === document.querySelector('[data-doc-key=\"intro\"] form.comment textarea')",
        SHOWN
    ));
    page.eval("document.querySelector('[data-doc-key=\"intro\"] form.comment .dx-select-trigger').focus()");
    key(&mut page, " ", "Space", 32);
    assert!(page.wait_for("document.querySelector('[role=listbox]') !== null", SHOWN));
    key(&mut page, "Escape", "Escape", 27);
    assert!(page.wait_for("document.querySelector('[role=listbox]') === null", SHOWN));
    assert_eq!(
        page.eval("document.querySelector('[data-doc-key=\"intro\"] form.comment').hidden"),
        json!(false),
        "the list's Escape is the list's"
    );
    key(&mut page, "Escape", "Escape", 27);
    assert!(
        page.wait_for(
            &format!(
                "document.querySelector('[data-doc-key=\"intro\"] form.comment').hidden && document.activeElement === document.querySelector({})",
                json!(add)
            ),
            SHOWN
        ),
        "Escape closes the comment editor and focus returns to + comment: {}",
        page.eval("document.activeElement.outerHTML.slice(0, 200)")
    );
}

/// The card `alpha`'s show button and its excerpt: the button's label and state, the gap between
/// the button and the excerpt, whether the excerpt's own words show, how many elements own the
/// card's anchor, and whether the excerpt offers excerpts of its own.
const EXCERPT: &str = r#"(() => {
    const card = document.querySelector('[data-doc-key="card:alpha"]'), show = card.querySelector('.section-prose .show-link');
    const box = card.querySelector('.excerpt-box'), button = show.getBoundingClientRect();
    const words = box && [...box.querySelectorAll('p')].find(p => p.textContent === 'The catalogue holds 141 lamps.');
    return {
        label: show.textContent, expanded: show.getAttribute('aria-expanded'),
        gap: box && Math.round(box.getBoundingClientRect().top - button.bottom),
        shown: !!words && words.getBoundingClientRect().height > 0 && words.getBoundingClientRect().bottom <= innerHeight,
        anchors: document.querySelectorAll('[id="alpha"]').length,
        nested: box ? box.querySelectorAll('.show-link').length : null,
    };
})()"#;

#[test]
fn document_card_link_opens_its_excerpt_beside_its_button_and_closes_it_again() {
    let site = probe_site();
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1280, 900);
    page.open(&site.url(&format!("/{DOC}#alpha")));
    let show = "[data-doc-key=\"card:alpha\"] .section-prose .show-link";
    assert!(page.wait_for(
        &format!("document.querySelector({}) !== null", json!(show)),
        SHOWN
    ));
    page.eval(&format!(
        "document.querySelector({}).scrollIntoView({{block:'start'}})",
        json!(show)
    ));
    page.eval(&format!("document.querySelector({}).click()", json!(show)));
    assert!(page.wait_for(
        "document.querySelector('[data-doc-key=\"card:alpha\"] .excerpt-box h2') !== null",
        SHOWN
    ));
    let open = page.eval(EXCERPT);
    let seen = ["label", "expanded", "anchors", "nested", "shown"].map(|key| open[key].clone());
    assert_eq!(
        json!(seen),
        json!(["▾ hide", "true", 1, 0, true]),
        "the open excerpt says how to hide it, shows its section's words, and its copy of the card owns no anchor and no excerpt: {open}"
    );
    let gap = open["gap"].as_i64().unwrap();
    assert!(
        (0..24).contains(&gap),
        "the excerpt opens right under its button, not at the card's end: {open}"
    );
    // A click on the excerpt's words reads them; it does not open the card's editor.
    page.eval("[...document.querySelectorAll('[data-doc-key=\"card:alpha\"] .excerpt-box p')].find(p => p.textContent === 'The catalogue holds 141 lamps.').click()");
    assert_eq!(
        page.eval("document.querySelector('[data-doc-key=\"card:alpha\"] form.verdict').hidden"),
        json!(true)
    );
    page.eval(&format!("document.querySelector({}).click()", json!(show)));
    assert!(page.wait_for(
        "document.querySelector('[data-doc-key=\"card:alpha\"] .excerpt-box') === null",
        SHOWN
    ));
    let closed = page.eval(EXCERPT);
    assert_eq!(
        [&closed["label"], &closed["expanded"]],
        [&json!("▸ show"), &json!("false")],
        "{closed}"
    );
}
