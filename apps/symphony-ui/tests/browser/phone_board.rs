//! The task board on a phone (`assets/board.css`'s `max-width: 600px`, the React client's
//! `src/board.tsx` and `src/boardswipe.ts`), app/phone-board-slides-a-column-at-a-time: one column
//! on the screen at a time, each as wide as the board's scroller, and a swipe lands on the next
//! one whole, a long or fast flick too, deep down a column as at its top, and a tap begins no
//! swipe that would hold the next scroll, a focus move's, to one column; the search and the
//! filters wait behind a button of their own, shut as the board opens, which a wider window does
//! not draw. Each waits on what the page shows, never on a pause.

use serde_json::{Value, json};

use crate::chrome::Chrome;
use crate::site::Site;
use crate::{SHOWN, chrome, dist, wait_for_feed};

/// The pipeline's columns with as many tasks as `columns` gives each.
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

/// Two tasks in each column of the pipeline.
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

/// Record from now on, in `window.phoneLandings`, the column each scroll of the board's scroller
/// ends on, by its place in the pipeline: the one whose left edge stands at the scroller's.
const LANDINGS: &str = "(() => { const board = document.querySelector('.board-scroll'); window.phoneLandings = [];
    board.onscrollend = () => { const left = board.getBoundingClientRect().left + board.clientLeft;
        const at = [...document.querySelectorAll('.pipeline > .column')].findIndex(column => Math.abs(column.getBoundingClientRect().left - left) < 1);
        window.phoneLandings.push(at < 0 ? null : at); }; })()";

/// Wait until the board's last scroll ended on the column at `column` of the pipeline, after
/// `what`; then the columns each scroll since the last call ended on, forgotten.
fn lands(page: &mut Chrome, column: usize, what: &str) -> Value {
    assert!(
        page.wait_for(&format!("window.phoneLandings.at(-1) === {column}"), SHOWN),
        "{what} never ended on column {column}: the scrolls ended on {}, the board at {}",
        page.eval("JSON.stringify(window.phoneLandings)"),
        page.eval("document.querySelector('.board-scroll').scrollLeft")
    );
    page.eval("window.phoneLandings.splice(0)")
}

/// A swipe of a finger across the board's scroller, `columns` of its widths to the left (a
/// negative one to the right), from near the edge it leaves, at `speed` px/s, let go still moving
/// when `flick` (its momentum carrying the scroll on).
fn swipe(page: &mut Chrome, columns: f64, speed: u32, flick: bool) {
    let from = page.eval(&format!(
        "(() => {{ const board = document.querySelector('.board-scroll'), r = board.getBoundingClientRect();
            return [{columns} > 0 ? r.right - 8 : r.left + 8, r.top + Math.min(r.height / 2, 120), board.clientWidth]; }})()"
    ));
    let distance = -columns * from[2].as_f64().unwrap();
    page.call(
        "Input.synthesizeScrollGesture",
        json!({"x": from[0], "y": from[1], "xDistance": distance, "yDistance": 0,
            "gestureSourceType": "touch", "speed": speed, "preventFling": !flick}),
    );
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
    page.eval(LANDINGS);
    swipe(&mut page, 0.6, 800, false);
    lands(&mut page, 1, "a swipe to the next column");
    // A short flick, its momentum enough to carry it on past the next column where the sheet let
    // it pass one (`scroll-snap-stop: normal`).
    swipe(&mut page, 0.3, 4000, true);
    lands(&mut page, 2, "a short flick");
    swipe(&mut page, -0.6, 800, false);
    lands(&mut page, 1, "a swipe back");
}

/// A long or fast flick, whose momentum carried Chrome past the next column despite
/// `scroll-snap-stop: always` (0.7 of a column at 6000 px/s, 0.9 at 3000 and at 6000), ends on
/// the next column (`src/boardswipe.ts`), forward and back. Each line printed names the columns
/// its scrolls ended on, the first where the momentum left it.
#[test]
fn a_long_or_fast_flick_on_a_phone_passes_one_column_at_most() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    open_on_a_phone(&mut page, &site);
    page.eval(LANDINGS);
    for (columns, speed, column) in [
        (0.9, 6000, 1),
        (0.7, 6000, 2),
        (0.9, 3000, 3),
        (-0.9, 6000, 2),
        (-0.95, 6000, 1),
    ] {
        swipe(&mut page, columns, speed, true);
        let ended = lands(
            &mut page,
            column,
            &format!("a flick of {columns} at {speed} px/s"),
        );
        eprintln!("PHONE FLICK {columns} of a column at {speed} px/s ended on {ended}");
    }
}

/// The frames a board that stays is watched for.
const FRAMES: u32 = 30;

/// Count in `window.phoneFrames` the frames drawn from now on, up to `FRAMES`, and in
/// `window.phoneScrolls` the board's scroll events among them.
fn still() -> String {
    format!(
        "(() => {{ const board = document.querySelector('.board-scroll'); window.phoneFrames = 0; window.phoneScrolls = 0;
            board.onscroll = () => {{ window.phoneScrolls += 1; }};
            const frame = () => {{ window.phoneFrames += 1; if (window.phoneFrames < {FRAMES}) requestAnimationFrame(frame); }};
            requestAnimationFrame(frame); }})()"
    )
}

/// A tap on the board begins no swipe, so the scroll that ends next, a focus move's here, lands
/// where it lands. Before, a tap left its press noted (`src/boardswipe.ts` forgot it only at the
/// end of a scroll, which a tap never starts), and a focus moved two columns on, as Tab or a
/// screen reader moves it from In progress past the empty build and test rail to Review, was
/// pulled back one column, the focused card off the screen.
#[test]
fn a_tap_then_a_focus_move_two_columns_on_stays_there() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    open_on_a_phone(&mut page, &site);
    page.eval(LANDINGS);
    // Ready's cards drawn, as a column's are once it came on the screen, and back to Concept.
    reveal(&mut page, "ready", 2, 0);
    let link = ".column[data-status=ready] .taskcard-title";
    assert!(
        page.wait_for(
            &format!("document.querySelector({}) !== null", json!(link)),
            SHOWN
        ),
        "Ready's cards never drew"
    );
    reveal(&mut page, "concept", 0, 0);
    page.tap(".column[data-status=concept] > h2");
    page.eval(&format!("document.querySelector({}).focus()", json!(link)));
    lands(&mut page, 2, "the focus move to Ready");
    // A scroll the board asks for as that one ends starts within a frame or two of it.
    page.eval(&still());
    assert!(
        page.wait_for(&format!("window.phoneFrames >= {FRAMES}"), SHOWN),
        "the page drew no {FRAMES} frames: {}",
        page.eval("window.phoneFrames")
    );
    assert_eq!(
        page.eval(&format!(
            "(() => {{ const board = document.querySelector('.board-scroll').getBoundingClientRect(), focused = document.activeElement;
                const card = focused.getBoundingClientRect();
                return [window.phoneScrolls, window.phoneLandings, focused.matches({}), card.left >= board.left && card.right <= board.right]; }})()",
            json!(link)
        )),
        json!([0, [], true, true]),
        "after a tap the board stays on Ready, the focused card on its screen (scrolls, landings since, the card focused, on the screen)"
    );
}

/// Bring the column of `status`, the pipeline's `column`th, onto the board's screen, `top` px down
/// the board, once its scroll ends where it moved at all.
fn reveal(page: &mut Chrome, status: &str, column: usize, top: u32) {
    let moved = page.eval(&format!(
        "(() => {{ const board = document.querySelector('.board-scroll'), column = document.querySelector('.column[data-status={status}]');
            const before = [board.scrollLeft, board.scrollTop];
            board.scrollLeft += column.getBoundingClientRect().left - board.getBoundingClientRect().left - board.clientLeft;
            board.scrollTop = {top};
            return board.scrollLeft !== before[0] || board.scrollTop !== before[1]; }})()"
    ));
    if moved == json!(true) {
        lands(page, column, &format!("the scroll to {status}"));
    }
    assert!(
        page.wait_for(&deep(top), SHOWN),
        "the board never stood {top} px down: {}",
        page.eval("document.querySelector('.board-scroll').scrollTop")
    );
}

/// Whether the board stands `top` px down, to the few pixels the browser's scroll anchoring moves
/// it by as rows above the screen draw a little off what they reserved.
fn deep(top: u32) -> String {
    format!("Math.abs(document.querySelector('.board-scroll').scrollTop - {top}) <= 20")
}

/// A key pressed on the focused board.
fn key(page: &mut Chrome, name: &str, code: u32) {
    for kind in ["keyDown", "keyUp"] {
        page.call(
            "Input.dispatchKeyEvent",
            json!({"type": kind, "key": name, "code": name, "windowsVirtualKeyCode": code}),
        );
    }
}

/// A reader scrolled down a long column swipes to the next one, short or empty, and back, and
/// steps there by key: each column stands as tall as the longest (`align-items: stretch`), as
/// Chrome snaps only to a column that reaches as far down as the board shows. Before, 400 px down
/// Ready a swipe to the empty Design or past In progress to the short rail snapped back, and
/// ArrowRight from In progress passed the rail.
#[test]
fn a_swipe_deep_down_a_column_lands_on_the_next_one_short_or_empty() {
    let site = site_of(&listing_of(&[
        ("concept", 3),
        ("design", 0),
        ("ready", 12),
        ("in_progress", 30),
        ("review", 5),
    ]));
    let Some(mut page) = chrome() else { return };
    open_on_a_phone(&mut page, &site);
    page.eval(LANDINGS);
    reveal(&mut page, "ready", 2, 400);
    for (columns, column, what) in [
        (-0.6, 1, "a swipe to the empty Design"),
        (0.6, 2, "a swipe back to Ready"),
        (0.6, 3, "a swipe to In progress"),
        (0.6, 4, "a swipe to the rail"),
        (-0.6, 3, "a swipe back to In progress"),
    ] {
        swipe(&mut page, columns, 800, false);
        lands(&mut page, column, &format!("{what}, 400 px down"));
    }
    assert!(
        page.wait_for(&deep(400), SHOWN),
        "the swipes kept the reader's place down the columns: {}",
        page.eval("document.querySelector('.board-scroll').scrollTop")
    );
    page.eval("document.querySelector('.board-scroll').focus()");
    for top in [400, 900] {
        reveal(&mut page, "in_progress", 3, top);
        key(&mut page, "ArrowRight", 39);
        lands(
            &mut page,
            4,
            &format!("ArrowRight from In progress {top} px down"),
        );
        key(&mut page, "ArrowLeft", 37);
        lands(
            &mut page,
            3,
            &format!("ArrowLeft from the rail {top} px down"),
        );
    }
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
