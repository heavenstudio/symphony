//! Where the reader is on the task board (the React client's `src/boardplace.ts`), kept through
//! what moves it (app/board-scroll-and-focus-followups): a card the feed files above the reader,
//! in their column or another, moves nothing on their screen, wherever a touch screen's last tap
//! left the pointer, and deep in a column Load more brought to its end, where the refresh after
//! each filing keeps every row (app/board-keeps-its-place-deep-in-a-loaded-column); a focused card
//! the feed moves to another column keeps the focus, on the same link; and Back from a task draws
//! the board where the reader left it, sideways, down its columns and down the page, the card it
//! kept moved or gone meanwhile or not. Each waits on what the page shows, never on a pause.

use serde_json::{Value, json};

use crate::chrome::Chrome;
use crate::site::Site;
use crate::{SHOWN, chrome, dist, wait_for_feed};

/// The board's version, from which the page follows the feed: the fixture's.
fn version() -> u64 {
    let board: Value = serde_json::from_str(&crate::common::fixture("board.json")).unwrap();
    board["version"].as_u64().unwrap()
}

/// The statuses the board lists: the pipeline's, and Cancelled, a column under its scroller.
const STATUSES: [&str; 6] = [
    "concept",
    "design",
    "ready",
    "in_progress",
    "review",
    "cancelled",
];

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
            "document.querySelector('main[data-feed=live] .board-scroll .taskcard') !== null && document.documentElement.dataset.navigation === 'ready'",
            SHOWN
        ),
        "{width}px: the board never drew live: {}",
        page.eval("document.querySelector('main')?.innerText.slice(0, 600) ?? '(no main)'")
    );
    if let Err(stall) = wait_for_feed(site) {
        panic!("{stall}");
    }
}

/// What the board's scroller shows inside its scrollbars, its place, and what in it still waits
/// to draw (a column's rows, a row's card) within its sentinel's reach (600 px above and below,
/// `.viewport-sentinel`) of what it shows: each one's id and box. A pixel short of the reach,
/// which a sentinel's edge meets or not as the layout rounds.
const WAITING: &str = "(() => {
    const board = document.querySelector('.board-scroll'), box = board.getBoundingClientRect();
    const top = box.top + board.clientTop, left = box.left + board.clientLeft;
    const bottom = top + board.clientHeight, right = left + board.clientWidth;
    const near = [...board.querySelectorAll('[data-pending=true]')].flatMap(part => {
        const r = part.getBoundingClientRect();
        return r.bottom > top - 599 && r.top < bottom + 599 && r.right > left && r.left < right
            ? [[part.closest('[id]')?.id ?? part.closest('.column')?.dataset.status, Math.round(r.top), Math.round(r.bottom), Math.round(r.left), Math.round(r.right)]] : [];
    });
    return {width: innerWidth, shows: [top, bottom, left, right], left: board.scrollLeft, top: board.scrollTop, near};
})()";

/// The board's scroller at `top`, sideways where the column `status` starts (a place it snaps
/// to), once what is near what it shows is drawn (`WAITING`), and stays as it is.
fn scroll_board(page: &mut Chrome, status: &str, top: u32) {
    page.eval(&format!(
        "(() => {{ const board = document.querySelector('.board-scroll'), column = document.querySelector('.column[data-status={status}]');
            board.scrollLeft += column.getBoundingClientRect().left - board.getBoundingClientRect().left;
            board.scrollTop = {top}; }})()"
    ));
    assert!(
        page.wait_for(&format!("{WAITING}.near.length === 0"), SHOWN),
        "what is near the screen never drew: {}",
        page.eval(WAITING)
    );
    // A card the browser draws only near the screen (`content-visibility`, board.css) takes its
    // own height a frame or two after its row came in: the board stands once ten frames in a row
    // move no card on the screen.
    page.eval(STILL);
    assert!(
        page.wait_for("window.boardStill >= 10", SHOWN),
        "the cards on the screen never stood still"
    );
}

/// Count the frames in a row in which no card on the board's scroller moved, in `boardStill`,
/// up to ten.
const STILL: &str = "(() => {
    const cards = () => { const box = document.querySelector('.board-scroll').getBoundingClientRect();
        return JSON.stringify([...document.querySelectorAll('.board-scroll .taskcard')].flatMap(card => {
            const r = card.getBoundingClientRect();
            return r.bottom > box.top && r.top < box.bottom && r.right > box.left && r.left < box.right ? [[card.dataset.slug, r.top, r.left]] : [];
        })); };
    window.boardStill = 0;
    let last = cards();
    const frame = () => { const now = cards(); window.boardStill = now === last ? window.boardStill + 1 : 0; last = now;
        if (window.boardStill < 10) requestAnimationFrame(frame); };
    requestAnimationFrame(frame);
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

/// The tasks of a column's page, as the server pages it (`PAGE` in the server's
/// `src/api/task_board.rs`).
const PAGE: usize = 30;

/// The address of a read of Ready's rows, `extra` naming its cursor or its length.
fn ready_target(extra: &str) -> String {
    format!("/api/task-board?q=&status=ready&days=1{extra}&activity=1&work=0")
}

/// `rows` as a listing's page, counted over all of `board`, its cursor `next`.
fn page_of(rows: &[Value], board: &[Value], next: Option<String>) -> Value {
    let mut page = listing(board);
    page["tasks"] = json!(rows);
    page["next"] = json!(next);
    page
}

/// Serve `board`, all of it in Ready and in its order, as the server pages it: the counts, Ready's
/// first page and each page after it at the cursor the one before named, and every row of it in
/// one page as long as the board asks (`rows`), as a refresh reads a column loaded to its end
/// again.
fn serve_ready(site: &Site, board: &[Value]) {
    site.set_json("/api/task-board", &page_of(&[], board, None));
    for (at, rows) in board.chunks(PAGE).enumerate() {
        let start = at * PAGE;
        let cursor = if at == 0 {
            String::new()
        } else {
            format!("&cursor=p{start}")
        };
        let next = Some(format!("p{}", start + rows.len()));
        site.set_json(&ready_target(&cursor), &page_of(rows, board, next));
    }
    let all = Some(format!("p{}", board.len()));
    site.set_json(
        &ready_target(&format!("&rows={}", board.len())),
        &page_of(board, board, all),
    );
}

/// How many rows Ready draws, whether their cards drew or not.
const READY_ROWS: &str =
    "document.querySelectorAll('.board-scroll .column[data-status=ready] [id^=\"row-\"]').length";

/// Press Ready's Load more.
const LOAD_MORE: &str = "[...document.querySelectorAll('.column[data-status=ready] button')]
    .find(button => button.textContent === 'Load more tasks').click()";

/// A task of Ready with a title that never wraps, so a new one leaves its card's height as it was.
fn short(slug: &str, priority: u8, title: &str) -> Value {
    let mut task = task(slug, "ready", priority, json!([]));
    task["title"] = json!(title);
    task
}

/// A reader deep in a long column that Load more brought to its end stays on the card they read
/// when the feed files a task above them in it, then one below. The refresh after each filing reads
/// all the column's rows again, as many as it shows (the listing's `rows`), never its first page
/// alone, which would drop the rest and the reader's place with them. Each refresh's answer
/// carries a new title for the column's first card, so the test waits until the page has drawn it.
#[test]
fn a_refresh_keeps_every_row_of_a_column_loaded_to_its_end_and_the_reader_on_their_card() {
    let mut board: Vec<Value> = (1..=100)
        .map(|n| short(&format!("deep/ready-{n:03}"), 2, &format!("Ready {n:03}")))
        .collect();
    let site = Site::start(dist(), &crate::common::fixture("board.json"));
    serve_ready(&site, &board);
    let Some(mut page) = chrome() else { return };
    open(&mut page, &site, 1280, 800);
    for shown in [60, 90, 100] {
        page.eval(LOAD_MORE);
        assert!(
            page.wait_for(&format!("{READY_ROWS} === {shown}"), SHOWN),
            "Load more never took Ready to {shown} rows: it draws {}",
            page.eval(READY_ROWS)
        );
    }
    let middle = page.eval(
        "(() => { const board = document.querySelector('.board-scroll');
            return Math.round((board.scrollHeight - board.clientHeight) / 2); })()",
    );
    let middle = u32::try_from(middle.as_u64().expect("the scroller's middle")).unwrap();
    scroll_board(&mut page, "ready", middle);
    let reading = page.eval(&first_on_screen("ready"));
    let row: usize = reading[0]
        .as_str()
        .and_then(|slug| slug.strip_prefix("deep/ready-"))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    assert!(
        row > PAGE + PAGE / 2,
        "the setup puts the reader in Ready's middle, past its first page: {reading}"
    );
    // The pointer over a card of Ready on the screen: Ready is the reader's column.
    let card = page.eval(
        "(() => { const box = document.querySelector('.board-scroll').getBoundingClientRect();
            const card = [...document.querySelectorAll('.column[data-status=ready] .taskcard')].find(card => {
                const r = card.getBoundingClientRect(); return r.top > box.top && r.bottom < box.bottom; });
            const r = card.getBoundingClientRect(); return [r.left + 20, r.top + 20]; })()",
    );
    page.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mouseMoved","x":card[0],"y":card[1]}),
    );
    let mut version = version();
    for (round, (slug, priority)) in [("deep/filed-above", 0), ("deep/filed-below", 3)]
        .into_iter()
        .enumerate()
    {
        let before = page.eval(&first_on_screen("ready"));
        let top = scroll_top(&mut page);
        let filed = short(slug, priority, "Filed");
        if priority == 0 {
            board.insert(0, filed.clone());
        } else {
            board.push(filed.clone());
        }
        let mark = format!("Ready 001 · {}", round + 1);
        for task in &mut board {
            if task["slug"] == "deep/ready-001" {
                task["title"] = json!(mark);
            }
        }
        serve_ready(&site, &board);
        version += 1;
        site.push(
            version,
            &json!({"version":version,"changes":[
                {"region":"task","data":{"key":slug,"value":filed}},
                {"region":"task-counts","data":{}}]})
            .to_string(),
        );
        let read_again = format!(
            "document.querySelector('[data-slug=\"deep/ready-001\"] .taskcard-title')?.textContent === {}",
            json!(mark)
        );
        assert!(
            page.wait_for(&read_again, SHOWN),
            "{slug}: the refresh after it never drew; Ready draws {} rows",
            page.eval(READY_ROWS)
        );
        page.eval(STILL);
        assert!(
            page.wait_for("window.boardStill >= 10", SHOWN),
            "{slug}: the cards on the screen never stood still"
        );
        assert_eq!(
            page.eval(READY_ROWS),
            json!(board.len()),
            "{slug}: the refresh left Ready with only some of its rows"
        );
        assert_eq!(
            page.eval(&filed_and_held(slug, "ready", &before)),
            json!(true),
            "{slug}: Ready's first card on the screen went from {before} to {}",
            page.eval(&first_on_screen("ready"))
        );
        let now = scroll_top(&mut page);
        if priority == 0 {
            assert!(
                now > top + 40.0,
                "{slug} filed above scrolled the board from {top} to {now}"
            );
        } else {
            assert!(
                (now - top).abs() <= 1.0,
                "{slug} filed below scrolled the board from {top} to {now}"
            );
        }
    }
    // Every read the refreshes made of Ready asked for all its rows.
    let refreshes: Vec<String> = site
        .answers("/api/task-board")
        .into_iter()
        .map(|answer| answer.target)
        .filter(|target| target.contains("&rows="))
        .collect();
    assert_eq!(
        refreshes,
        [ready_target("&rows=101"), ready_target("&rows=102")],
        "the refreshes' reads of Ready"
    );
}

/// Whether the pointer is over In progress.
const TAPPED: &str = "document.querySelector('.column[data-status=in_progress]:hover') !== null";

/// On a touch screen the pointer stays where the reader last tapped, not where they are: on a
/// phone, a tap on a card of In progress, a swipe back to Ready, In progress still peeking in
/// beside it under the tap, and a task filed at Ready's top moves nothing of Ready on the screen.
#[test]
fn a_touch_screens_last_tap_leaves_the_readers_column_in_place() {
    let mut board = tasks();
    // In progress as long as Ready, so its cards stand on the screen beside Ready's.
    for n in 3..=30 {
        board.push(task(
            &format!("place/in_progress-{n}"),
            "in_progress",
            3,
            json!([]),
        ));
    }
    let served = site(&board);
    let Some(mut page) = chrome() else { return };
    page.call(
        "Emulation.setTouchEmulationEnabled",
        json!({"enabled":true,"maxTouchPoints":5}),
    );
    open(&mut page, &served, 390, 844);
    assert_eq!(
        page.eval("matchMedia('(hover: none)').matches"),
        json!(true),
        "the setup emulates a touch screen"
    );
    scroll_board(&mut page, "in_progress", 900);
    let at = page.eval(
        "(() => { const box = document.querySelector('.board-scroll').getBoundingClientRect();
            const slug = [...document.querySelectorAll('.column[data-status=in_progress] .taskcard-slug')].find(code => {
                const r = code.getBoundingClientRect();
                return r.top > box.top && r.bottom < box.bottom && r.left > box.left && r.right < box.right; });
            const r = slug.getBoundingClientRect();
            return [r.left + r.width / 2, r.top + r.height / 2]; })()",
    );
    page.call(
        "Input.dispatchTouchEvent",
        json!({"type":"touchStart","touchPoints":[{"x":at[0],"y":at[1]}]}),
    );
    page.call(
        "Input.dispatchTouchEvent",
        json!({"type":"touchEnd","touchPoints":[]}),
    );
    assert!(
        page.wait_for(TAPPED, SHOWN),
        "the tap never left the pointer over In progress"
    );
    scroll_board(&mut page, "ready", 900);
    // In progress peeks in beside Ready, the pointer still over it where the reader tapped.
    let peeks = "(() => { const box = document.querySelector('.board-scroll').getBoundingClientRect();
        const r = document.querySelector('.column[data-status=in_progress]').getBoundingClientRect();
        return r.left < box.right && r.right > box.left; })()";
    assert_eq!(
        page.eval(&format!("{TAPPED} && {peeks}")),
        json!(true),
        "the setup swipes to Ready, In progress beside it under the tap"
    );
    let slug = "place/filed-after-a-tap";
    let before = page.eval(&first_on_screen("ready"));
    let top = scroll_top(&mut page);
    let filed = task(slug, "ready", 0, json!([]));
    board.push(filed.clone());
    push(
        &served,
        version() + 1,
        &board,
        json!([{"region":"task","data":{"key":slug,"value":filed}}]),
    );
    assert!(
        page.wait_for(&filed_and_held(slug, "ready", &before), SHOWN),
        "{slug} filed moved Ready's first card on the screen from {before} to {}",
        page.eval(&first_on_screen("ready"))
    );
    let now = scroll_top(&mut page);
    assert!(
        now > top + 40.0,
        "{slug} filed above in Ready scrolled the board from {top} to {now}"
    );
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

/// Open the task of a Ready card's title on the screen, clicked as the reader clicks it: the
/// pointer over Ready and the focus on the link, so Ready is the reader's column; until its page
/// is drawn.
fn open_a_ready_task(page: &mut Chrome, width: u32) {
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
}

/// Back from a task opened from its card in Ready draws the board where the reader left it: its
/// scroller sideways, Ready's first card on the screen where it stood (the board drawn again
/// with no rows, then each column's as ghosts, then cards, at whatever height), and the window
/// down the page. On a desk the board is scrolled to Design, Ready beside it: Design's two cards
/// end above the screen, where the browser would snap a sideways scroll to another column. On a
/// phone Ready fills its width.
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
        open_a_ready_task(&mut page, width);
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

/// The slug of Ready's first row whose top is on the board's scroller: the row the board's place
/// is kept by when Ready is the reader's column.
const KEPT_ROW: &str = "(() => { const box = document.querySelector('.board-scroll').getBoundingClientRect();
    const row = [...document.querySelectorAll('.board-scroll .column[data-status=ready] [id^=\"row-\"]')].find(row => {
        const top = row.getBoundingClientRect().top; return top >= box.top && top < box.bottom; });
    return row ? row.id.slice(4) : null; })()";

/// How far, in px, the scroller moves once returned to its offset with no row to keep it by, as the
/// rows above the screen draw their cards where ghosts stood a pixel or so off each: the browser's
/// own scroll anchoring takes that up, nothing on the screen moving. A card that left its column
/// took the board hundreds of px away, to where it went.
const REDRAWN_ABOVE: f64 = 12.0;

/// Back from a task to a board whose card its place was kept by left Ready meanwhile, claimed into
/// In progress beside it or cancelled into the column under the board's scroller, draws the board
/// where the reader left its scroller, sideways and down (`REDRAWN_ABOVE`), and the window: never
/// where that card went, once the board has drawn it there and stands still.
#[test]
fn back_to_a_board_whose_kept_card_left_its_column_returns_to_the_scrollers_place() {
    let place = PLACE.replace("FIRST", "Math.round(board.scrollTop)");
    for status in ["in_progress", "cancelled"] {
        let mut board = tasks();
        let site = site(&board);
        let Some(mut page) = chrome() else { return };
        open(&mut page, &site, 1280, 800);
        scroll_board(&mut page, "design", 900);
        page.eval(
            "scrollTo(0, Math.min(120, document.scrollingElement.scrollHeight - innerHeight))",
        );
        let before = page.eval(&place);
        let kept = page.eval(KEPT_ROW);
        let Some(slug) = kept.as_str().map(str::to_owned) else {
            panic!(
                "{status}: no row of Ready on the screen: {}",
                page.eval(WAITING)
            );
        };
        open_a_ready_task(&mut page, 1280);
        for task in &mut board {
            if task["slug"] == slug.as_str() {
                task["status"] = json!(status);
            }
        }
        push(
            &site,
            version() + 1,
            &board,
            json!([{"region":"task-fields","data":{"key":slug,"fields":{"status":status}}}]),
        );
        page.eval("history.back()");
        let there = format!(
            "location.pathname === '/tasks' && document.documentElement.dataset.navigation === 'ready'
                && document.getElementById({})?.closest('.column')?.dataset.status === '{status}'
                && {WAITING}.near.length === 0",
            json!(format!("row-{slug}"))
        );
        assert!(
            page.wait_for(&there, SHOWN),
            "{status}: Back never drew the board with {slug} moved there: {}",
            page.eval(WAITING)
        );
        page.eval(STILL);
        assert!(
            page.wait_for("window.boardStill >= 10", SHOWN),
            "{status}: the cards on the screen never stood still"
        );
        let now = page.eval(&place);
        let down = |at: &Value| at[2].as_f64().unwrap();
        assert!(
            now[0] == before[0]
                && now[1] == before[1]
                && (down(&now) - down(&before)).abs() <= REDRAWN_ABOVE,
            "{status}: Back with {slug} moved drew the board at {now}, where the reader left it at {before}"
        );
    }
}
