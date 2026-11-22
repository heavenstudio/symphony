//! A document and an entrypoint's folder page drawn from this device's cache on a return visit
//! (docs/analysis/client-cache.md, "A return visit, step by step"; the web client's
//! apps/symphony-web/src/document-live.ts). With the
//! server's reads held back, a document draws its outline and the bodies of the sections it drew
//! before any answer, marked as possibly out of date, its card's verdicts waiting and its note
//! editable. Then it reads its outline naming its copy's tag: an unchanged document is a `304`
//! however far the feed's cursor moved, its feed opened once, from the cursor that answer names,
//! naming every section drawn from this device even when the answer comes before they name
//! themselves near the screen, and no section is read again (a `304` naming no cursor reads the outline again whole at once, with no
//! error shown); a section edited while away, or a card whose thread was answered, is read
//! alone, redrawn and kept. A document gone while away says so and is forgotten. An entrypoint's
//! folder page is kept pinned, its intro with it, so it outlives the documents visited once over
//! the 20-document cap. The release build, on the private stand-in (`site`), which serves each
//! document as the server does (`Site::set_document`).

use serde_json::{Value, json};

use crate::cache_pages::{
    PARTS, VALUES, answered, hold, keeps, kept, notice, oldest, return_held, shows, since,
    store_opened,
};
use crate::chrome::{Chrome, WEB_SOCKET};
use crate::site::{Answered, Site, asked_keys, query};

const DOC: &str = "docs/kept.md";
/// What this device keeps of it, as [`PARTS`] names them.
const KEPT: [&str; 2] = [
    "document:docs/kept.md outline",
    "document:docs/kept.md sections",
];
/// The reads a return visit checks, which the stand-in holds back to show what the page draws
/// before any answer.
pub(super) const READS: [&str; 2] = ["/api/document", "/api/section"];
/// The document page is current: its feed open, nothing shown from the device.
pub(super) const LIVE: &str = "(page => page?.dataset.feed === 'live' && !page.dataset.cache && !document.querySelector('.device-notice'))(document.querySelector('main > .document-page'))";
/// The card's verdicts and its submit button, as `[text, disabled]`.
const VERDICTS: &str = "[...document.querySelectorAll(\"section[data-doc-key='card:palette'] form.verdict .verdict-choices > button, section[data-doc-key='card:palette'] form.verdict button[type=submit]\")].map(b => [b.textContent.trim(), b.disabled])";

fn paragraph(text: &str) -> Value {
    json!({"type": "paragraph", "spans": [{"type": "text", "text": text}]})
}

/// A prose section `key` of `hash` from line `start`, saying `text`.
pub(super) fn prose(key: &str, hash: &str, start: usize, text: &str) -> Value {
    json!({"start": start, "end": start + 1, "key": key, "hash": hash, "part": 0, "blocks": [paragraph(text)]})
}

/// A card's thread: an agent's line saying each of `said`.
fn thread(said: &[&str]) -> Value {
    let entries: Vec<Value> = said
        .iter()
        .map(|text| json!({"who": "agent", "timestamp": "2026-11-12", "verdict": null, "blocks": [paragraph(text)]}))
        .collect();
    json!({"blocks": [{"type": "thread", "entries": entries}]})
}

/// The document `path` titled `title` at the feed's cursor `version`, as the server reads it
/// whole: an intro, a card waiting on a verdict with an agent's line on its thread, and lamps.
pub(super) fn document(path: &str, title: &str, version: u64) -> Value {
    json!({
        "version": version, "path": path, "title": title, "waiting": [], "crumbs": [],
        "headings": [], "tasks": [],
        "sections": [
            prose("intro", "i1", 1, &format!("{title}: the intro.")),
            {"start": 10, "end": 20, "key": "card:palette", "hash": "p1", "part": 1, "blocks": [{
                "type": "card", "slug": "palette", "mark": "pending", "title": "Palette",
                "body": [paragraph("Choose the palette.")],
                "verdict": {"group": "decide", "open": true, "posted": null, "decided": null},
                "thread": [], "line": 10, "line_offset": null, "comparison": []
            }]},
            prose("lamps", "l1", 30, "Lamps, as kept."),
        ],
        "cards": [{"slug": "palette", "state": "pending", "text": "Palette", "group": "decide", "open": true}],
        "threads": {"palette": thread(&["Posted."])},
    })
}

/// The stand-in, serving the fixture document.
fn site() -> Site {
    let site = crate::site();
    site.set_document(&document(DOC, "Kept", 4182));
    site.set_json("/api/asks", &json!({"asks": [], "recipient": null}));
    site.set_json("/api/postponed", &json!([]));
    site
}

/// The document page shows each of `texts` in its sections, scrolled to its end so each nears
/// the screen. By their text, not as laid out: a thread scrolled past is drawn but not rendered
/// (`content-visibility`), and its words leave `innerText`.
#[track_caller]
pub(super) fn draws(chrome: &mut Chrome, texts: &[&str]) {
    shows(
        chrome,
        &format!(
            "(texts => (window.scrollTo(0, document.body.scrollHeight), texts.every(text => [...document.querySelectorAll('main .doc-section')].some(section => section.textContent.includes(text)))))({})",
            json!(texts)
        ),
        &format!("the sections saying {texts:?}"),
    );
}

/// Wait until this device keeps the bodies of the fixture document's sections saying each of
/// `texts`. A page keeps what it read in a transaction of the store's after it draws it, and the
/// store aborts every transaction open when the page goes away (`pagehide`, `shell/cache.ts`): a
/// visit left as soon as it drew keeps nothing of what was not committed yet. Its sections part is
/// kept again for each change of the bodies held, so only what it holds, not when it was kept,
/// tells the last of them in.
#[track_caller]
fn keeps_bodies(chrome: &mut Chrome, texts: &[&str]) {
    shows(
        chrome,
        &format!(
            "{VALUES}.then(kept => (bodies => {}.every(text => bodies.includes(text)))(JSON.stringify(kept[{}] ?? null)))",
            json!(texts),
            json!(KEPT[1])
        ),
        &format!("the bodies saying {texts:?} kept"),
    );
}

/// A first visit to the fixture document: drawn from the server, live, and kept.
fn visit(chrome: &mut Chrome, site: &Site) {
    store_opened(chrome, site);
    chrome.open(&site.url(&format!("/{DOC}")));
    let texts = ["Kept: the intro.", "Posted.", "Lamps, as kept."];
    draws(chrome, &texts);
    shows(chrome, LIVE, "the first visit");
    keeps(chrome, &KEPT);
    keeps_bodies(chrome, &texts);
}

/// The page shows what this device kept, saying `said` of it.
#[track_caller]
fn from_the_device(chrome: &mut Chrome, said: &str) {
    shows(
        chrome,
        &format!(
            "(page => page?.dataset.feed === 'stale' && page.dataset.cache === 'device' && document.querySelector('.page-notices .device-notice')?.textContent === {})(document.querySelector('main > .document-page'))",
            json!(said)
        ),
        "the notice of a copy from this device",
    );
}

/// The card's editor, opened.
fn open_card(chrome: &mut Chrome) {
    chrome.eval(
        "[...document.querySelectorAll(\"section[data-doc-key='card:palette'] .doc-actions button\")].find(b => b.getAttribute('aria-expanded') === 'false')?.click()",
    );
    shows(
        chrome,
        "!!document.querySelector(\"section[data-doc-key='card:palette'] form.verdict:not([hidden]) textarea\")",
        "the card's editor",
    );
}

/// The card's verdicts wait, each saying the page is checking, and its note takes typing.
#[track_caller]
fn verdicts_wait(chrome: &mut Chrome) {
    open_card(chrome);
    let held = chrome.eval(VERDICTS);
    let held = held.as_array().unwrap();
    assert_eq!(
        held.len(),
        4,
        "approve, reject, question and send: {held:?}"
    );
    for verdict in held {
        assert_eq!(verdict, &json!(["Checking…", true]), "{held:?}");
    }
    let typed = chrome.eval(
        "(field => { field.focus(); document.execCommand('insertText', false, 'kept note'); return [field.disabled, field.value]; })(document.querySelector(\"section[data-doc-key='card:palette'] form.verdict textarea\"))",
    );
    assert_eq!(
        typed,
        json!([false, "kept note"]),
        "the note stays editable"
    );
}

/// The outline was read once, naming the kept copy's tag, and answered `status`.
#[track_caller]
pub(super) fn outline_once(answers: &[Answered], status: u16) {
    assert_eq!(answers.len(), 1, "{answers:#?}");
    let answer = &answers[0];
    assert!(
        answer
            .named
            .as_deref()
            .is_some_and(|tag| tag.starts_with("W/\"")),
        "{answer:?} names the kept copy's tag"
    );
    assert_eq!(answer.status, status, "{answer:?}");
}

/// The feeds the page shown opened, in order: the sockets Chrome made for it since it was opened,
/// each by its address. Never the stand-in's log of requests: a page re-subscribes its feed once
/// its sections name themselves, 200 ms or more after it is live, and the stand-in reads that
/// request in a thread of its own, so the first visit's last feed can enter the log after the
/// return visit began, and be counted as the return's (run 52196220, at a load average over 100).
fn feeds(chrome: &mut Chrome) -> Vec<String> {
    chrome
        .requests()
        .into_iter()
        .filter(|request| request.kind == WEB_SOCKET)
        .map(|request| request.url)
        .filter(|url| is_feed(url))
        .collect()
}

/// Whether a socket's address `url` is the feed's, whatever its query.
fn is_feed(url: &str) -> bool {
    url.split_once("://")
        .and_then(|(_, rest)| rest.find('/').map(|at| &rest[at..]))
        .is_some_and(|target| target.split('?').next() == Some(symphony_api::FEED_PATH))
}

/// The cursor each of `feeds` opened from, in order.
fn feed_cursors(feeds: &[String]) -> Vec<String> {
    feeds
        .iter()
        .filter_map(|target| query(target, "since"))
        .collect()
}

/// The sections each of `feeds` named, in order.
fn feed_sections(feeds: &[String]) -> Vec<Vec<String>> {
    feeds
        .iter()
        .map(|target| {
            query(target, "sections")
                .and_then(|keys| serde_json::from_str(&keys).ok())
                .unwrap_or_default()
        })
        .collect()
}

/// The key of each section the document page drew, or `null` while a section that came near the
/// screen still waits for its body (its ghost, `.section-loading`). A section is wanted from the
/// moment it comes near the screen and stays so, and drawn once its body is read, so with none
/// waiting the sections drawn are those the page wants its feed to name (document-live.ts
/// `followed`).
const DRAWN: &str = "document.querySelector('main .section-loading') ? null : [...document.querySelectorAll('main section.doc-section[data-doc-key]')].map(section => section.dataset.docKey)";

/// Wait until the page's feed follows every section it drew, none still waiting for its body: it
/// then has no section to open its feed again for. A page opens its feed again naming the
/// sections that came near the screen on the first quiet 200 ms of its stream after they did
/// (document-live.ts `followDocument`), which a loaded machine leaves until after their bodies
/// are drawn: the server sends the stream opened again what was pushed in between, the stand-in
/// does not (`Site::following`), and an event pushed there never reaches the page (the large
/// document read ahead, release run 2026-11-22T1030-89384, at a load average over 100).
fn follows_what_it_drew(chrome: &mut Chrome, site: &Site) {
    let mut drawn = Value::Null;
    test_time::until_true(
        "the page's feed following every section it drew",
        crate::STALL,
        || {
            drawn = chrome.eval(DRAWN);
            drawn.as_array().is_some_and(|keys| {
                keys.iter()
                    .all(|key| key.as_str().is_some_and(|key| site.following(key)))
            })
        },
    )
    .unwrap_or_else(|stall| {
        let feeds: Vec<String> = site
            .targets()
            .into_iter()
            .filter(|target| target.starts_with(symphony_api::FEED_PATH))
            .collect();
        panic!("{stall}: drawn {drawn}; the feed requests {feeds:?}")
    });
}

/// A feed event retitling the document `path`, pushed once the page's feed follows every section
/// it drew ([`follows_what_it_drew`]): once the page shows it, the page has followed its feed past
/// the outline's answer, every section read it would make made.
pub(super) fn retitled(chrome: &mut Chrome, site: &Site, path: &str, version: u64, title: &str) {
    follows_what_it_drew(chrome, site);
    site.push(
        version,
        &json!({"version": version, "changes": [{"region": "document", "data": {"path": path, "title": title}}]})
            .to_string(),
    );
    shows(
        chrome,
        &format!(
            "document.querySelector('main .doc-head h1')?.textContent === {}",
            json!(title)
        ),
        "the feed's title",
    );
}

#[test]
fn a_warm_document_draws_its_outline_and_sections_before_any_answer_and_an_unchanged_one_is_a_304_reading_no_section()
 {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit(&mut chrome, &site);
    // The board moved while away, the document did not: its cursor is all that changed.
    site.set_document(&document(DOC, "Kept", 4190));
    let saved = oldest(&mut chrome, |part| KEPT.contains(&part));
    let before = return_held(&mut chrome, &site, &READS, &READS, &format!("/{DOC}"));
    draws(
        &mut chrome,
        &["Kept: the intro.", "Posted.", "Lamps, as kept."],
    );
    let said = notice(&mut chrome, saved, false);
    from_the_device(&mut chrome, &said);
    verdicts_wait(&mut chrome);
    assert_eq!(
        answered(&site, &READS),
        before,
        "drawn before any read was answered"
    );
    hold(&site, &READS, false);
    shows(&mut chrome, LIVE, "the warm visit, once checked");
    retitled(&mut chrome, &site, DOC, 4191, "Kept, retitled");
    outline_once(&since(&site, &READS, &before, "/api/document"), 304);
    assert_eq!(
        since(&site, &READS, &before, "/api/section"),
        [],
        "every section kept is current: none is read again"
    );
    assert_eq!(
        feed_cursors(&feeds(&mut chrome))
            .first()
            .map(String::as_str),
        Some("4190"),
        "the feed opens from the cursor the 304 names, not the kept copy's"
    );
    let verdicts = chrome.eval(VERDICTS);
    assert_eq!(
        verdicts[0],
        json!(["✓ Approve", false]),
        "the verdicts, once current: {verdicts}"
    );
    assert_eq!(
        chrome.eval(
            "document.querySelector(\"section[data-doc-key='card:palette'] form.verdict textarea\").value"
        ),
        json!("kept note"),
        "the note typed meanwhile"
    );
}

#[test]
fn a_warm_document_checked_at_once_opens_one_feed_naming_the_sections_it_drew_from_this_device() {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit(&mut chrome, &site);
    chrome.open("about:blank");
    // Nothing held: the 304 comes back as fast as the stand-in answers, before the sections' slots
    // have come near the screen and named themselves.
    let before = answered(&site, &READS);
    chrome.open(&site.url(&format!("/{DOC}")));
    shows(&mut chrome, LIVE, "the warm visit, checked");
    retitled(&mut chrome, &site, DOC, 4183, "Kept, retitled");
    outline_once(&since(&site, &READS, &before, "/api/document"), 304);
    let opened = feed_sections(&feeds(&mut chrome));
    assert_eq!(
        opened,
        [vec!["card:palette", "intro", "lamps"]],
        "one feed, naming every section this device drew from the start: {opened:?}"
    );
}

#[test]
fn a_304_naming_no_cursor_reads_the_outline_again_whole_at_once_and_says_nothing_went_wrong() {
    let site = site();
    // The outline as an older server tags it: by its bytes, its cursor in no header, so a read
    // naming its tag is a `304` that names no cursor either.
    site.set_json(
        "/api/document",
        &crate::site::outline_of(&document(DOC, "Kept", 4182)),
    );
    site.names_no_cursor("/api/document");
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit(&mut chrome, &site);
    let before = return_held(&mut chrome, &site, &READS, &READS, &format!("/{DOC}"));
    draws(
        &mut chrome,
        &["Kept: the intro.", "Posted.", "Lamps, as kept."],
    );
    chrome.eval(
        "window.alerted = []; new MutationObserver(() => document.querySelectorAll('.page-notices [role=alert]').forEach(alert => alerted.push(alert.textContent))).observe(document.body, {subtree: true, childList: true, characterData: true})",
    );
    hold(&site, &READS, false);
    shows(&mut chrome, LIVE, "the warm visit, once read again");
    let outline = since(&site, &READS, &before, "/api/document");
    assert_eq!(
        outline
            .iter()
            .map(|answer| (answer.status, answer.named.is_some()))
            .collect::<Vec<_>>(),
        [(304, true), (200, false)],
        "the 304 naming no cursor, then the outline whole: {outline:#?}"
    );
    assert_eq!(
        since(&site, &READS, &before, "/api/section"),
        [],
        "the outline whole says every section kept is current"
    );
    assert_eq!(
        chrome.eval("window.alerted"),
        json!([]),
        "no error is shown meanwhile"
    );
}

#[test]
fn a_section_edited_or_a_card_answered_while_away_is_read_alone_redrawn_and_kept() {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit(&mut chrome, &site);
    // Lamps edited while away: the outline answers 200, and lamps alone is read.
    let mut edited = document(DOC, "Kept", 4182);
    edited["sections"][2] = prose("lamps", "l2", 30, "Lamps, edited while away.");
    site.set_document(&edited);
    let saved = oldest(&mut chrome, |part| KEPT.contains(&part));
    let before = return_held(&mut chrome, &site, &READS, &READS, &format!("/{DOC}"));
    let said = notice(&mut chrome, saved, false);
    from_the_device(&mut chrome, &said);
    draws(&mut chrome, &["Kept: the intro.", "Lamps, as kept."]);
    hold(&site, &READS, false);
    draws(&mut chrome, &["Lamps, edited while away."]);
    shows(&mut chrome, LIVE, "the edited section, once read");
    outline_once(&since(&site, &READS, &before, "/api/document"), 200);
    let sections = since(&site, &READS, &before, "/api/section");
    assert_eq!(
        sections
            .iter()
            .map(|answer| (asked_keys(&answer.target), answer.status))
            .collect::<Vec<_>>(),
        [(vec!["lamps".to_string()], 200)],
        "{sections:#?}"
    );
    keeps_bodies(&mut chrome, &["Lamps, edited while away."]);

    // The card's thread answered while away: the card alone is read, its new line drawn.
    let mut answered_card = edited.clone();
    answered_card["threads"]["palette"] = thread(&["Posted.", "Answered while away."]);
    site.set_document(&answered_card);
    let saved = oldest(&mut chrome, |part| KEPT.contains(&part));
    let before = return_held(&mut chrome, &site, &READS, &READS, &format!("/{DOC}"));
    draws(&mut chrome, &["Lamps, edited while away.", "Posted."]);
    let said = notice(&mut chrome, saved, false);
    from_the_device(&mut chrome, &said);
    hold(&site, &READS, false);
    draws(&mut chrome, &["Answered while away."]);
    shows(&mut chrome, LIVE, "the answered card, once read");
    let sections = since(&site, &READS, &before, "/api/section");
    assert_eq!(
        sections
            .iter()
            .map(|answer| asked_keys(&answer.target))
            .collect::<Vec<_>>(),
        [vec!["card:palette".to_string()]],
        "{sections:#?}"
    );
    keeps_bodies(
        &mut chrome,
        &["Lamps, edited while away.", "Answered while away."],
    );

    // The third visit draws both from the device, and reads no section.
    let before = return_held(&mut chrome, &site, &READS, &READS, &format!("/{DOC}"));
    draws(
        &mut chrome,
        &["Lamps, edited while away.", "Answered while away."],
    );
    hold(&site, &READS, false);
    shows(&mut chrome, LIVE, "the third visit");
    retitled(&mut chrome, &site, DOC, 4183, "Kept, retitled");
    outline_once(&since(&site, &READS, &before, "/api/document"), 304);
    assert_eq!(since(&site, &READS, &before, "/api/section"), []);
}

#[test]
fn a_document_gone_while_away_says_so_and_this_device_forgets_it() {
    let site = site();
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit(&mut chrome, &site);
    site.miss_route("/api/document", Some("no such document"));
    let saved = oldest(&mut chrome, |part| KEPT.contains(&part));
    return_held(&mut chrome, &site, &READS, &READS, &format!("/{DOC}"));
    let said = notice(&mut chrome, saved, false);
    from_the_device(&mut chrome, &said);
    hold(&site, &READS, false);
    shows(
        &mut chrome,
        "document.querySelector('main').innerText.includes('Document not found') && !document.querySelector('main .doc-section')",
        "the document gone",
    );
    shows(
        &mut chrome,
        &format!(
            "{PARTS}.then(kept => !Object.keys(kept).some(part => part.startsWith({})))",
            json!(format!("document:{DOC} "))
        ),
        "the document forgotten",
    );
}

/// The `n`th of the documents opened in turn.
fn numbered(n: usize) -> String {
    format!("docs/d{n:02}.md")
}

/// Open `path` from the page shown, by a link the shell follows in place.
fn follow(chrome: &mut Chrome, path: &str) {
    chrome.eval(&format!(
        "(link => {{ link.href = {}; document.body.append(link); link.click(); link.remove(); }})(document.createElement('a'))",
        json!(path)
    ));
}

/// The items this device keeps of documents, by their ids, in order.
fn documents_kept(chrome: &mut Chrome) -> Vec<String> {
    let mut items: Vec<String> = kept(chrome)
        .keys()
        .filter(|part| part.starts_with("document:"))
        .filter_map(|part| part.rsplit_once(' ').map(|(item, _)| item.to_string()))
        .collect();
    items.dedup();
    items
}

#[test]
fn an_entrypoints_page_outlives_documents_visited_once_over_the_20_document_cap() {
    let site = site();
    let intro = document("docs/README.md", "Documents", 4182);
    site.set_document(&intro);
    let mut outline = intro.clone();
    for section in outline["sections"].as_array_mut().unwrap() {
        section["blocks"] = json!([]);
    }
    outline["threads"] = json!({});
    site.set_json(
        "/api/folder",
        &json!({"path": "docs", "title": "Documents", "crumbs": [], "docs": 20, "intro": outline,
                "entries": [{"kind": "document", "path": numbered(1), "title": "Doc 01", "docs": 1, "waiting": 0, "summary": ""}]}),
    );
    for n in 1..=20 {
        site.set_document(&document(&numbered(n), &format!("Doc {n:02}"), 4182));
    }
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    store_opened(&mut chrome, &site);
    chrome.open(&site.url("/docs/"));
    draws(&mut chrome, &["Documents: the intro."]);
    let entrypoint = [
        "document:docs/ folder",
        "document:docs/ outline",
        "document:docs/ sections",
    ];
    keeps(&mut chrome, &entrypoint);
    for n in 1..=20 {
        let path = numbered(n);
        follow(&mut chrome, &format!("/{path}"));
        shows(
            &mut chrome,
            &format!("document.querySelector('main .doc-head h1')?.textContent === 'Doc {n:02}'"),
            &path,
        );
        keeps(&mut chrome, &[&format!("document:{path} outline")]);
    }
    let first = format!("document:{} ", numbered(1));
    shows(
        &mut chrome,
        &format!(
            "{PARTS}.then(kept => !Object.keys(kept).some(part => part.startsWith({})))",
            json!(first)
        ),
        "the first document dropped",
    );
    let mut expected: Vec<String> = (2..=20)
        .map(|n| format!("document:{}", numbered(n)))
        .chain(["document:docs/".to_string()])
        .collect();
    expected.sort();
    let mut items = documents_kept(&mut chrome);
    items.sort();
    assert_eq!(
        items, expected,
        "20 documents kept, the entrypoint among them and the first visited gone"
    );

    // The entrypoint opened again, its reads held, is drawn from the device, its intro with it.
    for read in ["/api/folder", "/api/document", "/api/section"] {
        site.hold_route(read, true);
    }
    chrome.open("about:blank");
    chrome.open(&site.url("/docs/"));
    draws(&mut chrome, &["Documents: the intro."]);
    shows(
        &mut chrome,
        "(page => page?.dataset.cache === 'device' && !!document.querySelector('.page-notices .device-notice'))(document.querySelector('main > .document-page'))",
        "the entrypoint from the device",
    );
    for read in ["/api/folder", "/api/document", "/api/section"] {
        site.hold_route(read, false);
    }
    shows(
        &mut chrome,
        "(page => !page.dataset.cache && !document.querySelector('.device-notice'))(document.querySelector('main > .document-page'))",
        "the entrypoint, once checked",
    );
    let folder = site.answers("/api/folder");
    let last = folder.last().unwrap();
    assert_eq!(
        (last.named.is_some(), last.status),
        (true, 304),
        "an unchanged folder is a 304: {folder:#?}"
    );
}
