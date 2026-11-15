//! The task board on a phone (`assets/board.css`'s `max-width: 600px`, the React client's
//! `src/board.tsx`), app/phone-board-slides-a-column-at-a-time: one column on the screen at a
//! time, each as wide as the board's scroller, and a swipe lands on the next one whole, a flick
//! too; the search and the filters wait behind a button of their own, shut as the board opens,
//! which a wider window does not draw. Each waits on what the page shows, never on a pause.

use serde_json::{Value, json};

use crate::chrome::Chrome;
use crate::site::Site;
use crate::{SHOWN, chrome, dist, wait_for_feed};

/// Two tasks in each column of the pipeline.
fn listing() -> Value {
    let statuses = ["concept", "design", "ready", "in_progress", "review"];
    let tasks: Vec<Value> = statuses
        .iter()
        .flat_map(|status| {
            (1..=2).map(move |n| {
                json!({"slug": format!("phone/{status}-{n}"), "title": format!("The task {status} {n}"),
                    "body": "", "status": status, "priority": 3, "parent": null,
                    "created_by": "owner", "created": 0.0, "updated": 0.0, "links": [],
                    "depends": [], "roles": [], "instruction": false})
            })
        })
        .collect();
    let counts: Vec<Value> = statuses.iter().map(|status| json!([status, 2])).collect();
    json!({"tasks": tasks, "statuses": statuses, "counts": counts, "signoff": [], "next": null})
}

fn site() -> Site {
    let site = Site::start(dist(), &crate::common::fixture("board.json"));
    site.set_json("/api/task-board", &listing());
    site
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
            "document.querySelector('main[data-feed=live] .board-scroll .taskcard') !== null && document.documentElement.dataset.navigation === 'ready'",
            SHOWN
        ),
        "the board never drew live: {}",
        page.eval("document.querySelector('main')?.innerText.slice(0, 600) ?? '(no main)'")
    );
    if let Err(stall) = wait_for_feed(site) {
        panic!("{stall}");
    }
}

/// Each of the pipeline's columns, the rail too, by its status, as wide as the board's scroller
/// shows inside its borders, or as wide as it is drawn where it is not.
const WIDTHS: &str = "(() => { const board = document.querySelector('.board-scroll');
    return [...document.querySelectorAll('.pipeline > .column')].map(column => {
        const width = column.getBoundingClientRect().width;
        return Math.abs(width - board.clientWidth) < 0.5 ? column.dataset.status : `${column.dataset.status} ${width} of ${board.clientWidth}`; }); })()";

/// A swipe of a finger across the board's scroller, `columns` of its widths to the left (a
/// negative one to the right), from near the edge it leaves, let go still moving when `flick`
/// (its momentum carrying the scroll on); and the column the scroll then ends on, by its place
/// in the pipeline: the one whose left edge stands at the scroller's, once the scroll ends.
fn swipe(page: &mut Chrome, columns: f64, flick: bool) -> Value {
    let from = page.eval(&format!(
        "(() => {{ const board = document.querySelector('.board-scroll'), r = board.getBoundingClientRect();
            window.phoneLanded = undefined;
            board.onscrollend = () => {{ const left = r.left + board.clientLeft;
                const at = [...document.querySelectorAll('.pipeline > .column')].findIndex(column => Math.abs(column.getBoundingClientRect().left - left) < 1);
                window.phoneLanded = at < 0 ? null : at; }};
            return [{} > 0 ? r.right - 8 : r.left + 8, r.top + Math.min(r.height / 2, 120), board.clientWidth]; }})()",
        columns
    ));
    let distance = -columns * from[2].as_f64().unwrap();
    page.call(
        "Input.synthesizeScrollGesture",
        json!({"x": from[0], "y": from[1], "xDistance": distance, "yDistance": 0,
            "gestureSourceType": "touch", "speed": if flick { 4000 } else { 800 },
            "preventFling": !flick}),
    );
    assert!(
        page.wait_for("typeof window.phoneLanded === 'number'", SHOWN),
        "a swipe of {columns} columns never ended on a column: {}",
        page.eval(
            "[document.querySelector('.board-scroll').scrollLeft, window.phoneLanded ?? 'none']"
        )
    );
    page.eval("window.phoneLanded")
}

/// On a phone each column is as wide as the board's scroller and the scroller snaps to each one:
/// a swipe past half a column's width lands on the next one, a short flick whose momentum would
/// carry it over several on the next one all the same, and one back on the one before. Before, a
/// column was 280 px of the phone's 358, a swipe stopped where it ended or nearly, and the next
/// column peeked in beside the last.
#[test]
fn a_swipe_on_a_phone_lands_on_the_next_column_whole() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    open_on_a_phone(&mut page, &site);
    assert_eq!(
        page.eval(WIDTHS),
        json!([
            "concept",
            "design",
            "ready",
            "in_progress",
            "build-test",
            "review"
        ]),
        "each column is as wide as the board's scroller"
    );
    assert_eq!(
        page.eval("document.querySelector('.board-scroll').scrollLeft"),
        json!(0)
    );
    assert_eq!(swipe(&mut page, 0.6, false), json!(1), "to the next column");
    // A short flick, its momentum enough to carry it on past the next column where the sheet let
    // it pass one (`scroll-snap-stop: normal`).
    assert_eq!(
        swipe(&mut page, 0.3, true),
        json!(2),
        "a flick passes no column"
    );
    assert_eq!(
        swipe(&mut page, -0.6, false),
        json!(1),
        "back to the one before"
    );
}

/// Whether the reader sees what `selector` names.
fn shown(page: &mut Chrome, selector: &str) -> bool {
    page.eval(&format!(
        "document.querySelector({})?.checkVisibility() === true",
        json!(selector)
    )) == json!(true)
}

/// On a phone the board opens with its search and its filters shut behind their button; a tap
/// opens them, a search typed there reads the board, and a tap again shuts them. A desk, and a
/// window between a phone's and a desk's, show them as they did, and no button, whether a phone
/// left them open or shut, and their columns as they were.
#[test]
fn a_phones_search_and_filters_open_from_their_button_and_a_desk_shows_them() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    open_on_a_phone(&mut page, &site);
    let toggle = ".board-filters-toggle";
    assert!(shown(&mut page, toggle), "a phone draws the button");
    assert!(!shown(&mut page, "#board-filters"), "the filters open shut");
    assert_eq!(
        page.eval("document.querySelector('.board-filters-toggle').getAttribute('aria-expanded')"),
        json!("false")
    );
    // What stands above the board's scroller: less, with the filters shut.
    let above = page
        .eval("document.querySelector('.board-scroll').getBoundingClientRect().top")
        .as_f64()
        .unwrap();
    page.tap(toggle);
    assert!(
        page.wait_for(
            "document.querySelector('#board-filters')?.checkVisibility() === true && document.querySelector('.board-filters-toggle').getAttribute('aria-expanded') === 'true'",
            SHOWN
        ),
        "a tap never opened the filters"
    );
    assert!(
        page.eval("document.querySelector('.board-scroll').getBoundingClientRect().top")
            .as_f64()
            .unwrap()
            > above,
        "the opened filters push the board down"
    );
    assert!(shown(&mut page, "#board-filters input[type=search]"));
    page.eval(
        "(() => { const input = document.querySelector('#board-filters input[type=search]'); input.value = 'found';
            input.dispatchEvent(new Event('input', {bubbles: true})); })()",
    );
    if let Err(stall) = test_time::until_true("the search's read", crate::STALL, || {
        site.targets()
            .iter()
            .any(|target| target.starts_with("/api/task-board?") && target.contains("q=found"))
            || {
                page.pump(std::time::Duration::from_millis(25));
                false
            }
    }) {
        panic!("{stall}: {:#?}", site.targets());
    }
    page.tap(toggle);
    assert!(
        page.wait_for(
            "document.querySelector('#board-filters')?.checkVisibility() === false && document.querySelector('.board-filters-toggle').getAttribute('aria-expanded') === 'false'",
            SHOWN
        ),
        "a tap again never shut the filters"
    );
    // A desk, and a window too wide for a phone's, show the filters and no button, and their
    // columns side by side.
    for (width, height) in [(1280, 800), (700, 1000)] {
        viewport(&mut page, width, height);
        assert!(
            page.wait_for(
                &format!(
                    "innerWidth === {width} && document.querySelector('#board-filters')?.checkVisibility() === true && document.querySelector('.board-filters-toggle')?.checkVisibility() === false"
                ),
                SHOWN
            ),
            "{width}px shows the filters and no button"
        );
        let columns = page.eval(
            "[...document.querySelectorAll('.pipeline > .column:not(.rail)')].map(column => column.getBoundingClientRect().width)",
        );
        let wide = if width > 760 { 288.0 } else { 280.0 };
        assert!(
            columns
                .as_array()
                .unwrap()
                .iter()
                .all(|column| column.as_f64() == Some(wide)),
            "{width}px: {columns}"
        );
    }
}
