//! The tokens and their estimated cost on the pages: a task's cost follows its time on its
//! timeline, the total after the task's and each stage stretch's after its own, with nothing at
//! the page's foot; the board asks the cost of the cards it drew in one read apart from its
//! listings, once they are drawn, and shows each card's figure before its time. With a folder
//! named, the four views' pictures, wide and on a phone, light and dark.
use super::{SHOWN, binaries, chrome, common, dist, look_parity, site, site::Site};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use symphony_api::{REPORTS_PATH, TASK_COSTS_PATH, TASK_TOKENS_PATH};

const PAGE: &str = "/task/visual/in-progress";

/// `n` input tokens in `calls` calls, as the wire writes tokens.
fn tokens(n: u64, calls: u64) -> serde_json::Value {
    json!({"input": n, "cache_write_5m": 0, "cache_write_1h": 0, "cache_read": 0, "output": 0,
           "calls": calls})
}

/// What `dollars` of input cost, with the day the prices were read.
fn cost(dollars: f64) -> serde_json::Value {
    json!({"dollars": dollars, "input": dollars, "cache_write_5m": 0.0, "cache_write_1h": 0.0,
           "cache_read": 0.0, "output": 0.0, "unpriced": [], "prices": "2026-11-16"})
}

/// The task the approved design draws (docs/analysis/token-cost.md), as this page's: its stages
/// from its making to its close, a day and three hours later, six reviews and four bounces.
fn worked_timeline() -> Value {
    const STAGES: [(&str, f64, Option<f64>, u32); 14] = [
        ("ready", 1795046965.4, Some(1795072112.5), 0),
        ("in_progress", 1795072112.5, Some(1795083775.8), 0),
        ("review", 1795083775.8, Some(1795084284.5), 1),
        ("in_progress", 1795084284.5, Some(1795086628.6), 0),
        ("review", 1795086628.6, Some(1795086861.8), 2),
        ("done", 1795086861.8, Some(1795132641.4), 0),
        ("review", 1795132641.4, Some(1795134886.9), 3),
        ("in_progress", 1795134886.9, Some(1795139051.0), 0),
        ("review", 1795139051.0, Some(1795139175.7), 4),
        ("in_progress", 1795139175.7, Some(1795143034.0), 0),
        ("review", 1795143034.0, Some(1795143318.2), 5),
        ("in_progress", 1795143318.2, Some(1795145545.4), 0),
        ("review", 1795145545.4, Some(1795145576.0), 6),
        ("done", 1795145576.0, None, 0),
    ];
    let verdicts = [
        "changes", "approved", "changes", "changes", "changes", "approved",
    ];
    let stages: Vec<Value> = (STAGES.iter())
        .map(|(status, from, to, round)| json!({"status": status, "from": from, "to": to, "round": round}))
        .collect();
    let rounds: Vec<Value> = (STAGES.iter())
        .filter(|stage| stage.0 == "review")
        .map(|(_, from, to, round)| {
            json!({"round": round, "from": from, "taken": from, "to": to, "by": "gilded-gecko-838",
                "verdict": verdicts[*round as usize - 1]})
        })
        .collect();
    let claims: Vec<Value> = (STAGES.iter())
        .filter(|stage| stage.0 == "in_progress")
        .map(|(_, from, to, _)| json!({"by": "keen-swift-85", "from": from, "to": to}))
        .collect();
    let mut timeline: Value =
        serde_json::from_str(&common::fixture("task-timeline.json")).expect("the timeline fixture");
    timeline["slug"] = json!("visual/in-progress");
    timeline["created"] = json!(STAGES[0].1);
    timeline["closed"] = json!(STAGES[13].1);
    timeline["now"] = json!(1795146335.7);
    timeline["zone"] = json!([{"from": STAGES[0].1, "seconds": 3600}]);
    timeline["first_claimed"] = json!(STAGES[1].1);
    timeline["bounces"] = json!(4);
    timeline["stages"] = json!(stages);
    timeline["rounds"] = json!(rounds);
    timeline["claims"] = json!(claims);
    // One build, so the totals have their second line, the machine's.
    let mut build = timeline["runs"][0].clone();
    build["queued"] = json!(STAGES[1].1 + 600.0);
    build["started"] = json!(STAGES[1].1 + 660.0);
    build["ended"] = json!(STAGES[1].1 + 1260.0);
    build["host"] = json!("");
    timeline["runs"] = json!([build]);
    timeline["gates"] = json!([]);
    timeline
}

/// That task's tokens as the board recorded them: thirteen rows, $80.02 in 884 calls.
fn worked_tokens() -> Value {
    const ROWS: [(&str, u32, f64, f64, u64, u64, f64); 13] = [
        (
            "reviewer",
            2,
            1795083839.862279,
            1795084288.631473,
            48,
            7_275_885,
            3.7834692,
        ),
        (
            "reviewer",
            3,
            1795086785.707946,
            1795086860.981354,
            8,
            1_835_226,
            0.6444798,
        ),
        (
            "reviewer",
            4,
            1795134650.933333,
            1795134907.099368,
            29,
            8_123_175,
            4.2336168,
        ),
        (
            "reviewer",
            5,
            1795139064.596429,
            1795139185.398185,
            14,
            4_668_022,
            3.60281,
        ),
        (
            "reviewer",
            6,
            1795143037.715704,
            1795145539.763228,
            21,
            4_142_416,
            4.7936542,
        ),
        (
            "reviewer",
            7,
            1795145548.146142,
            1795145563.445489,
            4,
            554_062,
            0.2129138,
        ),
        (
            "executor",
            1,
            1795072115.883158,
            1795083775.383301,
            349,
            80_503_848,
            30.430627,
        ),
        (
            "executor",
            2,
            1795083777.281189,
            1795086625.691532,
            94,
            21_307_210,
            7.0338238,
        ),
        (
            "executor",
            3,
            1795086630.197779,
            1795086639.993331,
            2,
            279_980,
            0.0760884,
        ),
        (
            "executor",
            4,
            1795132665.307121,
            1795139033.895445,
            160,
            43_306_142,
            13.225875,
        ),
        (
            "executor",
            5,
            1795139053.867474,
            1795143030.568916,
            124,
            26_056_320,
            9.211979,
        ),
        (
            "executor",
            6,
            1795143036.657329,
            1795145544.436137,
            28,
            8_887_113,
            2.443202,
        ),
        (
            "reserved",
            1,
            1795072104.565521,
            1795072112.049708,
            3,
            176_175,
            0.3228336,
        ),
    ];
    let rows: Vec<Value> = (ROWS.iter())
        .map(|(role, round, first, last, calls, n, dollars)| {
            let handle = if *role == "reviewer" {
                "gilded-gecko-838"
            } else {
                "keen-swift-85"
            };
            json!({"handle": handle, "slug": "visual/in-progress", "role": role,
                "model": "claude-fable-5-2", "speed": "standard", "round": round,
                "tokens": kinds(0, 0, *n, 0, *calls), "cost": cost(*dollars),
                "first": first, "last": last})
        })
        .collect();
    let role = |name: &str| {
        let of = || ROWS.iter().filter(|row| row.0 == name);
        json!({"role": name, "tokens": kinds(0, 0, of().map(|row| row.5).sum(), 0, of().map(|row| row.4).sum()),
            "cost": cost(of().map(|row| row.6).sum())})
    };
    json!({
        "total": {"input": 1838, "cache_write_5m": 0, "cache_write_1h": 3_018_348,
            "cache_read": 203_335_683_u64, "output": 759_705, "calls": 884},
        "cost": {"dollars": 80.0153726, "input": 0.007352, "cache_write_5m": 0.0,
            "cache_write_1h": 24.146784, "cache_read": 40.6671366, "output": 15.1941,
            "unpriced": [], "prices": "2026-11-16"},
        "by_role": [role("executor"), role("reviewer"), role("reserved")],
        "by_round": [],
        "rows": rows,
    })
}

/// A task that took no tokens, as the route answers it.
fn no_tokens() -> Value {
    json!({"total": tokens(0, 0), "cost": cost(0.0), "by_role": [], "by_round": [], "rows": []})
}

/// The stand-in answering the fixture task's page and thread, and the design's task's timeline.
fn worked_site() -> Site {
    let site = Site::start(dist(), &common::fixture("board.json"));
    let content = look_parity::responses();
    for path in ["/api/task-page", "/api/task-history"] {
        site.set_json(path, &content[path]);
    }
    site.set_json(symphony_api::TASK_TIMELINE_PATH, &worked_timeline());
    site.set_json(TASK_TOKENS_PATH, &worked_tokens());
    site
}

/// The text of each element `selector` matches, in the page's order.
fn texts(page: &mut chrome::Chrome, selector: &str) -> Vec<String> {
    serde_json::from_value(page.eval(&format!(
        "[...document.querySelectorAll({})].map(e => e.textContent.trim())",
        json!(selector)
    )))
    .unwrap_or_default()
}

/// What each stretch that carries a cost says after its time, in the design's table's order.
const STRETCHES: [&str; 11] = [
    "3 h 14 m $30.75",
    "8 m $3.78",
    "39 m $7.03",
    "4 m $0.72",
    "37 m $4.23",
    "1 h 9 m $13.23",
    "2 m $3.60",
    "1 h 4 m $9.21",
    "5 m $4.79",
    "37 m $2.44",
    "1 m $0.21",
];

#[test]
fn a_tasks_cost_follows_its_time_on_its_timeline_and_nothing_is_at_the_pages_foot() {
    for width in [1440, 390] {
        let site = worked_site();
        let Some(mut page) = chrome() else {
            panic!("Chrome is required for the tokens' acceptance")
        };
        page.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":width,"height":900,"deviceScaleFactor":1,"mobile":width < 400}),
        );
        page.open(&site.url(PAGE));
        assert!(
            page.wait_for(
                "document.querySelector('.tl-drawn .tl-cost')?.textContent === '$80.02'",
                SHOWN
            ),
            "{width}: {:?}",
            page.eval("document.getElementById('timeline')?.innerText")
        );
        // The total follows the task's time on the first line, with no word of its own; its
        // words say what the foot's table said.
        let first = page.eval("document.querySelector('.tl-totals').innerText");
        let lines = if width >= 900 {
            "1 d 3 h in all $80.02 · 6 review rounds · 4 bounces · machine 10 m, queued 1 m"
        } else {
            "1 d 3 h in all $80.02\n6 review rounds · 4 bounces · machine 10 m"
        };
        assert_eq!(first, json!(lines), "{width}");
        let words = page.eval("document.querySelector('.tl-cost').title");
        assert_eq!(
            words,
            json!(
                "$80.02, estimated at API list prices read 2026-11-16, not money spent: \
                 executor $62.42 · reviewer $17.27 · reserved $0.32 (input $0.01 · cache write \
                 $24.15 · cache read $40.67 · output $15.19); 207M tokens in 884 calls"
            ),
            "{width}"
        );
        // Nothing at the page's foot: no section, no table, no chunk and no sheet of its own.
        let foot = page.eval(
            "({section: document.querySelector('.task-tokens, .task-tokens-body') !== null,
               sheet: document.getElementById('task-tokens-style') !== null,
               tables: document.querySelectorAll('article.task > details table').length,
               said: document.body.innerText.includes('Tokens and estimated cost')})",
        );
        assert_eq!(
            foot,
            json!({"section": false, "sheet": false, "tables": 0, "said": false}),
            "{width}"
        );
        assert!(
            (site.targets().iter()).all(|path| !path.contains("/task-tokens-")),
            "{width}: a tokens chunk was asked for"
        );
        // Each stage's row ends with its time and its cost; a stage that carries none, its time.
        let rows = texts(&mut page, ".tl-list .tl-row:not(.tl-sub) .tl-took");
        let mut expected = vec!["6 h 59 m"];
        expected.extend(STRETCHES);
        assert_eq!(rows, expected, "{width}: the stage rows");
        // Every stretch's words say its cost, whose calls they were and when they ran.
        let said: Vec<String> =
            serde_json::from_value(page.eval(
                "[...document.querySelectorAll('[data-lane=stage] .tl-seg')].map(e => e.title)",
            ))
            .unwrap();
        let carrying: Vec<&String> = said.iter().filter(|title| title.contains('$')).collect();
        assert_eq!(carrying.len(), STRETCHES.len(), "{width}: {said:#?}");
        for (title, stretch) in carrying.iter().zip(STRETCHES) {
            // A stretch under a minute is said to the second in no label: its row's time.
            let cost = stretch.rsplit(' ').next().unwrap();
            assert!(title.contains(&format!(" {cost}, from ")), "{title}");
            assert!(
                title.ends_with("estimated at API list prices read 2026-11-16, not money spent"),
                "{title}"
            );
        }
        assert_eq!(
            said[1],
            "in progress 3 h 14 m $30.75, from 08:08 to 11:22: executor $30.43, 08:08–11:22, \
             in 349 calls; reserved $0.32, at 08:08, in 3 calls; estimated at API list prices \
             read 2026-11-16, not money spent",
        );
        assert_eq!(
            said[4],
            "review 4 m $0.72, from 12:10 to 12:14: reviewer $0.64, 12:13–12:14, in 8 calls; \
             executor $0.08, at 12:10, in 2 calls; estimated at API list prices read \
             2026-11-16, not money spent",
        );
        if width >= 900 {
            // The whole task at this width, a day and three hours on one track: the first stretch
            // of work has room for its time and its cost, without its word, and no stretch draws
            // two labels at once.
            let labels: Vec<String> = serde_json::from_value(page.eval(
                "[...document.querySelectorAll('[data-lane=stage] .tl-seg')].map(seg =>
                    [...seg.querySelectorAll('.tl-fit')].filter(fit => getComputedStyle(fit).display !== 'none')
                        .map(fit => fit.textContent).join('|'))",
            ))
            .unwrap();
            assert_eq!(labels[0], "ready 6 h 59 m", "{labels:?}");
            assert_eq!(labels[1], "3 h 14 m $30.75", "{labels:?}");
            assert!(
                labels[2..].iter().all(|label| !label.contains('|')),
                "a stretch draws two labels at once: {labels:?}"
            );
            // The focus on a stretch, and on the total, says its words in the legend's place.
            page.eval("document.getElementById('tl-stop-1').focus()");
            assert_eq!(
                page.eval("document.querySelector('.tl-detail')?.textContent"),
                json!(said[1])
            );
            page.eval("document.querySelector('.tl-cost').focus()");
            assert_eq!(
                page.eval("document.querySelector('.tl-detail')?.textContent"),
                words
            );
            page.eval("document.activeElement.blur()");
            assert_eq!(
                page.eval("document.querySelector('.tl-detail')"),
                Value::Null
            );
        } else {
            // A phone's box is as tall as it was with no cost, its totals two lines; a tap on the
            // total says its words under them, and a tap on a stage's row its own.
            let lines = "(() => { const totals = document.querySelector('.tl-totals');
                return Math.round(totals.getBoundingClientRect().height / parseFloat(getComputedStyle(totals).lineHeight)); })()";
            assert_eq!(page.eval(lines), json!(2), "the totals' lines");
            assert_eq!(
                page.eval(
                    "document.getElementById('timeline-slot').getBoundingClientRect().height"
                ),
                json!(240)
            );
            let shown = "getComputedStyle(document.querySelector('.tl-cost-said')).display";
            assert_eq!(page.eval(shown), json!("none"));
            page.eval("document.querySelector('.tl-cost').focus()");
            assert_eq!(page.eval(shown), json!("block"));
            assert_eq!(
                page.eval("document.querySelector('.tl-cost-said').textContent"),
                words
            );
            page.eval("document.activeElement.blur()");
            let row = "[...document.querySelectorAll('.tl-list .tl-row:not(.tl-sub)')][1]";
            page.eval(&format!("{row}.focus()"));
            assert_eq!(
                page.eval(&format!(
                    "(() => {{ const facts = {row}.querySelector('.tl-facts');
                        return [getComputedStyle(facts).display, facts.textContent.includes('executor $30.43')]; }})()"
                )),
                json!(["block", true])
            );
            page.eval("document.activeElement.blur()");
        }
        assert_eq!(
            page.eval("document.documentElement.scrollWidth <= innerWidth"),
            json!(true),
            "{width}: the page scrolls across"
        );
        // One read of the tokens, beside the timeline's.
        let reads: Vec<String> = (site.targets().into_iter())
            .filter(|path| path.starts_with(TASK_TOKENS_PATH))
            .collect();
        assert_eq!(
            reads,
            [format!("{TASK_TOKENS_PATH}?slug=visual%2Fin-progress")]
        );
    }
}

#[test]
fn a_task_with_no_tokens_draws_no_cost_anywhere_on_desktop_and_phone() {
    for width in [1440, 390] {
        for answer in [Some(no_tokens()), None] {
            let site = worked_site();
            match &answer {
                Some(none) => site.set_json(TASK_TOKENS_PATH, none),
                // A read that fails, which the page's store tries again: drawn as with none.
                None => site.fail_route(TASK_TOKENS_PATH, true),
            }
            let Some(mut page) = chrome() else {
                panic!("Chrome is required for the tokens' acceptance")
            };
            page.call(
                "Emulation.setDeviceMetricsOverride",
                json!({"width":width,"height":900,"deviceScaleFactor":1,"mobile":width < 400}),
            );
            page.open(&site.url(PAGE));
            assert!(
                page.wait_for(
                    "document.querySelectorAll('.tl-drawn .tl-list .tl-row').length > 0",
                    SHOWN
                ),
                "{width}: {:?}",
                page.eval("document.getElementById('timeline')?.innerText")
            );
            assert!(page.wait_for(
                &format!("performance.getEntriesByType('resource').some(e => e.name.includes('{TASK_TOKENS_PATH}'))"),
                SHOWN
            ));
            assert_eq!(
                page.eval("document.querySelector('.tl-totals > span').textContent"),
                json!("1 d 3 h in all · 6 review rounds · 4 bounces"),
                "{width}"
            );
            assert_eq!(
                page.eval(
                    "[document.querySelector('.tl-cost, .tl-cost-said'), document.getElementById('timeline').textContent.includes('$'),
                      [...document.querySelectorAll('[data-lane=stage] .tl-seg')].some(e => e.title.includes('$'))]"
                ),
                json!([null, false, false]),
                "{width}"
            );
            let rows = texts(&mut page, ".tl-list .tl-row:not(.tl-sub) .tl-took");
            assert_eq!(rows[..3], ["6 h 59 m", "3 h 14 m", "8 m"], "{width}");
            // A stage's row takes no tap's focus where it has no cost to say.
            assert_eq!(
                page.eval(
                    "document.querySelectorAll('.tl-list .tl-row:not(.tl-sub)[tabindex]').length"
                ),
                json!(0)
            );
        }
    }
}

#[test]
fn the_board_asks_the_cost_of_the_cards_it_drew_once_and_shows_it() {
    let site = Site::start(dist(), &common::fixture("board.json"));
    site.set_json("/api/task-board", &site::example_listing("ready"));
    site.set_json(
        TASK_COSTS_PATH,
        &json!({"prices": "2026-11-16", "costs": [{"slug": "tools/example", "dollars": 4.5}]}),
    );
    let Some(mut page) = chrome() else {
        panic!("Chrome is required for the cards' acceptance")
    };
    page.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":1440,"height":900,"deviceScaleFactor":1,"mobile":false}),
    );
    page.open(&site.url("/tasks"));
    assert!(
        page.wait_for(
            "document.querySelector(\"[data-slug='tools/example'] .card-cost\")?.textContent === '$4.50'",
            SHOWN
        ),
        "{:?}",
        page.eval("document.body.innerText")
    );
    // On the time's line, before the time and in its type; the slug's line holds none of it.
    let card = page.eval(
        "(() => { const card = document.querySelector(\"[data-slug='tools/example']\");
            const cost = card.querySelector('.card-cost'), age = card.querySelector('.task-wait .age');
            return {line: cost.parentElement === age, first: age.firstElementChild === cost,
                said: /^\\$4\\.50·\\d+ [hd]$/.test(age.textContent),
                slug: card.querySelector('.waiting-slug .card-cost') === null,
                weight: getComputedStyle(cost).fontWeight === getComputedStyle(age).fontWeight,
                level: Math.abs(cost.getBoundingClientRect().bottom - age.getBoundingClientRect().bottom) < 2}; })()",
    );
    assert_eq!(
        card,
        json!({"line": true, "first": true, "said": true, "slug": true, "weight": true, "level": true})
    );
    let title = page.eval(
        "document.querySelector(\"[data-slug='tools/example'] .card-cost\").getAttribute('title')",
    );
    assert!(
        (title.as_str().unwrap_or_default())
            .contains("estimated at API list prices read 2026-11-16, not money spent"),
        "{title}"
    );
    let reads: Vec<String> = (site.targets().into_iter())
        .filter(|path| path.starts_with(TASK_COSTS_PATH))
        .collect();
    assert_eq!(reads.len(), 1, "{reads:?}");
    assert!(reads[0].contains("tools%2Fexample"), "{reads:?}");
    // The listings carry no cost: the board's own reads are as they were.
    assert!(
        (site.targets().iter())
            .filter(|path| path.starts_with("/api/task-board"))
            .all(|path| !path.contains("cost")),
    );
}

/// Tokens of each kind in `calls` calls, as the wire writes them.
fn kinds(input: u64, write: u64, read: u64, output: u64, calls: u64) -> Value {
    json!({"input": input, "cache_write_5m": write, "cache_write_1h": 0, "cache_read": read,
           "output": output, "calls": calls})
}

/// `dollars` estimated, shared among the kinds, with the tokens of each model `unpriced` names.
fn priced(dollars: f64, unpriced: &[(&str, u64)]) -> Value {
    let unpriced: Vec<Value> = (unpriced.iter())
        .map(|(model, n)| {
            json!({"model": model, "speed": "standard", "tokens": kinds(n / 10, 0, n - n / 10, 0, 4)})
        })
        .collect();
    json!({"dollars": dollars, "input": dollars * 0.02, "cache_write_5m": dollars * 0.28,
           "cache_write_1h": 0.0, "cache_read": dollars * 0.3, "output": dollars * 0.4,
           "unpriced": unpriced, "prices": "2026-11-16"})
}

/// The board's stages, as its waiting view names them.
const STAGES: [&str; 8] = [
    "concept",
    "design",
    "ready",
    "in_progress",
    "build",
    "test",
    "review",
    "done",
];

/// Cards in Review, In progress and Ready, long slugs among them, every one with tokens but two.
fn pictured_board(now: f64) -> Value {
    let card = |slug: &str, title: &str, stage: &str, who: &str| {
        json!({"slug": slug, "title": title, "status": stage, "priority": 2, "body": "",
            "parent": null, "created_by": "owner", "created": now - 7200.0, "updated": now - 3600.0,
            "links": [], "depends": [], "roles": [], "instruction": false,
            "waits_on": {"kind": "working", "who": who, "why": "executor", "since": now - 3600.0}})
    };
    let tasks = [
        card(
            "app/token-cost-on-pages",
            "Pages show the tokens and estimated cost the board records",
            "review",
            "glacial-sparrow-263",
        ),
        card(
            "tools/workflow-reports",
            "Reports draw the time spent in each workflow phase",
            "review",
            "amber-otter-340",
        ),
        card(
            "app/workflow-pages-follow-ups-for-the-board-and-the-task-page",
            "The workflow pages' follow-ups",
            "review",
            "ivory-robin-143",
        ),
        card(
            "perf/board-first-visit-room",
            "Room under the board's first-visit budget",
            "in_progress",
            "slate-kestrel-28",
        ),
        card(
            "tools/attempt-history-presentation",
            "Present an attempt's history",
            "in_progress",
            "ivory-crane-629",
        ),
        card(
            "docs/quiet-card",
            "A card whose task took no tokens",
            "ready",
            "nobody",
        ),
        card(
            "tools/no-tokens-yet",
            "Another with none recorded",
            "ready",
            "nobody",
        ),
    ];
    let held = |stage: &str| tasks.iter().filter(|task| task["status"] == stage).count();
    let counts: Vec<Value> = STAGES
        .iter()
        .map(|stage| json!([stage, held(stage)]))
        .collect();
    let columns: Vec<Value> = (STAGES.iter())
        .map(|stage| {
            json!({"status": stage, "ordinary": held(stage), "blocked": 0,
            "oldest": if held(stage) == 0 {None} else {Some(now - 7200.0)}})
        })
        .collect();
    json!({"tasks": tasks, "statuses": STAGES, "counts": counts, "signoff": [], "next": null,
        "waiting": {"kinds": [["working", tasks.len()]], "columns": columns},
        "orchestrators": [{"handle": "coral-badger-11", "health": "live"}], "unstarted": 0})
}

/// What the cards drawn cost: one with a model the table has no price for, one all unpriced.
fn pictured_costs() -> Value {
    json!({"prices": "2026-11-16", "costs": [
        {"slug": "app/token-cost-on-pages", "dollars": 5.97, "unpriced": true},
        {"slug": "tools/workflow-reports", "dollars": 18.42},
        {"slug": "app/workflow-pages-follow-ups-for-the-board-and-the-task-page", "dollars": 0.84},
        {"slug": "perf/board-first-visit-room", "dollars": 127.5},
        {"slug": "tools/attempt-history-presentation", "dollars": 0.0, "unpriced": true},
    ]})
}

/// An orchestrator and an executor with tokens, and a worker with none yet.
fn pictured_agents() -> Value {
    let agent = |handle: &str, kind: &str, tokens: Value, cost: Value| {
        json!({"handle": handle, "kind": kind, "model": "claude-opus-5-5", "parent": null,
            "task": null, "role": null, "tasks": [], "worktree": "/fixture", "branch": "main",
            "agent": null, "session_id": null, "pane": null, "callback": format!("codex:{handle}"),
            "chat": false, "state": "working", "state_detail": null, "health": "live",
            "last_tool_call": null, "last_message": null, "tokens": tokens, "cost": cost})
    };
    let agents = [
        agent(
            "coral-badger-11",
            "orchestrator",
            kinds(9_000, 310_000, 8_050_000, 41_000, 412),
            priced(31.2, &[]),
        ),
        agent(
            "glacial-sparrow-263",
            "worker",
            kinds(1_600, 57_000, 1_440_000, 24_500, 85),
            priced(5.97, &[("gpt-x", 96_000)]),
        ),
        agent("fresh-wren-5", "worker", Value::Null, Value::Null),
    ];
    json!({"view": {"generated": 1.0, "probes": {"panes": "ok", "slots": "ok"}, "agents": agents},
        "tasks": [], "total": agents.len(), "next": null})
}

/// A week by the day from the Reports fixture, each day's tokens by role, one day with no record,
/// and the tasks landed in it.
fn pictured_reports() -> Value {
    const DAY: f64 = 86_400.0;
    let fixture: Value =
        serde_json::from_str(&common::fixture("reports-page.json")).expect("the reports fixture");
    let from = fixture["from"].as_f64().unwrap() - 6.0 * DAY;
    let role = |role: &str, n: u64, dollars: f64| {
        json!({"role": role, "tokens": kinds(n / 1000, n / 25, n, n / 60, n / 20_000 + 1),
            "cost": priced(dollars, &[])})
    };
    let buckets: Vec<Value> = (0..7_u32)
        .map(|day| {
            let start = from + f64::from(day) * DAY;
            let mut bucket = fixture["buckets"][0].clone();
            bucket["start"] = json!(start);
            bucket["end"] = json!(start + DAY);
            let n = u64::from(day % 3 + 2);
            bucket["spend"] = if day == 2 {
                Value::Null
            } else {
                json!([
                    role("orchestrator", n * 1_200_000, n as f64 * 4.1),
                    role("executor", n * 3_100_000, n as f64 * 9.8),
                    role("reviewer", n * 900_000, n as f64 * 2.7),
                    role("worker", 150_000, 0.4),
                ])
            };
            bucket
        })
        .collect();
    let landed = |slug: &str, dollars: f64| {
        json!({"slug": slug, "at": from + 5.0 * DAY, "tokens": kinds(2_000, 80_000, 2_400_000, 30_000, 120),
            "cost": priced(dollars, &[])})
    };
    let mut page = fixture;
    page["days"] = json!(7);
    page["from"] = json!(from);
    page["buckets"] = json!(buckets);
    page["marks"] = json!([]);
    page["unread"] = json!([]);
    page["prices"] = json!("2026-11-16");
    page["landed"] = json!({
        "tasks": 12, "without": 1, "median": 6.4,
        "dearest": [
            landed("perf/board-first-visit-room", 127.5),
            landed("tools/workflow-reports", 18.42),
            landed("app/token-cost-on-pages", 5.97),
        ],
        "first_round": {"tokens": kinds(20_000, 900_000, 28_000_000, 400_000, 1_400), "cost": priced(88.3, &[])},
        "later_rounds": {"tokens": kinds(6_000, 210_000, 7_900_000, 90_000, 380), "cost": priced(21.6, &[("gpt-x", 96_000)])},
        "again": 5,
    });
    page
}

/// The stand-in answering each view's reads with the pictures' figures.
fn pictured_site() -> Site {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let site = Site::start(dist(), &common::fixture("board.json"));
    let content = look_parity::responses();
    for path in ["/api/task-page", "/api/task-history"] {
        site.set_json(path, &content[path]);
    }
    site.set_json(symphony_api::TASK_TIMELINE_PATH, &worked_timeline());
    site.set_json(TASK_TOKENS_PATH, &worked_tokens());
    site.set_json("/api/task-board", &pictured_board(now));
    site.set_json(TASK_COSTS_PATH, &pictured_costs());
    site.set_json("/api/agent-page", &pictured_agents());
    // The week by the day, and the same as the last 24 hours, the window a first visit opens on
    // before the pictures choose the week (`CHOOSE_A_WEEK`).
    let week = pictured_reports();
    let mut first = week.clone();
    first["grain"] = json!("hour");
    first["days"] = json!(1);
    site.set_json(REPORTS_PATH, &week);
    site.set_json(&format!("{REPORTS_PATH}?grain=hour&days=1"), &first);
    site
}

/// Reports' window kept as a reader who chose a week by the day keeps it, then the page loaded
/// again on it.
const CHOOSE_A_WEEK: &str = "localStorage.setItem('reports-grain', 'day'); \
    localStorage.setItem('reports-days', '7'); setTimeout(() => location.reload(), 0); true";

/// `symphony shot` of `page` on `site` with `steps`, at each width and scheme, into `out` as
/// `NAME-WIDTH-SCHEME.png`.
fn shoot(
    symphony: &Path,
    browser: &str,
    site: &Site,
    out: &Path,
    name: &str,
    page: &str,
    steps: &[&str],
) {
    for (view, at) in [
        (&["--size", "1280x900"][..], "wide"),
        (&["--phone"], "phone"),
    ] {
        for scheme in ["light", "dark"] {
            let picture: PathBuf = out.join(format!("{name}-{at}-{scheme}.png"));
            let done = Command::new(symphony)
                .args(["shot", "--timeout", "120", &site.url(page)])
                .args(view)
                .args(["--scheme", scheme])
                .args(steps)
                .args(["--pause", "400", "--report", "--out"])
                .arg(&picture)
                .env("SYMPHONY_CHROME", browser)
                .output()
                .expect("run symphony shot");
            assert!(
                done.status.success() && picture.is_file(),
                "{}: {}\n{}",
                picture.display(),
                String::from_utf8_lossy(&done.stdout),
                String::from_utf8_lossy(&done.stderr)
            );
            eprintln!(
                "{}: {}",
                picture.display(),
                String::from_utf8_lossy(&done.stdout).trim()
            );
        }
    }
}

/// The task's timeline with its cost, the total's words said, a board's cards, the Agents page and Reports' tokens and
/// cost, from the stand-in server, wide and on a phone, light and dark: `symphony shot`, the
/// workspace's build run as an agent runs it, into the folder `SYMPHONY_TOKEN_SHOTS` names.
/// Without it the case takes none.
#[test]
fn pictures_of_the_tokens_when_a_folder_is_named() {
    let Some(out) = std::env::var_os("SYMPHONY_TOKEN_SHOTS").map(PathBuf::from) else {
        eprintln!("SYMPHONY_TOKEN_SHOTS names no folder: no pictures taken");
        return;
    };
    let Some(browser) = super::chrome::find() else {
        eprintln!("no Chrome on this machine (SYMPHONY_CHROME names one): no pictures taken");
        return;
    };
    std::fs::create_dir_all(&out).unwrap();
    let symphony = binaries::built("symphony");
    let site = pictured_site();
    let shoot = |name: &str, page: &str, steps: &[&str]| {
        shoot(&symphony, &browser, &site, &out, name, page, steps);
    };
    let timeline = "document.getElementById('timeline-slot').scrollIntoView({block: 'start'})";
    shoot(
        "task",
        PAGE,
        &["--wait-for", ".tl-cost", "--eval", timeline],
    );
    // The total's words, as the focus says them: in the legend's place wide, under the totals on
    // a phone.
    shoot(
        "task-words",
        PAGE,
        &[
            "--wait-for",
            ".tl-cost",
            "--eval",
            timeline,
            "--eval",
            "document.querySelector('.tl-cost').focus({preventScroll: true})",
        ],
    );
    shoot(
        "board",
        "/tasks",
        &[
            "--wait-for",
            ".card-cost",
            "--eval",
            "document.querySelector('[data-slug=\"tools/attempt-history-presentation\"]')?.scrollIntoView({block: 'center', inline: 'center'})",
        ],
    );
    shoot("agents", "/agents", &["--wait-for", ".agent-spent"]);
    for (name, scroll) in [
        (
            "reports",
            "document.getElementById('chart-cost')?.scrollIntoView({block: 'start'})",
        ),
        (
            "reports-summary",
            "document.querySelector('.reports-spend')?.scrollIntoView({block: 'start'})",
        ),
    ] {
        shoot(
            name,
            "/reports",
            &[
                "--wait-for",
                ".reports-spend",
                "--eval",
                CHOOSE_A_WEEK,
                "--pause",
                "3000",
                "--eval",
                scroll,
            ],
        );
    }
}
