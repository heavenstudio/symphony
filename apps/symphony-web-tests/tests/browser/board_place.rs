//! Where the reader is on the task board (the React client's `src/boardplace.ts`), kept through
//! what moves it (app/board-scroll-and-focus-followups): a card the feed files above the reader,
//! in their column or another, moves nothing on their screen, however late the page's tasks run,
//! wherever a touch screen's last tap left the pointer, and deep in a column Load more brought to
//! its end, where the refresh after each filing keeps every row
//! (app/board-keeps-its-place-deep-in-a-loaded-column), and one that changed nothing there
//! transfers no body
//! (app/deep-column-refresh-answers-304-when-unchanged); a focused card the feed moves to another
//! column keeps the focus, on the same link; and Back from a task draws the board where the reader
//! left it down the page, the card it kept moved or gone meanwhile
//! or not. Each waits on what the page shows or the answers the stand-in sent, never on a pause.

use serde_json::{Value, json};

use crate::chrome::Chrome;
use crate::site::Site;
use crate::{LATE_TASKS, LATE_TASKS_TAKEN, READY, SHOWN, STALL, chrome, wait_for_feed};

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
        "updated": if priority == 0 { -1.0 } else if slug == "deep/filed-below" { 1.0 } else { 0.0 }, "links": links, "depends": [], "roles": [], "instruction": false})
}

/// Thirty tasks in Concept and Ready, so both extend beyond the window, and two in each other
/// column. Document links remain in the fixture, but the waiting cards draw only their title link.
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
    let site = crate::site();
    site.set_json("/api/task-board", &listing(tasks));
    site
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
    page.viewport(width, height, width <= 760);
    page.open(&site.url("/tasks"));
    assert!(
        page.wait_for(
            &format!("document.querySelector('main[data-feed=live] .waiting-columns .taskcard') !== null && {READY}"),
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
    const board = document.querySelector('.waiting-columns');
    const box = board.getBoundingClientRect();
    const top = 0, left = Math.max(0, box.left), bottom = innerHeight, right = Math.min(innerWidth, box.right);
    const near = [...board.querySelectorAll('[data-pending=true]')].flatMap(part => {
        const r = part.getBoundingClientRect();
        return r.bottom > r.top && r.bottom > top - 599 && r.top < bottom + 599 && r.right > left && r.left < right
            ? [[part.closest('[id]')?.id ?? part.closest('.column')?.dataset.status, Math.round(r.top), Math.round(r.bottom), Math.round(r.left), Math.round(r.right)]] : [];
    });
    return {width: innerWidth, shows: [top, bottom, left, right], left: scrollX, top: scrollY, near};
})()";

/// The board's scroller at `top`, sideways where the column `status` starts (a place it snaps
/// to), once what is near what it shows is drawn (`WAITING`), and stays as it is.
fn scroll_board(page: &mut Chrome, status: &str, top: u32) {
    page.eval(&format!(
        "(() => {{ if (innerWidth <= 760) {{ const stages = ['review','in_progress','ready','done','design','concept','backlog']; const tab=document.querySelectorAll('.stage-tabs button')[stages.indexOf('{status}')]; tab.focus({{preventScroll:true}}); tab.click(); }} else {{ const board = document.querySelector('.waiting-columns'), column = board.querySelector('.column[data-status={status}]'); board.scrollLeft += column.getBoundingClientRect().left - board.getBoundingClientRect().left; }} }})()"
    ));
    assert!(page.wait_for(
        &format!("document.querySelector('.column[data-status={status}]') !== null"),
        SHOWN
    ));
    page.eval(&format!("scrollTo(0, {top})"));
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
    const cards = () => { const box = ({top:0,left:0,bottom:innerHeight,right:innerWidth});
        return JSON.stringify([...document.querySelectorAll('.waiting-columns .taskcard')].flatMap(card => {
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
        "(() => {{ const box = ({{top:0,left:0,bottom:innerHeight,right:innerWidth}});
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
    page.eval("scrollY").as_f64().unwrap()
}

/// A task filed above the reader keeps their column where it was on their screen: on a desk with
/// the pointer over Ready, one filed at Ready's top, then one at the top of Concept, the column
/// before it, which the browser's own scroll anchoring holds; on a phone, where Ready fills the
/// board's width, one at its top.
#[test]
fn a_task_filed_above_the_reader_moves_nothing_in_their_column() {
    filed_above_the_reader(0);
}

/// The same while every task React's scheduler posts runs twelve frames late ([`LATE_TASKS`]), as
/// on a slow phone or a loaded machine: the board draws the filing within the event that brings
/// it, so the shell's hold of the reader's place, which ends after four frames with nothing moved,
/// starts from the filed row in the page, never from before it.
#[test]
fn a_task_filed_above_the_reader_moves_nothing_while_the_pages_tasks_run_late() {
    filed_above_the_reader(12);
}

/// [`a_task_filed_above_the_reader_moves_nothing_in_their_column`], with React's scheduler's tasks
/// running `late` frames late from each filing until the reader's place is seen held.
fn filed_above_the_reader(late: u32) {
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
        page.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": LATE_TASKS }),
        );
        let mut version = version();
        open(&mut page, &served, width, height);
        if late > 0 {
            assert_eq!(
                page.eval(LATE_TASKS_TAKEN),
                json!(true),
                "{width}px: React's scheduler takes no task through the page script's channel, so none would run late"
            );
        }
        scroll_board(&mut page, first, 1500);
        if width > 760 {
            let card = page.eval(&format!(
                "(() => {{ const r = document.getElementById({}).getBoundingClientRect(); return [r.left + 20, r.top + 20]; }})()",
                json!("row-place/ready-14")
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
            page.eval(&format!("window.__lateTasks = {late}"));
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
            page.eval("window.__lateTasks = 0");
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
    format!("/api/task-board?view=waiting&q=&status=ready&days=1{extra}&activity=1&work=0")
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
const READY_ROWS: &str = "document.querySelectorAll('.waiting-columns .column[data-status=ready] [id^=\"row-\"]').length";

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
    let site = crate::site();
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
        "(() => { const board = document.querySelector('.waiting-columns');
            return Math.round((document.scrollingElement.scrollHeight - innerHeight) / 2); })()",
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
        "(() => { const box = ({top:0,left:0,bottom:innerHeight,right:innerWidth});
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

/// Wait until `done`, the page let run meanwhile; a stall past the backstop fails naming `what`.
fn until(page: &mut Chrome, site: &Site, what: &str, mut done: impl FnMut(&Site) -> bool) {
    if let Err(stall) = test_time::until_true(what, STALL, || {
        done(site) || {
            page.pump(std::time::Duration::from_millis(25));
            false
        }
    }) {
        panic!("{stall}: {:#?}", site.answers("/api/task-board"));
    }
}

/// Each read of `target` Chrome saw finish since the page opened, in order: its status and the
/// bytes of body it received (`Network.dataReceived`, decoded), none for an answer with no body.
fn received(page: &mut Chrome, target: &str) -> Vec<(i64, f64)> {
    let sent = page.wait_for_protocol_events("Network.requestWillBeSent", 0, SHOWN);
    let heads = page.wait_for_protocol_events("Network.responseReceived", 0, SHOWN);
    let data = page.wait_for_protocol_events("Network.dataReceived", 0, SHOWN);
    let finished = page.wait_for_protocol_events("Network.loadingFinished", 0, SHOWN);
    sent.iter()
        .filter(|event| {
            event["request"]["url"]
                .as_str()
                .is_some_and(|url| url.ends_with(target))
        })
        .map(|event| &event["requestId"])
        .filter(|id| finished.iter().any(|done| done["requestId"] == **id))
        .map(|id| {
            let status = heads
                .iter()
                .find(|head| head["requestId"] == *id)
                .and_then(|head| head["response"]["status"].as_i64())
                .unwrap_or_default();
            let body = data
                .iter()
                .filter(|chunk| chunk["requestId"] == *id)
                .filter_map(|chunk| chunk["dataLength"].as_f64())
                .sum();
            (status, body)
        })
        .collect()
}

/// A refresh that changed nothing in a column Load more brought past its first page transfers no
/// body: its read of every row the column shows (the listing's `rows`) names the tag of the last
/// such answer, which the page holds, and the server's `304` is that answer, every row still
/// drawn. The column's first such read holds no tag yet and pays the read, as one after a change
/// in the column does. Each step waits on the answers the stand-in sent and what the page draws.
#[test]
fn a_refresh_that_changed_nothing_in_a_deep_column_transfers_no_body() {
    let mut board: Vec<Value> = (1..=60)
        .map(|n| short(&format!("deep/ready-{n:03}"), 2, &format!("Ready {n:03}")))
        .collect();
    let site = crate::site();
    serve_ready(&site, &board);
    let Some(mut page) = chrome() else { return };
    open(&mut page, &site, 1280, 800);
    page.eval(LOAD_MORE);
    assert!(
        page.wait_for(&format!("{READY_ROWS} === 60"), SHOWN),
        "Load more never took Ready to 60 rows: it draws {}",
        page.eval(READY_ROWS)
    );
    let long = ready_target("&rows=60");
    let reads = |site: &Site| -> Vec<(Option<String>, u16)> {
        site.answers("/api/task-board")
            .into_iter()
            .filter(|answer| answer.target == long)
            .map(|answer| (answer.named, answer.status))
            .collect()
    };
    let mut version = version();
    let mut refresh = |site: &Site| {
        version += 1;
        site.push(
            version,
            &json!({"version":version,"changes":[{"region":"task-counts","data":{}}]}).to_string(),
        );
    };
    // The first refresh: the column's first long read names no tag.
    refresh(&site);
    until(
        &mut page,
        &site,
        "the first refresh's read of Ready",
        |site| reads(site).len() == 1,
    );
    assert_eq!(
        reads(&site),
        [(None, 200)],
        "the first refresh's read of Ready"
    );
    // Nothing changed in Ready: the next refresh's read names the first's tag and is answered
    // `304`, which Chrome saw carry no body, where the first carried the column.
    refresh(&site);
    until(
        &mut page,
        &site,
        "the unchanged refresh's read of Ready",
        |site| reads(site).len() == 2,
    );
    let named = reads(&site)[1].0.clone();
    assert!(
        named.is_some(),
        "the unchanged refresh named no tag: {:?}",
        reads(&site)
    );
    assert_eq!(reads(&site)[1].1, 304, "{:?}", reads(&site));
    let mut seen = Vec::new();
    if let Err(stall) =
        test_time::until_true("Chrome seeing Ready's long reads finish", STALL, || {
            seen = received(&mut page, &long);
            seen.len() == 2 || {
                page.pump(std::time::Duration::from_millis(25));
                false
            }
        })
    {
        panic!("{stall}: {seen:?}");
    }
    assert_eq!(seen[0].0, 200, "{seen:?}");
    assert!(
        seen[0].1 > 0.0,
        "the first long read carried no body: {seen:?}"
    );
    assert_eq!(
        seen[1],
        (304, 0.0),
        "the unchanged refresh transferred a body: {seen:?}"
    );
    page.eval(STILL);
    assert!(
        page.wait_for("window.boardStill >= 10", SHOWN),
        "the cards on the screen never stood still after the 304"
    );
    assert_eq!(
        page.eval(READY_ROWS),
        json!(60),
        "the 304 left Ready with only some of its rows"
    );
    // A card of Ready changed: the refresh names the same tag, and the server's whole answer is
    // drawn.
    for task in &mut board {
        if task["slug"] == "deep/ready-001" {
            task["title"] = json!("Ready 001 · changed");
        }
    }
    serve_ready(&site, &board);
    refresh(&site);
    assert!(
        page.wait_for(
            "document.querySelector('[data-slug=\"deep/ready-001\"] .taskcard-title')?.textContent === 'Ready 001 · changed'",
            SHOWN
        ),
        "the refresh after a change in Ready never drew it: {:?}",
        reads(&site)
    );
    assert_eq!(
        reads(&site),
        [(None, 200), (named.clone(), 304), (named, 200)],
        "Ready's long reads"
    );
    assert_eq!(page.eval(READY_ROWS), json!(60));
}

/// Whether the pointer is over In progress.
const TAPPED: &str = "document.querySelector('.column[data-status=in_progress]:hover') !== null";

/// On a touch screen the pointer stays where the reader last tapped, not where they are: on a
/// narrow one, whose columns stand side by side (a phone's show one at a time), a tap on a card of
/// In progress, a swipe back to Ready in the middle of the board, Design before it and In progress
/// still peeking in beside it under the tap, and a task filed at Ready's top moves nothing of Ready
/// on the screen.
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
    open(&mut page, &served, 700, 1000);
    assert_eq!(
        page.eval("matchMedia('(hover: none)').matches"),
        json!(true),
        "the setup emulates a touch screen"
    );
    scroll_board(&mut page, "in_progress", 900);
    let at = page.eval(
        "(() => { const box = ({top:0,left:0,bottom:innerHeight,right:innerWidth});
            const slug = [...document.querySelectorAll('.column[data-status=in_progress] .waiting-slug')].find(code => {
                const r = code.getBoundingClientRect();
                return r.top > Math.max(box.top, document.querySelector('header').getBoundingClientRect().bottom) && r.bottom < box.bottom && r.left > box.left && r.right < box.right; });
            window.__slugTapped = false; slug.addEventListener('click', () => { window.__slugTapped = true; }, {once:true});
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
        page.wait_for(&format!("{TAPPED} && window.__slugTapped === true"), SHOWN),
        "the tap never left the pointer over In progress"
    );
    scroll_board(&mut page, "ready", 900);
    assert_eq!(page.eval("[...document.querySelectorAll('.waiting-columns .column')].map(column => column.dataset.status)"), json!(["ready"]));
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

/// The focused title stays focused when the feed moves its task to another stage. On a phone
/// the destination stage becomes the selected tab so the focused title remains mounted.
#[test]
fn a_focused_card_moved_to_another_column_keeps_the_focus() {
    for (width, height) in [(1280, 800), (390, 844)] {
        let mut board = tasks();
        let site = site(&board);
        let Some(mut page) = chrome() else { return };
        open(&mut page, &site, width, height);
        scroll_board(&mut page, "ready", 0);
        let mut version = version();
        let moves: &[(&str, &str)] = &[("place/ready-03", "a.taskcard-title")];
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
            let left = page.eval("scrollX");
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
                page.eval("scrollX"),
                left,
                "{width}px: the focus followed {slug} without scrolling the board to it"
            );
        }
    }
}

/// Where the reader is on the board: its scroller's left, the window's top and Ready's first card
/// on the screen (`first_on_screen`).
const PLACE: &str = "(() => { const board = document.querySelector('.waiting-columns');
    return board ? [Math.round(board.scrollLeft), Math.round(scrollY), FIRST] : null; })()";

/// Open the task of a Ready card's title on the screen, clicked as the reader clicks it: the
/// pointer over Ready and the focus on the link, so Ready is the reader's column; until its page
/// is drawn.
fn open_a_ready_task(page: &mut Chrome, width: u32) {
    let link = page.eval(
        "(() => { const box = ({top:0,left:0,bottom:innerHeight,right:innerWidth});
            const link = [...document.querySelectorAll('.waiting-columns .column[data-status=ready] .taskcard-title')].find(a => {
                const r = a.getBoundingClientRect();
                return r.top > Math.max(box.top, document.querySelector('header').getBoundingClientRect().bottom) && r.bottom < box.bottom && r.left > box.left && r.right < box.right; });
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
        page.wait_for(&format!("location.pathname === {opened} && {READY}"), SHOWN),
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
        let before = page.eval(&place);
        assert!(
            before[1].as_f64().unwrap() > 0.0 && before[2].is_array(),
            "{width}px: the setup scrolls the board sideways and down Ready: {before}"
        );
        open_a_ready_task(&mut page, width);
        page.eval("history.back()");
        let back = format!(
            "(() => {{ const now = {place}, was = {before}; return location.pathname === '/tasks'
                && {READY} && now !== null
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

const KEPT_ROW: &str = "(() => { const box = ({top:0,left:0,bottom:innerHeight,right:innerWidth});
    const row = [...document.querySelectorAll('.waiting-columns .column[data-status=ready] [id^=\"row-\"]')].find(row => {
        const top = row.getBoundingClientRect().top; return top >= box.top && top < box.bottom; });
    return row ? row.id.slice(4) : null; })()";

/// How far, in px, the scroller moves once returned to its offset with no row to keep it by, as the
/// rows above the screen draw their cards where ghosts stood a pixel or so off each: the browser's
/// own scroll anchoring takes that up, nothing on the screen moving. A card that left its column
/// took the board hundreds of px away, to where it went.
const REDRAWN_ABOVE: f64 = 12.0;

/// Hold every read of the device's cache from now on, until `window.letCache()` lets them go: a
/// board opened meanwhile stands as its ghost, as on a device slow to answer, for as long as the
/// test says and no time of its own.
const HOLD_CACHE: &str = "(() => { const read = symphonyCache.read;
    const held = new Promise(go => { window.letCache = go; });
    symphonyCache.read = async (...asked) => { await held; return read(...asked); }; })()";

/// Back from a task to a board whose card its place was kept by left Ready meanwhile, claimed into
/// In progress beside it or cancelled into the column under the board's scroller, draws the board
/// where the reader left its scroller, sideways and down (`REDRAWN_ABOVE`), and the window: never
/// where that card went, once the board has drawn it there and stands still. So too when the
/// device answers what it kept only after the shell's navigation is ready (`late`): the board
/// stands as its ghost until then, at its top left, which is no place of the board's to read
/// (the ghost is `.waiting-columns` too, with no row, nothing pending and no card to move: a wait
/// it passes read [0,0,0] on a loaded machine, release run 2026-11-18T1053-68975). The wait is
/// the board's own (`section.kanban`), live, Ready read and drawn without the card.
#[test]
fn back_to_a_board_whose_kept_card_left_its_column_returns_to_the_windows_place() {
    let place = PLACE.replace("FIRST", "Math.round(scrollY)");
    for (status, late) in [
        ("in_progress", false),
        ("cancelled", false),
        ("in_progress", true),
    ] {
        let case = if late {
            format!("{status}, the device late")
        } else {
            status.to_owned()
        };
        let mut board = tasks();
        let site = site(&board);
        let Some(mut page) = chrome() else { return };
        open(&mut page, &site, 1280, 800);
        scroll_board(&mut page, "design", 900);
        let before = page.eval(&place);
        let kept = page.eval(KEPT_ROW);
        let Some(slug) = kept.as_str().map(str::to_owned) else {
            panic!(
                "{case}: no row of Ready on the screen: {}",
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
        if late {
            page.eval(HOLD_CACHE);
        }
        page.eval("history.back()");
        if late {
            assert!(
                page.wait_for(
                    &format!(
                        "location.pathname === '/tasks' && {READY}
                        && document.querySelector('.ghost-kanban') !== null"
                    ),
                    SHOWN
                ),
                "{case}: Back never drew the board's ghost while the device's reads were held"
            );
            page.eval("window.letCache()");
        }
        let there = format!(
            "location.pathname === '/tasks' && {READY}
                && document.querySelector('main[data-feed=live]') !== null
                && (() => {{ const ready = document.querySelector('.kanban.waiting-columns .column[data-status=ready]');
                    return ready !== null && ready.getAttribute('aria-busy') !== 'true'
                        && ready.querySelector('[id^=\"row-\"]') !== null
                        && ready.querySelector('[id=\"row-{slug}\"]') === null; }})()
                && {WAITING}.near.length === 0",
        );
        assert!(
            page.wait_for(&there, SHOWN),
            "{case}: Back never drew the board with {slug} moved there: {}",
            page.eval(WAITING)
        );
        page.eval(STILL);
        assert!(
            page.wait_for("window.boardStill >= 10", SHOWN),
            "{case}: the cards on the screen never stood still"
        );
        let now = page.eval(&place);
        let down = |at: &Value| at[2].as_f64().unwrap();
        assert!(
            now[0] == before[0]
                && now[1] == before[1]
                && (down(&now) - down(&before)).abs() <= REDRAWN_ABOVE,
            "{case}: Back with {slug} moved drew the board at {now}, where the reader left it at {before}"
        );
    }
}
