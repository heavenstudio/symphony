//! The restored queue page: a stylesheet that fails leaves the page before it and is tried again,
//! and the queue's counts, order, scope, anchors and refresh keep the owner's reading position.
use crate::{SHOWN, chrome, common, dist, site::Site, texts, wait_for_board, wait_for_feed};
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
        Object.defineProperty(window, 'reviewModule_queue', { configurable: true, set(module) {
            const run = module.run;
            module.run = function() {
                window.queueStartedWithStyles = heldStyles.length > 0 && heldStyles.every(link => link.sheet !== null);
                return run.apply(this, arguments);
            };
            Object.defineProperty(window, 'reviewModule_queue', { value: module });
        }});
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
        browser.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":width,"height":900,"deviceScaleFactor":1,"mobile":false}),
        );
        browser.eval("document.getElementById('agent').scrollIntoView()");
        assert!(
            browser.wait_for(anchored, SHOWN),
            "at {width} px, the heading must sit one 76 px inset plus its internal offset down, within one pixel"
        );
    }
    browser.call("Emulation.clearDeviceMetricsOverride", json!({}));
    // The restored viewport's resize must reach layout before the section is aligned.
    browser.eval("new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
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
    assert!(wait_for_feed(&site));
    browser.eval("window.scrollBy(0,100);window.keptScroll=scrollY;window.keptRow=document.querySelector('#agent li[data-card]')");
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
