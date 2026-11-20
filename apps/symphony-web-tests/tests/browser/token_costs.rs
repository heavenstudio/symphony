//! The tokens and their estimated cost on the pages: a task's disclosure reads nothing until it is
//! opened, then draws the figures; the board asks the cost of the cards it drew in one read apart
//! from its listings, once they are drawn, and shows each card's figure. With a folder named, the
//! four views' pictures, wide and on a phone, light and dark.
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

#[test]
fn a_tasks_tokens_are_read_and_their_chunk_loaded_only_once_opened_on_desktop_and_phone() {
    for width in [1280, 390] {
        let site = Site::start(dist(), &common::fixture("board.json"));
        let content = look_parity::responses();
        for path in ["/api/task-page", "/api/task-history"] {
            site.set_json(path, &content[path]);
        }
        site.set_json(
            TASK_TOKENS_PATH,
            &json!({
                "total": tokens(3_000_000, 7),
                "cost": cost(12.0),
                "by_role": [{"role": "executor", "tokens": tokens(3_000_000, 7), "cost": cost(12.0)}],
                "by_round": [{"round": 1, "tokens": tokens(3_000_000, 7), "cost": cost(12.0)}],
            }),
        );
        let Some(mut page) = chrome() else {
            panic!("Chrome is required for the tokens' acceptance")
        };
        page.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":width,"height":900,"deviceScaleFactor":1,"mobile":width < 400}),
        );
        page.open(&site.url(PAGE));
        assert!(page.wait_for(
            "document.querySelector('details.task-tokens') !== null",
            SHOWN
        ));
        page.eval("document.querySelector('details.task-tokens').scrollIntoView()");
        assert!(page.wait_for("document.querySelector('.task-diff') !== null", SHOWN));
        let asked = |site: &Site| {
            (site.targets().into_iter())
                .filter(|path| path.starts_with(TASK_TOKENS_PATH) || path.contains("/task-tokens-"))
                .count()
        };
        assert_eq!(asked(&site), 0, "{width}: read before it was opened");
        assert_eq!(
            page.eval("document.getElementById('task-tokens-style') === null"),
            json!(true),
            "{width}: the tokens' sheet came with the first visit"
        );
        page.eval("document.querySelector('details.task-tokens > summary').click()");
        assert!(
            page.wait_for(
                "document.querySelector('.task-tokens-body')?.innerText.includes('3M tokens in 7 calls')",
                SHOWN
            ),
            "{width}: {:?}",
            page.eval("document.querySelector('details.task-tokens').innerText")
        );
        let said = page.eval("document.querySelector('.task-tokens-body').innerText");
        let said = said.as_str().unwrap_or_default();
        assert!(
            said.contains("$12.00, estimated at API list prices read 2026-11-16, not money spent"),
            "{said}"
        );
        assert!(said.contains("round 1"), "{said}");
        // The chunk's sheet is in the page and drawn by: a row's name at the body's weight and
        // level with its figures, right-aligned and never wrapped, where the shared table's own
        // rules say none of it; and the whole table in its box, with no scroll across it.
        let drawn = page.eval(
            "(() => { const name = getComputedStyle(document.querySelector('.task-tokens-body tbody th'));
                const figure = getComputedStyle(document.querySelector('.task-tokens-body tbody td'));
                const table = document.querySelector('.task-tokens-body table');
                return {sheets: document.querySelectorAll('#task-tokens-style').length, weight: name.fontWeight,
                    level: name.verticalAlign, align: figure.textAlign, wrap: figure.whiteSpace,
                    whole: table.scrollWidth <= table.clientWidth}; })()",
        );
        assert_eq!(
            drawn,
            json!({"sheets": 1, "weight": "400", "level": "top", "align": "right", "wrap": "nowrap", "whole": true}),
            "{width}: the tokens' sheet is not drawn by"
        );
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

/// A task's tokens in two roles and two rounds, some of a model the table has no price for.
fn pictured_tokens() -> Value {
    let executor = [
        kinds(1_200, 45_000, 1_150_000, 18_000, 64),
        priced(4.85, &[("gpt-x", 96_000)]),
    ];
    let reviewer = [kinds(400, 12_000, 290_000, 6_500, 21), priced(1.12, &[])];
    json!({
        "total": kinds(1_600, 57_000, 1_440_000, 24_500, 85),
        "cost": priced(5.97, &[("gpt-x", 96_000)]),
        "by_role": [
            {"role": "executor", "tokens": executor[0], "cost": executor[1]},
            {"role": "reviewer", "tokens": reviewer[0], "cost": reviewer[1]},
        ],
        "by_round": [
            {"round": 1, "tokens": executor[0], "cost": executor[1]},
            {"round": 2, "tokens": reviewer[0], "cost": reviewer[1]},
        ],
    })
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
    site.set_json(TASK_TOKENS_PATH, &pictured_tokens());
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

/// The task page's disclosure opened, a board's cards, the Agents page and Reports' tokens and
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
    shoot(
        "task",
        PAGE,
        &[
            "--wait-for",
            "details.task-tokens",
            "--click",
            "details.task-tokens > summary",
            "--pause",
            "600",
            "--eval",
            "document.querySelector('details.task-tokens').scrollIntoView({block: 'start'})",
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
