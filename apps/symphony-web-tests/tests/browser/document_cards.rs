//! A card on a document page, read in the release client: its verdict form fits a phone's card,
//! its history keeps an agent's paragraphs, its buttons rank by what the owner does next, with
//! Escape closing the editor and Later and its Undo keeping the focus on their button, its focus
//! ring is the page's and not the card's colour, a link's excerpt opens beside the link's own
//! button and fills even opened below a short window's fold, callouts are as wide as cards, a
//! comment's colour its own, and a folded card opened by a click draws the picture in its history.
use super::components::key;
use super::{SHOWN, chrome, chrome::Chrome, focus_on, site::Site};
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

/// The comment callout's title: long enough to reach under its section's "+ comment".
const COMMENT_TITLE: &str = "141 lamps: the count includes the two prototype lamps nobody placed, so check the catalogue against the deck plans before merging any of them";

/// The document `docs/PROBE.md`: a heading, a paragraph, a pending card waiting on the owner with
/// its history and a link to its own section, and an approved card waiting for the owner to
/// confirm it.
pub(super) fn probe_site() -> Site {
    let site = crate::site();
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
    let callout = |kind: &str, start: usize, title: &str| {
        json!({"start":start,"end":start + 1,"key":kind,"hash":kind,"part":1,"blocks":[{"type":"callout","kind":kind,
            "title":[text(title)],"resolved":false,"blocks":[paragraph(vec![text("Worth checking before merging.")])]}]})
    };
    let sections = json!([
        {"start":1,"end":1,"key":"lamps","hash":"lamps","part":1,"blocks":[{"type":"heading","level":2,"anchor":"lamps","spans":[text("Lamps")]}]},
        {"start":3,"end":3,"key":"intro","hash":"intro","part":1,"blocks":[paragraph(vec![text("The catalogue holds 141 lamps.")])]},
        alpha,
        beta,
        callout("comment", 11, COMMENT_TITLE),
        callout("decision", 14, "Drop the brass finish."),
    ]);
    site.set_json("/api/document", &json!({"document":{"version":7,"path":DOC,"title":"Probe","waiting":[],"crumbs":[],
        "headings":[{"level":2,"anchor":"lamps","text":"Lamps"}],"sections":sections,"cards":[pending,approved],"threads":threads(),"tasks":[]},"anchors":{}}));
    site.set_json(
        "/api/section",
        &json!({"version":7,"change":{"path":DOC,"sections":sections,"threads":threads()}}),
    );
    site.set_json("/api/asks", &json!({"asks":[],"recipient":null}));
    // The excerpt is the card's own section, with paragraphs below its top level: its copy of the
    // card carries the card's history, then a list item and a quote of two paragraphs each.
    let mut own: Vec<Value> = sections.as_array().unwrap()[0..3]
        .iter()
        .flat_map(|section| section["blocks"].as_array().unwrap().clone())
        .collect();
    own[2]["thread"] = threads()["alpha"]["blocks"][0]["entries"].clone();
    own.push(json!({"type":"list","ordered":false,"start":1,"items":[[
        paragraph(vec![text("The corridor strips dim over twenty minutes.")]),
        paragraph(vec![text("The corridor strips stay at full on deck one.")]),
    ]]}));
    own.push(json!({"type":"quote","blocks":[
        paragraph(vec![text("The survey asked 40 people.")]),
        paragraph(vec![text("Most chose the warm end.")]),
    ]}));
    site.set_json(OWN_SECTION, &json!({"blocks":own}));
    site
}

/// Opens the card `slug`'s editor with its own button and waits for its form.
pub(super) fn open_editor(page: &mut Chrome, slug: &str) {
    let toggle = format!("[data-doc-key=\"card:{slug}\"] .doc-actions button");
    // As a reader would, once the card shows: under the document's ghost its editor takes no focus.
    assert!(
        page.wait_for(&crate::visible(&toggle), SHOWN),
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
    page.viewport(390, 844, true);
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

/// Where the open versions reach: the page's width beside the window's (less its scrollbar), whether the list stays
/// inside the document's head, and the versions whose words run past their button.
const VERSIONS_FIT: &str = "(() => {
    const versions = document.querySelector('details.versions'), head = document.querySelector('.doc-head');
    return {page: document.documentElement.scrollWidth, window: document.documentElement.clientWidth,
        inside: versions.getBoundingClientRect().right <= head.getBoundingClientRect().right + 0.5,
        cut: [...versions.querySelectorAll('li button')].filter(b => b.scrollWidth > b.clientWidth + 1).map(b => b.textContent)};
})()";

/// Opening "Versions and diffs" never makes the page scroll sideways (app/visual-polish-3), wide
/// or on a phone: the list keeps to the document's head, a version's long subject wraps inside its
/// button, and a diff's long line scrolls inside its own box.
#[test]
fn the_open_versions_and_a_diff_keep_the_page_the_windows_width() {
    let site = probe_site();
    let subject = "app/visual-polish-3: the open versions take a row of their own under the page's links, \
                   each version's date and subject wrapping inside its button";
    site.set_json(
        "/api/versions",
        &json!({"versions":[{"sha":"a1","date":"2026-11-14 01:12","subject":subject},
            {"sha":"b2","date":"2026-11-13 22:40","subject":"Short"}]}),
    );
    site.set_json(
        "/api/diff",
        &json!({"sha":"a1","subject":subject,"diff":format!("+{}", "an-unbroken-line-".repeat(30))}),
    );
    let Some(mut page) = chrome() else { return };
    for (width, height) in [(1425, 900), (390, 844)] {
        page.viewport(width, height, width < 600);
        page.open(&site.url(&format!("/{DOC}")));
        assert!(page.wait_for(
            "document.querySelector('details.versions > summary') !== null",
            SHOWN
        ));
        page.eval("document.querySelector('details.versions > summary').click()");
        assert!(
            page.wait_for(
                "document.querySelectorAll('details.versions li button').length === 2",
                SHOWN
            ),
            "{width}px: {}",
            page.eval("document.querySelector('details.versions')?.innerText")
        );
        let listed = page.eval(VERSIONS_FIT);
        page.eval("document.querySelector('details.versions li button').click()");
        assert!(page.wait_for(
            "document.querySelector('details.versions pre.diff') !== null",
            SHOWN
        ));
        let diffed = page.eval(VERSIONS_FIT);
        for (open, fit) in [("listed", listed), ("with a diff", diffed)] {
            assert_eq!(
                (&fit["page"], &fit["inside"], &fit["cut"]),
                (&fit["window"], &json!(true), &json!([])),
                "{width}px, {open}: the page keeps the window's width and each version its button's: {fit}"
            );
        }
    }
}

#[test]
fn document_card_history_keeps_an_agents_paragraphs() {
    let site = probe_site();
    let Some(mut page) = chrome() else { return };
    page.viewport(1280, 900, false);
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
    page.scheme("light");
    page.viewport(1280, 900, false);
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
    page.viewport(1280, 900, false);
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

/// A card's Later and its Undo, pressed from the keyboard in its editor, keep the focus on their
/// button while the server saves, where a second press posts nothing, and once the card's new state
/// is drawn: Undo Later after Later, Later after Undo. The server names no card next, so the reader
/// stays on this one.
#[test]
fn document_card_later_and_undo_keep_the_focus_on_their_button() {
    let site = probe_site();
    site.set_json("/api/postponed", &json!([]));
    site.set_json(
        "/api/postpone",
        &json!({"ok":true,"changed":false,"next":null}),
    );
    let Some(mut page) = chrome() else { return };
    page.viewport(1280, 900, false);
    page.open(&site.url(&format!("/{DOC}")));
    open_editor(&mut page, "alpha");
    let later = "[data-doc-key=\"card:alpha\"] .verdict-choices .postpone-controls button";
    for (pressed, postponed, label) in [
        (("Enter", "Enter", 13), json!(["alpha"]), "Undo Later"),
        ((" ", "Space", 32), json!([]), "Later"),
    ] {
        page.eval(&format!("document.querySelector({}).focus()", json!(later)));
        site.hold_route("/api/postpone", true);
        key(&mut page, pressed.0, pressed.1, pressed.2);
        assert!(
            page.wait_for(
                &format!(
                    "document.querySelector({})?.textContent === 'Saving…'",
                    json!(later)
                ),
                SHOWN
            ),
            "{} says it saves",
            pressed.0
        );
        assert_eq!(
            focus_on(&mut page, later),
            json!(true),
            "the focus stays on the button while it saves"
        );
        assert!(
            page.wait_for(&crate::drawn_disabled(later), SHOWN),
            "it looks as a disabled button does while it saves"
        );
        // Pressed again while it saves: a second post would show in the count of them below.
        key(&mut page, pressed.0, pressed.1, pressed.2);
        site.set_json("/api/postponed", &postponed);
        site.hold_route("/api/postpone", false);
        assert!(
            page.wait_for(
                &format!(
                    "document.querySelector({})?.textContent === {}",
                    json!(later),
                    json!(label)
                ),
                SHOWN
            ),
            "the card's button says {label} once the server answered"
        );
        assert_eq!(
            focus_on(&mut page, later),
            json!(true),
            "once the server answered, the focus is on {label}"
        );
    }
    let actions: Vec<_> = site
        .posts()
        .into_iter()
        .filter(|(path, body)| path == "/api/postpone" && body["action"] != "seen")
        .map(|(_, body)| body["action"].clone())
        .collect();
    assert_eq!(actions, [json!("later"), json!("undo")]);
}

/// Focus the button `selector` names from the keyboard, as a reader reaches it: Shift+Tab from it
/// and Tab back. A focus a script gives after a click shows no ring (`:focus-visible`).
fn tab_to(page: &mut Chrome, selector: &str) {
    page.eval(&format!(
        "document.querySelector({}).focus()",
        json!(selector)
    ));
    for (kind, modifiers) in [("keyDown", 8), ("keyUp", 8), ("keyDown", 0), ("keyUp", 0)] {
        page.call(
            "Input.dispatchKeyEvent",
            json!({"type":kind,"key":"Tab","code":"Tab","windowsVirtualKeyCode":9,"modifiers":modifiers}),
        );
    }
}

/// The ring the button `selector` names shows once the keyboard has focused it: whether it holds
/// the focus and shows it, the outline's style and colour, and the colour of the card it is in (its
/// text's, `.decision`), if any. A button's colours, its outline's among them, change by a
/// transition (`.dx-button`'s), so they are read once its transitions have ended.
fn focus_ring(page: &mut Chrome, selector: &str) -> Value {
    tab_to(page, selector);
    assert!(
        page.wait_for(
            &format!(
                "document.querySelector({}).getAnimations().length === 0",
                json!(selector)
            ),
            SHOWN
        ),
        "{selector}'s transitions end"
    );
    page.eval(&format!(
        r#"(button => {{
        const style = getComputedStyle(button), card = button.closest('.doc-card')?.querySelector('.decision');
        return {{shown: document.activeElement === button && button.matches(':focus-visible'),
            outline: style.outlineStyle, ring: style.outlineColor, card: card ? getComputedStyle(card).backgroundColor : null}};
    }})(document.querySelector({}))"#,
        json!(selector)
    ))
}

/// The keyboard's focus ring inside a card is the page's, in light and dark: on a button of the
/// card's text, the card's own button and a button of its editor, never the card's colour, which its
/// thread's dots take to stand clear of their line.
#[test]
fn document_card_focus_ring_is_the_pages_not_the_cards_colour() {
    let site = probe_site();
    let Some(mut page) = chrome() else { return };
    page.viewport(1280, 900, false);
    page.open(&site.url(&format!("/{DOC}")));
    open_editor(&mut page, "alpha");
    let card = "[data-doc-key=\"card:alpha\"]";
    assert!(
        page.wait_for(
            &format!(
                "document.querySelector({}) !== null",
                json!(format!("{card} .history .entry"))
            ),
            SHOWN
        ),
        "the card's thread is drawn"
    );
    for mode in ["light", "dark"] {
        page.scheme(mode);
        // A paragraph's + comment, outside any card: the page's ring.
        let page_ring = focus_ring(&mut page, "[data-doc-key=\"intro\"] > .add");
        assert_eq!(
            [
                &page_ring["shown"],
                &page_ring["outline"],
                &page_ring["card"]
            ],
            [&json!(true), &json!("solid"), &Value::Null],
            "{mode}: a button outside any card shows the page's ring: {page_ring}"
        );
        for button in [
            format!("{card} .section-prose .show-link"),
            format!("{card} .doc-actions button"),
            format!("{card} .verdict-choices .postpone-controls button"),
        ] {
            let look = focus_ring(&mut page, &button);
            assert_eq!(
                [&look["shown"], &look["outline"], &look["ring"]],
                [&json!(true), &json!("solid"), &page_ring["ring"]],
                "{mode}: {button} shows the page's ring when focused: {look}"
            );
            assert_ne!(
                look["ring"], look["card"],
                "{mode}: {button}'s ring is not its card's colour: {look}"
            );
        }
        let halo = page.eval(&format!(
            "[getComputedStyle(document.querySelector({}), '::before').boxShadow, getComputedStyle(document.querySelector({})).backgroundColor]",
            json!(format!("{card} .history .entry")),
            json!(format!("{card} .decision"))
        ));
        assert!(
            halo[0]
                .as_str()
                .zip(halo[1].as_str())
                .is_some_and(|(shadow, colour)| shadow.starts_with(colour)),
            "{mode}: the thread's dots stand clear of their line in the card's colour: {halo}"
        );
        let surfaces = page.eval(SURFACE_HALOS);
        for (surface, halo) in ["a document's prose", "a card"]
            .iter()
            .zip(surfaces.as_array().expect("a halo for each surface"))
        {
            assert!(
                halo[0]
                    .as_str()
                    .zip(halo[1].as_str())
                    .is_some_and(|(shadow, colour)| shadow.starts_with(colour)),
                "{mode}: a history's dots on {surface} stand clear in its colour: {halo}"
            );
        }
    }
}

/// The halo of a history's dot on each surface that holds one outside a card's thread, a document's
/// prose (`.doc-part`) and a card (`.card`, a task's brief among them): for each, the dot's shadow
/// and the surface's colour.
const SURFACE_HALOS: &str = r#"["doc-part", "card"].map(surface => {
    const box = document.createElement("div");
    box.className = surface;
    box.innerHTML = '<ul class="history"><li class="entry">A line</li></ul>';
    document.querySelector("main").append(box);
    const halo = [
        getComputedStyle(box.querySelector(".entry"), "::before").boxShadow,
        getComputedStyle(box).backgroundColor,
    ];
    box.remove();
    return halo;
})"#;

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

/// The card `alpha`'s open excerpt: how many paragraphs follow another in the same list item, quote
/// or history entry, and the words of each that runs on in its predecessor's line.
const PARAGRAPHS: &str = r#"(() => {
    const box = document.querySelector('[data-doc-key="card:alpha"] .excerpt-box');
    const holders = box ? [...box.querySelectorAll('li, blockquote, .entry-text')] : [];
    const pairs = holders.flatMap(holder => {
        const own = [...holder.querySelectorAll('p')].filter(p => p.closest('li, blockquote, .entry-text') === holder);
        return own.slice(1).map((p, i) => [own[i], p]);
    });
    const runOn = pairs.filter(([before, p]) => {
        const last = [...before.getClientRects()].pop(), first = p.getClientRects()[0];
        return !last || !first || first.top < last.bottom - 0.5;
    }).map(([, p]) => p.textContent.trim());
    return {pairs: pairs.length, runOn};
})()"#;

#[test]
fn document_card_link_opens_its_excerpt_beside_its_button_and_closes_it_again() {
    let site = probe_site();
    let Some(mut page) = chrome() else { return };
    page.viewport(1280, 900, false);
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
    // The card's paragraphs run on in its line; the excerpt's stay paragraphs at every depth.
    assert!(
        page.wait_for(&format!("{PARAGRAPHS}.pairs === 4"), SHOWN),
        "the excerpt draws its copied history, list item and quote: {}",
        page.eval(PARAGRAPHS)
    );
    assert_eq!(
        page.eval(PARAGRAPHS),
        json!({"pairs":4,"runOn":[]}),
        "in an excerpt each paragraph of a list item, a quote or a copied card's history starts on a line of its own"
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

/// Where the card `alpha`'s open excerpt is: the page's scroll, whether its box's top is below the
/// window, and how many of its regions still wait to come near the screen.
const EXCERPT_PLACE: &str = r#"(() => {
    const box = document.querySelector('[data-doc-key="card:alpha"] .excerpt-box');
    return {scrollY, below: box.getBoundingClientRect().top >= innerHeight,
        waiting: box.querySelectorAll('[data-pending=true]').length};
})()"#;

/// Where the card `alpha`'s show button is: the page's scroll, the card's top (its anchor's) and
/// how far the page scrolls on, in whole pixels, to put the button's bottom at the window's bottom
/// edge, the button whole on the screen.
const PLACE_OF_SHOW: &str = r#"(() => {
    const show = document.querySelector('[data-doc-key="card:alpha"] .section-prose .show-link');
    return {scrollY, card: document.getElementById('alpha')?.getBoundingClientRect().top ?? null,
        below: show ? Math.ceil(show.getBoundingClientRect().bottom - innerHeight) : null};
})()"#;

/// On a short window (a laptop's browser, a phone) the owner opens a card's excerpt from a button at
/// the window's bottom edge, its box opening below the screen: the excerpt fills without being
/// scrolled to. Its box scrolls, and so clipped the look-ahead of the parts of it that waited to
/// come near the screen (the copied card's history, the list item): opening it is the ask.
#[test]
fn document_card_excerpt_opened_below_a_short_windows_fold_fills_without_a_scroll() {
    let site = probe_site();
    let Some(mut page) = chrome() else { return };
    page.viewport(756, 469, false);
    // Room above the card, so its button can sit at the window's bottom edge.
    page.call("Page.addScriptToEvaluateOnNewDocument", json!({"source":
        "const observer = new MutationObserver(() => { if (document.head) { const style = document.createElement('style'); style.textContent = '[data-doc-key=\"intro\"] { margin-bottom: 900px }'; document.head.append(style); observer.disconnect(); } }); observer.observe(document, {childList: true, subtree: true});"
    }));
    page.open(&site.url(&format!("/{DOC}#alpha")));
    let show = "[data-doc-key=\"card:alpha\"] .section-prose .show-link";
    // The address names the card, so the page lands on it (`land`, shell/shell.ts) once its
    // section has drawn, in an effect that runs after the button is in the page: a scroll made
    // before it is taken back to the card. The landing holds the card at the scroll padding while
    // the sections near it draw: the intro, drawn above it late under its margin, pushes it some
    // 900 px down until the hold's next frame takes it back, and a wheel measured in between
    // scrolls the wrong way. So the reader's scroll is measured once the landing has ended, with
    // nothing near the card left to draw, and is a wheel, the reader's own input.
    assert!(
        page.wait_for(
            &format!(
                "{} && {}",
                crate::visible(show),
                crate::document_landing::landing_ended_on(DOC, "alpha")
            ),
            SHOWN
        ),
        "the card alpha is drawn, the page has landed on it and its landing has ended: {} {}",
        page.eval(PLACE_OF_SHOW),
        page.eval("getComputedStyle(document.documentElement).overflowAnchor")
    );
    let by = page.eval(&format!("{PLACE_OF_SHOW}.below"));
    page.call(
        "Input.dispatchMouseEvent",
        json!({"type": "mouseWheel", "x": 378, "y": 234, "deltaX": 0, "deltaY": by}),
    );
    assert!(
        page.wait_for(&format!("{PLACE_OF_SHOW}.below === 0"), SHOWN),
        "the wheel puts the card's button at the window's bottom edge, {by} px from where the page landed: {}",
        page.eval(PLACE_OF_SHOW)
    );
    page.eval(&format!("document.querySelector({}).click()", json!(show)));
    assert!(
        page.wait_for(
            "document.querySelector('[data-doc-key=\"card:alpha\"] .excerpt-box h2') !== null",
            SHOWN
        ),
        "the excerpt is read"
    );
    let opened = page.eval(EXCERPT_PLACE);
    assert_eq!(
        opened["below"],
        json!(true),
        "the excerpt's box opens below the window, so the case tests a box off screen: {opened}"
    );
    assert!(
        page.wait_for(&format!("{PARAGRAPHS}.pairs === 4"), SHOWN),
        "the excerpt draws its copied history and list item where it opened, below the window: {} {}",
        page.eval(PARAGRAPHS),
        page.eval(EXCERPT_PLACE)
    );
    assert_eq!(
        page.eval(EXCERPT_PLACE),
        json!({"scrollY": opened["scrollY"], "below": true, "waiting": 0}),
        "it filled with nothing scrolled and nothing left waiting in it"
    );
}

#[test]
fn document_callouts_are_as_wide_as_cards_and_a_comment_is_not_a_decision() {
    let site = probe_site();
    let Some(mut page) = chrome() else { return };
    // One load: the scheme and the width change the page in place, and opening the same address
    // again would only move to its fragment.
    page.open(&site.url(&format!("/{DOC}#L11")));
    assert!(page.wait_for(
        "document.querySelectorAll('.callout').length === 2 && document.querySelectorAll('.doc-card').length === 2",
        SHOWN
    ));
    for mode in ["light", "dark"] {
        page.scheme(mode);
        for (width, height) in [(1280, 900), (390, 844)] {
            page.viewport(width, height, width < 600);
            let look = page.eval(
                r#"(() => {
                const width = e => e.getBoundingClientRect().width, edge = e => getComputedStyle(e).borderLeftColor;
                const [comment, decision] = [...document.querySelectorAll('.callout')];
                return {card: width(document.querySelector('.doc-card')), comment: width(comment), decision: width(decision),
                    commentEdge: edge(comment), decisionEdge: edge(decision)};
            })()"#,
            );
            assert_eq!(
                [&look["comment"], &look["decision"]],
                [&look["card"], &look["card"]],
                "{mode} at {width}px: a callout is as wide as a card: {look}"
            );
            assert_ne!(
                look["commentEdge"], look["decisionEdge"],
                "{mode}: a comment's colour is not a decision's: {look}"
            );
        }
    }
}

/// The comment callout's section button and its title's words level with it: the button's label,
/// how many of those words' boxes reach under the button, and the gap between the last of them and
/// the button. The button's box is measured whether or not it shows: it shows on hover or focus.
const TITLE_BESIDE_ADD: &str = r#"(() => {
    const section = document.querySelector('[data-doc-key="comment"]'), add = section.querySelector(':scope > .add');
    const button = add.getBoundingClientRect(), range = document.createRange();
    range.selectNodeContents(section.querySelector('.callout-title'));
    const level = [...range.getClientRects()].filter(r => r.width > 0 && r.bottom > button.top && r.top < button.bottom);
    return {label: add.textContent.trim(), level: level.length, covered: level.filter(r => r.right > button.left).length,
        gap: level.length ? Math.round(button.left - Math.max(...level.map(r => r.right))) : null};
})()"#;

#[test]
fn document_callout_title_stops_short_of_its_sections_comment_button() {
    let site = probe_site();
    let Some(mut page) = chrome() else { return };
    page.open(&site.url(&format!("/{DOC}#L11")));
    assert!(page.wait_for(
        "document.querySelector('[data-doc-key=\"comment\"] > .add') !== null && document.querySelector('[data-doc-key=\"comment\"] .callout-title') !== null",
        SHOWN
    ));
    let mut seen = Vec::new();
    for (width, height) in [(1280, 900), (390, 844)] {
        page.viewport(width, height, width < 600);
        seen.push((width, page.eval(TITLE_BESIDE_ADD)));
    }
    // Open, the comment editor leaves the button showing, under its longer label.
    page.eval("document.querySelector('[data-doc-key=\"comment\"] > .add').click()");
    assert!(page.wait_for(
        "document.querySelector('[data-doc-key=\"comment\"] > .add').textContent.includes('Close')",
        SHOWN
    ));
    for (width, height) in [(1280, 900), (390, 844)] {
        page.viewport(width, height, width < 600);
        seen.push((width, page.eval(TITLE_BESIDE_ADD)));
    }
    for (width, look) in &seen {
        // The title's first line is level with the button, and ends at least 4px before it.
        assert!(
            look["level"].as_i64().is_some_and(|level| level > 0)
                && look["covered"] == json!(0)
                && look["gap"].as_i64().is_some_and(|gap| gap >= 4),
            "at {width}px the callout's title stops short of {}: {look}; all: {seen:?}",
            look["label"]
        );
    }
}

/// The folded card's document: the register's resolved card, its picture a block of its first
/// history entry between two paragraphs, its src landing as the entries draw and the card's slots
/// take `content-visibility: auto` (`DocumentPicture`). The outline carries no block, as the
/// server's does, and a folded card's section is read only once it is opened: the site, and the
/// answer the card's section is read with, set by the test before it opens the card.
const FOLDED: &str = "docs/FOLDED.md";
const FOLDED_PICTURE: &str = "/docs/analysis/split.svg";

fn folded_card_site() -> (Site, Value) {
    let site = crate::site();
    let question = "Where do the pictures a card shows live? A: the ignored folder. F: committed beside the card. Recommended: F.";
    let code = |words: &str| json!({"type":"code","text":words});
    let list = |items: Vec<Vec<Value>>| {
        let items: Vec<Vec<Value>> = items
            .into_iter()
            .map(|spans| vec![paragraph(spans)])
            .collect();
        json!({"type":"list","ordered":false,"start":1,"items":items})
    };
    let entry = |who: &str, at: &str, verdict: Option<&str>, blocks: Vec<Value>| json!({"type":"thread","entries":[{"who":who,"timestamp":at,"verdict":verdict,"blocks":blocks}]});
    let threads = json!({"folded-picture":{"blocks":[
        entry("agent", "2026-11-12 16:36", None, vec![
            paragraph(vec![text("Posted for a board task. F, drawn:")]),
            json!({"type":"image","image":{"src":format!("{FOLDED_PICTURE}?v=67795c11090a"),"alt":"F: what a card shows is committed","presentation":{}}}),
            paragraph(vec![strong("What F means for you:")]),
            list(vec![
                vec![strong("It commits the pictures,"), text(" so every clone shows them.")],
                vec![strong("Every checkout carries all of them.")],
                vec![strong("Evidence posted only on a thread is in no clone.")],
            ]),
            paragraph(vec![text("Approve and say A or F in the note.")]),
        ]),
        entry("owner", "2026-11-12 17:40", Some("approved"), vec![paragraph(vec![text("agreed with F")])]),
        entry("agent", "2026-11-12 17:59", None, vec![
            paragraph(vec![text("F is taken. These tasks are filed:")]),
            list(vec![
                vec![code("tools/doc-pictures-committed"), text(": the docs audit refuses an untracked card picture.")],
                vec![code("tools/attach-evidence"), text(": evidence goes to the store.")],
            ]),
            paragraph(vec![text("Nothing was changed elsewhere.")]),
        ]),
        entry("agent", "2026-11-12 17:59", Some("resolved"), vec![]),
    ]}});
    let summary = json!([code("folded-picture"), text(" "),
        {"type":"styled","tag":"span","presentation":{"class":"settled-state"},"spans":[text("resolved")]}, text(&format!(" {question}"))]);
    let card = json!({"type":"card","slug":"folded-picture","mark":"resolved","title":question,"body":[paragraph(vec![text(question)])],
        "verdict":{"group":"resolved","open":false,"posted":"2026-11-12 16:36","decided":"2026-11-12 17:40"},
        "thread":[],"line":null,"line_offset":0,"comparison":[]});
    let section = |start: usize, end: usize, key: &str, blocks: Vec<Value>| json!({"start":start,"end":end,"key":key,"hash":key,"part":0,"blocks":blocks});
    let heading = |level: u8, anchor: &str, words: &str| json!({"type":"heading","level":level,"anchor":anchor,"spans":[text(words)]});
    let sections = [
        section(
            1,
            1,
            "section:register:0",
            vec![heading(1, "decision-register", "Decision register")],
        ),
        section(
            3,
            4,
            "section:register:1",
            vec![heading(2, "resolved", "Resolved")],
        ),
        section(
            6,
            25,
            "card:folded-picture",
            vec![json!({"type":"settled-card","summary":summary,"open":false,"blocks":[card]})],
        ),
    ];
    let outline: Vec<Value> = sections
        .iter()
        .map(|section| {
            let mut outline = section.clone();
            outline["blocks"] = json!([]);
            outline
        })
        .collect();
    let folded = json!({"slug":"folded-picture","state":"resolved","text":question,"group":"resolved","open":false});
    site.set_json("/api/document", &json!({"document":{"version":2,"path":FOLDED,"title":"Decision register","waiting":[],"crumbs":[],
        "headings":[],"sections":outline,"cards":[folded],"threads":{},"tasks":[]},
        "anchors":{"card:folded-picture":["folded-picture"],"section:register:0":["decision-register"],"section:register:1":["resolved"]}}));
    // The server answers a read with the sections it names alone, and the page names a folded
    // card's only once it is opened: until then a read is the headings'.
    let [register, resolved, card_section] = sections;
    site.set_json(
        "/api/section",
        &json!({"version":2,"change":{"path":FOLDED,"sections":[register, resolved]}}),
    );
    site.set_json("/api/asks", &json!({"asks":[],"recipient":null}));
    site.set_file(
        FOLDED_PICTURE,
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="600" height="300"><rect width="600" height="300" fill="teal"/></svg>"#.to_vec(),
        "image/svg+xml",
    );
    let opened = json!({"version":2,"change":{"path":FOLDED,"sections":[card_section],"threads":threads,
        "cards":[{"key":"folded-picture","value":folded}]}});
    (site, opened)
}

/// The folded card's picture: how many boxes it has, its drawn size and its loaded width; with
/// none, whether the card is open and how many history entries it draws.
const FOLDED_IMAGE: &str = r#"(() => {
    const card = document.getElementById('folded-picture'), image = card?.querySelector('.history img');
    if (!image) return {open: card?.open, entries: card?.querySelectorAll('.history .entry').length};
    const box = image.getBoundingClientRect();
    return {boxes: image.getClientRects().length, width: Math.round(box.width), height: Math.round(box.height), natural: image.naturalWidth};
})()"#;

/// True once the folded card's summary has held its place for two polls in a row, with nothing
/// still waiting for its read but what the folded card keeps for its opening: a click aimed at
/// it then lands on it.
const SUMMARY_HOLDS_STILL: &str = r#"(() => {
    const box = document.querySelector('details#folded-picture > summary').getBoundingClientRect();
    const at = `${box.left},${box.top},${box.width},${box.height}`, was = window.summaryAt;
    window.summaryAt = at;
    const waiting = [...document.querySelectorAll('.doc-body [data-pending=true]')]
        .filter(region => !region.closest('details:not([open])'));
    return waiting.length === 0 && was === at;
})()"#;

#[test]
fn document_card_folded_and_opened_by_a_click_draws_its_picture() {
    let (site, opened) = folded_card_site();
    let Some(mut page) = chrome() else { return };
    page.viewport(1440, 900, false);
    page.open(&site.url(&format!("/{FOLDED}")));
    let summary = "details#folded-picture > summary";
    assert!(
        page.wait_for(&crate::visible(summary), SHOWN),
        "the folded card is drawn"
    );
    assert_eq!(
        page.eval("(() => { const card = document.getElementById('folded-picture'); return [card.open, card.querySelector('.history') !== null]; })()"),
        json!([false, false]),
        "the card starts folded, its history not drawn"
    );
    // A real click on its summary, as a reader opens it, once the page around it is drawn; the
    // card's section is read only then, as the page draws its history.
    assert!(
        page.wait_for(SUMMARY_HOLDS_STILL, SHOWN),
        "the folded card's summary holds still"
    );
    let card_read = |site: &Site| {
        site.targets()
            .iter()
            .any(|target| target.starts_with("/api/section") && target.contains("folded-picture"))
    };
    assert!(
        !card_read(&site),
        "a folded card's section is read only once it is opened: {:?}",
        site.targets()
    );
    site.set_json("/api/section", &opened);
    let point = page.eval(&format!(
        "(() => {{ const box = document.querySelector({}).getBoundingClientRect(); return {{x: box.left + 40, y: box.top + box.height / 2}}; }})()",
        json!(summary)
    ));
    for kind in ["mousePressed", "mouseReleased"] {
        page.call(
            "Input.dispatchMouseEvent",
            json!({"type":kind,"x":point["x"],"y":point["y"],"button":"left","clickCount":1}),
        );
    }
    assert!(
        page.wait_for("document.getElementById('folded-picture').open", SHOWN),
        "the click at {point} opens the card"
    );
    // Waited for by what needs no layout: a page's layout read in the frames the picture's src
    // lands in gives the picture back its box, and would hide its loss.
    let image = "document.querySelector('details#folded-picture .history img[src*=\"split.svg\"]')";
    assert!(
        page.wait_for(&format!("{image} !== null"), SHOWN),
        "the opened card reads its section, draws its history and gives its picture its src: {}; {:?}",
        page.eval(FOLDED_IMAGE),
        site.targets()
    );
    assert!(card_read(&site));
    // Lost, the picture has no box at all and is never requested, however long it is waited for.
    assert!(
        page.wait_for(&format!("{image}?.naturalWidth === 600"), SHOWN),
        "a folded card opened by a click loads its picture: {}",
        page.eval(FOLDED_IMAGE)
    );
    let seen = page.eval(FOLDED_IMAGE);
    assert!(
        seen["boxes"].as_u64() > Some(0)
            && seen["width"].as_u64() > Some(0)
            && seen["height"].as_u64() > Some(0),
        "and draws it: {seen}"
    );
}

/// A card's owner, as `/api/card-owners` answers it: drawn beside its actions with what gave it, a
/// card the read names no owner of drawn as before, and for a reader who may hand it on, a choice
/// in its select posted as one hand-over naming the document, the card and the name, said beside
/// it once done.
#[test]
fn a_cards_owner_is_drawn_beside_its_actions_and_a_choice_hands_it_on() {
    let site = probe_site();
    site.set_json(
        symphony_api::CARD_OWNERS_PATH,
        &json!({"cards":{"alpha":{"name":"design","kind":"group","by":"map",
            "said":"from the folder docs","hand_to":["engineering","ana"]}}}),
    );
    site.set_json(
        symphony_api::CARD_HAND_ROUTE,
        &json!({"ok":true,"repeat":false}),
    );
    let Some(mut page) = chrome() else { return };
    page.open(&site.url(&format!("/{DOC}")));
    let owner = "[data-doc-key=\"card:alpha\"] .doc-actions .owner";
    assert!(
        page.wait_for(&crate::visible(owner), SHOWN),
        "the card alpha says whose it is: {}",
        page.eval("document.body.innerText.slice(0, 1500)")
    );
    let text = |page: &mut Chrome, css: &str| {
        page.eval(&format!(
            "document.querySelector({}).textContent",
            json!(css)
        ))
    };
    assert_eq!(
        text(&mut page, &format!("{owner} .owner-name")),
        "group: design"
    );
    assert_eq!(
        text(&mut page, &format!("{owner} .owner-by")),
        "from the folder docs"
    );
    assert_eq!(
        page.eval("document.querySelector('[data-doc-key=\"card:beta\"] .owner') === null"),
        json!(true),
        "a card the read names no owner of draws none"
    );
    let trigger = format!("{owner} [id=\"hand-docs%2FPROBE.md%3Aalpha\"]");
    page.eval(&format!(
        "document.querySelector({}).click()",
        json!(trigger)
    ));
    super::components::popup_ready(&mut page);
    let point = page.eval("(() => {const option=[...document.querySelectorAll('[role=listbox] [role=option]')].find(option=>option.textContent.trim()==='engineering');const box=option.getBoundingClientRect();return {x:box.x+box.width/2,y:box.y+box.height/2}})()");
    for kind in ["mousePressed", "mouseReleased"] {
        page.call(
            "Input.dispatchMouseEvent",
            json!({"type":kind,"x":point["x"],"y":point["y"],"button":"left","clickCount":1}),
        );
    }
    assert!(
        page.wait_for(
            &format!(
                "document.querySelector({})?.textContent === 'Handed to engineering'",
                json!(format!("{owner} [role=status]"))
            ),
            SHOWN
        ),
        "{}",
        text(&mut page, owner)
    );
    let handed: Vec<Value> = (site.posts().into_iter())
        .filter(|(path, _)| path == symphony_api::CARD_HAND_ROUTE)
        .map(|(_, body)| body)
        .collect();
    assert_eq!(
        handed,
        [json!({"path": DOC, "slug": "alpha", "to": "engineering"})]
    );
}
