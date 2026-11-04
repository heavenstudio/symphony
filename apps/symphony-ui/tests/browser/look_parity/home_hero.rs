//! The home hero's pictures: a card's three candidates share one row on a desktop and stack in one
//! column on a phone, where the hero's 320px content box holds a single 160px-minimum track.
use super::{ParitySite, painted_layout};
use crate::{SHOWN, chrome};
use serde_json::json;

#[test]
fn the_heros_candidates_share_a_row_on_a_desktop_and_stack_on_a_phone() {
    let site = ParitySite::start();
    let Some(mut browser) = chrome() else { return };
    for width in [1280, 390] {
        browser.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":width,"height":900,"deviceScaleFactor":1,"mobile":width < 400}),
        );
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
