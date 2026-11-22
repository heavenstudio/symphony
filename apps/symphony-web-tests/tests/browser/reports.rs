//! The Reports page (apps/symphony-web/src/reports.tsx, the web client's `reports` kind), the
//! release client against the stand-in server answering `/api/reports` with a fortnight by the
//! day: a phone's width held, one tap showing a period's every value on every chart, a day no file
//! recorded told apart from a zero, the arrow keys moving between periods, a window's many
//! changes of the settings and the agents' instructions (a skill, the rules), each marked
//! alike, folded past the newest three so its first chart shows on a phone's first screen, two
//! columns on a wide screen, the dark scheme's own colours, the empty state saying when
//! the first roll-up comes, the last 24 hours with nothing chosen, and the window and the series
//! hidden kept for the next visit, a hidden series' room given to the rest. A chart's address
//! (`#chart-ID`) lands that chart just below the header, opened fresh (its charts drawn after the
//! shell looked for it) or followed in place from its figure, on a desk and a phone; a reader who
//! scrolls away from it is left there as the hour slides and a new window draws, and one who chose
//! a window before any chart drew (nothing recorded, or a failed read) is not taken to it. A
//! return visit draws the window this device kept before its read answers, checks it with the
//! kept tag (a `304` for one unchanged) and keeps a changed answer in its place; a read that fails
//! leaves the kept window drawn and said stale until a retry answers. A kept window chosen again
//! on the page draws from the device the same way, said as checking whatever another window's
//! read did before; on a phone the notice that says so leaves the page's end clear and the toasts
//! above it.
use super::cache_pages::{PARTS, keeps, kept, notice, shows, store_opened};
use super::document_landing::{COUNT_SCROLLS, TWO_FRAMES};
use super::{
    READY, SHOWN, chrome, chrome::Chrome, common, components::key, site, site::Site, texts,
};
use serde_json::{Value, json};

const DAY: f64 = 86_400.0;
/// The fixture's first day, the window's thirteenth.
const RECORDED: f64 = 1_799_884_800.0;
const FROM: f64 = RECORDED - 12.0 * DAY;
/// The days no file recorded: hatched, never drawn as zeros.
const MISSING: [usize; 2] = [3, 8];

/// A fortnight by the day, as the server answers the page's first read: the wire fixture's
/// recorded day on most days, each with its own count of tasks done, its agents-only last day
/// as today, and two days with no file at all.
fn fortnight() -> Value {
    let fixture: Value =
        serde_json::from_str(&common::fixture("reports-page.json")).expect("the reports fixture");
    let buckets: Vec<Value> = (0..14_u32)
        .map(|day| {
            let start = FROM + f64::from(day) * DAY;
            let at = usize::try_from(day).unwrap();
            let mut bucket = if day == 13 {
                fixture["buckets"][1].clone()
            } else if MISSING.contains(&at) {
                json!({"rolled": 0, "flow": null, "decisions": null, "git": null, "waits": null,
                    "lead": null, "verdicts": null, "red_fix": null, "agents": null, "health": null})
            } else {
                let mut recorded = fixture["buckets"][0].clone();
                recorded["flow"]["done"] = json!(day % 5 + 1);
                recorded
            };
            bucket["start"] = json!(start);
            bucket["end"] = json!(if day == 13 {
                fixture["to"].clone()
            } else {
                json!(start + DAY)
            });
            bucket
        })
        .collect();
    let mut page = fixture;
    page["days"] = json!(14);
    page["from"] = json!(FROM);
    page["buckets"] = json!(buckets);
    page["first"] = json!(FROM);
    page["marks"] = json!([{"at": FROM + 6.0 * DAY + 3600.0, "commit": "4f1c0a9b2d3e",
        "subject": "settings: land without the check"}]);
    page["unread"] = json!([]);
    page
}

/// `page` with no file recorded in any of its periods and no settings change: a window the
/// roll-up has nothing in, which draws no chart.
fn nothing_recorded(page: &Value) -> Value {
    let mut empty = page.clone();
    for bucket in empty["buckets"].as_array_mut().unwrap() {
        for part in [
            "flow",
            "decisions",
            "git",
            "waits",
            "lead",
            "verdicts",
            "red_fix",
            "agents",
            "health",
        ] {
            bucket[part] = Value::Null;
        }
        bucket["rolled"] = json!(0);
    }
    empty["marks"] = json!([]);
    empty
}

fn served(page: &Value) -> Site {
    let site = site();
    site.set_json(symphony_api::REPORTS_PATH, page);
    site
}

fn open(chrome: &mut Chrome, site: &Site, drawn: &str) {
    chrome.open(&site.url("/reports"));
    assert!(
        chrome.wait_for(
            &format!(
                "{READY} && document.querySelector({}) !== null",
                json!(drawn)
            ),
            SHOWN
        ),
        "the reports page did not draw {drawn}: {:?}",
        chrome.eval("document.body ? document.body.innerText.slice(0, 1500) : '(no body)'")
    );
}

/// A person's tap on the `at`th period of the first chart: brought to the middle of the screen,
/// pressed and released at its centre, where it is the column on top.
fn tap(chrome: &mut Chrome, at: usize) {
    let point = chrome.eval(&format!(
        "(() => {{ const e = document.querySelectorAll('.chart')[0].querySelectorAll('.chart-col')[{at}]; e.scrollIntoView({{block: 'center'}}); const b = e.getBoundingClientRect(); const x = b.left + b.width / 2, y = b.top + b.height / 2; return {{x, y, hit: document.elementFromPoint(x, y)?.closest('.chart-col') === e}}; }})()"
    ));
    assert_eq!(point["hit"], json!(true), "period {at} is covered: {point}");
    for kind in ["mousePressed", "mouseReleased"] {
        chrome.call(
            "Input.dispatchMouseEvent",
            json!({"type": kind, "x": point["x"], "y": point["y"], "button": "left", "clickCount": 1}),
        );
    }
}

/// The period each drawn chart shows (`null` where none is lit), and the first chart's readout.
fn shown(chrome: &mut Chrome) -> Value {
    chrome.eval(
        "(() => { const charts = [...document.querySelectorAll('.chart')]; return {lit: charts.map(c => [...c.querySelectorAll('.chart-col')].findIndex(col => col.classList.contains('on'))), readout: charts[0].querySelector('.chart-readout').innerText.replace(/\\s+/g, ' ').trim()}; })()",
    )
}

#[test]
fn the_reports_page_fits_a_phone_and_a_tap_shows_a_period_s_every_value_on_every_chart() {
    let site = served(&fortnight());
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport_scaled(390, 844, 3, true);
    open(&mut chrome, &site, "[data-ready=reports] .chart-plot");
    assert_eq!(texts(&mut chrome, ".app-pages a.active"), ["Reports"]);
    assert_eq!(
        chrome.eval(
            "({overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth, figures: document.querySelectorAll('.reports-figure').length, columns: getComputedStyle(document.querySelector('.reports-charts')).gridTemplateColumns.split(' ').length, periods: document.querySelector('.chart').querySelectorAll('.chart-col').length, gaps: [...document.querySelector('.chart').querySelectorAll('.chart-col')].flatMap((c, n) => c.classList.contains('gap') ? [n] : [])})"
        ),
        json!({"overflow": 0, "figures": 4, "columns": 1, "periods": 14, "gaps": [MISSING[0], MISSING[1], 13]}),
        "one column of charts within the phone's width, a column a day, the days with no roll-up \
         hatched: the two with no file and today, which has the agents' lines alone"
    );
    assert_eq!(
        texts(&mut chrome, ".reports-figure dt"),
        ["Done", "Lead time", "Agents at work", "Open decisions"]
    );
    // Before any tap the charts show the last period they hold.
    let before = shown(&mut chrome);
    assert!(
        before["lit"]
            .as_array()
            .unwrap()
            .iter()
            .all(|at| at == 12 || at == 13),
        "{before}"
    );
    tap(&mut chrome, 5);
    assert!(
        chrome.wait_for(
            "[...document.querySelectorAll('.chart')].every(c => c.querySelectorAll('.chart-col')[5].classList.contains('on'))",
            SHOWN
        ),
        "every drawn chart shows the period tapped: {}",
        shown(&mut chrome)
    );
    let after = shown(&mut chrome);
    assert!(
        after["readout"].as_str().unwrap().starts_with("Thu 7 Jan"),
        "{after}"
    );
    assert!(
        after["readout"].as_str().unwrap().contains("Done 1"),
        "the tapped day's value, as text: {after}"
    );
    tap(&mut chrome, MISSING[0]);
    assert!(
        chrome.wait_for(
            "document.querySelector('.chart .chart-readout').innerText.includes('no data recorded')",
            SHOWN
        ),
        "a day no file recorded says so: {}",
        shown(&mut chrome)
    );
    // The arrow keys move between periods on the plot that has the focus.
    chrome.eval("document.querySelector('.chart .chart-plot').focus()");
    for kind in ["keyDown", "keyUp"] {
        chrome.call(
            "Input.dispatchKeyEvent",
            json!({"type": kind, "key": "ArrowLeft", "code": "ArrowLeft", "windowsVirtualKeyCode": 37, "nativeVirtualKeyCode": 37}),
        );
    }
    assert!(
        chrome.wait_for(
            &format!(
                "document.querySelector('.chart').querySelectorAll('.chart-col')[{}].classList.contains('on')",
                MISSING[0] - 1
            ),
            SHOWN
        ),
        "ArrowLeft shows the day before: {}",
        shown(&mut chrome)
    );
    // A chart's table holds what it draws, a row a day.
    chrome.eval("document.querySelector('.chart .chart-table-toggle').click()");
    assert!(
        chrome.wait_for(
            "document.querySelectorAll('.chart .chart-table tbody tr').length === 14",
            SHOWN
        ),
        "the table's rows: {:?}",
        chrome.eval("document.querySelector('.chart .chart-table')?.innerText")
    );
    assert_eq!(
        chrome.eval("document.documentElement.scrollWidth - document.documentElement.clientWidth"),
        json!(0),
        "the open table scrolls within its chart"
    );
}

#[test]
fn on_a_wide_screen_the_charts_are_two_columns_and_dark_takes_its_own_colours() {
    let site = served(&fortnight());
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport(1440, 900, false);
    open(&mut chrome, &site, "[data-ready=reports] .chart-plot");
    assert_eq!(
        chrome.eval(
            "(() => { const [a, b] = [...document.querySelectorAll('.reports-section')[0].querySelectorAll('.chart')].map(c => c.getBoundingClientRect()); return {columns: getComputedStyle(document.querySelector('.reports-charts')).gridTemplateColumns.split(' ').length, sideBySide: Math.abs(a.top - b.top) < 1 && b.left > a.right, figuresOnOneRow: new Set([...document.querySelectorAll('.reports-figure')].map(f => Math.round(f.getBoundingClientRect().top))).size, overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth}; })()"
        ),
        json!({"columns": 2, "sideBySide": true, "figuresOnOneRow": 1, "overflow": 0})
    );
    let colours = "['--chart-1', '--chart-5'].map(name => getComputedStyle(document.documentElement).getPropertyValue(name).trim())";
    for (scheme, expected) in [
        ("light", ["#2a78d6", "#e87ba4"]),
        ("dark", ["#3987e5", "#d55181"]),
    ] {
        chrome.scheme(scheme);
        assert_eq!(chrome.eval(colours), json!(expected), "{scheme}");
    }
}

/// The days of [`fortnight`] its ten marked changes fall on, the last three before today, six on
/// the middle one: as this repository's own fortnight stood on 13 Nov, symphony.toml changed six
/// times on 12 Nov alone.
const CHANGED: [u32; 10] = [10, 10, 11, 11, 11, 11, 11, 11, 12, 12];

/// [`fortnight`] with the ten changes of [`CHANGED`], oldest first as the server sends them, the
/// `n`th at `n + 1` o'clock, each subject as long as a merge's; the oldest names a 64-character
/// hash, a word no line of a phone holds. Each says what it changed ([`what`]), but the oldest,
/// written as a server from before a mark named any wrote it: a settings change.
fn changed_often() -> Value {
    let mut page = fortnight();
    page["marks"] = (0_u32..)
        .zip(CHANGED)
        .map(|(n, day)| {
            let subject = if n == 0 {
                format!(
                    "Pin the board's SurrealDB at {}",
                    "0123456789abcdef".repeat(4)
                )
            } else {
                format!("Merge branch 'tools/settings-change-{n}': the board's limits and models")
            };
            let mut mark = json!({"at": FROM + f64::from(day) * DAY + 3600.0 * f64::from(n + 1),
                "commit": format!("{n}f1c0a9b2d3e"), "subject": subject});
            if n > 0 {
                mark["changed"] = what(n).split(", ").collect();
            }
            mark
        })
        .collect();
    page
}

/// What the `n`th change of [`changed_often`] changed, as the page says it: the settings, but the
/// work skill with the rules for the eighth and two skills for the ninth, the two newest but one.
fn what(n: u32) -> &'static str {
    match n {
        7 => "rules, skill work",
        8 => "skill review-task, skill tasks",
        _ => "settings",
    }
}

/// On a phone the list of a window's ten changes, of the settings, the rules and skills, shows the
/// newest three and folds the rest under a disclosure, so the first chart starts on the first
/// screen; a period with six says each in its readout.
#[test]
fn on_a_phone_a_window_s_many_changes_fold_and_the_first_chart_shows_on_the_first_screen() {
    let site = served(&changed_often());
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport_scaled(390, 844, 3, true);
    open(&mut chrome, &site, "[data-ready=reports] .chart-plot");
    // The first chart's place on the page against the screen's height.
    let first = chrome.eval(
        "(() => { const c = document.querySelector('.chart').getBoundingClientRect(); return {top: Math.round(c.top + scrollY), screen: innerHeight}; })()",
    );
    assert!(
        first["top"].as_i64().unwrap() < first["screen"].as_i64().unwrap(),
        "the first chart starts on the first screen: {first}"
    );
    // The list's words read as drawn (`textContent`), whether the newest show whole or cut to a
    // line, and whether the folded ones show (a closed `details` keeps its content's boxes,
    // hidden: `checkVisibility`).
    let marks = "(() => { const m = document.querySelector('.reports-marks'); const d = m.querySelector('details'); const newest = [...m.querySelectorAll(':scope > ul > li')]; return {listed: newest.map(li => li.textContent), whole: newest.every(li => li.scrollWidth <= li.clientWidth), more: d?.querySelector('summary').textContent ?? null, open: d?.open ?? null, folded: d ? [...d.querySelectorAll('li')].map(li => li.textContent.split(':')[0]) : null, drawn: d ? d.querySelector('ul').checkVisibility() : null, overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth}; })()";
    let mark = |n: u32, day: &str| {
        format!(
            "{day}: {} changed, Merge branch 'tools/settings-change-{n}': the board's limits and models ({n}f1c0a9b2d3e)",
            what(n)
        )
    };
    let folded = json!([
        "Wed 13 Jan",
        "Wed 13 Jan",
        "Wed 13 Jan",
        "Wed 13 Jan",
        "Wed 13 Jan",
        "Tue 12 Jan",
        "Tue 12 Jan"
    ]);
    assert_eq!(
        chrome.eval(marks),
        json!({"listed": [mark(9, "Thu 14 Jan"), mark(8, "Thu 14 Jan"), mark(7, "Wed 13 Jan")],
            "whole": false, "more": "7 more changes", "open": false, "folded": folded,
            "drawn": false, "overflow": 0}),
        "the newest three, newest first and cut to a line each, a settings change, two skills' \
         and a skill's with the rules' each saying what it changed, and the other seven, newest \
         first, folded"
    );
    // A skill's change is marked on the plot as a settings change is: one dashed line each, of
    // one look, and its title says what changed.
    assert_eq!(
        chrome.eval(
            "(() => { const marks = [...document.querySelector('.chart').querySelectorAll('.chart-mark')]; const look = m => { const s = getComputedStyle(m); return [s.borderLeftStyle, s.borderLeftWidth, s.borderLeftColor, s.opacity].join(' '); }; return {marks: marks.length, looks: new Set(marks.map(look)).size, style: getComputedStyle(marks[0]).borderLeftStyle, skill: marks[7].title.split(' changed, ')[0], settings: marks[9].title.split(' changed, ')[0], oldest: marks[0].title.split(' changed, ')[0]}; })()"
        ),
        json!({"marks": 10, "looks": 1, "style": "dashed", "skill": "rules, skill work",
            "settings": "settings", "oldest": "settings"}),
        "ten marks on the first chart, a skill's drawn as the settings' are"
    );
    chrome.eval("document.querySelector('.reports-marks summary').click()");
    assert!(
        chrome.wait_for(
            "document.querySelector('.reports-marks details').open && document.querySelector('.reports-marks details ul').checkVisibility()",
            SHOWN
        ),
        "the disclosure opens on the other seven: {}",
        chrome.eval(marks)
    );
    let opened = chrome.eval(marks);
    assert_eq!(
        (&opened["whole"], &opened["overflow"]),
        (&json!(true), &json!(0)),
        "opened, every change shows whole, the newest three too, within the phone's width, the \
         hash's line broken: {opened}"
    );
    // The period with six says each of them when tapped.
    tap(&mut chrome, usize::try_from(CHANGED[2]).unwrap());
    assert!(
        chrome.wait_for(
            "document.querySelector('.chart .chart-readout').textContent.startsWith('Wed 13 Jan')",
            SHOWN
        ),
        "the tapped day's readout: {}",
        shown(&mut chrome)
    );
    assert_eq!(
        chrome.eval(
            "[...document.querySelector('.chart .chart-readout').querySelectorAll('.chart-mark-note')].map(p => p.textContent.split(' (')[1])"
        ),
        json!(["2f1c0a9b2d3e)", "3f1c0a9b2d3e)", "4f1c0a9b2d3e)", "5f1c0a9b2d3e)", "6f1c0a9b2d3e)", "7f1c0a9b2d3e)"]),
        "each of the day's six changes, oldest first"
    );
    assert_eq!(
        chrome.eval(
            "[...document.querySelector('.chart .chart-readout').querySelectorAll('.chart-mark-note')].map(p => p.textContent.split(' changed, ')[0])"
        ),
        json!(["Settings", "Settings", "Settings", "Settings", "Settings", "Rules, skill work"]),
        "each saying what it changed"
    );
}

/// [`fortnight`] with Ready far over the other open stages each day it was rolled up, as the live
/// board's Open work stood on 14 Nov: 107 ready over 2 to 10 in each other stage.
fn crowded() -> Value {
    let mut page = fortnight();
    for bucket in page["buckets"].as_array_mut().unwrap() {
        if bucket["flow"].is_object() {
            bucket["flow"]["held"]["ready"] = json!(107);
        }
    }
    page
}

/// The windows the page asked the reports for so far, each its query.
fn asked(site: &Site) -> Vec<String> {
    site.targets()
        .iter()
        .filter_map(|target| target.strip_prefix(&format!("{}?", symphony_api::REPORTS_PATH)))
        .map(str::to_owned)
        .collect()
}

/// Open work's axis and legend: the top of its axis, each legend button's name with whether it is
/// pressed (shown), and whether any part of a bar wears Ready's colour. Its words are read from the
/// page as drawn (`textContent`), never as laid out (`innerText`): a chart just mounted sits in a
/// `content-visibility: auto` slot whose contents Chrome skips until its next rendering update,
/// which lays them out before it paints, and meanwhile every `innerText` in it is empty.
fn open_work(chrome: &mut Chrome) -> Value {
    chrome.eval(
        "(() => { const c = document.querySelector('#chart-open'); if (!c) return null; const ticks = [...c.querySelectorAll('.chart-y span')]; return {top: ticks.length ? ticks[ticks.length - 1].textContent : null, keys: [...c.querySelectorAll('.chart-key')].map(b => [b.textContent.trim(), b.getAttribute('aria-pressed')]), ready_drawn: [...c.querySelectorAll('.seg')].some(s => s.getAttribute('style').includes('--chart-1)'))}; })()",
    )
}

/// Ready's button in Open work's legend focused, and Space pressed on it, as a keyboard reaches it.
fn toggle_ready(chrome: &mut Chrome) {
    assert_eq!(
        chrome.eval(
            "(() => { const b = [...document.querySelectorAll('#chart-open .chart-key')].find(b => b.textContent.trim() === 'Ready'); b?.focus(); return document.activeElement === b; })()"
        ),
        json!(true),
        "Ready's button takes the focus"
    );
    key(chrome, " ", "Space", 32);
}

/// The page opens on the last 24 hours when this browser chose nothing; the window and a series
/// hidden are this browser's on the next visit; hiding Open work's Ready, far over the other
/// stages, scales its axis to the rest, and showing it again puts it back.
#[test]
fn the_window_and_the_series_hidden_are_kept_and_hiding_a_large_series_rescales_its_axis() {
    let site = served(&crowded());
    // The last 24 hours answered as that window (the fortnight's periods under its name), so the
    // page rests on it and draws Days only from Days' own answer.
    let mut hours = crowded();
    hours["grain"] = json!("hour");
    hours["days"] = json!(1);
    site.set_json(
        &format!("{}?grain=hour&days=1", symphony_api::REPORTS_PATH),
        &hours,
    );
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport(1440, 900, false);
    open(
        &mut chrome,
        &site,
        "[data-ready=reports][aria-busy=false] #chart-open .chart-plot",
    );
    assert_eq!(
        asked(&site),
        ["grain=hour&days=1"],
        "nothing chosen: the last 24 hours"
    );
    assert_eq!(
        texts(&mut chrome, ".reports-controls [aria-pressed=true]"),
        ["Hours", "24 hours"]
    );
    chrome.eval("document.querySelector('.reports-controls [data-grain=day]').click()");
    assert!(
        chrome.wait_for(
            "document.querySelector('.reports[aria-busy=false] #chart-open .chart-plot') !== null",
            SHOWN
        ),
        "Days drew the fortnight: {:?}",
        asked(&site)
    );
    assert_eq!(asked(&site), ["grain=hour&days=1", "grain=day&days=14"]);
    let before = open_work(&mut chrome);
    assert_eq!(
        before,
        json!({"top": "150", "ready_drawn": true, "keys": [["Ready", "true"], ["In progress", "true"], ["Review", "true"], ["Blocked", "true"]]}),
        "Ready's 107 sets the axis"
    );
    toggle_ready(&mut chrome);
    assert!(
        chrome.wait_for(
            "document.querySelector('#chart-open .chart-y span:last-child')?.textContent === '6'",
            SHOWN
        ),
        "Ready hidden, the axis is the other stages': {}",
        open_work(&mut chrome)
    );
    let hidden = json!({"top": "6", "ready_drawn": false, "keys": [["Ready", "false"], ["In progress", "true"], ["Review", "true"], ["Blocked", "true"]]});
    assert_eq!(open_work(&mut chrome), hidden);
    // The next visit asks for the window chosen and hides what was hidden.
    chrome.open("about:blank");
    open(
        &mut chrome,
        &site,
        "[data-ready=reports][aria-busy=false] #chart-open .chart-plot",
    );
    assert_eq!(
        asked(&site).last().map(String::as_str),
        Some("grain=day&days=14"),
        "{:?}",
        asked(&site)
    );
    assert_eq!(
        texts(&mut chrome, ".reports-controls [aria-pressed=true]"),
        ["Days", "14 days"]
    );
    assert_eq!(open_work(&mut chrome), hidden, "Ready still hidden");
    toggle_ready(&mut chrome);
    assert!(
        chrome.wait_for(
            "document.querySelector('#chart-open .chart-y span:last-child')?.textContent === '150'",
            SHOWN
        ),
        "Ready shown again: {}",
        open_work(&mut chrome)
    );
    assert_eq!(open_work(&mut chrome), before);
}

#[test]
fn with_no_roll_up_yet_the_reports_page_says_when_the_first_comes() {
    let mut empty = nothing_recorded(&fortnight());
    empty["first"] = Value::Null;
    let site = served(&empty);
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport_scaled(390, 844, 3, true);
    open(&mut chrome, &site, "[data-ready=reports] .reports-empty");
    let said = chrome.eval("document.querySelector('.reports-empty').innerText");
    assert!(
        said.as_str()
            .unwrap()
            .contains("the next is due by 09:00 UTC"),
        "{said}"
    );
    assert_eq!(
        chrome.eval("document.querySelectorAll('.chart-plot').length"),
        json!(0),
        "no chart of nothing"
    );
}

/// The chart the address names: Decisions, the first of the page's third section, eight charts
/// above it, so the scroll to it brings charts near that draw after it.
const ADDRESSED: &str = "chart-decisions";

/// `page` as the last 24 hours answer it, the window a browser that chose none opens on: its
/// periods under that window's name, so the page rests on it.
fn as_hours(page: &Value) -> Value {
    let mut hours = page.clone();
    hours["grain"] = json!("hour");
    hours["days"] = json!(1);
    hours
}

/// The target of the last 24 hours' read.
fn hours_read() -> String {
    format!("{}?grain=hour&days=1", symphony_api::REPORTS_PATH)
}

/// [`fortnight`] served as the last 24 hours ([`as_hours`]) and as Days' fortnight.
fn both_windows() -> Site {
    let site = served(&fortnight());
    site.set_json(&hours_read(), &as_hours(&fortnight()));
    site
}

/// `page` once the hour slid, as the next read of the same window answers it: every period, the
/// window's ends and its settings change an hour on, the change's subject saying so. Its first
/// period starts at another second, so the page draws it as a window of its own, every chart
/// mounted again.
fn slid(page: &Value) -> Value {
    let later = |value: &Value| json!(value.as_f64().expect("a second") + 3600.0);
    let mut next = page.clone();
    for key in ["from", "to"] {
        next[key] = later(&next[key]);
    }
    for bucket in next["buckets"].as_array_mut().expect("its periods") {
        for key in ["start", "end"] {
            bucket[key] = later(&bucket[key]);
        }
    }
    let at = later(&page["marks"][0]["at"]);
    next["marks"] =
        json!([{"at": at, "commit": "5a2b3c4d5e6f", "subject": "settings: the hour slid"}]);
    next
}

/// Where the addressed chart sits: the top of the element its id names (the chart once drawn, its
/// ghost's mark before), whether that is the chart drawn, the header's bottom, the page's scroll
/// padding, how far down the page the chart is, the page's scroll and the address's fragment.
fn place() -> String {
    format!(
        "(() => {{ const chart = document.getElementById({id});
        return {{top: chart?.getBoundingClientRect().top ?? null, drawn: chart?.matches('figure.chart') ?? false,
            header: document.querySelector('.app-header').getBoundingClientRect().bottom,
            padding: parseFloat(getComputedStyle(document.documentElement).scrollPaddingTop),
            down: chart ? chart.getBoundingClientRect().top + scrollY : null, y: scrollY, hash: location.hash}}; }})()",
        id = json!(ADDRESSED)
    )
}

/// True once the addressed chart is drawn and has held still for ten frames with nothing near the
/// screen still to draw (a reserved box within its sentinel's 600 px) and nothing busy; false after
/// sixty frames without that, for the wait to look again.
fn still() -> String {
    format!(
        "new Promise(done => {{
        let last = null, still = 0, frames = 0;
        const near = box => {{ const r = box.getBoundingClientRect(); return r.bottom > -600 && r.top < innerHeight + 600; }};
        const look = () => {{
            const chart = document.querySelector({chart});
            const top = chart ? chart.getBoundingClientRect().top : null;
            const waiting = document.querySelector('[aria-busy=true]') !== null || [...document.querySelectorAll('[data-pending=true]')].some(near);
            still = top !== null && top === last && !waiting ? still + 1 : 0;
            last = top;
            if (still >= 10) return done(true);
            if (++frames >= 60) return done(false);
            requestAnimationFrame(look);
        }};
        requestAnimationFrame(look);
    }})",
        chart = json!(format!("figure.chart#{ADDRESSED}"))
    )
}

/// Arrive at the addressed chart in a browser `width` by `height`: opened at its address with the
/// reports' read held until the shell has looked for it (`in_place` false), or by a click on its
/// figure's link once the page is drawn. Says where the chart is once the page around it has drawn,
/// and a moment later.
fn arrive(site: &Site, width: u32, height: u32, in_place: bool) -> Option<[Value; 2]> {
    let mut chrome = chrome()?;
    chrome.viewport_scaled(width, height, if width < 600 { 3 } else { 1 }, width < 600);
    let way = if in_place { "in place" } else { "opened" };
    if in_place {
        let link = format!("a[href=\"#{ADDRESSED}\"]");
        chrome.open(&site.url("/reports"));
        assert!(
            chrome.wait_for(
                &format!(
                    "{READY} && document.querySelector({}) !== null",
                    json!(format!(".reports[aria-busy=false] {link}"))
                ),
                SHOWN
            ),
            "{width}px {way}: the figure's link is drawn: {}",
            chrome.eval(&place())
        );
        chrome.eval(&format!("document.querySelector({}).click()", json!(link)));
    } else {
        site.hold_route(symphony_api::REPORTS_PATH, true);
        chrome.open(&site.url(&format!("/reports#{ADDRESSED}")));
        // The shell has looked for the chart (the navigation is ready once it has) while the page
        // shows its ghost: its charts come after.
        assert!(
            chrome.wait_for(
                &format!("{READY} && document.querySelector('.reports[aria-busy=true] .ghost-chart') !== null"),
                SHOWN
            ),
            "{width}px {way}: the shell is ready over the page's ghost: {}",
            chrome.eval(&place())
        );
        site.hold_route(symphony_api::REPORTS_PATH, false);
    }
    assert!(
        chrome.wait_for(&still(), SHOWN),
        "{width}px {way}: the chart and the page around it draw and hold still: {}",
        chrome.eval(&place())
    );
    let drawn = chrome.eval(&place());
    chrome.pump(std::time::Duration::from_millis(600));
    Some([drawn, chrome.eval(&place())])
}

#[test]
fn a_chart_s_address_lands_it_below_the_header_opened_fresh_or_followed_from_its_figure() {
    let site = both_windows();
    for (width, height) in [(1280, 900), (390, 844)] {
        for in_place in [false, true] {
            let Some([drawn, later]) = arrive(&site, width, height, in_place) else {
                return;
            };
            let way = if in_place { "in place" } else { "opened" };
            eprintln!("REPORTS LANDING {width}px {way}: drawn {drawn}, later {later}");
            let number = |place: &Value, key: &str| place[key].as_f64().unwrap_or(f64::NAN);
            for place in [&drawn, &later] {
                assert_eq!(
                    (&place["hash"], &place["drawn"]),
                    (&json!(format!("#{ADDRESSED}")), &json!(true)),
                    "{width}px {way}: at the chart's address, the chart drawn: {place}"
                );
                assert!(
                    (number(place, "top") - number(place, "padding")).abs() <= 1.0
                        && number(place, "top") >= number(place, "header"),
                    "{width}px {way}: the chart's top sits at the scroll padding, below the header: drawn {drawn}, a moment later {later}"
                );
            }
        }
    }
}

#[test]
fn a_reader_who_scrolls_away_from_a_landed_chart_is_left_there_as_the_hour_slides_and_a_new_window_draws()
 {
    let site = both_windows();
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport(1280, 900, false);
    chrome.open(&site.url(&format!("/reports#{ADDRESSED}")));
    assert!(
        chrome.wait_for(&still(), SHOWN),
        "the page lands on the chart and holds still: {}",
        chrome.eval(&place())
    );
    let landed = chrome.eval(&place());
    let number = |place: &Value, key: &str| place[key].as_f64().unwrap_or(f64::NAN);
    assert!(
        (number(&landed, "top") - number(&landed, "padding")).abs() <= 1.0,
        "the chart is at the scroll padding: {landed}"
    );
    chrome.call(
        "Input.dispatchMouseEvent",
        json!({"type": "mouseWheel", "x": 640, "y": 450, "deltaX": 0, "deltaY": -500}),
    );
    assert!(
        chrome.wait_for(&format!("scrollY < {} - 250", number(&landed, "y")), SHOWN),
        "the reader scrolls up from the chart: {}",
        chrome.eval(&place())
    );
    chrome.eval(TWO_FRAMES);
    let left = chrome.eval(&place());
    chrome.eval(COUNT_SCROLLS);
    // The hour slides: the same window read again answers its next hour, a window of its own.
    site.set_json(&hours_read(), &slid(&as_hours(&fortnight())));
    chrome.eval("document.querySelector('.reports-controls [data-days=\"1\"]').click()");
    assert!(
        chrome.wait_for(
            "document.querySelector('.reports-marks')?.textContent.includes('the hour slid') === true",
            SHOWN
        ) && chrome.wait_for(&still(), SHOWN),
        "the slid hour draws and the page holds still: {}",
        chrome.eval(&place())
    );
    let slide = chrome.eval(&place());
    // A new window: Days, its fortnight.
    chrome.eval("document.querySelector('.reports-controls [data-grain=day]').click()");
    assert!(
        chrome.wait_for(
            "document.querySelector('.reports[aria-busy=false] [data-grain=day][aria-pressed=true]') !== null",
            SHOWN
        ) && chrome.wait_for(&still(), SHOWN),
        "Days draws and the page holds still: {}",
        chrome.eval(&place())
    );
    chrome.pump(std::time::Duration::from_millis(600));
    let later = chrome.eval(&place());
    eprintln!(
        "REPORTS LEFT: landed {landed}, left {left}, the hour slid {slide}, a new window {later}"
    );
    assert_eq!(
        chrome.eval("window.pageScrolls"),
        json!(0),
        "nothing scrolls the page once the reader left the chart: left {left}, the hour slid {slide}, a new window {later}"
    );
    for place in [&slide, &later] {
        assert!(
            (number(place, "top") - number(place, "padding")).abs() > 100.0,
            "the chart is not brought back to the scroll padding: left {left}, the hour slid {slide}, a new window {later}"
        );
    }
}

#[test]
fn a_chart_s_address_lands_nothing_once_the_reader_chose_a_window_before_any_chart_drew() {
    // The first window draws no chart: it recorded nothing, or its read failed. The reader then
    // chooses Days, whose fortnight draws every chart, the addressed one among them.
    for failed in [false, true] {
        let site = served(&fortnight());
        let (first, way) = if failed {
            site.fail_route(&hours_read(), true);
            (".reports-error", "a failed read")
        } else {
            site.set_json(&hours_read(), &as_hours(&nothing_recorded(&fortnight())));
            (
                ".reports[aria-busy=false] .reports-figures",
                "nothing recorded",
            )
        };
        let Some(mut chrome) = chrome() else { return };
        chrome.viewport(1280, 900, false);
        chrome.open(&site.url(&format!("/reports#{ADDRESSED}")));
        assert!(
            chrome.wait_for(
                &format!(
                    "{READY} && document.querySelector({}) !== null && document.querySelector('.chart-slot') === null",
                    json!(first)
                ),
                SHOWN
            ),
            "{way}: the first window draws no chart: {}",
            chrome.eval(&place())
        );
        chrome.eval(TWO_FRAMES);
        chrome.eval(COUNT_SCROLLS);
        chrome.eval("document.querySelector('.reports-controls [data-grain=day]').click()");
        assert!(
            chrome.wait_for(
                "document.querySelector('.reports[aria-busy=false] [data-grain=day][aria-pressed=true]') !== null && document.querySelector('figure.chart') !== null",
                SHOWN
            ),
            "{way}: Days draws its charts: {}",
            chrome.eval(&place())
        );
        chrome.eval(TWO_FRAMES);
        chrome.pump(std::time::Duration::from_millis(600));
        let later = chrome.eval(&place());
        let scrolls = chrome.eval("window.pageScrolls");
        eprintln!("REPORTS CHOSE A WINDOW, {way}: {scrolls} page scrolls, {later}");
        assert_eq!(
            (&scrolls, &later["y"], &later["hash"]),
            (&json!(0), &json!(0), &json!(format!("#{ADDRESSED}"))),
            "{way}: the page stays where the reader chose the window, at the address they opened, \
             its chart not landed: {later}"
        );
    }
}

/// The part of the reports' item this device keeps the last 24 hours in, as [`PARTS`] names it.
const HOURS_KEPT: &str = "reports: hour:1";

/// The page shows the charts of the copy this device kept, marked as the device's, saying `said`.
fn from_the_device(said: &str) -> String {
    format!(
        "document.querySelector('.reports[data-ready=reports][data-cache=device] #chart-open .chart-plot') !== null && document.querySelector('.reports .page-notices .device-notice')?.textContent === {}",
        json!(said)
    )
}

/// The page shows the server's answer: nothing marked as the device's, nothing said of it.
const CURRENT: &str = "document.querySelector('.reports[data-ready=reports]:not([data-cache]) #chart-open .chart-plot') !== null && document.querySelector('.device-notice') === null";

/// The settings change the page lists says `subject`.
fn marked(subject: &str) -> String {
    format!(
        "document.querySelector('.reports-marks')?.textContent.includes({}) === true",
        json!(subject)
    )
}

/// A first visit to the last 24 hours: drawn from the server and kept on this device. When its
/// copy was kept.
fn first_visit(chrome: &mut Chrome, site: &Site) -> f64 {
    store_opened(chrome, site);
    open(
        chrome,
        site,
        "[data-ready=reports][aria-busy=false] #chart-open .chart-plot",
    );
    shows(chrome, CURRENT, "the first visit draws the server's answer");
    keeps(chrome, &[HOURS_KEPT]);
    kept(chrome)[HOURS_KEPT].as_f64().expect("when it was kept")
}

/// The page left and opened again.
fn come_back(chrome: &mut Chrome, site: &Site) {
    chrome.open("about:blank");
    chrome.open(&site.url("/reports"));
}

/// A return visit draws the window this device kept, charts and all, while its read is still
/// unanswered, saying so in the other pages' words; the read names the kept copy's tag, and a
/// window no roll-up changed is answered `304`, once, with no body, after which nothing says the
/// device. A window that changed is drawn from the device first, then as the server answers it,
/// and that answer replaces the kept copy: the visit after it is checked with a `304` again.
#[test]
fn a_return_visit_draws_the_kept_window_before_its_read_answers_and_an_unchanged_one_costs_no_body()
{
    let site = both_windows();
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport(1440, 900, false);
    let saved = first_visit(&mut chrome, &site);
    let reads = symphony_api::REPORTS_PATH;

    let before = site.answers(reads).len();
    site.hold_route(reads, true);
    come_back(&mut chrome, &site);
    let said = notice(&mut chrome, saved, false);
    shows(
        &mut chrome,
        &from_the_device(&said),
        "the kept window is drawn while its read is held",
    );
    assert!(
        chrome.wait_for(&marked("land without the check"), SHOWN),
        "the kept copy's settings change is listed"
    );
    assert_eq!(
        site.answers(reads).len(),
        before,
        "drawn before any read of the reports answered"
    );
    site.hold_route(reads, false);
    shows(&mut chrome, CURRENT, "the check answered");
    let checks = site.answers(reads)[before..].to_vec();
    assert_eq!(checks.len(), 1, "one check: {checks:#?}");
    assert!(
        checks[0]
            .named
            .as_deref()
            .is_some_and(|tag| tag.starts_with("W/\""))
            && checks[0].status == 304,
        "the check names the kept copy's tag and is answered with no body: {checks:#?}"
    );

    // The hour slid since: the kept copy first, then the server's, which is kept in its place.
    site.set_json(&hours_read(), &slid(&as_hours(&fortnight())));
    let before = site.answers(reads).len();
    site.hold_route(reads, true);
    come_back(&mut chrome, &site);
    shows(
        &mut chrome,
        &from_the_device(&said),
        "the kept window is drawn while the changed one is held",
    );
    assert_eq!(
        chrome.eval(&marked("the hour slid")),
        json!(false),
        "the device's copy is the hour before"
    );
    site.hold_route(reads, false);
    shows(
        &mut chrome,
        &marked("the hour slid"),
        "the changed window draws",
    );
    shows(&mut chrome, CURRENT, "the changed window is current");
    let changed = site.answers(reads)[before..].to_vec();
    assert_eq!(
        changed
            .iter()
            .map(|answer| (answer.named.is_some(), answer.status))
            .collect::<Vec<_>>(),
        [(true, 200)],
        "one read, naming the kept tag, answered whole: {changed:#?}"
    );
    shows(
        &mut chrome,
        &format!(
            "{PARTS}.then(kept => kept[{}] > {saved})",
            json!(HOURS_KEPT)
        ),
        "the new answer replaced the kept copy",
    );
    let before = site.answers(reads).len();
    come_back(&mut chrome, &site);
    shows(
        &mut chrome,
        &marked("the hour slid"),
        "the copy kept last draws",
    );
    shows(&mut chrome, CURRENT, "its check answered");
    let again = site.answers(reads)[before..].to_vec();
    assert_eq!(
        again.iter().map(|answer| answer.status).collect::<Vec<_>>(),
        [304],
        "the copy kept last is the one checked: {again:#?}"
    );
}

/// A return visit whose read fails keeps the kept window drawn, said as this device's and the
/// server as not reachable, with the failed read's own line saying it is tried again; the read
/// is tried again by itself, and its answer leaves nothing said of the device or the failure.
#[test]
fn a_failed_read_leaves_the_kept_window_drawn_and_said_stale_until_a_retry_answers() {
    let site = both_windows();
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport(1440, 900, false);
    let saved = first_visit(&mut chrome, &site);
    let reads = symphony_api::REPORTS_PATH;

    let before = site.answers(reads).len();
    site.fail_route(reads, true);
    come_back(&mut chrome, &site);
    let said = notice(&mut chrome, saved, true);
    shows(
        &mut chrome,
        &from_the_device(&said),
        "the kept window stays drawn, said not reachable",
    );
    assert_eq!(
        texts(&mut chrome, ".reports-error"),
        [
            "The reports could not be read. Board snapshot unavailable (the server answered 500). Trying again…"
        ],
    );
    assert!(
        site.answers(reads)[before..]
            .iter()
            .all(|answer| answer.status == 500 && answer.named.is_some()),
        "each failed read named the kept copy's tag: {:#?}",
        &site.answers(reads)[before..]
    );
    site.fail_route(reads, false);
    shows(&mut chrome, CURRENT, "the retry answered");
    assert_eq!(texts(&mut chrome, ".reports-error"), [] as [&str; 0]);
    let last = site.answers(reads).last().cloned().expect("an answer");
    assert_eq!(
        (last.named.is_some(), last.status),
        (true, 304),
        "the retry is the same check, answered with no body: {last:?}"
    );
}

/// The page at its end with what is near it drawn: true once nothing near the screen waits to
/// mount and the page rests at its end, scrolled there again on each look, as a chart mounted
/// there makes the page longer or shorter than its ghost.
const AT_ITS_END: &str = "(() => { scrollTo(0, document.documentElement.scrollHeight); const near = box => { const r = box.getBoundingClientRect(); return r.bottom > -600 && r.top < innerHeight + 600; }; return ![...document.querySelectorAll('[data-pending=true]')].some(near) && scrollY === Math.max(0, document.documentElement.scrollHeight - innerHeight); })()";

/// Where the notices stand at the page's end: whether it is there, the stack placed as the sheet
/// places it and whole in the window, the page's last section's bottom and the stack's top, and
/// whether the toasts are set above the stack.
const THE_END: &str = r#"(() => {
    const stack = document.querySelector('.reports .page-notices');
    const box = stack.getBoundingClientRect();
    const last = [...document.querySelectorAll('.reports > .reports-section')].at(-1).getBoundingClientRect();
    return {
        atEnd: scrollY === Math.max(0, document.documentElement.scrollHeight - innerHeight),
        position: getComputedStyle(stack).position,
        shown: box.height > 0 && box.top >= 0 && box.bottom <= innerHeight && box.left >= 0 && box.right <= innerWidth,
        clear: last.bottom <= box.top,
        toasts: getComputedStyle(document.documentElement).getPropertyValue('--notice-toast-bottom') === `${innerHeight - box.top + 8}px`,
        last: Math.round(last.bottom),
        notice: Math.round(box.top),
    };
})()"#;

/// On a phone, the notice that says the page is this device's copy floats over the window's foot
/// as the other pages' do, and the page keeps the room under its end that clears it: at the end,
/// the last chart's section ends above the notice, and the toasts are set above the notice.
#[test]
fn on_a_phone_the_kept_copy_s_notice_leaves_the_page_s_end_clear_and_the_toasts_above_it() {
    let site = both_windows();
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport_scaled(390, 844, 3, true);
    let saved = first_visit(&mut chrome, &site);
    site.hold_route(symphony_api::REPORTS_PATH, true);
    come_back(&mut chrome, &site);
    let said = notice(&mut chrome, saved, false);
    shows(
        &mut chrome,
        &from_the_device(&said),
        "the kept window is drawn while its read is held",
    );
    shows(&mut chrome, AT_ITS_END, "the page rests at its end");
    chrome.eval(TWO_FRAMES);
    let end = chrome.eval(THE_END);
    eprintln!("REPORTS PAGE END, phone, said from the device: {end}");
    assert_eq!(
        json!({"atEnd": end["atEnd"], "position": end["position"], "shown": end["shown"],
            "clear": end["clear"], "toasts": end["toasts"]}),
        json!({"atEnd": true, "position": "fixed", "shown": true, "clear": true, "toasts": true}),
        "at the page's end its last section is clear of the floating notice: {end}"
    );
    site.hold_route(symphony_api::REPORTS_PATH, false);
    shows(&mut chrome, CURRENT, "the check answered");
}

/// The target of a window of `days` by the day's read.
fn days_read(days: u32) -> String {
    format!("{}?grain=day&days={days}", symphony_api::REPORTS_PATH)
}

/// The part of the reports' item this device keeps Days' fortnight in.
const FORTNIGHT_KEPT: &str = "reports: day:14";

/// The answers so far to the reads of `target`, its query included.
fn answered(site: &Site, target: &str) -> Vec<site::Answered> {
    site.answers(symphony_api::REPORTS_PATH)
        .into_iter()
        .filter(|answer| answer.target == target)
        .collect()
}

/// A press of the window's control `button` names (`[data-grain=day]`, `[data-days="7"]`).
fn choose(chrome: &mut Chrome, button: &str) {
    chrome.eval(&format!(
        "document.querySelector({}).click()",
        json!(format!(".reports-controls {button}"))
    ));
}

/// A window this device kept, chosen again on the page, is drawn from the device at once while
/// its read is on its way, said as checking: the read that failed was another window's, the one
/// left, and is not said of it. Its read names the kept copy's tag and is answered with no body.
#[test]
fn a_kept_window_chosen_again_draws_from_the_device_and_is_not_said_unreachable_for_the_window_left()
 {
    let site = both_windows();
    let Some(mut chrome) = chrome() else { return };
    chrome.viewport(1440, 900, false);
    first_visit(&mut chrome, &site);
    // Days: its fortnight, drawn from the server and kept.
    choose(&mut chrome, "[data-grain=day]");
    shows(
        &mut chrome,
        &format!(
            "{CURRENT} && document.querySelector('.reports[aria-busy=false] [data-days=\"14\"][aria-pressed=true]') !== null"
        ),
        "Days' fortnight draws",
    );
    keeps(&mut chrome, &[HOURS_KEPT, FORTNIGHT_KEPT]);
    let saved = kept(&mut chrome)[FORTNIGHT_KEPT]
        .as_f64()
        .expect("when it was kept");
    // A week by the day, whose read fails: said over the fortnight, still drawn.
    site.fail_route(&days_read(7), true);
    choose(&mut chrome, "[data-days=\"7\"]");
    shows(
        &mut chrome,
        "document.querySelector('.reports[aria-busy=false] .reports-error') !== null",
        "the week's read fails",
    );
    assert_eq!(
        texts(&mut chrome, ".reports-error"),
        [
            "The reports could not be read. Board snapshot unavailable (the server answered 500). Trying again…"
        ],
    );
    // The fortnight again, its read held.
    let fortnight = days_read(14);
    let before = answered(&site, &fortnight).len();
    site.hold_route(&fortnight, true);
    choose(&mut chrome, "[data-days=\"14\"]");
    let said = notice(&mut chrome, saved, false);
    shows(
        &mut chrome,
        &format!(
            "{} && document.querySelector('.reports-error') === null",
            from_the_device(&said)
        ),
        "the fortnight kept is drawn while its read is held, said checking, the week's failure gone",
    );
    assert_eq!(
        answered(&site, &fortnight).len(),
        before,
        "drawn before its read answered"
    );
    site.hold_route(&fortnight, false);
    shows(&mut chrome, CURRENT, "its check answered");
    let checks = answered(&site, &fortnight)[before..].to_vec();
    assert_eq!(
        checks
            .iter()
            .map(|answer| (answer.named.is_some(), answer.status))
            .collect::<Vec<_>>(),
        [(true, 304)],
        "one check, naming the kept copy's tag, answered with no body: {checks:#?}"
    );
}
