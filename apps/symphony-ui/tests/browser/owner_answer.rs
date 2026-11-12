//! An owner's answer on a card shows at once on the page it was given on, and through the feed on
//! a second open page, with no reload: the card's new mark and line, and the register regrouped.
//! So on main's page and on a branch's, and whatever else the page holds open meanwhile: an excerpt
//! read beside the answer, or a draft left on another card.
use super::{SHOWN, chrome, chrome::Chrome, restored::Native};
use serde_json::{Value, json};

const REGISTER: &str = "# Decisions\n\n<!-- register:start -->\n## Needs your decision\n\n### Topic\n\n- [ ] **alpha** — First question, asked in [[NOTES#Open questions]].\n- [ ] **bravo** — Second question.\n<!-- register:end -->\n";
const NOTES: &str = "# Notes\n\n## Open questions\n\n1. Is it red?\n";
/// The branch's register: an unmerged commit of its own, so its pages are the branch's.
const BRANCH: &str = "feature/answer";

/// What the page shows of the card `slug`, as a JS expression: its mark, the register group it sits
/// under (the last group heading above it, without its count), each owner line of its thread as its
/// verdict and its words, and whether the page is still the visit [`opened`] marked.
fn card(slug: &str) -> String {
    format!(
        "(() => {{ const card = document.querySelector('[data-doc-key=\"card:{slug}\"]');
            const above = card ? [...document.querySelectorAll('.doc-body h2')].filter(h => h.compareDocumentPosition(card) & Node.DOCUMENT_POSITION_FOLLOWING) : [];
            return {{state: card?.dataset.state ?? null, group: above.length ? above[above.length - 1].textContent.trim().replace(/ \\(\\d+\\)$/, '') : null,
                owner: card ? [...card.querySelectorAll('.history .entry.owner')].map(e => [e.querySelector('.entry-verdict')?.textContent ?? '', e.querySelector('.entry-text')?.textContent.trim() ?? '']) : [],
                visit: window.sameVisit === true}}; }})()"
    )
}

/// `selector` within the card `slug`, as a JS string literal.
fn within(slug: &str, selector: &str) -> Value {
    json!(format!("[data-doc-key=\"card:{slug}\"] {selector}"))
}

/// Open `url` in a wide window and wait for the card `slug` and a live feed; marks the visit so a
/// reload shows.
fn opened(page: &mut Chrome, url: &str, slug: &str) {
    page.open(url);
    page.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width": 1280, "height": 900, "deviceScaleFactor": 1, "mobile": false}),
    );
    assert!(
        page.wait_for(
            &format!(
                "document.querySelector({}) !== null && document.querySelector('.document-page')?.dataset.feed === 'live'",
                within(slug, ".doc-actions button")
            ),
            SHOWN
        ),
        "{}",
        page.eval("document.body.innerText")
    );
    page.eval("window.sameVisit = true");
}

/// Open the card `slug`'s form and type `note` into its box, as the owner does.
fn type_note(page: &mut Chrome, slug: &str, note: &str) {
    page.eval(&format!(
        "document.querySelector({}).click()",
        within(slug, ".doc-actions button")
    ));
    let field = within(slug, "form.verdict textarea");
    assert!(page.wait_for(&format!("document.querySelector({field}) !== null"), SHOWN));
    page.eval(&format!(
        "(() => {{ const t = document.querySelector({field}); t.focus(); t.value = {}; t.dispatchEvent(new Event('input', {{bubbles: true}})); return true; }})()",
        json!(note)
    ));
}

/// Give the card `slug` the verdict whose button says `button`, with `note`.
fn answer(page: &mut Chrome, slug: &str, button: &str, note: &str) {
    type_note(page, slug, note);
    page.eval(&format!(
        "[...document.querySelectorAll({})].find(b => b.textContent.includes({})).click()",
        within(slug, ".verdict-choices button"),
        json!(button)
    ));
}

/// An object as JSON with its keys sorted, as a JS function, so the page's and the expected one
/// compare whatever order each was written in: serde_json writes keys sorted, but in the order they
/// were given where the whole workspace's build turns on its `preserve_order`, as `just verify`'s does.
const SORTED: &str = "(o => JSON.stringify(o, Object.keys(o).sort()))";

/// Wait until `page` shows `expected` of the card `slug`; what it shows instead when it never does.
/// Each look scrolls the card to the middle of the window first, wherever a regroup moved it: its
/// thread renders only once it is near the screen, as the owner reading it has it.
fn shows(page: &mut Chrome, slug: &str, expected: &Value) -> Result<(), Value> {
    let wanted = format!(
        "(() => {{ document.querySelector({})?.scrollIntoView({{block: 'center'}}); const s = {}; return {SORTED}(s) === {SORTED}({expected}); }})()",
        json!(format!("[data-doc-key=\"card:{slug}\"]")),
        card(slug),
    );
    if page.wait_for(&wanted, SHOWN) {
        Ok(())
    } else {
        Err(page.eval(&card(slug)))
    }
}

/// What `page` shows around the card `slug`, for a failure to say why it is not what was wanted: the
/// feed's state, the card's text (a refused or unanswered save's notice among it) and how long each
/// save the page sent that the server answered took, in ms.
fn around(page: &mut Chrome, slug: &str) -> Value {
    page.eval(&format!(
        "(() => {{ const card = document.querySelector({}); return {{feed: document.querySelector('.document-page')?.dataset.feed ?? null, text: card?.innerText ?? null,
            saves: performance.getEntriesByType('resource').filter(e => e.name.includes('/api/decision')).map(e => Math.round(e.duration))}}; }})()",
        json!(format!("[data-doc-key=\"card:{slug}\"]"))
    ))
}

/// Two tabs on `page` of a server whose main and branch both hold the register; `meanwhile` is what
/// the owner's tab holds open before the owner approves alpha there. Both tabs show the answer and
/// the regroup, and bravo is left as it was. The owner's tab, for what else it should still show.
fn answered_on_both_tabs(page: &str, meanwhile: impl FnOnce(&mut Chrome)) -> Option<Chrome> {
    let mut owner = chrome()?;
    let mut other = chrome()?;
    let branch_register = REGISTER.replace("Second question.", "Second question, on the branch.");
    let site = Native::start_committed(
        None,
        "main",
        &[("docs/DECISIONS.md", REGISTER), ("docs/NOTES.md", NOTES)],
        Some((BRANCH, &[("docs/DECISIONS.md", &branch_register)])),
    );
    let url = site.url(page);
    opened(&mut owner, &url, "alpha");
    opened(&mut other, &url, "alpha");
    let pending = json!({"state":"pending","group":"Needs your decision","owner":[],"visit":true});
    assert_eq!(shows(&mut owner, "alpha", &pending), Ok(()));
    meanwhile(&mut owner);

    let answered_at = std::time::Instant::now();
    answer(&mut owner, "alpha", "Approve", "Keep it");
    let answered = json!({"state":"approved","group":"Waiting on an agent","owner":[["approved","Keep it"]],"visit":true});
    let mine = shows(&mut owner, "alpha", &answered);
    let waited = answered_at.elapsed();
    let theirs = shows(&mut other, "alpha", &answered);
    let saved = site.root().join("docs/DECISIONS.md");
    let saved = if page.starts_with("/@") {
        site.root().with_extension("tree").join("docs/DECISIONS.md")
    } else {
        saved
    };
    assert!(
        mine.is_ok() && theirs.is_ok(),
        "{page}: {waited:?} after the answer the owner's tab shows {mine:?} ({}), the second tab {theirs:?} ({}); the file:\n{}\nthe server said:\n{}",
        around(&mut owner, "alpha"),
        around(&mut other, "alpha"),
        std::fs::read_to_string(saved).unwrap(),
        std::fs::read_to_string(site.root().join("artifacts/server.log")).unwrap_or_default()
    );
    let unanswered =
        json!({"state":"pending","group":"Needs your decision","owner":[],"visit":true});
    assert_eq!(shows(&mut owner, "bravo", &unanswered), Ok(()), "{page}");
    assert_eq!(shows(&mut other, "bravo", &unanswered), Ok(()), "{page}");
    Some(owner)
}

#[test]
fn an_owner_verdict_shows_and_regroups_on_its_page_and_a_second_tab_without_a_reload() {
    answered_on_both_tabs("/docs/DECISIONS.md", |_| {});
}

#[test]
fn an_owner_verdict_on_a_branch_page_shows_and_regroups_on_both_tabs() {
    let Some(mut owner) = answered_on_both_tabs(
        &format!("/@{}/docs/DECISIONS.md", BRANCH.replace('/', "~")),
        |_| {},
    ) else {
        return;
    };
    assert_eq!(
        owner.eval(&format!(
            "document.querySelector({}).textContent.includes('Second question, on the branch.')",
            within("bravo", ".section-prose")
        )),
        json!(true),
        "the page is the branch's"
    );
}

/// The owner read the card's linked notes in its excerpt while writing: the excerpt stays open, and
/// the answer still shows and regroups.
#[test]
fn an_owner_verdict_given_beside_an_open_excerpt_shows_and_regroups() {
    let Some(mut owner) = answered_on_both_tabs("/docs/DECISIONS.md", |owner| {
        owner.eval(&format!(
            "document.querySelector({}).click()",
            within("alpha", ".show-link")
        ));
        // The excerpt fills once its box is on screen: the owner reads it there.
        assert!(owner.wait_for(
            &format!(
                "(() => {{ const b = document.querySelector({}); b?.scrollIntoView({{block: 'center'}}); return b?.textContent.includes('Is it red?') === true; }})()",
                within("alpha", ".excerpt-box")
            ),
            SHOWN
        ));
    }) else {
        return;
    };
    // The excerpt opens in the card's line, beside its button, which says it is open.
    assert_eq!(
        owner.eval(&format!(
            "[document.querySelector({}) !== null, document.querySelector({})?.getAttribute('aria-expanded')]",
            within("alpha", ".excerpt-box"),
            within("alpha", ".show-link")
        )),
        json!([true, "true"]),
        "the excerpt the owner was reading stays open"
    );
}

/// A draft the owner left on another card holds that card's words, not the register's order.
#[test]
fn a_draft_left_on_another_card_does_not_stop_the_answer_regrouping() {
    let Some(mut owner) = answered_on_both_tabs("/docs/DECISIONS.md", |owner| {
        type_note(owner, "bravo", "half a thought");
        owner.eval(&format!(
            "document.querySelector({}).click()",
            within("bravo", ".doc-actions button")
        ));
        owner.eval("document.activeElement?.blur()");
        assert!(owner.wait_for(
            &format!(
                "[...document.querySelectorAll({})].some(b => b.textContent === 'Resume draft')",
                within("bravo", ".editor button")
            ),
            SHOWN
        ));
    }) else {
        return;
    };
    assert_eq!(
        owner.eval(&format!(
            "document.querySelector({}).value",
            within("bravo", "form.verdict textarea")
        )),
        json!("half a thought"),
        "the draft is kept"
    );
}
