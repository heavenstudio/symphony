//! Real server delivery, driven by a fixture process table, and the shell's own failure notices.
use super::{
    SHOWN, chrome, chrome::Chrome, owner_controls::ScratchBoard, projects::OneServer, site,
    wait_for_board,
};
use serde_json::json;

/// Geometry and computed paint, independently of the notice's class names.
fn contained(browser: &mut Chrome, header: bool) {
    let look = browser.eval(r#"(() => {
        const region = document.querySelector('#shell-notices');
        const notice = region.querySelector('[role=status], [role=alert]');
        const box = notice.getBoundingClientRect(), style = getComputedStyle(notice);
        const command = notice.querySelector('code');
        const header = document.querySelector('.app-header')?.getBoundingClientRect();
        return {
            insideApp: region.parentElement.id === 'main',
            singleRegion: document.querySelectorAll('#shell-notices').length === 1,
            visible: box.width > 0 && box.height > 0 && box.left >= 0 && box.right <= innerWidth,
            painted: parseFloat(style.borderLeftWidth) >= 1 && style.borderLeftStyle === 'solid' && style.backgroundColor !== 'rgba(0, 0, 0, 0)' && parseFloat(style.paddingLeft) >= 12,
            wrapped: !command || (command.scrollWidth <= command.clientWidth && (innerWidth > 390 || command.getBoundingClientRect().height > parseFloat(getComputedStyle(command).lineHeight))),
            overflow: document.documentElement.scrollWidth > innerWidth,
            header: !!header,
            overlap: !!header && box.bottom > header.top && box.top < header.bottom,
            hit: document.elementFromPoint(box.left + box.width / 2, box.top + 10)?.closest('[role]') === notice
        };
    })()"#);
    assert_eq!(
        look,
        json!({"insideApp":true,"singleRegion":true,"visible":true,"painted":true,"wrapped":true,"overflow":false,"header":header,"overlap":false,"hit":true})
    );
}

fn warning(browser: &mut Chrome, count: usize) {
    contained(browser, false);
    let noun = if count == 1 {
        "process from test runs is"
    } else {
        "processes from test runs are"
    };
    let text = browser.eval("document.querySelector('#shell-notices [role=status]').textContent");
    let text = text.as_str().unwrap();
    assert!(
        text.contains(&format!("{count} Symphony {noun} running with no parent")),
        "{text}"
    );
    assert!(
        text.contains("Symphony kills none")
            && text.contains("ps -axo pid,ppid,command")
            && text.contains("parent 1; stop"),
        "{text}"
    );
    let object = browser.call(
        "Runtime.evaluate",
        json!({"expression":"document.querySelector('#shell-notices [role=status]')"}),
    );
    let tree = browser.call(
        "Accessibility.getPartialAXTree",
        json!({"objectId":object["result"]["objectId"],"fetchRelatives":false}),
    );
    let node = &tree["nodes"][0];
    assert_eq!(node["role"]["value"], json!("status"));
    let properties = node["properties"].as_array().unwrap();
    assert!(
        properties
            .iter()
            .any(|p| p["name"] == "live" && p["value"]["value"] == "polite"),
        "{node}"
    );
    assert!(
        properties
            .iter()
            .any(|p| p["name"] == "atomic" && p["value"]["value"] == true),
        "{node}"
    );
    screenshot(browser, &format!("server-warning-{count}-initial"));
}

fn screenshot(browser: &mut Chrome, name: &str) {
    let shot = browser.call("Page.captureScreenshot", json!({"format":"png"}));
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../../artifacts/server-notices/{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(
        output.join(format!("{name}.base64")),
        shot["data"].as_str().unwrap(),
    )
    .unwrap();
}

#[test]
fn server_warnings_are_contained_before_hydration_and_across_native_navigation() {
    let board = ScratchBoard::start();
    for count in [1, 3, 0] {
        let server = OneServer::with_orphans(board.address(), count);
        let Some(mut browser) = chrome() else { return };
        browser.open(&server.url("/api/front"));
        assert!(browser.wait_for(
            "fetch('/api/front').then(r=>r.json()).then(front=>!front.uncounted)",
            SHOWN
        ));
        assert_eq!(
            browser.eval("fetch('/api/front').then(r=>r.json()).then(front=>front.orphans)"),
            json!(count)
        );
        browser.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":320,"height":900,"deviceScaleFactor":1,"mobile":true}),
        );
        browser.call(
            "Emulation.setScriptExecutionDisabled",
            json!({"value":true}),
        );
        browser.open(&server.url("/alpha/"));
        if count == 0 {
            assert_eq!(
                browser.eval(
                    "document.querySelector('#shell-notices').getBoundingClientRect().height"
                ),
                json!(0)
            );
        } else {
            warning(&mut browser, count);
        }
        browser.call(
            "Emulation.setScriptExecutionDisabled",
            json!({"value":false}),
        );
        browser.open(&server.url("/alpha/"));
        assert!(browser.wait_for("document.documentElement.dataset.navigation === 'ready' && document.querySelector('.app-header') !== null", SHOWN));
        browser.eval("window.keptWarningPage = true; document.querySelector('header a[href=\"/tasks\"]').click()");
        assert!(browser.wait_for("location.pathname === '/alpha/tasks' && document.documentElement.dataset.navigation === 'ready'", SHOWN));
        assert_eq!(
            browser.eval("window.keptWarningPage"),
            json!(true),
            "navigation must retain the document"
        );
        if count > 0 {
            contained(&mut browser, true);
            // A wider page may keep the command on one line; the phone assertion above proves wrapping.
            browser.call(
                "Emulation.setDeviceMetricsOverride",
                json!({"width":390,"height":900,"deviceScaleFactor":1,"mobile":true}),
            );
            contained(&mut browser, true);
            if count == 1 {
                screenshot(&mut browser, "server-warning-mobile");
            }
            browser.eval("history.back()");
            assert!(browser.wait_for("location.pathname === '/alpha/' && document.documentElement.dataset.navigation === 'ready'", SHOWN));
            contained(&mut browser, true);
            browser.call(
                "Emulation.setDeviceMetricsOverride",
                json!({"width":1024,"height":900,"deviceScaleFactor":1,"mobile":false}),
            );
            contained(&mut browser, true);
            if count == 1 {
                screenshot(&mut browser, "server-warning-desktop");
            }
        } else {
            assert_eq!(browser.eval("({notices:document.querySelector('#shell-notices').children.length, headerTop:document.querySelector('.app-header').getBoundingClientRect().top, overflow:document.documentElement.scrollWidth > innerWidth})"), json!({"notices":0,"headerTop":0,"overflow":false}));
        }
    }
}

#[test]
fn shell_loading_errors_use_the_notice_region_and_retry_in_place() {
    let site = site();
    let Some(mut browser) = chrome() else { return };
    browser.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":320,"height":900,"deviceScaleFactor":1,"mobile":true}),
    );
    // Block the actual modules, rather than implement an error UI in the fixture.
    browser.call(
        "Network.setBlockedURLs",
        json!({"urls":["*assets/review-*.js"]}),
    );
    browser.open(&site.url("/"));
    assert!(browser.wait_for(
        "document.querySelector('#shell-notices #module-error[role=alert] button') !== null",
        SHOWN
    ));
    contained(&mut browser, false);
    browser.call("Network.setBlockedURLs", json!({"urls":[]}));
    browser.eval("document.querySelector('#module-error button').click()");
    assert!(browser.wait_for("document.querySelector('.hero-title') !== null && !document.querySelector('#module-error')", SHOWN));
    fail_panel(&mut browser);
    browser.eval("document.querySelector('#panel-error button').click()");
    assert!(browser.wait_for(
        "window.reviewPanelReady === true && !document.querySelector('#panel-error')",
        SHOWN
    ));
}

/// A real failed module request with a visible alert, restored network and no loaded panel.
fn fail_panel(browser: &mut Chrome) {
    browser.call(
        "Network.setBlockedURLs",
        json!({"urls":["*assets/panel-*.js"]}),
    );
    browser.eval("document.querySelector('#quick-open').click()");
    assert!(browser.wait_for(
        "document.querySelector('#shell-notices #panel-error[role=alert] button') !== null",
        SHOWN
    ));
    contained(browser, true);
    assert_eq!(
        browser.eval("window.reviewPanelReady === true"),
        json!(false)
    );
    browser.call("Network.setBlockedURLs", json!({"urls":[]}));
}

#[test]
fn a_panel_recovered_by_activity_removes_its_failed_load_alert() {
    let site = site();
    let Some(mut browser) = chrome() else { return };
    browser.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":320,"height":900,"deviceScaleFactor":1,"mobile":true}),
    );
    browser.open(&site.url("/"));
    wait_for_board(&mut browser);
    fail_panel(&mut browser);
    screenshot(&mut browser, "panel-event-error");
    browser.eval("dispatchEvent(new Event('review:activity-arrived'))");
    assert!(browser.wait_for(
        "window.reviewPanelReady === true && document.querySelector('#quick-text') !== null && !document.querySelector('#panel-error')",
        SHOWN
    ));
    screenshot(&mut browser, "panel-event-recovered");
}
