//! The owner's polish findings (app/ui-polish-1), each read in the release client: popups draw on
//! an opaque surface of bounded height, the chat panel is one surface whose Send stays in view,
//! missing pages say so, panels answer the keyboard, badges stay readable and native controls
//! follow the scheme, in both schemes. Then the tasks page's (app/tasks-page-polish): a card opens
//! from its title, the filter row reads clearly and the board does not jump while it loads.
use super::components::{appearance, key, open_panel, site, theme_action};
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
fn an_open_select_and_the_appearance_panel_draw_on_an_opaque_bounded_surface_in_both_schemes() {
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
        // No machine reports the fixture's work, so Build and Test are drawn as one rail and the
        // filter offers neither (`tasks.rs`, docs/MACHINE_WORK.md).
        assert_eq!(
            list["options"],
            json!(10),
            "{mode}: all statuses and the board's nine, taller than the cap"
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
        // The header's Appearance panel lies over the page as the list does.
        appearance(&mut page);
        assert_eq!(
            page.eval(
                "getComputedStyle(document.getElementById('appearance-panel')).backgroundColor"
            ),
            json!(surface),
            "{mode}: the Appearance panel is opaque"
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
fn a_task_page_crumbs_only_its_slug_under_the_lit_board_and_an_unknown_slug_says_there_is_no_such_task()
 {
    let site = site();
    let Some(mut page) = chrome() else { return };
    page.open(&site.url("/task/visual/in-progress"));
    assert!(page.wait_for("document.querySelector('article.task') !== null", SHOWN));
    assert_eq!(
        crumbs(&mut page),
        json!([["visual/in-progress", null]]),
        "no crumb makes a task of a part of the slug, and none repeats the lit Board link"
    );
    assert_eq!(
        page.eval(
            "document.querySelector('.app-pages a[aria-current=page]')?.getAttribute('href')"
        ),
        json!("/tasks")
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
    assert_eq!(crumbs(&mut page), json!([["product/nope", null]]));
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
        // From the header, with the chat closed: the picker is drawn all the same.
        appearance(&mut page);
        theme_action(&mut page, "import");
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

/// The page's trouble block, once it shows: its title, sentence, retry and way back.
fn trouble(page: &mut Chrome) -> Value {
    assert!(
        page.wait_for(
            "document.querySelector('.trouble :is(h1, h3)') !== null",
            SHOWN
        ),
        "no trouble block: {}",
        page.eval("document.body.innerText")
    );
    page.eval("(() => {const block=document.querySelector('.trouble');return {title:block.querySelector('h1, h3').textContent,\
        level:block.querySelector('h1, h3').tagName,part:block.closest('section:not(.trouble)')?.querySelector('h2')?.textContent ?? null,\
        text:block.querySelector('p').textContent,\
        retry:block.querySelector('button')?.textContent ?? null,back:block.querySelector('a')?.getAttribute('href') ?? null,\
        headings:document.querySelectorAll('h1').length};})()")
}

/// Clicks the trouble block's Retry and waits for `shown`.
fn retry(page: &mut Chrome, shown: &str) {
    page.eval("document.querySelector('.trouble button').click()");
    assert!(
        page.wait_for(shown, SHOWN),
        "Retry did not bring the page back: {}",
        page.eval("document.body.innerText")
    );
}

#[test]
fn a_missing_document_says_so_and_a_failed_read_says_it_failed_with_a_retry() {
    let bare = bare_site();
    let Some(mut page) = chrome() else { return };
    page.open(&bare.url("/docs/MISSING.md"));
    let missing = trouble(&mut page);
    assert_eq!(missing["title"], json!("Document not found"), "{missing}");
    assert!(
        missing["text"]
            .as_str()
            .unwrap()
            .contains("docs/MISSING.md"),
        "it names the document: {missing}"
    );
    assert_eq!(missing["back"], json!("/docs/"), "{missing}");
    assert_eq!(
        (&missing["level"], &missing["headings"]),
        (&json!("H1"), &json!(1)),
        "the block is the page: {missing}"
    );
    assert_eq!(
        page.eval(
            "document.querySelector('.document-page').innerText.includes('Loading document')"
        ),
        json!(false)
    );

    let site = site();
    for (route, read, title, shown) in [
        (
            "/task/visual/in-progress",
            "/api/task-page",
            "The task could not be loaded",
            "document.querySelector('article.task') !== null",
        ),
        (
            "/agents",
            "/api/agent-page",
            "The agents could not be loaded",
            "document.querySelector('.trouble') === null && document.querySelector('main h1')?.textContent === 'Agents'",
        ),
        (
            "/",
            symphony_api::BOARD_PATH,
            "The board could not be loaded",
            "document.querySelector('.hero-title') !== null",
        ),
    ] {
        site.fail_route(read, true);
        page.open(&site.url(route));
        let failed = trouble(&mut page);
        assert_eq!(failed["title"], json!(title), "{route}: {failed}");
        assert!(
            failed["text"]
                .as_str()
                .unwrap()
                .contains("Check the connection"),
            "{route}: a failed read is not a missing one: {failed}"
        );
        assert_eq!(failed["retry"], json!("Retry"), "{route}: {failed}");
        site.fail_route(read, false);
        retry(&mut page, shown);
    }
}

#[test]
fn the_task_groups_say_when_they_could_not_be_loaded_and_retry() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    page.open(&site.url("/tasks"));
    assert!(page.wait_for(
        "document.querySelector('.pipeline .column') !== null",
        SHOWN
    ));
    site.fail_route("/api/task-board", true);
    page.eval("document.querySelector('[aria-controls=task-groups]').click()");
    let failed = trouble(&mut page);
    assert_eq!(
        failed["title"],
        json!("The task groups could not be loaded"),
        "{failed}"
    );
    assert_eq!(
        (&failed["level"], &failed["part"], &failed["headings"]),
        (&json!("H3"), &json!("Task groups"), &json!(1)),
        "a part of the tasks page, under its h2 and the page's one h1: {failed}"
    );
    site.fail_route("/api/task-board", false);
    retry(
        &mut page,
        "document.querySelector('#task-groups .trouble') === null && document.querySelector('#task-groups ul li') !== null",
    );
}

/// Each callout badge's text, with the WCAG contrast of its colour on its background.
const BADGE_CONTRAST: &str = "(() => {
    const linear = v => (v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
    const luminance = c => { const [r, g, b] = c.match(/[\\d.]+/g).slice(0, 3).map(Number).map(linear); return 0.2126 * r + 0.7152 * g + 0.0722 * b; };
    return [...document.querySelectorAll('.callout-kind')].map(badge => {
        const look = getComputedStyle(badge), [a, b] = [luminance(look.color), luminance(look.backgroundColor)];
        return [badge.textContent, Math.round((Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05) * 100) / 100];
    });
})()";

#[test]
fn every_callout_badge_reads_at_four_and_a_half_to_one_in_both_schemes() {
    let site = bare_site();
    let kinds = [
        "comment", "like", "dislike", "question", "decision", "resolved",
    ];
    super::restored::document(
        &site,
        kinds
            .iter()
            .map(|kind| json!({"type":"callout","kind":kind,"title":[{"type":"text","text":format!("A {kind}")}],"resolved":false,"blocks":[paragraph("Its body.")]}))
            .collect(),
    );
    let Some(mut page) = chrome() else { return };
    for (mode, _) in SURFACES {
        scheme(&mut page, mode);
        page.open(&site.url("/docs/PROBE.md"));
        assert!(page.wait_for(
            "document.querySelectorAll('.callout-kind').length === 6",
            SHOWN
        ));
        let badges = page.eval(BADGE_CONTRAST);
        let read: Vec<(String, f64)> = serde_json::from_value(badges.clone()).unwrap();
        assert_eq!(
            read.iter()
                .map(|(kind, _)| kind.as_str())
                .collect::<Vec<_>>(),
            kinds,
            "{mode}: {badges}"
        );
        for (kind, ratio) in read {
            assert!(
                ratio >= 4.5,
                "{mode}: the {kind} badge reads at {ratio}:1, under 4.5:1: {badges}"
            );
        }
    }
}

/// Where focus is: the active element's id and text, and the panel it is in.
const FOCUS: &str = "(() => {const a=document.activeElement;return {id:a.id,text:a.textContent.trim().slice(0,40),\
    sheet:a.closest('#quick-sheet')!==null,activity:a.closest('#activity-panel')!==null};})()";

/// Presses Escape and waits for `closed`, with focus back on the element `opener` names.
fn escape_returns_to(page: &mut Chrome, closed: &str, opener: &str) {
    key(page, "Escape", "Escape", 27);
    assert!(
        page.wait_for(
            &format!(
                "({closed}) && document.activeElement === document.querySelector({})",
                json!(opener)
            ),
            SHOWN
        ),
        "Escape must close the panel and return focus to {opener}: {}",
        page.eval(FOCUS)
    );
}

#[test]
fn the_chat_and_activity_panels_take_focus_on_open_and_escape_returns_it_to_their_opener() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    page.open(&site.url("/"));
    let sheet_closed = "document.querySelector('#quick-sheet')?.hidden !== false";
    let activity_closed = "document.querySelector('#activity-panel') === null";

    open_panel(&mut page);
    assert!(
        page.wait_for(
            "document.activeElement.id === 'quick-text' && document.activeElement.closest('#quick-sheet') !== null",
            SHOWN
        ),
        "opening the chat moves focus to its message field: {}",
        page.eval(FOCUS)
    );
    escape_returns_to(&mut page, sheet_closed, "#quick-open");

    // An open list in the sheet takes the first Escape itself; the next closes the sheet.
    open_panel(&mut page);
    super::components::task_tab(&mut page);
    page.eval("document.querySelector('#quick-sheet button[aria-label=\"Priority\"]').focus()");
    key(&mut page, " ", "Space", 32);
    popup_ready(&mut page);
    key(&mut page, "Escape", "Escape", 27);
    assert!(page.wait_for("document.querySelector('[role=listbox]') === null", SHOWN));
    assert_eq!(
        page.eval("document.querySelector('#quick-sheet').hidden"),
        json!(false),
        "the list's Escape is the list's: {}",
        page.eval(FOCUS)
    );
    escape_returns_to(&mut page, sheet_closed, "#quick-open");

    for opener in ["#bell", ".tile.tone-answered"] {
        page.eval(&format!(
            "document.querySelector({}).focus()",
            json!(opener)
        ));
        key(&mut page, "Enter", "Enter", 13);
        assert!(
            page.wait_for(
                "document.activeElement.closest('#activity-panel') !== null",
                SHOWN
            ),
            "{opener}: opening the activity moves focus into it: {}",
            page.eval(FOCUS)
        );
        escape_returns_to(&mut page, activity_closed, opener);
    }
}

#[test]
fn buttons_share_one_shape_and_one_focus_ring_and_pages_one_heading() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1280, 900);
    let mut headings = Vec::new();
    for route in [
        "/",
        "/tasks",
        "/queue",
        "/agents",
        "/task/visual/in-progress",
        "/docs/",
    ] {
        page.open(&site.url(route));
        assert!(
            page.wait_for("document.querySelector('h1') !== null", SHOWN),
            "{route}"
        );
        headings.push(page.eval(&format!("[{}, getComputedStyle(document.querySelector('h1')).fontSize, document.querySelectorAll('h1').length]", json!(route))));
    }
    assert!(
        headings
            .iter()
            .all(|h| h[1] == json!("32px") && h[2] == json!(1)),
        "every page has one h1, of one size: {headings:?}"
    );

    page.open(&site.url("/tasks"));
    assert!(page.wait_for(
        "document.querySelector('[aria-controls=task-groups]') !== null",
        SHOWN
    ));
    open_panel(&mut page);
    appearance(&mut page);
    let looks = page.eval("(() => {const look=e=>{const s=getComputedStyle(e),box=e.getBoundingClientRect();return {radius:s.borderRadius,height:box.height,cursor:s.cursor,weight:s.fontWeight};};\
        return {component:look(document.querySelector('[aria-controls=task-groups]')),plain:look(document.querySelector('#quick-sheet button.collapse')),\
        theme:look(document.querySelector('#appearance-panel [data-theme-action=import]'))};})()");
    assert_eq!(looks["component"]["cursor"], json!("pointer"), "{looks}");
    assert_eq!(
        looks["plain"], looks["component"],
        "a plain button is a component button's shape and size: {looks}"
    );
    assert_eq!(
        looks["theme"], looks["component"],
        "so is the Appearance panel's: {looks}"
    );
    for control in [
        "[aria-controls=task-groups]",
        "#appearance-panel [data-theme-action=import]",
        "#quick-sheet button.collapse",
    ] {
        let rings = page.eval(&format!(
            "(() => {{const e=document.querySelector({}),before=getComputedStyle(e).boxShadow;e.focus();const s=getComputedStyle(e);\
             return {{visible:e.matches(':focus-visible'),outline:s.outlineStyle,shadow:s.boxShadow===before}};}})()",
            json!(control)
        ));
        assert_eq!(
            rings,
            json!({"visible":true,"outline":"solid","shadow":true}),
            "{control}: one focus ring, the page's outline, and no second ring of shadow"
        );
    }
}

#[test]
fn smaller_ones_land_on_their_anchor_stay_on_screen_and_read_as_sentences() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    // a) A link to the machine context lands on it once the agents arrive.
    viewport(&mut page, 1280, 420);
    page.open(&site.url("/agents#machine-context"));
    assert!(
        page.wait_for("(() => {const d=document.getElementById('machine-context');const top=d?.getBoundingClientRect().top;return d !== null && d.open && top >= 0 && top < 140;})()", SHOWN),
        "{}",
        page.eval("(() => {const d=document.getElementById('machine-context');return {open:d?.open,top:d?.getBoundingClientRect().top,scroll:scrollY};})()")
    );
    // c) The agents page labels its probes and counts its followed tasks in words.
    let agents = page.eval("document.querySelector('main').innerText");
    let text = agents.as_str().unwrap();
    assert!(
        text.contains("Terminal panes: unavailable · Test slots: ok"),
        "{text}"
    );
    // The fixture's one agent follows nothing, so it offers no "Following 0 open tasks".
    assert!(!text.contains("Following"), "{text}");

    // b) The collapse button's tooltip stays inside the window.
    viewport(&mut page, 1280, 900);
    page.open(&site.url("/"));
    open_panel(&mut page);
    page.eval("document.querySelector('#quick-sheet button.collapse').focus()");
    assert!(page.wait_for(
        "document.querySelector('#quick-sheet [role=tooltip][data-state=open]') !== null",
        SHOWN
    ));
    let tip = page.eval("(() => {const r=document.querySelector('#quick-sheet [role=tooltip]').getBoundingClientRect();return {left:r.left,right:r.right,size:r.width,width:innerWidth};})()");
    assert!(
        tip["left"].as_f64().unwrap() >= 0.0
            && tip["right"].as_f64().unwrap() <= tip["width"].as_f64().unwrap(),
        "the tooltip is drawn inside the window: {tip}"
    );
    assert!(
        tip["size"].as_f64().unwrap() >= 150.0,
        "the tip reads as a sentence, not a column of words: {tip}"
    );

    // d) The task groups are a labelled region with a labelled toggle.
    page.open(&site.url("/tasks"));
    assert!(page.wait_for(
        "document.querySelector('[aria-controls=task-groups]') !== null",
        SHOWN
    ));
    assert_eq!(
        page.eval("(() => {const b=document.querySelector('[aria-controls=task-groups]');return [b.closest('section')?.querySelector('h2')?.textContent, b.textContent];})()"),
        json!(["Task groups", "Show task groups"])
    );
    page.eval("document.querySelector('[aria-controls=task-groups]').click()");
    assert!(page.wait_for(
        "document.querySelector('[aria-controls=task-groups]').textContent === 'Hide task groups' && document.querySelector('#task-groups') !== null",
        SHOWN
    ));

    // e) A task's thread is one timeline.
    page.open(&site.url("/task/visual/in-progress"));
    assert!(page.wait_for(
        "document.querySelector('#task-thread .entry') !== null",
        SHOWN
    ));
    assert_eq!(
        page.eval("document.querySelectorAll('#task-thread ul.history').length"),
        json!(1)
    );
}

/// A press of the mouse's left button in the middle of the first element `selector` matches.
fn click_middle(page: &mut Chrome, selector: &str) {
    let point = page.eval(&format!(
        "(() => {{const box=document.querySelector({}).getBoundingClientRect();return {{x:box.x+box.width/2,y:box.y+box.height/2}};}})()",
        json!(selector)
    ));
    for kind in ["mousePressed", "mouseReleased"] {
        page.call(
            "Input.dispatchMouseEvent",
            json!({"type":kind,"x":point["x"],"y":point["y"],"button":"left","clickCount":1}),
        );
    }
}

/// The tasks page (app/tasks-page-polish): a card opens its task from its bold title, which
/// shows the hand a link does, and not only from the small slug above it.
#[test]
fn a_task_card_opens_its_task_from_its_title() {
    let site = bare_site();
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1280, 900);
    page.open(&site.url("/tasks"));
    let title = "[data-slug='tools/example'] .taskcard-title";
    assert!(
        page.wait_for(
            &format!("document.querySelector({}) !== null", json!(title)),
            SHOWN
        ),
        "{}",
        page.eval("document.querySelector('.kanban')?.innerHTML.slice(0, 2000)")
    );
    assert_eq!(
        page.eval(&format!("(() => {{const e=document.querySelector({});return [e.textContent, getComputedStyle(e).cursor, getComputedStyle(e).fontWeight];}})()", json!(title))),
        json!(["Example task", "pointer", "600"])
    );
    click_middle(&mut page, title);
    assert!(
        page.wait_for("location.pathname === '/task/tools/example'", SHOWN),
        "the title did not open its task: {}",
        page.eval("location.href")
    );
}

/// The nine lifecycle statuses, one task in each, as the board answers the tasks page.
fn every_status() -> Value {
    let statuses = [
        "backlog",
        "concept",
        "design",
        "ready",
        "in_progress",
        "review",
        "done",
        "blocked",
        "cancelled",
    ];
    let tasks: Vec<_> = statuses
        .iter()
        .map(|status| {
            json!({"slug": format!("filters/{status}"), "title": format!("A task that is {status}"),
                "body": "", "status": status, "priority": 2, "parent": null, "created_by": "owner",
                "created": 0.0, "updated": 4102444800.0, "links": [], "depends": [], "roles": [],
                "instruction": false})
        })
        .collect();
    json!({"tasks": tasks, "statuses": statuses, "counts": statuses.map(|s| (s, 1)),
        "signoff": [], "next": null})
}

/// Where the filters are: whether the search field reaches past its label's text, whether the
/// Status label stands 16 px or more after that text or on a line below it, the field's
/// placeholder, the Status field's width and the left of the Finished tasks label.
const FILTERS: &str = "(() => {
    const [search, status, finished] = document.querySelectorAll('.filters > label');
    const text = document.createRange();
    text.selectNodeContents(search.firstChild);
    const words = text.getBoundingClientRect(), field = search.querySelector('input'), next = status.getBoundingClientRect();
    return {wide: field.getBoundingClientRect().right >= words.right,
        apart: next.left - words.right >= 16 || next.top >= search.getBoundingClientRect().bottom - 1,
        placeholder: field.placeholder, status: status.querySelector('button').getBoundingClientRect().width,
        finished: finished.getBoundingClientRect().left};
})()";

/// The tasks page's filters (app/tasks-page-polish): the search field is at least as wide as its
/// label and says what it takes, so the labels never read as one phrase; choosing a status keeps
/// the Status field's width and the Finished tasks filter where it was; the statuses are named as
/// "All statuses" is; and a status filtered alone is a column's width, not the page's.
#[test]
fn the_tasks_filters_read_apart_and_a_chosen_status_moves_nothing() {
    let site = bare_site();
    site.set_json("/api/task-board", &every_status());
    let Some(mut page) = chrome() else { return };
    for (width, height) in [(1280, 900), (390, 844)] {
        viewport(&mut page, width, height);
        page.open(&site.url("/tasks"));
        assert!(
            page.wait_for(
                // The nine statuses' columns and the one rail Build and Test are drawn as.
                "document.querySelectorAll('.column').length === 10 && document.querySelector('.taskcard') !== null",
                SHOWN
            ),
            "{width}px: {}",
            page.eval("document.querySelector('main')?.innerText")
        );
        let before = page.eval(FILTERS);
        assert_eq!(
            (&before["wide"], &before["apart"], &before["placeholder"]),
            (&json!(true), &json!(true), &json!("Slug or words")),
            "{width}px: the search field is as wide as its label, which stands apart from the next: {before}"
        );
        page.eval("document.querySelector('.filters button[aria-label=Status]').click()");
        assert!(page.wait_for(
            "document.querySelector('[role=listbox] [role=option]') !== null",
            SHOWN
        ));
        assert_eq!(
            page.eval("[...document.querySelectorAll('[role=listbox] [role=option]')].map(o => o.textContent.trim())"),
            json!(["All statuses", "Backlog", "Concept", "Design", "Ready", "In progress", "Review", "Done", "Blocked", "Cancelled"]),
            "{width}px"
        );
        key(&mut page, "Escape", "Escape", 27);
        assert!(page.wait_for(
            "document.querySelector('.filters button[aria-label=Status]').getAttribute('aria-expanded') === 'false'",
            SHOWN
        ));
        for (option, status) in [("Backlog", "backlog"), ("In progress", "in_progress")] {
            super::components::choose(&mut page, ".filters button[aria-label=Status]", option);
            assert!(page.wait_for(
                &format!("document.querySelectorAll('.column').length === 1 && document.querySelector('.column[data-status={status}] .taskcard') !== null"),
                SHOWN
            ));
            let after = page.eval(FILTERS);
            assert_eq!(
                (&after["status"], &after["finished"]),
                (&before["status"], &before["finished"]),
                "{width}px, {option}: choosing a status moved the filters: {before} then {after}"
            );
            assert_eq!(
                page.eval(&format!("document.querySelector('.column[data-status={status}]').getBoundingClientRect().width <= 288")),
                json!(true),
                "{width}px, {option}: a status filtered alone is a column's width"
            );
        }
    }
}

/// Thirty ready tasks numbered 1 to 30, in the order their slugs sort as text (`-1`, `-10`, …),
/// as the board answers the tasks page. They carry what cards on a live board carry, so their
/// heights differ as real cards' do: one role chip or two, a link chip or two on some, and titles
/// of one, two or three lines. (The board shows no subtasks, so no card here has a parent.)
fn thirty_ready() -> Value {
    let mut numbers: Vec<u32> = (1..=30).collect();
    numbers.sort_by_key(u32::to_string);
    let role = |role: &str, handle: &str| json!({"handle": handle, "role": role, "since": 0.0, "expires": null, "health": "live"});
    let tasks: Vec<_> = numbers
        .iter()
        .map(|n| {
            let title = match n % 3 {
                0 => format!("Ready task {n}"),
                1 => format!("Ready task number {n} in a long column"),
                _ => format!("Ready task number {n}, whose title runs on long enough to take three lines"),
            };
            let mut roles = vec![role("orchestrator", "quiet-plover")];
            if n % 3 == 0 {
                roles.push(role("executor", "misty-osprey"));
            }
            let links = match n % 5 {
                0 => json!(["docs/DECISIONS.md", "docs/TASKS.md"]),
                2 => json!(["docs/CHECKS.md"]),
                _ => json!([]),
            };
            json!({"slug": format!("load/ready-{n}"), "title": title, "body": "", "status": "ready",
                "priority": 3, "parent": null, "created_by": "owner", "created": 0.0,
                "updated": 0.0, "links": links, "depends": [], "roles": roles, "instruction": false})
        })
        .collect();
    json!({"tasks": tasks, "statuses": ["ready"], "counts": [["ready", 30]], "signoff": [], "next": null})
}

/// The ready column: its height, its rows' slugs in order, how many rows wait to be drawn, and
/// each drawn row's height against what it reserved (its slot's `contain-intrinsic-size`).
const READY_COLUMN: &str = "(() => {
    const column = document.querySelector('.column[data-status=ready]');
    const slots = [...column.querySelectorAll('[id^=\"row-\"]')];
    const drawn = slots.filter(slot => slot.querySelector('.taskcard')).map(slot =>
        [slot.id, Math.round(slot.getBoundingClientRect().height * 10) / 10, parseFloat(slot.style.containIntrinsicSize.split(' ').pop())]);
    return {height: column.getBoundingClientRect().height, rows: slots.length, drawn,
        waiting: slots.length - drawn.length,
        off: Math.max(0, ...drawn.map(([, height, reserved]) => Math.abs(height - reserved))),
        slugs: [...column.querySelectorAll('.taskcard')].map(card => card.dataset.slug)};
})()";

/// Bring the ready column into the board's view sideways, then scroll the board down it half its
/// height at a time, as a reader does, until every one of its rows is drawn.
fn draw_every_ready_row(page: &mut Chrome, width: u32) {
    page.eval("(() => {const board = document.querySelector('.board-scroll'), column = document.querySelector('.column[data-status=ready]'); board.scrollLeft += column.getBoundingClientRect().left - board.getBoundingClientRect().left;})()");
    let started = std::time::Instant::now();
    while page.eval("document.querySelectorAll('.column[data-status=ready] .taskcard').length")
        != json!(30)
    {
        assert!(
            started.elapsed() < SHOWN,
            "{width}px: the rows never came in: {}",
            page.eval(READY_COLUMN)
        );
        page.eval("(board => board.scrollTop += board.clientHeight / 2)(document.querySelector('.board-scroll'))");
        page.pump(std::time::Duration::from_millis(50));
    }
}

/// The tasks board (app/tasks-page-polish) does not jump while its rows load: a column waiting for
/// its page reserves the rows it will hold, and a row waiting to be drawn the height its own card
/// draws at, chips, links and parent included, so the column ends where it will once every row is
/// in, on a desk or a phone; and a column's numbered slugs read in number order, `-2` before `-10`.
#[test]
fn the_tasks_board_keeps_its_height_as_rows_load() {
    let site = bare_site();
    site.set_json("/api/task-board", &thirty_ready());
    let Some(mut page) = chrome() else { return };
    // The ready column's page waits for the test, so its loading reserve can be measured.
    page.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": "
            const fetchPage = window.fetch;
            let letIn;
            const held = new Promise(resolve => { letIn = resolve; });
            window.letReadyIn = () => letIn();
            window.fetch = async (...args) => {
                const url = typeof args[0] === 'string' ? args[0] : args[0].url;
                if (url.includes('status=ready')) { window.readyHeld = true; await held; }
                return fetchPage(...args);
            };
        "}),
    );
    for (width, height) in [(1280, 800), (390, 844)] {
        viewport(&mut page, width, height);
        page.open(&site.url("/tasks"));
        assert!(page.wait_for(
            "window.readyHeld === true && document.querySelector('#column-ready [role=status]') !== null",
            SHOWN
        ));
        let loading =
            page.eval("document.querySelector('#column-ready').getBoundingClientRect().height");
        page.eval("window.letReadyIn()");
        assert!(page.wait_for(
            "document.querySelectorAll('.column[data-status=ready] [id^=\"row-\"]').length === 30 && document.querySelector('.column[data-status=ready] .taskcard') !== null",
            SHOWN
        ));
        page.eval("document.fonts.ready.then(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))))");
        let first = page.eval(READY_COLUMN);
        let waiting = first["waiting"].as_u64().unwrap();
        assert!(
            waiting > 0,
            "{width}px: every row was drawn at once, so none tests its reserve: {first}"
        );
        draw_every_ready_row(&mut page, width);
        let last = page.eval(READY_COLUMN);
        // Each card draws within a few pixels of what its row reserved, whatever it carries.
        assert!(
            last["off"].as_f64().unwrap() <= 4.0,
            "{width}px: a row drew {}px from what it reserved: {last}",
            last["off"]
        );
        let moved = (last["height"].as_f64().unwrap() - first["height"].as_f64().unwrap()).abs();
        assert!(
            moved <= 2.0 * waiting as f64,
            "{width}px: the column's end moved {moved}px as {waiting} rows came in: {first} then {last}"
        );
        let rows =
            page.eval("document.querySelector('#column-ready').getBoundingClientRect().height");
        let moved = (rows.as_f64().unwrap() - loading.as_f64().unwrap()).abs();
        assert!(
            moved <= 12.0 * 30.0,
            "{width}px: the column reserved {loading}px for its page, which drew {rows}px"
        );
        let numbered: Vec<_> = (1..=30).map(|n| format!("load/ready-{n}")).collect();
        assert_eq!(last["slugs"], json!(numbered), "{width}px");
    }
}

/// Where the card of `load/ready-{row}` stands below the page's header, and whether the two rows
/// the board shows under it have drawn.
fn named_card(row: u32) -> String {
    format!(
        "(() => {{
    const card = document.getElementById('row-load/ready-{row}')?.querySelector('.taskcard');
    const header = document.querySelector('header')?.getBoundingClientRect().bottom ?? 0;
    const below = ['row-load/ready-{}', 'row-load/ready-{}'].map(id => document.getElementById(id)).filter(Boolean);
    const board = document.querySelector('.board-scroll');
    const at = card?.getBoundingClientRect(), view = board?.getBoundingClientRect();
    return {{top: card ? Math.round(at.top - header) : null,
        below: below.length === 2 && below.every(slot => slot.querySelector('.taskcard')),
        boardTop: Math.round(board?.scrollTop ?? 0),
        boardRange: board ? board.scrollHeight - board.clientHeight : null,
        atBoardTop: card && board ? Math.abs(at.top - view.top) < 2 : false,
        boardEnd: board ? board.scrollTop >= board.scrollHeight - board.clientHeight - 1 : false,
        inBoard: card && board ? at.top >= view.top - 1 && at.bottom <= view.bottom + 1 : false,
        pageEnd: scrollY >= document.documentElement.scrollHeight - innerHeight - 1}};
}})()",
        row + 1,
        row + 2
    )
}

/// A link to a card far down a long column (app/tasks-page-polish, `/tasks#row-load/ready-20`)
/// lands with the card just below the header, the board scrolled inside itself to it, and the card
/// stays there as the rows below it draw, on a desk and a phone; one among the column's last
/// (`ready-28`) stays too, the board scrolling no further for rows still to draw below it.
#[test]
fn a_link_to_a_card_far_down_a_column_lands_under_the_header_and_stays() {
    let site = bare_site();
    site.set_json("/api/task-board", &thirty_ready());
    let Some(mut page) = chrome() else { return };
    // Row 28 is among a column's last: the board cannot scroll it to its top, so the page brings
    // it up instead, and it stays as the last rows draw.
    for (width, height, row) in [(1280, 900, 20), (1280, 900, 28), (390, 844, 19)] {
        viewport(&mut page, width, height);
        // A blank page first, so the link loads the board afresh and is no move within the last.
        page.open("about:blank");
        page.open(&site.url(&format!("/tasks#row-load/ready-{row}")));
        let named = named_card(row);
        assert!(
            page.wait_for(
                &format!("(() => {{const at = {named}; return at.top !== null && at.below && document.documentElement.dataset.navigation === 'ready';}})()"),
                SHOWN
            ),
            "{width}px: the named card or the rows below it never drew: {}",
            page.eval(&named)
        );
        page.eval("document.fonts.ready.then(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))))");
        let landed = page.eval(&named);
        let top = landed["top"].as_f64().unwrap();
        // The board scrolls inside itself to put the card at its top, or to its end for one of
        // the column's last cards, and the page brings that to the header, as far as the page
        // runs: a phone's short page ends first, leaving the card lower but in full view.
        assert!(
            (landed["atBoardTop"] == json!(true)
                || (landed["boardEnd"] == json!(true) && landed["inBoard"] == json!(true)))
                && top >= 0.0
                && (top <= 60.0 || (landed["pageEnd"] == json!(true) && top <= 400.0)),
            "{width}px: the card landed {top}px below the header: {landed}"
        );
        page.pump(std::time::Duration::from_millis(600));
        let after = page.eval(&named);
        assert!(
            (after["top"].as_f64().unwrap() - top).abs() <= 2.0,
            "{width}px: the card moved from {landed} to {after}"
        );
    }
}

/// The pipeline's column labels (app/tasks-page-polish) start where their columns do, each the
/// same way in from its column's edge, with nothing drawn before them.
#[test]
fn the_pipeline_labels_start_alike_in_their_columns() {
    let site = bare_site();
    site.set_json("/api/task-board", &every_status());
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1280, 900);
    page.open(&site.url("/tasks"));
    assert!(page.wait_for(
        "document.querySelectorAll('.pipeline > .column').length === 7",
        SHOWN
    ));
    let labels = page.eval("[...document.querySelectorAll('.pipeline > .column')].map(column => { const label = column.querySelector('h2'), words = document.createRange(); words.selectNodeContents(label.firstChild); return [column.dataset.status, Math.round(words.getBoundingClientRect().left - column.getBoundingClientRect().left), getComputedStyle(label, '::before').content]; })");
    let first = &labels[0][1];
    assert!(
        labels
            .as_array()
            .unwrap()
            .iter()
            .all(|label| &label[1] == first && label[2] == json!("none")),
        "{labels}"
    );
}
