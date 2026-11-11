//! The owner's polish findings (app/ui-polish-1), each read in the release client: popups draw on
//! an opaque surface of bounded height, the chat panel is one surface whose Send stays in view,
//! missing pages say so, panels answer the keyboard, badges stay readable and native controls
//! follow the scheme, in both schemes.
use super::components::{appearance, key, open_panel, site};
use super::{SHOWN, chrome, chrome::Chrome};
use serde_json::{Value, json};

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
