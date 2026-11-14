//! Regressions found when native selects became deferred, styled controls.
use super::*;

fn popup_ready(page: &mut crate::Chrome) {
    assert!(page.wait_for("(() => {const list=document.querySelector('[role=listbox][data-state=open][data-positioned=true]');return list && list.getAnimations().every(animation=>animation.playState==='finished');})()", SHOWN));
}

fn all_options_visible(page: &mut crate::Chrome) {
    assert_eq!(page.eval("[...document.querySelectorAll('[role=listbox] [role=option]')].every(option=>{const r=option.getBoundingClientRect();return r.width>0 && r.height>0 && r.left>=0 && r.right<=innerWidth && r.top>=0 && r.bottom<=innerHeight && document.elementFromPoint(r.x+r.width/2,r.y+r.height/2)?.closest('[role=option]')===option;})"), json!(true), "every option must be inside the viewport and clickable: {}", page.eval("[...document.querySelectorAll('[role=listbox],[role=option]')].map(e=>({text:e.textContent,box:e.getBoundingClientRect().toJSON()}))"));
}

/// A script true once the task page has nothing left to draw and is scrolled to its true bottom.
/// Its end moves until then: the gates' answer, the thread's, a read's failure in the page's
/// notices, the timeline's, each comes when it comes, after the composer may already show
/// (`viewport::Settle` lets it out after three seconds whatever is still reading). So the page has
/// drawn its data and is idle (the shell loads a task's timeline only then, and its answer is
/// drawn), nothing on it is busy reading or waits for its read, its feed not stale; each look
/// scrolls to the bottom again.
const AT_REST_AT_THE_BOTTOM: &str = "(() => {
    scrollTo(0, document.documentElement.scrollHeight);
    return window.reviewTimelineReady === true && document.querySelector('#timeline .tl-drawn') !== null
        && document.querySelector('[aria-busy=true], [data-pending=true], main[data-feed=stale]') === null
        && scrollY + innerHeight >= document.documentElement.scrollHeight - 1;
})()";

/// What [`AT_REST_AT_THE_BOTTOM`] reads, for a failure to say.
const AT_REST: &str = "({timeline: [window.reviewTimelineReady ?? null, document.querySelector('#timeline .tl-drawn') !== null],
    busy: [...document.querySelectorAll('[aria-busy=true], [data-pending=true]')].map(e => e.id || e.className || e.tagName),
    feed: document.querySelector('main')?.dataset.feed ?? null,
    scroll: scrollY, height: innerHeight, page: document.documentElement.scrollHeight})";

fn recipient_at_bottom(page: &mut crate::Chrome, site: &Site) {
    // No landing offer, as the server answers when nothing offers one: its answer draws nothing,
    // whenever it comes. (Left unset, the stand-in answers it 404, which the page says in its
    // notices at its end, and nothing on the page marks that answer as still to come.)
    site.set_json(
        "/api/task-offer",
        &json!({"action":"","label":"","branch":"","payload":null}),
    );
    page.open(&site.url("/task/visual/in-progress"));
    assert!(page.wait_for("document.querySelector('article.task') !== null", SHOWN));
    // The thread and the composer under it read once they near the screen.
    page.eval("scrollTo(0,document.documentElement.scrollHeight)");
    assert!(page.wait_for(
        &crate::visible("#task-send button[aria-label=\"Message recipient\"]"),
        SHOWN
    ));
    assert!(
        page.wait_for(AT_REST_AT_THE_BOTTOM, SHOWN),
        "exercise the true page bottom, once the page has nothing left to draw: {}",
        page.eval(AT_REST)
    );
    page.eval("document.querySelector('#task-send button[aria-haspopup=listbox]').focus()");
    key(page, "Enter", "Enter", 13);
    popup_ready(page);
}

fn recipient_geometry(page: &mut crate::Chrome) -> serde_json::Value {
    page.eval(r#"(() => {
        const trigger = document.querySelector('#task-send button[aria-haspopup=listbox]');
        const list = document.querySelector('[role=listbox]');
        const anchor = trigger.getBoundingClientRect(), view = visualViewport;
        return {scroll: scrollY, pageHeight: document.documentElement.scrollHeight, height: innerHeight,
            visual: {top: view.offsetTop, height: view.height},
            roomAbove: anchor.top - view.offsetTop, roomBelow: view.offsetTop + view.height - anchor.bottom,
            trigger: anchor.toJSON(), popup: list.getBoundingClientRect().toJSON()};
    })()"#)
}

fn recipient_follows_background_scroll(page: &mut crate::Chrome, above: bool) {
    let (popup_edge, trigger_edge, gap, fraction, more_room) = if above {
        ("bottom", "top", -4.0, 0.7, "above > below + 48")
    } else {
        ("top", "bottom", 4.0, 0.25, "below > above + 48")
    };
    // The task's deferred content can change its page end. Arrange the trigger from live
    // geometry rather than assuming a viewport resize alone forces a different placement.
    // A 24px scroll changes the two sides' room difference by 48px; keep room for that scroll.
    assert!(
        page.wait_for(
            &format!(r#"(() => {{
                const anchor = document.querySelector('#task-send button[aria-haspopup=listbox]').getBoundingClientRect();
                const popup = document.querySelector('[role=listbox]');
                const list = popup.getBoundingClientRect(), view = visualViewport;
                const above = anchor.top - view.offsetTop;
                const below = view.offsetTop + view.height - anchor.bottom;
                const shift = anchor.top + anchor.height / 2 - (view.offsetTop + view.height * {fraction});
                if (Math.abs(shift) > 1) {{
                    scrollBy(0, shift);
                    return false;
                }}
                return ({more_room}) && anchor.top >= view.offsetTop + 24
                    && anchor.bottom <= view.offsetTop + view.height - 24
                    && popup.scrollHeight > Math.max(above, below)
                    && Math.abs(list.{popup_edge} - anchor.{trigger_edge} - ({gap})) < 1
                    && list.top >= view.offsetTop && list.bottom <= view.offsetTop + view.height;
            }})()"#),
            SHOWN
        ),
        "arrange a long popup {} its trigger with room for background scrolling: {}",
        if above { "above" } else { "below" },
        recipient_geometry(page)
    );
    let before = recipient_geometry(page);
    let popup = before["popup"][popup_edge].as_f64().unwrap();
    let trigger = before["trigger"][trigger_edge].as_f64().unwrap();
    assert!(
        ((popup - trigger) - gap).abs() < 1.0,
        "the popup must start at the select's 4px attachment gap: {before}"
    );
    let scroll = before["scroll"].as_f64().unwrap();
    assert!(
        scroll > 24.0,
        "the task must allow background scrolling: {before}"
    );
    page.eval("scrollBy(0,-24)");
    // A long list can fill the space below its trigger: its top follows the trigger while
    // its bottom stays at the viewport edge. Above the trigger, its bottom is attached.
    assert!(
        page.wait_for(
            &format!(r#"(() => {{
                const list = document.querySelector('[role=listbox]').getBoundingClientRect();
                const trigger = document.querySelector('#task-send button[aria-haspopup=listbox]').getBoundingClientRect();
                return scrollY < {scroll} && list.{popup_edge} > {popup} + 20
                    && trigger.{trigger_edge} > {trigger} + 20
                    && Math.abs((list.{popup_edge} - trigger.{trigger_edge}) - ({gap})) < 1
                    && list.top >= visualViewport.offsetTop
                    && list.bottom <= visualViewport.offsetTop + visualViewport.height;
            }})()"#),
            SHOWN
        ),
        "the popup follows its trigger when the document scrolls and stays in the viewport; before: {before}; after: {}",
        recipient_geometry(page)
    );
}

#[test]
fn select_options_stay_visible_at_panel_and_page_bottom_on_desktop_short_window_and_phone() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    for (width, height) in [(1280, 800), (390, 844), (1280, 640)] {
        page.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":width,"height":height,"deviceScaleFactor":1,"mobile":width==390}),
        );
        page.open(&site.url("/"));
        open_panel(&mut page);
        task_tab(&mut page);
        page.eval("document.querySelector('#quick-sheet button[aria-haspopup=listbox]').scrollIntoView({block:'nearest'});document.querySelector('#quick-sheet button[aria-haspopup=listbox]').focus()");
        key(&mut page, "Enter", "Enter", 13);
        popup_ready(&mut page);
        assert_eq!(
            page.eval("document.querySelectorAll('[role=listbox] [role=option]').length"),
            json!(5)
        );
        all_options_visible(&mut page);
        key(&mut page, "End", "End", 35);
        assert!(page.wait_for("document.activeElement.getAttribute('role')==='option' && document.activeElement.textContent.trim()==='p4'", SHOWN));
        key(&mut page, "Enter", "Enter", 13);
        assert!(page.wait_for("document.querySelector('#quick-sheet button[aria-haspopup=listbox]')?.textContent.trim()==='p4'", SHOWN));
        recipient_at_bottom(&mut page, &site);
        assert_eq!(
            page.eval("document.querySelectorAll('[role=listbox] [role=option]').length"),
            json!(5)
        );
        all_options_visible(&mut page);
        key(&mut page, "End", "End", 35);
        assert!(page.wait_for("document.activeElement.getAttribute('role')==='option' && document.activeElement.textContent.trim()==='example'", SHOWN));
        all_options_visible(&mut page);
        key(&mut page, "Escape", "Escape", 27);
        assert!(page.wait_for("document.activeElement.matches('#task-send button[aria-haspopup=listbox]') && document.querySelector('[role=listbox]')===null", SHOWN));
    }
}

#[test]
fn a_long_recipient_list_scrolls_each_option_into_view_and_repositions_on_resize() {
    let site = site();
    let mut data = look_parity::responses().remove("/api/task-page").unwrap();
    let role = data["task"]["roles"][0].clone();
    data["task"]["roles"] = json!(
        (0..40)
            .map(|index| {
                let mut role = role.clone();
                role["handle"] = json!(format!("recipient-{index:02}"));
                role
            })
            .collect::<Vec<_>>()
    );
    site.set_json("/api/task-page", &data);
    let Some(mut page) = chrome() else { return };
    page.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":390,"height":844,"deviceScaleFactor":1,"mobile":true}),
    );
    recipient_at_bottom(&mut page, &site);
    assert_eq!(
        page.eval("document.querySelectorAll('[role=listbox] [role=option]').length"),
        json!(44)
    );
    assert_eq!(page.eval("(() => {const list=document.querySelector('[role=listbox]');return list.scrollHeight>list.clientHeight && getComputedStyle(list).overflowY==='auto';})()"), json!(true), "the fixture must require scrolling");
    let offset = page.eval("scrollY");
    key(&mut page, "Home", "Home", 36);
    for index in 0..44 {
        if index > 0 {
            key(&mut page, "ArrowDown", "ArrowDown", 40);
        }
        assert!(page.wait_for(&format!("document.activeElement===document.querySelectorAll('[role=listbox] [role=option]')[{index}]"), SHOWN));
        assert_eq!(page.eval("(() => {const e=document.activeElement,r=e.getBoundingClientRect(),list=e.closest('[role=listbox]').getBoundingClientRect();return r.top>=list.top && r.bottom<=list.bottom && r.top>=0 && r.bottom<=innerHeight && document.elementFromPoint(r.x+r.width/2,r.y+r.height/2)?.closest('[role=option]')===e;})()"), json!(true), "recipient option {index} must be reachable");
    }
    assert_eq!(
        page.eval("scrollY"),
        offset,
        "option focus scrolls the list, not the page"
    );
    // The composer ends the task page. Give the document a window's room below it, so the
    // trigger can sit high in the viewport with the list below it, whatever notices the page
    // shows at its end.
    page.eval("document.querySelector('main').style.paddingBottom='100vh'");
    recipient_follows_background_scroll(&mut page, false);
    page.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":390,"height":360,"deviceScaleFactor":1,"mobile":true}),
    );
    assert!(page.wait_for("(() => {const r=document.querySelector('[role=listbox]').getBoundingClientRect();return r.top>=0&&r.bottom<=innerHeight;})()", SHOWN));
    recipient_follows_background_scroll(&mut page, true);
    key(&mut page, "Home", "Home", 36);
    assert!(page.wait_for("document.activeElement.textContent.trim()==='everyone on it' && document.activeElement.getBoundingClientRect().top>=0", SHOWN));
    key(&mut page, "End", "End", 35);
    assert!(page.wait_for("document.activeElement.textContent.trim()==='recipient-39' && document.activeElement.getBoundingClientRect().bottom<=innerHeight", SHOWN));
    key(&mut page, "Enter", "Enter", 13);
    assert!(page.wait_for("document.querySelector('#task-send button[aria-haspopup=listbox]').textContent.trim()==='recipient-39' && document.querySelector('[role=listbox]')===null", SHOWN));
}

fn select_ax(page: &mut crate::Chrome) -> serde_json::Value {
    let object = page.call(
        "Runtime.evaluate",
        json!({"expression":"document.getElementById('task-priority')"}),
    );
    let tree = page.call(
        "Accessibility.getPartialAXTree",
        json!({"objectId":object["result"]["objectId"],"fetchRelatives":false}),
    );
    tree["nodes"][0].clone()
}

#[test]
fn a_closed_select_exposes_its_name_and_current_value_in_the_accessibility_tree() {
    let site = site();
    site.set_json("/api/task-priority", &json!({"ok":true}));
    let Some(mut page) = chrome() else { return };
    page.open(&site.url("/task/visual/in-progress"));
    assert!(page.wait_for("document.getElementById('task-priority') !== null", SHOWN));
    for value in ["p1", "p4"] {
        if value == "p4" {
            choose(&mut page, "#task-priority", value);
        }
        let node = select_ax(&mut page);
        assert_eq!(node["role"]["value"], json!("combobox"));
        assert_eq!(node["name"]["value"], json!("Priority"));
        assert_eq!(
            node["value"]["value"],
            json!(value),
            "the closed control must announce its current choice: {node}"
        );
    }
    assert!(
        site.posts()
            .iter()
            .any(|(path, body)| path == "/api/task-priority" && body["priority"] == "4")
    );
}

#[test]
fn postponed_labels_and_comparison_borders_follow_the_theme_in_both_modes() {
    let site = site();
    let mut data = look_parity::responses();
    let queue = data.get_mut("/api/queue").unwrap();
    queue["waiting"][0]["postponed"] = json!(true);
    site.set_json("/api/queue", queue);
    let document = data.get_mut("/api/document").unwrap();
    document["document"]["comparison"] = json!({"task":"visual/appearance"});
    site.set_json("/api/document", document);
    site.set_json("/api/postponed", &json!(["palette"]));
    site.set_json("/api/postpone", &json!({"changed":false}));
    let Some(mut page) = chrome() else { return };
    for (mode, foreground, border) in [
        ("light", "rgb(107, 104, 98)", "rgb(228, 224, 216)"),
        ("dark", "rgb(161, 156, 147)", "rgb(44, 42, 38)"),
    ] {
        page.call(
            "Emulation.setEmulatedMedia",
            json!({"features":[{"name":"prefers-color-scheme","value":mode}]}),
        );
        for route in ["/queue", "/docs/look-parity.md#palette"] {
            page.open(&site.url(route));
            assert!(
                page.wait_for("document.querySelector('.postponed') !== null", SHOWN),
                "{mode} {route}"
            );
            assert_eq!(
                page.eval("getComputedStyle(document.querySelector('.postponed')).color"),
                json!(foreground),
                "{mode} {route}: postponed must use readable foreground"
            );
            if route.starts_with("/docs/") {
                assert_eq!(page.eval("getComputedStyle(document.querySelector('.document-comparison')).borderTopColor"), json!(border), "{mode}: comparison uses the theme border");
            }
            page.eval("document.documentElement.style.setProperty('--muted-foreground','#5a4632');document.documentElement.style.setProperty('--border','#654321')");
            assert_eq!(
                page.eval("getComputedStyle(document.querySelector('.postponed')).color"),
                json!("rgb(90, 70, 50)"),
                "{mode} {route}: the label follows a runtime token, not a fixed default"
            );
            if route.starts_with("/docs/") {
                assert_eq!(page.eval("getComputedStyle(document.querySelector('.document-comparison')).borderTopColor"), json!("rgb(101, 67, 33)"));
            }
        }
    }
}

#[test]
fn selected_queue_categories_keep_the_theme_fill_and_foreground_while_hovered() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    for mode in ["light", "dark"] {
        page.call(
            "Emulation.setEmulatedMedia",
            json!({"features":[{"name":"prefers-color-scheme","value":mode}]}),
        );
        page.open(&site.url("/queue"));
        assert!(page.wait_for(
            "document.querySelector('#you [data-category]:not([data-category=\"\"])') !== null",
            SHOWN
        ));
        // Runtime tokens distinguish selected fill from the outline button's hover palette.
        page.eval("document.documentElement.style.setProperty('--primary','#123456');document.documentElement.style.setProperty('--primary-foreground','#fafafa')");
        let point = page.eval("(() => {const chip=document.querySelector('#you [data-category]:not([data-category=\"\"])');chip.scrollIntoView({block:'center'});const r=chip.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})()");
        page.call(
            "Input.dispatchMouseEvent",
            json!({"type":"mouseMoved","x":point["x"],"y":point["y"]}),
        );
        for kind in ["mousePressed", "mouseReleased"] {
            page.call(
                "Input.dispatchMouseEvent",
                json!({"type":kind,"x":point["x"],"y":point["y"],"button":"left","clickCount":1}),
            );
        }
        assert!(page.wait_for("document.querySelector('#you [data-category]:not([data-category=\"\"])').getAttribute('aria-pressed')==='true'", SHOWN));
        assert!(page.wait_for("(() => {const s=getComputedStyle(document.querySelector('#you [data-category]:not([data-category=\"\"])'));return s.backgroundColor==='rgb(18, 52, 86)' && s.color==='rgb(250, 250, 250)'})()", SHOWN), "{mode}: selected theme colors settle under hover");
        let hovered = page.eval("(() => {const chip=document.querySelector('#you [data-category]:not([data-category=\"\"])');const s=getComputedStyle(chip);return [chip.matches(':hover'),s.backgroundColor,s.color]})()");
        assert_eq!(
            hovered,
            json!([true, "rgb(18, 52, 86)", "rgb(250, 250, 250)"]),
            "{mode}: the selected chip keeps its theme colors under pointer hover"
        );
    }
}

#[test]
fn redeploy_waits_for_a_focused_select_and_its_open_popup_then_reloads_when_disengaged() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    page.open(&site.url("/"));
    open_panel(&mut page);
    task_tab(&mut page);
    crate::wait_for_feed(&site).expect("the page never followed the feed");
    page.eval("window.keptSelectVisit=true;document.querySelector('#quick-sheet button[aria-haspopup=listbox]').focus()");
    for phase in 0..3 {
        if phase == 1 {
            key(&mut page, "Enter", "Enter", 13);
            popup_ready(&mut page);
        } else if phase == 2 {
            key(&mut page, "End", "End", 35);
            assert!(page.wait_for(
                "document.activeElement.getAttribute('role')==='option'",
                SHOWN
            ));
        }
        let marker = format!("Redeploy held at select phase {phase}");
        let mut delta: symphony_api::Delta =
            serde_json::from_str(&crate::common::fixture("delta-card.json")).unwrap();
        delta.version = 5000 + phase;
        for change in &mut delta.changes {
            if let symphony_api::Change::Card(card) = change {
                card.title = marker.clone();
            }
        }
        delta.changes.insert(
            0,
            symphony_api::Change::Client {
                build: "/assets/symphony-shell-0000000000000000.js".into(),
            },
        );
        site.push(delta.version, &serde_json::to_string(&delta).unwrap());
        assert!(page.wait_for(&format!("[...document.querySelectorAll('.nextcard strong')].some(e=>e.textContent==={})", json!(marker)), SHOWN), "the redeploy event must actually be handled");
        assert_eq!(
            page.eval("window.keptSelectVisit"),
            json!(true),
            "the focused trigger, listbox and option each defer reload"
        );
        assert_eq!(
            site.requests().iter().filter(|path| *path == "/").count(),
            1
        );
    }
    key(&mut page, "Escape", "Escape", 27);
    page.eval("document.querySelector('#quick-sheet [aria-label=\"Collapse panel\"]').click();document.activeElement.blur()");
    assert!(page.wait_for("document.querySelector('[role=listbox]')===null && !document.activeElement.matches('input,textarea,button[aria-haspopup=listbox]')", SHOWN));
    let delta = symphony_api::Delta {
        version: 5003,
        changes: vec![symphony_api::Change::Client {
            build: "/assets/symphony-shell-0000000000000000.js".into(),
        }],
    };
    site.push(delta.version, &serde_json::to_string(&delta).unwrap());
    assert!(
        page.wait_for(
            "window.keptSelectVisit===undefined && document.querySelector('.hero-title')!==null",
            SHOWN
        ),
        "a free page must still reload for the deferred build"
    );
    assert_eq!(
        site.requests().iter().filter(|path| *path == "/").count(),
        2
    );
}
