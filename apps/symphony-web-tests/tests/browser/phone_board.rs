//! The approved waiting board on a phone: one mounted stage, Review first, a saved stage,
//! filters behind their button, and no document overflow. Wider screens scroll readable columns
//! horizontally, including beside the open chat.

use serde_json::{Value, json};

use crate::chrome::Chrome;
use crate::site::Site;
use crate::{SHOWN, chrome, dist, wait_for_feed};

/// The lifecycle columns with as many tasks as `columns` gives each.
fn listing_of(columns: &[(&str, usize)]) -> Value {
    let tasks: Vec<Value> = columns
        .iter()
        .flat_map(|&(status, count)| {
            (1..=count).map(move |n| {
                json!({"slug": format!("phone/{status}-{n}"), "title": format!("The task {status} {n}"),
                    "body": "", "status": status, "priority": 3, "parent": null,
                    "created_by": "owner", "created": 0.0, "updated": 0.0, "links": [],
                    "depends": [], "roles": [], "instruction": false})
            })
        })
        .collect();
    let statuses: Vec<&str> = columns.iter().map(|&(status, _)| status).collect();
    let counts: Vec<Value> = columns
        .iter()
        .map(|&(status, count)| json!([status, count]))
        .collect();
    json!({"tasks": tasks, "statuses": statuses, "counts": counts, "signoff": [], "next": null})
}

/// Two tasks in each open lifecycle column.
fn listing() -> Value {
    listing_of(&[
        ("concept", 2),
        ("design", 2),
        ("ready", 2),
        ("in_progress", 2),
        ("review", 2),
    ])
}

fn site_of(listing: &Value) -> Site {
    let site = Site::start(dist(), &crate::common::fixture("board.json"));
    site.set_json("/api/task-board", listing);
    site
}

fn site() -> Site {
    site_of(&listing())
}

fn viewport(page: &mut Chrome, width: u32, height: u32) {
    page.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":1,"mobile":width <= 760}),
    );
}

/// Open the board, live, on a touch screen of 390 by 844 px.
fn open_on_a_phone(page: &mut Chrome, site: &Site) {
    open_on_screen(page, site, 390, 844);
}

fn open_on_screen(page: &mut Chrome, site: &Site, width: u32, height: u32) {
    page.call(
        "Emulation.setTouchEmulationEnabled",
        json!({"enabled":true,"maxTouchPoints":5}),
    );
    viewport(page, width, height);
    page.open(&site.url("/tasks"));
    assert!(
        page.wait_for(
            "document.querySelector('main[data-feed=live] .waiting-board .taskcard') !== null && document.documentElement.dataset.navigation === 'ready'",
            SHOWN
        ),
        "the board never drew live: {}",
        page.eval("document.querySelector('main')?.innerText.slice(0, 600) ?? '(no main)'")
    );
    if let Err(stall) = wait_for_feed(site) {
        panic!("{stall}");
    }
}

const MOUNTED: &str = "[...document.querySelectorAll('.waiting-columns .column')].map(column => column.dataset.status)";

fn choose(page: &mut Chrome, stage: &str) {
    page.eval(&format!("[...document.querySelectorAll('.stage-tabs button')].find(button => button.textContent.startsWith({})).click()", json!(stage)));
}

fn fits(page: &mut Chrome) {
    assert_eq!(
        page.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true),
        "the board must not overflow sideways"
    );
    assert_eq!(page.eval("(() => { const r = document.querySelector('.waiting-columns .taskcard').getBoundingClientRect(); return r.top >= 0 && r.bottom <= innerHeight && r.left >= 0 && r.right <= innerWidth; })()"), json!(true), "the first card is completely on screen");
}

#[test]
fn a_phone_opens_review_and_mounts_only_the_stage_chosen_by_its_tab() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    open_on_a_phone(&mut page, &site);
    assert_eq!(page.eval(MOUNTED), json!(["review"]));
    assert_eq!(page.eval("document.querySelector('.stage-tabs [aria-current=page]').textContent.startsWith('Review')"), json!(true));
    fits(&mut page);
    for (label, status) in [
        ("Ready", "ready"),
        ("Concept", "concept"),
        ("Review", "review"),
    ] {
        choose(&mut page, label);
        assert!(page.wait_for(&format!("JSON.stringify({MOUNTED}) === JSON.stringify([{}]) && document.querySelector('.waiting-columns .taskcard') !== null", json!(status)), SHOWN));
        fits(&mut page);
    }
}

#[test]
fn a_phones_chosen_stage_survives_a_full_load() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    open_on_a_phone(&mut page, &site);
    choose(&mut page, "Ready");
    assert!(page.wait_for(
        "document.querySelector('.waiting-columns .column[data-status=ready] .taskcard') !== null",
        SHOWN
    ));
    page.open(&site.url("/tasks"));
    assert!(page.wait_for("document.querySelector('.waiting-columns .column[data-status=ready] .taskcard') !== null && document.documentElement.dataset.navigation === 'ready'", SHOWN));
    assert_eq!(page.eval(MOUNTED), json!(["ready"]));
    fits(&mut page);
}

/// A refresh starts its position hold after the finger came down. Each subsequent movement
/// arrives before that hold's first frame, as a drag or its momentum can. Explicit offsets make
/// the regression deterministic; this checks the shell's response, not a device's physics.
#[test]
fn a_refresh_mid_drag_or_flick_never_pulls_vertical_scrolling_back() {
    let site = site_of(&listing_of(&[("review", 30)]));
    let Some(mut page) = chrome() else { return };
    open_on_a_phone(&mut page, &site);
    for dragging in [true, false] {
        let movements = page.eval(&format!(
            r#"new Promise(resolve => {{
                scrollTo(0, 600);
                const board = document.querySelector('.waiting-columns');
                const touch = type => board.dispatchEvent(new TouchEvent(type, {{bubbles: true}}));
                const movements = [];
                const move = () => {{
                    touch('touchstart');
                    if (!{dragging}) {{
                        touch('touchmove');
                        document.dispatchEvent(new Event('scroll'));
                        touch('touchend');
                    }}
                    const before = scrollY;
                    window.reviewHoldPlace('.waiting-columns [data-slug]');
                    if ({dragging}) touch('touchmove');
                    scrollBy(0, 180);
                    const wanted = scrollY;
                    requestAnimationFrame(() => {{
                        movements.push([before, wanted, scrollY]);
                        touch('touchend');
                        document.dispatchEvent(new Event('scrollend'));
                        if (movements.length < 3) move();
                        else resolve(movements);
                    }});
                }};
                requestAnimationFrame(move);
            }})"#
        ));
        let samples = movements.as_array().expect("the scroll samples");
        assert_eq!(samples.len(), 3);
        for sample in samples {
            let before = sample[0].as_f64().unwrap();
            let wanted = sample[1].as_f64().unwrap();
            let after = sample[2].as_f64().unwrap();
            assert_eq!(wanted - before, 180.0, "the fixture has room to scroll");
            assert_eq!(
                after, wanted,
                "dragging={dragging}: the hold pulled the scroll back: {movements}"
            );
        }
    }
}

/// A tap, a cancelled drag that scrolled nothing, and a completed flick must all release the
/// touch state. With native anchoring off, only the shell can keep a later layout change still.
#[test]
fn touches_release_the_position_hold_after_their_scrolling_finishes() {
    let site = site_of(&listing_of(&[("review", 30)]));
    let Some(mut page) = chrome() else { return };
    for kind in ["tap", "cancel", "flick"] {
        open_on_a_phone(&mut page, &site);
        let place = page.eval(&format!(
            r#"new Promise(resolve => {{
                document.documentElement.style.overflowAnchor = 'none';
                scrollTo(0,600);
                const board = document.querySelector('.waiting-columns');
                const touch = type => board.dispatchEvent(new TouchEvent(type, {{bubbles: true}}));
                touch('touchstart');
                if ({kind:?} !== 'tap') touch('touchmove');
                if ({kind:?} === 'flick') document.dispatchEvent(new Event('scroll'));
                touch({kind:?} === 'cancel' ? 'touchcancel' : 'touchend');
                if ({kind:?} === 'flick') document.dispatchEvent(new Event('scrollend'));
                requestAnimationFrame(() => {{
                    const row = [...board.querySelectorAll('[data-slug]')].find(row => {{
                        const top = row.getBoundingClientRect().top;
                        return top >= 0 && top < innerHeight;
                    }});
                    const before = row.getBoundingClientRect().top;
                    window.reviewHoldPlace('.waiting-columns [data-slug]');
                    row.style.transform = 'translateY(80px)';
                    const shifted = row.getBoundingClientRect().top;
                    requestAnimationFrame(() => resolve([before, shifted, row.getBoundingClientRect().top]));
                }});
            }})"#
        ));
        let before = place[0].as_f64().unwrap();
        assert_eq!(place[1].as_f64().unwrap() - before, 80.0);
        assert_eq!(place[2].as_f64().unwrap(), before, "{kind}: {place}");
    }
}

/// At the page's top updates scroll nothing. At its bottom an offset the page cannot absorb
/// must go quiet, rather than keep correcting forever or leave an anchor owning later updates.
#[test]
fn overlapping_holds_at_page_boundaries_go_quiet_and_release_their_anchor() {
    let site = site_of(&listing_of(&[("review", 30)]));
    let Some(mut page) = chrome() else { return };
    open_on_a_phone(&mut page, &site);
    let top = page.eval(
        r#"(async () => {
            document.documentElement.style.overflowAnchor = 'none';
            await document.fonts.ready;
            const row = document.querySelector('.waiting-columns [data-slug]');
            const before = row.getBoundingClientRect().top;
            window.reviewHoldPlace('.waiting-columns [data-slug]');
            row.style.transform = 'translateY(80px)';
            window.reviewHoldPlace('.waiting-columns [data-slug]');
            await new Promise(requestAnimationFrame);
            const result = [scrollY, row.getBoundingClientRect().top - before];
            row.style.transform = '';
            scrollTo(0,document.documentElement.scrollHeight);
            return result;
        })()"#,
    );
    assert_eq!(top, json!([0, 80]));
    assert!(page.wait_for(
        "document.querySelector('[data-slug=\"phone/review-30\"]') !== null",
        SHOWN
    ));
    let bottom = page.eval(
        r#"(async () => {
            const row = [...document.querySelectorAll('.waiting-columns [data-slug]')].find(row => {
                const top = row.getBoundingClientRect().top;
                return top >= 0 && top < innerHeight;
            });
            const before = row.getBoundingClientRect().top;
            const scroll = window.scrollBy, writes = [];
            window.scrollBy = (...args) => { writes.push(args); scroll(...args); };
            try {
                window.reviewHoldPlace('.waiting-columns [data-slug]');
                row.style.transform = 'translateY(80px)';
                window.reviewHoldPlace('.waiting-columns [data-slug]');
                for (let n = 0; n < 3; n++) await new Promise(requestAnimationFrame);
                const clamped = row.getBoundingClientRect().top - before;
                // Once the outward displacement disappears, competing anchors would start
                // pulling away from the boundary and back toward it on every frame.
                row.style.transform = '';
                for (let n = 0; n < 8; n++) await new Promise(requestAnimationFrame);
                const settled = row.getBoundingClientRect().top - before, attempts = writes.length;
                for (let n = 0; n < 4; n++) await new Promise(requestAnimationFrame);
                const quiet = writes.length === attempts;
                const next = row.getBoundingClientRect().top;
                window.reviewHoldPlace('.waiting-columns [data-slug]');
                row.style.transform = 'translateY(-80px)';
                await new Promise(requestAnimationFrame);
                return { clamped, settled, attempts, quiet, next, after: row.getBoundingClientRect().top };
            } finally { window.scrollBy = scroll; }
        })()"#,
    );
    assert_eq!(bottom["clamped"], json!(80), "{bottom}");
    assert_eq!(bottom["settled"], json!(0), "{bottom}");
    let attempts = bottom["attempts"].as_u64().unwrap();
    assert_eq!(attempts, 3, "one correction per clamped frame: {bottom}");
    assert_eq!(bottom["quiet"], json!(true), "{bottom}");
    assert_eq!(bottom["after"], bottom["next"], "{bottom}");
}

/// A hold is owned by its scrolling box, and ends on input, removal or pagehide. A queued frame
/// of an old hold must not clear a newer one. Independent boxes still keep independent anchors.
#[test]
fn shared_holds_release_and_leave_other_scrolling_boxes_independent() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    open_on_a_phone(&mut page, &site);
    let result = page.eval(
        r#"(async () => {
            document.documentElement.style.overflowAnchor = 'none';
            const make = left => {
                const box = document.createElement('div');
                box.style.cssText = `position:fixed;left:${left}px;top:100px;width:120px;height:300px;overflow:auto;overflow-anchor:none`;
                box.innerHTML = '<div style="height:100px"></div><div class="hold-probe" style="height:40px">Anchor</div><div style="height:1000px"></div>';
                document.body.append(box);
                box.scrollTop = 50;
                return [box, box.querySelector('.hold-probe')];
            };
            const [one, first] = make(10), [two, second] = make(150);
            await new Promise(requestAnimationFrame);
            const before = [first.getBoundingClientRect().top, second.getBoundingClientRect().top];
            window.reviewHoldPlace('.hold-probe');
            first.style.transform = 'translateY(80px)';
            first.classList.remove('hold-probe');
            window.reviewHoldPlace('.hold-probe');
            second.style.transform = 'translateY(40px)';
            await new Promise(requestAnimationFrame);
            const independent = [first.getBoundingClientRect().top, second.getBoundingClientRect().top];
            // Both holds end immediately, allowing another in the same task before their frames.
            dispatchEvent(new WheelEvent('wheel'));
            first.classList.add('hold-probe');
            window.reviewHoldPlace('.hold-probe');
            first.style.transform = 'translateY(160px)';
            window.reviewHoldPlace('.hold-probe');
            await new Promise(requestAnimationFrame);
            const renewed = first.getBoundingClientRect().top;
            // Replace an anchor while its old hold still has a queued frame.
            const replacement = first.cloneNode(true);
            first.replaceWith(replacement);
            window.reviewHoldPlace('.hold-probe');
            replacement.style.transform = 'translateY(240px)';
            window.reviewHoldPlace('.hold-probe');
            await new Promise(requestAnimationFrame);
            const replaced = replacement.getBoundingClientRect().top;
            dispatchEvent(new PageTransitionEvent('pagehide'));
            window.reviewHoldPlace('.hold-probe');
            replacement.style.transform = 'translateY(320px)';
            window.reviewHoldPlace('.hold-probe');
            await new Promise(requestAnimationFrame);
            const returned = replacement.getBoundingClientRect().top;
            one.remove(); two.remove();
            return { before, independent, renewed, replaced, returned };
        })()"#,
    );
    assert_eq!(result["independent"], result["before"], "{result}");
    for key in ["renewed", "replaced", "returned"] {
        assert_eq!(result[key], result["before"][0], "{key}: {result}");
    }
}

/// A detached element cannot deliver scrollend to the shell's window listener. Removing it must
/// release later layout holds, but not the momentum of another, still-connected scroller. The
/// touch input is synthetic; both element scrolls and the connected scrollend are native events.
/// Release and cancellation target the original element, before or after its removal. A second
/// scroller also has a second contact: its release leaves the detached contact down until last.
#[test]
fn removed_scrollers_release_idle_holds_without_ending_connected_scrolling() {
    let site = site_of(&listing_of(&[("review", 30)]));
    let Some(mut page) = chrome() else { return };
    for (release_before, release_kind, continuing) in [
        (true, "touchend", false),
        (true, "touchend", true),
        (true, "touchcancel", false),
        (true, "touchcancel", true),
        (false, "touchend", false),
        (false, "touchend", true),
        (false, "touchcancel", false),
        (false, "touchcancel", true),
    ] {
        open_on_a_phone(&mut page, &site);
        let result = page.eval(&format!(
            r#"(async () => {{
                document.documentElement.style.overflowAnchor = 'none';
                // The page's own scroll is over before the touch begins: its scrollend comes a
                // frame or more later, and one landing between a release and the touch's first
                // scroll event ends the touch as one that scrolled nothing.
                const from = scrollY, rested = new Promise(resolve => addEventListener('scrollend', resolve, {{once:true}}));
                scrollTo(0,600);
                if (scrollY !== from) await rested;
                await new Promise(requestAnimationFrame);
                const board = document.querySelector('.waiting-columns');
                const row = [...board.querySelectorAll('[data-slug]')].find(row => {{
                    const top = row.getBoundingClientRect().top;
                    return top >= 0 && top < innerHeight;
                }});
                const before = row.getBoundingClientRect().top;
                const scroller = () => {{
                    const box = document.createElement('div');
                    box.style.cssText = 'position:fixed;top:0;right:0;width:40px;height:40px;overflow:auto';
                    const content = document.createElement('div');
                    content.style.height = '400px';
                    box.append(content);
                    document.body.append(box);
                    return box;
                }};
                const removed = scroller(), kept = {continuing} ? scroller() : null;
                const firstContact = new Touch({{identifier:1, target:removed}});
                const contacts = kept ? [firstContact, new Touch({{identifier:2, target:kept}})] : [firstContact];
                const events = [];
                let whileDown = null, releasedConnected = null;
                const touch = (type, target = removed, touches = contacts) => target.dispatchEvent(new TouchEvent(type, {{bubbles:true, touches}}));
                const release = () => {{
                    if (kept) touch({release_kind:?}, kept, [firstContact]);
                    releasedConnected = removed.isConnected;
                    touch({release_kind:?}, removed, []);
                }};
                const ended = kept && new Promise(resolve => kept.addEventListener('scrollend', event => {{
                    events.push(['end', event.isTrusted]);
                    resolve();
                }}, {{once:true}}));
                const displaced = () => {{
                    window.reviewHoldPlace('.waiting-columns [data-slug]');
                    row.style.transform = 'translateY(80px)';
                    const shifted = row.getBoundingClientRect().top;
                    return new Promise(resolve => requestAnimationFrame(() => resolve([shifted, row.getBoundingClientRect().top])));
                }};
                const scrolled = new Promise(resolve => {{
                    kept?.addEventListener('scroll', event => {{
                        events.push(['kept', event.isTrusted]);
                        resolve(displaced());
                    }}, {{once:true}});
                    removed.addEventListener('scroll', event => {{
                        events.push(['removed', event.isTrusted]);
                        removed.remove();
                        const onward = () => {{
                            if (kept) kept.scrollTop = 40;
                            else resolve(displaced());
                        }};
                        if ({release_before}) onward();
                        else {{
                            window.reviewHoldPlace('.waiting-columns [data-slug]');
                            row.style.transform = 'translateY(40px)';
                            requestAnimationFrame(() => {{
                                whileDown = row.getBoundingClientRect().top;
                                row.style.transform = '';
                                release();
                                onward();
                            }});
                        }}
                    }}, {{once:true}});
                }});
                touch('touchstart', removed, [firstContact]);
                if (kept) touch('touchstart', kept);
                touch('touchmove');
                removed.scrollTop = 40;
                if ({release_before}) release();
                const first = await scrolled;
                if (ended) await ended;
                const idle = row.getBoundingClientRect().top;
                window.reviewHoldPlace('.waiting-columns [data-slug]');
                row.style.transform = 'translateY(160px)';
                await new Promise(requestAnimationFrame);
                kept?.remove();
                return {{before, first, idle, after:row.getBoundingClientRect().top, events, whileDown, releasedConnected}};
            }})()"#
        ));
        let before = result["before"].as_f64().unwrap();
        assert_eq!(
            result["releasedConnected"],
            json!(release_before),
            "{result}"
        );
        if !release_before {
            assert_eq!(
                result["whileDown"].as_f64().unwrap(),
                before + 40.0,
                "{result}"
            );
        }
        assert_eq!(result["first"][0].as_f64().unwrap(), before + 80.0);
        assert_eq!(
            result["first"][1].as_f64().unwrap(),
            before + if continuing { 80.0 } else { 0.0 },
            "continuing={continuing}: {result}"
        );
        assert_eq!(result["after"], result["idle"], "{result}");
        assert_eq!(
            result["events"],
            if continuing {
                json!([["removed", true], ["kept", true], ["end", true]])
            } else {
                json!([["removed", true]])
            }
        );
    }
}

#[test]
fn desktop_columns_keep_a_readable_width_and_scroll_inside_the_page_with_chat_open() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1440, 900);
    page.open(&site.url("/tasks"));
    assert!(page.wait_for(
        "document.querySelector('.waiting-columns .taskcard') !== null",
        SHOWN
    ));
    assert_eq!(
        page.eval(MOUNTED),
        json!([
            "concept",
            "design",
            "ready",
            "in_progress",
            "build",
            "test",
            "review",
            "done"
        ])
    );
    for width in [1440, 1100, 800] {
        viewport(&mut page, width, 900);
        for chat in [false, true] {
            if chat {
                page.eval("document.querySelector('#quick-open').click()");
                assert!(page.wait_for(
                    "document.querySelector('#quick-sheet .collapse')?.checkVisibility() === true",
                    SHOWN
                ));
            }
            assert_eq!(page.eval("[...document.querySelectorAll('.waiting-columns .column')].every(column => column.getBoundingClientRect().width >= 280)"), json!(true), "{width}px, chat={chat}");
            assert_eq!(
                page.eval("document.documentElement.scrollWidth <= innerWidth"),
                json!(true)
            );
            page.eval("document.querySelector('.waiting-columns').scrollLeft = 10000");
            assert!(page.wait_for("(() => { const board = document.querySelector('.waiting-columns'); const r = board.getBoundingClientRect(), done = board.querySelector('[data-status=done]').getBoundingClientRect(); return board.scrollLeft > 0 && done.right <= r.right + 1 && done.left >= r.left; })()", SHOWN));
            if chat {
                page.eval("document.querySelector('#quick-sheet .collapse').click()");
            }
        }
    }
    // A keyboard can reach the horizontal scroll surface without choosing a card first.
    page.eval("const board = document.querySelector('.waiting-columns'); board.scrollLeft = 0; board.focus()");
    for kind in ["keyDown", "keyUp"] {
        page.call(
            "Input.dispatchKeyEvent",
            json!({"type":kind,"key":"ArrowRight","code":"ArrowRight","windowsVirtualKeyCode":39}),
        );
    }
    assert!(page.wait_for(
        "document.querySelector('.waiting-columns').scrollLeft > 0",
        SHOWN
    ));
}

#[test]
fn search_and_filters_start_collapsed_and_keep_the_query_when_toggled_at_every_width() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    open_on_a_phone(&mut page, &site);
    assert_eq!(
        page.eval("document.querySelector('#board-filters').checkVisibility()"),
        json!(false)
    );
    page.tap(".board-filters-toggle");
    assert!(page.wait_for("document.querySelector('#board-filters')?.checkVisibility() === true && document.querySelector('.board-filters-toggle').getAttribute('aria-expanded') === 'true'", SHOWN));
    page.eval("(() => { const input = document.querySelector('#board-filters input[type=search]'); input.value = 'found'; input.dispatchEvent(new Event('input', {bubbles: true})); })()");
    if let Err(stall) =
        test_time::until_true("the search's waiting projection read", crate::STALL, || {
            site.targets().iter().any(|target| {
                target.starts_with("/api/task-board?view=waiting&") && target.contains("q=found")
            }) || {
                page.pump(std::time::Duration::from_millis(25));
                false
            }
        })
    {
        panic!("{stall}: {:#?}", site.targets());
    }
    page.tap(".board-filters-toggle");
    assert!(page.wait_for(
        "document.querySelector('#board-filters')?.checkVisibility() === false",
        SHOWN
    ));
    for (width, height) in [(1440, 900), (800, 1000)] {
        viewport(&mut page, width, height);
        assert!(page.wait_for(&format!("innerWidth === {width} && document.querySelector('#board-filters')?.checkVisibility() === false && document.querySelector('.board-filters-toggle')?.checkVisibility() === true"), SHOWN));
        page.eval("document.querySelector('.board-filters-toggle').click()");
        assert!(page.wait_for("document.querySelector('#board-filters').checkVisibility() && document.querySelector('#board-filters input').value === 'found'", SHOWN));
        page.eval("document.querySelector('.board-filters-toggle').click()");
    }
}

#[test]
fn backlog_stays_horizontal_while_pressed_and_returns_to_the_columns() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1100, 850);
    page.open(&site.url("/tasks"));
    assert!(page.wait_for("document.querySelector('.board-backlog') !== null", SHOWN));
    let position = page.eval("(() => { const r = document.querySelector('.board-backlog').getBoundingClientRect(); return [r.x + r.width/2, r.y + r.height/2]; })()");
    page.call("Input.dispatchMouseEvent", json!({"type":"mousePressed","x":position[0],"y":position[1],"button":"left","clickCount":1}));
    assert_eq!(page.eval("(() => { const b = document.querySelector('.board-backlog'); const m = new DOMMatrixReadOnly(getComputedStyle(b).transform); return b.matches(':active') && m.a > 0 && m.d > 0 && m.b === 0 && m.c === 0; })()"), json!(true));
    page.call("Input.dispatchMouseEvent", json!({"type":"mouseReleased","x":position[0],"y":position[1],"button":"left","clickCount":1}));
    assert!(page.wait_for("document.querySelector('.board-backlog').getAttribute('aria-expanded') === 'true' && document.querySelectorAll('.waiting-columns .column').length === 1 && document.querySelector('.column[data-status=backlog]') !== null", SHOWN));
    page.eval("document.querySelector('.back-to-stages').click()");
    assert!(page.wait_for("document.querySelector('.board-backlog').getAttribute('aria-expanded') === 'false' && document.querySelectorAll('.waiting-columns .column').length === 8", SHOWN));
}

#[test]
fn an_explicit_row_link_reveals_its_offscreen_desktop_column() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1100, 850);
    page.open(&site.url("/tasks#row-phone%2Freview-1"));
    assert!(page.wait_for("(() => { const board = document.querySelector('.waiting-columns'), card = document.querySelector('[data-slug=\"phone/review-1\"]'); if (!board || !card) return false; const r = card.getBoundingClientRect(), b = board.getBoundingClientRect(); return board.scrollLeft > 0 && r.left >= b.left && r.right <= b.right && r.top >= 0 && r.bottom <= innerHeight; })()", SHOWN));
}

#[test]
fn returning_from_a_list_restores_the_columns_horizontal_position() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1100, 850);
    page.open(&site.url("/tasks"));
    assert!(page.wait_for(
        "document.querySelector('.column[data-status=done]') !== null",
        SHOWN
    ));
    for list in ["backlog", "cancelled", "wait"] {
        page.eval("(() => { const board = document.querySelector('.waiting-columns'); board.scrollLeft = board.scrollWidth; })()");
        assert!(page.wait_for(
            "document.querySelector('.waiting-columns').scrollLeft > 0",
            SHOWN
        ));
        page.eval("window.beforeList = document.querySelector('.waiting-columns').scrollLeft");
        let opener = match list {
            "backlog" => ".board-backlog",
            "cancelled" => "[data-open-stage=cancelled]",
            _ => ".wait-chip.k-queued",
        };
        page.eval(&format!(
            "document.querySelector({}).click()",
            json!(opener)
        ));
        assert!(page.wait_for("document.querySelector('.waiting-pipeline') === null && document.querySelectorAll('.waiting-columns .column').length === 1", SHOWN));
        let back = if list == "wait" {
            ".open-count"
        } else {
            ".back-to-stages"
        };
        page.eval(&format!("document.querySelector({}).click()", json!(back)));
        assert!(page.wait_for("document.querySelector('.waiting-pipeline') !== null && document.querySelector('.waiting-columns').scrollLeft === window.beforeList", SHOWN));
    }
}
