//! The Reports page (apps/symphony-web/src/reports.tsx, the web client's `reports` kind), the
//! release client against the stand-in server answering `/api/reports` with a fortnight by the
//! day: a phone's width held, one tap showing a period's every value on every chart, a day no file
//! recorded told apart from a zero, the arrow keys moving between periods, a window's many
//! settings changes folded past the newest three so its first chart shows on a phone's first
//! screen, two columns on a wide screen, the dark scheme's own colours, the empty state saying when
//! the first roll-up comes, the last 24 hours with nothing chosen, and the window and the series
//! hidden kept for the next visit, a hidden series' room given to the rest.
use super::{SHOWN, chrome, chrome::Chrome, common, components::key, site, site::Site, texts};
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

fn served(page: &Value) -> Site {
    let site = site();
    site.set_json(symphony_api::REPORTS_PATH, page);
    site
}

fn screen(chrome: &mut Chrome, width: u32, height: u32, mobile: bool) {
    chrome.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":if mobile { 3 } else { 1 },"mobile":mobile}),
    );
}

fn open(chrome: &mut Chrome, site: &Site, drawn: &str) {
    chrome.open(&site.url("/reports"));
    assert!(
        chrome.wait_for(
            &format!(
                "document.documentElement.dataset.navigation === 'ready' && document.querySelector({}) !== null",
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
    screen(&mut chrome, 390, 844, true);
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
    screen(&mut chrome, 1440, 900, false);
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
        chrome.call(
            "Emulation.setEmulatedMedia",
            json!({"features": [{"name": "prefers-color-scheme", "value": scheme}]}),
        );
        assert_eq!(chrome.eval(colours), json!(expected), "{scheme}");
    }
}

/// The days of [`fortnight`] its ten settings changes fall on, the last three before today, six on
/// the middle one: as this repository's own fortnight stood on 13 Nov, symphony.toml changed six
/// times on 12 Nov alone.
const CHANGED: [u32; 10] = [10, 10, 11, 11, 11, 11, 11, 11, 12, 12];

/// [`fortnight`] with the ten settings changes of [`CHANGED`], oldest first as the server sends
/// them, the `n`th at `n + 1` o'clock, each subject as long as a merge's.
fn changed_often() -> Value {
    let mut page = fortnight();
    page["marks"] = (0_u32..)
        .zip(CHANGED)
        .map(|(n, day)| {
            json!({"at": FROM + f64::from(day) * DAY + 3600.0 * f64::from(n + 1),
                "commit": format!("{n}f1c0a9b2d3e"),
                "subject": format!("Merge branch 'tools/settings-change-{n}': the board's limits and models")})
        })
        .collect();
    page
}

/// On a phone the list of a window's ten settings changes shows the newest three and folds the
/// rest under a disclosure, so the first chart starts on the first screen; a period with six says
/// each in its readout.
#[test]
fn on_a_phone_a_window_s_many_settings_changes_fold_and_the_first_chart_shows_on_the_first_screen()
{
    let site = served(&changed_often());
    let Some(mut chrome) = chrome() else { return };
    screen(&mut chrome, 390, 844, true);
    open(&mut chrome, &site, "[data-ready=reports] .chart-plot");
    // The first chart's place on the page against the screen's height.
    let first = chrome.eval(
        "(() => { const c = document.querySelector('.chart').getBoundingClientRect(); return {top: Math.round(c.top + scrollY), screen: innerHeight}; })()",
    );
    assert!(
        first["top"].as_i64().unwrap() < first["screen"].as_i64().unwrap(),
        "the first chart starts on the first screen: {first}"
    );
    // The list's words read as drawn (`textContent`), and whether the folded ones take room.
    let marks = "(() => { const m = document.querySelector('.reports-marks'); const d = m.querySelector('details'); return {listed: [...m.querySelectorAll(':scope > ul > li')].map(li => li.textContent), more: d?.querySelector('summary').textContent ?? null, open: d?.open ?? null, folded: d ? [...d.querySelectorAll('li')].map(li => li.textContent.split(':')[0]) : null, drawn: d ? d.querySelector('ul').getClientRects().length > 0 : null, overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth}; })()";
    let mark = |n: usize, day: &str| {
        format!(
            "{day}: settings changed, Merge branch 'tools/settings-change-{n}': the board's limits and models ({n}f1c0a9b2d3e)"
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
            "more": "7 more settings changes", "open": false, "folded": folded, "drawn": false,
            "overflow": 0}),
        "the newest three, newest first, and the other seven, newest first, folded"
    );
    chrome.eval("document.querySelector('.reports-marks summary').click()");
    assert!(
        chrome.wait_for(
            "document.querySelector('.reports-marks details').open && document.querySelector('.reports-marks details ul').getClientRects().length > 0",
            SHOWN
        ),
        "the disclosure opens on the other seven: {}",
        chrome.eval(marks)
    );
    assert_eq!(
        chrome.eval("document.documentElement.scrollWidth - document.documentElement.clientWidth"),
        json!(0),
        "the opened list keeps within the phone's width"
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
        "each of the day's six settings changes, oldest first"
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
    screen(&mut chrome, 1440, 900, false);
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
    let mut empty = fortnight();
    empty["first"] = Value::Null;
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
    let site = served(&empty);
    let Some(mut chrome) = chrome() else { return };
    screen(&mut chrome, 390, 844, true);
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
