//! The shell's own failure notices: a page module or the conversation that fails to load is
//! said in the shell's notice region, contained on the page, and retried in place.
use super::{SHOWN, assets::module, chrome, chrome::Chrome, dist, site, wait_for_board};
use serde_json::json;

enum HeaderPresence {
    Absent,
    Present,
}

fn wait_for(browser: &mut Chrome, condition: &str) {
    assert!(
        browser.wait_for(condition, SHOWN),
        "{condition}\n{}",
        browser.eval("({url:location.href, navigation:document.documentElement.dataset.navigation, panelReady:window.reviewPanelReady === true, panelError:document.querySelector('#panel-error')?.textContent, body:document.body?.innerText.slice(0,4000)})")
    );
}

/// Geometry and computed paint, independently of the notice's class names.
fn contained(browser: &mut Chrome, header: HeaderPresence) {
    let header = matches!(header, HeaderPresence::Present);
    let look = browser.eval(r#"(() => {
        const region = document.querySelector('#shell-notices');
        const notice = region.querySelector('[role=status], [role=alert]');
        const box = notice.getBoundingClientRect(), style = getComputedStyle(notice);
        const header = document.querySelector('.app-header')?.getBoundingClientRect();
        return {
            insideApp: region.parentElement.id === 'main',
            singleRegion: document.querySelectorAll('#shell-notices').length === 1,
            visible: box.width > 0 && box.height > 0 && box.left >= 0 && box.right <= innerWidth,
            painted: parseFloat(style.borderLeftWidth) >= 1 && style.borderLeftStyle === 'solid' && style.backgroundColor !== 'rgba(0, 0, 0, 0)' && parseFloat(style.paddingLeft) >= 12,
            overflow: document.documentElement.scrollWidth > innerWidth,
            header: !!header,
            overlap: !!header && box.bottom > header.top && box.top < header.bottom,
            hit: document.elementFromPoint(box.left + box.width / 2, box.top + 10)?.closest('[role]') === notice
        };
    })()"#);
    assert_eq!(
        look,
        json!({"insideApp":true,"singleRegion":true,"visible":true,"painted":true,"overflow":false,"header":header,"overlap":false,"hit":true})
    );
}

fn screenshot(browser: &mut Chrome, name: &str) {
    let shot = browser.call("Page.captureScreenshot", json!({"format":"png"}));
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../artifacts/server-notices/{}",
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
fn shell_loading_errors_use_the_notice_region_and_retry_in_place() {
    let site = site();
    let Some(mut browser) = chrome() else { return };
    // No idle prefetch serves the blocked panel script from the cache first.
    browser.save_data();
    browser.viewport(320, 900, true);
    // Block the actual modules, rather than implement an error UI in the fixture.
    browser.call(
        "Network.setBlockedURLs",
        json!({"urls":["*assets/review-*.js"]}),
    );
    browser.open(&site.url("/"));
    wait_for(
        &mut browser,
        "document.querySelector('#shell-notices #module-error[role=alert] button') !== null",
    );
    contained(&mut browser, HeaderPresence::Absent);
    browser.call("Network.setBlockedURLs", json!({"urls":[]}));
    browser.eval("document.querySelector('#module-error button').click()");
    wait_for(
        &mut browser,
        "document.querySelector('.hero-title') !== null && !document.querySelector('#module-error')",
    );
    fail_panel(&mut browser);
    browser.eval("document.querySelector('#panel-error button').click()");
    wait_for(
        &mut browser,
        "window.reviewPanelReady === true && !document.querySelector('#panel-error')",
    );
}

/// A real failed module request with a visible alert, restored network and no loaded panel: the
/// chat's own file, its script.
fn fail_panel(browser: &mut Chrome) {
    let script = module(dist(), "panel").script;
    browser.call(
        "Network.setBlockedURLs",
        json!({"urls":[format!("*{script}")]}),
    );
    browser.eval("document.querySelector('#quick-open').click()");
    wait_for(
        browser,
        "document.querySelector('#shell-notices #panel-error[role=alert] button') !== null",
    );
    contained(browser, HeaderPresence::Present);
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
    browser.save_data();
    browser.viewport(320, 900, true);
    browser.open(&site.url("/"));
    wait_for_board(&mut browser);
    fail_panel(&mut browser);
    screenshot(&mut browser, "panel-event-error");
    browser.eval("dispatchEvent(new Event('review:activity-arrived'))");
    wait_for(
        &mut browser,
        "window.reviewPanelReady === true && document.querySelector('#quick-text') !== null && !document.querySelector('#panel-error')",
    );
    screenshot(&mut browser, "panel-event-recovered");
}
