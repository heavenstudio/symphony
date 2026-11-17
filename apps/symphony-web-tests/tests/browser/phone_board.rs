//! The approved waiting board on a phone: one mounted stage, Review first, a saved stage,
//! filters behind their button, and no horizontal overflow. Wide screens show every lifecycle
//! heading together. Assertions follow the controls and layout the reader sees.

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
    page.call(
        "Emulation.setTouchEmulationEnabled",
        json!({"enabled":true,"maxTouchPoints":5}),
    );
    viewport(page, 390, 844);
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
                scrollTo(0,600);
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
fn all_six_lifecycle_headings_fit_together_on_a_wide_board() {
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
            "build-test",
            "review",
            "done"
        ])
    );
    assert_eq!(page.eval("[...document.querySelectorAll('.waiting-columns .column h2')].every(heading => { const r = heading.getBoundingClientRect(); return r.left >= 0 && r.right <= innerWidth && r.top >= 0 && r.bottom <= innerHeight; })"), json!(true));
    assert_eq!(
        page.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
    assert_eq!(page.eval("document.querySelector('.column[data-status=blocked], .signoff-lane, .pipeline, .board-scroll')"), Value::Null);
}

#[test]
fn a_phones_search_and_filters_open_from_their_button_and_a_desk_shows_them() {
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
        assert!(page.wait_for(&format!("innerWidth === {width} && document.querySelector('#board-filters')?.checkVisibility() === true && document.querySelector('.board-filters-toggle')?.checkVisibility() === false"), SHOWN));
    }
}
