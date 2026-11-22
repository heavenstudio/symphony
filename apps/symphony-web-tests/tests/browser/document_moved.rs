//! A route no file has finds the document of its name, as the server answers where it may have
//! moved (`/api/moved`, `docview::moved`): a task's link filed before its document moved folders
//! opens the document from the task's page, its heading kept, and Back leaves the route that had
//! none; a name documents share is offered to choose under Document not found; a name no document
//! has says Document not found alone. A heading such a link names by its text, as a task's link
//! stores it (`#The order`), is landed on by the id the renderer gives it, as is one a link in the
//! document names so once the page is drawn (the shell's landing).

use serde_json::{Value, json};

use super::{READY, SHOWN, chrome, site::Site};

/// The route a task's link was filed with, and the one its document has now.
const OLD: &str = "docs/MOVED.md";
const NEW: &str = "docs/area/MOVED.md";
const SLUG: &str = "tools/filed-before-the-move";
/// The heading the link names, as the address carries it.
const HEADING: &str = "#the-order";

/// The document at `path`, as the server reads it whole: a heading and a sentence naming it.
fn document(path: &str, title: &str) -> Value {
    let text = |words: &str| json!({"type": "text", "text": words});
    json!({
        "version": 3, "path": path, "title": title, "waiting": [], "crumbs": [], "tasks": [],
        "cards": [], "threads": {},
        "headings": [{"level": 2, "anchor": "the-order", "text": "The order"}],
        "sections": [{"start": 1, "end": 4, "key": "the-order", "hash": "the-order", "part": 0,
            "blocks": [
                {"type": "heading", "level": 2, "anchor": "the-order", "spans": [text("The order")]},
                {"type": "paragraph", "spans": [text(&format!("The document at {path}."))]},
            ]}],
    })
}

/// The heading a task's link names by its text, as `--link` stores it, and the id the renderer
/// gives it.
const BY_TEXT: &str = "#The order";
const ANCHOR: &str = "the-order";

/// The document at `path`, taller than a window: lines of words, the heading `The order`, and more
/// lines under it, so the heading can be scrolled to the top; a link at its start names the heading
/// by its text. Its outline names the heading's id, as the server's does.
fn tall(path: &str) -> Value {
    let text = |words: &str| json!({"type": "text", "text": words});
    let lines = |from: usize| {
        (from..from + 60)
            .map(|n| json!({"type": "paragraph", "spans": [text(&format!("Line {n} of {path}."))]}))
    };
    let link = json!({"type": "link", "href": format!("/{path}{BY_TEXT}"), "target": null,
        "title": "", "spans": [text("the order")]});
    let intro: Vec<Value> =
        std::iter::once(json!({"type": "paragraph", "spans": [text("Read "), link]}))
            .chain(lines(2))
            .collect();
    let order: Vec<Value> = std::iter::once(
        json!({"type": "heading", "level": 2, "anchor": ANCHOR, "spans": [text("The order")]}),
    )
    .chain(lines(100))
    .collect();
    json!({
        "version": 3, "path": path, "title": "Tall", "waiting": [], "crumbs": [], "tasks": [],
        "cards": [], "threads": {},
        "headings": [{"level": 2, "anchor": ANCHOR, "text": "The order"}],
        "anchors": {"the-order": [ANCHOR]},
        "sections": [
            {"start": 1, "end": 62, "key": "intro", "hash": "intro", "part": 0, "blocks": intro},
            {"start": 100, "end": 161, "key": ANCHOR, "hash": ANCHOR, "part": 0, "blocks": order},
        ],
    })
}

/// True once the page at `path` is scrolled down to its drawn heading `The order`, at the page's
/// scroll padding.
fn at_the_heading(path: &str) -> String {
    format!(
        "(() => {{ const heading = document.getElementById('{ANCHOR}');
            return location.pathname === '/{path}' && heading?.tagName === 'H2' && scrollY > 0
                && Math.abs(heading.getBoundingClientRect().top - parseFloat(getComputedStyle(document.documentElement).scrollPaddingTop)) < 1; }})()"
    )
}

/// Where the page is: its address, its scroll, and the top of the element with the heading's id.
const PLACE: &str = "JSON.stringify({href: location.href, y: scrollY,
    heading: document.getElementById('the-order')?.getBoundingClientRect().top ?? null})";

/// The site serving the task `SLUG`'s page, its one link `link`.
fn task_linking(site: &Site, link: &str) {
    let task = json!({"slug": SLUG, "title": "Filed before the move", "body": "",
        "status": "ready", "priority": 1, "parent": null, "created_by": "owner", "created": 1.0,
        "updated": 1.0, "links": [link], "depends": [], "roles": [],
        "instruction": false});
    site.set_json(
        "/api/task-page",
        &json!({"task": task, "blocks": [], "children": [], "children_count": 0,
            "gates": {"count": 0, "waiting": 0, "unsigned": 0}}),
    );
    site.set_json(
        "/api/task-history",
        &json!({"entries": [], "before": null, "after": 0}),
    );
}

/// Open the task `SLUG`'s page and click its link, drawn as `link` was filed.
fn follow_the_task_link(page: &mut crate::chrome::Chrome, site: &Site, link: &str) {
    page.open(&site.url(&format!("/task/{SLUG}")));
    let selector = format!(".task .facts a[href='/{link}']");
    assert!(
        page.wait_for(
            &format!("document.querySelector(\"{selector}\")?.textContent === '{link}'"),
            SHOWN
        ),
        "the task's link as filed: {}",
        page.eval("document.body.innerText")
    );
    page.eval(&format!("document.querySelector(\"{selector}\").click()"));
}

/// The site, its answer to where a document no file has may be `moved`.
fn site_moving_to(moved: Value) -> Site {
    let site = crate::site();
    site.set_json("/api/asks", &json!({"asks": [], "recipient": null}));
    site.set_json("/api/postponed", &json!([]));
    site.set_json("/api/moved", &moved);
    site
}

/// Whether the page shows the document at `path`, drawn whole.
fn drawn(path: &str) -> String {
    format!(
        "location.pathname === '/{path}' && document.querySelector('.document-page[data-ready=document]')?.textContent.includes('The document at {path}.') === true"
    )
}

/// The page's trouble block, once it shows: its title, its choices as `[text, href]`, and the
/// page's readiness and busy state.
fn missing(page: &mut crate::chrome::Chrome) -> Value {
    assert!(
        page.wait_for(
            "document.querySelector('.document-page[data-ready=missing] .trouble h1') !== null",
            SHOWN
        ),
        "no Document not found: {}",
        page.eval("document.body.innerText")
    );
    page.eval(
        "(() => {const page = document.querySelector('.document-page'); return {\
            title: page.querySelector('.trouble h1').textContent, path: location.pathname,\
            busy: page.getAttribute('aria-busy'),\
            choices: [...page.querySelectorAll('.trouble-choices a')].map(a => [a.textContent, a.getAttribute('href')])};})()",
    )
}

#[test]
fn a_task_pages_link_to_a_document_that_moved_opens_it_by_its_name_its_heading_kept() {
    let site = site_moving_to(json!([NEW]));
    site.set_document(&document(NEW, "Moved"));
    task_linking(&site, &format!("{OLD}{HEADING}"));
    let Some(mut page) = chrome() else { return };
    // The task's page draws its link as it was filed.
    follow_the_task_link(&mut page, &site, &format!("{OLD}{HEADING}"));
    let link = format!(".task .facts a[href='/{OLD}{HEADING}']");
    assert!(
        page.wait_for(&drawn(NEW), SHOWN),
        "the moved document, opened by its name: {} at {}",
        page.eval("document.body.innerText"),
        page.eval("location.href")
    );
    assert_eq!(
        page.eval("location.hash"),
        json!(HEADING),
        "the heading kept"
    );
    assert!(
        site.targets()
            .contains(&"/api/moved?path=docs%2FMOVED.md".to_string()),
        "asked where the route's document went: {:?}",
        site.targets()
    );
    // The route that had no document left the history: Back is the task's page.
    page.eval("history.back()");
    assert!(
        page.wait_for(
            &format!(
                "location.pathname === '/task/{SLUG}' && document.querySelector(\"{link}\") !== null"
            ),
            SHOWN
        ),
        "Back to the task's page: {}",
        page.eval("location.href")
    );
}

#[test]
fn a_document_route_whose_name_documents_share_offers_each_to_choose() {
    let site = site_moving_to(json!(["docs/product/RUNS.md", "docs/workflow/RUNS.md"]));
    site.set_document(&document("docs/workflow/RUNS.md", "Runs"));
    let Some(mut page) = chrome() else { return };
    page.open(&site.url(&format!("/docs/RUNS.md{HEADING}")));
    assert_eq!(
        missing(&mut page),
        json!({"title": "Document not found", "path": "/docs/RUNS.md", "busy": "false",
        "choices": [
            ["docs/product/RUNS.md", format!("/docs/product/RUNS.md{HEADING}")],
            ["docs/workflow/RUNS.md", format!("/docs/workflow/RUNS.md{HEADING}")],
        ]}),
        "neither is opened for the reader"
    );
    page.eval("document.querySelectorAll('.trouble-choices a')[1].click()");
    assert!(
        page.wait_for(&drawn("docs/workflow/RUNS.md"), SHOWN),
        "the chosen document: {}",
        page.eval("document.body.innerText")
    );
    assert_eq!(
        page.eval("location.hash"),
        json!(HEADING),
        "the heading kept"
    );
}

#[test]
fn a_document_route_whose_name_no_document_has_says_it_is_not_found() {
    let site = site_moving_to(json!([]));
    let Some(mut page) = chrome() else { return };
    page.open(&site.url("/docs/GONE.md"));
    assert_eq!(
        missing(&mut page),
        json!({"title": "Document not found", "path": "/docs/GONE.md", "busy": "false",
            "choices": []}),
    );
}

#[test]
fn a_document_opened_at_a_headings_text_lands_on_it_as_does_a_link_to_it_once_drawn() {
    let site = site_moving_to(json!([]));
    site.set_document(&tall(NEW));
    let Some(mut page) = chrome() else { return };
    page.open(&site.url(&format!("/{NEW}#The%20order")));
    assert!(
        page.wait_for(&at_the_heading(NEW), SHOWN),
        "opened at its heading's text, the page lands on the heading: {}",
        page.eval(PLACE)
    );
    // Opened at its top, then a link in it to the heading by its text: the page is drawn, so the
    // shell's landing alone brings the heading to the top.
    page.open(&site.url(&format!("/{NEW}")));
    let link = format!("a[href='/{NEW}{BY_TEXT}']");
    assert!(
        page.wait_for(
            &format!("{READY} && scrollY === 0 && {}", crate::visible(&link)),
            SHOWN
        ),
        "opened at its top, the link drawn: {}",
        page.eval(PLACE)
    );
    page.eval(&format!("document.querySelector(\"{link}\").click()"));
    assert!(
        page.wait_for(&at_the_heading(NEW), SHOWN),
        "a link to the heading's text lands on the heading: {}",
        page.eval(PLACE)
    );
    assert_eq!(
        page.eval("location.hash"),
        json!("#The%20order"),
        "the address as the link names it"
    );
}

#[test]
fn a_task_link_to_a_moved_document_lands_on_the_heading_it_names_by_its_text() {
    let site = site_moving_to(json!([NEW]));
    site.set_document(&tall(NEW));
    task_linking(&site, &format!("{OLD}{BY_TEXT}"));
    let Some(mut page) = chrome() else { return };
    follow_the_task_link(&mut page, &site, &format!("{OLD}{BY_TEXT}"));
    assert!(
        page.wait_for(&at_the_heading(NEW), SHOWN),
        "the moved document, landed on the heading the link names by its text: {}",
        page.eval(PLACE)
    );
    assert_eq!(
        page.eval("location.hash"),
        json!("#The%20order"),
        "the heading as filed"
    );
}
