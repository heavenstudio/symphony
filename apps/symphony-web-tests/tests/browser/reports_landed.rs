//! The Reports page's Landed tasks tab (apps/symphony-web/src/reports-landed.tsx), the release
//! client against the stand-in server answering `/api/reports/landed` with fourteen landed tasks of
//! every kind and none: its four figures and four charts on a wide screen and a phone, with no
//! horizontal overflow and no console error, a long task name cut within its row, the dearest ten
//! drawn and all fourteen in the table, the task given no kind grey and last; the tab named by the
//! address and reloaded onto it, reached from Over time by its link and left by the browser's Back;
//! a window with no task landed saying so in place of the charts. Over time's two new charts, the
//! cost of a landed task and of 100 lines, are drawn under Tokens and cost.
use super::{READY, SHOWN, chrome, chrome::Chrome, common, site, site::Site, texts};
use serde_json::{Value, json};

/// The page's first window, which the Landed tasks answer as, so the page rests on it.
const WINDOW: &str = "grain=hour&days=1";
/// The tab's four charts, by their ids, in the page's order.
const CHARTS: [&str; 4] = ["dearest", "kinds", "rounds", "spread"];
/// The kinds a task is given, and none, in the order the tasks take them in turn.
const KINDS: [Option<&str>; 6] = [
    Some("feature"),
    Some("fix"),
    Some("red"),
    Some("tests"),
    Some("docs"),
    None,
];
/// A task whose name is longer than a phone's row.
const LONG: &str = "app/reports-landed-cost-with-a-name-long-enough-to-run-past-a-phone";

/// Every console error and uncaught error of the page from its first script on, kept in
/// `window.__errors` for the test to read.
const ERRORS_KEPT: &str = "window.__errors = []; const said = console.error.bind(console); console.error = (...args) => { window.__errors.push(args.map(String).join(' ')); said(...args); }; addEventListener('error', event => window.__errors.push(String(event.message))); addEventListener('unhandledrejection', event => window.__errors.push(String(event.reason)));";

/// The median of `values`, sorted, as the server reads it: the middle one, or the two's mean.
fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        f64::midpoint(values[n / 2 - 1], values[n / 2])
    }
}

/// One landed task as the fixture makes it.
struct Landed {
    slug: String,
    kind: Option<&'static str>,
    first: f64,
    later: f64,
    reviews: u32,
    lines: Option<u32>,
}

impl Landed {
    fn cost(&self) -> f64 {
        self.first + self.later
    }

    fn lined(&self) -> bool {
        self.lines.is_some_and(|lines| lines > 0)
    }
}

/// The mean of `values` over `n` of them.
fn mean(values: impl Iterator<Item = f64>, n: usize) -> f64 {
    values.sum::<f64>() / f64::from(u32::try_from(n).unwrap())
}

/// Fourteen top-level tasks landed in the last 24 hours, the dearest first as the server sends
/// them: each kind in turn and one in six with none, reviewed none to five times, one landed
/// before lines were kept and one that changed no counted line; with the window's figures and its
/// groups by kind and by review rounds made from them, as the server makes them.
fn landed() -> Value {
    let fixture: Value =
        serde_json::from_str(&common::fixture("reports-landed.json")).expect("the landed fixture");
    let mut rows: Vec<Landed> = (0..14_u32)
        .zip(KINDS.iter().cycle())
        .map(|(n, kind)| Landed {
            slug: if n == 2 {
                LONG.to_owned()
            } else {
                format!("app/task-{n}")
            },
            kind: *kind,
            first: 2.0 + f64::from(14 - n) * 1.5,
            later: if n % 3 == 0 { f64::from(n) * 0.8 } else { 0.0 },
            reviews: n % 6,
            lines: match n {
                5 => None,
                9 => Some(0),
                _ => Some(40 * (n + 1)),
            },
        })
        .collect();
    rows.sort_by(|a, b| b.cost().total_cmp(&a.cost()));
    let at = fixture["to"].as_f64().unwrap() - 3600.0;
    let tasks: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({"slug": row.slug, "at": at, "kind": row.kind, "first": row.first,
                "later": row.later, "tokens": 1_200_000, "unpriced": false,
                "reviews": row.reviews, "lines": row.lines})
        })
        .collect();
    let lined: Vec<&Landed> = rows.iter().filter(|row| row.lined()).collect();
    let mut kinds: Vec<Value> = KINDS
        .iter()
        .map(|kind| {
            let of: Vec<&Landed> = rows.iter().filter(|row| row.kind == *kind).collect();
            json!({"kind": kind, "tasks": of.len(),
                "median": median(of.iter().map(|row| row.cost()).collect()),
                "cost": of.iter().map(|row| row.cost()).sum::<f64>(),
                "lined_cost": of.iter().filter(|row| row.lined()).map(|row| row.cost()).sum::<f64>(),
                "lines": of.iter().filter_map(|row| row.lines).sum::<u32>()})
        })
        .collect();
    // The kinds that cost the most first, No kind last.
    let named = kinds.len() - 1;
    kinds[..named].sort_by(|a, b| {
        b["cost"]
            .as_f64()
            .unwrap()
            .total_cmp(&a["cost"].as_f64().unwrap())
    });
    let reviews: Vec<Value> = (0..5_u32)
        .map(|group| {
            let of: Vec<&Landed> = rows
                .iter()
                .filter(|row| row.reviews.min(4) == group)
                .collect();
            json!({"reviews": group, "tasks": of.len(),
                "first": mean(of.iter().map(|row| row.first), of.len()),
                "later": mean(of.iter().map(|row| row.later), of.len()),
                "median": median(of.iter().map(|row| row.cost()).collect())})
        })
        .collect();
    let costs: Vec<f64> = rows.iter().map(Landed::cost).collect();
    let mut page = fixture;
    page["grain"] = json!("hour");
    page["days"] = json!(1);
    page["tasks"] = json!(tasks);
    page["without"] = json!(2);
    page["figures"] = json!({
        "tasks": rows.len(),
        "median": median(costs.clone()),
        "mean": mean(costs.iter().copied(), costs.len()),
        "p90": costs[1],
        "cost": costs.iter().sum::<f64>(),
        "later": rows.iter().map(|row| row.later).sum::<f64>(),
        "back": rows.iter().filter(|row| row.reviews >= 2).count(),
        "lined": lined.len(),
        "lined_cost": lined.iter().map(|row| row.cost()).sum::<f64>(),
        "lines": lined.iter().filter_map(|row| row.lines).sum::<u32>(),
        "unlined": 1,
        "unread": 1,
    });
    page["kinds"] = json!(kinds);
    page["reviews"] = json!(reviews);
    page["unread"] = json!([]);
    page
}

/// `page` as a window in which no top-level task landed.
fn none_landed(page: &Value) -> Value {
    let mut empty = page.clone();
    empty["tasks"] = json!([]);
    empty["without"] = json!(0);
    empty["kinds"] = json!([]);
    empty["reviews"] = json!([]);
    empty["kinds_from"] = Value::Null;
    for (name, value) in empty["figures"].clone().as_object().unwrap() {
        empty["figures"][name] = if value.is_u64() {
            json!(0)
        } else {
            Value::Null
        };
    }
    for name in ["cost", "later", "lined_cost"] {
        empty["figures"][name] = json!(0.0);
    }
    empty
}

/// The stand-in server: the reports' fixture by the day under every window, and `landed` as the
/// Landed tasks of the page's first window.
fn served(landed: &Value) -> Site {
    let site = site();
    let reports: Value =
        serde_json::from_str(&common::fixture("reports-page.json")).expect("the reports fixture");
    site.set_json(symphony_api::REPORTS_PATH, &reports);
    site.set_json(
        &format!("{}?{WINDOW}", symphony_api::REPORTS_LANDED_PATH),
        landed,
    );
    site
}

/// The tab opened at its address, drawn with its figures once its read answered.
fn open_landed(chrome: &mut Chrome, site: &Site) {
    chrome.open(&site.url("/reports?view=landed"));
    drawn(chrome);
}

/// Wait for the Landed tasks tab drawn from its answer, the page at rest.
fn drawn(chrome: &mut Chrome) {
    assert!(
        chrome.wait_for(
            &format!(
                "{READY} && document.querySelector('.reports[data-view=landed][data-ready=reports][aria-busy=false] .reports-figure') !== null"
            ),
            SHOWN
        ),
        "the Landed tasks tab did not draw: {:?}",
        chrome.eval("document.body ? document.body.innerText.slice(0, 1500) : '(no body)'")
    );
}

/// Each chart of the tab brought near the screen, so it mounts in place of its ghost.
fn reveal_all(chrome: &mut Chrome) {
    for id in CHARTS {
        chrome.reveal(&format!(".chart-slot:has(#chart-{id})"));
        chrome.until(
            &format!("the {id} chart drawn"),
            &format!("document.querySelector('figure#chart-{id}') !== null"),
        );
    }
}

/// What the tab draws, as the test compares it: its overflow, its tab, figures and charts, the
/// dearest chart's rows and table button, the kinds' rows with whether the last is drawn in the
/// scheme's grey (`--chart-none`), the
/// rounds' columns, the dots, and whether the long name is cut and kept within its row.
fn layout(chrome: &mut Chrome) -> Value {
    chrome.eval(
        "(() => { const label = [...document.querySelectorAll('#chart-dearest .hbar-label')].find(a => a.textContent.startsWith('app/reports-landed-cost')); const row = label.closest('.hbar').getBoundingClientRect(); const last = [...document.querySelectorAll('#chart-kinds .hbar')].at(-1); return {overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth, tab: [...document.querySelectorAll('.reports-tabs .tab[aria-current=page]')].map(a => a.textContent), figures: document.querySelectorAll('.reports-figure').length, charts: [...document.querySelectorAll('.reports-landed figure.chart')].map(f => f.id), dearest: document.querySelectorAll('#chart-dearest .hbar').length, toggle: document.querySelector('#chart-dearest .chart-table-toggle').textContent, kinds: [...document.querySelectorAll('#chart-kinds .hbar-label')].map(l => l.textContent), noKind: (() => { const probe = document.createElement('span'); probe.style.background = 'var(--chart-none)'; document.body.append(probe); const grey = getComputedStyle(probe).backgroundColor; probe.remove(); const drawn = getComputedStyle(last.querySelector('.hseg')).backgroundColor; const [r, g, b] = drawn.match(/\\d+/g); return drawn === grey && r === g && g === b; })(), rounds: [...document.querySelectorAll('#chart-rounds .cat-x b')].map(b => b.textContent), dots: document.querySelectorAll('#chart-spread .pt').length, cut: label.scrollWidth > label.clientWidth, within: label.getBoundingClientRect().right <= row.right + 0.5}; })()",
    )
}

#[test]
fn the_landed_tasks_tab_draws_four_figures_and_four_charts_wide_and_on_a_phone() {
    let site = served(&landed());
    let Some(mut chrome) = chrome() else { return };
    chrome.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": ERRORS_KEPT }),
    );
    for phone in [false, true] {
        if phone {
            chrome.viewport_scaled(390, 844, 3, true);
        } else {
            chrome.viewport(1440, 900, false);
        }
        open_landed(&mut chrome, &site);
        assert_eq!(
            texts(&mut chrome, ".reports-figure dt"),
            [
                "Landed",
                "Median a task",
                "Per 100 lines changed",
                "Back after review"
            ]
        );
        reveal_all(&mut chrome);
        let drawn = layout(&mut chrome);
        assert_eq!(
            drawn,
            json!({"overflow": 0, "tab": ["Landed tasks"], "figures": 4,
                "charts": ["chart-dearest", "chart-kinds", "chart-rounds", "chart-spread"],
                "dearest": 10, "toggle": "Show all 14 as a table",
                "kinds": drawn["kinds"].clone(), "noKind": true,
                "rounds": ["No review", "1 review", "2 reviews", "3 reviews", "4 or more"],
                "dots": 12, "cut": phone, "within": true}),
            "a long name is cut with an ellipsis on a phone alone, phone: {phone}"
        );
        let kinds = drawn["kinds"].as_array().unwrap();
        assert_eq!(kinds.len(), 6, "{drawn}");
        assert_eq!(
            kinds.last(),
            Some(&json!("No kind")),
            "No kind last: {drawn}"
        );
        // The dearest chart's table lists every task, scrolling within its chart.
        chrome.eval("document.querySelector('#chart-dearest .chart-table-toggle').click()");
        chrome.until(
            "the dearest tasks' table",
            "document.querySelectorAll('#chart-dearest .chart-table tbody tr').length === 14",
        );
        assert_eq!(
            chrome.eval(
                "document.documentElement.scrollWidth - document.documentElement.clientWidth"
            ),
            json!(0),
            "the open table scrolls within its chart, phone: {phone}"
        );
        // A tap on a dot names its task, a link to it.
        chrome.eval("document.querySelector('#chart-spread .pt').click()");
        chrome.until(
            "a dot named",
            "document.querySelector('#chart-spread .chart-readout a[href^=\"/task/\"]') !== null",
        );
    }
    let columns = chrome.eval(
        "getComputedStyle(document.querySelector('.reports-landed .reports-charts')).gridTemplateColumns.split(' ').length",
    );
    assert_eq!(columns, json!(1), "one column of charts on a phone");
    assert_eq!(
        chrome.eval("window.__errors"),
        json!([]),
        "no console error on the tab"
    );
}

#[test]
fn the_tab_is_its_address_reloaded_reached_by_its_link_and_left_by_back() {
    let site = served(&landed());
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport(1440, 900, false);
    chrome.open(&site.url("/reports"));
    assert!(
        chrome.wait_for(
            &format!(
                "{READY} && document.querySelector('.reports[data-view=time][data-ready=reports]') !== null"
            ),
            SHOWN
        ),
        "Over time drawn"
    );
    assert_eq!(
        texts(&mut chrome, ".reports-tabs .tab[aria-current=page]"),
        ["Over time"]
    );
    chrome.eval("document.querySelector('.reports-tabs .tab[href$=\"view=landed\"]').click()");
    drawn(&mut chrome);
    assert_eq!(
        chrome.eval("location.pathname + location.search"),
        json!("/reports?view=landed")
    );
    // Reloaded, the address draws the same tab.
    chrome.call("Page.reload", json!({}));
    drawn(&mut chrome);
    assert_eq!(
        texts(&mut chrome, ".reports-tabs .tab[aria-current=page]"),
        ["Landed tasks"]
    );
    // Back is Over time again.
    chrome.eval("history.back()");
    chrome.until(
        "Over time again",
        "document.querySelector('.reports[data-view=time][data-ready=reports]') !== null && location.search === ''",
    );
    // Over time draws the cost of a landed task and of 100 lines under Tokens and cost.
    for id in ["landed-cost", "line-cost"] {
        chrome.reveal(&format!(".chart-slot:has(#chart-{id})"));
    }
    assert_eq!(
        chrome.eval(
            "['landed-cost', 'line-cost'].map(id => { const f = document.querySelector('figure#chart-' + id); return [f.closest('.reports-section').getAttribute('aria-label'), f.querySelector('h3').textContent, [...f.querySelectorAll('.chart-key')].map(b => b.textContent.trim())]; })"
        ),
        json!([
            ["Tokens and cost", "Cost per landed task", ["Median", "p90", "All in"]],
            ["Tokens and cost", "Cost per 100 lines changed", ["Landed tasks", "All in"]]
        ])
    );
}

#[test]
fn a_window_with_no_task_landed_says_so_in_place_of_the_charts() {
    let site = served(&none_landed(&landed()));
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport_scaled(390, 844, 3, true);
    open_landed(&mut chrome, &site);
    assert_eq!(
        chrome.eval(
            "({empty: document.querySelector('.reports-empty')?.textContent, charts: document.querySelectorAll('.reports-landed .chart-slot').length, links: document.querySelectorAll('.reports-figure dt a').length, values: [...document.querySelectorAll('.reports-figure .value')].map(v => v.textContent), overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth})"
        ),
        json!({"empty": "No top-level task landed in the window.", "charts": 0, "links": 0,
            "values": ["–", "–", "–", "–"], "overflow": 0})
    );
}
