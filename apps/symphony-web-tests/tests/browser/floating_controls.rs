//! The page's floating controls and side panels never cover the page (app/floating-controls): the
//! pending bar, the chat button and a document's contents pill sit in the dock along the window's
//! foot, which the page scrolls clear of and the task board's scroller ends above; an open side
//! panel pushes the header and the page aside;
//! the chat sheet is one surface from the top of the window, beside the header's controls.
use super::components::{open_panel, site};
use super::{SHOWN, at_page_width, chrome, chrome::Chrome, wait_for_board};
use serde_json::{Value, json};

/// Three cards waiting on the owner, so the pending bar shows on every page.
fn pending() -> Value {
    json!({"docs":["docs/look-parity.md"],"items":[
        {"path":"docs/look-parity.md","slug":"one","href":"/docs/look-parity.md#one","branch":null},
        {"path":"docs/look-parity.md","slug":"two","href":"/docs/look-parity.md#two","branch":null},
        {"path":"docs/look-parity.md","slug":"three","href":"/docs/look-parity.md#three","branch":null}]})
}

fn viewport(page: &mut Chrome, width: u32, height: u32) {
    page.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":1,"mobile":width < 600}),
    );
}

/// Two frames, so what a viewport or a click changed is laid out and painted before it is read.
fn settle(page: &mut Chrome) {
    page.eval("new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
}

/// The dock and the controls it holds: whether it spans the window's foot, which fixed controls
/// stick out of it, which a click at their middle would miss, and which overlap each other. A
/// page function of the window's `width`, the width it leaves the page beside its scrollbar
/// ([`at_page_width`]).
const DOCK: &str = r#"width => {
    const name = control => control.id || control.className;
    const dock = document.querySelector('.frame-dock')?.getBoundingClientRect();
    if (!dock) return 'no dock';
    const height = document.documentElement.clientHeight;
    const controls = [...document.querySelectorAll('.frame-pending, #quick-open, .doc-toc')]
        .filter(control => getComputedStyle(control).position === 'fixed' && control.getBoundingClientRect().width > 0);
    const boxes = controls.map(control => [control, control.getBoundingClientRect()]);
    return {
        spans: dock.left === 0 && Math.abs(dock.right - width) < 1 && Math.abs(dock.bottom - height) < 1 && dock.height >= 40,
        controls: controls.map(name),
        outside: boxes.filter(([, b]) => b.top < dock.top || b.bottom > dock.bottom || b.left < dock.left || b.right > dock.right).map(([c]) => name(c)),
        missed: boxes.filter(([c, b]) => !c.contains(document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2))).map(([c]) => name(c)),
        overlapping: boxes.flatMap(([c, b], i) => boxes.slice(i + 1).filter(([, o]) => b.left < o.right && o.left < b.right && b.top < o.bottom && o.top < b.bottom).map(([o]) => name(c) + ' / ' + name(o))),
    };
}"#;

/// Whether `selector`'s element is clear of every fixed control and of the dock, and a click at
/// its middle reaches it.
fn clear(selector: &str) -> String {
    format!(
        r#"(() => {{
        const element = document.querySelector({selector}), box = element.getBoundingClientRect();
        const dock = document.querySelector('.frame-dock')?.getBoundingClientRect();
        const fixed = [...document.querySelectorAll('.frame-pending, #quick-open, .doc-toc')]
            .filter(control => getComputedStyle(control).position === 'fixed')
            .map(control => control.getBoundingClientRect());
        const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
        return {{
            inView: box.top >= 0 && box.bottom <= document.documentElement.clientHeight,
            aboveDock: !!dock && box.bottom <= dock.top + 0.5,
            underControl: fixed.some(b => box.left < b.right && b.left < box.right && box.top < b.bottom && b.top < box.bottom),
            reached: element.contains(hit),
        }};
    }})()"#,
        selector = json!(selector)
    )
}

const CLEAR: &str = r#"{"inView":true,"aboveDock":true,"underControl":false,"reached":true}"#;

/// Makes the window as short as puts the home's primary button's foot 2px above the window's,
/// scrolled to the top, and returns that height.
fn foot_at_window_foot(page: &mut Chrome, width: u32) -> u32 {
    let bottom = page
        .eval("Math.ceil(document.querySelector('a.go').getBoundingClientRect().bottom + scrollY)");
    let height = u32::try_from(bottom.as_u64().unwrap()).unwrap() + 2;
    viewport(page, width, height);
    page.eval("scrollTo(0,0)");
    settle(page);
    height
}

/// Whether the home's primary button is wholly in the window and its foot under the dock's top.
const UNDER_DOCK: &str = "(() => {const box=document.querySelector('a.go').getBoundingClientRect(), dock=document.querySelector('.frame-dock').getBoundingClientRect();\
    return {inView: box.top >= 0 && box.bottom <= document.documentElement.clientHeight, underDock: box.bottom > dock.top};})()";

/// What the window's foot draws with nothing but the chat button to hold: whether the dock's bar
/// draws, whether a pending bar is there, and whether the "+" sits alone in the window's corner,
/// at the dock's lift. A page function of the window's `width`, the width it leaves the page
/// beside its scrollbar ([`at_page_width`]).
const BARE: &str = r#"width => {
    const dock = document.querySelector('.frame-dock')?.getBoundingClientRect();
    const open = document.querySelector('#quick-open').getBoundingClientRect();
    const height = document.documentElement.clientHeight;
    return {
        bar: !!dock && dock.width > 0 && dock.height > 0,
        pending: document.querySelector('.frame-pending') !== null,
        corner: Math.abs(height - 8 - open.bottom) < 1 && Math.abs(width - 16 - open.right) < 1,
    };
}"#;

/// Whether the home's primary button is wholly in the window, and whether its foot is above the
/// chat button's top.
const BESIDE_CHAT: &str = "(() => {const box=document.querySelector('a.go').getBoundingClientRect(), open=document.querySelector('#quick-open').getBoundingClientRect();\
    return {inView: box.top >= 0 && box.bottom <= document.documentElement.clientHeight, aboveChat: box.bottom <= open.top + 0.5};})()";

#[test]
fn with_nothing_pending_no_dock_draws_and_the_chat_button_sits_alone_in_the_corner() {
    // The stand-in answers /api/pending with no items.
    let site = site();
    let Some(mut page) = chrome() else { return };
    let ready = "document.querySelector('#quick-open') !== null && document.documentElement.dataset.navigation === 'ready'";
    for width in [390, 1280] {
        viewport(&mut page, width, 900);
        page.open(&site.url("/tasks"));
        assert!(page.wait_for(
            &format!(
                "{ready} && document.querySelector('.waiting-board:not(.waiting-ghost)') !== null"
            ),
            SHOWN
        ));
        settle(&mut page);
        assert_eq!(
            page.eval(&at_page_width(BARE)),
            json!({"bar":false,"pending":false,"corner":true}),
            "at {width}px the tasks page draws no dock, and the \"+\" sits alone in the corner"
        );
        page.open(&site.url("/"));
        wait_for_board(&mut page);
        assert!(page.wait_for(ready, SHOWN));
        settle(&mut page);
        assert_eq!(
            page.eval(&at_page_width(BARE)),
            json!({"bar":false,"pending":false,"corner":true}),
            "at {width}px the board draws no dock, and the \"+\" sits alone in the corner"
        );
        // A control in the window under the "+": focus lifts it clear, as it would above a dock.
        let height = foot_at_window_foot(&mut page, width);
        assert_eq!(
            page.eval(BESIDE_CHAT),
            json!({"inView":true,"aboveChat":false}),
            "at {width}px x {height}px the setup starts with the primary button in the window, its foot below the \"+\"'s top"
        );
        page.eval("document.querySelector('a.go').focus()");
        settle(&mut page);
        assert_eq!(
            page.eval(BESIDE_CHAT),
            json!({"inView":true,"aboveChat":true}),
            "at {width}px, focusing the primary button lifts it clear of the \"+\""
        );
    }
    // A document draws the dock only while it holds the folded contents pill, on a desktop.
    let document = site.url("/docs/look-parity.md");
    let toc = "document.querySelector('.doc-toc details') !== null && document.documentElement.dataset.navigation === 'ready'";
    viewport(&mut page, 1280, 800);
    page.open(&document);
    assert!(page.wait_for(toc, SHOWN));
    assert!(page.wait_for("document.querySelector('.doc-toc details').open", SHOWN));
    settle(&mut page);
    assert_eq!(
        page.eval(&at_page_width(BARE)),
        json!({"bar":false,"pending":false,"corner":true}),
        "a wide document with its contents open draws no dock"
    );
    page.eval("document.querySelector('.doc-toc summary').click()");
    assert!(page.wait_for("!document.querySelector('.doc-toc details').open", SHOWN));
    settle(&mut page);
    assert_eq!(
        page.eval(&at_page_width(DOCK)),
        json!({"spans":true,"controls":["doc-toc","quick-open"],"outside":[],"missed":[],"overlapping":[]}),
        "folded, the contents pill brings the dock back, beside the \"+\""
    );
    viewport(&mut page, 390, 844);
    page.open(&document);
    assert!(page.wait_for(toc, SHOWN));
    settle(&mut page);
    assert_eq!(
        page.eval(&at_page_width(BARE)),
        json!({"bar":false,"pending":false,"corner":true}),
        "on a phone the folded contents sit in the page, and no dock draws"
    );
}

#[test]
fn the_dock_holds_the_floating_controls_and_a_focused_or_last_control_sits_above_it() {
    let site = site();
    site.set_json("/api/pending", &pending());
    let Some(mut page) = chrome() else { return };
    // A phone, the narrowest phone and a desktop window, each as short as puts the home's primary
    // button wholly inside the window but under the dock, its foot 2px above the window's: a
    // control the browser counts as in view, which focus scrolls only for the scroll padding.
    for width in [390, 320, 1280] {
        viewport(&mut page, width, 900);
        page.open(&site.url("/"));
        wait_for_board(&mut page);
        assert!(page.wait_for(
            "document.querySelector('.frame-pending') !== null && document.querySelector('#quick-open') !== null && document.documentElement.dataset.navigation === 'ready'",
            SHOWN
        ));
        settle(&mut page);
        let height = foot_at_window_foot(&mut page, width);
        assert_eq!(
            page.eval(&at_page_width(DOCK)),
            json!({"spans":true,"controls":["pending-nav frame-pending","quick-open"],"outside":[],"missed":[],"overlapping":[]}),
            "at {width}px, the pending bar and the chat button sit in the dock, apart"
        );
        assert_eq!(
            page.eval(UNDER_DOCK),
            json!({"inView":true,"underDock":true}),
            "at {width}px x {height}px the setup starts with the primary button in the window, under the dock"
        );
        page.eval("document.querySelector('a.go').focus()");
        settle(&mut page);
        assert_eq!(
            page.eval(&clear("a.go")),
            serde_json::from_str::<Value>(CLEAR).unwrap(),
            "at {width}px, focusing the home's primary button scrolls it above the dock"
        );
        // The page's last control, at the end of the page, clears the dock too.
        page.eval("scrollTo(0, document.documentElement.scrollHeight)");
        settle(&mut page);
        page.eval("[...document.querySelectorAll('main a[href], main button')].filter(e => e.getBoundingClientRect().height > 0).pop().setAttribute('data-last', '')");
        assert_eq!(
            page.eval(&clear("[data-last]")),
            serde_json::from_str::<Value>(CLEAR).unwrap(),
            "at {width}px, the page ends above the dock"
        );
    }
}

/// The window-scrolled board lets its last card clear the floating dock or chat button. There
/// is no private horizontal scrollbar to hide beneath either control in the waiting layout.
#[test]
fn the_task_boards_last_card_scrolls_clear_of_the_dock_and_chat_button() {
    for pending_items in [false, true] {
        let site = site();
        if pending_items {
            site.set_json("/api/pending", &pending());
        }
        site.set_json("/api/task-board", &super::polish::thirty_ready());
        let Some(mut page) = chrome() else { return };
        for (width, height) in [(390, 844), (1280, 900), (1500, 1700)] {
            viewport(&mut page, width, height);
            page.open(&site.url("/tasks"));
            assert!(page.wait_for("document.querySelector('.waiting-columns') !== null && document.documentElement.dataset.navigation === 'ready'",SHOWN));
            if width <= 760 {
                assert!(page.wait_for(
                    "document.querySelector('.stage-tabs button') !== null",
                    SHOWN
                ));
                page.eval("[...document.querySelectorAll('.stage-tabs button')].find(button=>button.textContent.startsWith('Ready')).click()");
            }
            assert!(page.wait_for(
                "document.querySelector('[id=\"row-load/ready-30\"]') !== null",
                SHOWN
            ));
            page.eval("scrollTo(0,document.documentElement.scrollHeight)");
            assert!(page.wait_for(
                "document.querySelector('[data-slug=\"load/ready-30\"]') !== null",
                SHOWN
            ));
            settle(&mut page);
            let shown = page.eval(&clear("[data-slug=\"load/ready-30\"]"));
            assert_eq!(shown["inView"], json!(true), "{width}px: {shown}");
            assert_eq!(shown["underControl"], json!(false), "{width}px: {shown}");
            assert_eq!(shown["reached"], json!(true), "{width}px: {shown}");
            if pending_items {
                assert_eq!(shown["aboveDock"], json!(true), "{width}px: {shown}");
            }
            assert_eq!(
                page.eval("document.documentElement.scrollWidth <= innerWidth"),
                json!(true)
            );
        }
    }
}

#[test]
fn a_documents_contents_pill_sits_in_the_dock_and_in_the_page_on_a_phone() {
    let site = site();
    site.set_json("/api/pending", &pending());
    let Some(mut page) = chrome() else { return };
    let document = site.url("/docs/look-parity.md");
    let ready = "document.querySelector('.doc-toc details') !== null && document.querySelector('.frame-pending') !== null && document.documentElement.dataset.navigation === 'ready'";
    // A wide window opens the contents beside the page; folded, the pill sits in the dock.
    viewport(&mut page, 1280, 800);
    page.open(&document);
    assert!(page.wait_for(ready, SHOWN));
    page.eval("document.querySelector('.doc-toc summary').click()");
    assert!(page.wait_for("!document.querySelector('.doc-toc details').open", SHOWN));
    settle(&mut page);
    assert_eq!(
        page.eval(&at_page_width(DOCK)),
        json!({"spans":true,"controls":["doc-toc","pending-nav frame-pending","quick-open"],"outside":[],"missed":[],"overlapping":[]}),
        "a folded contents pill sits in the dock, apart from the pending bar and the chat button"
    );
    assert_eq!(
        page.eval("getComputedStyle(document.querySelector('.doc-toc')).overflow"),
        json!("visible"),
        "the folded contents clip nothing, so the pill's shadow is drawn whole"
    );
    // A phone starts with the contents folded: a pill in the page, under the document's title,
    // which opens where it is.
    viewport(&mut page, 390, 844);
    page.open(&document);
    assert!(page.wait_for(ready, SHOWN));
    settle(&mut page);
    let place = "(() => {const toc=document.querySelector('.doc-toc'), box=toc.getBoundingClientRect();\
        return {position:getComputedStyle(toc).position, overflow:getComputedStyle(toc).overflow, open:toc.querySelector('details').open,\
        top:Math.round(box.top + scrollY), belowHead:box.top >= document.querySelector('.doc-head').getBoundingClientRect().bottom,\
        aboveBody:box.bottom <= document.querySelector('.doc-body').getBoundingClientRect().top};})()";
    let folded = page.eval(place);
    assert_eq!(
        json!({"position":folded["position"],"overflow":folded["overflow"],"open":folded["open"],"belowHead":folded["belowHead"],"aboveBody":folded["aboveBody"]}),
        json!({"position":"relative","overflow":"visible","open":false,"belowHead":true,"aboveBody":true}),
        "on a phone the folded contents are in the page, between its title and its body"
    );
    assert_eq!(
        page.eval(&at_page_width(DOCK))["controls"],
        json!(["pending-nav frame-pending", "quick-open"]),
        "and not in the dock"
    );
    page.eval("document.querySelector('.doc-toc summary').click()");
    assert!(page.wait_for("document.querySelector('.doc-toc details').open", SHOWN));
    settle(&mut page);
    let opened = page.eval(place);
    assert_eq!(
        (&opened["position"], &opened["top"]),
        (&folded["position"], &folded["top"]),
        "the contents open where the pill was"
    );
    assert_eq!(
        opened["overflow"],
        json!("auto"),
        "open, a long list scrolls within its box"
    );
}

/// Whether the header, the page and any page notice end where the side surface `panel` starts,
/// every control of theirs left of it, and a click at the activity button's middle reaches it.
fn beside(panel: &str) -> String {
    format!(
        r#"(() => {{
        const panel = document.querySelector({panel}).getBoundingClientRect();
        const left = panel.left + 0.5;
        const controls = [...document.querySelectorAll('.app-header a, .app-header button, .frame-page a[href], .frame-page button, #shell-notices button')]
            .filter(control => {{ const box = control.getBoundingClientRect(); return box.width > 0 && box.height > 0 && box.top < innerHeight && box.bottom > 0; }});
        const bell = document.getElementById('bell').getBoundingClientRect();
        const notice = document.querySelector('#shell-notices .shell-notice')?.getBoundingClientRect();
        const dock = document.querySelector('.frame-dock')?.getBoundingClientRect();
        return {{
            fullHeight: panel.top === 0 && Math.abs(panel.bottom - document.documentElement.clientHeight) < 1,
            header: document.querySelector('.app-header').getBoundingClientRect().right <= left,
            page: document.querySelector('.frame-page').getBoundingClientRect().right <= left,
            notice: !notice || notice.right <= left,
            dock: !!dock && dock.right <= left,
            covered: controls.filter(control => control.getBoundingClientRect().right > left).map(control => control.id || control.textContent.trim()),
            bell: document.getElementById('bell').contains(document.elementFromPoint(bell.left + bell.width / 2, bell.top + bell.height / 2)),
        }};
    }})()"#,
        panel = json!(panel)
    )
}

const BESIDE: &str = r#"{"fullHeight":true,"header":true,"page":true,"notice":true,"dock":true,"covered":[],"bell":true}"#;

#[test]
fn the_open_activity_panel_pushes_the_header_and_the_page_aside() {
    let site = site();
    site.set_json("/api/pending", &pending());
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1280, 800);
    page.open(&site.url("/"));
    wait_for_board(&mut page);
    page.eval("document.getElementById('bell').click()");
    // The shell draws a loading sheet of the same id at once; the panel's own, with its close
    // button, replaces it.
    assert!(page.wait_for(
        "document.querySelector('#activity-panel [aria-label=\"Close activity\"]') !== null && document.getElementById('bell').getAttribute('aria-expanded') === 'true'",
        SHOWN
    ));
    settle(&mut page);
    assert_eq!(
        page.eval(&beside("#activity-panel")),
        serde_json::from_str::<Value>(BESIDE).unwrap(),
        "the home's tiles and pills stay in view beside the activity panel"
    );
    page.eval("document.querySelector('#activity-panel [aria-label=\"Close activity\"]').click()");
    assert!(page.wait_for("document.getElementById('activity-panel') === null", SHOWN));
    settle(&mut page);
    // The page's right edge, and the width the window leaves the page beside its scrollbar.
    let [right, window]: [u64; 2] = serde_json::from_value(page.eval(&at_page_width(
        "width => [Math.round(document.querySelector('.frame-page').getBoundingClientRect().right), width]",
    )))
    .unwrap();
    assert_eq!(right, window, "closing it gives the page its width back");
}

#[test]
fn the_chat_sheet_runs_from_the_top_of_the_window_beside_the_header_under_a_notice() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1280, 800);
    page.open(&site.url("/"));
    wait_for_board(&mut page);
    // A server notice above the header, as the server's own warnings draw one.
    page.eval("document.getElementById('shell-notices').innerHTML = '<div class=\"shell-notice\" role=\"status\"><p><strong>1 Symphony process from test runs is running with no parent</strong>: nothing ends it on its own.</p><code>ps -axo pid,ppid,command</code><p>lists it; stop it by pid.</p></div>'");
    open_panel(&mut page);
    settle(&mut page);
    assert_eq!(
        page.eval("document.querySelector('.app-header').getBoundingClientRect().top > 62"),
        json!(true),
        "the notice puts the header lower than the sheet's old top"
    );
    assert_eq!(
        page.eval(&beside("#quick-sheet")),
        serde_json::from_str::<Value>(BESIDE).unwrap(),
        "the sheet is one surface from the window's top, and no header control is under it"
    );
    // A real click on the activity button reaches it, and opens the activity panel.
    let bell = page.eval("(() => {const box=document.getElementById('bell').getBoundingClientRect();return {x:box.left+box.width/2,y:box.top+box.height/2};})()");
    for kind in ["mousePressed", "mouseReleased"] {
        page.call(
            "Input.dispatchMouseEvent",
            json!({"type":kind,"x":bell["x"],"y":bell["y"],"button":"left","clickCount":1}),
        );
    }
    assert!(
        page.wait_for(
            "document.getElementById('activity-panel') !== null && document.getElementById('bell').getAttribute('aria-expanded') === 'true'",
            SHOWN
        ),
        "the click on the activity button opens the activity panel"
    );
}
