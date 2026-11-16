//! The agents page's Following list (apps/symphony-web `AgentLines` in src/agents.tsx, `Followed`
//! in src/agents-live.ts): the open tasks an orchestrator follows, read once the list opens, a page
//! of the server's at a time, Load more bringing each page after the first until the list holds as
//! many as the agent follows.
use super::feed_deltas::{agent, agent_page, held};
use super::{SHOWN, chrome, common, dist, site::Site};
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
    let site = Site::start(dist(), &common::fixture("board.json"));
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
