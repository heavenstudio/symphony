//! The comments held on documents in the review queue and on Home (apps/symphony-web/src/queue.tsx,
//! home.tsx; docs/product/analysis/users-and-identity.md, card `held-on-document-waits-where`, B):
//! the release client against the stand-in server answering two readers as the server shows each
//! (`apps/symphony-server/src/live/held.rs`, tested there): ana, whom two comments wait on to pass
//! on, and bo, whom none does. Her queue lists them right after what waits on her, each leading to
//! its box on its document's page and saying who wrote it, with no Pass on; her Home counts them
//! under the greeting, leading to that group. His queue and Home draw as they would with none, and
//! say nothing of them. Each on a desk and a phone, within the screen's width. Once the feed says
//! a held comment moved, the queue reads itself again and draws what it now holds.
//!
//! With `SYMPHONY_HELD_QUEUE_SHOTS` naming a folder, each page is saved there, whole, wide in light
//! and on a phone in dark: a visitor who is not the person at the machine reaches a server only
//! under another Tailscale login, so their page is pictured from the stand-in server.
use super::{SHOWN, STALL, chrome, chrome::Chrome, common, site::Site};
use serde_json::{Value, json};

/// The desk's screen in light and the phone's in dark, by the names their pictures take.
const SCREENS: [Screen; 2] = [
    ("wide", 1440, 900, false, "light"),
    ("phone", 390, 844, true, "dark"),
];

type Screen = (&'static str, u32, u32, bool, &'static str);

/// Who wrote the held comments: a name no other part of the fixture holds.
const AUTHOR: &str = "marisol";

/// The comments held on documents that wait on ana, as the queue lists them: one on main's page,
/// one on a branch's.
fn held() -> Value {
    json!([
        {"id": 4, "href": "/docs/lamps/north.md#held-4", "doc_title": "North lamp",
         "author": AUTHOR},
        {"id": 7, "href": "/@proposal~lamps/docs/game/MINING.md#held-7", "doc_title": "Mining",
         "author": AUTHOR},
    ])
}

/// The board fixture, and the queue of its first cards, as the server shows a reader whom the
/// comments `held` wait on.
fn answers(held: &Value) -> (Value, Value) {
    let mut board: Value = serde_json::from_str(&common::fixture("board.json")).unwrap();
    let count = held.as_array().unwrap().len();
    if count > 0 {
        board["counts"]["held"] = json!(count);
    }
    let mut queue = json!({"waiting": board["next"], "spot_check": [], "agent": [],
                           "branches": []});
    if count > 0 {
        queue["held"] = held.clone();
    }
    (board, queue)
}

/// The stand-in serving a reader whom the comments `held` wait on.
fn site_for(held: &Value) -> Site {
    let (board, queue) = answers(held);
    let site = super::site();
    site.set_board(&board.to_string());
    site.set_json("/api/queue", &queue);
    site.set_json("/api/postponed", &json!([]));
    site
}

fn screen(chrome: &mut Chrome, width: u32, height: u32, mobile: bool) {
    chrome.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":if mobile { 3 } else { 1 },"mobile":mobile}),
    );
}

/// The whole page, its window made as tall as it is, in `scheme`, saved as
/// `NAME-SCREEN-SCHEME.png` in the folder `SYMPHONY_HELD_QUEUE_SHOTS` names; nothing without one.
/// The window is put back as it was.
fn shot(chrome: &mut Chrome, name: &str, (screen_name, width, height, mobile, scheme): Screen) {
    let Some(folder) = std::env::var_os("SYMPHONY_HELD_QUEUE_SHOTS") else {
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

/// The queue as drawn: its groups in their order, each held comment's link, title and author and
/// the buttons by it, and whether the page stays within the screen's width.
fn queue(chrome: &mut Chrome) -> Value {
    chrome.reveal("#agent");
    chrome.eval(
        r#"(() => ({
            groups: [...document.querySelectorAll('.queue-page section.queue-group')].map(s => s.id),
            held: [...document.querySelectorAll('#held [data-held]')].map(li => {
                const a = li.querySelector('a');
                return {id: li.dataset.held, to: new URL(a.href).pathname + new URL(a.href).hash,
                        title: a.textContent, author: li.querySelector('.muted').textContent};
            }),
            buttons: document.querySelectorAll('#held button').length,
            heading: document.querySelector('#held h2')?.textContent ?? null,
            within: document.documentElement.scrollWidth <= innerWidth,
        }))()"#,
    )
}

/// Home as drawn: the line under the greeting, where its link leads, the hero's title, and
/// whether the page stays within the screen's width.
fn home(chrome: &mut Chrome) -> Value {
    chrome.eval(
        r#"(() => {
            const link = document.querySelector('.head-state a');
            return {
                lead: document.querySelector('.head-state').textContent,
                to: link ? new URL(link.href).pathname + new URL(link.href).hash : null,
                hero: document.querySelector('.hero-title').textContent,
                within: document.documentElement.scrollWidth <= innerWidth,
            };
        })()"#,
    )
}

#[test]
fn the_held_comments_are_listed_and_counted_for_the_reader_they_wait_on_and_for_no_other() {
    let Some(mut chrome) = chrome() else { return };
    for (reader, held) in [("ana", held()), ("bo", json!([]))] {
        let site = site_for(&held);
        let theirs = !held.as_array().unwrap().is_empty();
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
            let drawn = queue(&mut chrome);
            if theirs {
                assert_eq!(
                    drawn,
                    json!({
                        "groups": ["you", "held", "spot", "agent"],
                        "held": [
                            {"id": "4", "to": "/docs/lamps/north.md#held-4", "title": "North lamp",
                             "author": AUTHOR},
                            {"id": "7", "to": "/@proposal~lamps/docs/game/MINING.md#held-7",
                             "title": "Mining", "author": AUTHOR},
                        ],
                        "buttons": 0,
                        "heading": "Held comments (2)",
                        "within": true,
                    }),
                    "{reader}, {width}: the queue"
                );
            } else {
                assert_eq!(
                    drawn,
                    json!({"groups": ["you", "spot", "agent"], "held": [], "buttons": 0,
                           "heading": null, "within": true}),
                    "{reader}, {width}: the queue"
                );
                let text = chrome.eval("document.body.innerText").to_string();
                assert!(!text.contains(AUTHOR), "{reader}, {width}: {text}");
            }
            shot(&mut chrome, &format!("queue-{reader}"), at);

            open(&mut chrome, &site, "/", "[data-ready=board] .hero-title");
            let lead = "37 cards wait on you across 9 documents.";
            let expected = if theirs {
                json!({"lead": format!("{lead} 2 held comments to pass on."), "to": "/queue#held",
                       "hero": "North · pendant picture", "within": true})
            } else {
                json!({"lead": lead, "to": null, "hero": "North · pendant picture",
                       "within": true})
            };
            assert_eq!(home(&mut chrome), expected, "{reader}, {width}: Home");
            let text = chrome.eval("document.body.innerText").to_string();
            assert_eq!(text.contains("held comment"), theirs, "{reader}, {width}");
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

#[test]
fn the_queue_reads_itself_again_once_the_feed_says_a_held_comment_moved() {
    let Some(mut chrome) = chrome() else { return };
    let site = site_for(&json!([]));
    open(
        &mut chrome,
        &site,
        "/queue",
        "[data-ready=queue] [data-card]",
    );
    assert_eq!(
        queue(&mut chrome)["groups"],
        json!(["you", "spot", "agent"])
    );
    test_time::until_true("the queue's feed open", STALL, || site.feeds() > 0)
        .expect("the queue never opened its feed");
    let read = site.answers("/api/queue").len();
    site.set_json("/api/queue", &answers(&held()).1);
    let version = serde_json::from_str::<Value>(&common::fixture("board.json")).unwrap()["version"]
        .as_u64()
        .unwrap()
        + 1;
    site.push(
        version,
        &json!({"version": version, "changes": [{"region": "held", "data": {}}]}).to_string(),
    );
    assert!(
        chrome.wait_for(
            "document.querySelectorAll('#held [data-held]').length === 2",
            SHOWN
        ),
        "the held comments never drawn: {}",
        chrome.eval("document.body.innerText.slice(0, 1500)")
    );
    assert!(site.answers("/api/queue").len() > read, "read again");
    assert_eq!(
        queue(&mut chrome)["groups"],
        json!(["you", "held", "spot", "agent"])
    );
}
