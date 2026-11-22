//! The Runs page (apps/symphony-web/src/runs.tsx, the web client's `runs` kind), the release
//! client against the stand-in server answering the runs' reads (`/api/runs`, `/api/run`,
//! `/api/run-log`) and the machine's work: on a desk and a phone, what runs now and the list a
//! line a run within the screen's width, a run opened from its line on its steps, the tests it
//! failed and its log's tail with the output before it on demand, and back; a filter chosen asked
//! for, kept for the next visit, and Load more asking for the runs before the last shown.
use super::{READY, SHOWN, chrome, chrome::Chrome, site, site::Site, texts};
use serde_json::{Value, json};

/// Sunday 2026-11-15, 00:00 UTC.
const TODAY: f64 = 1_794_700_800.0;

/// The run id `n`, 32 hex digits as the recorder writes one.
fn id(n: u32) -> String {
    format!("{n:032x}")
}

/// A run's line as the server answers it: a test run of `sim/a` asked for `n` minutes into the
/// day, with `fields` over it.
fn line(n: u32, fields: &Value) -> Value {
    let queued = TODAY + f64::from(n) * 60.0;
    let mut line = json!({
        "schema": 1, "run": id(n), "parent": null, "project": null, "machine": null,
        "pipeline": "test", "step": null, "label": "just test-rs -p symphony-app",
        "task": "sim/a", "handle": "worker-a", "trigger": "worker-a", "branch": "sim/a",
        "commit": "0123456789abcdef0123456789abcdef01234567", "tree": null, "dirty": false,
        "queued": queued, "started": queued, "ended": null, "end": null, "exit": null,
        "signal": null, "cpu_s": null, "rerun_of": null, "log": "logs/2026-11-15/run.log",
    });
    for (key, value) in fields.as_object().expect("fields") {
        line[key] = value.clone();
    }
    line
}

/// `line` ended `end` `seconds` after it started.
fn ended(mut line: Value, end: &str, seconds: f64) -> Value {
    line["ended"] = json!(line["started"].as_f64().unwrap() + seconds);
    line["end"] = json!(end);
    line["exit"] = json!(i32::from(end != "green"));
    line
}

/// The red run's label, longer than a line of the list on a desk.
const LONG_LABEL: &str = "just test-rs -p symphony-web-tests --test browser -- runs:: header:: preload:: \
     a_missing_or_failed_page_says_which entry_pages_fit_the_first_visit_budget \
     a_first_visit_and_a_return_visit_fit_their_budgets";

/// The red run the list opens on, two steps under it, the second failing two tests.
fn red() -> Value {
    ended(
        line(30, &json!({"cpu_s": 140.0, "label": LONG_LABEL})),
        "red",
        150.0,
    )
}

fn steps() -> Value {
    let start = red()["started"].as_f64().unwrap();
    json!([
        ended(
            line(
                31,
                &json!({"parent": id(30), "step": "build", "started": start, "queued": start})
            ),
            "green",
            60.0
        ),
        ended(
            line(
                32,
                &json!({"parent": id(30), "step": "unit", "started": start + 60.0, "queued": start,
                "failed": [
                    {"suite": "symphony", "name": "api::runs::tests::a_run_opens_on_its_steps", "line": "assertion `left == right` failed: a very long line that a phone wraps rather than scrolling the page sideways"},
                    {"suite": null, "name": "project::reads", "line": "panicked at src/project.rs:12"}
                ],
                "suites": [
                    {"name": "unittests src/lib.rs (symphony_server)", "passed": 1204, "failed": 1, "ignored": 3, "s": 212.4},
                    {"name": "tests/cli/main.rs (cli)", "passed": 0, "failed": 0, "ignored": 0, "s": 41.0, "error": true}
                ]})
            ),
            "red",
            90.0,
        ),
    ])
}

/// The stand-in server answering the page's reads: three runs, the newest still running on the
/// desk machine; the red run's page; its log's tail and then the output before it.
fn served() -> Site {
    let site = site();
    site.set_json(
        symphony_api::RUNS_PATH,
        &json!({
            "runs": [line(40, &json!({"pipeline": "lint", "label": "just lint-rs"})), red(), ended(line(10, &json!({})), "green", 45.0)],
            "more": false, "pipelines": ["lint", "test"], "unread": [],
        }),
    );
    site.set_json(
        "/api/machine-work",
        &json!({"freshness": "fresh", "unassigned": [], "guidance": {}, "machines": [{
        "machine_id": "desk", "label": "desk", "producer_id": "fixture", "sequence": 1,
        "age_ms": 1200, "freshness": "fresh", "complete": true, "reason": "",
        "counts": {"running_builds": 0, "queued_builds": 0, "running_tests": 1, "queued_tests": 1},
        "operations": [
            {"machine_id": "desk", "operation_id": id(40), "parent_id": null, "handle": "worker-a",
             "task": "sim/a", "checkout": "/opaque", "recipe": "just lint-rs", "kind": "test",
             "state": "running", "resource": "run_slot", "reason": "", "phase_since": TODAY},
            {"machine_id": "desk", "operation_id": null, "parent_id": null, "handle": "worker-b",
             "task": "sim/b", "checkout": "/opaque", "recipe": "just test-rs", "kind": "test",
             "state": "queued", "resource": "run_slot", "reason": "", "phase_since": null}
        ]}]}),
    );
    site.set_json(
        symphony_api::RUN_PATH,
        &json!({"run": red(), "steps": steps(), "unread": []}),
    );
    answer_log(&site);
    site
}

/// The red run's log, 200 bytes of it: its last 120 for its first read, its first 80 the next.
fn answer_log(site: &Site) {
    let log = |from: u32, to: u32, text: String| json!({"run": id(30), "log": "logs/2026-11-15/run.log", "size": 200, "from": from, "to": to, "text": text});
    site.answer_in_turn(
        symphony_api::RUN_LOG_PATH,
        &[
            log(80, 200, format!("{}\ntest result: FAILED", "t".repeat(99))),
            log(0, 80, format!("{}\n", "c".repeat(79))),
        ],
    );
}

/// Waits until the page is ready and draws `drawn`.
fn drawn(chrome: &mut Chrome, drawn: &str) {
    assert!(
        chrome.wait_for(
            &format!(
                "{READY} && document.querySelector({}) !== null",
                json!(drawn)
            ),
            SHOWN
        ),
        "the runs page did not draw {drawn}: {:?}",
        chrome.eval("document.body ? document.body.innerText.slice(0, 1500) : '(no body)'")
    );
}

/// How far the page reaches past the screen's width, in pixels: none. A desk's scrollbar takes its
/// gutter from the width the page is laid out in, so a page narrower than the screen is none too.
const OVERFLOW: &str =
    "Math.max(0, document.documentElement.scrollWidth - document.documentElement.clientWidth)";

/// How a run's page is laid out: whether each step's time shares its name's line, how far its
/// suites' table scrolls across, on how many lines each binary's name is drawn, and what the one
/// that ended with no result says.
const LAID_OUT: &str = "(() => {
    const beside = (a, b) => { const x = a.getBoundingClientRect(), y = b.getBoundingClientRect(); return x.top < y.bottom && y.top < x.bottom; };
    const lines = (cell) => { const range = document.createRange(); range.selectNodeContents(cell.firstChild); return new Set([...range.getClientRects()].map(r => Math.round(r.top))).size; };
    const table = document.querySelector('.run-suites table');
    return {
        times: [...document.querySelectorAll('.run-step')].map(s => beside(s.querySelector('a'), s.querySelector('.run-time'))),
        scroll: table.scrollWidth - table.clientWidth,
        lines: [...table.querySelectorAll('tbody th')].map(lines),
        error: document.querySelector('.run-suite-error')?.textContent.trim(),
    };
})()";

/// How the runs' labels are drawn, in the list and on a run's page: on how many lines each is, and
/// whether it is cut short (wider than its box), with the run's page's text.
const LABELS: &str = "(() => {
    const lines = (e) => { const range = document.createRange(); range.selectNodeContents(e); return new Set([...range.getClientRects()].map(r => Math.round(r.top))).size; };
    const drawn = (e) => ({lines: lines(e), cut: e.scrollWidth > e.clientWidth});
    const head = document.querySelector('.run-head .run-label');
    return head === null
        ? [...document.querySelectorAll('.run-row .run-label')].map(drawn)
        : {wraps: lines(head) > 1, cut: head.scrollWidth > head.clientWidth, text: head.textContent};
})()";

/// A click on the first element `selector` names, as a person's.
fn click(chrome: &mut Chrome, selector: &str) {
    assert_eq!(
        chrome.eval(&format!(
            "(() => {{ const e = document.querySelector({}); if (!e) return false; e.click(); return true; }})()",
            json!(selector)
        )),
        json!(true),
        "nothing to click at {selector}"
    );
}

#[test]
fn the_runs_page_fits_a_desk_and_a_phone_and_opens_a_run_on_its_steps_failures_and_log() {
    let site = served();
    let Some(mut chrome) = chrome() else { return };
    for (width, height, phone) in [(1440, 900, false), (390, 844, true)] {
        chrome.viewport_scaled(width, height, if phone { 3 } else { 1 }, phone);
        answer_log(&site);
        chrome.open(&site.url("/runs"));
        drawn(&mut chrome, "[data-ready=runs] .run-row");
        assert_eq!(
            texts(&mut chrome, ".app-pages a.active"),
            ["Runs"],
            "{width}"
        );
        assert_eq!(
            chrome.eval(&format!(
                "({{overflow: {OVERFLOW}, rows: [...document.querySelectorAll('.run-row')].map(r => r.dataset.result), now: [...document.querySelectorAll('.runs-op')].map(o => o.dataset.state), count: document.querySelector('.runs-count')?.textContent}})"
            )),
            json!({"overflow": 0, "rows": ["running", "red", "green"], "now": ["running", "queued"], "count": "1 running · 1 queued"}),
            "{width}: the list a line a run, the newest running as the machine's work says, within the screen"
        );
        let line = |cut| json!({"lines": 1, "cut": cut});
        assert_eq!(
            chrome.eval(LABELS),
            json!([line(false), line(true), line(false)]),
            "{width}: each run's label on one line, the long one cut short"
        );
        assert!(
            chrome.wait_for(
                "document.querySelector('.runs-now')?.textContent.includes('desk · up to date') === true",
                SHOWN
            ),
            "{width}: the machine's word is said"
        );
        click(&mut chrome, ".run-row[data-result=red]");
        drawn(&mut chrome, "[data-ready=run] .run-log-text");
        assert_eq!(
            chrome.eval(&format!(
                "({{overflow: {OVERFLOW}, search: location.search, steps: [...document.querySelectorAll('.run-step-name')].map(s => s.textContent), failed: document.querySelectorAll('.run-failed-test').length, part: document.querySelector('.run-log-part').textContent, ends: document.querySelector('.run-log-text').textContent.endsWith('test result: FAILED')}})"
            )),
            json!({"overflow": 0, "search": format!("?run={}", id(30)), "steps": ["build", "unit"], "failed": 2, "part": "The last 120 B of 200 B", "ends": true}),
            "{width}: the run's page, its steps, the two tests it failed and its log's tail, within the screen"
        );
        assert_eq!(
            chrome.eval(LAID_OUT),
            json!({"times": [true, true], "scroll": 0, "lines": [1, 1], "error": "ended with no result"}),
            "{width}: each step's time on its name's line, each binary's name whole on one line, the \
             one that ended with no result saying so, and its table never scrolling across"
        );
        assert_eq!(
            chrome.eval(LABELS),
            json!({"wraps": phone, "cut": false, "text": LONG_LABEL}),
            "{width}: the run's page says its label whole, on more lines on a phone"
        );
        click(&mut chrome, ".run-log .dx-button");
        assert!(
            chrome.wait_for(
                "document.querySelector('.run-log-part')?.textContent === 'The whole log, 200 B' && document.querySelector('.run-log-text').textContent.startsWith('ccc')",
                SHOWN
            ),
            "{width}: the output before the tail goes in front of it: {:?}",
            chrome.eval("document.querySelector('.run-log')?.innerText")
        );
        assert!(
            site.targets().iter().any(|target| target
                == &format!("{}?run={}&from=0&to=80", symphony_api::RUN_LOG_PATH, id(30))),
            "{width}: the earlier output is asked for by its range: {:?}",
            site.targets()
        );
        click(&mut chrome, ".run-back");
        drawn(&mut chrome, "[data-ready=runs] .run-row");
        assert_eq!(chrome.eval("location.search"), json!(""));
    }
}

#[test]
fn a_filter_chosen_is_asked_for_and_kept_and_load_more_asks_for_the_runs_before_the_last() {
    let site = served();
    // The stand-in answers every page alike: the one Load more reads holds the runs shown.
    let green = ended(line(10, &json!({})), "green", 45.0);
    site.set_json(
        symphony_api::RUNS_PATH,
        &json!({"runs": [red(), green.clone()], "more": true, "pipelines": ["test"], "unread": []}),
    );
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport_scaled(390, 844, 3, true);
    chrome.open(&site.url("/runs"));
    drawn(&mut chrome, "[data-ready=runs] .runs-more");
    click(&mut chrome, ".runs-more");
    let before = format!(
        "{}?before={}",
        symphony_api::RUNS_PATH,
        green["queued"].as_f64().unwrap()
    );
    assert!(
        chrome.wait_for(
            &format!(
                "performance.getEntriesByType('resource').some(e => e.name.endsWith({})) && document.querySelector('.runs-more')?.textContent === 'Load more'",
                json!(before)
            ),
            SHOWN
        ),
        "Load more asks for the runs before the last shown: {:?}",
        site.targets()
    );
    assert_eq!(
        chrome.eval("[...document.querySelectorAll('.run-row')].map(r => r.dataset.run)"),
        json!([id(30), id(10)]),
        "a run the older page holds again is shown once"
    );
    // A result chosen reads its runs at once, and the next visit opens on it.
    chrome.eval(
        "(() => { const s = document.querySelector('select[name=end]'); s.value = 'red'; s.dispatchEvent(new Event('change', {bubbles: true})); })()",
    );
    assert!(
        chrome.wait_for(
            &format!(
                "performance.getEntriesByType('resource').some(e => e.name.endsWith({}))",
                json!(format!("{}?end=red", symphony_api::RUNS_PATH))
            ),
            SHOWN
        ),
        "the chosen result is asked for"
    );
    chrome.open(&site.url("/runs"));
    drawn(&mut chrome, "[data-ready=runs] .run-row");
    assert_eq!(
        chrome.eval("document.querySelector('select[name=end]').value"),
        json!("red"),
        "the next visit opens on the result chosen"
    );
    assert_eq!(chrome.eval(OVERFLOW), json!(0));
}
