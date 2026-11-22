//! A task page's gates (apps/symphony-web/src/gates.tsx) in the release client: three lines a gate on a phone
//! and bounded columns wide, every word of long text shown at 1280, 1000 and 820 px with none
//! kept for a hover; the theme's colours for each state; the room the card holds before its
//! answer, so the brief does not move, a gate with no who and when included; a read asked only
//! near the screen, again on the board events that change the gates and never for a plain post,
//! the latest alone drawn; a read that fails, its retry saying it reads while the read it asked
//! is out; and a task nothing gates. The task is the
//! look-parity fixture's, in review; its gates are the A pictures' task
//! (crates/symphony-api/fixtures/task-gates.json). That a gate's link names its page under the
//! project's base is the one server's to show (projects.rs).
use super::{SHOWN, STALL, chrome, chrome::Chrome, common, look_parity, site::Site, wait_for_feed};
use serde_json::{Value, json};

const GATES: &str = "/api/task-gates";
const PAGE: &str = "/task/visual/in-progress";
const SLUG: &str = "visual/in-progress";
/// The branch the pictured answer was given for, as the panel learns it.
const BRANCH: &str = "tools/verify-stops-when-moved";

/// The fixture task's page, in review, where the pill says the gates it waits on, changed by
/// `edit`.
fn task_page(edit: impl FnOnce(&mut Value)) -> Value {
    let mut page = look_parity::responses()["/api/task-page"].clone();
    page["task"]["status"] = json!("review");
    edit(&mut page);
    page
}

/// A server with the fixture task's page (`task_page`) and thread, and `gates` as its gates.
fn site(gates: &Value) -> Site {
    let site = crate::site();
    site.set_json("/api/task-page", &task_page(|_| {}));
    site.set_json(
        "/api/task-history",
        &look_parity::responses()["/api/task-history"],
    );
    site.set_json(GATES, gates);
    site
}

fn pictured() -> Value {
    serde_json::from_str(&common::fixture("task-gates.json")).unwrap()
}

/// `count` gates with words short enough for one line on a phone: the first the Symphony's review
/// before any verdict, missing with no who and when, the rest met.
fn short(count: usize) -> Value {
    if count == 4 {
        return pictured();
    }
    let gates: Vec<Value> = (1..=count)
        .map(|n| match n {
            1 => json!({"name":"review","state":"missing","detail":"Not reviewed yet"}),
            _ => json!({"name":format!("gate-{n}"),"state":"met",
                "by":"nightly","at":1_794_363_240.0,"detail":format!("Green on main a71f0d{n}")}),
        })
        .collect();
    json!({"slug":SLUG,"branch":SLUG,"gates":gates})
}

/// What the server says of `answer` with the page: its count, how many it waits on and how many
/// have no who and when (none of these fixtures' commits goes unsaid).
fn seen(answer: &Value) -> Value {
    let gates = answer["gates"].as_array().unwrap();
    let waiting = gates
        .iter()
        .filter(|g| g["state"] == "missing" || g["state"] == "held")
        .count();
    let unsigned = gates
        .iter()
        .filter(|g| g.get("by").is_none() && g.get("at").is_none())
        .count();
    json!({"count":gates.len(),"waiting":waiting,"unsigned":unsigned})
}

fn pause(page: &mut Chrome, ms: u64) {
    page.eval(&format!("new Promise(r => setTimeout(r, {ms}))"));
}

/// Open the task page and wait for its `count` gates to be drawn.
fn open_gates(page: &mut Chrome, site: &Site, count: usize) {
    page.open(&site.url(PAGE));
    drawn(page, count);
}

fn drawn(page: &mut Chrome, count: usize) {
    assert!(
        page.wait_for(
            &format!("document.querySelectorAll('.task-gates[data-gates=read] li.gate').length === {count} && document.fonts.status === 'loaded'"),
            SHOWN
        ),
        "the gates were not drawn: {:?}",
        page.eval("document.querySelector('main')?.innerText")
    );
    page.frames_with_fonts();
}

/// How many times the page asked for the gates.
fn reads(site: &Site) -> usize {
    site.requests().iter().filter(|path| *path == GATES).count()
}

/// Wait until the page has asked for the gates `count` times.
#[track_caller]
fn wait_reads(page: &mut Chrome, site: &Site, count: usize) {
    let read = test_time::until_true(format!("the gates read {count} times"), STALL, || {
        reads(site) >= count || {
            pause(page, 50);
            false
        }
    });
    if let Err(stall) = read {
        panic!("{stall}: read {} times", reads(site));
    }
}

/// Each gate's parts as boxes: `[name, state, detail, by]`, each `{top, bottom, left, right}`.
const BOXES: &str = "[...document.querySelectorAll('.task-gates li.gate')].map(row => ['.gate-name','.gate-state','.gate-detail','.gate-by'].map(part => {const box=row.querySelector(part).getBoundingClientRect();return {top:Math.round(box.top),bottom:Math.round(box.bottom),left:Math.round(box.left),right:Math.round(box.right)};}))";

fn boxes(page: &mut Chrome) -> Vec<Vec<Value>> {
    serde_json::from_value(page.eval(BOXES)).unwrap()
}

fn at(part: &Value, edge: &str) -> i64 {
    part[edge].as_i64().unwrap()
}

/// The parts of the section's gates whose words run past their box, as `gate part`, and the
/// elements that keep words in a title for a hover.
const CUT: &str = "(() => {const cut=[];for (const row of document.querySelectorAll('.task-gates li.gate')) for (const part of ['.gate-label','.gate-detail','.gate-by']) {const el=row.querySelector(part);if (el.scrollWidth > el.clientWidth + 1 || el.scrollHeight > el.clientHeight + 1) cut.push(row.dataset.gate + ' ' + part);}return {cut, titled:document.querySelectorAll('.task-gates [title]').length};})()";

/// Wide: every word of every gate shown in its box, none cut and none kept in a title; the parts in
/// order on their first line, each column starting at one place, the detail at least `detail` px
/// wide and nothing past the card's inner edge; with `one_line`, no part wraps.
fn every_word_shown(page: &mut Chrome, detail: i64, one_line: bool, label: &str) {
    let card = page.eval("(() => {const card=document.querySelector('.task-gates').getBoundingClientRect(), list=document.querySelector('.task-gates .gates');return {right:Math.round(card.right), list:list.scrollWidth - list.clientWidth, page:document.documentElement.scrollWidth - document.documentElement.clientWidth};})()");
    assert_eq!(
        (&card["list"], &card["page"]),
        (&json!(0), &json!(0)),
        "{label}: nothing overflows: {card}"
    );
    assert_eq!(
        page.eval(CUT),
        json!({"cut": [], "titled": 0}),
        "{label}: every word is drawn, none cut short or left to a hover"
    );
    let wide = boxes(page);
    for (row, parts) in wide.iter().enumerate() {
        let [name, state, detail_box, by] = &parts[..] else {
            panic!("{parts:?}")
        };
        for part in [state, detail_box, by] {
            assert!(
                (at(part, "top") - at(name, "top")).abs() <= 6,
                "{label}: gate {row}'s parts start on one line: {parts:?}"
            );
        }
        if one_line {
            for part in [name, state, detail_box, by] {
                assert!(
                    at(part, "bottom") - at(part, "top") < 30,
                    "{label}: gate {row} is one line: {parts:?}"
                );
            }
        }
        assert!(
            at(name, "right") <= at(state, "left")
                && at(state, "right") <= at(detail_box, "left")
                && at(detail_box, "right") <= at(by, "left"),
            "{label}: gate {row}: name, state, detail, then who and when: {parts:?}"
        );
        assert!(
            at(detail_box, "right") - at(detail_box, "left") >= detail,
            "{label}: gate {row}'s detail keeps {detail} px: {parts:?}"
        );
        assert!(
            at(by, "right") <= card["right"].as_i64().unwrap(),
            "{label}: gate {row} ends inside the card: {parts:?} {card}"
        );
    }
    for column in 0..3 {
        let lefts: Vec<i64> = wide
            .iter()
            .map(|parts| at(&parts[column], "left"))
            .collect();
        assert!(
            lefts.windows(2).all(|pair| pair[0] == pair[1]),
            "{label}: column {column} starts at one place in every gate: {lefts:?}"
        );
    }
}

/// An 86-character name and a 65-character handle, a detail longer than any line and a waiver
/// whose why is 198 characters; the task waits on the long-named gate and on review.
fn long() -> Value {
    let name =
        "a-very-long-gate-name-from-a-plugin-that-says-all-it-checks-before-the-branch-can-land";
    let handle = "an-equally-long-handle-of-the-pipeline-that-ran-this-check-for-it";
    let why = "tools/slot-pool-by-tree shares no code with this branch: it changes the slot pool's \
               tree walk alone, and this branch only reads the pool's totals and records, which \
               that task leaves just as they are";
    assert_eq!((name.len(), handle.len(), why.len()), (86, 65, 198));
    json!({"slug":SLUG,"branch":SLUG,"gates":[
        {"name":name,"state":"missing","by":handle,"at":1_794_401_520.0,"commit":"4be91c2",
         "detail":"Verify on the merge tree 4be91c2 running, since main moved at 14:50, and it goes on until the plugin says otherwise"},
        {"name":"review","state":"missing","by":"lucid-heron","at":1_794_398_700.0,"detail":"Changes asked in round 2 of 2"},
        {"name":"dependency","state":"waived","by":"cobalt-otter","at":1_794_399_120.0,"why":why,
         "detail":"Waits on tools/slot-pool-by-tree, waived","link":"/task/tools/slot-pool-by-tree"},
        {"name":"nightly","state":"met","by":handle,"at":1_794_363_240.0,"detail":"Night of 11-10 green on main a71f0d3"},
    ]})
}

#[test]
fn task_page_gates_are_three_lines_each_on_a_phone_and_bounded_columns_wide_every_word_shown() {
    let site = site(&pictured());
    let Some(mut page) = chrome() else { return };
    page.viewport(390, 844, true);
    open_gates(&mut page, &site, 4);
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
    // Only the evidence's words carry the link.
    assert_eq!(
        page.eval(
            "[...document.querySelectorAll('.task-gates li.gate a')].map(a => a.textContent)"
        ),
        json!(["tools/slot-pool-by-tree", "round 2 of 2"])
    );
    // The pictures' width is one line a gate, as A draws it; narrower, as an upright tablet is,
    // a detail and its why wrap rather than lose their end.
    for (width, one_line) in [(1280, true), (1000, false), (820, false)] {
        page.viewport(width, 900, false);
        page.frames_with_fonts();
        every_word_shown(
            &mut page,
            140,
            one_line,
            &format!("the pictures at {width}"),
        );
    }

    site.set_json(GATES, &long());
    for width in [1280, 1000, 820] {
        page.viewport(width, 900, false);
        open_gates(&mut page, &site, 4);
        every_word_shown(&mut page, 140, false, &format!("long text at {width}"));
        assert_eq!(
            page.eval("[...document.querySelectorAll('.task-gates li.gate')].map(row => [row.querySelector('.gate-label').innerText.length, row.querySelector('.gate-by').innerText.split(' · ')[0].length, row.querySelector('.gate-why')?.innerText.length ?? 0])"),
            json!([[86, 65, 0], [6, 11, 0], [10, 12, 200], [7, 65, 0]]),
            "at {width}: the whole of the name, the handle and the why are drawn"
        );
    }
    // The pill's names and the words between them are one run of text, spaced as written.
    page.viewport(1280, 900, false);
    open_gates(&mut page, &site, 4);
    assert_eq!(
        page.eval("(() => {const name=document.querySelector('.gates-waits span[dir=auto]'), and=document.createRange();and.selectNodeContents(name.nextSibling);const a=[...name.getClientRects()].pop(), b=and.getClientRects()[0];return {joiner: name.nextSibling.textContent, line: Math.round(a.top) === Math.round(b.top), gap: Math.round(b.left - a.right)};})()"),
        json!({"joiner": " and ", "line": true, "gap": 0}),
        "no space opens between a name in the waits-on pill and the words after it"
    );
    // A phone keeps every word, the waits-on pill wrapped inside the card.
    page.viewport(390, 844, true);
    open_gates(&mut page, &site, 4);
    assert_eq!(
        page.eval(CUT),
        json!({"cut": [], "titled": 0}),
        "a phone draws every word"
    );
    assert_eq!(
        page.eval("(() => {const card=document.querySelector('.task-gates').getBoundingClientRect(), pill=document.querySelector('.gates-waits');const box=pill.getBoundingClientRect();return {inside: box.right <= card.right && box.left >= card.left, clipped: pill.scrollWidth > pill.clientWidth, wrapped: box.height > 40, said: pill.textContent, page: document.documentElement.scrollWidth - document.documentElement.clientWidth};})()"),
        json!({"inside": true, "clipped": false, "wrapped": true,
            "said": format!("waits on {} and review", long()["gates"][0]["name"].as_str().unwrap()), "page": 0}),
        "the waits-on pill wraps inside the card at 390 px"
    );
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
    page.viewport(1280, 900, false);
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
        page.scheme(mode);
        open_gates(&mut page, &site, 4);
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
    page.scheme("light");
    open_gates(&mut page, &site, 4);
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

/// A feed event of `changes` at `version`.
fn push(site: &Site, version: u64, changes: Value) {
    site.push(
        version,
        &json!({"version":version,"changes":changes}).to_string(),
    );
}

fn task_changed(updated: f64) -> Value {
    json!([{"region":"task-fields","data":{"key":SLUG,"fields":{"updated":updated}}}])
}

#[test]
fn task_page_gates_draw_only_the_latest_read_and_a_failed_read_leaves_the_page_working() {
    let site = site(&pictured());
    let Some(mut page) = chrome() else { return };
    page.viewport(1280, 900, false);
    // The first read is held and answers last, with other gates: a newer read drops it.
    let first = json!({"slug":SLUG,"branch":SLUG,"gates":[{"name":"stale","state":"missing","detail":"the first read's answer"}]});
    site.answer_in_turn(GATES, &[first, pictured()]);
    site.hold_next(GATES);
    page.open(&site.url(PAGE));
    wait_reads(&mut page, &site, 1);
    page.reveal("[id=\"composer-visual/in-progress\"]");
    assert!(
        page.wait_for(
            "document.querySelectorAll('.task-gates[data-gates=reading] li.gate-shadow').length === 4 && document.querySelector('.task-body') !== null && document.querySelector('#task-message') !== null",
            SHOWN
        ),
        "the head, the brief and the message box do not wait on the gates, which hold four rows: {:?}",
        page.eval("document.querySelector('main')?.innerText")
    );
    wait_for_feed(&site).expect("the page never followed the feed");
    push(&site, 4183, task_changed(2.0));
    wait_reads(&mut page, &site, 2);
    drawn(&mut page, 4);
    site.release_held();
    pause(&mut page, 500);
    assert_eq!(
        page.eval(
            "[...document.querySelectorAll('.task-gates li.gate')].map(row => row.dataset.gate)"
        ),
        json!(["certificate", "dependency", "nightly", "review"]),
        "the first read's late answer is never drawn"
    );
    let asked: Vec<_> = page
        .requests()
        .into_iter()
        .filter(|request| request.url.contains(GATES))
        .collect();
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert!(
        asked.iter().any(|request| request.canceled) && asked.iter().any(|r| r.finished),
        "the read dropped was aborted, not left waiting: {asked:?}"
    );

    // A read that fails while the gates are shown leaves them shown, and is tried again.
    site.fail_route(GATES, true);
    push(&site, 4184, task_changed(3.0));
    wait_reads(&mut page, &site, 3);
    drawn(&mut page, 4);
    // One that fails with none to show says so in its place, and the page goes on working.
    page.open(&site.url(PAGE));
    assert!(
        page.wait_for(
            "document.querySelector('.task-gates[data-gates=failed]') !== null",
            SHOWN
        ),
        "{:?}",
        page.eval("document.querySelector('main')?.innerText")
    );
    page.reveal("[id=\"composer-visual/in-progress\"]");
    assert!(page.wait_for("document.querySelector('#task-message') !== null", SHOWN));
    assert_eq!(
        page.eval("({said:document.querySelector('.task-gates [role=status]').textContent,brief:document.querySelector('.task-body').innerText.trim(),composer:document.querySelector('#task-message') !== null,notice:document.querySelector('.notice-stack')?.innerText.includes('Board snapshot') ?? false})"),
        json!({"said":"The gates could not be read: Board snapshot unavailable (the server answered 500). The rest of the page still works.","brief":"Task brief","composer":true,"notice":false}),
        "the failure is the section's own, not the page's"
    );
    // Its retry asks a read (the store tries one by itself too), and while that read is out says
    // it reads and asks for none more, the note kept in its place.
    site.hold_route(GATES, true);
    site.fail_route(GATES, false);
    page.eval("document.querySelector('.task-gates button').click()");
    let retrying = page.wait_for(
        "(button => button?.disabled && button.textContent === 'Retrying…')(document.querySelector('.task-gates[data-gates=failed] button'))",
        SHOWN,
    );
    let shown = page.eval("document.querySelector('main')?.innerText");
    site.hold_route(GATES, false);
    assert!(retrying, "the retry never said it reads: {shown:?}");
    drawn(&mut page, 4);
}

#[test]
fn task_page_gates_are_absent_when_nothing_gates_the_task_or_the_board_has_no_answer() {
    for gates in [Some(json!({"slug":SLUG,"branch":SLUG,"gates":[]})), None] {
        let site = match &gates {
            Some(gates) => site(gates),
            None => {
                // The stand-in answers 404 for a route it has no reply for, as the server does
                // for a task not on its board.
                let site = crate::site();
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
        wait_reads(&mut page, &site, 1);
        assert!(
            page.wait_for("document.querySelector('.task-gates') === null", SHOWN),
            "{gates:?}: no section: {:?}",
            page.eval("document.querySelector('.task-gates')?.outerHTML")
        );
    }
}

/// A message on the task's thread of `kind`, its line `id`.
fn message(id: u64, kind: &str) -> Value {
    json!([{"region":"message","data":{"id":id,"slug":SLUG,"sender":"lucid-heron","recipient":null,
        "kind":kind,"text":format!("{kind} {id}"),"ts":2.0,"stamp":"","blocks":[]}}])
}

/// The machines' work: a test of the task's on one machine, `phase_since` into its phase, or none.
fn machine_work(test: Option<f64>) -> Value {
    let operations: Vec<Value> = test
        .map(|since| {
            json!({"machine_id":"alpha","operation_id":"run-1","parent_id":null,
            "handle":"lucid-heron","task":SLUG,"checkout":"/c","recipe":"just test-rs",
            "kind":"test","state":"running","resource":"run_slot","reason":"","phase_since":since})
        })
        .into_iter()
        .collect();
    json!([{"region":"machine-work","data":{"freshness":"fresh","unassigned":[],"machines":[{
        "machine_id":"alpha","label":"alpha","producer_id":"p","sequence":test.map_or(1, |since| since as u64),
        "age_ms":0,"freshness":"fresh","complete":true,"reason":"","counts":null,
        "operations":operations}]}}])
}

#[test]
fn task_page_gates_read_again_on_the_events_that_change_them_and_never_on_a_plain_post() {
    let site = site(&pictured());
    site.set_json(
        "/api/task-page",
        &task_page(|page| page["task"]["links"] = json!(["docs/look-parity.md#palette"])),
    );
    let Some(mut page) = chrome() else { return };
    page.viewport(1280, 900, false);
    open_gates(&mut page, &site, 4);
    wait_for_feed(&site).expect("the page never followed the feed");
    assert_eq!(reads(&site), 1);
    let mut expected = 1;
    // Each step: what the feed says, then whether the gates are read again.
    let steps: Vec<(&str, Value, bool)> = vec![
        ("a plain post", message(501, "message"), false),
        ("a verdict", message(502, "verdict"), true),
        ("the task's record", task_changed(5.0), true),
        (
            "another branch's tip",
            json!([{"region":"branches","data":["tools/other"]}]),
            false,
        ),
        (
            "its branch's tip",
            json!([{"region":"branches","data":[BRANCH]}]),
            true,
        ),
        (
            "a card in another document",
            json!([{"region":"pending","data":{"key":"/docs/OTHER.md#x","removed":true}}]),
            false,
        ),
        (
            "a card in a document it links",
            json!([{"region":"pending","data":{"key":"/docs/look-parity.md#palette","removed":true}}]),
            true,
        ),
        ("its test starts", machine_work(Some(10.0)), true),
        ("its test moves on in time", machine_work(Some(40.0)), false),
        ("its test ends", machine_work(None), true),
    ];
    for (version, (said, changes, again)) in (4183..).zip(steps) {
        push(&site, version, changes);
        if again {
            expected += 1;
            wait_reads(&mut page, &site, expected);
            drawn(&mut page, 4);
        }
        // Long enough for a read the page should not make to show up.
        pause(&mut page, 400);
        assert_eq!(reads(&site), expected, "after {said}");
    }
    assert!(
        page.wait_for("document.querySelector('#message-501') !== null", SHOWN),
        "the plain post was applied, and asked nothing"
    );
}

#[test]
fn task_page_gates_are_read_as_the_page_mounts_however_far_below_the_screen() {
    let site = site(&pictured());
    // A head 150 dependencies tall puts the gates far below a phone's screen.
    site.set_json(
        "/api/task-page",
        &task_page(|page| {
            page["task"]["depends"] = json!(
                (0..150)
                    .map(|n| format!("visual/dep-{n}"))
                    .collect::<Vec<_>>()
            );
        }),
    );
    let Some(mut page) = chrome() else { return };
    page.viewport(390, 844, true);
    page.open(&site.url(PAGE));
    assert!(page.wait_for(
        "document.querySelectorAll('.task-head .facts li').length === 150 && document.querySelector('.task-gates-slot') !== null",
        SHOWN
    ));
    page.frames_with_fonts();
    assert_eq!(
        page.eval("Math.round(document.querySelector('.task-gates-slot').getBoundingClientRect().top) > innerHeight + 600"),
        json!(true),
        "the card is far below the screen"
    );
    // A part of the page, read as it mounts behind nothing: drawn before the reader gets there.
    drawn(&mut page, 4);
    assert_eq!(reads(&site), 1, "one read, with no scroll");
    page.eval("document.querySelector('.task-gates-slot').scrollIntoView()");
    page.frames_with_fonts();
    pause(&mut page, 400);
    assert_eq!(reads(&site), 1, "nearing the screen asks nothing more");
}

/// The brief's top on the page, in px from the document's start.
const BRIEF: &str =
    "Math.round(document.querySelector('.task-body').getBoundingClientRect().top + scrollY)";

/// Forget every page this device kept (`shell/cache.ts`), beside the page's own connection: the
/// next open reads the server's page before it draws.
const FORGET_KEPT: &str = r#"(async () => {
    const db = await new Promise(resolve => {
        const request = indexedDB.open("symphony-cache");
        request.onupgradeneeded = () => request.transaction.abort();
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => resolve(null);
    });
    if (!db) return true;
    await new Promise(resolve => {
        const transaction = db.transaction(["items", "parts"], "readwrite");
        transaction.objectStore("items").clear();
        transaction.objectStore("parts").clear();
        transaction.oncomplete = resolve;
    });
    db.close();
    return true;
})()"#;

/// Where the brief stands while the gates are read and once they are drawn.
fn brief_while_and_after(page: &mut Chrome, site: &Site, count: usize) -> (i64, i64) {
    let asked = reads(site);
    site.hold_route(GATES, true);
    page.open(&site.url(PAGE));
    wait_reads(page, site, asked + 1);
    assert!(page.wait_for("document.querySelector('.task-body') !== null", SHOWN));
    page.frames_with_fonts();
    let before = page.eval(BRIEF).as_i64().unwrap();
    site.hold_route(GATES, false);
    if count > 0 {
        drawn(page, count);
    } else {
        test_time::until_true("the gates answered", STALL, || {
            page.requests()
                .iter()
                .any(|request| request.url.contains(GATES) && request.finished)
        })
        .expect("the gates were never answered");
        page.frames_with_fonts();
    }
    (before, page.eval(BRIEF).as_i64().unwrap())
}

#[test]
fn task_page_gates_hold_the_room_they_will_take_so_the_brief_stays_put() {
    let site = site(&pictured());
    let Some(mut page) = chrome() else { return };
    page.open(&site.url("/"));
    let mut table = Vec::new();
    for (view_name, width, height, mobile) in
        [("phone", 390, 844, true), ("wide", 1280, 900, false)]
    {
        page.viewport(width, height, mobile);
        for count in [0, 1, 4, 12] {
            let answer = short(count);
            site.set_json(GATES, &answer);
            // Nothing known; the server's count with the page; that and this browser's room.
            for known in ["nothing", "server", "browser"] {
                if known != "browser" {
                    page.eval("localStorage.clear()");
                    // Nor the page this device kept, which a return draws before its answer.
                    assert_eq!(page.eval(FORGET_KEPT), json!(true));
                }
                site.set_json(
                    "/api/task-page",
                    &task_page(|page| {
                        if known != "nothing" {
                            page["gates"] = seen(&answer);
                        }
                    }),
                );
                let (before, after) = brief_while_and_after(&mut page, &site, count);
                table.push((view_name, count, known, before, after));
            }
        }
    }
    eprintln!("view   gates  known    brief while read  after  moved");
    for (view_name, count, known, before, after) in &table {
        eprintln!(
            "{view_name:<6} {count:>5}  {known:<8} {before:>16}  {after:>5}  {:>+5}",
            after - before
        );
    }
    for (view_name, count, known, before, after) in &table {
        // The pictures' details wrap on a phone, which only this browser's own drawing knows.
        let held = match (*view_name, *known) {
            (_, "browser") => true,
            ("wide", "server") => true,
            ("phone", "server") => *count != 4,
            _ => false,
        };
        if held {
            assert!(
                (after - before).abs() <= 3,
                "{view_name}, {count} gates, {known} known: the brief moved {before} → {after}"
            );
        }
    }
}
