//! Approved B's named waits, complete facets and original-stage placement. These fixtures carry
//! the waiting API metadata, including counts beyond the first page, rather than inferring it
//! from the cards loaded in the browser.
use serde_json::{Value, json};

use crate::{PAGE_WIDTH, READY, SHOWN, chrome, site::Site};

const STAGES: [&str; 6] = [
    "concept",
    "design",
    "ready",
    "in_progress",
    "review",
    "done",
];
const KINDS: [&str; 7] = [
    "decision", "nobody", "check", "task", "working", "queued", "parked",
];

fn task(slug: &str, stage: &str, kind: &str, blocked: bool, since: f64) -> Value {
    json!({"slug": slug, "title": match kind {
        "decision" => "Choose the review route", "check" => "Verify the PostgreSQL write rules",
        "task" => "Wait for the shared listing API", "nobody" => "Recover the task left without an executor",
        "working" => "Draw the board at a glance", "queued" => "Make the next ready change",
        _ => "Revisit after the current release" },
        "status": if blocked {"blocked"} else {stage}, "from": if blocked {Some(stage)} else {None},
        "priority": if kind == "decision" {1} else {2}, "body": "", "parent": null,
        "created_by": "owner", "created": since, "updated": since, "links": [], "depends": [],
        "roles": if kind == "nobody" {json!([{"handle":"left-worker", "role":"executor", "health":"left", "since":since, "expires":null}])} else {json!([])},
        "instruction": false, "waits_on": {"kind": kind, "who": match kind {"decision"=>"DEC-42", "check"=>"tests", "task"=>"tools/shared-api", "nobody"=>"left-worker", "working"=>"busy-worker", _=>""},
            "why": match kind {"decision"=>"Owner chooses the route", "check"=>"The store checks have not passed", "task"=>"Needs the shared listing API", "nobody"=>"No executor holds this work", "working"=>"executor", "queued"=>"Ready for an executor", _=>"After this release"}, "since":since}})
}

fn listing() -> Value {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let mut tasks = Vec::new();
    // Reverse the wait kinds and age deliberately: the display must use its documented order.
    for kind in KINDS.into_iter().rev() {
        tasks.push(task(
            &format!("review/{kind}"),
            "review",
            kind,
            !["working", "queued"].contains(&kind),
            now - 7200.0,
        ));
    }
    tasks.push(task(
        "review/check-older",
        "review",
        "check",
        true,
        now - 864000.0,
    ));
    for stage in ["concept", "design", "ready", "in_progress"] {
        tasks.push(task(
            &format!("{stage}/work"),
            stage,
            if stage == "ready" {
                "queued"
            } else {
                "working"
            },
            false,
            now - 3600.0,
        ));
    }
    tasks.push(task(
        "ready/decision",
        "ready",
        "decision",
        true,
        now - 3600.0,
    ));
    for row in &mut tasks {
        match row["slug"].as_str().unwrap() {
            "review/decision" => row["review_rounds"] = json!(1),
            "review/nobody" => row["review_rounds"] = json!(3),
            "review/check-older" => row["review_rounds"] = json!(0),
            _ => {}
        }
    }
    let counts: Vec<Value> = STAGES
        .iter()
        .map(|stage| {
            json!([
                stage,
                tasks
                    .iter()
                    .filter(|task| task["status"] == *stage || task["from"] == *stage)
                    .count()
            ])
        })
        .collect();
    let columns: Vec<Value> = STAGES.iter().map(|stage| {
        let rows: Vec<_> = tasks.iter().filter(|task| task["status"]==*stage || task["from"]==*stage).collect();
        json!({"status": stage, "ordinary": rows.iter().filter(|task|task["status"]!="blocked").count(), "blocked": rows.iter().filter(|task|task["status"]=="blocked").count(), "oldest": if rows.is_empty() {None} else {Some(now-864000.0)}})
    }).collect();
    let kinds: Vec<Value> = KINDS
        .iter()
        .map(|kind| {
            json!([
                kind,
                tasks
                    .iter()
                    .filter(|task| task["waits_on"]["kind"] == *kind)
                    .count()
            ])
        })
        .collect();
    json!({"tasks":tasks,"statuses":STAGES,"counts":counts,"signoff":[],"next":null,
        "waiting":{"kinds":kinds,"columns":columns},"orchestrators":[{"handle":"quiet-orchestrator","health":"stale"}],"unstarted":1})
}

fn open(page: &mut crate::Chrome, site: &Site, width: u32) {
    page.viewport(width, if width <= 760 { 844 } else { 900 }, width <= 760);
    page.open(&site.url("/tasks"));
    if width > 760 {
        assert!(page.wait_for(
            "document.querySelector('[data-slug=\"ready/decision\"]') !== null",
            SHOWN
        ));
        page.eval("const board = document.querySelector('.waiting-columns'), column = board.querySelector('.column[data-status=review]'); board.scrollLeft += column.getBoundingClientRect().left - board.getBoundingClientRect().left");
    }
    assert!(page.wait_for(
        &format!("document.querySelector('[data-slug=\"review/decision\"]') !== null && {READY}"),
        SHOWN
    ));
}

#[test]
fn the_board_fills_the_window_for_both_its_ghost_and_its_cards() {
    for width in [390, 1600] {
        let site = crate::site();
        site.set_json("/api/task-board", &listing());
        site.hold_route("/api/task-board", true);
        let Some(mut page) = chrome() else { return };
        page.viewport(width, 900, width <= 760);
        page.open(&site.url("/tasks"));
        assert!(page.wait_for(
            "document.querySelector('.waiting-board.waiting-ghost') !== null",
            SHOWN
        ));
        // The page's main and the width the window leaves the page beside its scrollbar.
        let widths = format!(
            "[Math.round(document.querySelector('main').getBoundingClientRect().width), Math.round({PAGE_WIDTH})]"
        );
        let [main, window]: [u64; 2] = serde_json::from_value(page.eval(&widths)).unwrap();
        assert_eq!(main, window, "{width}: the ghost");
        site.hold_route("/api/task-board", false);
        assert!(page.wait_for(
            "document.querySelector('.waiting-board .taskcard') !== null",
            SHOWN
        ));
        let [main, window]: [u64; 2] = serde_json::from_value(page.eval(&widths)).unwrap();
        assert_eq!(main, window, "{width}: the cards");
    }
}

#[test]
fn named_waits_stay_in_their_stage_in_order_with_parked_last_and_no_unnamed_block() {
    let site = crate::site();
    site.set_json("/api/task-board", &listing());
    let Some(mut page) = chrome() else { return };
    open(&mut page, &site, 1440);
    assert_eq!(page.eval("[...document.querySelectorAll('.column[data-status=review] .taskcard')].map(card=>card.dataset.slug)"),json!(["review/decision","review/nobody","review/check-older","review/check","review/task","review/queued","review/working","review/parked"]));
    assert_eq!(page.eval("document.querySelector('[data-slug=\"ready/decision\"]').closest('.column').dataset.status"),json!("ready"));
    assert_eq!(
        page.eval("document.querySelector('.column[data-status=blocked]') === null"),
        json!(true)
    );
    assert_eq!(
        page.eval("document.querySelector('.column[data-status=review] h2 .count').textContent"),
        json!("2")
    );
    assert!(
        page.eval(
            "document.querySelector('.column[data-status=review] .column-summary').textContent"
        )
        .as_str()
        .unwrap()
        .contains("+6 blocked")
    );
    assert_eq!(
        page.eval("document.querySelector('.orchestrator-banner.warn') !== null"),
        json!(true)
    );
    assert_eq!(page.eval("document.querySelector('[data-slug=\"review/nobody\"] .task-wait').textContent.includes('On nobody')"),json!(true));
    assert_eq!(
        page.eval("document.querySelectorAll('.taskcard .docchip, .taskcard .chips').length"),
        json!(0)
    );
    assert_eq!(page.eval("(() => {const css=getComputedStyle(document.querySelector('.task-wait .why'));return [css.backgroundColor,css.paddingLeft,css.paddingRight]})()"),json!(["rgba(0, 0, 0, 0)","0px","0px"]),"the wait reason is plain text, not the retired why pill");
    open(&mut page, &site, 390);
    assert_eq!(
        page.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
    assert_eq!(page.eval("(() => {const r=document.querySelector('.taskcard').getBoundingClientRect();return r.top>=0&&r.bottom<=innerHeight})()"),json!(true));
    assert_eq!(page.eval("document.querySelector('.column[data-status=review] .column-summary').checkVisibility()"),json!(true),"a phone keeps its blocked count and oldest age below the tabs");
    assert!(
        page.eval(
            "document.querySelector('.column[data-status=review] .column-summary').textContent"
        )
        .as_str()
        .unwrap()
        .contains("oldest 10 d")
    );
}

#[test]
fn review_marks_follow_live_counts_without_changing_card_geometry_or_reading_details() {
    let site = crate::site();
    let mut answer = listing();
    site.set_json("/api/task-board", &answer);
    let Some(mut page) = chrome() else { return };
    open(&mut page, &site, 390);
    crate::wait_for_feed(&site).expect("the board never followed its feed");
    let mark =
        "document.querySelector('[data-slug=\"review/decision\"] .review-mark')?.textContent ?? ''";
    assert_eq!(page.eval(mark), json!("1 review"));
    assert_eq!(
        page.eval(
            "document.querySelector('[data-slug=\"review/nobody\"] .review-mark')?.textContent"
        ),
        json!("3 reviews")
    );
    assert_eq!(
        page.eval("document.querySelector('[data-slug=\"review/check-older\"] .review-mark')"),
        Value::Null
    );
    page.eval("window.roundCardHeight=document.querySelector('[data-slug=\"review/decision\"]').getBoundingClientRect().height");
    let board: Value = serde_json::from_str(&crate::common::fixture("board.json")).unwrap();
    let version = board["version"].as_u64().unwrap();
    for (step, count, expected) in [(1, 3, "3 reviews"), (2, 0, ""), (3, 1, "1 review")] {
        answer["tasks"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["slug"] == "review/decision")
            .unwrap()["review_rounds"] = json!(count);
        site.set_json("/api/task-board", &answer);
        site.push(version+step,&json!({"version":version+step,"changes":[{"region":"task-fields","data":{"key":"review/decision","fields":{"review_rounds":count}}}]}).to_string());
        assert!(page.wait_for(&format!("({mark}) === {}", json!(expected)), SHOWN));
        assert_eq!(page.eval("document.querySelector('[data-slug=\"review/decision\"]').getBoundingClientRect().height===roundCardHeight"),json!(true));
    }
    let replacement = answer["tasks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["slug"] == "review/decision")
        .unwrap();
    replacement.as_object_mut().unwrap().remove("review_rounds");
    let replacement = replacement.clone();
    site.set_json("/api/task-board", &answer);
    site.push(version+4,&json!({"version":version+4,"changes":[{"region":"task","data":{"key":"review/decision","value":replacement}}]}).to_string());
    assert!(page.wait_for(&format!("({mark}) === ''"), SHOWN));
    assert!(
        !site
            .targets()
            .iter()
            .any(|url| url.contains("/api/task-page") || url.contains("/api/task-history")),
        "badges must use board summaries"
    );
}

#[test]
fn a_wait_chip_uses_complete_counts_and_pages_one_list_across_stages() {
    let site = crate::site();
    let mut complete = listing();
    complete["waiting"]["kinds"][0] = json!(["decision", 31]);
    site.set_json("/api/task-board", &complete);
    let mut selected = complete.clone();
    selected["tasks"] = json!(
        complete["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|task| task["waits_on"]["kind"] == "decision")
            .cloned()
            .collect::<Vec<_>>()
    );
    selected["counts"] = json!([["ready", 1], ["review", 30]]);
    selected["next"] = json!("decisions-next");
    site.set_json(
        "/api/task-board?view=waiting&wait=decision&counts=1&q=&days=1&activity=1&work=0",
        &selected,
    );
    site.set_json(
        "/api/task-board?view=waiting&wait=decision&q=&days=1&activity=1&work=0",
        &selected,
    );
    let mut more = selected.clone();
    more["tasks"] = json!([task(
        "review/decision-next",
        "review",
        "decision",
        true,
        0.0
    )]);
    more["next"] = Value::Null;
    site.set_json("/api/task-board?view=waiting&wait=decision&q=&days=1&cursor=decisions-next&activity=1&work=0",&more);
    let Some(mut page) = chrome() else { return };
    open(&mut page, &site, 1440);
    assert_eq!(
        page.eval("document.querySelector('.wait-chip.k-decision b').textContent"),
        json!("31")
    );
    page.eval("document.querySelector('.wait-chip.k-decision').click()");
    assert!(page.wait_for(
        "document.querySelectorAll('.column[data-status=waiting] .taskcard').length === 2",
        SHOWN
    ));
    assert_eq!(
        page.eval("document.querySelectorAll('.waiting-columns .column').length"),
        json!(1)
    );
    assert_eq!(
        page.eval("document.querySelector('.wait-chip.k-decision').getAttribute('aria-pressed')"),
        json!("true")
    );
    assert_eq!(
        page.eval("document.querySelector('.column[data-status=waiting] .count').textContent"),
        json!("31")
    );
    assert_eq!(
        page.eval("document.querySelector('.wait-chip.k-check b').textContent"),
        json!("2")
    );
    page.eval("[...document.querySelectorAll('.column[data-status=waiting] button')].find(button=>button.textContent.includes('Load more')).click()");
    assert!(page.wait_for(
        "document.querySelector('[data-slug=\"review/decision-next\"]') !== null",
        SHOWN
    ));
    assert_eq!(
        page.eval("document.querySelectorAll('.column[data-status=waiting] .taskcard').length"),
        json!(3)
    );
    let reads: Vec<_> = site
        .targets()
        .into_iter()
        .filter(|target| target.starts_with("/api/task-board?view=waiting&wait=decision&"))
        .collect();
    assert!(
        reads.iter().all(|target| !target.contains("status=")),
        "cross-stage reads: {reads:?}"
    );
}

#[test]
fn an_unnamed_legacy_block_is_ordinary_nobody_work_in_its_original_stage() {
    let site = crate::site();
    let mut legacy = task("review/decision", "review", "nobody", true, 0.0);
    legacy["waits_on"] = json!({"kind":"nobody","who":"","why":"no reason recorded","since":0.0});
    let mut answer = listing();
    answer["tasks"] = json!([legacy, task("ready/queued", "ready", "queued", false, 0.0)]);
    answer["counts"] = json!([["review", 1], ["ready", 1]]);
    answer["waiting"] = json!({"kinds":[["nobody",1],["queued",1]],"columns":[{"status":"review","ordinary":1,"blocked":0,"oldest":0.0},{"status":"ready","ordinary":1,"blocked":0,"oldest":0.0}]});
    answer["orchestrators"] = json!([]);
    answer["unstarted"] = json!(1);
    site.set_json("/api/task-board", &answer);
    let Some(mut page) = chrome() else { return };
    open(&mut page, &site, 390);
    assert_eq!(page.eval("document.querySelector('[data-slug=\"review/decision\"]').closest('.column').dataset.status"),json!("review"));
    assert_eq!(
        page.eval("document.querySelector('.blocked-mark') === null"),
        json!(true)
    );
    assert_eq!(
        page.eval("document.querySelector('.task-wait .on').textContent"),
        json!("On nobody")
    );
    assert_eq!(
        page.eval("document.querySelector('.column-summary b') === null"),
        json!(true)
    );
    assert_eq!(
        page.eval("document.querySelector('.orchestrator-banner:not(.warn)') !== null"),
        json!(true)
    );
}
