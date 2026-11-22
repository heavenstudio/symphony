//! Read ahead (docs/analysis/client-cache.md, "Read ahead"; `ahead` in shell/shell.ts and
//! shell/cache.ts). Once a page has drawn, its feed is open and the browser is idle, the first
//! eight task and document links it shows on the screen are read into this device's cache one
//! after another, after every read of the page's own, at the lowest priority and marked as a
//! prefetch, and kept unvisited: a task whole (its page and its thread's newest page), a document
//! its first part (`view=first`), within `symphony_api::AHEAD_BYTES`. Opening one draws it from
//! the device before any answer, then checks it as a return visit does. A browser asking to save
//! data, or on a 3g or slower link, reads nothing ahead; a link that acts (a download, another
//! window), a search, the page itself, a folder and a link off the screen are never read. The
//! release build, on the private stand-in (`site`), which tells the reads ahead from the page's
//! own (`Site::read_ahead`).

use serde_json::{Map, Value, json};
use symphony_api::AHEAD_BYTES;

use crate::cache_documents::{self as documents, prose};
use crate::cache_pages::{self, LIVE, hold, keeps, shows, since, store_opened};
use crate::cache_tasks;
use crate::chrome::Chrome;
use crate::site::{Site, asked_keys, first_of};
use crate::{SHOWN, common, look_parity};

/// The look-parity fixture's task, which the stand-in serves whatever the slug asked.
const TASK: &str = "visual/in-progress";
/// The page the links are on.
const LINKS: &str = "docs/links.md";
/// A task page drawn from this device.
const TASK_FROM_THE_DEVICE: &str =
    "(main => main?.dataset.cache === 'device')(document.querySelector('main'))";
/// A document page drawn from this device.
const DOCUMENT_FROM_THE_DEVICE: &str =
    "(page => page?.dataset.cache === 'device')(document.querySelector('main > .document-page'))";

fn text(said: &str) -> Value {
    json!({"type": "text", "text": said})
}

/// A link to `href` saying `said`.
fn link(href: &str, said: &str) -> Value {
    json!({"type": "link", "href": href, "target": null, "title": "", "spans": [text(said)]})
}

/// A link that acts: [`link`] with the attribute `name` set to `value`.
fn acting(name: &str, value: &str, href: &str, said: &str) -> Value {
    let attributes = Map::from_iter([(name.to_string(), Value::from(value))]);
    json!({"type": "styled", "tag": "span", "presentation": {"attributes": attributes}, "spans": [link(href, said)]})
}

/// The document `path` titled `title` of `sections`.
fn page(path: &str, title: &str, sections: Vec<Value>) -> Value {
    json!({
        "version": 4182, "path": path, "title": title, "waiting": [], "crumbs": [],
        "headings": [], "tasks": [], "sections": sections, "cards": [], "threads": {},
    })
}

/// A section `key` of one paragraph of `spans`, from line `start`.
fn sentence(key: &str, start: usize, spans: Vec<Value>) -> Value {
    json!({"start": start, "end": start + 1, "key": key, "hash": key, "part": 0,
           "blocks": [{"type": "paragraph", "spans": spans}]})
}

/// `words` words of text unlike any other seed's, so gzip takes a fixture of them about as it
/// takes prose rather than to nothing.
fn varied(seed: usize, words: usize) -> String {
    let mut state = seed as u64 ^ 0x9e37_79b9_7f4a_7c15;
    (0..words)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let letters = 2 + (state >> 61) as usize;
            (0..letters)
                .map(|at| char::from(b'a' + ((state >> (8 + 5 * at)) % 26) as u8))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The links page: a sentence at its top linking to each of `links` in turn, then `below`
/// sections of prose and, under them, off the screen, a link to `docs/below.md`.
fn links_page(links: Vec<Value>, below: usize) -> Value {
    let mut spans = vec![text("Read next: ")];
    for each in links {
        spans.extend([each, text(", ")]);
    }
    let mut sections = vec![sentence("links", 1, spans)];
    for n in 0..below {
        let key = format!("filler-{n}");
        sections.push(prose(&key, &key, 10 + 10 * n, &varied(n, 120)));
    }
    if below > 0 {
        sections.push(sentence(
            "below",
            10 + 10 * below,
            vec![link("/docs/below.md", "below")],
        ));
    }
    page(LINKS, "Links", sections)
}

/// A document of 60 sections of some 800 characters each, `Part N: …`, each under a heading of
/// its own, well over twice the cap.
fn large() -> Value {
    let sections = (1..=60)
        .map(|n| {
            let key = format!("part-{n}");
            prose(
                &key,
                &key,
                10 * n,
                &format!("Part {n}: {}", varied(100 + n, 120)),
            )
        })
        .collect();
    let mut large = page("docs/large.md", "Large", sections);
    large["headings"] = (1..=60)
        .map(|n| json!({"level": 2, "anchor": format!("part-{n}"), "text": format!("Part {n}")}))
        .collect();
    large
}

/// The document page's contents ("On this page") hold an entry for each of its 60 headings, a
/// link or, off the screen, its placeholder.
const CONTENTS_WHOLE: &str = "document.querySelectorAll('main nav.doc-toc .toc-links a, main nav.doc-toc .toc-links .viewport-placeholder').length === 60";

/// The stand-in, serving the fixture task's page and thread for any slug, and no landing offer.
fn site() -> Site {
    let site = Site::start(crate::dist(), &common::fixture("board.json"));
    let content = look_parity::responses();
    for path in ["/api/task-page", "/api/task-history"] {
        site.set_json(path, &content[path]);
    }
    site.set_json(
        "/api/task-offer",
        &json!({"action": "approve", "label": "Land it", "branch": TASK, "payload": null}),
    );
    site.set_json("/api/asks", &json!({"asks": [], "recipient": null}));
    site.set_json("/api/postponed", &json!([]));
    site
}

/// Open the links page on a store opened beforehand: drawn from the server and live.
fn visit_links(chrome: &mut Chrome, site: &Site) {
    store_opened(chrome, site);
    chrome.open(&site.url(&format!("/{LINKS}")));
    shows(chrome, documents::LIVE, "the links page");
}

/// Each section of the document page saying `said`, as it stands, without scrolling it.
fn section_says(said: &str) -> String {
    format!(
        "[...document.querySelectorAll('main .doc-section')].some(section => section.textContent.includes({}))",
        json!(said)
    )
}

/// A request the page sent, as Chrome's network events tell of it.
#[derive(Debug)]
struct Sent {
    /// Its path and query.
    target: String,
    kind: String,
    priority: String,
    /// It carried `Purpose: prefetch`.
    prefetch: bool,
    /// When it was sent, when it was answered (its response's head, or its failure) and when it
    /// ended, finished or failed, in Chrome's seconds. A page counts a read in flight until it is
    /// answered (the shell's `settled`); its body may still be on its way.
    at: f64,
    answered: Option<f64>,
    ended: Option<f64>,
    /// Why it failed, when it did.
    failed: Option<String>,
    /// Its body as the page read it, decoded, and what it took on the wire, head included.
    decoded: f64,
    wire: f64,
}

/// The requests the page sent since it was opened, in order.
fn sent(chrome: &mut Chrome) -> Vec<Sent> {
    let finished = chrome.wait_for_protocol_events("Network.loadingFinished", 0, SHOWN);
    let failed = chrome.wait_for_protocol_events("Network.loadingFailed", 0, SHOWN);
    let responses = chrome.wait_for_protocol_events("Network.responseReceived", 0, SHOWN);
    let data = chrome.wait_for_protocol_events("Network.dataReceived", 0, SHOWN);
    chrome
        .wait_for_protocol_events("Network.requestWillBeSent", 0, SHOWN)
        .into_iter()
        .map(|params| {
            let id = params["requestId"].clone();
            let done = finished.iter().find(|event| event["requestId"] == id);
            let gone = failed.iter().find(|event| event["requestId"] == id);
            let head = responses.iter().find(|event| event["requestId"] == id);
            let url = params["request"]["url"].as_str().unwrap_or_default();
            let target = url
                .split_once("://")
                .map_or(url, |(_, rest)| rest.find('/').map_or("", |at| &rest[at..]));
            let prefetch = params["request"]["headers"]
                .as_object()
                .is_some_and(|headers| {
                    headers.iter().any(|(name, value)| {
                        name.eq_ignore_ascii_case("purpose") && value == "prefetch"
                    })
                });
            Sent {
                target: target.to_string(),
                kind: params["type"].as_str().unwrap_or_default().to_string(),
                priority: params["request"]["initialPriority"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                prefetch,
                at: params["timestamp"].as_f64().unwrap_or_default(),
                answered: head.or(gone).and_then(|event| event["timestamp"].as_f64()),
                ended: done.or(gone).and_then(|event| event["timestamp"].as_f64()),
                failed: gone.and_then(|event| event["errorText"].as_str().map(String::from)),
                decoded: data
                    .iter()
                    .filter(|event| event["requestId"] == id)
                    .filter_map(|event| event["dataLength"].as_f64())
                    .sum(),
                wire: done
                    .and_then(|event| event["encodedDataLength"].as_f64())
                    .unwrap_or_default(),
            }
        })
        .collect()
}

/// The reads ahead among `sent`: of the API, marked as a prefetch.
fn ahead(sent: &[Sent]) -> Vec<&Sent> {
    sent.iter()
        .filter(|read| read.prefetch && read.target.starts_with("/api/"))
        .collect()
}

/// Each record of this device's store `store` (`items`, `parts`) by its key's names but the
/// project's, with its `field`: read beside the store, as `PARTS` reads the parts.
fn records(chrome: &mut Chrome, store: &str, field: &str) -> Map<String, Value> {
    let read = chrome.eval(&format!(
        r#"(async () => {{
            const db = await new Promise(resolve => {{
                const request = indexedDB.open("symphony-cache");
                request.onupgradeneeded = () => request.transaction.abort();
                request.onsuccess = () => resolve(request.result);
                request.onerror = () => resolve(null);
            }});
            if (!db) return {{}};
            const all = await new Promise(resolve => {{
                const request = db.transaction({store}).objectStore({store}).getAll();
                request.onsuccess = () => resolve(request.result);
            }});
            db.close();
            const names = key => {{
                const names = [];
                for (let at = 0; at < key.length;) {{
                    const colon = key.indexOf(":", at), end = colon + 1 + Number(key.slice(at, colon));
                    names.push(key.slice(colon + 1, end));
                    at = end;
                }}
                return names.slice(1).join(" ");
            }};
            return Object.fromEntries(all.map(record => [names(record.key), record[{field}]]));
        }})()"#,
        store = json!(store),
        field = json!(field),
    ));
    match read {
        Value::Object(records) => records,
        other => panic!("the store's {store}: {other}"),
    }
}

#[test]
fn the_first_eight_links_on_the_screen_are_read_once_after_the_pages_own_reads_at_low_priority_and_one_opened_draws_from_the_device()
 {
    let site = site();
    site.set_document(&documents::document("docs/near.md", "Near", 4182));
    let mut links = vec![
        link(&format!("/task/{TASK}"), "the task"),
        link("/docs/near.md", "near"),
    ];
    for n in 1..=7 {
        let path = format!("docs/d{n}.md");
        site.set_document(&documents::document(&path, &format!("D{n}"), 4182));
        links.push(link(&format!("/{path}"), &format!("d{n}")));
    }
    site.set_document(&links_page(links, 0));
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit_links(&mut chrome, &site);
    // The eighth link's, read last.
    keeps(
        &mut chrome,
        &[
            "task:visual/in-progress page",
            "task:visual/in-progress thread",
            "document:docs/near.md outline",
            "document:docs/d6.md outline",
        ],
    );
    let mut expected = vec![
        "/api/task-page?slug=visual%2Fin-progress".to_string(),
        "/api/task-history?slug=visual%2Fin-progress".to_string(),
        "/api/document?path=docs%2Fnear.md&view=first".to_string(),
    ];
    expected.extend((1..=6).map(|n| format!("/api/document?path=docs%2Fd{n}.md&view=first")));
    let answered: Vec<(String, Option<String>, u16)> = site
        .read_ahead()
        .into_iter()
        .map(|answer| (answer.target, answer.named, answer.status))
        .collect();
    assert_eq!(
        answered,
        expected
            .iter()
            .map(|target| (target.clone(), None, 200))
            .collect::<Vec<_>>(),
        "the first eight links on the screen, the ninth not, each read once in order, naming no tag"
    );

    let sent = sent(&mut chrome);
    let ahead = ahead(&sent);
    assert_eq!(
        ahead
            .iter()
            .map(|read| read.target.as_str())
            .collect::<Vec<_>>(),
        expected,
        "{sent:#?}"
    );
    for read in &ahead {
        assert_eq!(
            read.priority, "Low",
            "read at the lowest priority: {read:#?}"
        );
    }
    let first = ahead[0].at;
    let own: Vec<&Sent> = sent
        .iter()
        .filter(|read| {
            !read.prefetch
                && read.target.starts_with("/api/")
                && read.kind != "EventSource"
                && read.at <= first
        })
        .collect();
    assert!(!own.is_empty(), "the page's own reads: {sent:#?}");
    for read in &own {
        assert!(
            read.answered.is_some_and(|answered| answered <= first),
            "the page's own read answered before the first read ahead was sent, at {first}: {read:#?}"
        );
    }
    for read in &ahead {
        assert_eq!(read.failed, None, "{read:#?}");
    }
    for pair in ahead.windows(2) {
        assert!(
            pair[0].ended.is_some_and(|ended| ended <= pair[1].at),
            "one read ahead after another: {pair:#?}"
        );
    }
    eprintln!(
        "the links page's reads ahead: {} bytes on the wire, their JSON {} bytes, after the page's own {} reads ({} bytes on the wire)\n{ahead:#?}",
        ahead.iter().map(|read| read.wire).sum::<f64>(),
        ahead.iter().map(|read| read.decoded).sum::<f64>(),
        own.len(),
        own.iter().map(|read| read.wire).sum::<f64>(),
    );
    let items = records(&mut chrome, "items", "visited");
    for item in [
        "task:visual/in-progress",
        "document:docs/near.md",
        "document:docs/d6.md",
    ] {
        assert_eq!(
            items.get(item),
            Some(&json!(false)),
            "{item} kept unvisited: {items:#?}"
        );
    }
    assert_eq!(items.get("document:docs/links.md"), Some(&json!(true)));
    assert_eq!(items.get("document:docs/d7.md"), None, "{items:#?}");

    // The task opened: drawn from the device before any answer, then checked, unchanged.
    let before = cache_pages::answered(&site, &cache_tasks::READS);
    hold(&site, &cache_tasks::READS, true);
    chrome.open("about:blank");
    chrome.open(&site.url(&format!("/task/{TASK}")));
    cache_tasks::draws(&mut chrome, TASK, "Task brief", "Owner message");
    shows(
        &mut chrome,
        TASK_FROM_THE_DEVICE,
        "the task from this device",
    );
    assert_eq!(
        cache_pages::answered(&site, &cache_tasks::READS),
        before,
        "drawn before any read was answered"
    );
    hold(&site, &cache_tasks::READS, false);
    shows(&mut chrome, LIVE, "the task, once checked");
    cache_tasks::unchanged_once(
        &since(&site, &cache_tasks::READS, &before, "/api/task-page"),
        "the task's page read ahead",
    );
    cache_tasks::unchanged_once(
        &since(&site, &cache_tasks::READS, &before, "/api/task-history"),
        "its thread read ahead",
    );

    // A document opened: drawn whole from its first part, then its outline read, and no section.
    let before = cache_pages::answered(&site, &documents::READS);
    hold(&site, &documents::READS, true);
    chrome.open("about:blank");
    chrome.open(&site.url("/docs/near.md"));
    documents::draws(
        &mut chrome,
        &["Near: the intro.", "Posted.", "Lamps, as kept."],
    );
    shows(
        &mut chrome,
        DOCUMENT_FROM_THE_DEVICE,
        "the document from this device",
    );
    assert_eq!(
        cache_pages::answered(&site, &documents::READS),
        before,
        "drawn before any read was answered"
    );
    hold(&site, &documents::READS, false);
    shows(&mut chrome, documents::LIVE, "the document, once checked");
    documents::retitled(&mut chrome, &site, "docs/near.md", 4183, "Near, retitled");
    documents::outline_once(
        &since(&site, &documents::READS, &before, "/api/document"),
        200,
    );
    assert_eq!(
        since(&site, &documents::READS, &before, "/api/section"),
        [],
        "every section came with the read ahead and is current"
    );
    keeps(&mut chrome, &["document:docs/near.md sections"]);
    let items = records(&mut chrome, "items", "visited");
    assert_eq!(items.get("document:docs/near.md"), Some(&json!(true)));
}

/// A link to a document the project lacks is read ahead, refused `404` with the server's error as
/// the server refuses it, and that read is read to its end before the next link's: Chrome ends a
/// read only once the page reads its body, and one left unread stayed in flight for as long as the
/// page was open, which held `symphony shot` until its timeout (a new project's home linking a
/// register it has no file of yet). Nothing is kept of it.
#[test]
fn a_read_ahead_the_server_refuses_ends_read_to_its_end_and_the_next_link_is_read_after_it() {
    const GONE: &str = "/api/document?path=docs%2Fgone.md&view=first";
    const NEAR: &str = "/api/document?path=docs%2Fnear.md&view=first";
    let site = site();
    site.set_document(&documents::document("docs/near.md", "Near", 4182));
    site.miss_route(GONE, Some("not found: docs/gone.md"));
    site.set_document(&links_page(
        vec![link("/docs/gone.md", "gone"), link("/docs/near.md", "near")],
        0,
    ));
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit_links(&mut chrome, &site);
    keeps(&mut chrome, &["document:docs/near.md outline"]);
    let answered: Vec<(String, u16)> = site
        .read_ahead()
        .into_iter()
        .map(|answer| (answer.target, answer.status))
        .collect();
    assert_eq!(
        answered,
        [(GONE.to_string(), 404), (NEAR.to_string(), 200)],
        "the missing document's read first, then the next link's"
    );
    let ended = test_time::until("the refused read ahead ended", crate::STALL, || {
        chrome
            .requests()
            .into_iter()
            .find(|request| request.url.ends_with(GONE))
            .filter(|request| request.finished || request.failed.is_some())
    })
    .unwrap_or_else(|stall| panic!("{stall}: {:#?}", chrome.requests()));
    assert!(
        ended.finished && ended.failed.is_none() && ended.status == 404,
        "answered 404 and read to its end, not given up: {ended:#?}"
    );
    let sent = sent(&mut chrome);
    let ahead = ahead(&sent);
    assert!(
        ahead[0].ended.is_some_and(|ended| ended <= ahead[1].at),
        "the refused read ended before the next one was sent: {ahead:#?}"
    );
    let items = records(&mut chrome, "items", "visited");
    assert_eq!(items.get("document:docs/gone.md"), None, "{items:#?}");
    assert_eq!(items.get("document:docs/near.md"), Some(&json!(false)));
}

#[test]
fn a_link_that_acts_a_search_the_page_itself_a_folder_and_a_link_off_the_screen_are_never_read_ahead()
 {
    let site = site();
    for path in [
        "docs/near.md",
        "docs/download.md",
        "docs/window.md",
        "docs/search.md",
        "docs/below.md",
    ] {
        site.set_document(&documents::document(path, "Linked", 4182));
    }
    site.set_document(&links_page(
        vec![
            acting("download", "", "/docs/download.md", "a download"),
            acting("target", "_blank", "/docs/window.md", "another window"),
            link("/docs/search.md?q=lamps", "a search"),
            link(&format!("/{LINKS}"), "this page"),
            link("/docs/", "the folder"),
            link("/docs/near.md", "near"),
        ],
        40,
    ));
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit_links(&mut chrome, &site);
    // The one link read, the last on the screen: once it is kept, the page has read all it will.
    keeps(&mut chrome, &["document:docs/near.md outline"]);
    assert_eq!(
        chrome.eval(
            "[!!document.querySelector('main a[download][href=\"/docs/download.md\"]'), !!document.querySelector('main a[target=_blank][href=\"/docs/window.md\"]'), !!document.querySelector('main a[href=\"/docs/search.md?q=lamps\"]'), (a => !a || a.getBoundingClientRect().top >= innerHeight)(document.querySelector('main a[href=\"/docs/below.md\"]'))]"
        ),
        json!([true, true, true, true]),
        "the page draws each link as written, the last off the screen"
    );
    assert_eq!(
        site.asked_ahead(),
        ["/api/document?path=docs%2Fnear.md&view=first"],
        "only the document link on the screen that neither acts nor searches"
    );
    let sent = sent(&mut chrome);
    assert_eq!(ahead(&sent).len(), 1, "{sent:#?}");
}

#[test]
fn a_large_document_is_read_ahead_in_its_first_part_within_the_cap_on_the_wire_and_its_visit_reads_only_the_rest()
 {
    let site = site();
    let large = large();
    let whole = serde_json::to_vec(&large).unwrap().len();
    assert!(
        whole > 2 * AHEAD_BYTES,
        "the fixture is large: {whole} bytes"
    );
    site.set_document(&large);
    site.set_document(&links_page(vec![link("/docs/large.md", "large")], 0));
    let part: Vec<String> = first_of(&large)["document"]["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|section| section["key"].as_str().unwrap().to_string())
        .collect();
    assert!(part.len() > 1 && part.len() < 60, "{part:?}");
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    visit_links(&mut chrome, &site);
    keeps(&mut chrome, &["document:docs/large.md outline"]);
    let sent = sent(&mut chrome);
    let read = ahead(&sent);
    let [read] = read.as_slice() else {
        panic!("one read ahead: {sent:#?}");
    };
    let kept = records(&mut chrome, "parts", "size")["document:docs/large.md outline"]
        .as_f64()
        .unwrap();
    eprintln!(
        "the large document's read ahead: {} bytes on the wire, its JSON {} bytes ({kept} kept), \
         {} of its {} sections; the whole document's JSON is {whole} bytes",
        read.wire,
        read.decoded,
        part.len(),
        60
    );
    let cap = AHEAD_BYTES as f64;
    assert!(
        read.decoded <= cap && read.wire <= cap && kept <= cap,
        "within {AHEAD_BYTES} bytes: {read:#?}, {kept} kept"
    );

    // Opened: its first sections drawn from the device before any answer; then its outline is
    // read, and only the sections its first part left out.
    let before = cache_pages::answered(&site, &documents::READS);
    hold(&site, &documents::READS, true);
    chrome.open("about:blank");
    chrome.open(&site.url("/docs/large.md"));
    shows(
        &mut chrome,
        &section_says("Part 1: "),
        "the first section from the device",
    );
    shows(
        &mut chrome,
        DOCUMENT_FROM_THE_DEVICE,
        "the document from this device",
    );
    // Its contents whole, those past the first part too: they do not appear once the outline
    // answers and move the body (the cold path reserves their box for the same reason).
    shows(
        &mut chrome,
        CONTENTS_WHOLE,
        "every heading in the contents, from the device",
    );
    assert_eq!(
        cache_pages::answered(&site, &documents::READS),
        before,
        "drawn before any read was answered"
    );
    hold(&site, &documents::READS, false);
    shows(&mut chrome, documents::LIVE, "the document, once checked");
    assert_eq!(
        chrome.eval(CONTENTS_WHOLE),
        json!(true),
        "the same contents once checked"
    );
    documents::draws(&mut chrome, &["Part 60: "]);
    documents::retitled(&mut chrome, &site, "docs/large.md", 4183, "Large, retitled");
    documents::outline_once(
        &since(&site, &documents::READS, &before, "/api/document"),
        200,
    );
    let asked: Vec<String> = since(&site, &documents::READS, &before, "/api/section")
        .iter()
        .flat_map(|answer| asked_keys(&answer.target))
        .collect();
    assert!(asked.iter().any(|key| key == "part-60"), "{asked:?}");
    assert!(
        asked.iter().all(|key| !part.contains(key)),
        "no section of the first part is read again: {asked:?} of {part:?}"
    );
}

#[test]
fn nothing_is_read_ahead_when_the_browser_saves_data_or_its_link_is_3g_or_slower() {
    let site = site();
    site.set_document(&documents::document("docs/near.md", "Near", 4182));
    site.set_document(&links_page(
        vec![
            link(&format!("/task/{TASK}"), "the task"),
            link("/docs/near.md", "near"),
        ],
        0,
    ));
    let Some(mut chrome) = crate::chrome() else {
        return;
    };
    // A fast link first: what it reads ahead, once the browser is idle.
    visit_links(&mut chrome, &site);
    keeps(
        &mut chrome,
        &[
            "task:visual/in-progress thread",
            "document:docs/near.md outline",
        ],
    );
    let read = site.asked_ahead();
    assert_eq!(
        read.len(),
        3,
        "the task's two reads and the document's: {read:?}"
    );
    // From here the page's idle callbacks are kept until the test runs them. A read ahead is
    // asked of the idle browser as the page is marked ready, so a ready page that asked for none
    // reads nothing ahead; the one a 3g link asks for is its warm-up's, which only a 2g link or a
    // browser saving data goes without.
    chrome.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": crate::HOLD_IDLE }),
    );
    for (save_data, link, warms) in [
        (true, "4g", false),
        (false, "3g", true),
        (false, "2g", false),
        (false, "slow-2g", false),
    ] {
        chrome.connection(save_data, link);
        chrome.open("about:blank");
        chrome.open(&site.url(&format!("/{LINKS}")));
        shows(&mut chrome, documents::LIVE, "the links page");
        assert_eq!(
            chrome.eval("[navigator.connection.saveData, navigator.connection.effectiveType]"),
            json!([save_data, link])
        );
        shows(&mut chrome, crate::READY, "the links page, ready");
        assert_eq!(
            chrome.eval(crate::RUN_IDLE),
            json!(u8::from(warms)),
            "saveData {save_data} on {link} asks the idle browser for no read ahead"
        );
        if warms {
            // The one callback it asked for was the warm-up's, which goes on to warm.
            shows(
                &mut chrome,
                "document.querySelector('link[rel=prefetch]') !== null",
                "the warm-up on a 3g link",
            );
        }
        assert_eq!(
            site.asked_ahead(),
            read,
            "saveData {save_data} on {link} reads nothing ahead"
        );
    }
}
