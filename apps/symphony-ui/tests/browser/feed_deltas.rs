//! A holder's health off the feed (perf/feed-deltas): the server sends it once, as the holder's
//! `session` change, never again in each task it holds, so the role chips of the task board and of
//! a task page take it from that change: every chip of that holder, none of another's, and `lost`
//! once the session is gone from the board, as the board reads a role of no session. The tasks are
//! the look-parity fixture's, each executed by `example`.
use super::{SHOWN, chrome, chrome::Chrome, common, dist, look_parity, site::Site, wait_for_feed};
use serde_json::{Value, json};

/// The board fixture at its version, with the look-parity tasks, task page and thread.
fn site() -> Site {
    let site = Site::start(dist(), &common::fixture("board.json"));
    let content = look_parity::responses();
    for path in ["/api/task-board", "/api/task-page", "/api/task-history"] {
        site.set_json(path, &content[path]);
    }
    site
}

/// Each role chip the page shows, as its health and its title.
const CHIPS: &str = "[...document.querySelectorAll('main .chip[class*=\"health-\"]')].map(chip => [chip.className.match(/health-([a-z]+)/)[1], chip.title])";

fn chips(page: &mut Chrome) -> Vec<(String, String)> {
    serde_json::from_value(page.eval(CHIPS)).unwrap()
}

/// `handle`'s session, as the feed's `session` change carries it, at `health`.
fn session(handle: &str, health: &str) -> Value {
    json!({"handle":handle,"kind":"worker","parent":null,"worktree":"/fixture","branch":"visual/example",
        "model":"fixture","host":"fixture","affinity":[],"connected":1.0,"last_seen":0.0,
        "last_tool":null,"left_at":null,"health":health})
}

/// Push one `session` change of `handle` at `version`: its record, or its removal.
fn push(site: &Site, version: u64, handle: &str, health: Option<&str>) {
    let data = match health {
        Some(health) => json!({"key":handle,"value":session(handle, health)}),
        None => json!({"key":handle,"removed":true}),
    };
    site.push(
        version,
        &json!({"version":version,"changes":[{"region":"session","data":data}]}).to_string(),
    );
}

/// Wait until every one of the `count` role chips the page shows says `health`.
#[track_caller]
fn every_chip_says(page: &mut Chrome, count: usize, health: &str) {
    let all = format!(
        "(() => {{ const chips = {CHIPS}; return chips.length === {count} && chips.every(([h]) => h === {}); }})()",
        json!(health)
    );
    assert!(
        page.wait_for(&all, SHOWN),
        "not every chip says {health}: {:?}",
        chips(page)
    );
    for (_, title) in chips(page) {
        assert!(title.ends_with(&format!(" is {health}")), "{title}");
    }
}

/// Open `path` once its role chips are drawn and its feed is open: how many it shows.
fn opened(page: &mut Chrome, site: &Site, path: &str) -> usize {
    page.open(&site.url(path));
    assert!(
        page.wait_for(
            "document.querySelectorAll('main .chip.health-live').length > 0",
            SHOWN
        ),
        "no role chip was drawn on {path}: {:?}",
        page.eval("document.querySelector('main')?.innerText")
    );
    wait_for_feed(site).expect("the page never followed the feed");
    chips(page).len()
}

#[test]
fn the_boards_role_chips_take_their_holders_health_from_its_session() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    let count = opened(&mut page, &site, "/tasks");
    every_chip_says(&mut page, count, "live");

    push(&site, 4183, "someone-else", Some("stale"));
    push(&site, 4184, "example", Some("stale"));
    every_chip_says(&mut page, count, "stale");

    push(&site, 4185, "example", None);
    every_chip_says(&mut page, count, "lost");

    push(&site, 4186, "example", Some("live"));
    every_chip_says(&mut page, count, "live");
}

#[test]
fn a_task_pages_role_chips_take_their_holders_health_from_its_session() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    let count = opened(&mut page, &site, "/task/visual/in-progress");
    assert_eq!(count, 1, "{:?}", chips(&mut page));

    push(&site, 4183, "example", Some("stale"));
    every_chip_says(&mut page, count, "stale");

    push(&site, 4184, "example", None);
    every_chip_says(&mut page, count, "lost");
}
