//! All document entry points exercise the production task panel and responsive stylesheet.
use super::{ParitySite, folder_responses, painted_layout, report_styles, track_styles};
use crate::{SHOWN, chrome};
use serde_json::json;

#[test]
fn a_long_task_panel_keeps_its_last_link_clear_of_fixed_page_controls() {
    let mut data = folder_responses();
    let document = &mut data.get_mut("/api/document").unwrap()["document"];
    document["path"] = json!("docs/game/SPEC.md");
    document["tasks"] = json!((0..20).map(|index| json!({"slug":format!("ui/{index}"),"title":format!("Related task number {index}"),"status":"ready"})).collect::<Vec<_>>());
    document["sections"][0]["blocks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"code","language":"text","text":"Read the document.\n".repeat(200)}));
    let sections = document["sections"].clone();
    data.get_mut("/api/section").unwrap()["change"]["path"] = json!("docs/game/SPEC.md");
    data.get_mut("/api/section").unwrap()["change"]["sections"] = sections;
    let site = ParitySite::with_data(data);
    let Some(mut browser) = chrome() else { return };
    browser.viewport(1440, 900, false);
    browser.open(&site.url("/docs/game/SPEC.md"));
    assert!(browser.wait_for("document.querySelector('.doc-body pre') !== null && document.querySelector('.doc-tasks a') !== null && document.querySelector('#quick-open') !== null",SHOWN));
    painted_layout(&mut browser);
    assert_eq!(browser.eval("document.documentElement.scrollHeight > 2000 && document.querySelector('.doc-tasks').scrollHeight > document.querySelector('.doc-tasks').clientHeight"),json!(true),"the fixture must exercise both page scrolling and an overflowing task list");
    browser.eval("(() => {scrollTo(0,1000);const panel=document.querySelector('.doc-tasks');panel.scrollTop=panel.scrollHeight;})()");
    assert!(browser.wait_for(
        "document.querySelector('.doc-tasks a[href=\"/task/ui/19\"]') !== null",
        SHOWN
    ));
    painted_layout(&mut browser);
    browser.eval(
        "(() => {const panel=document.querySelector('.doc-tasks');panel.scrollTop=panel.scrollHeight;})()",
    );
    assert_eq!(browser.eval("(() => {const panel=document.querySelector('.doc-tasks').getBoundingClientRect(), control=document.querySelector('#quick-open').getBoundingClientRect(), link=document.querySelector('.doc-tasks a[href=\"/task/ui/19\"]'), box=link.getBoundingClientRect();return panel.bottom <= control.top && box.top >= panel.top && document.elementFromPoint(box.x+box.width/2,box.y+box.height/2)?.closest('a') === link;})()"),json!(true),"the last task must remain clickable above the fixed page controls");
}

/// The owner, 2026-11-11, of a document's panel: "Side panel for related tasks shows repeated
/// content". One open row with its slug, the finished ones folded under their count, and no task
/// the Symphony filed for the owner's verdicts.
#[test]
fn the_document_task_panel_tells_its_rows_apart_and_folds_finished_work() {
    let mut data = folder_responses();
    let document = &mut data.get_mut("/api/document").unwrap()["document"];
    document["path"] = json!("docs/game/SPEC.md");
    document["tasks"] = json!([
        {"slug":"perf/client-cache/measure","title":"Measure return visits","status":"done"},
        {"slug":"verdicts/2026-11-11-1244","title":"Answer the owner on lamp-a-pic","status":"ready"},
        {"slug":"perf/client-cache","title":"Measure return visits","status":"in_progress"},
        {"slug":"perf/client-cache/design","title":"Measure return visits","status":"done"}
    ]);
    data.get_mut("/api/section").unwrap()["change"]["path"] = json!("docs/game/SPEC.md");
    let site = ParitySite::with_data(data);
    let Some(mut browser) = chrome() else { return };
    browser.viewport(1440, 900, false);
    browser.open(&site.url("/docs/game/SPEC.md"));
    assert!(
        browser.wait_for(
            "document.querySelector('.doc-tasks-open a') !== null && document.querySelector('.doc-tasks-finished') !== null",
            SHOWN
        ),
        "{}",
        browser.eval("document.body.innerText")
    );
    painted_layout(&mut browser);
    let rows = "(list => [...document.querySelectorAll(list + ' li')].filter(li => li.checkVisibility()).map(li => [li.querySelector('a')?.getAttribute('href'), li.querySelector('a')?.textContent, li.querySelector('.doc-task-slug')?.textContent, li.querySelector('.doc-task-status')?.textContent]))";
    assert_eq!(
        browser.eval(&format!("{rows}('.doc-tasks-open')")),
        json!([[
            "/task/perf/client-cache",
            "Measure return visits",
            "perf/client-cache",
            "in progress"
        ]]),
        "one open row, its slug beside its title and its status by the board column's name"
    );
    assert_eq!(
        browser.eval("document.querySelector('.doc-tasks-open .doc-task-meta').textContent"),
        json!("perf/client-cache · in progress"),
        "the slug and the status are set apart"
    );
    assert_eq!(
        browser.eval("(() => { const folded = document.querySelector('.doc-tasks-finished'); return [folded.open, folded.querySelector('summary').textContent, folded.querySelector('summary').getBoundingClientRect().top > document.querySelector('.doc-tasks-open').getBoundingClientRect().bottom]; })()"),
        json!([false, "2 finished", true]),
        "the finished work is folded under its count, below the open work"
    );
    assert_eq!(
        browser.eval("(() => { const folded = document.querySelector('.doc-tasks-finished'), summary = folded.querySelector('summary'), row = folded.getBoundingClientRect(), box = summary.getBoundingClientRect(); return Math.abs(box.right - row.right) <= 1 && document.elementFromPoint(row.right - 4, box.top + box.height / 2)?.closest('summary') === summary; })()"),
        json!(true),
        "the count's whole row opens it, beside its words too, as a phone's tap does"
    );
    assert_eq!(
        browser.eval(&format!("{rows}('.doc-tasks-finished')")),
        json!([]),
        "nothing finished shows while it is folded"
    );
    assert_eq!(
        browser.eval("document.querySelector('.doc-tasks a[href^=\"/task/verdicts/\"]') === null && !document.querySelector('.doc-tasks').textContent.includes('verdicts/') && !document.querySelector('.doc-tasks').textContent.includes('Answer')"),
        json!(true),
        "no row of the task filed for the owner's verdicts"
    );
    browser.eval("document.querySelector('.doc-tasks-finished summary').click()");
    assert!(
        browser.wait_for(
            &format!(
                "{rows}('.doc-tasks-finished').filter(row => row[0] !== undefined).length === 2"
            ),
            SHOWN
        ),
        "{}",
        browser.eval("document.querySelector('.doc-tasks').innerHTML")
    );
    assert_eq!(
        browser.eval(&format!("{rows}('.doc-tasks-finished')")),
        json!([
            [
                "/task/perf/client-cache/measure",
                "Measure return visits",
                "perf/client-cache/measure",
                "done"
            ],
            [
                "/task/perf/client-cache/design",
                "Measure return visits",
                "perf/client-cache/design",
                "done"
            ]
        ]),
        "opening the count shows the finished rows, each by its slug"
    );
}

#[test]
fn every_document_entry_uses_the_same_responsive_task_panel() {
    let Some(mut browser) = chrome() else { return };
    for (route, path, headings) in [
        ("/docs/lamps/LAMPS.md", "docs/lamps/LAMPS.md", true),
        ("/lamps", "docs/lamps/LAMPS.md", true),
        ("/docs/game/SPEC.md", "docs/game/SPEC.md", false),
        ("/docs/", "docs/README.md", true),
        ("/@task-layout/docs/game/SPEC.md", "docs/game/SPEC.md", true),
        ("/@task-layout/docs/", "docs/README.md", true),
    ] {
        let mut data = folder_responses();
        let document = &mut data.get_mut("/api/document").unwrap()["document"];
        document["path"] = json!(path);
        if !headings {
            document["headings"] = json!([]);
        }
        let title = "A long task title that remains readable ".repeat(4);
        document["tasks"] = json!([
            {"slug":"ui/layout","title":title,"status":"in_progress"},
            {"slug":"ui/navigation","title":"Navigation","status":"ready"}
        ]);
        data.get_mut("/api/section").unwrap()["change"]["path"] = json!(path);
        let site = ParitySite::with_data(data);
        for width in [1440, 390] {
            browser.viewport(width, 900, false);
            // Include the new document's first style calculation in the coverage sample.
            track_styles(&mut browser);
            browser.open(&site.url(route));
            assert!(browser.wait_for("document.querySelectorAll('.doc-tasks a').length === 2 && document.querySelector('.doc-body p')?.textContent === 'Design references for the Symphony.'",SHOWN),"{route} at {width}: {}",browser.eval("document.body.innerText"));
            painted_layout(&mut browser);
            assert_eq!(
                browser.eval(
                    "[...document.querySelectorAll('.doc-tasks a')].map(a=>a.getAttribute('href'))"
                ),
                json!(["/task/ui/layout", "/task/ui/navigation"]),
                "{route} at {width}"
            );
            assert_eq!(
                browser.eval(
                    "[...document.querySelectorAll('.doc-task-status')].map(e=>e.textContent)"
                ),
                json!(["in progress", "ready"])
            );
            assert_eq!(browser.eval("document.querySelectorAll('.document-content-layout > aside[aria-label=\"Related tasks\"]').length === 1 && document.querySelector('.doc-body .doc-tasks') === null"),json!(true),"{route} must share the panel owner");
            assert_eq!(
                browser.eval("document.documentElement.scrollWidth <= innerWidth"),
                json!(true),
                "{route} at {width} must not overflow horizontally"
            );
            let placement = if width > 1100 {
                "panel.left >= content.right + 23 && panel.width === 280 && Math.abs(panel.top-content.top) <= 1"
            } else {
                "panel.top >= content.bottom - 1 && Math.abs(panel.left-content.left) <= 1 && Math.abs(panel.width-content.width) <= 1"
            };
            assert_eq!(browser.eval(&format!("(() => {{ const panel=document.querySelector('.doc-tasks').getBoundingClientRect(), content=document.querySelector('.document-content').getBoundingClientRect(); return {placement}; }})()")),json!(true),"{route} at {width}: {}",browser.eval("[...document.querySelectorAll('.document-content,.doc-tasks')].map(e=>({class:e.className,box:e.getBoundingClientRect().toJSON()}))"));
            report_styles(&mut browser, &format!("document tasks: {route} at {width}"));
        }
    }
}
