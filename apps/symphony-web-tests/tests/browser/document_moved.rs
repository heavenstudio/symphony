//! A route no file has finds the document of its name, as the server answers where it may have
//! moved (`/api/moved`, `docview::moved`): a task's link filed before its document moved folders
//! opens the document from the task's page, its heading kept, and Back leaves the route that had
//! none; a name documents share is offered to choose under Document not found; a name no document
//! has says Document not found alone.

use serde_json::{Value, json};

use super::{SHOWN, chrome, site::Site};

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
    let task = json!({"slug": SLUG, "title": "Filed before the move", "body": "",
        "status": "ready", "priority": 1, "parent": null, "created_by": "owner", "created": 1.0,
        "updated": 1.0, "links": [format!("{OLD}{HEADING}")], "depends": [], "roles": [],
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
    let Some(mut page) = chrome() else { return };
    page.open(&site.url(&format!("/task/{SLUG}")));
    // The task's page draws its link as it was filed.
    let link = format!(".task .facts a[href='/{OLD}{HEADING}']");
    assert!(
        page.wait_for(
            &format!("document.querySelector(\"{link}\")?.textContent === '{OLD}{HEADING}'"),
            SHOWN
        ),
        "the task's link as filed: {}",
        page.eval("document.body.innerText")
    );
    page.eval(&format!("document.querySelector(\"{link}\").click()"));
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
