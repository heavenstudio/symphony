//! Computed styles and real input against the release client. API fixtures supply content only;
//! hover, focus, layout and component markup are all the production client and stylesheets.
use super::{READY, SHOWN, assets::module, at_page_width, chrome, common, dist, site};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

mod document_task_layout;
mod list_and_queue_spacing;
mod pictures;

const DOC: &str = "docs/look-parity.md";
const PICTURE: &str = "data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20width='600'%20height='900'%3E%3Crect%20width='600'%20height='900'%20fill='teal'/%3E%3C/svg%3E";

fn paragraph(text: &str) -> Value {
    json!({"type":"paragraph","spans":[{"type":"text","text":text}]})
}

/// The folder's README is a live document, but its introduction shares the folder's heading
/// and has no reader chrome of its own. Content mirrors the isolated original-app capture.
fn folder_responses() -> BTreeMap<String, Value> {
    let mut data = responses();
    let sections = json!([
        {"start":1,"end":1,"key":"title","hash":"title","part":0,"blocks":[{"type":"heading","level":1,"anchor":"documents","spans":[{"type":"text","text":"Documents"}]}]},
        {"start":3,"end":3,"key":"intro","hash":"intro","part":0,"blocks":[paragraph("Design references for the Symphony.")]},
        {"start":5,"end":5,"key":"read","hash":"read","part":1,"blocks":[{"type":"heading","level":2,"anchor":"read-the-designs","spans":[{"type":"text","text":"Read the designs"}]}]},
        {"start":7,"end":7,"key":"choose","hash":"choose","part":1,"blocks":[paragraph("Choose a topic below to read its current design and leave a comment.")]}
    ]);
    let doc = &mut data.get_mut("/api/document").unwrap()["document"];
    doc["path"] = json!("docs/README.md");
    doc["title"] = json!("Documents");
    // A long introduction still uses the folder's navigation, even when its outline would
    // warrant contents on a standalone document.
    doc["headings"] = json!([{"level":2,"anchor":"read-the-designs","text":"Read the designs"},{"level":3,"anchor":"L7","text":"Choose a topic"},{"level":3,"anchor":"L1","text":"Review references"}]);
    doc["cards"] = json!([]);
    doc["sections"] = sections.clone();
    let intro = doc.clone();
    data.get_mut("/api/folder").unwrap()["intro"] = intro;
    data.get_mut("/api/folder").unwrap()["entries"].as_array_mut().unwrap().push(json!({"kind":"document","path":DOC,"title":"Look parity","docs":1,"waiting":1,"summary":"A quiet document block."}));
    data.get_mut("/api/section").unwrap()["change"]["sections"] = sections;
    data
}

pub(super) fn responses() -> BTreeMap<String, Value> {
    let sections = vec![
        json!({"start":1,"end":3,"key":"intro","hash":"intro","part":0,"blocks":[paragraph("A quiet document block."),{"type":"table","head":[{"header":true,"spans":[{"type":"text","text":"Lamp"}],"presentation":{}}],"rows":[[{"header":false,"spans":[{"type":"text","text":"Example"}],"presentation":{}}]]}]}),
        json!({"start":5,"end":8,"key":"callout","hash":"callout","part":0,"blocks":[{"type":"callout","kind":"question","title":[{"type":"text","text":"A question"}],"resolved":false,"blocks":[paragraph("Keep the question colour.")]}]}),
        json!({"start":10,"end":20,"key":"card:palette","hash":"palette","part":1,"blocks":[{"type":"card","slug":"palette","mark":"approved","title":"Palette","body":[paragraph("Compare the verdict controls.")],"verdict":{"group":"confirm","open":true,"posted":null,"decided":null},"thread":[],"line":10,"line_offset":null,"comparison":[]}]}),
        json!({"start":30,"end":32,"key":"gallery","hash":"gallery","part":2,"blocks":[{"type":"media","media":{"kind":"gallery","items":[{"name":"NOR-L01","src":"/model.item","poster":PICTURE}],"viewer":{"element":"item-view","script":"/static/item-view.js","attributes":[]}}}]}),
    ];
    let document = json!({"version":4182,"path":DOC,"title":"Look parity","waiting":[],"crumbs":[],"headings":[{"level":2,"anchor":"L1","text":"Introduction"},{"level":2,"anchor":"L10","text":"Decisions"},{"level":2,"anchor":"L30","text":"Lamps"}],"sections":sections,"cards":[{"slug":"palette","state":"approved","text":"Palette","group":"confirm","open":true}],"threads":{},"tasks":[]});
    let statuses = [
        "backlog",
        "concept",
        "design",
        "ready",
        "in_progress",
        "review",
        "done",
        "blocked",
        "cancelled",
    ];
    let tasks: Vec<_> = statuses.iter().map(|status| json!({
        "slug":format!("visual/{}",status.replace('_',"-")),"title":format!("Example {status}"),"body":"",
        "status":status,"priority":1,"parent":null,"created_by":"owner","created":1.0,"updated":1.0,
        "links":[],"depends":[],"roles":[{"handle":"example","role":"executor","since":1.0,"expires":null,"health":"live"}],"instruction":false
    })).collect();
    let board: Value = serde_json::from_str(&common::fixture("board.json")).unwrap();
    let agent = json!({"handle":"example","kind":"worker","model":"fixture","parent":null,"task":null,"role":null,"tasks":[],"worktree":"/fixture","branch":"visual/example","agent":null,"session_id":null,"pane":null,"callback":"codex:example","chat":false,"state":"working","state_detail":null,"health":"live","last_tool_call":null,"last_message":null});
    let mut detail = tasks[4].clone();
    detail["depends"] = json!(["visual/ready"]);
    BTreeMap::from([
        ("/api/board".into(), board.clone()),
        (
            "/api/document".into(),
            json!({"document":document,"anchors":{}}),
        ),
        (
            "/api/section".into(),
            json!({"version":4182,"change":{"path":DOC,"sections":sections}}),
        ),
        (
            "/api/task-board".into(),
            json!({"tasks":tasks,"statuses":statuses,"signoff":[],"counts":statuses.map(|s|(s,1)),"next":null}),
        ),
        (
            "/api/folder".into(),
            json!({"path":"docs","title":"Documents","crumbs":[],"docs":1,"intro":null,"entries":[{"kind":"folder","path":"docs/lamps","title":"Lamps","docs":1,"waiting":0,"summary":"Lamp references"}]}),
        ),
        ("/api/activity".into(), json!({"items":[]})),
        ("/api/pending".into(), json!({"items":[],"docs":[]})),
        ("/api/asks".into(), json!({"asks":[],"listening":false})),
        (
            "/api/queue".into(),
            json!({"waiting":[board["next"][0]],"spot_check":[],"agent":[],"branches":[]}),
        ),
        (
            "/api/agent-page".into(),
            json!({"view":{"generated":1.0,"probes":{"panes":"unavailable","slots":"ok"},"agents":[agent]},"tasks":[tasks[4]],"total":1,"next":null}),
        ),
        (
            "/api/task-page".into(),
            json!({"task":detail,"blocks":[paragraph("Task brief")],"children":[],"children_count":0}),
        ),
        (
            "/api/task-history".into(),
            json!({"entries":[{"message":{"id":1,"slug":"visual/in-progress","sender":"owner","recipient":null,"kind":"message","text":"Owner message","ts":1.0},"stamp":"2026-10-26","blocks":[]}],"before":null,"after":1}),
        ),
    ])
}

fn track_styles(browser: &mut super::chrome::Chrome) {
    browser.call("DOM.enable", json!({}));
    browser.call("CSS.enable", json!({}));
    browser.call("CSS.startRuleUsageTracking", json!({}));
}

fn painted_layout(browser: &mut super::chrome::Chrome) {
    // shell.ts awaits the current module's links before mounting it; this reads that state
    // from the page, so a sheet that never loaded fails here rather than at a style read.
    assert!(browser.wait_for(&format!("{READY} && document.readyState === 'complete' && document.querySelectorAll('link[rel=stylesheet]').length > 0 && [...document.querySelectorAll('link[rel=stylesheet]')].every(link=>link.sheet !== null)"), SHOWN),"navigation and stylesheets must finish before measuring the page");
    // The first frame resolves styles/layout; the next runs after that frame painted.
    browser.eval("document.fonts.ready.then(()=>new Promise(resolve=>requestAnimationFrame(()=>{document.documentElement.getBoundingClientRect();requestAnimationFrame(resolve)})))");
}

fn report_styles(browser: &mut super::chrome::Chrome, label: &str) {
    painted_layout(browser);
    let measured = browser.call("CSS.stopRuleUsageTracking", json!({}));
    let rules = measured["ruleUsage"].as_array().expect("CSS rule coverage");
    let used = rules
        .iter()
        .filter(|rule| rule["used"] == json!(true))
        .count();
    assert!(
        !rules.is_empty(),
        "the CSS coverage instrument returned no rules"
    );
    eprintln!(
        "CSS COVERAGE {label}: {used}/{} observed rules used",
        rules.len()
    );
}

// Reference: bbe70f89^, captured with this harness at 1280x900. The expected values below
// are measured browser output, independent of the client tokens. No baseline fault runs.
#[test]
fn the_docs_index_keeps_one_heading_and_the_original_intro_and_folder_spacing() {
    let site = ParitySite::with_data(folder_responses());
    let Some(mut browser) = chrome() else { return };
    track_styles(&mut browser);
    browser.viewport(1280, 900, false);
    browser.open(&site.url("/docs/"));
    assert!(browser.wait_for("document.querySelector('.doc-body p')?.textContent === 'Design references for the Symphony.' && document.querySelector('.doc-body h2')?.textContent === 'Read the designs' && document.querySelectorAll('.folder-entry').length === 2", SHOWN));
    assert!(browser.wait_for("(() => {const slots=[...document.querySelectorAll('.doc-body > .doc-part, .folder-entries > .viewport-slot')];return slots.length === 6 && slots.every(e=>e.style.minHeight === '0px' && !e.querySelector('.viewport-placeholder,.section-loading'));})()", SHOWN),"folder prose and entries must release their lazy height reservations before measuring");
    painted_layout(&mut browser);
    assert_eq!(
        browser.eval("document.querySelectorAll('.doc-head').length"),
        json!(1),
        "the README must not mount another page header"
    );
    assert_eq!(
        browser.eval("document.querySelector('.doc-toc, .doc-tools, .asks') === null"),
        json!(true),
        "the introduction uses the folder's navigation"
    );
    for (selector, property, expected) in [
        ("main", "padding", "24px 24px 120px"),
        (".doc-head", "margin", "8px 0px 24px"),
        (".doc-head h1", "lineHeight", "36.8px"),
        (".doc-head h1", "margin", "6.4px 0px 4.8px"),
        (".doc-part-start", "paddingTop", "8px"),
        (".doc-part-end", "paddingBottom", "16px"),
        (".doc-part-end", "marginBottom", "16px"),
        (".doc-part-start", "paddingLeft", "32px"),
        (".doc-body h2", "marginTop", "12px"),
        (".section-title", "margin", "32px 0px 12px"),
        (".folder-entries", "gap", "16px"),
        (".folder-entries", "marginBottom", "32px"),
        (".folder-entry", "padding", "16px 18px"),
        (".folder-entry", "borderRadius", "14px"),
    ] {
        assert_eq!(
            browser.eval(&format!(
                "getComputedStyle(document.querySelector({}))[{}]",
                json!(selector),
                json!(property)
            )),
            json!(expected),
            "{selector} {property}"
        );
    }
    assert_eq!(
        browser.eval("document.querySelector('.tree-folder .tree-pills .pill')?.textContent"),
        json!("1 doc")
    );
    // The page counts its documents once, in its pill: the entries' heading adds no number.
    assert_eq!(
        browser.eval("[document.querySelector('.doc-meta .pill')?.textContent, document.querySelector('.section-title')?.textContent]"),
        json!(["1 doc", "In this folder"])
    );
    assert_eq!(browser.eval("const heading=document.querySelector('.doc-body h2'), paragraph=heading.closest('.doc-part').nextElementSibling.querySelector('p'); paragraph.getBoundingClientRect().top-heading.getBoundingClientRect().bottom"),json!(16),"separate lazy slots must retain the original collapsed 16px heading-to-prose gap");
    assert_eq!(browser.eval("const cards=[...document.querySelectorAll('.folder-entry')].map(e=>e.getBoundingClientRect()); cards.every(r=>r.height === cards[0].height)"),json!(true),"folder cards share the row height");
    browser.viewport(390, 844, true);
    assert!(browser.wait_for(
        "document.querySelector('.folder-entry').getBoundingClientRect().width < 400",
        SHOWN
    ));
    assert_eq!(
        browser.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.doc-part')).paddingLeft"),
        json!("12px")
    );
    // Real docs/README.md puts Folders and Naming headings before lists in separate
    // lazy slots. Check that pattern, its ordered variant and the prose after Naming.
    for ordered in [false, true] {
        let mut data = folder_responses();
        let sections = json!([
            {"start":1,"end":1,"key":"title","hash":"title","part":0,"blocks":[{"type":"heading","level":1,"anchor":"documents","spans":[{"type":"text","text":"Documents"}]}]},
            {"start":3,"end":3,"key":"intro","hash":"intro","part":0,"blocks":[paragraph("Design references for the Symphony.")]},
            {"start":8,"end":8,"key":"folders","hash":"folders","part":1,"blocks":[{"type":"heading","level":2,"anchor":"folders","spans":[{"type":"text","text":"Folders"}]}]},
            {"start":10,"end":11,"key":"folder-list","hash":"folder-list","part":1,"blocks":[{"type":"list","ordered":ordered,"start":1,"items":[[paragraph("game/ — what the game is and where it goes.")],[paragraph("combat/ — weapons, defences, gear and lamp capabilities.")]]}]},
            {"start":13,"end":13,"key":"naming","hash":"naming","part":2,"blocks":[{"type":"heading","level":2,"anchor":"naming","spans":[{"type":"text","text":"Naming"}]}]},
            {"start":15,"end":16,"key":"naming-list","hash":"naming-list","part":2,"blocks":[{"type":"list","ordered":ordered,"start":1,"items":[[paragraph("UPPERCASE.md is one current topic, kept true to today.")],[paragraph("lowercase.md is one page per entity inside a topic folder.")]]}]},
            {"start":18,"end":18,"key":"links","hash":"links","part":2,"blocks":[paragraph("A document link finds its document by file name wherever it lives.")]}
        ]);
        data.get_mut("/api/document").unwrap()["document"]["sections"] = sections.clone();
        data.get_mut("/api/folder").unwrap()["intro"]["sections"] = sections.clone();
        data.get_mut("/api/section").unwrap()["change"]["sections"] = sections;
        let lists = ParitySite::with_data(data);
        browser.open(&lists.url("/docs/"));
        assert!(browser.wait_for("document.querySelectorAll('.doc-body li p').length === 4 && document.querySelector('.doc-body > .doc-part:last-child p')?.textContent.startsWith('A document link') && [...document.querySelectorAll('.doc-body > .doc-part')].every(e=>e.style.minHeight === '0px' && !e.querySelector('.viewport-placeholder,.section-loading'))", SHOWN));
        painted_layout(&mut browser);
        for phone in [false, true] {
            browser.viewport(if phone { 390 } else { 1280 }, 900, phone);
            browser.frames();
            assert_eq!(browser.eval("(() => {const headings=[...document.querySelectorAll('.doc-body h2')]; return headings.map(h=>{const list=h.closest('.doc-part').nextElementSibling.querySelector('ul,ol'), bottom=h.getBoundingClientRect().bottom; return {heading:h.textContent,tag:list.tagName,gap:list.getBoundingClientRect().top-bottom,itemGap:list.firstElementChild.getBoundingClientRect().top-bottom};});})()"),json!([{"heading":"Folders","tag":if ordered { "OL" } else { "UL" },"gap":16,"itemGap":16},{"heading":"Naming","tag":if ordered { "OL" } else { "UL" },"gap":16,"itemGap":16}]),"README heading-to-list spacing retains one collapsed margin (ordered={ordered}, phone={phone})");
            assert_eq!(browser.eval("(() => {const tail=document.querySelector('.doc-body > .doc-part:last-child p'), list=tail.closest('.doc-part').previousElementSibling.querySelector('ul,ol'); return tail.getBoundingClientRect().top-list.getBoundingClientRect().bottom;})()"),json!(16),"the list's following prose retains one collapsed margin (ordered={ordered}, phone={phone})");
        }
    }
    // A folder with no README or entries still has one compact page heading.
    let mut empty = responses();
    empty.get_mut("/api/folder").unwrap()["entries"] = json!([]);
    empty.get_mut("/api/folder").unwrap()["docs"] = json!(2);
    let empty_site = ParitySite::with_data(empty);
    browser.open(&empty_site.url("/docs/"));
    assert!(browser.wait_for(
        "document.querySelector('[data-ready=folder]') !== null",
        SHOWN
    ));
    assert_eq!(browser.eval("document.querySelectorAll('.doc-head').length === 1 && document.querySelector('.section-title, .doc-layout') === null"),json!(true));
    assert_eq!(
        browser.eval("document.querySelector('.doc-meta .pill').textContent"),
        json!("2 docs")
    );
    report_styles(
        &mut browser,
        "folder introduction, README lists, empty folder and phone spacing",
    );
}

#[test]
fn document_titles_share_the_contents_column_and_folding_returns_the_full_width() {
    let site = ParitySite::start();
    let Some(mut browser) = chrome() else { return };
    track_styles(&mut browser);
    browser.viewport(1280, 900, false);
    browser.open(&site.url(&format!("/{DOC}")));
    assert!(browser.wait_for(
        "document.querySelector('.doc-body table tbody td')?.textContent === 'Example'",
        SHOWN
    ));
    painted_layout(&mut browser);
    assert_eq!(browser.eval("(() => { const head=document.querySelector('.doc-head').getBoundingClientRect(), body=document.querySelector('.doc-body').getBoundingClientRect(), toc=document.querySelector('.doc-toc').getBoundingClientRect(); return {head:head.x,body:body.x,toc:toc.x,gap:head.x-toc.right}; })()"),json!({"head":276,"body":276,"toc":24,"gap":32}));
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.doc-head')).marginBottom"),
        json!("24px")
    );
    browser.eval("document.querySelector('.doc-toc summary').click()");
    assert!(browser.wait_for("!document.querySelector('.doc-toc details').open", SHOWN));
    // A vertical scrollbar consumes viewport width, scrolling or not; folding must release the
    // entire contents column within the width it leaves the page (`PAGE_WIDTH`), keeping exactly
    // the two 24 px page gutters.
    assert_eq!(browser.eval(&at_page_width("width => { const head=document.querySelector('.doc-head').getBoundingClientRect(), body=document.querySelector('.doc-body').getBoundingClientRect(); return {head:head.x,body:body.x,gutters:width-body.width}; }")),json!({"head":24,"body":24,"gutters":48}));
    let mut plain = responses();
    plain.get_mut("/api/document").unwrap()["document"]["cards"] = json!([]);
    let paragraphs = json!([
        {"start":1,"end":1,"key":"first","hash":"first","part":0,"blocks":[paragraph("First paragraph.")]},
        {"start":3,"end":3,"key":"second","hash":"second","part":0,"blocks":[paragraph("Second paragraph.")]}
    ]);
    plain.get_mut("/api/document").unwrap()["document"]["sections"] = paragraphs.clone();
    plain.get_mut("/api/section").unwrap()["change"]["sections"] = paragraphs;
    for count in 0..3 {
        let mut data = plain.clone();
        data.get_mut("/api/document").unwrap()["document"]["headings"]
            .as_array_mut()
            .unwrap()
            .truncate(count);
        let short = ParitySite::with_data(data);
        browser.open(&short.url(&format!("/{DOC}")));
        assert!(browser.wait_for(
            "[...document.querySelectorAll('.doc-body p')].map(e=>e.textContent).join('|') === 'First paragraph.|Second paragraph.'",
            SHOWN
        ));
        painted_layout(&mut browser);
        assert_eq!(browser.eval("document.querySelector('.doc-toc') === null && document.querySelector('.doc-body').getBoundingClientRect().x === 24"),json!(true),"{count} headings must not reserve a contents column (original TOC_MIN=3)");
        assert!(browser.wait_for("[...document.querySelectorAll('.doc-body > .doc-part')].every(e=>e.style.minHeight === '0px' && !e.querySelector('.viewport-placeholder, .section-loading'))",SHOWN),"paragraphs must release their lazy height reservation before measuring their gap");
        browser.frames();
        assert_eq!(browser.eval("const p=[...document.querySelectorAll('.doc-body p')]; p[1].getBoundingClientRect().top-p[0].getBoundingClientRect().bottom"),json!(16),"adjacent lazy prose slots keep one paragraph margin");
    }
    report_styles(
        &mut browser,
        "document title, open and folded contents, zero to two headings",
    );
}

#[test]
fn the_comment_dropdown_keeps_the_approved_component_geometry_and_action_row() {
    let mut data = responses();
    // A prose-only section, as in the reference; a table deliberately takes the full width.
    let prose = json!([paragraph("A quiet document block.")]);
    data.get_mut("/api/document").unwrap()["document"]["sections"][0]["blocks"] = prose.clone();
    data.get_mut("/api/section").unwrap()["change"]["sections"][0]["blocks"] = prose;
    let site = ParitySite::with_data(data);
    let Some(mut browser) = chrome() else { return };
    track_styles(&mut browser);
    browser.viewport(1280, 900, false);
    browser.open(&site.url(&format!("/{DOC}")));
    // As a reader would, once the control shows: under the document's ghost (`Settle`) the editor
    // it opens takes no focus, and nothing focuses it again once the ghost lifts.
    assert!(browser.wait_for(&crate::visible(".doc-section > .add"), SHOWN));
    browser.eval("document.querySelector('.doc-section > .add').click()");
    assert!(browser.wait_for(
        "document.querySelector('form.comment:not([hidden]) .dx-select-trigger') !== null",
        SHOWN
    ));
    painted_layout(&mut browser);
    assert_eq!(browser.eval("const form=document.querySelector('form.comment:not([hidden])'), select=form.querySelector('.dx-select-trigger'), text=form.querySelector('textarea'); ({inRow:select.closest('.dx-select').parentElement.classList.contains('row'),padding:getComputedStyle(select).padding,radius:getComputedStyle(select).borderRadius,gap:getComputedStyle(form).gap,width:select.getBoundingClientRect().width,height:select.getBoundingClientRect().height,noteHeight:text.getBoundingClientRect().height,below:select.getBoundingClientRect().top >= text.getBoundingClientRect().bottom+8})"),json!({"inRow":true,"padding":"8px 12px","radius":"8px","gap":"8px","width":105.578125,"height":39.09375,"noteHeight":76,"below":true}));
    assert_eq!(
        browser.eval(
            "document.querySelector('form.comment:not([hidden])').getBoundingClientRect().width"
        ),
        json!(640.6875),
        "the original prose measure contains the whole editor, not only its text"
    );
    assert!(
        browser.wait_for(
            "document.activeElement === document.querySelector('form.comment textarea')",
            SHOWN
        ),
        "wait for the editor's initial focus before opening the dropdown"
    );
    browser.eval("document.querySelector('form.comment .dx-select-trigger').focus()");
    assert_eq!(
        browser.eval(
            "document.activeElement === document.querySelector('form.comment .dx-select-trigger')"
        ),
        json!(true)
    );
    assert!(browser.wait_for(
        "document.querySelector('.callout')?.closest('.doc-part').style.minHeight === '0px'",
        SHOWN
    ));
    browser.frames();
    assert_eq!(browser.eval("document.querySelector('.callout').getBoundingClientRect().top-document.querySelector('form.comment:not([hidden])').getBoundingClientRect().bottom"),json!(12),"an open comment keeps its following block's margin");
    browser.eval("document.querySelector('form.comment .dx-select-trigger').click()");
    assert!(browser.wait_for(
        "document.querySelectorAll('[role=listbox] [role=option]').length===3",
        SHOWN
    ));
    assert_eq!(
        browser.eval("[...document.querySelectorAll('[role=listbox] [role=option]')].map(e=>e.textContent.trim())"),
        json!(["comment", "question", "decision"])
    );
    report_styles(&mut browser, "comment dropdown field geometry and focus");
}

#[test]
fn approve_reject_and_question_are_equal_sized_in_the_approved_component_style() {
    let site = ParitySite::start();
    let Some(mut browser) = chrome() else { return };
    track_styles(&mut browser);
    browser.viewport(1280, 900, false);
    browser.open(&site.url(&format!("/{DOC}#palette")));
    assert!(browser.wait_for(
        "document.querySelector('.doc-card .doc-actions button') !== null",
        SHOWN
    ));
    browser.eval("document.querySelector('.doc-card .doc-actions button').click()");
    assert!(browser.wait_for(
        "document.querySelector('form.verdict:not([hidden]) .v-question') !== null",
        SHOWN
    ));
    painted_layout(&mut browser);
    assert_eq!(
        browser.eval(
            "document.querySelector('form.verdict:not([hidden])').getBoundingClientRect().width"
        ),
        json!(706.46875),
        "decision editors retain the original wider prose measure"
    );
    for phone in [false, true] {
        if phone {
            browser.viewport(390, 844, true);
            assert!(browser.wait_for(
                "document.querySelector('form.verdict').getBoundingClientRect().width < 400",
                SHOWN
            ));
        }
        let sizes = browser.eval("[...document.querySelectorAll('form.verdict .v-approved, form.verdict .v-rejected, form.verdict .v-question')].map(e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return {width:r.width,height:r.height,font:s.fontSize,padding:s.padding,radius:s.borderRadius}})");
        let choices = sizes.as_array().unwrap();
        assert_eq!(choices.len(), 3);
        for choice in choices {
            // Each is 36px tall; widths share a grid track rather
            // than each label's intrinsic width. A phone may wrap tracks, never labels.
            assert_eq!(choice["height"], json!(36));
            assert!(
                (choice["width"].as_f64().unwrap() - choices[0]["width"].as_f64().unwrap()).abs()
                    <= 1.0 / 64.0,
                "equal tracks may differ by one Chromium layout unit: {sizes}"
            );
            assert!(choice["width"].as_f64().unwrap() >= 99.125);
            assert_eq!(choice["font"], json!("14px"));
            assert_eq!(choice["padding"], json!("8px 16px"));
            assert_eq!(choice["radius"], json!("8px"));
        }
        assert_eq!(
            browser.eval("document.documentElement.scrollWidth <= innerWidth"),
            json!(true)
        );
    }
    report_styles(&mut browser, "equal decision choices on desktop and phone");
}

#[test]
fn document_surfaces_and_component_fields_keep_the_approved_radius_scale() {
    let mut data = responses();
    let original = data["/api/document"]["document"]["sections"]
        .as_array()
        .unwrap()
        .clone();
    let table = original[0]["blocks"][1].clone();
    let mut matrix = table.clone();
    matrix["presentation"] = json!({"class":"matrix relationships"});
    let mut intro = original[0].clone();
    intro["blocks"] = json!([{"type":"heading","level":2,"anchor":"introduction","spans":[{"type":"text","text":"Introduction"}]}]);
    let tables =
        json!({"start":3,"end":4,"key":"tables","hash":"tables","part":0,"blocks":[table,matrix]});
    let heading = json!({"start":9,"end":9,"key":"decisions","hash":"decisions","part":1,"blocks":[{"type":"heading","level":2,"anchor":"decisions","spans":[{"type":"text","text":"Decisions"}]}]});
    let mut second = original[2].clone();
    second["start"] = json!(22);
    second["end"] = json!(26);
    second["key"] = json!("card:palette-next");
    second["blocks"][0]["slug"] = json!("palette-next");
    second["blocks"][0]["line"] = json!(22);
    second["blocks"][0]["title"] = json!("Another palette");
    let sections = json!([
        intro,
        tables,
        original[1],
        heading,
        original[2],
        second,
        original[3]
    ]);
    data.get_mut("/api/document").unwrap()["document"]["sections"] = sections.clone();
    data.get_mut("/api/document").unwrap()["document"]["cards"].as_array_mut().unwrap().push(json!({"slug":"palette-next","state":"approved","text":"Another palette","group":"confirm","open":true}));
    data.get_mut("/api/section").unwrap()["change"]["sections"] = sections;
    let site = ParitySite::with_data(data);
    let Some(mut browser) = chrome() else { return };
    track_styles(&mut browser);
    browser.open(&site.url(&format!("/{DOC}")));
    assert!(browser.wait_for("document.querySelectorAll('.doc-body table tbody td').length === 2 && [...document.querySelectorAll('.doc-body table tbody td')].every(e=>e.textContent === 'Example') && document.querySelector('.callout') !== null && document.querySelectorAll('.doc-card').length === 2 && document.querySelectorAll('.doc-body h2').length === 2",SHOWN));
    assert!(browser.wait_for("[...document.querySelectorAll('.doc-body > .doc-part')].filter(e=>e.querySelector('table,.callout,h2,.doc-card')).every(e=>e.style.minHeight === '0px' && !e.querySelector('.section-loading'))",SHOWN));
    painted_layout(&mut browser);
    assert_eq!(browser.eval("(() => {const h=document.querySelectorAll('.doc-body h2'),t=document.querySelectorAll('.doc-body table'),c=document.querySelectorAll('.doc-card'),q=document.querySelector('.callout');return {headingTable:t[0].getBoundingClientRect().top-h[0].getBoundingClientRect().bottom,tableCallout:q.getBoundingClientRect().top-t[1].getBoundingClientRect().bottom,headingCard:c[0].getBoundingClientRect().top-h[1].getBoundingClientRect().bottom,cards:c[1].getBoundingClientRect().top-c[0].getBoundingClientRect().bottom};})()"),json!({"headingTable":13.1875,"tableCallout":12,"headingCard":13.1875,"cards":12}),"lazy wrappers retain the original collapsed margins without opening gaps in the card border");
    assert_eq!(browser.eval("[...document.querySelectorAll('.doc-body table')].map(e=>{const s=getComputedStyle(e);return {radius:s.borderRadius,collapse:s.borderCollapse,border:s.borderTopWidth}})"),json!([{"radius":"10px","collapse":"separate","border":"1px"},{"radius":"10px","collapse":"separate","border":"1px"}]),"collapsed relationships borders paint square corners even with a declared radius");
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.callout')).borderRadius"),
        json!("10px")
    );
    assert_eq!(
        browser.eval(
            "getComputedStyle(document.querySelector('.doc-part-start')).borderTopLeftRadius"
        ),
        json!("14px")
    );
    browser.eval("document.querySelector('.doc-section > .add').click()");
    assert!(browser.wait_for(
        "document.querySelector('form.comment:not([hidden])') !== null",
        SHOWN
    ));
    for (selector, radius) in [
        ("form.comment", "10px"),
        ("form.comment .dx-select-trigger", "8px"),
        ("form.comment textarea", "8px"),
        ("form.comment button", "8px"),
    ] {
        assert_eq!(
            browser.eval(&format!(
                "getComputedStyle(document.querySelector({})).borderRadius",
                json!(selector)
            )),
            json!(radius),
            "{selector}"
        );
    }
    report_styles(&mut browser, "document border scale and both table kinds");
}

/// A root-mounted release shell and deterministic API data, on an owned ephemeral listener.
/// This deliberately does no byte accounting; the shared browser suite owns the unchanged budgets.
struct ParitySite {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ParitySite {
    fn start() -> Self {
        Self::with_data(responses())
    }

    fn with_data(mut data: BTreeMap<String, Value>) -> Self {
        let root = dist();
        let shell = std::fs::read_to_string(root.join("index.html")).unwrap();
        // The root shell names its current module in the board snapshot, just as the real server.
        let script = shell
            .split("<script")
            .find(|s| s.contains("type=\"module\""))
            .unwrap();
        let build = script
            .split("src=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        data.get_mut("/api/board").unwrap()["build"] = json!(build);
        let data = Arc::new(data);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopping.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut out) = stream else { continue };
                let data = data.clone();
                let stop = stopping.clone();
                std::thread::spawn(move || {
                    let mut request = String::new();
                    let mut reader = BufReader::new(out.try_clone().unwrap());
                    reader.read_line(&mut request).unwrap();
                    let path = request
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .split('?')
                        .next()
                        .unwrap();
                    let path = path
                        .strip_prefix("/@")
                        .and_then(|rest| rest.split_once('/'))
                        .map_or_else(|| path.to_string(), |(_, route)| format!("/{route}"));
                    if path == "/api/feed" {
                        // A healthy stream preserves the document and focused editor during input,
                        // on the WebSocket every page follows the feed on.
                        let mut key = None;
                        loop {
                            let mut header = String::new();
                            if reader.read_line(&mut header).unwrap_or(0) == 0 {
                                return;
                            }
                            let header = header.trim_end();
                            if header.is_empty() {
                                break;
                            }
                            if let Some((name, value)) = header.split_once(':')
                                && name.trim().eq_ignore_ascii_case("sec-websocket-key")
                            {
                                key = Some(value.trim().to_string());
                            }
                        }
                        let Some(key) = key else { return };
                        let ping = site::Said::ping().frame();
                        if out.write_all(site::socket_head(&key).as_bytes()).is_err() {
                            return;
                        }
                        let mut pinged = Instant::now();
                        while !stop.load(Ordering::SeqCst) {
                            if pinged.elapsed() >= Duration::from_secs(1) {
                                if out.write_all(&ping).is_err() {
                                    return;
                                }
                                pinged = Instant::now();
                            }
                            #[expect(
                                clippy::disallowed_methods,
                                reason = "no-timed-waits baseline: a test paces the pings it sends by a length of time"
                            )]
                            std::thread::sleep(Duration::from_millis(20));
                        }
                        return;
                    }
                    let (bytes, mime) = if let Some(value) = data.get(&path) {
                        (value.to_string().into_bytes(), "application/json")
                    } else if path.starts_with("/api/") {
                        (b"{}".to_vec(), "application/json")
                    } else {
                        let candidate = root.join(path.trim_start_matches('/'));
                        let file = if candidate.is_file() {
                            candidate
                        } else {
                            root.join("index.html")
                        };
                        let mime = match file.extension().and_then(|s| s.to_str()).unwrap_or("") {
                            "wasm" => "application/wasm",
                            "js" => "text/javascript",
                            "css" => "text/css",
                            _ => "text/html",
                        };
                        let bytes = std::fs::read(&file).unwrap();
                        if file == root.join("index.html") {
                            let shell = String::from_utf8(bytes).unwrap();
                            (crate::common::served_shell(&shell).into_bytes(), mime)
                        } else {
                            (bytes, mime)
                        }
                    };
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        bytes.len()
                    );
                    let _ = out
                        .write_all(head.as_bytes())
                        .and_then(|()| out.write_all(&bytes));
                });
            }
        });
        Self {
            addr,
            stop,
            thread: Some(thread),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }
}

#[test]
fn a_wide_table_folds_the_contents_after_its_body_arrives() {
    let mut data = responses();
    let mut table = data["/api/document"]["document"]["sections"][0]["blocks"][1].clone();
    table["rows"][0][0]["spans"][0]["text"] = json!("W".repeat(1600));
    // Enough initial placeholders keep the table outside the look-ahead before the first
    // bodies arrive; the tall prose then keeps it below the first ready viewport.
    let mut sections = vec![
        json!({"start":1,"end":3,"key":"intro","hash":"intro","part":0,"blocks":[paragraph("Read before opening the table."),{"type":"code","language":"text","text":"A line of prose\n".repeat(100)}]}),
    ];
    for index in 1..20 {
        sections.push(json!({"start":index*10,"end":index*10+1,"key":format!("spacer-{index}"),"hash":"spacer","part":0,"blocks":[paragraph("Another document block.")]}));
    }
    sections.push(
        json!({"start":1000,"end":1001,"key":"wide-table","hash":"wide","part":1,"blocks":[table]}),
    );
    data.get_mut("/api/document").unwrap()["document"]["sections"] = json!(sections);
    data.get_mut("/api/document").unwrap()["document"]["cards"] = json!([]);
    data.get_mut("/api/section").unwrap()["change"]["sections"] = json!(sections);
    let site = ParitySite::with_data(data);
    let Some(mut browser) = chrome() else { return };
    track_styles(&mut browser);
    browser.viewport(1280, 900, false);
    browser.open(&site.url(&format!("/{DOC}")));
    assert!(browser.wait_for("document.querySelector('[data-ready=document]') !== null && document.querySelector('.doc-body pre') !== null",SHOWN));
    painted_layout(&mut browser);
    assert_eq!(browser.eval("document.querySelector('.doc-toc details').open && document.querySelector('.doc-body table') === null"),json!(true));
    browser.eval("window.scrollTo(0,document.body.scrollHeight)");
    assert!(browser.wait_for("document.querySelector('.doc-body table') !== null", SHOWN));
    browser.eval("document.querySelector('.doc-body table').scrollIntoView({block:'center'})");
    // TableRow first reserves space with an empty cell. Its box is not the loaded row.
    assert!(
        browser.wait_for(
            "document.querySelector('.doc-body table tbody td')?.textContent === 'W'.repeat(1600) && document.querySelector('.doc-body table').clientWidth > 0",
            SHOWN
        ),
        "the late table's real cell must be visible before measuring overflow"
    );
    assert_eq!(browser.eval("const table=document.querySelector('.doc-body table'); table.scrollWidth > table.clientWidth"),json!(true));
    assert!(browser.wait_for("!document.querySelector('.doc-toc details').open", SHOWN));
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.doc-layout')).display"),
        json!("block")
    );
    browser.eval("document.querySelector('.doc-toc summary').click()");
    assert_eq!(
        browser.eval("document.querySelector('.doc-toc details').open"),
        json!(true)
    );
    browser.viewport(1200, 900, false);
    browser.frames();
    assert_eq!(
        browser.eval("document.querySelector('.doc-toc details').open"),
        json!(true),
        "a reader's choice survives table resize"
    );
    report_styles(&mut browser, "late table and manual contents choice");
}

impl Drop for ParitySite {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

#[test]
fn history_pictures_keep_their_original_caps_outside_the_inner_decision() {
    let Some(mut browser) = chrome() else { return };
    browser.scheme("light");
    for comparison in [false, true] {
        let mut data = responses();
        let image = json!({"type":"image","image":{"src":PICTURE,"alt":"History picture","presentation":{"attributes":{"width":"600","height":"900"}}}});
        let thread = json!({"palette":{"blocks":[{"type":"thread","entries":[{"who":"agent","timestamp":"2026-10-26","verdict":"auto-approved","blocks":[paragraph("An earlier version."),image]}]}]}});
        if comparison {
            let compare = json!([{"type":"comparison","figures":[{"type":"container","tag":"figure","presentation":{},"blocks":[paragraph("Current version")]}]}]);
            data.get_mut("/api/document").unwrap()["document"]["sections"][2]["blocks"][0]["comparison"] =
                compare.clone();
            data.get_mut("/api/section").unwrap()["change"]["sections"][2]["blocks"][0]["comparison"] =
                compare;
        }
        data.get_mut("/api/document").unwrap()["document"]["threads"] = thread.clone();
        data.get_mut("/api/section").unwrap()["change"]["threads"] = thread;
        let site = ParitySite::with_data(data);
        browser.open(&site.url(&format!("/{DOC}#palette")));
        assert!(browser.wait_for("document.getElementById('palette') !== null", SHOWN));
        painted_layout(&mut browser);
        // Reveal the lazy card explicitly: the shell's earlier hash scroll can precede
        // the hydrated card's replacement.
        browser.eval("document.getElementById('palette').scrollIntoView({block:'center'})");
        assert!(browser.wait_for(
            "document.querySelector('.doc-card .history img') !== null",
            SHOWN
        ));
        assert!(
            browser.wait_for(
                // DocumentPicture holds the waiting picture until its visibility latch fires, and
                // that is already complete; wait for revelation as well as load completion. The
                // picture is brought to the middle of the screen at each look while it still
                // waits: a scroll made in the task that revealed its history entry is laid out
                // with the history's slot at its remembered height (it took `content-visibility:
                // auto` in that task), and Chrome's scroll anchoring then moves the view by the
                // slot's growth once the next frame lays it out whole, leaving the picture above
                // the screen, where its latch never fires.
                &format!(
                    "(() => {{ const image=document.querySelector('.doc-card .history img'); if (image.getAttribute('src') === {}) {{ image.scrollIntoView({{block:'center'}}); return false; }} return image.complete; }})()",
                    json!(symphony_web_tests::WAITING)
                ),
                SHOWN
            ),
            "the history picture shows once it is on the screen"
        );
        assert_eq!(
            browser.eval("document.querySelector('.doc-card .history img').naturalWidth"),
            json!(600)
        );
        assert_eq!(
            browser
                .eval("document.querySelector('.doc-card .history').closest('.decision') === null"),
            json!(true)
        );
        assert_eq!(
            browser.eval("document.querySelector('.history .entry-verdict').textContent"),
            json!("auto-approved")
        );
        assert_eq!(
            browser.eval(
                "getComputedStyle(document.querySelector('.history .entry-verdict')).borderRadius"
            ),
            json!("999px")
        );
        assert_eq!(
            browser.eval(
                "getComputedStyle(document.querySelector('.history .entry-verdict')).borderTopWidth"
            ),
            json!("1px")
        );
        assert_eq!(
            browser.eval(
                "getComputedStyle(document.querySelector('.history .entry-verdict')).textTransform"
            ),
            json!("uppercase")
        );
        assert_eq!(browser.eval("getComputedStyle(document.querySelector('.history .entry-verdict')).backgroundColor"), json!("rgba(127, 127, 127, 0.07)"));
        let size = browser.eval("const image=document.querySelector('.doc-card .history img'); const r=image.getBoundingClientRect(); ({width:r.width,height:r.height})");
        assert_eq!(size["width"], json!(if comparison { 48 } else { 460 }));
        assert_eq!(size["height"], json!(if comparison { 72 } else { 690 }));
        assert_eq!(browser.eval("getComputedStyle(document.querySelector('.doc-card .history .entry-text > p')).display"), json!("inline"));
    }
}

#[test]
fn document_controls_reveal_on_hover_and_keyboard_focus_with_the_original_verdict_palette() {
    let site = ParitySite::start();
    let Some(mut browser) = chrome() else { return };
    track_styles(&mut browser);
    browser.open(&site.url(&format!("/{DOC}")));
    assert!(browser.wait_for(
        "document.querySelector('.doc-section > .add') !== null",
        SHOWN
    ));
    painted_layout(&mut browser);
    assert_eq!(
        browser.eval(
            "getComputedStyle(document.querySelector('.doc-section > .add')).transitionDuration"
        ),
        json!("0s"),
        "hover and focus reads require an immediate reveal"
    );
    assert_eq!(
        browser.eval("document.querySelectorAll('.doc-part-start').length"),
        json!(3)
    );
    assert_eq!(
        browser.eval(
            "getComputedStyle(document.querySelector('.doc-part-start + .doc-part')).borderTopWidth"
        ),
        json!("0px")
    );
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.doc-section > .add')).opacity"),
        json!("0")
    );
    let point = browser.eval("const r=document.querySelector('.doc-section').getBoundingClientRect(); ({x:r.x+20,y:r.y+20})");
    browser.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mouseMoved","x":point["x"],"y":point["y"]}),
    );
    // The control has no transition; its opacity is final once Chrome applies :hover.
    assert!(
        browser.wait_for(
            "document.querySelector('.doc-section').matches(':hover')",
            SHOWN
        ),
        "the section must receive hover before its control is measured"
    );
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.doc-section > .add')).opacity"),
        json!("1")
    );
    browser.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mouseMoved","x":1,"y":1}),
    );
    // Focus is measured with hover cleared so each reveal path is independent.
    assert!(
        browser.wait_for(
            "!document.querySelector('.doc-section').matches(':hover')",
            SHOWN
        ),
        "the section must lose hover before keyboard focus is tested"
    );
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.doc-section > .add')).opacity"),
        json!("0")
    );
    browser.eval("document.querySelector('.doc-section > .add').focus()");
    assert_eq!(
        browser.eval("document.activeElement.matches('.add')"),
        json!(true)
    );
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.doc-section > .add')).opacity"),
        json!("1")
    );
    browser.call(
        "Input.dispatchKeyEvent",
        json!({"type":"keyDown","key":"Enter","code":"Enter","windowsVirtualKeyCode":13,"text":"\r","unmodifiedText":"\r"}),
    );
    browser.call(
        "Input.dispatchKeyEvent",
        json!({"type":"keyUp","key":"Enter","code":"Enter","windowsVirtualKeyCode":13}),
    );
    assert!(browser.wait_for("!document.querySelector('form.comment').hidden", SHOWN));
    assert!(browser.wait_for(
        "document.querySelector('.doc-card .doc-actions button') !== null",
        SHOWN
    ));
    browser.eval("document.querySelector('.doc-card .doc-actions button').click()");
    for (mode, approved, rejected, question) in [
        (
            "light",
            "rgb(46, 125, 79)",
            "rgb(179, 57, 43)",
            "rgb(148, 106, 0)",
        ),
        (
            "dark",
            "rgb(111, 196, 143)",
            "rgb(229, 122, 106)",
            "rgb(224, 184, 76)",
        ),
    ] {
        browser.scheme(mode);
        // Read the completed button colour transition without changing the palette
        // assertion or the existing bounded browser wait.
        assert!(browser.wait_for(&format!("getComputedStyle(document.querySelector('.v-approved')).backgroundColor === {} && getComputedStyle(document.querySelector('.v-rejected')).color === {} && getComputedStyle(document.querySelector('.v-question')).color === {}", json!(approved), json!(rejected), json!(question)), SHOWN));
        assert_eq!(
            browser.eval("getComputedStyle(document.querySelector('.v-approved')).backgroundColor"),
            json!(approved)
        );
        for (selector, colour) in [(".v-rejected", rejected), (".v-question", question)] {
            assert_eq!(
                browser.eval(&format!(
                    "getComputedStyle(document.querySelector({})).color",
                    json!(selector)
                )),
                json!(colour)
            );
        }
    }
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.v-approved')).borderRadius"),
        json!("8px")
    );
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.callout')).borderRadius"),
        json!("10px")
    );
    assert!(
        browser
            .eval("document.querySelector('.doc-head h1').textContent")
            .as_str()
            .unwrap()
            .contains("Look parity")
    );
    browser.eval("document.querySelector('.doc-toc details').open=false");
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.doc-layout')).display"),
        json!("block")
    );
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.doc-body table')).borderCollapse"),
        json!("separate")
    );
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.doc-body table')).borderRadius"),
        json!("10px")
    );
    report_styles(&mut browser, "document, hover, keyboard and both palettes");
}

#[test]
fn waiting_columns_keep_status_tones_priority_edges_and_a_single_phone_stage() {
    let site = ParitySite::start();
    let Some(mut browser) = chrome() else { return };
    browser.viewport(1440, 900, false);
    track_styles(&mut browser);
    browser.scheme("light");
    browser.open(&site.url("/tasks"));
    // Lifecycle and machine columns keep their status colours before observations arrive.
    assert!(browser.wait_for("document.querySelectorAll('.waiting-columns .column').length === 8 && document.querySelector('.taskcard') !== null",SHOWN), "the board shows {}", browser.eval("JSON.stringify({columns: [...document.querySelectorAll('.column')].map(c => c.dataset.status), cards: [...document.querySelectorAll('.taskcard')].map(c => c.dataset.slug)})"));
    painted_layout(&mut browser);
    for (status, colour) in [
        ("concept", "rgb(74, 63, 158)"),
        ("design", "rgb(74, 63, 158)"),
        ("ready", "rgb(31, 122, 140)"),
        ("in_progress", "rgb(47, 93, 158)"),
        ("build", "rgb(47, 93, 158)"),
        ("test", "rgb(47, 93, 158)"),
        ("review", "rgb(148, 106, 0)"),
        ("done", "rgb(46, 125, 79)"),
    ] {
        assert_eq!(browser.eval(&format!("getComputedStyle(document.querySelector('.column[data-status={status}]')).borderTopColor")),json!(colour),"{status}");
    }
    assert_eq!(
        browser.eval(
            "[...document.querySelectorAll('.waiting-columns .column')].map(e=>e.dataset.status)"
        ),
        json!([
            "concept",
            "design",
            "ready",
            "in_progress",
            "build",
            "test",
            "review",
            "done"
        ])
    );
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.taskcard.p1')).borderLeftWidth"),
        json!("3px")
    );
    browser.viewport(390, 844, true);
    assert!(browser.wait_for("document.querySelectorAll('.waiting-columns .column').length === 1 && document.querySelector('.column[data-status=review] .taskcard') !== null", SHOWN));
    assert_eq!(
        browser.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
    report_styles(&mut browser, "task columns, desktop and phone");
}

#[test]
fn task_priority_and_recipient_labels_stay_inline_with_their_controls() {
    let site = ParitySite::start();
    let Some(mut browser) = chrome() else { return };
    browser.viewport(1280, 800, false);
    browser.open(&site.url("/task/visual/in-progress"));
    browser.reveal("[id=\"composer-visual/in-progress\"]");
    assert!(browser.wait_for(
        "document.querySelector('#task-send .inline-control .dx-select') !== null",
        SHOWN
    ));
    assert!(browser.wait_for(
        "document.querySelector('#task-thread .entry.owner') !== null",
        SHOWN
    ));
    for (mode, background) in [("light", "rgb(251, 250, 247)"), ("dark", "rgb(22, 21, 19)")] {
        browser.scheme(mode);
        painted_layout(&mut browser);
        assert_eq!(browser.eval("(() => {const dot=getComputedStyle(document.querySelector('#task-thread .entry.owner'),'::before');return {width:dot.width,height:dot.height,shadow:dot.boxShadow};})()"), json!({"width":"8px","height":"8px","shadow":format!("{background} 0px 0px 0px 3px")}), "{mode}: the timeline gap must use the page background, leaving only the 8px comment dot visible");
        for selector in [".task-controls > label", "#task-send .row > label"] {
            assert_eq!(browser.eval(&format!("(() => {{const label=document.querySelector({}), select=label.querySelector('.dx-select').getBoundingClientRect(), text=document.createRange();text.selectNodeContents(label.firstChild);const caption=text.getBoundingClientRect(), row=label.parentElement.getBoundingClientRect();return caption.right <= select.left && Math.abs((caption.top+caption.bottom-select.top-select.bottom)/2) <= 1 && row.height <= select.height+2;}})()", json!(selector))), json!(true), "{mode}: {selector} must share one line with its select and adjacent action");
        }
        assert_eq!(
            browser.eval(
                "getComputedStyle(document.querySelector('label[for=task-message]')).display"
            ),
            json!("block"),
            "form field labels remain stacked"
        );
    }
}

#[test]
fn queue_agent_task_and_chat_surfaces_keep_their_compact_review_treatment() {
    let site = ParitySite::start();
    let Some(mut browser) = chrome() else { return };
    track_styles(&mut browser);
    browser.scheme("light");
    browser.open(&site.url("/queue"));
    assert!(browser.wait_for("document.querySelector('.queue-rank') !== null", SHOWN));
    painted_layout(&mut browser);
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('#bell svg')).stroke"),
        json!("rgb(29, 28, 26)")
    );
    assert_eq!(
        browser.eval("window.reviewPanelReady === true"),
        json!(false)
    );
    // The activity panel's own file: the chat's script.
    let panel = module(dist(), "panel").script;
    assert!(
        browser
            .requests()
            .iter()
            .all(|request| !request.url.ends_with(&panel) || request.prefetch),
        "the activity module loaded before interaction, beyond an idle prefetch"
    );
    let activity_open = "window.reviewPanelReady === true && document.getElementById('activity-panel') !== null && document.getElementById('bell').getAttribute('aria-expanded') === 'true'";
    let activity_closed = "document.getElementById('activity-panel') === null && document.getElementById('bell').getAttribute('aria-expanded') === 'false'";
    assert!(browser.wait_for(activity_closed, SHOWN));
    // Reopening after each close checks the current bell capture and the persistent panel's
    // state flowing back to the page. The quick action also closes activity from the page.
    for close in [
        "document.getElementById('bell').click()",
        "document.querySelector('#activity-panel [aria-label=\"Close activity\"]').click()",
        "document.getElementById('quick-open').click()",
    ] {
        browser.eval("document.getElementById('bell').click()");
        assert!(
            browser.wait_for(activity_open, SHOWN),
            "bell did not open activity"
        );
        browser.eval(close);
        assert!(
            browser.wait_for(activity_closed, SHOWN),
            "activity did not close: {close}"
        );
    }
    assert_eq!(
        browser
            .requests()
            .iter()
            .filter(|request| {
                request.url.ends_with(&panel) && request.status == 200 && !request.prefetch
            })
            .count(),
        1,
        "reopening activity should reuse its deferred module"
    );
    assert!(browser.wait_for(
        "document.querySelector('#quick-sheet:not([hidden]) form') !== null && document.getElementById('quick-open').getAttribute('aria-expanded') === 'true'",
        SHOWN
    ));
    browser.eval("document.querySelector('#quick-sheet [aria-label=\"Collapse panel\"]').click()");
    assert!(browser.wait_for(
        "document.getElementById('quick-sheet').hidden && document.getElementById('quick-open').getAttribute('aria-expanded') === 'false'",
        SHOWN
    ));
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.queue-rank')).borderRadius"),
        json!("999px")
    );
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.queue-row a')).textDecorationLine"),
        json!("none")
    );
    // A row's title is a link and reads as one: the light theme's link colour, not the text's.
    assert_eq!(
        browser.eval("[getComputedStyle(document.querySelector('#you .queue-row a:not(.chip)')).color, getComputedStyle(document.querySelector('.page-head h1')).color]"),
        json!(["rgb(47, 93, 158)", "rgb(29, 28, 26)"])
    );
    browser.open(&site.url("/agents"));
    assert!(
        browser.wait_for(
            "document.querySelector('.agents-card .agent') !== null && document.querySelector('.page-head + .muted')?.textContent === 'Terminal panes: unavailable · Test slots: ok'",
            SHOWN
        ),
        "agents: {}",
        browser.eval("document.body.innerText")
    );
    painted_layout(&mut browser);
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.agent')).borderTopWidth"),
        json!("0px")
    );
    assert_eq!(
        browser.eval("document.querySelector('.agent-connection').open"),
        json!(false)
    );
    assert!(
        browser
            .eval("document.querySelector('.agent-connection').textContent")
            .as_str()
            .unwrap()
            .contains("codex:example")
    );
    assert_eq!(
        browser.eval(
            "getComputedStyle(document.querySelector('.agents-now .taskcard')).borderTopWidth"
        ),
        json!("3px")
    );
    browser.open(&site.url("/task/visual/in-progress"));
    assert!(
        browser.wait_for("document.querySelector('.task-head') !== null", SHOWN),
        "task view: {}",
        browser.eval("document.body.innerText")
    );
    painted_layout(&mut browser);
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.task-head')).borderRadius"),
        json!("14px")
    );
    assert_eq!(
        browser.eval("document.querySelector('.task-head .facts li')?.textContent"),
        json!("after visual/ready")
    );
    assert_eq!(
        browser.eval("const facts=document.querySelector('.task-head .facts'); facts.getBoundingClientRect().top-facts.previousElementSibling.getBoundingClientRect().bottom"),
        json!(14),
        "the state and priority row keeps its gap above task facts"
    );
    // The head, the gates and the timeline's box can put the lazy thread below the window and past
    // its look-ahead: scroll to its place, always in the page, as a person would.
    browser.eval(
        "document.getElementById('thread-region-visual/in-progress').scrollIntoView({block:'center'})",
    );
    assert!(
        browser.wait_for("document.querySelector('.entry.owner') !== null", SHOWN),
        "task thread: {}",
        browser.eval("document.getElementById('thread-region-visual/in-progress').outerHTML")
    );
    assert_eq!(
        browser.eval(
            "getComputedStyle(document.querySelector('.entry.owner'),'::before').backgroundColor"
        ),
        json!("rgb(47, 93, 158)")
    );
    browser.eval("document.getElementById('quick-open').click()");
    assert!(browser.wait_for(
        "document.querySelector('#quick-sheet form') !== null",
        SHOWN
    ));
    // The deferred panel also appends its own stylesheet before mounting its form.
    painted_layout(&mut browser);
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.quick-thread')).overflowY"),
        json!("auto")
    );
    browser.viewport(390, 844, true);
    assert!(browser.wait_for(
        "document.getElementById('quick-sheet').getBoundingClientRect().width < 400",
        SHOWN
    ));
    assert_eq!(browser.eval("const sheet=document.getElementById('quick-sheet').getBoundingClientRect(); sheet.x >= 0 && sheet.bottom <= innerHeight + 1"),json!(true));
    browser.open(&site.url("/docs/"));
    assert!(browser.wait_for(
        "document.querySelector('.folder-entry.tree-folder') !== null",
        SHOWN
    ));
    painted_layout(&mut browser);
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.tree-folder')).borderTopWidth"),
        json!("3px")
    );
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.tree-icon')).backgroundColor"),
        json!("rgba(47, 93, 158, 0.07)")
    );
    report_styles(&mut browser, "queue, agents, task, chat and folder");
}
