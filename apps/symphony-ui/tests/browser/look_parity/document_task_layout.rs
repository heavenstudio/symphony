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
    browser.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":1440,"height":900,"deviceScaleFactor":1,"mobile":false}),
    );
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
            browser.call(
                "Emulation.setDeviceMetricsOverride",
                json!({"width":width,"height":900,"deviceScaleFactor":1,"mobile":false}),
            );
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
                json!(["in_progress", "ready"])
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
