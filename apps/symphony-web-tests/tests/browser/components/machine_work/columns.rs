//! Machine activity appears in full waiting board columns, preserving complete counts, paging and
//! disconnected lifecycle fallback.

use crate::{READY, SHOWN, chrome, look_parity, site, wait_for_feed};
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
    chrome.frames();
    let shot = chrome.call("Page.captureScreenshot", json!({"format":"png"}));
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../artifacts/app/board-at-a-glance");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join(format!("{name}.base64")),
        shot["data"].as_str().unwrap(),
    )
    .unwrap();
}

fn ready(chrome: &mut crate::Chrome) {
    assert!(chrome.wait_for(
        &format!("document.querySelector('.waiting-columns .taskcard') !== null && {READY}"),
        SHOWN
    ));
}

pub(super) fn reveal(chrome: &mut crate::Chrome, status: &str) {
    let label = match status {
        "build" => "Build",
        "test" => "Test",
        "in_progress" => "In progress",
        _ => panic!("unsupported activity stage {status}"),
    };
    if chrome.eval("innerWidth <= 760") == json!(true) {
        chrome.eval(&format!("[...document.querySelectorAll('.stage-tabs button')].find(button=>button.textContent.startsWith({})).click()",json!(label)));
    }
    assert!(chrome.wait_for(
        &format!("document.querySelector('.column[data-status={status}]') !== null"),
        SHOWN
    ));
    chrome.eval(&format!(
        "document.querySelector('.column[data-status={status}]').scrollIntoView({{block:'start',inline:'center'}})"
    ));
}

#[test]
fn machine_columns_show_observations_once_a_machine_reports() {
    let site = site();
    let silent = look_parity::responses().remove("/api/task-board").unwrap();
    site.set_json("/api/task-board", &silent);
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport(1440, 900, false);
    chrome.open(&site.url("/tasks"));
    ready(&mut chrome);
    assert_eq!(chrome.eval("document.querySelector('.column[data-status=build] .work-empty').textContent === 'No build observations reported.' && document.querySelector('.column[data-status=test] .work-empty').textContent === 'No test observations reported.'"), json!(true));
    let reporting = listing();
    site.set_json("/api/task-board", &reporting);
    site.set_json("/api/machine-work", &reporting["machine_work"]);
    chrome.open(&site.url("/tasks"));
    ready(&mut chrome);
    assert!(chrome.wait_for(
        "document.querySelector('.column[data-status=build] .count').textContent === '33' && document.querySelector('.column[data-status=test] .count').textContent === '1'",
        SHOWN
    ));
    reveal(&mut chrome, "test");
    assert!(chrome.wait_for(
        "document.querySelector('[data-status=test] [data-slug=\"feature/test-00\"]') !== null",
        SHOWN
    ));
    assert_eq!(
        chrome.eval("document.querySelector('[data-status=test] .count').textContent"),
        json!("1")
    );
    assert_eq!(
        chrome.eval("document.querySelector('button[aria-label=Status]') === null"),
        json!(true)
    );
}

#[test]
fn opened_machine_activity_keeps_complete_counts_paging_and_disconnect_fallback() {
    let site = site();
    let listing = listing();
    site.set_json("/api/task-board", &listing);
    site.set_json("/api/machine-work", &listing["machine_work"]);
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport(1440, 1000, false);
    chrome.open(&site.url("/tasks"));
    ready(&mut chrome);
    reveal(&mut chrome, "build");
    assert!(chrome.wait_for(
        "document.querySelector('[data-status=build] [data-slug=\"feature/build-00\"]') !== null",
        SHOWN
    ));
    assert_eq!(
        chrome.eval("document.querySelector('[data-status=build] .count').textContent"),
        json!("33")
    );
    wait_for_feed(&site).expect("the page never followed the feed");
    let reads = chrome.eval("performance.getEntriesByType('resource').filter(e=>e.name.includes('/api/task-board?view=waiting&counts=1')).length").as_u64().unwrap();
    site.push(
        4190,
        &json!({"version":4190,"changes":[{"region":"task-counts","data":[["in_progress",35]]}]})
            .to_string(),
    );
    assert!(chrome.wait_for(&format!("performance.getEntriesByType('resource').filter(e=>e.name.includes('/api/task-board?view=waiting&counts=1')).length > {reads} && document.querySelector('[data-status=build] .count').textContent === '33'"), SHOWN), "lifecycle feed totals replaced the complete stage totals");
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
        "/api/task-board?view=waiting&q=&status=build&days=1&cursor=more-builds&activity=1&work=0",
        &more,
    );
    chrome.eval("const more = document.querySelector('[data-status=build] button:not(.card-more)'); more.scrollIntoView({block:'nearest'}); more.click()");
    assert!(chrome.wait_for("document.querySelector('[data-status=build] [id=\"row-feature/build-32\"]') !== null && document.querySelector('[data-status=build] button:not(.card-more)') === null", SHOWN));
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
    chrome.viewport(390, 844, true);
    assert!(chrome.wait_for(
        "document.querySelector('.stage-tabs')?.checkVisibility() === true",
        SHOWN
    ));
    reveal(&mut chrome, "test");
    assert!(chrome.wait_for(
        "document.querySelector('[data-status=test] [data-slug=\"feature/test-00\"]') !== null",
        SHOWN
    ));
    assert_eq!(
        chrome.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
    reveal(&mut chrome, "build");
    site.hold_route("/api/board", true);
    site.hold_route("/api/task-board", true);
    site.drop_feeds();
    assert!(chrome.wait_for("document.querySelector('main').dataset.feed === 'stale' && document.querySelector('[data-status=build] .count').textContent === '0'", SHOWN));
    reveal(&mut chrome, "in_progress");
    assert!(chrome.wait_for("document.querySelector('[data-status=in_progress] [data-slug=\"feature/build-00\"]') !== null", SHOWN));
    assert_eq!(
        chrome.eval("document.querySelector('[data-status=in_progress] .count').textContent"),
        json!("35")
    );
    assert_eq!(
        chrome.eval("document.querySelectorAll('[data-slug=\"feature/build-00\"]').length"),
        json!(1)
    );
}
