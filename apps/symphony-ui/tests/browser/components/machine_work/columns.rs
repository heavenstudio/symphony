//! Full-card operational columns, readable board-only scrolling and disconnect fallback.

use crate::{SHOWN, chrome, look_parity, site, wait_for_feed};
use serde_json::{Value, json};

fn listing() -> Value {
    let mut listing = look_parity::responses().remove("/api/task-board").unwrap();
    let template = listing["tasks"][4].clone();
    let mut operations = Vec::new();
    for (kind, count) in [("build", 33), ("test", 1)] {
        for index in 0..count {
            let slug = format!("feature/{kind}-{index:02}");
            let mut task = template.clone();
            task["slug"] = json!(slug);
            task["title"] = json!(format!(
                "{} the board with full task cards",
                if kind == "build" { "Build" } else { "Test" }
            ));
            if index < 30 {
                listing["tasks"].as_array_mut().unwrap().push(task);
            }
            operations.push(
                json!({"machine_id":"alpha","operation_id":slug,"parent_id":null,
                "handle":"example","task":slug,"checkout":"opaque","recipe":"fixture",
                "kind":kind,"state":if index % 2 == 0 {"running"} else {"queued"},
                "resource":"native","reason":"fixture","phase_since":null}),
            );
        }
    }
    listing["machine_work"] = json!({"freshness":"fresh","unassigned":[],"machines":[{
        "machine_id":"alpha","label":"Workstation","producer_id":"fixture","sequence":1,
        "age_ms":0,"freshness":"incomplete","complete":false,"reason":"partial native inventory","counts":null,"operations":operations}]});
    listing["statuses"]
        .as_array_mut()
        .unwrap()
        .extend([json!("build"), json!("test")]);
    listing["counts"]
        .as_array_mut()
        .unwrap()
        .extend([json!(["build", 33]), json!(["test", 1])]);
    listing["lifecycle_counts"] = json!([["in_progress", 35], ["ready", 1], ["review", 1]]);
    listing["next"] = json!("more-builds");
    listing
}

pub(super) fn screenshot(chrome: &mut crate::Chrome, name: &str) {
    chrome.eval("new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))");
    let shot = chrome.call("Page.captureScreenshot", json!({"format":"png"}));
    let folder =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../artifacts/silver-owl-416");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join(format!("{name}.base64")),
        shot["data"].as_str().unwrap(),
    )
    .unwrap();
}

fn ready(chrome: &mut crate::Chrome) {
    assert!(chrome.wait_for("document.querySelector('.board-scroll') !== null && document.querySelector('.pipeline .taskcard') !== null && document.documentElement.dataset.navigation === 'ready'", SHOWN));
    chrome.eval("document.fonts.ready.then(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))))");
}

fn activity(chrome: &mut crate::Chrome) {
    reveal(chrome, "build");
    assert!(chrome.wait_for("document.querySelector('[data-status=build] [data-slug=\"feature/build-00\"]') !== null && document.querySelector('[data-status=test] [data-slug=\"feature/test-00\"]') !== null", SHOWN));
}

/// The Status filter's options as its open popup draws them, each one's text, the popup closed
/// again after.
fn status_options(chrome: &mut crate::Chrome) -> Value {
    chrome.eval("document.querySelector('button[aria-label=Status]').focus()");
    assert!(
        chrome.wait_for(
            "document.activeElement?.matches('button[aria-label=Status]') === true",
            SHOWN
        ),
        "the Status field took no focus"
    );
    crate::components::key(chrome, " ", "Space", 32);
    assert!(
        chrome.wait_for(
            "document.querySelector('[role=listbox][data-state=open][data-positioned=true] [role=option]') !== null",
            SHOWN
        ),
        "the Status popup did not open: {}",
        chrome.eval("({expanded: document.querySelector('button[aria-label=Status]')?.getAttribute('aria-expanded'), active: document.activeElement?.outerHTML.slice(0, 200), overlay: window.reviewOverlayReady === true, notices: document.querySelector('#shell-notices')?.innerText ?? null, navigation: document.documentElement.dataset.navigation})")
    );
    let options = chrome.eval(
        "[...document.querySelectorAll('[role=listbox] [role=option]')].map(option => option.textContent.trim())",
    );
    crate::components::key(chrome, "Escape", "Escape", 27);
    assert!(chrome.wait_for("document.querySelector('[role=listbox]') === null", SHOWN));
    options
}

/// What the Status filter offers while no machine reports: all statuses and the nine a task holds.
const UNREPORTED: [&str; 10] = [
    "All statuses",
    "backlog",
    "concept",
    "design",
    "ready",
    "in progress",
    "review",
    "done",
    "blocked",
    "cancelled",
];

/// The board's columns as drawn, by `data-status`, the sign-off lane's and the lower row's too.
const DRAWN: &str =
    "[...document.querySelectorAll('.kanban section.column')].map(column => column.dataset.status)";

/// Until a machine reports the project's work the Status filter offers neither Build nor Test,
/// which one rail stands for, though the server's listing names both; once one reports, it
/// offers both after the nine, and Test shows its column alone (docs/MACHINE_WORK.md).
#[test]
fn the_status_filter_offers_build_and_test_once_a_machine_reports_and_neither_before() {
    let site = site();
    // The listing the server sends a connected page (`activity=1`) when no machine reports: it
    // names build and test after the nine (apps/symphony-server/src/api/task_board.rs) and holds
    // no machine work, so leaving them out is the client's own doing.
    let mut silent = look_parity::responses().remove("/api/task-board").unwrap();
    silent["statuses"]
        .as_array_mut()
        .unwrap()
        .extend([json!("build"), json!("test")]);
    assert_eq!(silent["statuses"].as_array().unwrap().len(), 11);
    assert!(silent.get("machine_work").is_none());
    site.set_json("/api/task-board", &silent);
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/tasks"));
    assert!(chrome.wait_for(
        "document.querySelector('[data-status=build-test]') !== null && document.querySelector('button[aria-label=Status]') !== null && document.documentElement.dataset.navigation === 'ready'",
        SHOWN
    ));
    let offered = status_options(&mut chrome);
    assert_eq!(
        offered,
        json!(UNREPORTED),
        "no machine reports: ten options, neither build nor test"
    );

    site.set_json("/api/task-board", &listing());
    chrome.open(&site.url("/tasks"));
    ready(&mut chrome);
    let mut expected = UNREPORTED.to_vec();
    expected.extend(["build", "test"]);
    let reported = status_options(&mut chrome);
    assert_eq!(
        reported,
        json!(expected),
        "a machine reports: twelve options, build and test after the nine"
    );
    crate::components::choose(&mut chrome, "[aria-label=Status]", "test");
    assert!(
        chrome.wait_for(
            &format!("JSON.stringify({DRAWN}) === '[\"test\"]' && document.querySelector('[data-status=test] [data-slug=\"feature/test-00\"]') !== null"),
            SHOWN
        ),
        "test shows its column alone, with its task: {}",
        chrome.eval(DRAWN)
    );
    assert_eq!(
        chrome.eval("[document.querySelector('[data-status=test] .count').textContent, document.querySelector('button[aria-label=Status]').textContent.trim()]"),
        json!(["1", "test"]),
        "the column counts its one task, and the field reads test"
    );
}

pub(super) fn reveal(chrome: &mut crate::Chrome, status: &str) {
    chrome.eval(&format!("(() => {{ const board = document.querySelector('.board-scroll'); const column = document.querySelector('[data-status={status}]'); board.scrollTo({{left:board.scrollLeft + column.getBoundingClientRect().left - board.getBoundingClientRect().left,top:0,behavior:'instant'}}); }})()"));
    assert!(chrome.wait_for(&format!("(() => {{ const board=document.querySelector('.board-scroll').getBoundingClientRect(); const col=document.querySelector('[data-status={status}]').getBoundingClientRect(); return col.left >= board.left - 2 && col.right <= board.right + 2; }})()"), SHOWN), "{status} is clipped: {}", chrome.eval("document.querySelector('.board-scroll').outerHTML.slice(0,2000)"));
}

#[test]
fn full_task_build_test_columns_keep_counts_readable_cards_and_owned_keyboard_touch_scroll() {
    let site = site();
    let listing = listing();
    site.set_json("/api/task-board", &listing);
    let Some(mut chrome) = chrome() else { return };
    chrome.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":1440,"height":1000,"deviceScaleFactor":1,"mobile":false}),
    );
    chrome.call("DOM.enable", json!({}));
    chrome.call(
        "Emulation.setEmulatedMedia",
        json!({"features":[{"name":"prefers-color-scheme","value":"light"}]}),
    );
    chrome.call("CSS.enable", json!({}));
    chrome.call("CSS.startRuleUsageTracking", json!({}));
    chrome.open(&site.url("/tasks"));
    ready(&mut chrome);
    screenshot(&mut chrome, "board-desktop-start");
    assert_eq!(
        chrome
            .eval("[...document.querySelectorAll('.pipeline > .column')].map(e=>e.dataset.status)"),
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
        chrome.eval("document.querySelector('main > .machine-work') === null"),
        json!(true)
    );
    let filter_x = chrome.eval("document.querySelector('.filters').getBoundingClientRect().x");
    chrome.eval("document.querySelector('.board-scroll').focus()");
    chrome.call(
        "Input.dispatchKeyEvent",
        json!({"type":"keyDown","key":"ArrowRight","code":"ArrowRight","windowsVirtualKeyCode":39}),
    );
    chrome.call(
        "Input.dispatchKeyEvent",
        json!({"type":"keyUp","key":"ArrowRight","code":"ArrowRight","windowsVirtualKeyCode":39}),
    );
    assert!(chrome.wait_for(
        "document.querySelector('.board-scroll').scrollLeft > 0",
        SHOWN
    ));
    assert_eq!(
        chrome.eval("document.activeElement.className"),
        json!("board-scroll")
    );
    activity(&mut chrome);
    // The board is as tall as its columns and the page scrolls down it, so focus brings its top
    // into view, below the header rather than behind it.
    assert_eq!(chrome.eval("document.querySelector('.board-scroll').getBoundingClientRect().top >= document.querySelector('header').getBoundingClientRect().bottom"), json!(true), "board focus hid the board under the header");
    assert_eq!(
        chrome.eval("document.querySelector('.filters').getBoundingClientRect().x"),
        filter_x
    );
    assert_eq!(
        chrome.eval("document.querySelectorAll('[data-slug=\"feature/build-00\"]').length"),
        json!(1)
    );
    assert_eq!(chrome.eval("document.querySelector('[data-status=in_progress] [data-slug=\"feature/build-00\"]') === null"), json!(true));
    assert_eq!(
        chrome.eval("document.querySelector('[data-status=build] .work-phase').textContent.trim()"),
        json!("Observed Build running · Workstation · partial inventory")
    );
    assert_eq!(
        chrome.eval("document.querySelector('[data-status=build] .count').textContent"),
        json!("33")
    );
    assert_eq!(
        chrome.eval("document.querySelector('[data-status=test] .count').textContent"),
        json!("1")
    );
    assert_eq!(
        chrome.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
    assert_eq!(
        chrome.eval(
            "document.querySelector('.pipeline .column').getBoundingClientRect().width >= 280"
        ),
        json!(true)
    );
    screenshot(&mut chrome, "board-desktop-activity");
    assert!(wait_for_feed(&site));
    let reads = chrome.eval("performance.getEntriesByType('resource').filter(e=>e.name.includes('/api/task-board?counts=1')).length").as_u64().unwrap();
    site.push(
        4190,
        &json!({"version":4190,"changes":[{"region":"task-counts","data":[["in_progress",35]]}]})
            .to_string(),
    );
    assert!(chrome.wait_for(&format!("performance.getEntriesByType('resource').filter(e=>e.name.includes('/api/task-board?counts=1')).length > {reads} && document.querySelector('[data-status=in_progress] .count').textContent === '1' && document.querySelector('[data-status=build] .count').textContent === '33'"), SHOWN), "lifecycle feed totals replaced the complete stage totals");
    let mut more = listing.clone();
    more["tasks"] = json!(
        (30..33)
            .map(|index| {
                let mut task = listing["tasks"][9].clone();
                task["slug"] = json!(format!("feature/build-{index:02}"));
                task
            })
            .collect::<Vec<_>>()
    );
    more["next"] = Value::Null;
    site.set_json(
        "/api/task-board?q=&status=build&days=1&cursor=more-builds&activity=1",
        &more,
    );
    chrome.eval("const more = document.querySelector('[data-status=build] button'); more.scrollIntoView({block:'nearest',inline:'nearest'}); more.click()");
    assert!(chrome.wait_for(
        "document.querySelector('[data-status=build] [id=\"row-feature/build-32\"]') !== null && document.querySelector('[data-status=build] button') === null",
        SHOWN
    ));
    assert_eq!(
        chrome.eval(
            "document.querySelector('[data-status=build] [id=\"row-feature/build-00\"]') !== null"
        ),
        json!(true),
        "Load more replaced the earlier page"
    );
    assert_eq!(
        chrome.eval("document.querySelector('[data-status=build] .count').textContent"),
        json!("33")
    );
    crate::components::choose(&mut chrome, "[aria-label=Status]", "Build");
    assert!(chrome.wait_for("document.querySelectorAll('.pipeline > .column').length === 1 && document.querySelector('[data-status=build] .count').textContent === '33'", SHOWN));
    crate::components::choose(&mut chrome, "[aria-label=Status]", "All statuses");
    assert!(chrome.wait_for(
        "document.querySelectorAll('.pipeline > .column').length === 8",
        SHOWN
    ));
    chrome.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":390,"height":844,"deviceScaleFactor":1,"mobile":true}),
    );
    chrome.call(
        "Emulation.setTouchEmulationEnabled",
        json!({"enabled":true}),
    );
    chrome.eval("document.querySelector('.board-scroll').scrollLeft = 0; document.querySelector('.board-scroll').scrollTop = 0; scrollTo(0,0)");
    ready(&mut chrome);
    screenshot(&mut chrome, "board-mobile-start");
    let y = chrome.eval("Math.min(innerHeight - 80,document.querySelector('.board-scroll').getBoundingClientRect().top + 100)").as_f64().unwrap();
    chrome.eval("window.boardSwipeEndedAt = 0; document.querySelector('.board-scroll').addEventListener('scrollend', event => { window.boardSwipeEndedAt = event.target.scrollLeft; })");
    chrome.call(
        "Input.synthesizeScrollGesture",
        json!({"x":280,"y":y,"xDistance":-240,"yDistance":0,"gestureSourceType":"touch"}),
    );
    assert!(chrome.wait_for(
        "window.boardSwipeEndedAt > 0 && window.boardSwipeEndedAt === document.querySelector('.board-scroll').scrollLeft",
        SHOWN
    ));
    activity(&mut chrome);
    assert_eq!(
        chrome.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
    assert_eq!(
        chrome.eval("document.querySelector('.pipeline .column').getBoundingClientRect().width"),
        json!(280)
    );
    screenshot(&mut chrome, "board-mobile-activity");
    let coverage = chrome.call("CSS.stopRuleUsageTracking", json!({}));
    let rules = coverage["ruleUsage"].as_array().unwrap();
    eprintln!(
        "machine column CSS coverage: {} of {} observed rules used",
        rules.iter().filter(|r| r["used"] == true).count(),
        rules.len()
    );
    assert!(wait_for_feed(&site));
    site.hold_route("/api/board", true);
    site.hold_route("/api/task-board", true);
    site.drop_feeds();
    assert!(chrome.wait_for("document.querySelector('main').dataset.feed === 'stale' && document.querySelector('[data-status=build] .count').textContent === '0'", SHOWN));
    assert_eq!(
        chrome.eval("document.querySelector('[data-status=in_progress] .count').textContent"),
        json!("35")
    );
    reveal(&mut chrome, "in_progress");
    assert!(chrome.wait_for("document.querySelector('[data-status=in_progress] [data-slug=\"feature/build-00\"]') !== null", SHOWN), "fallback column: {}", chrome.eval("JSON.stringify({top:document.querySelector('.board-scroll').scrollTop,left:document.querySelector('.board-scroll').scrollLeft,html:document.querySelector('[data-status=in_progress]').innerHTML.slice(0,3000)})"));
    assert_eq!(
        chrome.eval("document.querySelectorAll('[data-slug=\"feature/build-00\"]').length"),
        json!(1)
    );
    assert!(
        chrome
            .eval(
                "document.querySelector('[data-slug=\"feature/build-00\"] .work-phase').textContent"
            )
            .as_str()
            .unwrap()
            .contains("disconnected")
    );
    screenshot(&mut chrome, "board-mobile-disconnected");
}
