//! The owner's polish findings (app/ui-polish-1 and app/ui-polish-2), each read in the release
//! client: popups draw on an opaque surface inside the window, as wide as their longest option and
//! scrolling only when the window cannot hold them, the chat panel is one surface whose
//! Send stays in view, missing pages say so and every page reaches a final state, panels answer the
//! keyboard, badges stay readable and native controls follow the scheme, in both schemes. Then the
//! tasks page's (app/tasks-page-polish): a card opens from its title, the filter row reads clearly
//! and the board does not jump while it loads. A select's popup grows from the edge nearest its
//! trigger, leaves drawn and easing out, and under reduced motion only fades.
use super::components::{appearance, key, open_panel, popup_ready, site, task_tab, theme_action};
use super::{READY, SHOWN, STALL, chrome, chrome::Chrome, dist, site::Site};
use serde_json::{Value, json};

/// The board and nothing else: every other read answers 404, as a server without the task or
/// document a page names does.
fn bare_site() -> Site {
    crate::site()
}

/// The page's breadcrumbs, as `[text, href]`, `href` null for the page itself.
fn crumbs(page: &mut Chrome) -> Value {
    page.eval("[...document.querySelectorAll('nav.crumbs > a, nav.crumbs > span')].map(e => [e.textContent, e.getAttribute('href')])")
}

/// The card colour of each scheme's default theme: the surface every popup draws on.
const SURFACES: [(&str, &str); 2] = [("light", "rgb(255, 255, 255)"), ("dark", "rgb(29, 28, 25)")];

#[test]
fn an_open_select_and_the_appearance_panel_draw_on_an_opaque_bounded_surface_in_both_schemes() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    page.viewport(1280, 900, false);
    for (mode, surface) in SURFACES {
        page.scheme(mode);
        page.open(&site.url("/tasks"));
        assert!(page.wait_for(
            "document.querySelector('button[aria-label=\"Finished tasks\"]') !== null",
            SHOWN
        ));
        super::components::board_filters(&mut page);
        page.eval("document.querySelector('button[aria-label=\"Finished tasks\"]').focus()");
        key(&mut page, " ", "Space", 32);
        popup_ready(&mut page);
        let list = page.eval(
            "(() => {const list=document.querySelector('[role=listbox]'),style=getComputedStyle(list),box=list.getBoundingClientRect();\
             return {focused:document.activeElement===list,outline:style.outlineStyle,background:style.backgroundColor,\
             inside:box.top>=0&&box.bottom<=innerHeight,scrolls:list.scrollHeight>list.clientHeight,options:list.querySelectorAll('[role=option]').length};})()",
        );
        assert_eq!(
            list["options"],
            json!(2),
            "{mode}: both finished-work windows"
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
        assert_eq!(
            (&list["inside"], &list["scrolls"]),
            (&json!(true), &json!(false)),
            "{mode}: with 900px of window, every option shows and nothing scrolls: {list}"
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

/// The open list's options: whether it scrolls, its width, its box (left, top, right, bottom) and
/// the window's size, its longest option's text width, how far the chosen option's check mark
/// stands from its text, and each option's text with its height.
const OPTIONS: &str = "(() => {const list=document.querySelector('[role=listbox]'),range=document.createRange(),\
    options=[...list.querySelectorAll('[role=option]')],texts=o=>[...o.childNodes].filter(n=>n.nodeType===3).map(n=>{range.selectNodeContents(n);return range.getBoundingClientRect();}),\
    text=o=>texts(o).reduce((w,box)=>w+box.width,0),chosen=list.querySelector('[role=option][aria-selected=true]'),mark=chosen?.querySelector('svg'),\
    box=list.getBoundingClientRect();\
    return {scrolls:list.scrollHeight>list.clientHeight,width:Math.round(box.width),\
    box:[box.left,box.top,box.right,box.bottom].map(Math.round),window:[innerWidth,innerHeight],\
    longest:Math.round(Math.max(...options.map(text))),\
    apart:mark?Math.round(mark.getBoundingClientRect().left-Math.max(...texts(chosen).map(box=>box.right))):null,\
    options:options.map(o=>[o.textContent.trim(),Math.round(o.getBoundingClientRect().height)])};})()";

#[test]
fn a_list_too_tall_for_the_window_is_as_wide_as_its_longest_option_beside_its_scrollbar() {
    let site = site();
    let mut detail = super::look_parity::responses()["/api/task-page"].clone();
    detail["task"]["roles"] = json!((0..8).map(|i| json!({"handle":format!("example-worker-{i}"),"role":"executor","health":"live","since":1.0,"expires":null})).collect::<Vec<_>>());
    site.set_json("/api/task-page", &detail);
    let Some(mut page) = chrome() else { return };
    // Too short for the task recipients: the list scrolls, its scrollbar inside its box.
    page.viewport(1280, 320, false);
    page.open(&site.url("/task/visual/in-progress"));
    assert!(page.wait_for("document.querySelector('[id^=composer-]') !== null", SHOWN));
    page.eval("document.querySelector('[id^=composer-]').scrollIntoView({block:'center'})");
    assert!(page.wait_for(
        "document.querySelector('button[aria-label=\"Message recipient\"]') !== null",
        SHOWN
    ));
    // The original width case selects its longest label, so its check mark also takes space.
    super::components::choose(
        &mut page,
        "button[aria-label=\"Message recipient\"]",
        "example-worker-0",
    );
    page.eval("document.querySelector('button[aria-label=\"Message recipient\"]').scrollIntoView({block:'center'});document.querySelector('button[aria-label=\"Message recipient\"]').focus()");
    key(&mut page, " ", "Space", 32);
    popup_ready(&mut page);
    let shown = page.eval(OPTIONS);
    assert_eq!(shown["scrolls"], json!(true), "{shown}");
    let options = shown["options"].as_array().unwrap();
    assert_eq!(options[0][0], json!("everyone on it"), "{shown}");
    assert!(
        options.iter().all(|option| option[1] == options[1][1]),
        "every option on one line, \"everyone on it\" and its check mark among them: {shown}"
    );
    // As wide as that longest line and no wider: an option's padding (24px), the check mark
    // (16px) 12px from the text, the list's padding (8px) and a scrollbar's room past its text,
    // not the window's width.
    let (width, longest) = (
        shown["width"].as_i64().unwrap(),
        shown["longest"].as_i64().unwrap(),
    );
    assert!(
        longest > 0 && width >= longest + 60 && width <= longest + 96,
        "the list is as wide as its longest option: {shown}"
    );
    assert!(
        shown["apart"].as_i64().is_some_and(|apart| apart >= 12),
        "the check mark stands apart from the chosen recipient: {shown}"
    );
    // And the whole list stays inside the window, scrolling rather than running past it.
    let [left, top, right, bottom] = [0, 1, 2, 3].map(|side| shown["box"][side].as_i64().unwrap());
    assert_eq!(shown["window"], json!([1280, 320]), "{shown}");
    assert!(
        left >= 0 && top >= 0 && right <= 1280 && bottom <= 320 && bottom > top,
        "the list stays inside the window: {shown}"
    );
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
    page.scheme("dark");
    for (width, height) in [(1280, 800), (390, 844)] {
        page.viewport(width, height, width < 600);
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
        page.viewport(width, height, width < 600);
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
    page.viewport(1440, 900, false);
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
        page.scheme(mode);
        page.open(&site.url("/task/visual/in-progress"));
        // The composer mounts as it nears the window, below the brief and the task's timeline.
        assert!(page.wait_for(
            "document.querySelector('[id^=\"composer-\"]') !== null",
            SHOWN
        ));
        page.eval("document.querySelector('[id^=\"composer-\"]').scrollIntoView({block:'center'})");
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

/// The home's design docs card: how far its summary sits below the card's top edge, and the rule
/// above the summary, as `[gap, border]`.
fn design_docs(page: &mut Chrome) -> Value {
    assert!(page.wait_for(
        "document.querySelector('.docsummary h3') !== null",
        super::SHOWN
    ));
    page.eval("(() => {const card=document.querySelector('.progress.panel'),summary=card.querySelector('.docsummary');\
        return [Math.round(summary.getBoundingClientRect().top-card.getBoundingClientRect().top),getComputedStyle(summary).borderTopWidth];})()")
}

#[test]
fn the_design_docs_card_opens_on_its_summary_and_is_ruled_off_only_below_a_region() {
    let Some(mut page) = chrome() else { return };
    let mut board: Value = serde_json::from_str(&super::common::fixture("board.json")).unwrap();
    board["progress"]["regions"] = json!([]);
    let bare = Site::start(dist(), &board.to_string());
    page.open(&bare.url("/"));
    // The card's own border and padding, and no band or rule above the summary.
    assert_eq!(design_docs(&mut page), json!([19, "0px"]));
    // A region above it is set apart by the rule, as before.
    let regions = bare_site();
    page.open(&regions.url("/"));
    assert!(page.wait_for(
        "document.querySelector('.progress.panel .region') !== null",
        SHOWN
    ));
    let [gap, border] = design_docs(&mut page)
        .as_array()
        .cloned()
        .unwrap()
        .try_into()
        .unwrap();
    assert!(gap.as_i64().unwrap() > 100, "below the region: {gap}");
    assert_eq!(border, json!("1px"));
    // The cards after the next one, without a picture, show their place in the queue.
    assert_eq!(
        page.eval(
            "[...document.querySelectorAll('.nextcard .thumb.glyph')].map(e => e.textContent)"
        ),
        json!(["2", "4"])
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
    for (mode, _) in SURFACES {
        page.scheme(mode);
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
        page.scheme(mode);
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

/// Clicks the trouble block's Retry and waits for `shown`. The page reads again by itself too: a
/// read of its own may have drawn it already.
fn retry(page: &mut Chrome, shown: &str) {
    page.eval("document.querySelector('.trouble button')?.click()");
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
        // No answer at all: the server could not be reached, and only then is the connection named.
        // Dropped before the error ends, so the page's own next read meets no answering route.
        site.drop_route(read, true);
        site.fail_route(read, false);
        page.eval("document.querySelector('.trouble button').click()");
        assert!(
            page.wait_for(
                "document.querySelector('.trouble p')?.textContent === 'The server could not be reached; check the connection. Trying again…'",
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

/// A task page whose first read of its task gets no answer within the limit (`symphonyAnswerMs`, a
/// page global that shortens the 15 s for this test) says so in a read's words and that it is
/// trying again, and tries again by itself: once the server answers, the task is drawn with no
/// click.
#[test]
fn a_task_read_with_no_answer_in_time_says_it_is_trying_again_and_draws_with_no_click() {
    const READ: &str = "/api/task-page";
    let site = site();
    let Some(mut page) = chrome() else { return };
    page.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": "window.symphonyAnswerMs = 5000;"}),
    );
    site.hold_route(READ, true);
    page.open(&site.url("/task/visual/in-progress"));
    let said = page.wait_for(
        "document.querySelector('.trouble h1')?.textContent === 'The task could not be loaded'",
        SHOWN,
    );
    let failed = page.eval("(() => {const block=document.querySelector('.trouble');return {text:block?.querySelector('p')?.textContent ?? null,\
        retry:block?.querySelector('button')?.textContent ?? null};})()");
    let asked = |site: &Site| site.requests().iter().filter(|path| *path == READ).count();
    // Asked again by itself while the server still holds its reads.
    let again = test_time::until_true("the page asking for its task again", STALL, || {
        asked(&site) >= 2
    });
    site.hold_route(READ, false);
    assert!(
        said,
        "no trouble block: {}",
        page.eval("document.body.innerText")
    );
    assert_eq!(
        failed,
        json!({"text": "The server did not answer in time. Trying again…", "retry": "Retry"}),
        "a read's words, never a write's, and its Retry kept"
    );
    again.unwrap_or_else(|stall| panic!("{stall}: asked {} times", asked(&site)));
    assert!(
        page.wait_for(
            "document.querySelector('article.task') !== null && document.querySelector('.trouble') === null",
            SHOWN
        ),
        "the task was not drawn by itself: {}",
        page.eval("document.body.innerText")
    );
}

/// What the page's notice stack says, its server notice aside.
const NOTICES: &str = "[...document.querySelectorAll('.page-notices:not([hidden]) > p:not(.server-notice)')].map(p => p.textContent).join(' | ')";

/// The page's regions that say how far they got, as `[data-ready]` values in document order.
const REGIONS: &str = "[...document.querySelectorAll('[data-ready]')].map(e => e.dataset.ready)";

/// Waits until a region says `state`, then reads every region's state: none is still loading.
fn settles_as(page: &mut Chrome, route: &str, state: &str) -> Value {
    assert!(
        page.wait_for(
            &format!("document.querySelector('[data-ready={state}]') !== null"),
            SHOWN
        ),
        "{route}: no region says {state}: {} {}",
        page.eval(REGIONS),
        page.eval("document.body.innerText")
    );
    page.eval(REGIONS)
}

#[test]
fn a_missing_or_failed_page_says_which_and_no_region_is_left_loading() {
    let bare = bare_site();
    let Some(mut page) = chrome() else { return };
    // The read the server has nothing for answers 404 with the server's own error for it.
    for (route, read, error, regions) in [
        (
            "/docs/MISSING.md",
            "/api/document",
            "not found: docs/MISSING.md",
            json!(["missing"]),
        ),
        (
            "/docs/nofolder/",
            "/api/folder",
            "not found: docs/nofolder",
            json!(["missing"]),
        ),
        (
            "/task/product/nope",
            "/api/task-page",
            "no task 'product/nope'",
            json!(["board", "missing"]),
        ),
        (
            "/runs",
            symphony_api::RUNS_PATH,
            "this project keeps no runs where this server reads",
            json!(["missing"]),
        ),
    ] {
        bare.miss_route(read, Some(error));
        page.open(&bare.url(route));
        assert_eq!(settles_as(&mut page, route, "missing"), regions, "{route}");
        bare.miss_route(read, None);
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
        ("/reports", symphony_api::REPORTS_PATH, json!(["failed"])),
        ("/runs", symphony_api::RUNS_PATH, json!(["failed"])),
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
fn a_wait_filter_read_failure_is_said_and_retries_without_a_click() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    page.viewport(1440, 900, false);
    page.open(&site.url("/tasks"));
    assert!(page.wait_for(
        "document.querySelector('.taskcard') !== null && document.querySelector('.wait-chip.k-nobody') !== null",
        SHOWN
    ));
    site.fail_route("/api/task-board", true);
    page.eval("document.querySelector('.wait-chip.k-nobody').click()");
    assert!(page.wait_for("document.querySelector('.page-notices [role=alert]')?.textContent === 'Board snapshot unavailable (the server answered 500). Trying again…'",SHOWN));
    site.fail_route("/api/task-board", false);
    assert!(page.wait_for("document.querySelector('.page-notices [role=alert]') === null && document.querySelector('.column[data-status=waiting] .taskcard') !== null",SHOWN), "recovered reads: {:?}; page: {}", site.targets(), page.eval("document.body.innerText"));
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
        page.scheme(mode);
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

/// Whether the focus is on the sheet's shown tab panel, which a send holds it on.
const ON_PANEL: &str = "document.activeElement === document.querySelector('#quick-sheet [role=tabpanel]:not([hidden])')";

#[test]
fn a_send_keeps_the_focus_in_the_sheet_so_escape_still_closes_it() {
    let site = site();
    let mut thread = super::panel_open::chat(&["Hello"]);
    thread["id"] = json!(7);
    site.set_json("/api/ask", &thread);
    site.set_json("/api/quick-task", &json!({"slug":"inbox/focus"}));
    let Some(mut page) = chrome() else { return };
    page.open(&site.url("/"));
    let sheet_closed = "document.querySelector('#quick-sheet')?.hidden !== false";

    // A chat sent from its Send button, which the send disables with the message.
    open_panel(&mut page);
    assert!(
        page.wait_for("document.activeElement.id === 'quick-text'", SHOWN),
        "{}",
        page.eval(FOCUS)
    );
    page.call("Input.insertText", json!({"text":"Hello"}));
    site.hold_route("/api/ask", true);
    page.eval("document.querySelector('#quick-sheet button[type=submit]').focus()");
    key(&mut page, "Enter", "Enter", 13);
    assert!(
        page.wait_for(
            &format!("document.querySelector('#quick-text').disabled && {ON_PANEL}"),
            SHOWN
        ),
        "while it sends, the focus stays in the sheet: {}",
        page.eval(FOCUS)
    );
    site.hold_route("/api/ask", false);
    assert!(
        page.wait_for(
            "document.querySelector('#quick-text')?.value === '' && !document.querySelector('#quick-text').disabled && document.activeElement.id === 'quick-text'",
            SHOWN
        ),
        "once sent, the focus is back in the message field: {}",
        page.eval(FOCUS)
    );
    escape_returns_to(&mut page, sheet_closed, "#quick-open");

    // A task sent by Enter in its title, the sheet closed by Escape while it saves: the save's end
    // leaves the focus where Escape gave it back.
    open_panel(&mut page);
    task_tab(&mut page);
    page.eval("document.querySelector('#quick-title').focus()");
    page.call("Input.insertText", json!({"text":"Focus task"}));
    site.hold_route("/api/quick-task", true);
    key(&mut page, "Enter", "Enter", 13);
    assert!(
        page.wait_for(
            &format!("document.querySelector('#quick-title').disabled && {ON_PANEL}"),
            SHOWN
        ),
        "while it saves, the focus stays in the sheet: {}",
        page.eval(FOCUS)
    );
    escape_returns_to(&mut page, sheet_closed, "#quick-open");
    site.hold_route("/api/quick-task", false);
    assert!(
        page.wait_for(
            "document.querySelector('#quick-sheet a[href=\"/task/inbox/focus\"]') !== null",
            SHOWN
        ),
        "the save never lands"
    );
    // Read two frames after the save's end was drawn, its effects run.
    assert_eq!(
        page.eval("new Promise(done => requestAnimationFrame(() => requestAnimationFrame(() => done(document.activeElement === document.querySelector('#quick-open')))))"),
        json!(true),
        "the save's end took the focus from where the reader was: {}",
        page.eval(FOCUS)
    );
}

#[test]
fn buttons_share_one_shape_and_one_focus_ring_and_pages_one_heading() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    page.viewport(1280, 900, false);
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

    page.viewport(390, 844, true);
    page.open(&site.url("/tasks"));
    assert!(page.wait_for(
        "document.querySelector('.board-filters-toggle') !== null",
        SHOWN
    ));
    open_panel(&mut page);
    appearance(&mut page);
    let looks = page.eval("(() => {const look=e=>{const s=getComputedStyle(e),box=e.getBoundingClientRect();return {radius:s.borderRadius,height:box.height,cursor:s.cursor,weight:s.fontWeight};};\
        return {component:look(document.querySelector('.board-filters-toggle')),plain:look(document.querySelector('#quick-sheet button.collapse')),\
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
        ".board-filters-toggle",
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
    page.viewport(1280, 420, false);
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
    page.viewport(1280, 900, false);
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

    // d) The phone's search/filter button names the region and its expanded state.
    page.viewport(390, 844, true);
    page.open(&site.url("/tasks"));
    assert!(page.wait_for(
        "document.querySelector('.board-filters-toggle') !== null",
        SHOWN
    ));
    assert_eq!(page.eval("(() => {const b=document.querySelector('.board-filters-toggle');return [b.getAttribute('aria-controls'),b.textContent,b.getAttribute('aria-expanded')];})()"),json!(["board-filters","Search and filters","false"]));
    page.eval("document.querySelector('.board-filters-toggle').click()");
    assert!(page.wait_for("document.querySelector('.board-filters-toggle').getAttribute('aria-expanded') === 'true' && document.querySelector('#board-filters').checkVisibility()",SHOWN));

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
    page.viewport(1280, 900, false);
    page.open(&site.url("/tasks"));
    page.reveal("#column-review");
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
    // A card's slot skips drawing its contents (`content-visibility: auto`) until a rendering
    // pass of the browser's finds it on the screen. A press before that pass lands on the slot,
    // not the title: the page has not drawn the card yet, so no reader could have clicked it. The
    // title is clicked once hit testing reaches it.
    assert!(
        page.wait_for(
            &format!(
                "document.querySelector({}).checkVisibility({{contentVisibilityAuto: true}})",
                json!(title)
            ),
            SHOWN
        ),
        "the title was never drawn for hit testing"
    );
    page.eval(
        "window.__pressed = []; for (const kind of ['mousedown', 'click']) document.addEventListener(kind, e => window.__pressed.push(kind + ' ' + e.target.nodeName + '.' + e.target.className), true); null",
    );
    click_middle(&mut page, title);
    assert!(
        page.wait_for("location.pathname === '/task/tools/example'", SHOWN),
        "the title did not open its task: {}; the press went to {}",
        page.eval("location.href"),
        page.eval("window.__pressed")
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
/// Finished tasks label stands 16 px or more after that text or on a line below it, the field's
/// placeholder, the finished-window field's width and the left of its label.
const FILTERS: &str = "(() => {
    const [search, finished] = document.querySelectorAll('.filters > label');
    const text = document.createRange();
    text.selectNodeContents(search.firstChild);
    const words = text.getBoundingClientRect(), field = search.querySelector('input'), next = finished.getBoundingClientRect();
    return {wide: field.getBoundingClientRect().right >= words.right,
        apart: next.left - words.right >= 16 || next.top >= search.getBoundingClientRect().bottom - 1,
        placeholder: field.placeholder, status: finished.querySelector('button').getBoundingClientRect().width,
        finished: finished.getBoundingClientRect().left};
})()";

/// The search field fits its label and stays apart from the finished window. Choosing a window
/// preserves the controls' widths and positions. A phone opens them from its filters button.
#[test]
fn the_tasks_filters_read_apart_and_a_chosen_window_moves_nothing() {
    let site = bare_site();
    site.set_json("/api/task-board", &every_status());
    let Some(mut page) = chrome() else { return };
    for (width, height) in [(1280, 900), (390, 844)] {
        page.viewport(width, height, width < 600);
        page.open(&site.url("/tasks"));
        assert!(
            page.wait_for(
                // The nine statuses' columns and the one rail Build and Test are drawn as.
                "document.querySelector('.waiting-columns .taskcard') !== null",
                SHOWN
            ),
            "{width}px: {}",
            page.eval("document.querySelector('main')?.innerText")
        );
        {
            page.eval("document.querySelector('.board-filters-toggle').click()");
            assert!(
                page.wait_for(
                    "document.querySelector('#board-filters')?.checkVisibility() === true",
                    SHOWN
                ),
                "{width}px: the button never showed the filters"
            );
        }
        let before = page.eval(FILTERS);
        assert_eq!(
            (&before["wide"], &before["apart"], &before["placeholder"]),
            (&json!(true), &json!(true), &json!("Slug or words")),
            "{width}px: the search field is as wide as its label, which stands apart from the next: {before}"
        );
        for option in ["Last week", "Last 24 hours"] {
            super::components::choose(
                &mut page,
                ".filters button[aria-label='Finished tasks']",
                option,
            );
            let after = page.eval(FILTERS);
            assert_eq!(
                (&after["status"], &after["finished"]),
                (&before["status"], &before["finished"]),
                "{width}px: changing the finished window moved the filters"
            );
        }
    }
}

/// Opening a short Backlog list or a long Cancelled list uses only the chosen list's height.
#[test]
fn separately_opened_backlog_and_cancelled_lists_take_the_height_of_their_own_cards() {
    let site = bare_site();
    let mut board = every_status();
    let tasks = board["tasks"].as_array_mut().unwrap();
    for n in 1..=12 {
        tasks.push(json!({"slug": format!("filters/cancelled-{n}"), "title": format!("A cancelled task {n}"),
            "body": "", "status": "cancelled", "priority": 2, "parent": null, "created_by": "owner",
            "created": 0.0, "updated": 4102444800.0, "links": [], "depends": [], "roles": [],
            "instruction": false}));
    }
    board["counts"][8] = json!(["cancelled", 13]);
    site.set_json("/api/task-board", &board);
    let Some(mut page) = chrome() else { return };
    page.viewport(1280, 900, false);
    page.open(&site.url("/tasks"));
    assert!(page.wait_for(
        "document.querySelector('button.board-backlog') !== null",
        SHOWN
    ));
    page.eval("document.querySelector('.board-backlog').click()");
    assert!(page.wait_for(
        "document.querySelector('.column[data-status=backlog] .taskcard') !== null",
        SHOWN
    ));
    let short = page
        .eval(
            "document.querySelector('.column[data-status=backlog]').getBoundingClientRect().height",
        )
        .as_f64()
        .unwrap();
    page.eval("document.querySelector('.back-to-stages').click()");
    page.eval("document.querySelector('[data-open-stage=cancelled]').click()");
    assert!(page.wait_for(
        "document.querySelectorAll('.column[data-status=cancelled] [id^=row-]').length === 13",
        SHOWN
    ));
    let long=page.eval("document.querySelector('.column[data-status=cancelled]').getBoundingClientRect().height").as_f64().unwrap();
    assert!(
        short * 3.0 < long,
        "one backlog card takes its own height {short}, not the thirteen cancelled cards' {long}"
    );
}

/// Thirty ready tasks numbered 1 to 30, in the order their slugs sort as text (`-1`, `-10`, …),
/// as the board answers the tasks page. They carry what cards on a live board carry, so their
/// heights differ as real cards' do: one role chip or two, a link chip or two on some, and titles
/// of one, two or three lines. (The board shows no subtasks, so no card here has a parent.)
pub(super) fn thirty_ready() -> Value {
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
    let drawn = test_time::until_true(format!("{width}px: every ready row drawn"), STALL, || {
        page.eval("document.querySelectorAll('.column[data-status=ready] .taskcard').length")
            == json!(30)
            || {
                page.eval("scrollBy(0, innerHeight / 2)");
                page.pump(std::time::Duration::from_millis(50));
                false
            }
    });
    if let Err(stall) = drawn {
        panic!("{stall}: {}", page.eval(READY_COLUMN));
    }
}

/// A browser of its own whose every page holds the ready column's page until the test calls
/// `window.letReadyIn()`, so the column's loading reserve can be measured. Its profile is new, so
/// the board is a first visit, drawn from the network rather than the device's copy.
fn chrome_holding_ready() -> Option<Chrome> {
    let mut page = chrome()?;
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
    Some(page)
}

/// The tasks board (app/tasks-page-polish) does not jump while its rows load: a column waiting for
/// its page reserves the rows it will hold, and a row waiting to be drawn the height its own card
/// draws at, chips, links and parent included, so the column ends where it will once every row is
/// in, on a desk or a phone; and a column's numbered slugs read in number order, `-2` before `-10`.
/// Each width is a first visit in a browser of its own: a second would draw from the device's copy.
#[test]
fn the_tasks_board_keeps_its_height_as_rows_load() {
    let site = bare_site();
    site.set_json("/api/task-board", &thirty_ready());
    for (width, height) in [(1280, 800), (390, 844)] {
        let Some(mut page) = chrome_holding_ready() else {
            return;
        };
        page.viewport(width, height, width < 600);
        page.open(&site.url("/tasks"));
        if width <= 760 {
            assert!(page.wait_for(
                "document.querySelector('.stage-tabs button') !== null",
                SHOWN
            ));
            page.eval("[...document.querySelectorAll('.stage-tabs button')].find(button=>button.textContent.startsWith('Ready')).click()");
        }
        assert!(
            page.wait_for(
                // The column reading its page is busy, its rows' place held by their ghost.
                "window.readyHeld === true && document.querySelector('.column[data-status=ready][aria-busy=true] #column-ready [data-pending=true]') !== null",
                SHOWN
            ),
            "{width}px: the ready column never waited for its page: {}",
            page.eval("[window.readyHeld ?? null, document.querySelector('main')?.dataset.cache ?? null, document.querySelectorAll('#column-ready [id^=\"row-\"]').length]")
        );
        let loading =
            page.eval("document.querySelector('#column-ready').getBoundingClientRect().height");
        page.eval("window.letReadyIn()");
        assert!(page.wait_for(
            "document.querySelectorAll('.column[data-status=ready] [id^=\"row-\"]').length === 30 && document.querySelector('.column[data-status=ready] .taskcard') !== null",
            SHOWN
        ));
        page.frames_with_fonts();
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

/// Where the card of `load/ready-{row}` stands below what the window shows of the page's header (a
/// phone's scrolls away with the page), the room the page's scroll padding leaves above a card it
/// scrolls to, whether the two rows the board shows under it have drawn, and whether it stands at
/// the board's top, below the room the board's own scroll padding leaves there (a phone's, for its
/// column's heading, which stays at the board's top) and clear of its column's heading.
fn named_card(row: u32) -> String {
    format!("(() => {{const card=document.getElementById('row-load/ready-{row}')?.querySelector('.taskcard');
        const header=Math.max(0,document.querySelector('.app-header')?.getBoundingClientRect().bottom??0);
        const below=['row-load/ready-{}','row-load/ready-{}'].map(id=>document.getElementById(id));
        const r=card?.getBoundingClientRect();
        return {{top:card?Math.round(r.top-header):null, room:Math.round(parseFloat(getComputedStyle(document.documentElement).scrollPaddingTop)-header),
            below:below.every(slot=>slot?.querySelector('.taskcard')), clear:card?r.top>=header:false,
            inBoard:card?r.top>=0&&r.bottom<=innerHeight:false, pageEnd:scrollY>=document.documentElement.scrollHeight-innerHeight-1}}; }})()",row+1,row+2)
}

/// A link to a card far down a long column (app/tasks-page-polish, `/tasks#row-load/ready-20`)
/// lands with the card just below the header, where the page's scroll padding puts it, the board
/// scrolled inside itself to it (on a phone, below its column's heading, which stays at the
/// board's top), and the card stays there as the rows below it draw, on a desk and a phone; one
/// among the column's last (`ready-28`) stays too, the board scrolling no further for rows still to
/// draw below it. Each link is a first visit, in a browser of its own, so the board loads from the
/// network as the rows below the card draw, not at once from the device's copy.
#[test]
fn a_link_to_a_card_far_down_a_column_lands_under_the_header_and_stays() {
    let site = bare_site();
    site.set_json("/api/task-board", &thirty_ready());
    // Row 28 is among a column's last: the board cannot scroll it to its top, so the page brings
    // it up instead, and it stays as the last rows draw.
    for (width, height, row) in [(1280, 900, 20), (1280, 900, 28), (390, 844, 19)] {
        let Some(mut page) = chrome() else { return };
        page.viewport(width, height, width < 600);
        page.open(&site.url(&format!("/tasks#row-load/ready-{row}")));
        let named = named_card(row);
        assert!(
            page.wait_for(
                &format!("(() => {{const at = {named}; return at.top !== null && at.below && {READY};}})()"),
                SHOWN
            ),
            "{width}px: the named card or the rows below it never drew: {}",
            page.eval(&named)
        );
        page.frames_with_fonts();
        let landed = page.eval(&named);
        let top = landed["top"].as_f64().unwrap();
        // The board scrolls inside itself to put the card at its top, or to its end for one of
        // the column's last cards, and the page brings that to its scroll padding under the
        // header, as far as the page runs: a phone's short page ends first, leaving the card
        // lower but in full view.
        let room = landed["room"].as_f64().unwrap().max(0.0);
        assert!(
            landed["inBoard"] == json!(true)
                && landed["clear"] == json!(true)
                && top >= 0.0
                && (top <= room + 4.0 || (landed["pageEnd"] == json!(true) && top <= 400.0)),
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
fn the_waiting_stage_labels_start_alike_in_their_columns() {
    let site = bare_site();
    site.set_json("/api/task-board", &every_status());
    let Some(mut page) = chrome() else { return };
    page.viewport(1280, 900, false);
    page.open(&site.url("/tasks"));
    assert!(page.wait_for(
        "document.querySelectorAll('.waiting-columns .column').length === 8",
        SHOWN
    ));
    let labels = page.eval("[...document.querySelectorAll('.waiting-columns .column')].map(column => { const label = column.querySelector('h2'), words = document.createRange(); words.selectNodeContents(label.firstChild); return [column.dataset.status, Math.round(words.getBoundingClientRect().left - column.getBoundingClientRect().left), getComputedStyle(label, '::before').content]; })");
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

/// The board's Finished tasks select opened from the keyboard, its popup settled.
fn open_finished(page: &mut Chrome) {
    assert!(page.wait_for(
        "document.querySelector('button[aria-label=\"Finished tasks\"]') !== null",
        SHOWN
    ));
    super::components::board_filters(page);
    page.eval("document.querySelector('button[aria-label=\"Finished tasks\"]').focus()");
    key(page, " ", "Space", 32);
    popup_ready(page);
}

/// The open popup: the side it opened on, its transform's origin as a share of its width and
/// height, where it lies and where its trigger does.
const POPUP: &str = "(() => {const list=document.querySelector('[role=listbox]'),box=list.getBoundingClientRect(),\
    trigger=document.querySelector('button[aria-label=\"Finished tasks\"]').getBoundingClientRect(),\
    [x,y]=getComputedStyle(list).transformOrigin.split(' ').map(parseFloat),share=v=>Math.round(v*100)/100;\
    return {side:list.dataset.side,origin:[share(x/box.width),share(y/box.height)],\
    top:box.top,bottom:box.bottom,trigger:[trigger.top,trigger.bottom]};})()";

/// The open popup's entrance at its first frame, as `[transform, opacity]`, then let finish again.
const ENTRANCE: &str = "(() => {const list=document.querySelector('[role=listbox]'),[entrance]=list.getAnimations();\
    entrance.pause();entrance.currentTime=0;const style=getComputedStyle(list),first=[style.transform,style.opacity];\
    entrance.finish();return first;})()";

/// Watches the open popup leave: as it is drawn closed, its exit's easing and keyframes, and at
/// the next frame whether it is still in the page to draw (`window.leaving`).
const WATCH_EXIT: &str = "(() => {const list=document.querySelector('[role=listbox]');window.leaving=[];\
    new MutationObserver(()=>{if(list.dataset.state!=='closed')return;const style=getComputedStyle(list);\
    window.leaving.push([list.isConnected,style.animationName,style.animationTimingFunction]);\
    requestAnimationFrame(()=>window.leaving.push(list.isConnected));}).observe(list,{attributes:true,attributeFilter:['data-state']});})()";

/// Escape on the open popup, once it has left: what `WATCH_EXIT` saw.
fn leave(page: &mut Chrome) -> Value {
    page.eval(WATCH_EXIT);
    key(page, "Escape", "Escape", 27);
    assert!(page.wait_for("document.querySelector('[role=listbox]') === null", SHOWN));
    page.eval("window.leaving")
}

#[test]
fn a_select_popup_grows_from_its_trigger_leaves_easing_out_and_under_reduced_motion_only_fades() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    page.viewport(1280, 900, false);
    page.open(&site.url("/tasks"));
    // Below its trigger it grows from its top edge, drifting down from the trigger.
    open_finished(&mut page);
    let below = page.eval(POPUP);
    assert_eq!(below["side"], json!("bottom"), "{below}");
    assert_eq!(
        below["origin"],
        json!([0.5, 0]),
        "from the middle of its top edge: {below}"
    );
    assert_eq!(
        page.eval(ENTRANCE),
        json!(["matrix(0.95, 0, 0, 0.95, 0, -1.9)", "0"]),
        "it enters scaled down and drawn toward its trigger"
    );
    // Its exit is drawn, easing out, and then it goes.
    assert_eq!(
        leave(&mut page),
        json!([[true, "dx-picker-out", "ease-out"], true]),
        "the popup closed is drawn leaving"
    );
    // A window too short below its trigger opens it above, growing from its bottom edge.
    let trigger_bottom = below["trigger"][1].as_f64().unwrap();
    page.viewport(1280, (trigger_bottom + 52.0).ceil() as u32, false);
    open_finished(&mut page);
    let above = page.eval(POPUP);
    assert_eq!(above["side"], json!("top"), "{above}");
    assert!(
        above["bottom"].as_f64() <= above["trigger"][0].as_f64(),
        "it lies above its trigger: {above}"
    );
    assert_eq!(
        above["origin"],
        json!([0.5, 1]),
        "from the middle of its bottom edge: {above}"
    );
    assert_eq!(
        page.eval(ENTRANCE),
        json!(["matrix(0.95, 0, 0, 0.95, 0, 1.9)", "0"]),
        "it enters drawn toward its trigger, from below"
    );
    leave(&mut page);
    // Under reduced motion it neither scales nor drifts: it fades in, and out.
    page.call(
        "Emulation.setEmulatedMedia",
        json!({"features":[{"name":"prefers-reduced-motion","value":"reduce"}]}),
    );
    for height in [900, (trigger_bottom + 52.0).ceil() as u32] {
        page.viewport(1280, height, false);
        open_finished(&mut page);
        assert_eq!(
            page.eval(ENTRANCE),
            json!(["matrix(1, 0, 0, 1, 0, 0)", "0"]),
            "at {height}px it only fades in: {}",
            page.eval(POPUP)
        );
        assert_eq!(
            leave(&mut page),
            json!([[true, "dx-picker-out", "ease-out"], true]),
            "and fades out"
        );
    }
}
