//! The owner's polish findings (app/ui-polish-1), each read in the release client: popups draw on
//! an opaque surface of bounded height, the chat panel is one surface whose Send stays in view,
//! missing pages say so, panels answer the keyboard, badges stay readable and native controls
//! follow the scheme, in both schemes.
use super::components::{appearance, key, open_panel, site};
use super::{SHOWN, chrome, chrome::Chrome, dist, site::Site};
use serde_json::{Value, json};

/// The board and nothing else: every other read answers 404, as a server without the task or
/// document a page names does.
fn bare_site() -> Site {
    Site::start(dist(), &super::common::fixture("board.json"))
}

/// The page's breadcrumbs, as `[text, href]`, `href` null for the page itself.
fn crumbs(page: &mut Chrome) -> Value {
    page.eval("[...document.querySelectorAll('nav.crumbs > a, nav.crumbs > span')].map(e => [e.textContent, e.getAttribute('href')])")
}

/// The page's colour scheme, as the operating system would set it.
fn scheme(page: &mut Chrome, mode: &str) {
    page.call(
        "Emulation.setEmulatedMedia",
        json!({"features":[{"name":"prefers-color-scheme","value":mode}]}),
    );
}

fn viewport(page: &mut Chrome, width: u32, height: u32) {
    page.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":1,"mobile":width < 600}),
    );
}

/// The card colour of each scheme's default theme: the surface every popup draws on.
const SURFACES: [(&str, &str); 2] = [("light", "rgb(255, 255, 255)"), ("dark", "rgb(29, 28, 25)")];

/// The open select popup, positioned and done animating in.
fn popup_ready(page: &mut Chrome) {
    assert!(page.wait_for("(() => {const list=document.querySelector('[role=listbox][data-state=open][data-positioned=true]');return list && list.getAnimations().every(animation=>animation.playState==='finished');})()", SHOWN));
}

#[test]
fn an_open_select_and_the_appearance_menu_draw_on_an_opaque_bounded_surface_in_both_schemes() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1280, 900);
    for (mode, surface) in SURFACES {
        scheme(&mut page, mode);
        page.open(&site.url("/tasks"));
        assert!(page.wait_for(
            "document.querySelector('button[aria-label=\"Status\"]') !== null",
            SHOWN
        ));
        page.eval("document.querySelector('button[aria-label=\"Status\"]').focus()");
        key(&mut page, " ", "Space", 32);
        popup_ready(&mut page);
        let list = page.eval(
            "(() => {const list=document.querySelector('[role=listbox]'),style=getComputedStyle(list),box=list.getBoundingClientRect();\
             return {focused:document.activeElement===list,outline:style.outlineStyle,background:style.backgroundColor,\
             height:box.height,scrolls:list.scrollHeight>list.clientHeight,options:list.querySelectorAll('[role=option]').length};})()",
        );
        assert_eq!(
            list["options"],
            json!(12),
            "{mode}: all statuses and the board's eleven, far taller than the cap"
        );
        assert_eq!(
            list["focused"],
            json!(true),
            "{mode}: the list takes focus: {list}"
        );
        assert_eq!(
            list["outline"],
            json!("none"),
            "{mode}: no ring around the whole list"
        );
        assert_eq!(
            list["background"],
            json!(surface),
            "{mode}: the list is opaque"
        );
        let height = list["height"].as_f64().unwrap();
        assert!(
            height <= 320.0,
            "{mode}: the list stops at 20rem with 900px of window: {list}"
        );
        assert_eq!(
            list["scrolls"],
            json!(true),
            "{mode}: the options past the cap scroll"
        );
        key(&mut page, "ArrowDown", "ArrowDown", 40);
        assert!(page.wait_for(
            "document.activeElement.getAttribute('role')==='option'",
            SHOWN
        ));
        let option = page.eval("(() => {const style=getComputedStyle(document.activeElement);return {outline:style.outlineStyle,background:style.backgroundColor};})()");
        assert_eq!(
            option["outline"],
            json!("none"),
            "{mode}: the focused option is the indicator"
        );
        assert_ne!(
            option["background"],
            json!(surface),
            "{mode}: the focused option is highlighted"
        );
        key(&mut page, "Escape", "Escape", 27);
        assert!(page.wait_for("document.querySelector('[role=listbox]') === null", SHOWN));
        // The sheet stays open across loads: open it only when it is not, or Enter would reach
        // the Status field, which Escape returned focus to.
        if page.eval("document.querySelector('#quick-sheet:not([hidden])') === null") == json!(true)
        {
            open_panel(&mut page);
        }
        appearance(&mut page);
        assert_eq!(
            page.eval("getComputedStyle(document.querySelector('.theme-menu [role=menu]')).backgroundColor"),
            json!(surface),
            "{mode}: the Appearance menu is opaque"
        );
        key(&mut page, "Escape", "Escape", 27);
    }
}

fn paragraph(text: &str) -> Value {
    json!({"type":"paragraph","spans":[{"type":"text","text":text}]})
}

/// A conversation of forty messages, far taller than the panel.
fn long_conversation() -> Value {
    let messages: Vec<_> = (1..=40)
        .map(|seq| {
            json!({"seq":seq,"kind":if seq % 2 == 1 { "question" } else { "answer" },
                "by":if seq % 2 == 1 { "owner" } else { "example" },"text":"","at":1.0,
                "blocks":[paragraph(&format!("Message {seq}: a reply long enough to wrap over two lines of the panel."))]})
        })
        .collect();
    json!({"ask":{"id":7,"path":"docs/README.md","anchor":"","quote":"","question":"Message 1",
        "asked":1.0,"updated":1.0,"answers":[],"messages":messages,"status":"answered"},"recipient":"example"})
}

/// One attached file whose name has no break opportunity, as the draft saves it.
const LONG_FILE: &str = "/artifacts/uploads/0123456789abcdef.pdf\tquarterly-review-of-every-lamp-and-chandelier-picture-with-notes-final-v3.pdf\tfalse\t1024\t[quarterly-review-of-every-lamp-and-chandelier-picture-with-notes-final-v3.pdf](/artifacts/uploads/0123456789abcdef.pdf)";

/// Where the sheet's controls are: whether Send can be seen and clicked, whether anything in the
/// sheet scrolls sideways or sticks out of it, and the open tab panel's own surface.
fn sheet_layout(page: &mut Chrome) -> Value {
    page.eval(r#"(() => {
        const sheet = document.querySelector('#quick-sheet'), edge = sheet.getBoundingClientRect();
        const send = sheet.querySelector('button[type=submit]'), box = send.getBoundingClientRect();
        const hit = document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2);
        const panel = sheet.querySelector('[role=tabpanel]:not([hidden])'), look = getComputedStyle(panel);
        const thread = sheet.querySelector('.quick-thread');
        const sideways = [sheet, ...sheet.querySelectorAll('*')]
            .filter(e => ['auto', 'scroll'].includes(getComputedStyle(e).overflowX) && e.scrollWidth > e.clientWidth + 1)
            .map(e => e.tagName + '.' + e.className);
        const outside = [...sheet.querySelectorAll('form *')]
            .filter(e => { const r = e.getBoundingClientRect(); return r.width > 0 && (r.left < edge.left - 1 || r.right > edge.right + 1); })
            .map(e => e.tagName + '.' + e.className);
        return {
            send: box.top >= edge.top && box.bottom <= Math.min(edge.bottom, innerHeight) && box.left >= edge.left && box.right <= edge.right,
            clickable: hit === send || send.contains(hit),
            sideways, outside,
            panel: {padding: look.paddingTop, border: look.borderTopStyle, background: look.backgroundColor, scrolls: panel.scrollHeight > panel.clientHeight + 1},
            threadScrolls: thread.scrollHeight > thread.clientHeight,
        };
    })()"#)
}

#[test]
fn the_chat_panel_is_one_surface_whose_send_stays_in_view_with_a_long_thread_or_file_name() {
    let site = site();
    site.set_json("/api/ask", &long_conversation());
    let Some(mut page) = chrome() else { return };
    scheme(&mut page, "dark");
    for (width, height) in [(1280, 800), (390, 844)] {
        viewport(&mut page, width, height);
        for (case, draft, files) in [
            // A reply in the composer, so Send is enabled and takes clicks.
            (
                "thread",
                json!({"ask":7,"priority":"2","text":"A reply"}),
                "",
            ),
            (
                "file",
                json!({"task":true,"title":"Long file","priority":"2"}),
                LONG_FILE,
            ),
        ] {
            page.open(&site.url("/"));
            page.eval(&format!(
                "localStorage.setItem('review-quick-draft', {}); localStorage.setItem('review-attach:quick', {})",
                json!(draft.to_string()),
                json!(files)
            ));
            page.open(&site.url("/"));
            open_panel(&mut page);
            let ready = if case == "thread" {
                "document.querySelectorAll('#quick-sheet .quick-message').length === 40"
            } else {
                "document.querySelector('#quick-title') !== null && document.querySelector('#quick-sheet .attach-file .attach-name') !== null"
            };
            assert!(
                page.wait_for(ready, SHOWN),
                "{case} at {width}: {}",
                page.eval("document.querySelector('#quick-sheet').innerText")
            );
            let layout = sheet_layout(&mut page);
            let at = format!("{case} at {width}x{height}: {layout}");
            assert_eq!(
                layout["send"],
                json!(true),
                "Send is inside the sheet and the window, {at}"
            );
            assert_eq!(
                layout["clickable"],
                json!(true),
                "nothing covers Send, {at}"
            );
            assert_eq!(
                layout["sideways"],
                json!([]),
                "nothing scrolls sideways, {at}"
            );
            assert_eq!(
                layout["outside"],
                json!([]),
                "the composer stays inside the sheet, {at}"
            );
            assert_eq!(
                layout["panel"],
                json!({"padding":"0px","border":"none","background":"rgba(0, 0, 0, 0)","scrolls":false}),
                "the tab panel is part of the sheet's one surface and never scrolls itself, {at}"
            );
            if case == "thread" {
                assert_eq!(
                    layout["threadScrolls"],
                    json!(true),
                    "the thread scrolls, {at}"
                );
            }
        }
    }
}

#[test]
fn the_count_of_todays_replies_opens_the_activity_panel_where_they_are_listed() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    page.open(&site.url("/"));
    assert!(page.wait_for(
        "document.querySelector('.tile.tone-answered') !== null",
        SHOWN
    ));
    let tile = page.eval("(() => {const tile=document.querySelector('.tile.tone-answered'),link=document.querySelector('.tile.tone-you');\
        return {tag:tile.tagName,href:tile.getAttribute('href'),height:tile.offsetHeight===link.offsetHeight,cursor:getComputedStyle(tile).cursor};})()");
    assert_eq!(
        tile,
        json!({"tag":"BUTTON","href":null,"height":true,"cursor":"pointer"}),
        "a button in the row of links, of their size"
    );
    page.eval("document.querySelector('.tile.tone-answered').click()");
    assert!(
        page.wait_for(
            "document.getElementById('activity-panel') !== null && document.getElementById('bell').getAttribute('aria-expanded') === 'true'",
            SHOWN
        ),
        "the tile opens the activity panel"
    );
    assert_eq!(
        page.eval("location.pathname + location.hash"),
        json!("/"),
        "and leads nowhere else"
    );
}

#[test]
fn a_task_page_crumbs_only_the_tasks_page_and_an_unknown_slug_says_there_is_no_such_task() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    page.open(&site.url("/task/visual/in-progress"));
    assert!(page.wait_for("document.querySelector('article.task') !== null", SHOWN));
    assert_eq!(
        crumbs(&mut page),
        json!([["tasks", "/tasks"], ["visual/in-progress", null]]),
        "no crumb makes a task of a part of the slug"
    );
    let bare = bare_site();
    page.open(&bare.url("/task/product/nope"));
    assert!(
        page.wait_for("document.querySelector('.trouble h1') !== null", SHOWN),
        "{}",
        page.eval("document.querySelector('main').innerText")
    );
    let shown = page.eval("(() => {const block=document.querySelector('.trouble');return {title:block.querySelector('h1').textContent,\
        back:block.querySelector('a').getAttribute('href'),retry:[...block.querySelectorAll('button')].map(b=>b.textContent),\
        loading:document.querySelector('main').innerText.includes('Loading task')};})()");
    assert_eq!(
        shown,
        json!({"title":"No task product/nope","back":"/tasks","retry":["Retry"],"loading":false}),
        "the body says the task is not there, instead of loading forever"
    );
    assert_eq!(
        crumbs(&mut page),
        json!([["tasks", "/tasks"], ["product/nope", null]])
    );
}

#[test]
fn native_controls_follow_the_scheme_and_the_theme_file_is_picked_from_a_drawn_button() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    for (mode, surface) in SURFACES {
        scheme(&mut page, mode);
        for route in ["/", "/tasks", "/docs/"] {
            page.open(&site.url(route));
            assert!(page.wait_for("document.querySelector('main') !== null", SHOWN));
            assert_eq!(
                page.eval("getComputedStyle(document.documentElement).colorScheme"),
                json!("light dark"),
                "{mode} {route}: scrollbars and native controls take the page's scheme"
            );
        }
        open_panel(&mut page);
        appearance(&mut page);
        key(&mut page, "Home", "Home", 36);
        key(&mut page, "Enter", "Enter", 13);
        assert!(page.wait_for("document.querySelector('#theme-file') !== null", SHOWN));
        let picker = page.eval("(() => {const input=document.querySelector('#theme-file'),label=input.closest('label'),look=getComputedStyle(label),box=label.getBoundingClientRect();\
            return {input:getComputedStyle(input).opacity,label:label.textContent.trim(),background:look.backgroundColor,\
            covers:[[0.1,0.5],[0.5,0.5],[0.9,0.5]].every(([x,y])=>document.elementFromPoint(box.x+box.width*x,box.y+box.height*y)===input)};})()");
        assert_eq!(
            picker,
            json!({"input":"0","label":"Choose a theme file…","background":surface,"covers":true}),
            "{mode}: the native file input is drawn by its label, which takes its clicks"
        );
    }
}
