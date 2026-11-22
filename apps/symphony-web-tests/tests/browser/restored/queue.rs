//! The restored queue page: a stylesheet that fails leaves the page before it and is tried again,
//! the queue's counts, order, scope, anchors and refresh keep the owner's reading position, and a
//! card's Later and Undo keep the keyboard's focus.
use crate::{
    SHOWN, chrome, chrome::Chrome, common, components::key, dist, focus_on, site::Site, texts,
    wait_for_board, wait_for_feed,
};
use serde_json::{Value, json};

fn queue_fixture() -> Value {
    let mut card: Value = serde_json::from_str(&common::fixture("board.json")).unwrap();
    let mut first = card["next"][0].take();
    first["slug"] = json!("first");
    first["title"] = json!("First decision");
    first["doc"] = json!("@topic/docs/PROBE.md");
    let mut second = first.clone();
    second["slug"] = json!("second");
    second["title"] = json!("Second decision");
    second["task"] = json!("topic");
    let item = |slug: &str| json!({"slug":slug,"path":"@topic/docs/PROBE.md","title":slug,"href":format!("/@topic/docs/PROBE.md#{slug}"),"kind":"question","text":"Review it","branch":null});
    json!({"waiting":[second,first],"spot_check":[item("spot")],"agent":[item("agent")],"branches":[]})
}

#[test]
fn queue_stylesheet_failure_keeps_the_previous_page_and_allows_retry() {
    let site = Site::start(dist(), &common::fixture("board.json"));
    let Some(mut browser) = chrome() else { return };
    browser.open(&site.url("/"));
    wait_for_board(&mut browser);
    browser.call(
        "Network.setBlockedURLs",
        json!({"urls":["*/assets/queue-*.css"]}),
    );
    browser.eval(
        "history.pushState(null, '', '/queue'); dispatchEvent(new PopStateEvent('popstate'))",
    );
    assert!(browser.wait_for(
        "document.documentElement.dataset.navigation === 'failed'",
        SHOWN
    ));
    assert_eq!(browser.eval("document.querySelector('.hero-title') !== null && document.getElementById('you') === null"), json!(true),
        "a stylesheet failure must leave the previous page mounted");
    browser.call("Network.setBlockedURLs", json!({"urls":[]}));
    browser.eval("document.querySelector('#module-error button').click()");
    assert!(browser.wait_for("document.documentElement.dataset.navigation === 'ready' && document.querySelector('[data-ready=queue] #you') !== null", SHOWN),
        "retry must fetch the failed stylesheet and mount the queue");
    // queue.css overrides the generic card padding only when its rules actually apply.
    assert_eq!(
        browser.eval("getComputedStyle(document.getElementById('you')).paddingTop"),
        json!("6px"),
        "the retried queue stylesheet must apply when the queue is ready"
    );
}

/// Wait until the page shown has opened a feed stream since the stand-in had served `before` feed
/// requests, and that stream is the only one open: the page it replaced closed its own. A quiet
/// machine has it within 10 s; a stall past its backstop ([`test_time::backstop`]) is the error.
fn follows_alone(site: &Site, before: usize) -> Result<(), test_time::Stall> {
    test_time::until_true(
        "the page shown following the feed on the only stream open",
        test_time::backstop(std::time::Duration::from_secs(10)),
        || site.feed_requests() > before && site.feeds() == 1,
    )
}

/// The queue and home are kinds of their own: a move from one to the other, by a
/// link, Back or Forward, keeps the page before it drawn until the next is, never a frame with no
/// page, and the page shown follows the feed on a stream of its own.
#[test]
fn moves_between_home_and_the_queue_draw_each_page_and_keep_the_feed_both_ways() {
    let site = Site::start(dist(), &common::fixture("board.json"));
    let mut queue = queue_fixture();
    site.set_json("/api/queue", &queue);
    let Some(mut browser) = chrome() else { return };
    browser.open(&site.url("/"));
    wait_for_board(&mut browser);
    follows_alone(&site, 0).expect("home never followed the feed");
    // Each frame drawn from here on: one with no page root shown and drawn is a blank page.
    browser.eval(
        r#"window.blankFrames = 0;
        window.drawnFrames = 0;
        const tick = () => {
            const roots = [...document.querySelectorAll('#main > [id^="review-root-"]')];
            drawnFrames += 1;
            if (!roots.some(root => !root.hidden && root.firstElementChild)) blankFrames += 1;
            requestAnimationFrame(tick);
        };
        requestAnimationFrame(tick);"#,
    );
    // Home is the kind `review`, in its root; the queue the kind `queue`, in its own.
    let queue_shown = "document.documentElement.dataset.navigation === 'ready' \
        && document.querySelector('#review-root-queue:not([hidden]) [data-ready=queue] #you h2') !== null \
        && document.getElementById('review-root-review')?.hidden === true";
    let home_shown = "document.documentElement.dataset.navigation === 'ready' \
        && document.querySelector('#review-root-review:not([hidden]) [data-ready=board] .hero-title') !== null \
        && document.getElementById('review-root-queue')?.hidden === true";

    let before = site.feed_requests();
    browser.eval("document.querySelector('a[href=\"/queue\"]').click()");
    assert!(
        browser.wait_for(queue_shown, SHOWN),
        "home's link must show the queue"
    );
    follows_alone(&site, before).expect("the queue never followed the feed");
    queue["waiting"] = json!([]);
    site.set_json("/api/queue", &queue);
    site.push(
        4183,
        &json!({"version":4183,"changes":[{"region":"queue-item","data":{"key":"fixture","removed":true}}]}).to_string(),
    );
    assert!(
        browser.wait_for(
            "document.querySelector('#you h2')?.textContent === 'Waiting on you (0)'",
            SHOWN
        ),
        "the queue must follow the feed it opened"
    );

    let before = site.feed_requests();
    browser.eval("history.back()");
    assert!(browser.wait_for(home_shown, SHOWN), "Back must show home");
    follows_alone(&site, before).expect("home never followed the feed again");

    let before = site.feed_requests();
    browser.eval("history.forward()");
    assert!(
        browser.wait_for(queue_shown, SHOWN),
        "Forward must show the queue"
    );
    follows_alone(&site, before).expect("the queue never followed the feed again");
    assert_eq!(
        texts(&mut browser, "#you h2"),
        ["Waiting on you (0)"],
        "the queue shown again reads the queue as it is"
    );
    assert_eq!(
        browser.eval("drawnFrames > 0 && blankFrames"),
        json!(0),
        "no frame may show no page, of the {} drawn",
        browser.eval("drawnFrames")
    );
}

/// The feed's `client` event naming another build than the page's loads the queue again, once per
/// build: the page loaded again (still the old build here, as this stand-in serves the same files)
/// hears the same event again and stays, following the feed.
#[test]
fn the_feeds_client_event_naming_another_build_loads_the_queue_again_once() {
    let site = Site::start(dist(), &common::fixture("board.json"));
    let mut queue = queue_fixture();
    site.set_json("/api/queue", &queue);
    let Some(mut browser) = chrome() else { return };
    browser.open(&site.url("/queue"));
    let drawn = "document.querySelector('[data-ready=queue] #you h2') !== null";
    assert!(browser.wait_for(drawn, SHOWN), "the queue never drew");
    follows_alone(&site, 0).expect("the queue never followed the feed");
    browser.eval("window.firstPage = true");
    let loads = |site: &Site| {
        site.requests()
            .iter()
            .filter(|path| *path == "/queue")
            .count()
    };
    assert_eq!(loads(&site), 1);
    let client = |version: u64| {
        json!({"version":version,"changes":[{"region":"client","data":{"build":"/assets/symphony-shell-0000000000000000.js"}}]})
            .to_string()
    };
    let before = site.feed_requests();
    site.push(4183, &client(4183));
    if let Err(stall) = test_time::until_true(
        "the queue loaded again for the build the feed named",
        test_time::backstop(std::time::Duration::from_secs(10)),
        || loads(&site) >= 2,
    ) {
        panic!("{stall}: {:?}", site.requests());
    }
    assert!(
        browser.wait_for(drawn, SHOWN),
        "the queue loaded again never drew"
    );
    follows_alone(&site, before).expect("the queue loaded again never followed the feed");
    assert_eq!(
        browser.eval("window.firstPage === undefined"),
        json!(true),
        "the page shown is a new one"
    );
    // The same build named again loads nothing: the page then hears the queue's next change.
    browser.eval("window.secondPage = true");
    site.push(4184, &client(4184));
    queue["waiting"] = json!([]);
    site.set_json("/api/queue", &queue);
    site.push(
        4185,
        &json!({"version":4185,"changes":[{"region":"queue-item","data":{"key":"fixture","removed":true}}]}).to_string(),
    );
    assert!(
        browser.wait_for(
            "document.querySelector('#you h2')?.textContent === 'Waiting on you (0)'",
            SHOWN
        ),
        "the page loaded again stopped following the feed"
    );
    assert_eq!(
        (loads(&site), browser.eval("window.secondPage === true")),
        (2, json!(true)),
        "the queue loaded again for a build it had loaded for already"
    );
}

/// The card `slug` of docs/PROBE.md waiting on the owner, `postponed` or not.
fn waiting(slug: &str, postponed: bool) -> Value {
    let mut board: Value = serde_json::from_str(&common::fixture("board.json")).unwrap();
    let mut card = board["next"][0].take();
    card["slug"] = json!(slug);
    card["title"] = json!(slug);
    card["doc"] = json!("docs/PROBE.md");
    card["postponed"] = json!(postponed);
    card
}

/// What waits on the owner is `cards`, and nothing else waits.
fn waiting_on_you(cards: &[Value]) -> Value {
    json!({"waiting":cards,"spot_check":[],"agent":[],"branches":[]})
}

/// Press `pressed` (a key, its code and its key code) on the button `button` while the server holds
/// its post, and once more while it saves, which posts nothing: the focus stays on it while it
/// saves, and on the button that replaces it, saying `label`, once the server has answered and, as
/// the server does, its feed's event `version` has made the queue read `after`.
fn press_keeping_focus(
    site: &Site,
    browser: &mut Chrome,
    button: &str,
    pressed: (&str, &str, u16),
    (after, version): (&Value, u64),
    label: &str,
) {
    browser.eval(&format!(
        "document.querySelector({}).focus()",
        json!(button)
    ));
    site.hold_route("/api/postpone", true);
    key(browser, pressed.0, pressed.1, pressed.2);
    assert!(
        browser.wait_for(
            &format!(
                "document.querySelector({})?.textContent === 'Saving…'",
                json!(button)
            ),
            SHOWN
        ),
        "{} says it saves",
        pressed.0
    );
    assert_eq!(
        focus_on(browser, button),
        json!(true),
        "the focus stays on the button while it saves"
    );
    assert!(
        browser.wait_for(&crate::drawn_disabled(button), SHOWN),
        "it looks as a disabled button does while it saves"
    );
    // Pressed again while it saves: a second post would show in the test's count of them.
    key(browser, pressed.0, pressed.1, pressed.2);
    site.set_json("/api/queue", after);
    site.hold_route("/api/postpone", false);
    site.push(
        version,
        &json!({"version":version,"changes":[{"region":"queue-item-order","data":{"key":"docs/PROBE.md#alpha","after":null}}]}).to_string(),
    );
    assert!(
        browser.wait_for(
            &format!(
                "document.querySelector({})?.textContent === {}",
                json!(button),
                json!(label)
            ),
            SHOWN
        ),
        "the card's button says {label} once the server answered"
    );
}

/// Later and its Undo, pressed from the keyboard, keep the focus on the card's button: while the
/// server saves, where a second press posts nothing, and once the card's new state is drawn, on
/// Undo Later after Later and on Later after Undo, its row moved back before the others. Later here is the only card's, so the server
/// names none next and the reader stays; with another card waiting they go to its document.
#[test]
fn later_and_undo_keep_the_focus_on_the_cards_button() {
    let site = Site::start(dist(), &common::fixture("board.json"));
    site.set_json("/api/queue", &waiting_on_you(&[waiting("alpha", false)]));
    site.set_json(
        "/api/postpone",
        &json!({"ok":true,"changed":true,"next":null}),
    );
    let Some(mut browser) = chrome() else { return };
    browser.open(&site.url("/queue"));
    let button = "[data-card=\"docs/PROBE.md#alpha\"] .postpone-controls button";
    assert!(
        browser.wait_for(&crate::visible(button), SHOWN),
        "the card's Later shows"
    );
    wait_for_feed(&site).expect("the queue never followed the feed");
    press_keeping_focus(
        &site,
        &mut browser,
        button,
        ("Enter", "Enter", 13),
        (&waiting_on_you(&[waiting("alpha", true)]), 4183),
        "Undo Later",
    );
    assert_eq!(
        focus_on(&mut browser, button),
        json!(true),
        "after Later, the focus is on its Undo"
    );

    // Two more cards come, before the one postponed: its Undo puts it back first.
    site.set_json(
        "/api/queue",
        &waiting_on_you(&[
            waiting("bravo", false),
            waiting("charlie", false),
            waiting("alpha", true),
        ]),
    );
    site.push(
        4184,
        &json!({"version":4184,"changes":[{"region":"queue-item","data":{"key":"fixture","removed":true}}]}).to_string(),
    );
    let order = "[...document.querySelectorAll('#you li[data-card]')].map(row => row.dataset.card.split('#').pop()).join(' ')";
    assert!(
        browser.wait_for(&format!("{order} === 'bravo charlie alpha'"), SHOWN),
        "the queue shows the cards that came"
    );
    press_keeping_focus(
        &site,
        &mut browser,
        button,
        (" ", "Space", 32),
        (
            &waiting_on_you(&[
                waiting("alpha", false),
                waiting("bravo", false),
                waiting("charlie", false),
            ]),
            4185,
        ),
        "Later",
    );
    assert!(
        browser.wait_for(&format!("{order} === 'alpha bravo charlie'"), SHOWN),
        "Undo puts the card back before the others"
    );
    assert_eq!(
        focus_on(&mut browser, button),
        json!(true),
        "after Undo, the focus is on Later, its row moved"
    );
    let actions: Vec<_> = site
        .posts()
        .into_iter()
        .filter(|(path, body)| path == "/api/postpone" && body["slug"] == "alpha")
        .map(|(_, body)| body["action"].clone())
        .collect();
    assert_eq!(actions, [json!("later"), json!("undo")]);
}

#[test]
fn restored_queue_counts_order_scope_anchors_and_refresh_keep_reading_position() {
    let site = Site::start(dist(), &common::fixture("board.json"));
    site.set_json("/api/postpone", &json!({"ok":true,"changed":true}));
    let mut queue = queue_fixture();
    site.set_json("/api/queue", &queue);
    // The fixture reserves enough space to align the last section before any module runs.
    let shell = std::fs::read_to_string(dist().join("index.html")).unwrap();
    site.set_file(
        "/queue",
        shell
            .replace(
                "</head>",
                "<style>.queue-group{min-height:900px}</style></head>",
            )
            .into_bytes(),
        "text/html; charset=utf-8",
    );
    let Some(mut browser) = chrome() else { return };
    browser.call("Page.addScriptToEvaluateOnNewDocument", json!({"source":r#"
        // The shell inserts a page's stylesheet before the project's theme package: held, it
        // goes where it was headed once released.
        window.heldStyles = [];
        const headed = new Map();
        const insertBefore = Node.prototype.insertBefore;
        Node.prototype.insertBefore = function(node, before) {
            if (this === document.head && node?.matches?.('link[rel=stylesheet]')) {
                heldStyles.push(node);
                headed.set(node, before);
                return node;
            }
            return insertBefore.call(this, node, before);
        };
        window.releaseStyles = () => {
            Node.prototype.insertBefore = insertBefore;
            for (const link of heldStyles) insertBefore.call(document.head, link, headed.get(link));
        };
        // Whether the held styles were in when the queue drew its first element in its root: the
        // page's own mount, whichever client's module draws it.
        new MutationObserver((records, observer) => {
            if (!document.getElementById('review-root-queue')?.firstElementChild) return;
            window.queueStartedWithStyles = heldStyles.length > 0 && heldStyles.every(link => link.sheet !== null);
            observer.disconnect();
        }).observe(document, { childList: true, subtree: true });
    "#}));
    browser.open(&site.url("/@topic/queue#agent"));
    assert!(browser.wait_for("heldStyles.length > 0", SHOWN));
    browser.eval("releaseStyles()");
    assert!(browser.wait_for(
        "document.querySelector('#agent .queue-row') !== null",
        SHOWN
    ));
    assert_eq!(
        browser.eval("queueStartedWithStyles"),
        json!(true),
        "the queue must mount only after its delayed styles apply"
    );
    // The shared document inset reserves 76 px once, including on phones where the header
    // scrolls away. Derive only the heading's offset inside the section from its box styles.
    let anchored = r#"(() => {
        const section = document.getElementById('agent'), heading = section.querySelector('h2');
        const box = getComputedStyle(section), text = getComputedStyle(heading);
        const inside = parseFloat(box.borderTopWidth) + parseFloat(box.paddingTop) + parseFloat(text.marginTop);
        return Math.abs(heading.getBoundingClientRect().top - (76 + inside)) <= 1;
    })()"#;
    assert!(
        browser.wait_for(anchored, SHOWN),
        "the addressed heading must match the single anchor inset within one pixel"
    );
    for width in [1280, 390] {
        browser.viewport(width, 900, false);
        browser.eval("document.getElementById('agent').scrollIntoView()");
        assert!(
            browser.wait_for(anchored, SHOWN),
            "at {width} px, the heading must sit one 76 px inset plus its internal offset down, within one pixel"
        );
    }
    browser.call("Emulation.clearDeviceMetricsOverride", json!({}));
    // The restored viewport's resize must reach layout before the section is aligned.
    browser.frames();
    browser.eval("document.getElementById('agent').scrollIntoView()");
    assert!(
        browser.wait_for(anchored, SHOWN),
        "the heading must match the single anchor inset after restoring the viewport: {}",
        browser.eval("(()=>{const s=document.getElementById('agent'),h=s.querySelector('h2');return {viewport:[innerWidth,innerHeight],heading:h.getBoundingClientRect().top,section:s.getBoundingClientRect().top,scroll:scrollY,padding:getComputedStyle(document.documentElement).scrollPaddingTop,margin:getComputedStyle(s).scrollMarginTop}})()")
    );
    assert_eq!(
        texts(&mut browser, ".queue-group h2"),
        [
            "Waiting on you (2)",
            "Auto-approved: spot-check (1)",
            "Waiting on the agent (1)"
        ]
    );
    assert_eq!(
        texts(&mut browser, "#you li[data-card]"),
        vec!["", ""],
        "far rows stay deferred"
    );
    assert_eq!(browser.eval("[...document.querySelectorAll('#you [data-card]')].map(e=>e.dataset.card.split('#').pop())"),json!(["second","first"]));
    assert!(
        site.targets()
            .iter()
            .any(|p| p.starts_with("/@topic/api/queue"))
    );
    assert_eq!(
        browser.eval("document.querySelector('#agent .queue-row a').getAttribute('href')"),
        json!("/@topic/docs/PROBE.md#agent")
    );
    browser.eval("document.getElementById('you').scrollIntoView()");
    assert!(browser.wait_for(
        "document.querySelectorAll('#you .queue-row').length===2",
        SHOWN
    ));
    assert_eq!(
        texts(&mut browser, "#you .queue-row a:not(.chip)"),
        ["Second decision", "First decision"]
    );
    assert_eq!(texts(&mut browser, "#you .queue-rank"), ["1", "2"]);
    assert_eq!(
        texts(&mut browser, "#you .postpone-controls button"),
        ["Later", "Later"]
    );
    assert_eq!(texts(&mut browser, "#you .chip"), ["topic"]);
    assert_eq!(
        browser.eval("document.querySelector('#you .chip').getAttribute('href')"),
        json!("/task/topic")
    );
    assert_eq!(
        browser.eval("document.querySelector('#you .queue-row a').getAttribute('href')"),
        json!("/@topic/docs/PROBE.md#second")
    );
    browser.eval("document.querySelector('#you .postpone-controls button').click()");
    assert!(browser.wait_for(
        "performance.getEntriesByType('resource').some(e=>new URL(e.name).pathname==='/api/postpone')",
        SHOWN
    ));
    assert!(
        site.posts()
            .iter()
            .any(|(path, body)| path == "/api/postpone"
                && body["path"] == "@topic/docs/PROBE.md"
                && body["slug"] == "second"
                && body["action"] == "later")
    );
    browser.eval("document.getElementById('agent').scrollIntoView()");
    assert!(
        browser.wait_for(anchored, SHOWN),
        "returning to the agent section must align its heading within one pixel"
    );
    wait_for_feed(&site).expect("the page never followed the feed");
    // Ends on a value Chrome can send back: an element React drew holds its fiber, a chain too long
    // to serialize.
    browser.eval("window.scrollBy(0,100);window.keptScroll=scrollY;window.keptRow=document.querySelector('#agent li[data-card]');true");
    queue["waiting"] = json!([]);
    queue["spot_check"] = json!([]);
    queue["branches"] = json!([]);
    site.set_json("/api/queue", &queue);
    site.push(
        4183,
        &json!({"version":4183,"changes":[{"region":"queue-item-order","data":{"key":"@topic/docs/PROBE.md#first","after":null}}]}).to_string(),
    );
    assert!(browser.wait_for(
        "document.querySelector('#you h2')?.textContent==='Waiting on you (0)'",
        SHOWN
    ));
    assert_eq!(
        browser.eval(
            "scrollY===keptScroll && document.querySelector('#agent li[data-card]')===keptRow"
        ),
        json!(true)
    );
    assert_eq!(
        texts(&mut browser, "#you li, #spot li"),
        ["nothing to decide", "none"]
    );
    assert_eq!(
        browser.eval("document.getElementById('branches')===null"),
        json!(true)
    );
    for (index, region) in [
        "queue-item",
        "queue-item-order",
        "spot-check",
        "spot-check-order",
        "agent-queue",
        "agent-queue-order",
    ]
    .into_iter()
    .enumerate()
    {
        let title = format!("Updated via {region}");
        queue["agent"][0]["title"] = json!(title);
        site.set_json("/api/queue", &queue);
        let data = if region.ends_with("-order") {
            json!({"key":"fixture","after":null})
        } else {
            json!({"key":"fixture","removed":true})
        };
        let version = 4184 + index as u64;
        site.push(
            version,
            &json!({"version":version,"changes":[{"region":region,"data":data}]}).to_string(),
        );
        assert!(
            browser.wait_for(
                &format!(
                    "document.querySelector('#agent .queue-row a')?.textContent==={}",
                    json!(title)
                ),
                SHOWN
            ),
            "{region} did not refresh the queue"
        );
        assert_eq!(
            browser.eval(
                "scrollY===keptScroll && document.querySelector('#agent li[data-card]')===keptRow"
            ),
            json!(true),
            "{region}"
        );
    }
    queue["agent"] = json!([]);
    site.set_json("/api/queue", &queue);
    browser
        .eval("window.keptAgentTop=document.getElementById('agent').getBoundingClientRect().top");
    // Keep the outage stable until it is observed, rather than racing the reconnect backoff.
    site.fail_feeds(usize::MAX);
    site.drop_feeds();
    assert!(browser.wait_for(
        "document.querySelector('main')?.dataset.feed==='stale'",
        SHOWN
    ));
    assert_eq!(
        browser.eval("scrollY===keptScroll"),
        json!(true),
        "entering the stale state must keep the reading position"
    );
    assert_eq!(
        browser.eval("document.getElementById('agent').getBoundingClientRect().top===keptAgentTop"),
        json!(true),
        "the stale notice must not move the section, even if native scroll anchoring is inactive"
    );
    site.fail_feeds(0);
    assert!(
        browser.wait_for("document.querySelector('#agent h2')?.textContent==='Waiting on the agent (0)' && document.querySelector('main')?.dataset.feed==='live'",SHOWN),
        "after reconnecting, the queue must show no agent work and a live feed"
    );
    assert_eq!(browser.eval("scrollY===keptScroll"), json!(true));
    assert_eq!(
        texts(&mut browser, "#agent li"),
        ["every comment is answered"]
    );
}
