//! The owner's polish findings (app/ui-polish-1 and app/ui-polish-2), each read in the release
//! client: popups draw on an opaque surface of bounded height, the chat panel is one surface whose
//! Send stays in view, missing pages say so and every page reaches a final state, panels answer the
//! keyboard, badges stay readable and native controls follow the scheme, in both schemes.
use super::components::{appearance, key, open_panel, site, task_tab};
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

/// The Add task tab under the tabs: from the tabs to its thread (`lead`), the thread's height and
/// what it draws around its lines (`around`), from the thread to the form (`gap`), the rule over
/// the form and what the thread says.
const TASK_FORM: &str = "(() => {const sheet=document.querySelector('#quick-sheet'),tabs=sheet.querySelector('.tabs').getBoundingClientRect(),\
    panel=sheet.querySelector('#quick-task-panel'),thread=panel.querySelector('.quick-thread'),form=panel.querySelector('form'),\
    box=form.getBoundingClientRect(),said=thread.getBoundingClientRect(),\
    line=[...thread.children].reduce((sum,child)=>sum+child.getBoundingClientRect().height,0);\
    return {lead:Math.round(said.top-tabs.bottom),thread:Math.round(said.height),around:Math.round(said.height-line),gap:Math.round(box.top-said.bottom),\
    rule:getComputedStyle(form).borderTopStyle,said:thread.textContent};})()";

#[test]
fn the_add_task_tab_draws_its_form_under_the_tabs_and_what_a_save_came_to_above_it() {
    let site = site();
    site.set_json("/api/quick-task", &json!({"slug":"inbox/first"}));
    let Some(mut page) = chrome() else { return };
    for (width, height) in [(1280, 800), (390, 844)] {
        viewport(&mut page, width, height);
        page.open(&site.url("/"));
        page.eval("localStorage.removeItem('review-quick-draft')");
        page.open(&site.url("/"));
        open_panel(&mut page);
        task_tab(&mut page);
        let quiet = page.eval(TASK_FORM);
        assert!(
            quiet["lead"]
                .as_i64()
                .is_some_and(|lead| (0..=8).contains(&lead)),
            "at {width}: the tab starts under the tabs: {quiet}"
        );
        assert_eq!(
            (
                &quiet["thread"],
                &quiet["gap"],
                &quiet["rule"],
                &quiet["said"]
            ),
            (&json!(0), &json!(0), &json!("none"), &json!("")),
            "at {width}: with nothing to say the form starts there, no band and no rule above it: {quiet}"
        );
        page.eval("for(const [id,value] of [['quick-title','First task'],['quick-text','First details']]) { const field=document.getElementById(id);field.value=value;field.dispatchEvent(new Event('input',{bubbles:true})); }");
        page.eval("document.querySelector('#quick-sheet form').requestSubmit()");
        assert!(page.wait_for(
            "document.querySelector('#quick-task-panel a[href=\"/task/inbox/first\"]') !== null",
            SHOWN
        ));
        let saved = page.eval(TASK_FORM);
        assert_eq!(
            (&saved["said"], &saved["rule"]),
            (&json!("✓ Task inbox/first created"), &json!("solid")),
            "at {width}: {saved}"
        );
        // The thread's own padding alone, 8px over its line and 16px under it: no paragraph
        // margin widens it into a band.
        assert!(
            saved["thread"].as_i64().unwrap() > 24
                && saved["around"] == json!(24)
                && saved["gap"] == json!(0)
                && saved["lead"] == quiet["lead"],
            "at {width}: the thread is as tall as its line, between the tabs and the form: {saved}"
        );
    }
}

/// The task header and brief as drawn: the pills of the state row, who filed the task, whether the
/// owner's controls stand apart from that, whether the slug reads at 12px or more, and whether the
/// brief's paragraph keeps to 80 characters of its own font.
const TASK_HEAD: &str = "(() => {const head=document.querySelector('.task-head'),state=head.querySelector('.task-state'),\
    by=state.querySelector('.by'),controls=state.querySelector('.task-controls'),\
    p=document.querySelector('.task-body p'),probe=document.createElement('span');\
    probe.style.cssText='display:inline-block;width:80ch';p.append(probe);const measure=probe.getBoundingClientRect().width;probe.remove();\
    return {pills:[...state.querySelectorAll('.pill')].map(pill=>pill.textContent),by:by?.textContent ?? null,\
    apart:controls.getBoundingClientRect().left-by.getBoundingClientRect().right>=24,\
    slug:parseFloat(getComputedStyle(head.querySelector('.eyebrow code')).fontSize)>=12,\
    measure:p.getBoundingClientRect().width<=measure+1};})()";

#[test]
fn a_task_header_says_each_thing_once_and_apart_and_its_brief_keeps_a_measure() {
    let site = site();
    let task = json!({"slug":"visual/long","title":"A task with a long brief","body":"","status":"in_progress",
        "priority":2,"parent":null,"created_by":"quiet-plover","created":1.0,"updated":1.0,"links":[],"depends":[],
        "roles":[],"instruction":false});
    let brief = "a brief of many words ".repeat(60);
    site.set_json(
        "/api/task-page",
        &json!({"task":task,"blocks":[paragraph(brief.trim())],"children":[],"children_count":0}),
    );
    let Some(mut page) = chrome() else { return };
    viewport(&mut page, 1440, 900);
    page.open(&site.url("/task/visual/long"));
    assert!(page.wait_for(
        "document.querySelector('.task-body p') !== null && document.querySelector('#task-priority') !== null",
        SHOWN
    ));
    assert_eq!(
        page.eval(TASK_HEAD),
        json!({"pills":["in progress"],"by":"filed by quiet-plover","apart":true,"slug":true,"measure":true}),
        "the priority is said once, by its control; the byline and the controls read apart; the slug \
         and the brief read at a size and a measure"
    );
}

#[test]
fn a_tasks_send_is_the_sheets_filled_send() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    for (mode, _) in SURFACES {
        scheme(&mut page, mode);
        page.open(&site.url("/task/visual/in-progress"));
        assert!(page.wait_for(
            "document.querySelector('#task-send button[type=submit]') !== null",
            SHOWN
        ));
        if page.eval("document.querySelector('#quick-sheet:not([hidden])') === null") == json!(true)
        {
            open_panel(&mut page);
        }
        assert!(page.wait_for(
            "document.querySelector('#quick-sheet button[type=submit]') !== null",
            SHOWN
        ));
        let sends = page.eval("(() => {const look=selector=>{const style=getComputedStyle(document.querySelector(selector));\
            return [style.backgroundColor,style.color];},probe=document.createElement('span');probe.style.color='var(--primary)';\
            document.body.append(probe);const primary=getComputedStyle(probe).color;probe.remove();\
            return {task:look('#task-send button[type=submit]'),sheet:look('#quick-sheet button[type=submit]'),primary};})()");
        assert_eq!(sends["task"], sends["sheet"], "{mode}: one Send: {sends}");
        assert_eq!(
            sends["task"][0], sends["primary"],
            "{mode}: filled with the primary colour: {sends}"
        );
    }
}

#[test]
fn an_agents_line_leaves_out_what_is_not_known_and_its_finished_tasks_end_on_the_last() {
    let site = site();
    let finished: Vec<Value> = (1..=2)
        .map(|n| json!({"slug":format!("done/{n}"),"title":format!("Finished {n}"),"body":"","status":"done",
            "priority":1,"parent":null,"created_by":"owner","created":1.0,"updated":f64::from(n),"links":[],"depends":[],
            "roles":[{"handle":"example","role":"executor","since":1.0,"expires":null,"health":"live"}],"instruction":false}))
        .collect();
    // A worker whose model, branch and worktree are not known.
    let agent = json!({"handle":"example","kind":"worker","model":"","parent":null,"task":null,"role":null,"tasks":[],
        "worktree":"","branch":"","agent":null,"session_id":null,"pane":null,"callback":null,"chat":false,"state":"working",
        "state_detail":null,"health":"live","last_tool_call":null,"last_message":null});
    site.set_json(
        "/api/agent-page",
        &json!({"view":{"generated":1.0,"probes":{"panes":"ok","slots":"ok"},"agents":[agent]},"tasks":finished,"total":1,"next":null}),
    );
    let Some(mut page) = chrome() else { return };
    page.open(&site.url("/agents"));
    assert!(page.wait_for("document.querySelector('.agent') !== null", SHOWN));
    let lines =
        page.eval("[...document.querySelectorAll('.agent > p.muted')].map(p => p.textContent)");
    assert_eq!(
        lines,
        json!(["worker · working", "Finished: Finished 2 · Finished 1"]),
        "no gap between two dots, and none after the last task"
    );
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
    for (mode, _) in SURFACES {
        scheme(&mut page, mode);
        page.open(&bare.url("/task/product/nope"));
        assert!(page.wait_for("document.querySelector('.trouble a') !== null", SHOWN));
        assert_eq!(
            page.eval(WAY_BACK),
            json!({"text":"← All tasks","border":"0px","background":"rgba(0, 0, 0, 0)","link":true,"size":true}),
            "{mode}: the way back reads as a link, at the retry's size, not as a tag"
        );
    }
}

/// The trouble block's way back as drawn: its text, its box and whether it takes the link colour
/// and the retry's text size.
const WAY_BACK: &str = "(() => {const block=document.querySelector('.trouble'),back=block.querySelector('a'),look=getComputedStyle(back),\
    probe=document.createElement('a');block.append(probe);const link=getComputedStyle(probe).color;probe.remove();\
    return {text:back.textContent,border:look.borderTopWidth,background:look.backgroundColor,link:look.color===link,\
    size:look.fontSize===getComputedStyle(block.querySelector('button')).fontSize};})()";

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
        page.eval(WAY_BACK)["text"],
        json!("← All documents"),
        "the way back is named as one: {missing}"
    );
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
        // The server answers with an error of its own: the page says it, with its status, and
        // does not blame the connection; nor does the notice stack say it a second time.
        site.fail_route(read, true);
        page.open(&site.url(route));
        let failed = trouble(&mut page);
        assert_eq!(failed["title"], json!(title), "{route}: {failed}");
        assert!(
            failed["text"]
                .as_str()
                .unwrap()
                .starts_with("Board snapshot unavailable (the server answered 500)."),
            "{route}: a refused read says the server's error and status: {failed}"
        );
        assert!(
            !failed["text"].as_str().unwrap().contains("connection"),
            "{route}: {failed}"
        );
        assert_eq!(failed["retry"], json!("Retry"), "{route}: {failed}");
        assert_eq!(
            page.eval(NOTICES),
            json!(""),
            "{route}: the block says it once"
        );
        site.fail_route(read, false);
        // No answer at all: the server could not be reached, and only then is the connection named.
        site.drop_route(read, true);
        page.eval("document.querySelector('.trouble button').click()");
        assert!(
            page.wait_for(
                "document.querySelector('.trouble p')?.textContent.startsWith('The server could not be reached. Check the connection and try again.')",
                SHOWN
            ),
            "{route}: {}",
            page.eval("document.body.innerText")
        );
        assert_eq!(
            page.eval(NOTICES),
            json!(""),
            "{route}: the block says it once"
        );
        site.drop_route(read, false);
        retry(&mut page, shown);
    }
}

/// What the page's notice stack says, its server notice aside.
const NOTICES: &str = "[...document.querySelectorAll('.page-notices:not([hidden]) > p:not(.server-notice)')].map(p => p.textContent).join(' | ')";

/// The page's regions that say how far they got, as `[data-ready]` values in document order.
const READY: &str = "[...document.querySelectorAll('[data-ready]')].map(e => e.dataset.ready)";

/// Waits until a region says `state`, then reads every region's state: none is still loading.
fn settles_as(page: &mut Chrome, route: &str, state: &str) -> Value {
    assert!(
        page.wait_for(
            &format!("document.querySelector('[data-ready={state}]') !== null"),
            SHOWN
        ),
        "{route}: no region says {state}: {} {}",
        page.eval(READY),
        page.eval("document.body.innerText")
    );
    page.eval(READY)
}

#[test]
fn a_missing_or_failed_page_says_which_and_no_region_is_left_loading() {
    let bare = bare_site();
    let Some(mut page) = chrome() else { return };
    // The read the server has nothing for: the fixture has no such document or task, and a folder
    // it has none of answers 404 as the server's does.
    for (route, read, regions) in [
        ("/docs/MISSING.md", "/api/document", json!(["missing"])),
        ("/docs/nofolder/", "/api/folder", json!(["missing"])),
        (
            "/task/product/nope",
            "/api/task-page",
            json!(["board", "missing"]),
        ),
    ] {
        bare.miss_route(read, true);
        page.open(&bare.url(route));
        assert_eq!(settles_as(&mut page, route, "missing"), regions, "{route}");
        bare.miss_route(read, false);
    }
    let site = site();
    for (route, read, regions) in [
        ("/docs/PROBE.md", "/api/document", json!(["failed"])),
        ("/docs/", "/api/folder", json!(["failed"])),
        (
            "/task/visual/in-progress",
            "/api/task-page",
            json!(["board", "failed"]),
        ),
        ("/agents", "/api/agent-page", json!(["board", "failed"])),
        ("/", symphony_api::BOARD_PATH, json!(["failed"])),
        ("/queue", "/api/queue", json!(["board", "failed"])),
    ] {
        site.fail_route(read, true);
        page.open(&site.url(route));
        assert_eq!(settles_as(&mut page, route, "failed"), regions, "{route}");
        site.fail_route(read, false);
    }
    // Once its read answers, the page is ready as itself.
    page.open(&site.url("/task/visual/in-progress"));
    assert_eq!(
        settles_as(&mut page, "/task/visual/in-progress", "task"),
        json!(["board", "task"])
    );
}

#[test]
fn a_folders_intro_the_server_no_longer_has_draws_nothing_and_reconnects_nothing() {
    let site = bare_site();
    // The folder still names its intro, whose own read answers 404, as after the file was removed.
    let intro = json!({"version":1,"path":"docs/README.md","title":"Documents","waiting":[],"crumbs":[],
        "headings":[],"sections":[],"cards":[],"threads":{},"tasks":[]});
    site.set_json(
        "/api/folder",
        &json!({"path":"docs","title":"Documents","crumbs":[],"docs":1,"intro":intro,
            "entries":[{"kind":"folder","path":"docs/lamps","title":"Lamps","docs":1,"waiting":0,"summary":"Lamp references"}]}),
    );
    let Some(mut page) = chrome() else { return };
    page.open(&site.url("/docs/"));
    assert_eq!(
        settles_as(&mut page, "/docs/", "missing"),
        json!(["folder", "missing"]),
        "the folder is drawn and its intro says it is missing"
    );
    let shown = "(() => ({intro: document.querySelector('.document-page .document-page').innerText, \
        notices: document.querySelector('.page-notices').innerText.trim(), \
        lamps: document.querySelector('.folder-entries').innerText.includes('Lamps')}))()";
    assert!(
        page.wait_for(
            &format!(
                "JSON.stringify({shown}) === JSON.stringify({{intro:'',notices:'',lamps:true}})"
            ),
            SHOWN
        ),
        "the intro draws nothing in its place, not a loading line, and the folder shows no \
         reconnecting notice for it: {}",
        page.eval(shown)
    );
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
    let looks = page.eval("(() => {const look=e=>{const s=getComputedStyle(e),box=e.getBoundingClientRect();return {radius:s.borderRadius,height:box.height,cursor:s.cursor,weight:s.fontWeight};};\
        return {component:look(document.querySelector('[aria-controls=task-groups]')),plain:look(document.querySelector('#quick-sheet button.collapse')),\
        menu:look(document.querySelector('.theme-menu button[aria-haspopup=menu]'))};})()");
    assert_eq!(looks["component"]["cursor"], json!("pointer"), "{looks}");
    assert_eq!(
        looks["plain"], looks["component"],
        "a plain button is a component button's shape and size: {looks}"
    );
    assert_eq!(
        looks["menu"], looks["component"],
        "so is the menu's trigger: {looks}"
    );
    for control in [
        "[aria-controls=task-groups]",
        ".theme-menu button[aria-haspopup=menu]",
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
