//! The approved waiting board on a phone: one mounted stage, Review first, a saved stage,
//! filters behind their button, and no horizontal overflow. Wide screens show every lifecycle
//! heading together. Assertions follow the controls and layout the reader sees.

use serde_json::{Value, json};

use crate::chrome::Chrome;
use crate::site::Site;
use crate::{SHOWN, chrome, dist, wait_for_feed};

/// The lifecycle columns with as many tasks as `columns` gives each.
fn listing_of(columns: &[(&str, usize)]) -> Value {
    let tasks: Vec<Value> = columns
        .iter()
        .flat_map(|&(status, count)| {
            (1..=count).map(move |n| {
                json!({"slug": format!("phone/{status}-{n}"), "title": format!("The task {status} {n}"),
                    "body": "", "status": status, "priority": 3, "parent": null,
                    "created_by": "owner", "created": 0.0, "updated": 0.0, "links": [],
                    "depends": [], "roles": [], "instruction": false})
            })
        })
        .collect();
    let statuses: Vec<&str> = columns.iter().map(|&(status, _)| status).collect();
    let counts: Vec<Value> = columns
        .iter()
        .map(|&(status, count)| json!([status, count]))
        .collect();
    json!({"tasks": tasks, "statuses": statuses, "counts": counts, "signoff": [], "next": null})
}

/// Two tasks in each open lifecycle column.
fn listing() -> Value {
    listing_of(&[
        ("concept", 2),
        ("design", 2),
        ("ready", 2),
        ("in_progress", 2),
        ("review", 2),
    ])
}

fn site_of(listing: &Value) -> Site {
    let site = Site::start(dist(), &crate::common::fixture("board.json"));
    site.set_json("/api/task-board", listing);
    site
}

fn site() -> Site {
    site_of(&listing())
}

fn viewport(page: &mut Chrome, width: u32, height: u32) {
    page.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":1,"mobile":width <= 760}),
    );
}

/// Open the board, live, on a touch screen of 390 by 844 px.
fn open_on_a_phone(page: &mut Chrome, site: &Site) {
    page.call(
        "Emulation.setTouchEmulationEnabled",
        json!({"enabled":true,"maxTouchPoints":5}),
    );
    viewport(page, 390, 844);
    page.open(&site.url("/tasks"));
    assert!(
        page.wait_for(
            "document.querySelector('main[data-feed=live] .waiting-board .taskcard') !== null && document.documentElement.dataset.navigation === 'ready'",
            SHOWN
        ),
        "the board never drew live: {}",
        page.eval("document.querySelector('main')?.innerText.slice(0, 600) ?? '(no main)'")
    );
    if let Err(stall) = wait_for_feed(site) {
        panic!("{stall}");
    }
}

const MOUNTED: &str = "[...document.querySelectorAll('.waiting-columns .column')].map(column => column.dataset.status)";

fn choose(page: &mut Chrome, stage: &str) {
    page.eval(&format!("[...document.querySelectorAll('.stage-tabs button')].find(button => button.textContent.startsWith({})).click()", json!(stage)));
}

fn fits(page: &mut Chrome) {
    assert_eq!(
        page.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true),
        "the board must not overflow sideways"
    );
    assert_eq!(page.eval("(() => { const r = document.querySelector('.waiting-columns .taskcard').getBoundingClientRect(); return r.top >= 0 && r.bottom <= innerHeight && r.left >= 0 && r.right <= innerWidth; })()"), json!(true), "the first card is completely on screen");
}

#[test]
fn a_phone_opens_review_and_mounts_only_the_stage_chosen_by_its_tab() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    open_on_a_phone(&mut page, &site);
    assert_eq!(page.eval(MOUNTED), json!(["review"]));
    assert_eq!(page.eval("document.querySelector('.stage-tabs [aria-current=page]').textContent.startsWith('Review')"), json!(true));
    fits(&mut page);
    for (label, status) in [
        ("Ready", "ready"),
        ("Concept", "concept"),
        ("Review", "review"),
    ] {
        choose(&mut page, label);
        assert!(page.wait_for(&format!("JSON.stringify({MOUNTED}) === JSON.stringify([{}]) && document.querySelector('.waiting-columns .taskcard') !== null", json!(status)), SHOWN));
        fits(&mut page);
    }
}

#[test]
fn a_phones_chosen_stage_survives_a_full_load() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    open_on_a_phone(&mut page, &site);
    choose(&mut page, "Ready");
    assert!(page.wait_for(
        "document.querySelector('.waiting-columns .column[data-status=ready] .taskcard') !== null",
        SHOWN
    ));
    page.open(&site.url("/tasks"));
    assert!(page.wait_for("document.querySelector('.waiting-columns .column[data-status=ready] .taskcard') !== null && document.documentElement.dataset.navigation === 'ready'", SHOWN));
    assert_eq!(page.eval(MOUNTED), json!(["ready"]));
    fits(&mut page);
}

#[test]
fn all_six_lifecycle_headings_fit_together_on_a_wide_board() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1440, 900);
    page.open(&site.url("/tasks"));
    assert!(page.wait_for(
        "document.querySelector('.waiting-columns .taskcard') !== null",
        SHOWN
    ));
    assert_eq!(
        page.eval(MOUNTED),
        json!([
            "concept",
            "design",
            "ready",
            "in_progress",
            "build-test",
            "review",
            "done"
        ])
    );
    assert_eq!(page.eval("[...document.querySelectorAll('.waiting-columns .column h2')].every(heading => { const r = heading.getBoundingClientRect(); return r.left >= 0 && r.right <= innerWidth && r.top >= 0 && r.bottom <= innerHeight; })"), json!(true));
    assert_eq!(
        page.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
    assert_eq!(page.eval("document.querySelector('.column[data-status=blocked], .signoff-lane, .pipeline, .board-scroll')"), Value::Null);
}

#[test]
fn a_phones_search_and_filters_open_from_their_button_and_a_desk_shows_them() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    open_on_a_phone(&mut page, &site);
    assert_eq!(
        page.eval("document.querySelector('#board-filters').checkVisibility()"),
        json!(false)
    );
    page.tap(".board-filters-toggle");
    assert!(page.wait_for("document.querySelector('#board-filters')?.checkVisibility() === true && document.querySelector('.board-filters-toggle').getAttribute('aria-expanded') === 'true'", SHOWN));
    page.eval("(() => { const input = document.querySelector('#board-filters input[type=search]'); input.value = 'found'; input.dispatchEvent(new Event('input', {bubbles: true})); })()");
    if let Err(stall) =
        test_time::until_true("the search's waiting projection read", crate::STALL, || {
            site.targets().iter().any(|target| {
                target.starts_with("/api/task-board?view=waiting&") && target.contains("q=found")
            }) || {
                page.pump(std::time::Duration::from_millis(25));
                false
            }
        })
    {
        panic!("{stall}: {:#?}", site.targets());
    }
    page.tap(".board-filters-toggle");
    assert!(page.wait_for(
        "document.querySelector('#board-filters')?.checkVisibility() === false",
        SHOWN
    ));
    for (width, height) in [(1440, 900), (800, 1000)] {
        viewport(&mut page, width, height);
        assert!(page.wait_for(&format!("innerWidth === {width} && document.querySelector('#board-filters')?.checkVisibility() === true && document.querySelector('.board-filters-toggle')?.checkVisibility() === false"), SHOWN));
    }
}
