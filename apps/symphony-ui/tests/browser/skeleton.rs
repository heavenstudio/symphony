//! A page's skeleton: it stays over the page until the regions on its first screen have their
//! reads (`viewport::Settle`), at most three seconds, and not at all once a read fails; its look
//! follows the theme and the reader's motion setting. Past it, a live update that adds or removes
//! a row leaves what the owner reads or types in where it is, and never scrolls against the
//! owner's own scroll; the chat's form stays at the foot of its sheet.
use super::{SHOWN, chrome, chrome::Chrome, layout_shift, site::Site, wait_for_feed};
use serde_json::{Value, json};

const TASK: &str = "/task/visual/in-progress";

/// When the page's `.settle` was first drawn, when it settled, and whether the task's thread was
/// still waiting for its read then.
const WATCH: &str = r#"window.__settle = {};
new MutationObserver(() => {
    const root = document.querySelector('.settle');
    if (!root) return;
    if (window.__settle.drawn === undefined) window.__settle.drawn = performance.now();
    if (root.dataset.settled === 'true' && window.__settle.at === undefined) {
        window.__settle.at = performance.now();
        window.__settle.thread = document.querySelector('#task-thread')?.dataset.pending ?? null;
    }
}).observe(document, {subtree: true, childList: true, attributes: true, attributeFilter: ['data-settled']});"#;

const PAGE: &str = "document.body ? document.body.innerText.slice(0, 2000) : '(no body)'";

fn site() -> Site {
    layout_shift::site(
        "docs/PROBE.md",
        &layout_shift::document_sections(),
        json!([]),
    )
}

/// A browser at `width` x `height` watching `.settle`, on `path`.
fn open(site: &Site, path: &str, width: u32, height: u32) -> Option<Chrome> {
    let mut browser = chrome()?;
    browser.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width": width, "height": height, "deviceScaleFactor": 1, "mobile": false}),
    );
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": WATCH }),
    );
    browser.open(&site.url(path));
    Some(browser)
}

fn settled(browser: &mut Chrome) -> Value {
    assert!(
        browser.wait_for("window.__settle.at !== undefined", SHOWN),
        "the page never settled; it shows {}",
        browser.eval(PAGE)
    );
    browser.eval(
        "({waited: window.__settle.at - window.__settle.drawn, thread: window.__settle.thread})",
    )
}

#[test]
fn a_task_stays_under_its_skeleton_until_its_thread_is_in() {
    let site = site();
    site.hold_route("/api/task-history", true);
    let Some(mut browser) = open(&site, TASK, 1280, 900) else {
        site.hold_route("/api/task-history", false);
        return;
    };
    assert!(
        browser.wait_for(
            "document.querySelector('.settle article.task #task-thread') !== null",
            SHOWN
        ),
        "the task was never drawn: {}",
        browser.eval(PAGE)
    );
    let under = browser.eval(
        "(() => { const root = document.querySelector('.settle'), cover = root.querySelector(':scope > .skeleton'); return {
            settled: root.dataset.settled, cover: getComputedStyle(cover).visibility,
            status: cover.querySelector('[role=status]').textContent,
            page: getComputedStyle(root.querySelector('article.task')).visibility,
            thread: document.querySelector('#task-thread').dataset.pending}; })()",
    );
    site.hold_route("/api/task-history", false);
    assert_eq!(
        under,
        json!({"settled": "false", "cover": "visible", "status": "Loading…", "page": "hidden", "thread": "true"}),
        "while the thread on its first screen is read, the task is drawn hidden under its skeleton"
    );
    let at = settled(&mut browser);
    assert_eq!(
        at["thread"],
        json!("false"),
        "the task settled before its thread was in: {at}"
    );
    assert_eq!(
        browser.eval(
            "({cover: document.querySelector('.settle > .skeleton') === null, page: getComputedStyle(document.querySelector('article.task')).visibility})"
        ),
        json!({"cover": true, "page": "visible"})
    );
}

#[test]
fn a_task_whose_thread_never_comes_shows_after_three_seconds_with_its_thread_waiting() {
    let site = site();
    site.hold_route("/api/task-history", true);
    let Some(mut browser) = open(&site, TASK, 1280, 900) else {
        site.hold_route("/api/task-history", false);
        return;
    };
    let at = settled(&mut browser);
    let shown = browser.eval(
        "({page: getComputedStyle(document.querySelector('article.task')).visibility, thread: document.querySelector('#task-thread').dataset.pending})",
    );
    site.hold_route("/api/task-history", false);
    assert_eq!(at["thread"], json!("true"), "{at}");
    assert!(
        at["waited"].as_f64().unwrap_or_default() >= 2_900.0,
        "the page gave up on its thread early: {at}"
    );
    assert_eq!(shown, json!({"page": "visible", "thread": "true"}));
}

#[test]
fn a_task_whose_thread_cannot_be_read_shows_at_once_with_the_error() {
    let site = site();
    site.fail_route("/api/task-history", true);
    let Some(mut browser) = open(&site, TASK, 1280, 900) else {
        return;
    };
    let at = settled(&mut browser);
    assert!(
        at["waited"].as_f64().unwrap_or(f64::MAX) < 2_900.0,
        "a failed read waited for the deadline: {at}"
    );
    assert!(
        browser.wait_for(
            "document.querySelector('.page-notices [role=alert]')?.textContent.length > 0",
            SHOWN
        ),
        "the failed read is not said: {}",
        browser.eval(PAGE)
    );
}

#[test]
fn a_skeleton_shimmers_only_for_a_reader_who_accepts_motion_in_the_theme_s_colour() {
    let site = site();
    site.hold_route("/api/section", true);
    let Some(mut browser) = open(&site, "/docs/PROBE.md", 1280, 900) else {
        site.hold_route("/api/section", false);
        return;
    };
    let bars = "[document.querySelector('.settle > .skeleton > .bar'), document.querySelector('.section-loading .skeleton > .bar')]";
    let drawn = browser.wait_for(&format!("{bars}.every(bar => bar !== null)"), SHOWN);
    let look = |browser: &mut Chrome, media: &str| {
        browser.call(
            "Emulation.setEmulatedMedia",
            json!({"features": [{"name": "prefers-reduced-motion", "value": media}]}),
        );
        browser.eval(&format!(
            "{bars}.map(bar => ({{animation: getComputedStyle(bar).animationName, colour: getComputedStyle(bar).backgroundColor}}))"
        ))
    };
    let moving = look(&mut browser, "no-preference");
    let still = look(&mut browser, "reduce");
    browser.eval("document.documentElement.style.setProperty('--skeleton', 'rgb(1, 2, 3)')");
    let themed = look(&mut browser, "no-preference");
    site.hold_route("/api/section", false);
    assert!(drawn, "no skeleton was drawn: {}", browser.eval(PAGE));
    let each = |animation: &str, colour: &str| json!([{"animation": animation, "colour": colour}, {"animation": animation, "colour": colour}]);
    assert_eq!(moving, each("shimmer", "rgba(127, 127, 127, 0.14)"));
    assert_eq!(still, each("none", "rgba(127, 127, 127, 0.14)"));
    assert_eq!(themed, each("shimmer", "rgb(1, 2, 3)"));
}

/// Let the page draw three more frames: what lands is drawn in one, and held in the next.
fn frames(browser: &mut Chrome) {
    browser.eval("window.__framed = false; requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(() => { window.__framed = true; })))");
    assert!(browser.wait_for("window.__framed === true", SHOWN));
}

/// The top of `selector`'s box, and the page's scroll offset.
fn place(browser: &mut Chrome, selector: &str) -> (f64, f64) {
    let at = browser.eval(&format!(
        "({{top: document.querySelector({}).getBoundingClientRect().top, scroll: scrollY}})",
        json!(selector)
    ));
    (
        at["top"].as_f64().unwrap_or(f64::NAN),
        at["scroll"].as_f64().unwrap_or(f64::NAN),
    )
}

#[test]
fn a_message_that_lands_above_the_composer_leaves_the_owner_typing_in_place() {
    let site = site();
    // A thread long enough to put the composer below the first screen.
    let entries: Vec<Value> = (1..=6)
        .map(|id| json!({"message": {"id": id, "slug": "visual/in-progress", "sender": "owner", "recipient": null,
            "kind": "message", "text": "An earlier message of the thread, long enough to wrap onto a second line.", "ts": 1.0},
            "stamp": "2026-11-10", "blocks": []}))
        .collect();
    site.set_json(
        "/api/task-history",
        &json!({"entries": entries, "before": null, "after": 6}),
    );
    let Some(mut browser) = open(&site, TASK, 390, 844) else {
        return;
    };
    assert!(
        browser.wait_for(
            "document.querySelector('.settle[data-settled=true] .history') !== null && document.querySelector('[id^=composer-] textarea') !== null",
            SHOWN
        ),
        "the task and its composer were never drawn: {}",
        browser.eval(PAGE)
    );
    assert!(wait_for_feed(&site), "the page never followed the feed");
    let composer = "[id^=composer-] textarea";
    browser.eval(&format!(
        "(() => {{ const field = document.querySelector({}); field.scrollIntoView({{block: 'end'}}); field.focus(); }})()",
        json!(composer)
    ));
    browser.call(
        "Input.insertText",
        json!({"text": "A reply the owner is typing"}),
    );
    let (before, scrolled) = place(&mut browser, composer);
    assert!(
        scrolled > 0.0,
        "the composer must sit below the first screen for the page to have room to hold it"
    );
    // A busy machine's frames: the message's box gives way to it frames after it is drawn.
    browser.call("Emulation.setCPUThrottlingRate", json!({"rate": 6}));
    let message = json!({"id": 7, "slug": "visual/in-progress", "sender": "agent", "recipient": null,
        "kind": "message", "text": "", "ts": 2.0, "stamp": "2026-11-11",
        "blocks": [{"type": "paragraph", "spans": [{"type": "text", "text": "An answer that lands while the owner types, long enough to wrap over a few lines of a phone's thread."}]}]});
    site.push(
        4183,
        &json!({"version": 4183, "changes": [{"region": "message", "data": message}]}).to_string(),
    );
    assert!(
        browser.wait_for(
            "document.querySelectorAll('#task-thread .entry').length === 7",
            SHOWN
        ),
        "the message never landed: {}",
        browser.eval(PAGE)
    );
    frames(&mut browser);
    let (after, _) = place(&mut browser, composer);
    assert_eq!(
        browser.eval(&format!(
            "(() => {{ const field = document.querySelector({}); return {{focused: document.activeElement === field, text: field.value}}; }})()",
            json!(composer)
        )),
        json!({"focused": true, "text": "A reply the owner is typing"})
    );
    assert!(
        (after - before).abs() < 1.0,
        "the composer moved from {before} to {after}"
    );
}

#[test]
fn the_chat_keeps_its_form_at_the_foot_of_its_sheet_with_new_conversation_just_above() {
    let site = site();
    let chat = layout_shift::CHAT;
    let Some(mut browser) = chrome() else {
        return;
    };
    browser.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width": 390, "height": 844, "deviceScaleFactor": 1, "mobile": true}),
    );
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": chat.held }),
    );
    browser.open(&site.url("/"));
    assert!(
        browser.wait_for(
            &format!("document.querySelector({}) !== null", json!(chat.control)),
            SHOWN
        ),
        "the chat's control was never drawn: {}",
        browser.eval(PAGE)
    );
    browser.eval(&format!(
        "document.querySelector({}).click()",
        json!(chat.control)
    ));
    assert!(
        browser.wait_for(chat.shown, SHOWN),
        "the conversation never landed: {}",
        browser.eval(PAGE)
    );
    let at = browser.eval(
        "(() => { const sheet = document.getElementById('quick-sheet'), box = s => sheet.querySelector(s).getBoundingClientRect();
            const thread = box('.quick-thread'), restart = box('.quick-restart'), form = box('form');
            return {thread: [thread.bottom], restart: [restart.top, restart.bottom], form: [form.top, form.bottom], sheet: [sheet.getBoundingClientRect().bottom]}; })()",
    );
    let edge = |key: &str, index: usize| at[key][index].as_f64().unwrap_or(f64::NAN);
    // In this order, top to bottom, none over the next, the form inside the sheet.
    assert!(
        edge("thread", 0) <= edge("restart", 0) + 0.5
            && edge("restart", 1) <= edge("form", 0) + 0.5
            && edge("form", 1) <= edge("sheet", 0) + 0.5,
        "the thread, New conversation and the form overlap or leave the sheet: {at}"
    );
}

/// The row the owner reads on the task board.
const READ: &str = "[data-slug=\"tools/t-08\"]";

/// Each frame for about a second: the read row's top, the board scroller's offset, the page's and
/// the tops of the reserved boxes still waiting; and each scroller offset the page sets.
const TRACE: &str = "window.__trace = [];
const offset = Object.getOwnPropertyDescriptor(Element.prototype, 'scrollTop');
Object.defineProperty(Element.prototype, 'scrollTop', {configurable: true, get() { return offset.get.call(this); }, set(value) { window.__trace.push(['set', Math.round(performance.now()), value]); offset.set.call(this, value); }});
(function frame(left) { const row = document.querySelector('[data-slug=\"tools/t-08\"]'), board = document.querySelector('.board-scroll'); window.__trace.push([Math.round(performance.now()), row ? row.getBoundingClientRect().top : null, board ? board.scrollTop : null, scrollY, [...document.querySelectorAll('[data-pending=true]')].map(box => Math.round(box.getBoundingClientRect().top))]); if (left > 0) requestAnimationFrame(() => frame(left - 1)); })(60)";

fn task(n: u32, priority: u32) -> Value {
    json!({"slug": format!("tools/t-{n:02}"), "title": format!("Task {n}"), "body": "",
        "status": "ready", "priority": priority, "parent": null, "created_by": "owner",
        "created": 0.0, "updated": 0.0, "links": [], "depends": [], "roles": [],
        "instruction": false})
}

/// Push `row` onto the board's feed as `tools/t-00` at `version`; `None` removes it.
fn land(site: &Site, version: u64, row: Option<Value>) {
    let data = match row {
        Some(row) => json!({"key": "tools/t-00", "value": row}),
        None => json!({"key": "tools/t-00", "removed": true}),
    };
    site.push(
        version,
        &json!({"version": version, "changes": [{"region": "task", "data": data}]}).to_string(),
    );
}

/// The task board at 1280 x 900 with fourteen ready tasks at p2, following the feed, the page
/// scrolled toward `READ`'s row; and where that row then is.
fn board_scrolled_to_a_row(site: &Site) -> Option<(Chrome, f64)> {
    let tasks: Vec<Value> = (1..=14).map(|n| task(n, 2)).collect();
    site.set_json(
        "/api/task-board",
        &json!({"tasks": tasks, "statuses": ["ready"], "counts": [["ready", 14]], "signoff": [], "next": null}),
    );
    let mut browser = open(site, "/tasks", 1280, 900)?;
    assert!(
        browser.wait_for(
            "document.querySelector('[data-slug=\"tools/t-01\"]') !== null",
            SHOWN
        ),
        "the task board was never drawn: {}",
        browser.eval(PAGE)
    );
    assert!(wait_for_feed(site), "the page never followed the feed");
    browser.eval("(() => { const row = document.querySelector('[id=\"row-tools/t-08\"]'); scrollTo(0, row.getBoundingClientRect().top + scrollY - 120); })()");
    assert!(
        browser.wait_for(
            &format!("document.querySelector({}) !== null", json!(READ)),
            SHOWN
        ),
        "the row read never drew"
    );
    let (top, scrolled) = place(&mut browser, READ);
    assert!(scrolled > 0.0, "the row must sit below the first screen");
    // A busy machine's frames: a landed row's box gives way to it frames after it is drawn.
    browser.call("Emulation.setCPUThrottlingRate", json!({"rate": 6}));
    Some((browser, top))
}

#[test]
fn a_task_added_or_removed_above_the_row_the_owner_reads_leaves_it_in_place() {
    let site = site();
    let Some((mut browser, before)) = board_scrolled_to_a_row(&site) else {
        return;
    };
    browser.eval(TRACE);
    land(&site, 4183, Some(task(0, 0)));
    // Its reserved box gives way to the row once it is near the screen.
    assert!(
        browser.wait_for(
            "document.querySelector('[data-slug=\"tools/t-00\"]') !== null",
            SHOWN
        ),
        "the added task never landed: {}",
        browser.eval(PAGE)
    );
    frames(&mut browser);
    let (added, _) = place(&mut browser, READ);
    let trace = browser.eval("window.__trace");
    land(&site, 4184, None);
    assert!(
        browser.wait_for(
            "document.querySelector('[id=\"row-tools/t-00\"]') === null",
            SHOWN
        ),
        "the removed task never left"
    );
    frames(&mut browser);
    let (removed, _) = place(&mut browser, READ);
    assert!(
        (added - before).abs() < 1.0 && (removed - before).abs() < 1.0,
        "the row read moved from {before} to {added} when a task was added above it, and to {removed} when it was removed; [ms, row top, board scroll, page scroll] each frame from the add: {trace}"
    );
}

/// In the frame after the added task is drawn, the owner's own wheel scrolls the board 300 px
/// further down; the board's offset then, and a second later.
const SCROLL_DURING_THE_HOLD: &str = r#"window.__scrolled = null;
new MutationObserver((records, observer) => {
    if (!document.getElementById('row-tools/t-00')) return;
    observer.disconnect();
    requestAnimationFrame(() => {
        const board = document.querySelector('.board-scroll');
        board.dispatchEvent(new WheelEvent('wheel', {deltaY: 300, bubbles: true}));
        board.scrollTop += 300;
        const then = board.scrollTop;
        setTimeout(() => { window.__scrolled = {then, later: board.scrollTop}; }, 1000);
    });
}).observe(document.body, {subtree: true, childList: true});"#;

#[test]
fn the_owner_s_own_scroll_while_a_task_lands_is_never_scrolled_back() {
    let site = site();
    let Some((mut browser, _)) = board_scrolled_to_a_row(&site) else {
        return;
    };
    browser.eval(SCROLL_DURING_THE_HOLD);
    land(&site, 4183, Some(task(0, 0)));
    assert!(
        browser.wait_for("window.__scrolled !== null", SHOWN),
        "the added task never landed: {}",
        browser.eval(PAGE)
    );
    let scrolled = browser.eval("window.__scrolled");
    let (then, later) = (
        scrolled["then"].as_f64().unwrap_or(f64::NAN),
        scrolled["later"].as_f64().unwrap_or(f64::NAN),
    );
    // The owner's scroll took (the board had room for it), and the hold let it be: the new row
    // drawn in place of its 130 px reserved box may move the board after it, never 300 px back.
    assert!(then >= 300.0, "the owner's scroll never took: {scrolled}");
    assert!(
        (later - then).abs() < 130.0,
        "the board was scrolled back from {then} to {later} against the owner's own scroll"
    );
}
