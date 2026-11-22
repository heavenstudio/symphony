//! The agents page's Following list (apps/symphony-web `AgentLines` in src/agents.tsx, `Followed`
//! in src/agents-live.ts): the open tasks an orchestrator follows, read once the list opens, a page
//! of the server's at a time, Load more bringing each page after the first until the list holds as
//! many as the agent follows. And the reader's place in Happening now (`LiveAgents.heard`), which a
//! task coming to happen above the card they read moves nothing of, however late the page's tasks
//! run.
use super::feed_deltas::{agent, agent_page, held};
use super::{LATE_TASKS, LATE_TASKS_TAKEN, SHOWN, chrome, site::Site, wait_for_feed};
use serde_json::{Value, json};

/// The read of the open tasks `orch-agent` orchestrates, from `cursor` when one is given.
fn followed(cursor: Option<&str>) -> String {
    let after = cursor.map(|at| format!("&cursor={at}")).unwrap_or_default();
    format!("/api/task-board?holder=orch-agent&role=orchestrator&open=1&older=1{after}")
}

/// The Following list's rows and whether it offers Load more.
const LIST: &str = "[document.querySelectorAll('[id^=\"following-orch-agent-\"]').length, \
    [...document.querySelectorAll('.agent button')].some(button => button.textContent.trim() === 'Load more followed tasks')]";

#[test]
fn a_following_list_past_one_page_reads_the_rest_by_load_more() {
    let site = crate::site();
    let mut orchestrator = agent("orch-agent", "live");
    orchestrator["kind"] = json!("orchestrator");
    orchestrator["following"] = json!(32);
    site.set_json("/api/agent-page", &agent_page(&[orchestrator], &[]));
    let tasks: Vec<Value> = (1..=32)
        .map(|n| {
            let slug = format!("work/followed-{n:02}");
            held(
                &slug,
                &format!("Followed {n}"),
                "ready",
                &[("orch-agent", "orchestrator")],
            )
        })
        .collect();
    // The server's listing pages hold 30 tasks, the first naming where the next starts.
    site.set_json(
        &followed(None),
        &json!({"tasks": tasks[..30], "next": "page-2"}),
    );
    site.set_json(
        &followed(Some("page-2")),
        &json!({"tasks": tasks[30..], "next": null}),
    );
    let Some(mut page) = chrome() else { return };
    page.open(&site.url("/agents"));
    assert!(
        page.wait_for("document.querySelector('.agent summary') !== null", SHOWN),
        "the agent was never drawn: {:?}",
        page.eval("document.querySelector('main')?.innerText")
    );
    let asked = |site: &Site, target: &str| {
        site.targets()
            .iter()
            .filter(|asked| *asked == target)
            .count()
    };
    assert_eq!(asked(&site, &followed(None)), 0, "read before it opened");
    page.eval("[...document.querySelectorAll('.agent summary')].find(summary => summary.textContent.trim() === 'Following 32 open tasks').click()");
    assert!(
        page.wait_for(&format!("JSON.stringify({LIST}) === '[30,true]'"), SHOWN),
        "the first page never showed with Load more: {:?}",
        page.eval(LIST)
    );
    assert_eq!(asked(&site, &followed(None)), 1);
    assert_eq!(asked(&site, &followed(Some("page-2"))), 0);
    page.eval("[...document.querySelectorAll('.agent button')].find(button => button.textContent.trim() === 'Load more followed tasks').click()");
    assert!(
        page.wait_for(&format!("JSON.stringify({LIST}) === '[32,false]'"), SHOWN),
        "Load more never brought the last two: {:?}",
        page.eval(LIST)
    );
    assert_eq!(asked(&site, &followed(Some("page-2"))), 1);
    assert_eq!(asked(&site, &followed(None)), 1);
    for n in [1, 30, 31, 32] {
        let id = format!("following-orch-agent-work/followed-{n:02}");
        assert_eq!(
            page.eval(&format!("document.getElementById({}) !== null", json!(id))),
            json!(true),
            "{id} is not listed"
        );
    }
}

/// The card of Happening now the reader reads, the first whose top is on the screen: its slug and
/// that top.
const FIRST_ON_SCREEN: &str = "(() => { const card = [...document.querySelectorAll('.agents-now .taskcard')]
    .find(card => { const top = card.getBoundingClientRect().top; return top >= 0 && top < innerHeight; });
    return card ? [card.dataset.slug, card.getBoundingClientRect().top] : null; })()";

/// Counts in `window.agentsStill` the frames in a row in which no card of Happening now moved and
/// no box waits to draw within its sentinel's reach (600 px above and below the window), up to ten.
const STILL: &str = "window.agentsStill = 0;
(function frame(last) {
    const tops = [...document.querySelectorAll('.agents-now .taskcard')].map(card => card.getBoundingClientRect().top).join();
    const waits = [...document.querySelectorAll('[data-pending=true]')].some(box => {
        const r = box.getBoundingClientRect(); return r.bottom > -600 && r.top < innerHeight + 600; });
    window.agentsStill = tops === last && !waits ? window.agentsStill + 1 : 0;
    if (window.agentsStill < 10) requestAnimationFrame(() => frame(tops));
})(null)";

/// A task that comes to happen above the card the reader reads in Happening now, on a phone, moves
/// nothing on their screen: the page holds their place (the shell's `reviewHoldPlace`) and scrolls
/// by the card it adds. The browser's own scroll anchoring is off (`overflow-anchor: none`), as in
/// a browser that has none, so what holds the place is the page's hold alone.
#[test]
fn a_task_happening_above_the_reader_moves_nothing_on_the_agents_page() {
    happening_above_the_reader(0);
}

/// The same while every task React's scheduler posts runs twelve frames late ([`LATE_TASKS`]), as
/// on a slow phone or a loaded machine: the page draws the change within the event that brings it,
/// so the hold, which ends after four frames with nothing moved, starts from the card in the page,
/// never from before it.
#[test]
fn a_task_happening_above_the_reader_moves_nothing_while_the_pages_tasks_run_late() {
    happening_above_the_reader(12);
}

/// [`a_task_happening_above_the_reader_moves_nothing_on_the_agents_page`], with React's scheduler's
/// tasks running `late` frames late from the change until the reader's place is seen held.
fn happening_above_the_reader(late: u32) {
    let site = crate::site();
    let roles = [("work-agent", "executor")];
    // `place/t-00`, first of the page's tasks, waits in Ready: Happening now opens with `place/t-01`.
    let tasks: Vec<Value> = (0..=14)
        .map(|n| {
            let status = if n == 0 { "ready" } else { "in_progress" };
            held(
                &format!("place/t-{n:02}"),
                &format!("Task {n}"),
                status,
                &roles,
            )
        })
        .collect();
    site.set_json(
        "/api/agent-page",
        &agent_page(&[agent("work-agent", "live")], &tasks),
    );
    let Some(mut page) = chrome() else { return };
    page.viewport(390, 844, true);
    page.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": LATE_TASKS }),
    );
    page.open(&site.url("/agents"));
    assert!(
        page.wait_for(
            "document.querySelectorAll('main[data-feed=live] .agents-now > *').length === 14",
            SHOWN
        ),
        "Happening now was never drawn: {:?}",
        page.eval("document.querySelector('main')?.innerText.slice(0, 600)")
    );
    wait_for_feed(&site).expect("the page never followed the feed");
    if late > 0 {
        assert_eq!(
            page.eval(LATE_TASKS_TAKEN),
            json!(true),
            "React's scheduler takes no task through the page script's channel, so none would run late"
        );
    }
    // The reader goes down Happening now to its eighth card, the browser's own anchoring off.
    page.eval(
        "(() => { document.documentElement.style.overflowAnchor = 'none';
            scrollBy(0, document.getElementById('now-place/t-08').getBoundingClientRect().top - 120); })()",
    );
    page.eval(STILL);
    assert!(
        page.wait_for("window.agentsStill >= 10", SHOWN),
        "the cards on the screen never stood still"
    );
    let before = page.eval(FIRST_ON_SCREEN);
    let top = page.eval("scrollY").as_f64().unwrap_or(f64::NAN);
    assert_eq!(
        before[0],
        json!("place/t-08"),
        "the reader must read the eighth card: {before}"
    );
    page.eval(&format!("window.__lateTasks = {late}"));
    site.push(
        4183,
        &json!({"version": 4183, "changes": [{"region": "task-fields",
            "data": {"key": "place/t-00", "fields": {"status": "in_progress"}}}]})
        .to_string(),
    );
    let kept = format!(
        "(() => {{ const now = {FIRST_ON_SCREEN}, was = {before};
            return document.getElementById('now-place/t-00') !== null && now !== null
                && now[0] === was[0] && Math.abs(now[1] - was[1]) <= 1; }})()"
    );
    assert!(
        page.wait_for(&kept, SHOWN),
        "the task come to happen above moved the card read from {before} to {}",
        page.eval(FIRST_ON_SCREEN)
    );
    page.eval("window.__lateTasks = 0");
    // The page scrolled by the card added above the one read, which kept its place on the screen.
    let now = page.eval("scrollY").as_f64().unwrap_or(f64::NAN);
    assert!(
        now > top + 40.0,
        "the card added above the one read scrolled the page from {top} to {now}"
    );
}
