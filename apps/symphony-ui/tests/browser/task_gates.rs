//! A task page's gates (src/task_gates.rs) in the release client: three lines a gate on a phone
//! and one wide, the theme's colours for each state, a read that fails or holds, and a task
//! nothing gates. The task is the look-parity fixture's; its gates are the A pictures' task
//! (crates/symphony-api/fixtures/task-gates.json).
use super::{SHOWN, chrome, chrome::Chrome, common, dist, look_parity, site::Site};
use serde_json::{Value, json};

const GATES: &str = "/api/task-gates";
const PAGE: &str = "/task/visual/in-progress";

/// A server with the fixture task's page and thread, and `gates` as its gates.
fn site(gates: &Value) -> Site {
    let site = Site::start(dist(), &common::fixture("board.json"));
    let content = look_parity::responses();
    for path in ["/api/task-page", "/api/task-history"] {
        site.set_json(path, &content[path]);
    }
    site.set_json(GATES, gates);
    site
}

fn pictured() -> Value {
    serde_json::from_str(&common::fixture("task-gates.json")).unwrap()
}

fn view(page: &mut Chrome, width: u32, height: u32, mobile: bool) {
    page.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":1,"mobile":mobile}),
    );
}

fn scheme(page: &mut Chrome, mode: &str) {
    page.call(
        "Emulation.setEmulatedMedia",
        json!({"features":[{"name":"prefers-color-scheme","value":mode}]}),
    );
}

/// Open the task page and wait for its gates to be drawn.
fn open_gates(page: &mut Chrome, site: &Site) {
    page.open(&site.url(PAGE));
    assert!(
        page.wait_for(
            "document.querySelectorAll('.task-gates[data-gates=read] li.gate').length === 4 && document.fonts.status === 'loaded'",
            SHOWN
        ),
        "the gates were not drawn: {:?}",
        page.eval("document.querySelector('main')?.innerText")
    );
}

/// Each gate's parts as boxes: `[name, state, detail, by]`, each `{top, bottom, left, right}`.
const BOXES: &str = "[...document.querySelectorAll('.task-gates li.gate')].map(row => ['.gate-name','.gate-state','.gate-detail','.gate-by'].map(part => {const box=row.querySelector(part).getBoundingClientRect();return {top:Math.round(box.top),bottom:Math.round(box.bottom),left:Math.round(box.left),right:Math.round(box.right)};}))";

fn boxes(page: &mut Chrome) -> Vec<Vec<Value>> {
    serde_json::from_value(page.eval(BOXES)).unwrap()
}

fn at(part: &Value, edge: &str) -> i64 {
    part[edge].as_i64().unwrap()
}

#[test]
fn task_page_gates_are_three_lines_each_on_a_phone_and_one_line_each_wide() {
    let site = site(&pictured());
    let Some(mut page) = chrome() else { return };
    view(&mut page, 390, 844, true);
    open_gates(&mut page, &site);
    assert_eq!(
        page.eval("[...document.querySelectorAll('.task-gates li.gate')].map(row => row.dataset.gate + ' ' + row.dataset.state)"),
        json!(["certificate missing", "dependency waived", "nightly met", "review met"]),
        "what the task waits on comes first"
    );
    assert_eq!(
        page.eval("({wide: document.documentElement.scrollWidth, view: innerWidth, head: document.querySelector('.task-gates .gates-head').innerText.replace(/\\s+/g, ' ')})"),
        json!({"wide": 390, "view": 390, "head": "GATES waits on certificate 2 met · 1 waived · 1 missing"}),
        "a phone does not scroll sideways"
    );
    for (row, parts) in boxes(&mut page).iter().enumerate() {
        let [name, state, detail, by] = &parts[..] else {
            panic!("{parts:?}")
        };
        assert!(
            (at(name, "top") - at(state, "top")).abs() <= 6,
            "gate {row}: its name and state share the first line: {parts:?}"
        );
        assert!(
            at(state, "right") > at(detail, "right") - 2 && at(name, "left") <= at(detail, "left"),
            "gate {row}: the state sits at the line's end: {parts:?}"
        );
        assert!(
            at(detail, "top") >= at(name, "bottom") - 2
                && at(by, "top") >= at(detail, "bottom") - 2,
            "gate {row}: its detail, then who and when, each below: {parts:?}"
        );
        assert_eq!(
            at(by, "left"),
            at(detail, "left"),
            "gate {row}: the lines start together: {parts:?}"
        );
    }
    view(&mut page, 1280, 900, false);
    page.eval("new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))");
    let wide = boxes(&mut page);
    for (row, parts) in wide.iter().enumerate() {
        let [name, state, detail, by] = &parts[..] else {
            panic!("{parts:?}")
        };
        for part in [state, detail, by] {
            assert!(
                (at(part, "top") - at(name, "top")).abs() <= 6,
                "gate {row}: one line, each part beside the last: {parts:?}"
            );
        }
        assert!(
            at(name, "right") <= at(state, "left")
                && at(state, "right") <= at(detail, "left")
                && at(detail, "right") <= at(by, "left"),
            "gate {row}: name, state, detail, then who and when: {parts:?}"
        );
        assert!(
            at(detail, "bottom") - at(detail, "top") < 30,
            "gate {row}: its detail is one line: {parts:?}"
        );
    }
    for column in 0..4 {
        let lefts: Vec<i64> = wide
            .iter()
            .map(|parts| at(&parts[column], "left"))
            .collect();
        if column < 3 {
            assert!(
                lefts.windows(2).all(|pair| pair[0] == pair[1]),
                "column {column} starts at one place in every gate: {lefts:?}"
            );
        }
    }
}

/// Each state's pill colour and its dot's, and the waits-on pill's.
const COLOURS: &str = "(() => {const row=state=>document.querySelector(`.task-gates li.gate[data-state=${state}]`);\
    const look=state=>[getComputedStyle(row(state).querySelector('.gate-state')).color,getComputedStyle(row(state).querySelector('.gate-name'),'::before').backgroundColor];\
    return {missing:look('missing'),waived:look('waived'),met:look('met'),waits:getComputedStyle(document.querySelector('.gates-waits')).color};})()";

fn colours(missing: &str, waived: &str, met: &str) -> Value {
    json!({"missing":[missing,missing],"waived":[waived,waived],"met":[met,met],"waits":missing})
}

#[test]
fn task_page_gates_take_each_states_colour_from_the_theme_light_dark_and_imported() {
    let site = site(&pictured());
    let Some(mut page) = chrome() else { return };
    view(&mut page, 1280, 900, false);
    for (mode, expected) in [
        (
            "light",
            colours("rgb(148, 106, 0)", "rgb(95, 107, 115)", "rgb(46, 125, 79)"),
        ),
        (
            "dark",
            colours(
                "rgb(224, 184, 76)",
                "rgb(143, 154, 163)",
                "rgb(111, 196, 143)",
            ),
        ),
    ] {
        scheme(&mut page, mode);
        open_gates(&mut page, &site);
        assert_eq!(
            page.eval(COLOURS),
            expected,
            "{mode}: question, resolved and like"
        );
    }
    // What the Appearance menu stores when it imports a theme that sets the status tokens.
    let seed = page.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source":"localStorage.setItem('studio.cssVars', ':root[data-studio-theme]{--like:#00aa00;--question:#aa0000;--resolved:#0000aa;}');"}),
    );
    scheme(&mut page, "light");
    open_gates(&mut page, &site);
    page.call(
        "Page.removeScriptToEvaluateOnNewDocument",
        json!({"identifier":seed["identifier"]}),
    );
    assert!(
        page.wait_for("document.querySelector('#studio-theme') !== null", SHOWN),
        "the imported theme applies"
    );
    assert_eq!(
        page.eval(COLOURS),
        colours("rgb(170, 0, 0)", "rgb(0, 0, 170)", "rgb(0, 170, 0)"),
        "an imported theme colours the states"
    );
}

#[test]
fn task_page_gates_hold_their_height_while_read_and_a_failed_read_leaves_the_page_working() {
    let site = site(&pictured());
    let Some(mut page) = chrome() else { return };
    view(&mut page, 1280, 900, false);
    site.hold_route(GATES, true);
    page.open(&site.url(PAGE));
    assert!(
        page.wait_for(
            "document.querySelector('.task-gates[data-gates=reading]') !== null && document.querySelector('.task-body') !== null && document.querySelector('#task-message') !== null",
            SHOWN
        ),
        "the head, the brief and the message box do not wait on the gates: {:?}",
        page.eval("document.querySelector('main')?.innerText")
    );
    assert_eq!(
        page.eval("(() => {const section=document.querySelector('.task-gates');return {height:section.getBoundingClientRect().height>=160,said:section.innerText.includes('Reading the gates'),busy:section.getAttribute('aria-busy')};})()"),
        json!({"height":true,"said":true,"busy":"true"}),
        "while it reads, the section holds its height and says so"
    );
    let below =
        page.eval("Math.round(document.querySelector('.task-body').getBoundingClientRect().top)");
    site.fail_route(GATES, true);
    site.hold_route(GATES, false);
    assert!(
        page.wait_for(
            "document.querySelector('.task-gates[data-gates=failed]') !== null",
            SHOWN
        ),
        "{:?}",
        page.eval("document.querySelector('main')?.innerText")
    );
    assert_eq!(
        page.eval("({said:document.querySelector('.task-gates [role=status]').textContent,brief:document.querySelector('.task-body').innerText.trim(),composer:document.querySelector('#task-message') !== null,notice:document.querySelector('.notice-stack')?.innerText.includes('Board snapshot') ?? false})"),
        json!({"said":"The gates could not be read: Board snapshot unavailable. The rest of the page still works.","brief":"Task brief","composer":true,"notice":false}),
        "the failure is the section's own, not the page's"
    );
    site.fail_route(GATES, false);
    page.eval("document.querySelector('.task-gates button').click()");
    assert!(
        page.wait_for(
            "document.querySelectorAll('.task-gates[data-gates=read] li.gate').length === 4",
            SHOWN
        ),
        "Retry reads them again"
    );
    assert!(
        below.as_i64().unwrap() > 0,
        "the brief sat below the held section: {below}"
    );
}

#[test]
fn task_page_gates_are_absent_when_nothing_gates_the_task_or_the_board_has_no_answer() {
    for gates in [
        Some(json!({"slug":"visual/in-progress","branch":"visual/in-progress","gates":[]})),
        None,
    ] {
        let site = match &gates {
            Some(gates) => site(gates),
            None => {
                // The stand-in answers 404 for a route it has no reply for, as the server does
                // for a task not on its board.
                let site = Site::start(dist(), &common::fixture("board.json"));
                let content = look_parity::responses();
                for path in ["/api/task-page", "/api/task-history"] {
                    site.set_json(path, &content[path]);
                }
                site
            }
        };
        let Some(mut page) = chrome() else { return };
        page.open(&site.url(PAGE));
        assert!(page.wait_for("document.querySelector('.task-body') !== null", SHOWN));
        let asked = std::time::Instant::now();
        while !site.requests().iter().any(|path| path == GATES) {
            assert!(asked.elapsed() < SHOWN, "the gates were never read");
            page.eval("new Promise(r => setTimeout(r, 50))");
        }
        assert!(
            page.wait_for("document.querySelector('.task-gates') === null", SHOWN),
            "{gates:?}: no section: {:?}",
            page.eval("document.querySelector('.task-gates')?.outerHTML")
        );
    }
}
