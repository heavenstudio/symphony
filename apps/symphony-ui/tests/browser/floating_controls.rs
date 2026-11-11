//! The page's floating controls never cover the page (app/floating-controls): the pending bar, the
//! chat button and a document's contents pill sit in the dock along the window's foot, which the
//! page scrolls clear of.
use super::components::site;
use super::{SHOWN, chrome, chrome::Chrome, wait_for_board};
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
/// stick out of it, which a click at their middle would miss, and which overlap each other.
const DOCK: &str = r#"(() => {
    const name = control => control.id || control.className;
    const dock = document.querySelector('.frame-dock')?.getBoundingClientRect();
    if (!dock) return 'no dock';
    const width = document.documentElement.clientWidth, height = document.documentElement.clientHeight;
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
})()"#;

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

#[test]
fn the_dock_holds_the_floating_controls_and_a_focused_or_last_control_sits_above_it() {
    let site = site();
    site.set_json("/api/pending", &pending());
    let Some(mut page) = chrome() else { return };
    // A phone, the narrowest phone and a desktop window, each as short as puts the home's primary
    // button just below the window's foot, so focusing it scrolls it into view.
    for width in [390, 320, 1280] {
        viewport(&mut page, width, 900);
        page.open(&site.url("/"));
        wait_for_board(&mut page);
        assert!(page.wait_for(
            "document.querySelector('.frame-pending') !== null && document.querySelector('#quick-open') !== null && document.documentElement.dataset.navigation === 'ready'",
            SHOWN
        ));
        settle(&mut page);
        let top = page.eval(
            "Math.floor(document.querySelector('a.go').getBoundingClientRect().top + scrollY)",
        );
        let height = u32::try_from(top.as_u64().unwrap()).unwrap() - 8;
        viewport(&mut page, width, height);
        page.eval("scrollTo(0,0)");
        settle(&mut page);
        assert_eq!(
            page.eval(DOCK),
            json!({"spans":true,"controls":["pending-nav frame-pending","quick-open"],"outside":[],"missed":[],"overlapping":[]}),
            "at {width}px, the pending bar and the chat button sit in the dock, apart"
        );
        assert_eq!(
            page.eval("document.querySelector('a.go').getBoundingClientRect().top > document.documentElement.clientHeight"),
            json!(true),
            "at {width}px x {height}px the setup starts with the primary button below the window"
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
        page.eval(DOCK),
        json!({"spans":true,"controls":["doc-toc","pending-nav frame-pending","quick-open"],"outside":[],"missed":[],"overlapping":[]}),
        "a folded contents pill sits in the dock, apart from the pending bar and the chat button"
    );
    // A phone starts with the contents folded: a pill in the page, under the document's title,
    // which opens where it is.
    viewport(&mut page, 390, 844);
    page.open(&document);
    assert!(page.wait_for(ready, SHOWN));
    settle(&mut page);
    let place = "(() => {const toc=document.querySelector('.doc-toc'), box=toc.getBoundingClientRect();\
        return {position:getComputedStyle(toc).position, open:toc.querySelector('details').open,\
        top:Math.round(box.top + scrollY), belowHead:box.top >= document.querySelector('.doc-head').getBoundingClientRect().bottom,\
        aboveBody:box.bottom <= document.querySelector('.doc-body').getBoundingClientRect().top};})()";
    let folded = page.eval(place);
    assert_eq!(
        json!({"position":folded["position"],"open":folded["open"],"belowHead":folded["belowHead"],"aboveBody":folded["aboveBody"]}),
        json!({"position":"relative","open":false,"belowHead":true,"aboveBody":true}),
        "on a phone the folded contents are in the page, between its title and its body"
    );
    assert_eq!(
        page.eval(DOCK)["controls"],
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
        const dock = document.querySelector('.frame-dock').getBoundingClientRect();
        return {{
            fullHeight: panel.top === 0 && Math.abs(panel.bottom - document.documentElement.clientHeight) < 1,
            header: document.querySelector('.app-header').getBoundingClientRect().right <= left,
            page: document.querySelector('.frame-page').getBoundingClientRect().right <= left,
            notice: !notice || notice.right <= left,
            dock: dock.right <= left,
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
    assert!(page.wait_for(
        "document.getElementById('activity-panel') !== null && document.getElementById('bell').getAttribute('aria-expanded') === 'true'",
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
    assert_eq!(
        page.eval("Math.round(document.querySelector('.frame-page').getBoundingClientRect().right) === document.documentElement.clientWidth"),
        json!(true),
        "closing it gives the page its width back"
    );
}
