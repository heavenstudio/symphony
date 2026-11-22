//! Keyboard interaction and runtime theme imports against the shipped controls.
use super::{SHOWN, assets::module, chrome, dist, look_parity, site::Site};
use serde_json::json;
mod machine_work;
mod regressions;

pub(super) fn site() -> Site {
    let site = crate::site();
    for (path, value) in look_parity::responses() {
        if path != "/api/board" {
            site.set_json(&path, &value);
        }
    }
    site
}

pub(super) fn key(page: &mut super::Chrome, key: &str, code: &str, vk: u16) {
    let mut down = json!({"type":"keyDown","key":key,"code":code,"windowsVirtualKeyCode":vk});
    if key == "Enter" {
        down["text"] = json!("\r");
        down["unmodifiedText"] = json!("\r");
    }
    if key == " " {
        down["text"] = json!(" ");
        down["unmodifiedText"] = json!(" ");
    }
    page.call("Input.dispatchKeyEvent", down);
    page.call(
        "Input.dispatchKeyEvent",
        json!({"type":"keyUp","key":key,"code":code,"windowsVirtualKeyCode":vk}),
    );
}

pub(super) fn board_filters(page: &mut super::Chrome) {
    assert!(page.wait_for("document.querySelector('#board-filters') !== null", SHOWN));
    if page.eval("document.querySelector('#board-filters').checkVisibility()") == json!(false) {
        page.eval("document.querySelector('.board-filters-toggle').click()");
    }
    assert!(page.wait_for(
        "document.querySelector('#board-filters').checkVisibility()",
        SHOWN
    ));
}

/// The open select popup, positioned and done animating in: its options are drawn where they rest.
pub(super) fn popup_ready(page: &mut super::Chrome) {
    assert!(page.wait_for("(() => {const list=document.querySelector('[role=listbox][data-state=open][data-positioned=true]');return list && list.getAnimations().every(animation=>animation.playState==='finished');})()", SHOWN));
}

/// The option `text` chosen in the select `selector` with a mouse's press, and the trigger showing
/// it once its popup has closed.
pub(super) fn choose(page: &mut super::Chrome, selector: &str, text: &str) {
    page.eval(&format!(
        "document.querySelector({}).click()",
        json!(selector)
    ));
    // Aimed once the list is placed and done growing in: an option aimed at while the list still
    // grows, or before it is placed, is no longer where the press lands.
    popup_ready(page);
    // A list taller than the window scrolls: bring the option into the list's view first,
    // as a reader scrolls to it, and click where it is drawn.
    let point = page.eval(&format!("(() => {{const option=[...document.querySelectorAll('[role=listbox] [role=option]')].find(option=>option.textContent.trim()==={});option.scrollIntoView({{block:'nearest'}});const box=option.getBoundingClientRect();return {{x:box.x+box.width/2,y:box.y+box.height/2}}}})()", json!(text)));
    for kind in ["mousePressed", "mouseReleased"] {
        page.call(
            "Input.dispatchMouseEvent",
            json!({"type":kind,"x":point["x"],"y":point["y"],"button":"left","clickCount":1}),
        );
    }
    // The press chose that option, and no other: the closed trigger shows it.
    let trigger = format!("document.querySelector({})", json!(selector));
    assert!(
        page.wait_for(
            &format!(
                "{trigger}.getAttribute('aria-expanded') === 'false' && {trigger}.textContent.trim() === {}",
                json!(text)
            ),
            SHOWN
        ),
        "the press at {point} chose {text}, which the trigger shows: {}",
        page.eval(&format!("{trigger}.textContent.trim()"))
    );
}

pub(super) fn open_panel(page: &mut super::Chrome) {
    assert!(page.wait_for("document.querySelector('#quick-open') !== null", SHOWN));
    page.eval("document.querySelector('#quick-open').focus()");
    key(page, "Enter", "Enter", 13);
    assert!(page.wait_for(
        "document.querySelector('#quick-sheet:not([hidden]) [role=tab]') !== null",
        SHOWN
    ));
}

pub(super) fn task_tab(page: &mut super::Chrome) {
    page.eval("document.querySelector('#quick-sheet [role=tab]').focus()");
    key(page, "ArrowRight", "ArrowRight", 39);
    assert!(page.wait_for("document.activeElement.textContent==='Add task'", SHOWN));
    key(page, "Enter", "Enter", 13);
    assert!(page.wait_for("document.querySelector('#quick-title') !== null && [...document.querySelectorAll('[role=tab]')].find(t=>t.textContent==='Add task')?.getAttribute('aria-selected')==='true'", SHOWN));
}

/// Open the header's Appearance panel from the keyboard, which puts focus on its first action; a
/// panel already open stays open.
pub(super) fn appearance(page: &mut super::Chrome) {
    if page.eval("document.getElementById('appearance-panel') === null") == json!(true) {
        page.eval("document.getElementById('appearance').focus()");
        key(page, "Enter", "Enter", 13);
        assert!(page.wait_for(
            "document.activeElement?.matches('#appearance-panel [data-theme-action=import]')",
            SHOWN
        ));
    }
}

/// Press `action`'s button in the open Appearance panel from the keyboard.
pub(super) fn theme_action(page: &mut super::Chrome, action: &str) {
    page.eval(&format!(
        "document.querySelector('#appearance-panel [data-theme-action={action}]').focus()"
    ));
    key(page, "Enter", "Enter", 13);
}

fn choose_file(page: &mut super::Chrome, name: &str) {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let input = page.call(
        "Runtime.evaluate",
        json!({"expression":"document.querySelector('#theme-file')"}),
    );
    page.call(
        "DOM.setFileInputFiles",
        json!({"files":[file],"objectId":input["result"]["objectId"]}),
    );
}

#[test]
fn an_entry_select_loads_its_popup_on_use_and_keeps_keyboard_selection_and_escape() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    page.viewport(1440, 900, false);
    page.save_data();
    page.open(&site.url("/tasks"));
    assert!(page.wait_for(
        "document.querySelector('button[aria-label=\"Finished tasks\"]') !== null",
        SHOWN
    ));
    assert_eq!(page.eval("window.reviewPanelReady === true"), json!(false));
    board_filters(&mut page);
    super::completed_requests(&mut page);
    page.call("Fetch.enable", json!({"patterns":[{"urlPattern":"*","resourceType":"Stylesheet","requestStage":"Request"}]}));
    page.eval("document.querySelector('button[aria-label=\"Finished tasks\"]').focus()");
    key(&mut page, " ", "Space", 32);
    assert!(page.wait_for(
        "[...document.querySelectorAll('link[rel=stylesheet]')].some(link=>!link.sheet)",
        SHOWN
    ));
    let waiting = page.eval("[...document.querySelectorAll('link[rel=stylesheet]')].filter(link=>!link.sheet).length").as_u64().unwrap() as usize;
    assert!(
        waiting > 0,
        "the first overlay must request its deferred styles"
    );
    let held = page.wait_for_protocol_events("Fetch.requestPaused", waiting, SHOWN);
    if held.len() != waiting {
        let unfinished: Vec<_> = page
            .requests()
            .into_iter()
            .filter(|request| !request.finished)
            .collect();
        panic!(
            "all deferred styles must be held before checking first render: held {:?}, waiting {}, unfinished {unfinished:#?}",
            held.iter()
                .map(|paused| paused["request"]["url"].clone())
                .collect::<Vec<_>>(),
            page.eval("JSON.stringify([...document.querySelectorAll('link[rel=stylesheet]')].filter(link=>!link.sheet).map(link=>link.getAttribute('href')))")
        );
    }
    assert_eq!(
        page.eval(
            "document.querySelector('[role=listbox]') === null && window.reviewOverlayReady !== true"
        ),
        json!(true),
        "an overlay must not render before its styles finish loading"
    );
    key(&mut page, "Escape", "Escape", 27);
    assert!(page.wait_for("document.activeElement.matches('button[aria-label=\"Finished tasks\"]') && document.activeElement.getAttribute('aria-expanded')==='false'", SHOWN));
    key(&mut page, " ", "Space", 32);
    assert!(page.wait_for("document.querySelector('button[aria-label=\"Finished tasks\"]').getAttribute('aria-expanded')==='true'", SHOWN));
    assert_eq!(
        page.eval("document.querySelector('[role=listbox]') === null"),
        json!(true)
    );
    for (index, request) in held.iter().enumerate() {
        if index == 0 {
            page.call(
                "Fetch.failRequest",
                json!({"requestId":request["requestId"],"errorReason":"Failed"}),
            );
        } else {
            page.call(
                "Fetch.continueRequest",
                json!({"requestId":request["requestId"]}),
            );
        }
    }
    assert!(page.wait_for("document.querySelector('#shell-notices #overlay-error[role=alert] > button')?.textContent === 'Retry loading menus'", SHOWN));
    assert_eq!(
        page.eval(
            "document.querySelector('[role=listbox]') === null && window.reviewOverlayReady !== true"
        ),
        json!(true)
    );
    page.eval(
        "document.querySelector('#shell-notices #overlay-error[role=alert] > button').click()",
    );
    let retried = page.wait_for_protocol_events("Fetch.requestPaused", held.len() + 1, SHOWN);
    assert!(
        retried.len() > held.len(),
        "retry must request the failed stylesheet again"
    );
    assert_eq!(
        page.eval("document.querySelector('[role=listbox]') === null"),
        json!(true)
    );
    for request in retried.into_iter().skip(held.len()) {
        page.call(
            "Fetch.continueRequest",
            json!({"requestId":request["requestId"]}),
        );
    }
    page.call("Fetch.disable", json!({}));
    assert!(page.wait_for(
        "document.activeElement.closest('[role=listbox]') !== null",
        SHOWN
    ));
    assert_eq!(
        page.eval("getComputedStyle(document.querySelector('[role=listbox]')).borderRadius"),
        json!("8px")
    );
    assert_eq!(
        page.eval(
            "window.reviewOverlayReady === true && window.reviewPanelReady !== true && document.getElementById('quick-root') === null"
        ),
        json!(true),
        "a select's popup is drawn by the overlay module, never the chat's"
    );
    let chat_script = module(dist(), "panel").script;
    let chat: Vec<_> = page
        .requests()
        .into_iter()
        .filter(|request| request.url.ends_with(&chat_script))
        .collect();
    assert!(
        chat.is_empty(),
        "a select requested the chat's module: {chat:#?}"
    );
    assert_eq!(page.eval("document.querySelector('button[aria-label=\"Finished tasks\"]').getAttribute('aria-controls')===document.querySelector('[role=listbox]').id"), json!(true));
    key(&mut page, "End", "End", 35);
    assert!(page.wait_for("document.activeElement.getAttribute('role')==='option' && document.activeElement.textContent.trim()==='Last week'", SHOWN));
    key(&mut page, "Enter", "Enter", 13);
    assert!(page.wait_for("document.activeElement.matches('button[aria-label=\"Finished tasks\"]') && document.activeElement.getAttribute('aria-expanded')==='false' && document.activeElement.textContent.trim().startsWith('Last week')", SHOWN));
    key(&mut page, "Enter", "Enter", 13);
    assert!(page.wait_for(
        "document.querySelector('[role=listbox][data-state=open]') !== null",
        SHOWN
    ));
    key(&mut page, "Escape", "Escape", 27);
    assert!(page.wait_for("document.activeElement.matches('button[aria-label=\"Finished tasks\"]') && document.activeElement.getAttribute('aria-expanded')==='false'", SHOWN));
    assert!(
        page.eval("document.activeElement.textContent.trim().startsWith('Last week')")
            .as_bool()
            .unwrap()
    );
    key(&mut page, "ArrowDown", "ArrowDown", 40);
    assert!(page.wait_for(
        "document.activeElement.closest('[role=listbox]') !== null",
        SHOWN
    ));
    key(&mut page, "Home", "Home", 36);
    assert!(page.wait_for("document.activeElement.getAttribute('role')==='option' && document.activeElement.textContent.trim()==='Last 24 hours'", SHOWN));
    key(&mut page, "Escape", "Escape", 27);
    assert!(page.wait_for("document.activeElement.matches('button[aria-label=\"Finished tasks\"]') && document.activeElement.getAttribute('aria-expanded')==='false' && document.activeElement.textContent.trim()==='Last week'", SHOWN));
    key(&mut page, "ArrowUp", "ArrowUp", 38);
    assert!(page.wait_for(
        "document.querySelector('[role=listbox][data-state=open]') !== null",
        SHOWN
    ));
    page.eval("(() => { const link=document.createElement('a');link.href='/';document.body.append(link);link.click();link.remove(); })()");
    assert!(page.wait_for("document.querySelector('.hero-title') !== null && document.querySelector('[role=listbox]') === null", SHOWN), "{}", page.eval("({path:location.pathname,navigation:document.documentElement.dataset.navigation,overlay:window.reviewOverlayRequest?.owner,active:document.activeElement.outerHTML.slice(0,500),body:document.body.innerText.slice(0,500),popup:document.querySelector('[role=listbox]')?.outerHTML.slice(0,250)})"));
}

#[test]
fn a_shadcn_theme_file_changes_controls_in_both_modes_survives_navigation_and_resets() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    page.scheme("light");
    // A device's saved theme contains its selector as well as the stored declarations.
    let seed = page.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source":"localStorage.setItem('studio.cssVars', ':root[data-studio-theme]{--radius:23px;}');"}),
    );
    page.open(&site.url("/"));
    page.call(
        "Page.removeScriptToEvaluateOnNewDocument",
        json!({"identifier":seed["identifier"]}),
    );
    assert!(page.wait_for(
        "getComputedStyle(document.documentElement).getPropertyValue('--radius').trim()==='23px'",
        SHOWN
    ), "the stored device theme must apply before importing another theme");
    open_panel(&mut page);
    task_tab(&mut page);
    appearance(&mut page);
    theme_action(&mut page, "import");
    assert!(page.wait_for("document.querySelector('#theme-file') !== null", SHOWN));
    choose_file(&mut page, "shadcn-theme.json");
    assert!(page.wait_for(
        "document.querySelector('.theme-menu [role=status]')?.textContent==='Theme applied.'",
        SHOWN
    ));
    assert_eq!(
        page.eval("({styles:document.querySelectorAll('#studio-theme').length,css:localStorage.getItem('studio.cssVars')===document.querySelector('#studio-theme')?.textContent})"),
        json!({"styles":1,"css":true})
    );
    assert!(page.wait_for("(() => { const style=getComputedStyle(document.querySelector('#quick-sheet button[type=submit]'));return style.backgroundColor==='rgb(201, 93, 34)' && style.borderRadius==='16px' })()", SHOWN), "theme transitions must finish on the button");
    assert_eq!(page.eval("(() => { const field=getComputedStyle(document.querySelector('#quick-title')),button=getComputedStyle(document.querySelector('#quick-sheet button[type=submit]')); return {color:field.color,radius:field.borderRadius,font:field.fontFamily,primary:button.backgroundColor,buttonRadius:button.borderRadius,like:getComputedStyle(document.documentElement).getPropertyValue('--like').trim()}; })()"), json!({"color":"rgb(18, 52, 86)","radius":"16px","font":"Georgia, serif","primary":"rgb(201, 93, 34)","buttonRadius":"16px","like":"#2e7d4f"}));

    appearance(&mut page);
    theme_action(&mut page, "import");
    assert!(page.wait_for("document.querySelector('#theme-file') !== null", SHOWN));
    choose_file(&mut page, "invalid-theme.json");
    assert!(page.wait_for("document.querySelector('.theme-menu [role=status]')?.textContent.includes('Invalid theme token')", SHOWN));
    assert_eq!(
        page.eval("getComputedStyle(document.querySelector('#quick-title')).color"),
        json!("rgb(18, 52, 86)")
    );

    page.open(&site.url("/tasks"));
    assert!(page.wait_for("document.querySelector('.filters input') !== null", SHOWN));
    assert_eq!(
        page.eval("getComputedStyle(document.querySelector('.filters input')).borderRadius"),
        json!("16px")
    );
    page.scheme("dark");
    assert_eq!(
        page.eval("getComputedStyle(document.querySelector('.filters input')).color"),
        json!("rgb(217, 230, 242)")
    );
    assert_eq!(
        page.eval(
            "getComputedStyle(document.documentElement).getPropertyValue('--primary').trim()"
        ),
        json!("#ffb875")
    );
    appearance(&mut page);
    theme_action(&mut page, "default");
    assert!(page.wait_for("document.querySelector('.theme-menu [role=status]')?.textContent==='Default theme restored.'", SHOWN));
    assert_eq!(
        page.eval("getComputedStyle(document.querySelector('.filters input')).borderRadius"),
        json!("8px")
    );
    assert_eq!(
        page.eval(
            "getComputedStyle(document.documentElement).getPropertyValue('--primary').trim()"
        ),
        json!("#7fa7e0")
    );
}

#[test]
fn buttons_tabs_selects_menus_and_tooltips_keep_keyboard_focus_and_drafts() {
    let site = site();
    let Some(mut page) = chrome() else { return };
    page.open(&site.url("/"));
    open_panel(&mut page);
    task_tab(&mut page);
    page.eval("document.querySelector('#quick-title').focus()");
    assert_eq!(page.eval("(() => {const style=getComputedStyle(document.activeElement);return [style.outlineStyle,style.outlineWidth]})()"), json!(["solid", "2px"]));
    page.call("Input.insertText", json!({"text":"Keyboard task"}));
    page.eval("document.querySelector('#quick-text').focus()");
    page.call("Input.insertText", json!({"text":"First"}));
    key(&mut page, "Enter", "Enter", 13);
    page.call("Input.insertText", json!({"text":"Second"}));
    assert_eq!(
        page.eval("document.querySelector('#quick-text').value"),
        json!("First\nSecond")
    );
    assert!(
        site.posts().is_empty(),
        "Enter in the text area must not submit the task"
    );
    page.eval("document.querySelector('#quick-sheet [role=tab][aria-selected=true]').focus()");
    key(&mut page, "Home", "Home", 36);
    assert!(page.wait_for("document.activeElement.textContent==='Chat'", SHOWN));
    assert_eq!(
        page.eval("document.querySelector('#quick-title').value"),
        json!("Keyboard task")
    );
    key(&mut page, "Enter", "Enter", 13);
    assert!(page.wait_for("document.querySelector('#quick-title') === null && document.activeElement.textContent==='Chat'", SHOWN));
    key(&mut page, "End", "End", 35);
    assert!(page.wait_for("document.activeElement.textContent==='Add task'", SHOWN));
    key(&mut page, "Enter", "Enter", 13);
    assert!(page.wait_for(
        "document.querySelector('#quick-title')?.value==='Keyboard task'",
        SHOWN
    ));
    assert_eq!(
        page.eval("document.querySelector('#quick-text').value"),
        json!("First\nSecond")
    );
    assert_eq!(page.eval("document.querySelector('[role=tabpanel]:not([hidden])').getAttribute('aria-labelledby')===document.activeElement.id"), json!(true));
    assert!(page.wait_for("[...document.querySelectorAll('[role=tab]')].every(tab=>document.getElementById(tab.getAttribute('aria-controls'))?.getAttribute('role')==='tabpanel')", SHOWN));

    page.eval("document.querySelector('#quick-sheet button[aria-haspopup=listbox]').focus()");
    key(&mut page, " ", "Space", 32);
    assert!(page.wait_for(
        "document.querySelectorAll('[role=listbox] [role=option]').length===5",
        SHOWN
    ));
    assert_eq!(page.eval("[...document.querySelectorAll('[role=listbox] [role=option]')].map(e=>e.textContent.trim())"), json!(["p0","p1","p2","p3","p4"]));
    assert_eq!(page.eval("document.querySelector('#quick-sheet button[aria-haspopup=listbox]').getAttribute('aria-controls')===document.querySelector('[role=listbox]').id"), json!(true));
    assert!(
        page.wait_for(
            "document.activeElement.closest('[role=listbox]') !== null",
            SHOWN
        ),
        "the opened select must receive keyboard focus: {}",
        page.eval("document.activeElement.outerHTML")
    );
    key(&mut page, "End", "End", 35);
    assert!(page.wait_for("document.activeElement.getAttribute('role')==='option' && document.activeElement.textContent.trim()==='p4'", SHOWN));
    key(&mut page, "Enter", "Enter", 13);
    assert!(page.wait_for("document.querySelector('#quick-sheet button[aria-haspopup=listbox]')?.textContent.trim()==='p4' && document.querySelector('#quick-sheet button[aria-haspopup=listbox]')?.getAttribute('aria-expanded')==='false' && document.querySelector('#quick-sheet button[aria-haspopup=listbox]')===document.activeElement", SHOWN), "{}", page.eval("({active:document.activeElement.outerHTML,select:document.querySelector('#quick-sheet button[aria-haspopup=listbox]').outerHTML})"));
    key(&mut page, "Enter", "Enter", 13);
    assert!(page.wait_for(
        "document.querySelector('[role=listbox][data-state=open]') !== null",
        SHOWN
    ));
    assert!(page.wait_for("document.querySelector('[role=listbox] [role=option][aria-selected=true]')?.textContent.trim()==='p4'", SHOWN));
    key(&mut page, "Escape", "Escape", 27);
    assert!(page.wait_for("document.querySelector('#quick-sheet button[aria-haspopup=listbox]')?.getAttribute('aria-expanded')==='false' && document.activeElement.matches('#quick-sheet button[aria-haspopup=listbox]')", SHOWN), "{}", page.eval("({active:document.activeElement.outerHTML.slice(0,500),trigger:document.querySelector('#quick-sheet button[aria-haspopup=listbox]').outerHTML,popup:document.querySelector('[role=listbox]')?.outerHTML.slice(0,500)})"));

    // The header's Appearance panel takes focus when opened and gives it back on Escape.
    appearance(&mut page);
    key(&mut page, "Escape", "Escape", 27);
    assert!(page.wait_for(
        "document.getElementById('appearance-panel') === null && document.activeElement.matches('#appearance') && document.getElementById('appearance').getAttribute('aria-expanded') === 'false'",
        SHOWN
    ));

    page.eval("document.querySelector('#quick-sheet .collapse').focus()");
    assert!(page.wait_for("document.querySelector('[role=tooltip]')?.textContent==='Collapse panel; keep the conversation'", SHOWN));
    key(&mut page, "Escape", "Escape", 27);
    assert!(page.wait_for("document.querySelector('[role=tooltip]') === null", SHOWN));
    assert_eq!(
        page.eval("document.querySelector('#quick-title').value"),
        json!("Keyboard task")
    );
}
