//! Pictures as the owner picks them: the home hero's three candidates share one row on a desktop and
//! stack in one column on a phone, where the hero's 320px content box holds a single 160px-minimum
//! track; a gallery's picker has one card border and a visible keyboard focus ring.
use super::{DOC, PICTURE, ParitySite, painted_layout, report_styles, track_styles};
use crate::{SHOWN, chrome};
use serde_json::json;

#[test]
fn the_heros_candidates_share_a_row_on_a_desktop_and_stack_on_a_phone() {
    let site = ParitySite::start();
    let Some(mut browser) = chrome() else { return };
    for width in [1280, 390] {
        browser.viewport(width, 900, width < 400);
        browser.open(&site.url("/"));
        assert!(browser.wait_for(
            "document.querySelectorAll('.hero .pics figure').length === 3",
            SHOWN
        ));
        painted_layout(&mut browser);
        let grid = browser.eval("(() => {const pics=document.querySelector('.hero .pics'), box=pics.getBoundingClientRect(), figures=[...pics.querySelectorAll('figure')].map(e=>e.getBoundingClientRect()); return {width:box.width, tops:new Set(figures.map(r=>r.top)).size, lefts:new Set(figures.map(r=>r.left)).size, full:figures.every(r=>r.width === box.width), narrowest:Math.min(...figures.map(r=>r.width)), overflow:document.documentElement.scrollWidth > innerWidth};})()");
        assert_eq!(grid["overflow"], json!(false), "at {width}px: {grid}");
        if width == 1280 {
            assert_eq!(
                (&grid["tops"], &grid["lefts"]),
                (&json!(1), &json!(3)),
                "three columns on one row: {grid}"
            );
            assert!(
                grid["narrowest"].as_f64().unwrap() >= 160.0,
                "no track under its minimum: {grid}"
            );
        } else {
            assert_eq!(grid["width"], json!(320), "the hero's content box: {grid}");
            assert_eq!(
                (&grid["tops"], &grid["lefts"], &grid["full"]),
                (&json!(3), &json!(1), &json!(true)),
                "one column of full-width candidates: {grid}"
            );
        }
    }
}

#[test]
fn the_lamp_picker_has_one_card_border_and_a_visible_keyboard_focus_ring() {
    let site = ParitySite::start();
    let Some(mut browser) = chrome() else { return };
    track_styles(&mut browser);
    browser.open(&site.url(&format!("/{DOC}#L30")));
    assert!(browser.wait_for("document.querySelector('.gallery-pick') !== null", SHOWN));
    assert!(browser.wait_for(
        "document.querySelector('.gallery-pick img')?.naturalWidth === 600",
        SHOWN
    ));
    painted_layout(&mut browser);
    assert_eq!(
        browser.eval("document.querySelector('.gallery-pick img').getAttribute('src')"),
        json!(PICTURE),
        "use the supplied picture thumbnail"
    );
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.gallery-card')).borderTopWidth"),
        json!("1px")
    );
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.gallery-card')).borderRadius"),
        json!("14px")
    );
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.gallery-pick')).borderTopWidth"),
        json!("0px")
    );
    for mode in ["light", "dark"] {
        browser.scheme(mode);
        painted_layout(&mut browser);
        assert_eq!(browser.eval("(() => {const pick=document.querySelector('.gallery-pick'), style=getComputedStyle(pick), card=getComputedStyle(pick.parentElement), image=pick.querySelector('img').getBoundingClientRect(), outer=pick.parentElement.getBoundingClientRect();return style.borderTopWidth === '0px' && style.boxShadow === 'none' && Math.abs(image.left-outer.left-parseFloat(card.borderLeftWidth)-parseFloat(card.paddingLeft)) <= 1 && image.width === 80 && image.height === 80;})()"), json!(true), "{mode}: the thumbnail stays left aligned inside the card's single border");
    }
    assert!(browser.wait_for(&crate::visible(".gallery-pick"), SHOWN));
    browser.eval("document.querySelector('.gallery-pick').focus()");
    assert_eq!(
        browser.eval("getComputedStyle(document.querySelector('.gallery-pick')).outlineWidth"),
        json!("2px")
    );
    report_styles(&mut browser, "lamp picker and keyboard focus");
}
