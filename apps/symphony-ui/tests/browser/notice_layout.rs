//! What the stale notice and its invisible reserve leave clear: the header, fixed controls and
//! page-end content, at each width and across the phone breakpoint.
use super::{SHOWN, chrome, look_parity, site, wait_for_board, wait_for_feed};
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
