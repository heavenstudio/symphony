//! The phone budget: on an emulated mid phone over a tailnet link the board is interactive within
//! 2 s of wall time, and a board answered later misses it whatever the machine's load.
use super::{SHOWN, chrome, chrome::Chrome, site, site::Site};
use serde_json::json;
use std::time::Duration;

const PHONE_MS: f64 = 2_000.0;

/// The time the phone budget holds: the median wall of `rounds` first visits on the emulated mid
/// phone, in ms, with each round's.
fn phone_ms(site: &Site, chrome: &mut Chrome, rounds: usize) -> (f64, Vec<f64>) {
    chrome.mid_phone();
    chrome.call("Network.setCacheDisabled", json!({"cacheDisabled": true}));
    let mut seen = Vec::new();
    for _ in 0..rounds {
        chrome.open(&site.url("/"));
        let ready = chrome.eval(&format!(
            "Promise.race([new Promise(r => {{ const look = () => {{ const e = performance.getEntriesByName('board-ready')[0]; \
             e ? r(e.startTime) : setTimeout(look, 20) }}; look() }}), new Promise(r => setTimeout(() => r(null), {}))])",
            SHOWN.as_millis()
        ));
        let Some(wall) = ready.as_f64() else {
            panic!(
                "the board never marked itself ready on the emulated phone (performance mark `board-ready`)"
            );
        };
        seen.push(wall);
        chrome.open("about:blank");
    }
    let mut sorted = seen.clone();
    sorted.sort_by(f64::total_cmp);
    (sorted[sorted.len() / 2], seen)
}

#[test]
fn the_board_is_interactive_within_two_seconds_on_a_mid_phone() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    let (median, rounds) = phone_ms(&site, &mut chrome, 3);
    eprintln!("phone rounds (wall ms): {rounds:?}; median {median:.0} ms");
    assert!(
        median <= PHONE_MS,
        "the board is interactive after {median:.0} ms, over {PHONE_MS} ms: {rounds:?}"
    );
}

#[test]
fn a_board_answered_after_two_and_a_half_seconds_misses_the_phone_budget() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    site.delay_board(Duration::from_millis(2_500));
    let (ms, rounds) = phone_ms(&site, &mut chrome, 1);
    assert!(
        ms > PHONE_MS,
        "a board answered after 2.5 s measures {ms:.0} ms, within the {PHONE_MS} ms budget: {rounds:?}"
    );
}
