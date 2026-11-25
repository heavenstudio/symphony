//! What the queue page and Home draw for two readers a board names, each of a group that holds one
//! folder of the board fixture's waiting cards (docs/workflow/TASKS.md, "People"): the release
//! client against the stand-in server answering each with what the server shows them (the filter
//! of `symphony::owners::theirs`, tested in the server): ana the cards of `docs/lamps/` and
//! `docs/makers/`, bo those of `docs/game/`. Each reader's queue lists their cards alone, each
//! with its Later; Home leads with their first, lists their next and counts what waits on them;
//! and no page names a card of the other's, on a desk or a phone.
//!
//! With `SYMPHONY_QUEUES_SHOTS` naming a folder, each page is saved there, whole, wide in light
//! and on a phone in dark: a visitor who is not the person at the machine reaches a server only
//! under another Tailscale login, so their page is pictured from the stand-in server.
use super::{SHOWN, chrome, chrome::Chrome, common, site::Site};
use serde_json::{Value, json};

/// The desk's screen in light and the phone's in dark, by the names their pictures take.
const SCREENS: [(&str, u32, u32, bool, &str); 2] = [
    ("wide", 1440, 900, false, "light"),
    ("phone", 390, 844, true, "dark"),
];

/// Each reader and the folders of the documents whose cards they decide.
const READERS: [(&str, &[&str]); 2] = [
    ("ana", &["docs/lamps/", "docs/makers/"]),
    ("bo", &["docs/game/"]),
];

fn screen(chrome: &mut Chrome, width: u32, height: u32, mobile: bool) {
    chrome.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":if mobile { 3 } else { 1 },"mobile":mobile}),
    );
}

/// The whole page, its window made as tall as it is, in `scheme`, saved as
/// `NAME-SCREEN-SCHEME.png` in the folder `SYMPHONY_QUEUES_SHOTS` names; nothing without one. The
/// window and the scheme are put back as they were.
fn shot(chrome: &mut Chrome, name: &str, (screen_name, width, height, mobile, scheme): Screen) {
    let Some(folder) = std::env::var_os("SYMPHONY_QUEUES_SHOTS") else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    let tall = chrome
        .eval("Math.ceil(document.documentElement.scrollHeight)")
        .as_u64()
        .unwrap() as u32;
    screen(chrome, width, tall.max(height), mobile);
    chrome.frames_with_fonts();
    chrome.wait_for("document.getAnimations().length === 0", SHOWN);
    std::fs::write(
        folder.join(format!("{name}-{screen_name}-{scheme}.png")),
        chrome.screenshot(),
    )
    .unwrap();
    screen(chrome, width, height, mobile);
}

type Screen = (&'static str, u32, u32, bool, &'static str);

/// Whether the card `card` is in one of the folders `folders`.
fn of(card: &Value, folders: &[&str]) -> bool {
    let doc = card["doc"].as_str().unwrap();
    folders.iter().any(|folder| doc.starts_with(folder))
}

/// The board fixture as the server shows a reader of `folders`: its first cards theirs, its counts
/// of what waits on them, everything else as it is.
fn board(folders: &[&str]) -> (Value, Vec<Value>) {
    let mut board: Value = serde_json::from_str(&common::fixture("board.json")).unwrap();
    let theirs: Vec<Value> = (board["next"].as_array().unwrap().iter())
        .filter(|card| of(card, folders))
        .cloned()
        .collect();
    let docs: std::collections::BTreeSet<&str> =
        theirs.iter().map(|c| c["doc"].as_str().unwrap()).collect();
    board["next"] = json!(theirs);
    board["counts"]["waiting"] = json!(theirs.len());
    board["counts"]["docs_waiting"] = json!(docs.len());
    board["counts"]["spot_check"] = json!(0);
    board["progress"]["decisions_waiting"] = json!(theirs.len());
    (board, theirs)
}

/// The stand-in serving a reader of `folders` their board and their queue.
fn site_for(folders: &[&str]) -> (Site, Vec<Value>) {
    let (board, theirs) = board(folders);
    let site = super::site();
    site.set_board(&board.to_string());
    site.set_json(
        "/api/queue",
        &json!({"waiting": theirs, "spot_check": [], "agent": [], "branches": []}),
    );
    site.set_json("/api/postponed", &json!([]));
    (site, theirs)
}

fn open(chrome: &mut Chrome, site: &Site, path: &str, ready: &str) {
    chrome.open(&site.url(path));
    assert!(
        chrome.wait_for(
            &format!(
                "document.documentElement.dataset.navigation === 'ready' && document.querySelector({}) !== null && document.fonts.status === 'loaded'",
                json!(ready)
            ),
            SHOWN
        ),
        "{path}: {}",
        chrome.eval("document.body ? document.body.innerText.slice(0, 1000) : '(no body)'")
    );
    chrome.frames_with_fonts();
}

/// The slug of each card the page links to, in its order, once each.
fn linked(chrome: &mut Chrome) -> Vec<String> {
    let linked = chrome.eval(
        r##"[...new Set([...document.querySelectorAll('main a[href*="#"]')]
            .map(a => new URL(a.href).hash.slice(1)).filter(Boolean))]"##,
    );
    serde_json::from_value(linked).unwrap()
}

#[test]
fn each_readers_queue_and_home_draw_the_cards_they_decide_and_none_of_anothers() {
    let Some(mut chrome) = chrome() else { return };
    let every: Vec<Value> = board(&["docs/"]).1;
    for (reader, folders) in READERS {
        let (site, theirs) = site_for(folders);
        let slugs: Vec<&str> = theirs.iter().map(|c| c["slug"].as_str().unwrap()).collect();
        let others: Vec<(&str, &str)> = (every.iter())
            .filter(|card| !of(card, folders))
            .map(|card| {
                (
                    card["slug"].as_str().unwrap(),
                    card["title"].as_str().unwrap(),
                )
            })
            .collect();
        assert!(!slugs.is_empty() && !others.is_empty(), "{reader}");
        for at in SCREENS {
            let (_, width, height, mobile, scheme) = at;
            screen(&mut chrome, width, height, mobile);
            chrome.scheme(scheme);
            open(
                &mut chrome,
                &site,
                "/queue",
                "[data-ready=queue] [data-card]",
            );
            let queued = chrome.eval(
                r#"(() => ({
                    cards: [...document.querySelectorAll('#you [data-card]')]
                        .map(li => li.dataset.card.split('#')[1]),
                    later: [...document.querySelectorAll('#you [data-card]')]
                        .every(li => [...li.querySelectorAll('button')].some(b => b.textContent === 'Later')),
                    within: document.documentElement.scrollWidth <= innerWidth,
                    fault: /could not be read|server answered/.test(document.body.innerText),
                }))()"#,
            );
            assert_eq!(
                queued,
                json!({"cards": slugs, "later": true, "within": true, "fault": false}),
                "{reader}, {width}: the queue"
            );
            let (text, links) = (
                chrome.eval("document.body.innerText").to_string(),
                linked(&mut chrome),
            );
            for (slug, title) in &others {
                assert!(!links.iter().any(|s| s == slug), "{reader}: {slug}");
                assert!(
                    !text.contains(title),
                    "{reader}, {width}: {title} on the queue"
                );
            }
            shot(&mut chrome, &format!("queue-{reader}"), at);

            open(&mut chrome, &site, "/", "[data-ready=board] .hero-title");
            let home = chrome.eval(
                r#"(() => ({
                    lead: document.querySelector('.head-state').textContent,
                    hero: document.querySelector('.hero-title').textContent,
                    tile: document.querySelector('.tiles .tone-you b')?.textContent ?? null,
                    within: document.documentElement.scrollWidth <= innerWidth,
                }))()"#,
            );
            // Each of the fixture's cards is of a document of its own, and each reader has more than one.
            let lead = format!("{0} cards wait on you across {0} documents.", theirs.len());
            assert_eq!(
                home,
                json!({"lead": lead, "hero": theirs[0]["title"], "tile": theirs.len().to_string(),
                       "within": true}),
                "{reader}, {width}: Home"
            );
            let home_links = linked(&mut chrome);
            for slug in &slugs {
                assert!(
                    home_links.iter().any(|s| s == slug),
                    "{reader}: {slug} on Home"
                );
            }
            let text = chrome.eval("document.body.innerText").to_string();
            for (slug, title) in &others {
                assert!(
                    !home_links.iter().any(|s| s == slug),
                    "{reader}: {slug} on Home"
                );
                assert!(!text.contains(title), "{reader}, {width}: {title} on Home");
            }
            shot(&mut chrome, &format!("home-{reader}"), at);
        }
        chrome.call("Emulation.setEmulatedMedia", json!({"features": []}));
        assert!(
            (site.posts().iter()).all(|(route, _)| route == "/api/postpone"),
            "nothing but a visit sent on opening: {:?}",
            site.posts()
        );
    }
}
