//! What the stale notice and its invisible reserve leave clear: the header, fixed controls and
//! page-end content, at each width and across the phone breakpoint; on a phone, the composer
//! wherever the page is scrolled, while a page with none keeps its notices on screen.
use super::{
    SHOWN, cache_documents, chrome, chrome::Chrome, document_landing::TWO_FRAMES, look_parity,
    restored, site, site::Site, wait_for_board, wait_for_feed,
};
use serde_json::{Value, json};

#[test]
fn a_stale_notice_leaves_header_and_fixed_controls_uncovered() {
    let site = site();
    site.set_json(
        "/api/pending",
        &json!({"docs":["docs/PROBE.md"],"items":[{"path":"docs/PROBE.md","slug":"probe","href":"/docs/PROBE.md#probe","branch":null}]}),
    );
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/"));
    wait_for_board(&mut chrome);
    assert!(chrome.wait_for(
        "document.querySelector('#quick-open') !== null && document.querySelector('.frame-pending') !== null",
        SHOWN
    ));
    wait_for_feed(&site).expect("the page never followed the feed");
    site.fail_feeds(usize::MAX);
    site.drop_feeds();
    assert!(chrome.wait_for(
        "document.querySelector('main[data-feed=stale] .stale') !== null",
        SHOWN
    ));
    for width in [320, 390, 1024] {
        chrome.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":width,"height":900,"deviceScaleFactor":1,"mobile":false}),
        );
        chrome.eval("scrollTo(0,0); new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
        assert_eq!(
            chrome.eval(r#"(() => {
                const pill = document.querySelector('main[data-feed=stale] .stale').getBoundingClientRect();
                const links = [...document.querySelectorAll('.app-header a')];
                const controls = [...links, ...document.querySelectorAll('.app-header button, #quick-open, .frame-pending')];
                const overlaps = controls.filter(control => {
                    const box = control.getBoundingClientRect();
                    return box.width > 0 && box.height > 0 && pill.left < box.right && pill.right > box.left && pill.top < box.bottom && pill.bottom > box.top;
                }).map(control => control.id || control.textContent.trim());
                return {
                    visible: pill.width > 0 && pill.height > 0 && pill.left >= 0 && pill.right <= innerWidth && pill.top >= 0 && pill.bottom <= innerHeight,
                    headerLinksPresent: links.length >= 5 && links.every(link => { const box = link.getBoundingClientRect(); return box.width > 0 && box.height > 0 && box.top >= 0 && box.bottom <= innerHeight; }),
                    overlaps
                };
            })()"#),
            json!({"visible":true,"headerLinksPresent":true,"overlaps":[]}),
            "at {width}px, the stale notice must leave visible header links and fixed controls uncovered"
        );
    }
}

// 760px is the phone rule's bound, where main's own end padding changes and the reserve's
// lift changes with it. The page keeps the reserve's tallest box, so the lift must not be in it.
#[test]
fn the_notice_reserve_keeps_one_page_end_across_the_phone_breakpoint() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/tasks"));
    assert!(chrome.wait_for(
        "document.querySelector('.kanban') !== null && document.querySelector('.page-notice-reserve') !== null",
        SHOWN
    ));
    let ends: Vec<Value> = [761, 760, 761]
        .into_iter()
        .map(|width| {
            chrome.call(
                "Emulation.setDeviceMetricsOverride",
                json!({"width":width,"height":900,"deviceScaleFactor":1,"mobile":false}),
            );
            chrome.eval(
                "new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))",
            );
            chrome.eval(r#"(() => {
                const reserve = document.querySelector('.page-notice-reserve').getBoundingClientRect();
                return {reserve: reserve.height, end: document.querySelector('main').getBoundingClientRect().bottom - reserve.top};
            })()"#)
        })
        .collect();
    assert_eq!(
        ends[0], ends[1],
        "both sides of 760px keep the same reserve and page end: {ends:?}"
    );
    assert_eq!(
        ends[0], ends[2],
        "returning to 761px restores the reserve and page end: {ends:?}"
    );
}

/// Where the page stands at its end: whether it is scrolled to its end, the page's height, the
/// scroll, and whether the composer's Send clears the notices' reserve under it.
const PAGE_END: &str = r#"(() => {
    const reserve = document.querySelector('.page-notice-reserve').getBoundingClientRect();
    const send = document.querySelector('#task-send button[type=submit]').getBoundingClientRect();
    return {
        atEnd: scrollY === Math.max(0, document.documentElement.scrollHeight - innerHeight),
        height: document.documentElement.scrollHeight,
        scroll: scrollY,
        clear: send.bottom <= reserve.top,
    };
})()"#;

// The task page on a phone's narrowest width (#5559): its stale notice, drawn while the feed is
// lost, goes once the feed is back, and the page's end stays where it was: the reserve under the
// composer keeps its height, so the scroll is never clamped. Each step waits on what it shows.
#[test]
fn at_320_px_a_task_pages_stale_notice_going_live_keeps_the_reserve_under_the_composer() {
    let site = site();
    let responses = look_parity::responses();
    for path in ["/api/task-page", "/api/task-history"] {
        site.set_json(path, &responses[path]);
    }
    let Some(mut chrome) = chrome() else { return };
    chrome.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":320,"height":900,"deviceScaleFactor":1,"mobile":false}),
    );
    chrome.open(&site.url("/task/visual/in-progress"));
    assert!(
        chrome.wait_for(
            "document.querySelector('main[data-feed=live] #task-thread .entry') !== null && document.querySelector('#task-send button[type=submit]') !== null",
            SHOWN
        ),
        "the task page never drew its thread and composer: {}",
        chrome.eval("document.body.innerText")
    );
    wait_for_feed(&site).expect("the page never followed the feed");
    site.fail_feeds(usize::MAX);
    site.drop_feeds();
    assert!(chrome.wait_for(
        "document.querySelector('main')?.dataset.feed === 'stale' && document.querySelector('.page-notices:not([hidden]) [role=status]') !== null",
        SHOWN
    ));
    chrome.eval("scrollTo(0,document.documentElement.scrollHeight); new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
    let stale = chrome.eval(PAGE_END);
    assert_eq!(
        (&stale["atEnd"], &stale["clear"]),
        (&json!(true), &json!(true)),
        "at the page end the composer clears the stale notice's reserve: {stale}"
    );
    site.fail_feeds(0);
    assert!(
        chrome.wait_for(
            "document.querySelector('main')?.dataset.feed === 'live' && document.querySelector('.page-notices')?.hidden === true",
            SHOWN
        ),
        "the stale notice must go once the feed is back: {}",
        chrome.eval("document.body.innerText")
    );
    chrome.eval("new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
    assert_eq!(
        chrome.eval(PAGE_END),
        stale,
        "the notice going keeps the page end and its reserve, and never clamps the scroll"
    );
}

// A task page's failed fetch (here its owner offer) is a page notice, as the board's is: an
// alert in the page's flow would shrink the page when the reconnect's reload clears it.
#[test]
fn a_task_page_error_is_a_notice_and_clearing_it_on_reconnect_keeps_the_page_end() {
    let site = site();
    let responses = look_parity::responses();
    for path in ["/api/task-page", "/api/task-history"] {
        site.set_json(path, &responses[path]);
    }
    site.fail_route("/api/task-offer", true);
    let Some(mut chrome) = chrome() else { return };
    chrome.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":320,"height":900,"deviceScaleFactor":1,"mobile":false}),
    );
    chrome.open(&site.url("/task/visual/in-progress"));
    assert!(
        chrome.wait_for(
            "document.querySelector('#task-thread .entry') !== null && document.querySelector('#task-send button[type=submit]') !== null && document.querySelector('.page-notices [role=alert]')?.textContent === 'Board snapshot unavailable (the server answered 500)'",
            SHOWN
        ),
        "the failed offer must be reported in the notice stack: {}",
        chrome.eval("document.body.innerText")
    );
    assert_eq!(
        chrome.eval("[...document.querySelectorAll('main [role=alert]')].filter(alert => !alert.closest('.page-notices')).length"),
        json!(0),
        "the error is drawn once, outside the page's flow"
    );
    chrome.eval("scrollTo(0,document.documentElement.scrollHeight); new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
    chrome.eval(
        "window.errorScroll=scrollY; window.errorHeight=document.documentElement.scrollHeight",
    );
    assert_eq!(
        chrome.eval(r#"(() => {
            const notice = document.querySelector('.page-notices').getBoundingClientRect();
            const send = document.querySelector('#task-send button[type=submit]').getBoundingClientRect();
            return scrollY === Math.max(0, document.documentElement.scrollHeight - innerHeight) && send.bottom <= notice.top;
        })()"#),
        json!(true),
        "at the page end the composer clears the error notice"
    );
    // The server recovers, so whatever the reconnect asks for again succeeds.
    site.set_json(
        "/api/task-offer",
        &json!({"action":"approve","label":"Approve","branch":"visual/in-progress","payload":null}),
    );
    site.fail_route("/api/task-offer", false);
    wait_for_feed(&site).expect("the page never followed the feed");
    site.fail_feeds(usize::MAX);
    site.drop_feeds();
    assert!(chrome.wait_for(
        "document.querySelector('main')?.dataset.feed === 'stale'",
        SHOWN
    ));
    site.fail_feeds(0);
    assert!(
        chrome.wait_for(
            "document.querySelector('main')?.dataset.feed === 'live' && !document.querySelector('.page-notices [role=alert]')",
            SHOWN
        ),
        "the reconnect's reload must clear the error: {}",
        chrome.eval("document.body.innerText")
    );
    assert_eq!(
        chrome.eval(
            "scrollY === errorScroll && document.documentElement.scrollHeight === errorHeight"
        ),
        json!(true),
        "clearing the error must keep the page end and never clamp the scroll"
    );
}

/// The held-switch notice as the server says it on the holder's pages: six lines at 390 px.
const HELD: &str = "The machine's Symphony stays a project's build, not the release release-1: pantry names the linked plugin fixture-shelf, and a release's server serves a plugin only from its process: a [plugins.process] command for it in its symphony.toml lets the release serve every project (docs/engineering/PLUGIN_PROTOCOL.md).";

/// The two pages with a composer at their end: a task's, its thread's, and a document's, its
/// questions'.
const COMPOSED: [&str; 2] = ["/task/visual/in-progress", "/docs/PROBE.md"];

/// The page's composer: a task's message form, else a document's question form.
const COMPOSER: &str =
    "(document.querySelector('#task-send') ?? document.querySelector('#asks form'))";

/// `page` of [`COMPOSED`] on a fresh stand-in, `width` by `height`, drawn with its composer and
/// following its feed, so the server's notice reaches it.
fn composed(page: &str, width: u32, height: u32) -> Option<(Site, Chrome)> {
    let site = site();
    let responses = look_parity::responses();
    for path in ["/api/task-page", "/api/task-history"] {
        site.set_json(path, &responses[path]);
    }
    restored::document(
        &site,
        vec![json!({"type":"paragraph","spans":[{"type":"text","text":"What the pantry keeps."}]})],
    );
    let mut chrome = chrome()?;
    chrome.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":1,"mobile":width <= 600}),
    );
    chrome.open(&site.url(page));
    assert!(
        chrome.wait_for(
            &format!("{COMPOSER}?.querySelector('button[type=submit]') != null"),
            SHOWN
        ),
        "{page} never drew its composer: {}",
        chrome.eval("document.body.innerText")
    );
    wait_for_feed(&site).expect("the page never followed the feed");
    Some((site, chrome))
}

/// Wait until the page shows `count` notices, then for two frames, so the stack's measurement
/// (`noticeGeometry`, on its resize observer) has run.
fn notices_shown(chrome: &mut Chrome, count: usize) {
    assert!(
        chrome.wait_for(
            &format!(
                "document.querySelectorAll('.page-notices:not([hidden]) > p').length === {count}"
            ),
            SHOWN
        ),
        "the page never showed {count} notices: {}",
        chrome.eval("document.querySelector('.page-notices')?.innerText")
    );
    chrome.eval(TWO_FRAMES);
}

/// Every scroll of the page from its top to its end, 20 px apart, at which a notice shown lies
/// over a part of the composer (its textarea, Attach, its Send or Ask), with the parts it covers.
/// Each scroll is read as laid out; the page is left at its end.
const COVERED: &str = r#"(() => {
    const form = (document.querySelector('#task-send') ?? document.querySelector('#asks form'));
    const parts = {textarea: form.querySelector('textarea'), attach: form.querySelector('.attach-pick'), send: form.querySelector('button[type=submit]')};
    const notices = [...document.querySelectorAll('.page-notices:not([hidden]) > p')];
    const end = Math.max(0, document.documentElement.scrollHeight - innerHeight);
    const covered = [];
    for (let y = 0; ; y = Math.min(y + 20, end)) {
        scrollTo(0, y);
        const under = Object.entries(parts).filter(([, part]) => {
            const b = part.getBoundingClientRect();
            return notices.some(notice => {
                const n = notice.getBoundingClientRect();
                return n.left < b.right && n.right > b.left && n.top < b.bottom && n.bottom > b.top;
            });
        }).map(([name]) => name);
        if (under.length > 0) covered.push([scrollY, under]);
        if (y === end) break;
    }
    return {notices: notices.length, scrolls: end > 0, covered};
})()"#;

/// At the page's end: whether it is there, every notice shown whole in the window, and under the
/// composer.
const AT_THE_END: &str = r#"(() => {
    const form = (document.querySelector('#task-send') ?? document.querySelector('#asks form')).getBoundingClientRect();
    const notices = [...document.querySelectorAll('.page-notices:not([hidden]) > p')].map(notice => notice.getBoundingClientRect());
    const stack = document.querySelector('.page-notices').getBoundingClientRect();
    const reserve = document.querySelector('.page-notice-reserve').getBoundingClientRect();
    return {
        atEnd: scrollY === Math.max(0, document.documentElement.scrollHeight - innerHeight),
        shown: notices.length > 0 && notices.every(n => n.height > 0 && n.top >= 0 && n.bottom <= innerHeight && n.left >= 0 && n.right <= innerWidth),
        underTheComposer: notices.every(n => n.top >= form.bottom),
        inTheReserve: Math.round(stack.top) === Math.round(reserve.top) && Math.round(stack.bottom) <= Math.round(reserve.bottom),
    };
})()"#;

// On a phone the notices are in the page's flow, under its end content, so the long held-switch
// notice, then it and the reconnecting one, lie over no part of a task's or a document's composer
// wherever the page is scrolled, and are read whole at the page's end, standing in their
// reserve's room. Clearing both there keeps the page's end and its scroll: the reserve they stood
// in keeps its height.
#[test]
fn at_390_px_no_notice_covers_the_composer_of_a_task_or_a_document() {
    let end = json!({"atEnd": true, "shown": true, "underTheComposer": true, "inTheReserve": true});
    for page in COMPOSED {
        let Some((site, mut chrome)) = composed(page, 390, 844) else {
            return;
        };
        site.server_notice(HELD);
        notices_shown(&mut chrome, 1);
        assert_eq!(
            chrome.eval(COVERED),
            json!({"notices": 1, "scrolls": true, "covered": []}),
            "{page}: the held-switch notice covers the composer"
        );
        assert_eq!(chrome.eval(AT_THE_END), end, "{page}: one notice");
        // A page laid out as a grid (Reports' sections are) leaves the stack in its reserve's room:
        // it is placed in the block it shares with the reserve, never by the page around it.
        chrome.eval("document.querySelector('main').style.display = 'grid'");
        chrome.eval(TWO_FRAMES);
        assert_eq!(
            chrome.eval(AT_THE_END)["inTheReserve"],
            json!(true),
            "{page}: the stack left its reserve in a grid"
        );
        chrome.eval("document.querySelector('main').style.display = ''");
        chrome.eval(TWO_FRAMES);
        site.fail_feeds(usize::MAX);
        site.drop_feeds();
        notices_shown(&mut chrome, 2);
        assert_eq!(
            chrome.eval(COVERED),
            json!({"notices": 2, "scrolls": true, "covered": []}),
            "{page}: the held-switch and the reconnecting notices cover the composer"
        );
        assert_eq!(chrome.eval(AT_THE_END), end, "{page}: two notices");
        chrome.eval(
            "window.endScroll = scrollY; window.endHeight = document.documentElement.scrollHeight",
        );
        site.fail_feeds(0);
        notices_shown(&mut chrome, 1);
        wait_for_feed(&site).expect("the page never followed the feed again");
        site.server_notice("");
        assert!(
            chrome.wait_for(
                "document.querySelector('.page-notices')?.hidden === true",
                SHOWN
            ),
            "{page}: the notices never went: {}",
            chrome.eval("document.querySelector('.page-notices')?.innerText")
        );
        chrome.eval(TWO_FRAMES);
        assert_eq!(
            chrome.eval(
                "scrollY === endScroll && document.documentElement.scrollHeight === endHeight"
            ),
            json!(true),
            "{page}: clearing the notices at the page's end must keep its end and never clamp its scroll"
        );
    }
}

/// Where the notices stand in the window: placed as the sheet places them, their gap to its foot
/// and its left, and whether the toasts are set above them.
const FLOATING: &str = r#"(() => {
    const stack = document.querySelector('.page-notices');
    const box = stack.getBoundingClientRect();
    return {
        position: getComputedStyle(stack).position,
        foot: Math.round(innerHeight - box.bottom),
        left: Math.round(box.left),
        toasts: getComputedStyle(document.documentElement).getPropertyValue('--notice-toast-bottom') === `${innerHeight - box.top + 8}px`,
    };
})()"#;

// Past a phone's width the notices float over the window's foot as before, from the first width
// past it to a wide desk: 132 px over its foot and 16 px from its left, wherever the page is
// scrolled, with the toasts above them.
#[test]
fn past_a_phones_width_the_notices_float_over_the_windows_foot() {
    let floating = json!({"position": "fixed", "foot": 132, "left": 16, "toasts": true});
    for width in [601, 1440] {
        let Some((site, mut chrome)) = composed(COMPOSED[0], width, 900) else {
            return;
        };
        site.server_notice(HELD);
        notices_shown(&mut chrome, 1);
        for scroll in [
            "scrollTo(0, 0)",
            "scrollTo(0, document.documentElement.scrollHeight)",
        ] {
            chrome.eval(scroll);
            assert_eq!(chrome.eval(FLOATING), floating, "{width} px, {scroll}");
        }
    }
}

/// Whether every notice shown is whole in the window.
const ON_SCREEN: &str = "[...document.querySelectorAll('.page-notices:not([hidden]) > p')].every(notice => { const box = notice.getBoundingClientRect(); return box.height > 0 && box.top >= 0 && box.bottom <= innerHeight && box.left >= 0 && box.right <= innerWidth; })";

// The pages that end in no composer keep their notices floating on a phone as wide, over the
// window's foot wherever the page is scrolled: the board, read in its own scroller, home, the
// queue, the agents page and a folder, whose end is its entries. In the page's flow there, the
// held-switch and the reconnecting notices would show only past the whole page, and they are its
// one sign that it is out of date.
#[test]
fn on_a_phone_the_pages_without_a_composer_keep_their_notices_on_screen() {
    let floating = json!({"position": "fixed", "foot": 132, "left": 16, "toasts": true});
    let Some(mut chrome) = chrome() else { return };
    for page in ["/tasks", "/", "/queue", "/agents", "/docs/"] {
        let site = site();
        if page == "/docs/" {
            // The folder follows its intro's feed: an intro of its own, read as the server reads one.
            let intro = cache_documents::document("docs/README.md", "Documents", 4182);
            site.set_document(&intro);
            let mut outline = intro.clone();
            for section in outline["sections"].as_array_mut().unwrap() {
                section["blocks"] = json!([]);
            }
            outline["threads"] = json!({});
            site.set_json(
                "/api/folder",
                &json!({"path": "docs", "title": "Documents", "crumbs": [], "docs": 1, "intro": outline,
                        "entries": [{"kind": "document", "path": "docs/PROBE.md", "title": "Probe", "docs": 1, "waiting": 0, "summary": ""}]}),
            );
        }
        // Each page a first visit, on a stand-in and in a browser context of its own.
        chrome.fresh_context();
        chrome.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":390,"height":844,"deviceScaleFactor":1,"mobile":true}),
        );
        chrome.open(&site.url(page));
        wait_for_feed(&site).expect("the page never followed the feed");
        site.server_notice(HELD);
        notices_shown(&mut chrome, 1);
        site.fail_feeds(usize::MAX);
        site.drop_feeds();
        notices_shown(&mut chrome, 2);
        for scroll in [
            "scrollTo(0, 0)",
            "scrollTo(0, document.documentElement.scrollHeight)",
        ] {
            chrome.eval(scroll);
            chrome.eval(TWO_FRAMES);
            assert_eq!(
                (chrome.eval(FLOATING), chrome.eval(ON_SCREEN)),
                (floating.clone(), json!(true)),
                "{page}, {scroll}: the notices must float whole over the window's foot"
            );
        }
    }
}
