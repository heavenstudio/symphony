//! Where the reader is on the task board (the React client's `src/boardplace.ts`), kept through
//! what moves it (app/board-scroll-and-focus-followups): a card the feed files above the reader,
//! in their column or another, moves nothing on their screen; a focused card the feed moves to
//! another column keeps the focus, on the same link; Back from a task draws the board where the
//! reader left it, sideways, down its columns and down the page; and a row still waiting for its
//! card skips its ghost's style while it is off the screen. Each waits on what the page shows,
//! never on a pause.

use serde_json::{Value, json};

use crate::chrome::Chrome;
use crate::site::Site;
use crate::{SHOWN, chrome, dist, wait_for_feed};

/// The board's version, from which the page follows the feed: the fixture's.
fn version() -> u64 {
    let board: Value = serde_json::from_str(&crate::common::fixture("board.json")).unwrap();
    board["version"].as_u64().unwrap()
}

/// The pipeline's statuses the board lists.
const STATUSES: [&str; 5] = ["concept", "design", "ready", "in_progress", "review"];

fn task(slug: &str, status: &str, priority: u8, links: Value) -> Value {
    json!({"slug": slug, "title": format!("The task {slug}"), "body": "", "status": status,
        "priority": priority, "parent": null, "created_by": "owner", "created": 0.0,
        "updated": 0.0, "links": links, "depends": [], "roles": [], "instruction": false})
}

/// Thirty tasks in Concept and in Ready, so both scroll down past the screen, and two in each
/// other column, so the board scrolls sideways too. The fourth ready one links a document: its
/// card has a second link.
fn tasks() -> Vec<Value> {
    let mut tasks = Vec::new();
    for n in 1..=30 {
        tasks.push(task(
            &format!("place/concept-{n:02}"),
            "concept",
            3,
            json!([]),
        ));
        let links = if n == 4 {
            json!(["docs/TASKS.md"])
        } else {
            json!([])
        };
        tasks.push(task(&format!("place/ready-{n:02}"), "ready", 3, links));
    }
    for status in ["design", "in_progress", "review"] {
        for n in 1..=2 {
            tasks.push(task(&format!("place/{status}-{n}"), status, 3, json!([])));
        }
    }
    tasks
}

/// The board's answer for `tasks`, counted by status.
fn listing(tasks: &[Value]) -> Value {
    let counts: Vec<Value> = STATUSES
        .iter()
        .map(|status| {
            json!([
                status,
                tasks
                    .iter()
                    .filter(|task| task["status"] == *status)
                    .count()
            ])
        })
        .collect();
    json!({"tasks": tasks, "statuses": STATUSES, "counts": counts, "signoff": [], "next": null})
}

fn site(tasks: &[Value]) -> Site {
    let site = Site::start(dist(), &crate::common::fixture("board.json"));
    site.set_json("/api/task-board", &listing(tasks));
    site
}

fn viewport(page: &mut Chrome, width: u32, height: u32) {
    page.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":1,"mobile":width < 600}),
    );
}

/// One feed event of `changes`, the board's answer moved to `tasks` first, as a server's is.
fn push(site: &Site, version: u64, tasks: &[Value], changes: Value) {
    site.set_json("/api/task-board", &listing(tasks));
    site.push(
        version,
        &json!({"version":version,"changes":changes}).to_string(),
    );
}

/// Open the board, live and following the feed, at `width` by `height`.
fn open(page: &mut Chrome, site: &Site, width: u32, height: u32) {
    viewport(page, width, height);
    page.open(&site.url("/tasks"));
    assert!(
        page.wait_for(
            "document.querySelector('main[data-feed=live] .board-scroll .column[data-status=ready] .taskcard') !== null && document.documentElement.dataset.navigation === 'ready'",
            SHOWN
        ),
        "{width}px: the board never drew live: {}",
        page.eval("document.querySelector('main')?.innerText.slice(0, 600) ?? '(no main)'")
    );
    if let Err(stall) = wait_for_feed(site) {
        panic!("{stall}");
    }
}

/// True once no row within its sentinel's reach of what the board's scroller shows (600 px above
/// and below) still waits for its card: what is near the screen is drawn, and stays as it is.
const SETTLED: &str = "(() => {
    const box = document.querySelector('.board-scroll').getBoundingClientRect();
    return ![...document.querySelectorAll('.board-scroll .board-row [data-pending=true]')].some(row => {
        const r = row.getBoundingClientRect();
        return r.bottom > box.top - 600 && r.top < box.bottom + 600 && r.right > box.left && r.left < box.right;
    });
})()";

/// The board's scroller at `top`, sideways where the column `status` starts (a place it snaps
/// to), once its rows near the screen are drawn.
fn scroll_board(page: &mut Chrome, status: &str, top: u32) {
    page.eval(&format!(
        "(() => {{ const board = document.querySelector('.board-scroll'), column = document.querySelector('.column[data-status={status}]');
            board.scrollLeft += column.getBoundingClientRect().left - board.getBoundingClientRect().left;
            board.scrollTop = {top}; }})()"
    ));
    assert!(
        page.wait_for(SETTLED, SHOWN),
        "the rows near the screen never drew: {}",
        page.eval(WAITING)
    );
}

/// The board's scroller's box, its place, and each row near it still waiting: its id and box.
const WAITING: &str = "(() => {
    const board = document.querySelector('.board-scroll'), box = board.getBoundingClientRect();
    const near = [...document.querySelectorAll('.board-scroll .board-row [data-pending=true]')].flatMap(row => {
        const r = row.getBoundingClientRect();
        return r.bottom > box.top - 600 && r.top < box.bottom + 600 && r.right > box.left && r.left < box.right
            ? [[row.closest('.board-row').id, Math.round(r.top), Math.round(r.bottom), Math.round(r.left), Math.round(r.right)]] : [];
    });
    return {width: innerWidth, box: [box.top, box.bottom, box.left, box.right], left: board.scrollLeft, top: board.scrollTop, near};
})()";

/// The first card of the column `status` whose top is on the board's scroller: its slug and how
/// far below the scroller's top it stands.
fn first_on_screen(status: &str) -> String {
    format!(
        "(() => {{ const box = document.querySelector('.board-scroll')?.getBoundingClientRect();
            const card = box && [...document.querySelectorAll('.column[data-status={status}] .taskcard')].find(card => {{
                const top = card.getBoundingClientRect().top; return top >= box.top && top < box.bottom; }});
            return card ? [card.dataset.slug, card.getBoundingClientRect().top - box.top] : null; }})()"
    )
}

/// True once the row of `slug` is on the page and the column `status`'s first card on the screen
/// is `place`'s card, within a pixel of `place`'s top.
fn filed_and_held(slug: &str, status: &str, place: &Value) -> String {
    format!(
        "(() => {{ const now = {}, was = {place}; return document.getElementById({}) !== null && now !== null
            && now[0] === was[0] && Math.abs(now[1] - was[1]) <= 1; }})()",
        first_on_screen(status),
        json!(format!("row-{slug}"))
    )
}

fn scroll_top(page: &mut Chrome) -> f64 {
    page.eval("document.querySelector('.board-scroll').scrollTop")
        .as_f64()
        .unwrap()
}

/// A task filed above the reader keeps their column where it was on their screen: on a desk with
/// the pointer over Ready, one filed at Ready's top, then one at the top of Concept, the column
/// before it, which the browser's own scroll anchoring holds; on a phone, where Ready fills the
/// board's width, one at its top.
#[test]
fn a_task_filed_above_the_reader_moves_nothing_in_their_column() {
    let desk: &[(&str, &str)] = &[
        ("place/filed-ready", "ready"),
        ("place/filed-concept", "concept"),
    ];
    let phone: &[(&str, &str)] = &[("place/filed-phone", "ready")];
    for (width, height, first, filings) in
        [(1280, 800, "concept", desk), (390, 844, "ready", phone)]
    {
        let mut board = tasks();
        let served = site(&board);
        let Some(mut page) = chrome() else { return };
        let mut version = version();
        open(&mut page, &served, width, height);
        scroll_board(&mut page, first, 1500);
        if width >= 600 {
            let card = page.eval(&format!(
                "(() => {{ const r = document.getElementById({}).getBoundingClientRect(); return [r.left + 20, r.top + 20]; }})()",
                json!("row-place/ready-20")
            ));
            page.call(
                "Input.dispatchMouseEvent",
                json!({"type":"mouseMoved","x":card[0],"y":card[1]}),
            );
        }
        for (slug, status) in filings {
            let before = page.eval(&first_on_screen("ready"));
            let top = scroll_top(&mut page);
            let filed = task(slug, status, 0, json!([]));
            board.push(filed.clone());
            version += 1;
            push(
                &served,
                version,
                &board,
                json!([{"region":"task","data":{"key":slug,"value":filed}}]),
            );
            assert!(
                page.wait_for(&filed_and_held(slug, "ready", &before), SHOWN),
                "{width}px: {slug} filed moved Ready's first card on the screen from {before} to {}",
                page.eval(&first_on_screen("ready"))
            );
            // The board scrolled by the row filed above the reader in their column, and by none for
            // one filed in another, though the browser's own anchoring held that one's column: the
            // hold of the reader's took the board back.
            let now = scroll_top(&mut page);
            if *status == "ready" {
                assert!(
                    now > top + 40.0,
                    "{width}px: {slug} filed above in Ready scrolled the board from {top} to {now}"
                );
            } else {
                assert!(
                    (now - top).abs() <= 1.0,
                    "{width}px: {slug} filed in {status} scrolled the board from {top} to {now}"
                );
            }
        }
    }
}

/// What holds the focus: the slug of the board's card it is in, the column it stands in and the
/// link's own class, or the element's tag outside every card.
const FOCUSED: &str = "(() => { const active = document.activeElement, card = active?.closest('[data-slug]');
    return card ? [card.dataset.slug, card.closest('.column')?.dataset.status, active.className] : [active?.tagName ?? null]; })()";

/// The reader's focus on a card's link stays on that link of the card when the feed moves the task
/// to another column: on a desk its title, then its document chip, its second link; on a phone,
/// whose In progress column beside Ready had not drawn its rows, its title, the board not scrolled
/// to it.
#[test]
fn a_focused_card_moved_to_another_column_keeps_the_focus() {
    for (width, height) in [(1280, 800), (390, 844)] {
        let mut board = tasks();
        let site = site(&board);
        let Some(mut page) = chrome() else { return };
        open(&mut page, &site, width, height);
        scroll_board(&mut page, "ready", 0);
        let mut version = version();
        let moves: &[(&str, &str)] = if width < 600 {
            &[("place/ready-03", "a.taskcard-title")]
        } else {
            &[
                ("place/ready-03", "a.taskcard-title"),
                ("place/ready-04", "a.docchip"),
            ]
        };
        for (slug, link) in moves {
            page.eval(&format!(
                "document.querySelector({}).focus()",
                json!(format!("[data-slug=\"{slug}\"] {link}"))
            ));
            let class = link.trim_start_matches("a.");
            assert_eq!(
                page.eval(FOCUSED),
                json!([slug, "ready", class]),
                "{width}px: the setup focuses {slug}'s {link} in Ready"
            );
            let left = page.eval("document.querySelector('.board-scroll').scrollLeft");
            for task in &mut board {
                if task["slug"] == *slug {
                    task["status"] = json!("in_progress");
                }
            }
            version += 1;
            push(
                &site,
                version,
                &board,
                json!([{"region":"task-fields","data":{"key":slug,"fields":{"status":"in_progress"}}}]),
            );
            assert!(
                page.wait_for(
                    &format!(
                        "JSON.stringify({FOCUSED}) === {}",
                        json!(json!([slug, "in_progress", class]).to_string())
                    ),
                    SHOWN
                ),
                "{width}px: {slug} moved to In progress and the focus is on {}",
                page.eval(FOCUSED)
            );
            assert_eq!(
                page.eval("document.querySelector('.board-scroll').scrollLeft"),
                left,
                "{width}px: the focus followed {slug} without scrolling the board to it"
            );
        }
    }
}

/// Where the reader is on the board: its scroller's left, the window's top and Ready's first card
/// on the screen (`first_on_screen`).
const PLACE: &str = "(() => { const board = document.querySelector('.board-scroll');
    return board ? [Math.round(board.scrollLeft), Math.round(scrollY), FIRST] : null; })()";

/// Back from a task opened from its card in Ready draws the board where the reader left it: its
/// scroller sideways, Ready's first card on the screen where it stood (the rows above it drawn
/// again, as ghosts or cards, at whatever height), and the window down the page; on a desk, the
/// board scrolled to Design, Ready beside it, and on a phone, where Ready fills its width.
#[test]
fn back_from_a_task_draws_the_board_where_the_reader_left_it() {
    let place = PLACE.replace("FIRST", &first_on_screen("ready"));
    for (width, height, first) in [(1280, 800, "design"), (390, 844, "ready")] {
        let site = site(&tasks());
        let Some(mut page) = chrome() else { return };
        open(&mut page, &site, width, height);
        scroll_board(&mut page, first, 900);
        page.eval(
            "scrollTo(0, Math.min(120, document.scrollingElement.scrollHeight - innerHeight))",
        );
        let before = page.eval(&place);
        assert!(
            before[0].as_f64().unwrap() > 0.0 && before[2].is_array(),
            "{width}px: the setup scrolls the board sideways and down Ready: {before}"
        );
        // A Ready card's title on the screen, clicked as the reader clicks it: the pointer over
        // Ready and the focus on the link, so Ready is the reader's column.
        let link = page.eval(
            "(() => { const box = document.querySelector('.board-scroll').getBoundingClientRect();
                const link = [...document.querySelectorAll('.board-scroll .column[data-status=ready] .taskcard-title')].find(a => {
                    const r = a.getBoundingClientRect();
                    return r.top > box.top && r.bottom < box.bottom && r.left > box.left && r.right < box.right; });
                const r = link.getBoundingClientRect();
                return [link.getAttribute('href'), r.left + Math.min(20, r.width / 2), r.top + r.height / 2]; })()",
        );
        let (x, y) = (&link[1], &link[2]);
        page.call(
            "Input.dispatchMouseEvent",
            json!({"type":"mouseMoved","x":x,"y":y}),
        );
        for kind in ["mousePressed", "mouseReleased"] {
            page.call(
                "Input.dispatchMouseEvent",
                json!({"type":kind,"x":x,"y":y,"button":"left","clickCount":1}),
            );
        }
        let opened = &link[0];
        assert!(
            page.wait_for(
                &format!("location.pathname === {opened} && document.documentElement.dataset.navigation === 'ready'"),
                SHOWN
            ),
            "{width}px: {opened} never opened"
        );
        page.eval("history.back()");
        let back = format!(
            "(() => {{ const now = {place}, was = {before}; return location.pathname === '/tasks'
                && document.documentElement.dataset.navigation === 'ready' && now !== null
                && now[0] === was[0] && now[1] === was[1] && now[2] !== null && now[2][0] === was[2][0]
                && Math.abs(now[2][1] - was[2][1]) <= 1; }})()"
        );
        assert!(
            page.wait_for(&back, SHOWN),
            "{width}px: Back drew the board at {} where the reader left it at {before}",
            page.eval(&place)
        );
    }
}

/// A row still waiting for its card, off the screen, skips its ghost's style and paint (a column
/// loaded to its end holds a thousand, each bar's shimmer restyled every frame), at the height it
/// holds for its card; one near the screen draws its card.
#[test]
fn a_row_waiting_off_the_screen_skips_its_ghost() {
    let site = site(&tasks());
    let Some(mut page) = chrome() else { return };
    page.call(
        "Emulation.setEmulatedMedia",
        json!({"features":[{"name":"prefers-reduced-motion","value":"no-preference"}]}),
    );
    open(&mut page, &site, 1280, 800);
    let last = json!("row-place/ready-30");
    assert!(
        page.wait_for(
            &format!("(() => {{ const ghost = document.getElementById({last})?.querySelector('.viewport-placeholder > .ghost-card');
                return ghost !== null && ghost !== undefined && !ghost.checkVisibility({{contentVisibilityAuto: true}}); }})()"),
            SHOWN
        ),
        "the last ready row's ghost is drawn though off the screen: {}",
        page.eval(&format!("document.getElementById({last})?.outerHTML.slice(0, 400)"))
    );
    assert_eq!(
        page.eval(&format!(
            "(() => {{ const row = document.getElementById({last}), ghost = row.querySelector('.viewport-placeholder');
                return [getComputedStyle(ghost).contentVisibility, Math.round(row.getBoundingClientRect().height) === parseInt(ghost.style.minHeight)]; }})()"
        )),
        json!(["auto", true]),
        "the waiting row is as tall as it holds for its card"
    );
    page.eval(&format!(
        "(board => board.scrollTop += document.getElementById({last}).getBoundingClientRect().top - board.getBoundingClientRect().top)(document.querySelector('.board-scroll'))"
    ));
    assert!(
        page.wait_for(
            &format!("document.getElementById({last}).querySelector('.taskcard') !== null"),
            SHOWN
        ),
        "the last ready row never drew its card near the screen"
    );
}
