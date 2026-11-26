//! Comments held for people on a document's page in the release client
//! (apps/symphony-web/src/document-held.tsx), against the stand-in server: for a reader who may
//! not direct agents, a comment of theirs answered as held says so, and once the feed says one was
//! held the page reads its held comments again and draws each where it was left, a callout under
//! its section and a card's message in its card, its author's name, kind, the state `held` and its
//! words, with no Pass on. For one who may pass them on, Pass on posts the pass once, and the page's
//! next read leaves the comment out. A callout whose paragraph changed while the page held it is
//! read again as the page's copy moves and drawn where the server says its pass writes it now, at
//! the end of its section, and passed on there. On a desk and a phone, each is drawn within the
//! screen's width. The document is the card probe's.
//!
//! With `SYMPHONY_HELD_SHOTS` naming a folder, the page is saved there at each width and in each
//! scheme, held for ana and offered to a passer: a reader who is not the person at the machine
//! reaches a server only under another Tailscale login, so it is pictured from the stand-in server.
use super::{SHOWN, chrome, chrome::Chrome, document_cards, site::Site};
use serde_json::{Value, json};

const PAGE: &str = "/docs/PROBE.md";
const WORDS: &str = "Too many lamps\nShow the thirty that matter.";

/// ana's question held after the probe's paragraph, and her message held on its card `alpha`.
fn callout() -> Value {
    json!({"id": 4, "author": "ana", "kind": "question", "text": WORDS, "key": "intro", "line": 3})
}

/// ana's question once the paragraph it was left after changed: at the end of its section, after
/// the last block under the probe's one heading, its decision callout.
fn at_its_sections_end() -> Value {
    json!({"id": 4, "author": "ana", "kind": "question", "text": WORDS, "key": "decision",
        "line": 15})
}

fn message() -> Value {
    json!({"id": 5, "author": "ana", "kind": "message", "text": "And why ninety?", "card": "alpha"})
}

/// The probe's document, `held` its held comments, the reader passing them on where `passes`.
fn served(held: &[Value], passes: bool) -> Site {
    let site = document_cards::probe_site();
    site.set_json("/api/postponed", &json!([]));
    holding(&site, held, passes);
    site.set_json("/api/comment", &json!({"ok": true, "held": 4}));
    site.set_json(symphony_api::DOCUMENT_PASS_ROUTE, &json!({"ok": true}));
    site
}

/// `site`'s held comments read as `held` from now on, the reader passing them on where `passes`.
fn holding(site: &Site, held: &[Value], passes: bool) {
    let read = json!({"held": held, "passes": passes});
    site.set_json(symphony_api::DOCUMENT_HELD_PATH, &read);
}

/// Each held comment as drawn: where it is, by its section's key, its author, kind, state, words
/// and buttons, and whether it and the page stay within the screen's width.
fn drawn(page: &mut Chrome) -> Value {
    page.eval(
        r#"(() => [...document.querySelectorAll('.held-comment')].map(held => {
            const box = held.getBoundingClientRect();
            return {
                id: held.id,
                under: held.closest('[data-doc-key]')?.dataset.docKey ?? null,
                author: held.querySelector('.held-head > b').textContent,
                kind: held.querySelector('.held-kind')?.textContent ?? null,
                mark: held.querySelector('.entry-held').textContent,
                words: held.querySelector('.held-text').textContent,
                buttons: [...held.querySelectorAll('button')].map(b => b.textContent),
                within: box.right <= innerWidth + 0.5 && document.documentElement.scrollWidth <= innerWidth,
            };
        }))()"#,
    )
}

/// The page's held comments once `count` are drawn.
fn shown(page: &mut Chrome, count: usize) -> Value {
    page.reveal("[data-doc-key=\"card:alpha\"]");
    assert!(
        page.wait_for(
            &format!("document.querySelectorAll('.held-comment').length === {count}"),
            SHOWN
        ),
        "{count} held comments never drawn: {}",
        page.eval("document.body.innerText.slice(0, 1500)")
    );
    page.frames_with_fonts();
    drawn(page)
}

/// ana's callout and message as drawn, Pass on with them where `passes`.
fn both(passes: bool) -> Value {
    let buttons = if passes {
        json!(["Pass on"])
    } else {
        json!([])
    };
    json!([
        {"id": "held-4", "under": "intro", "author": "ana", "kind": "question", "mark": "held",
         "words": WORDS, "buttons": buttons, "within": true},
        {"id": "held-5", "under": "card:alpha", "author": "ana", "kind": null, "mark": "held",
         "words": "And why ninety?", "buttons": buttons, "within": true},
    ])
}

/// The desk's and the phone's screens.
const SCREENS: [(u32, u32, bool); 2] = [(1440, 900, false), (390, 844, true)];

/// The screen with the held callout in its middle, saved as `NAME-WIDTH-light.png` and
/// `NAME-WIDTH-dark.png` in the folder `SYMPHONY_HELD_SHOTS` names; nothing without one. The
/// scheme is put back as it was.
fn shot(page: &mut Chrome, name: &str, width: u32) {
    let Some(folder) = std::env::var_os("SYMPHONY_HELD_SHOTS") else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    page.eval("document.querySelector('#held-4').scrollIntoView({block: 'start'})");
    for scheme in ["light", "dark"] {
        page.scheme(scheme);
        page.frames_with_fonts();
        page.wait_for("document.getAnimations().length === 0", SHOWN);
        let file = folder.join(format!("{name}-{width}-{scheme}.png"));
        std::fs::write(file, page.screenshot()).unwrap();
    }
    page.call("Emulation.setEmulatedMedia", json!({"features": []}));
}

#[test]
fn a_comment_held_for_one_who_may_not_direct_agents_is_drawn_where_it_was_left_with_no_pass_on() {
    for (width, height, mobile) in SCREENS {
        // Nothing held at first; once the feed says one was, both of hers.
        let site = served(&[], false);
        let Some(mut page) = chrome() else { return };
        page.viewport(width, height, mobile);
        page.open(&site.url(PAGE));
        let add = "[data-doc-key=\"intro\"] > .add";
        assert!(page.wait_for(&crate::visible(add), SHOWN));
        super::wait_for_feed(&site).expect("the page never followed the feed");
        page.eval(&format!("document.querySelector({}).click()", json!(add)));
        let text = "[data-doc-key=\"intro\"] form.comment textarea";
        assert!(page.wait_for(
            &format!(
                "document.activeElement === document.querySelector({})",
                json!(text)
            ),
            SHOWN
        ));
        page.call("Input.insertText", json!({"text": WORDS}));
        page.eval("document.querySelector('[data-doc-key=\"intro\"] form.comment button[type=submit]').click()");
        assert!(
            page.wait_for(
                "[...document.querySelectorAll('[data-doc-key=\"intro\"] [role=status]')].some(s => s.textContent === 'Comment held')",
                SHOWN
            ),
            "the save says it was held: {}",
            page.eval("document.querySelector('[data-doc-key=\"intro\"]').innerText")
        );
        let posts = site.posts();
        assert_eq!(posts.len(), 1, "{posts:?}");
        assert_eq!(posts[0].0, "/api/comment");
        assert_eq!(
            drawn(&mut page),
            json!([]),
            "nothing until the feed says so"
        );
        holding(&site, &[callout(), message()], false);
        // The page opens its feed again naming its sections: an event pushed before that is lost.
        test_time::until_true("the page's feed following the intro", super::STALL, || {
            site.following("intro")
        })
        .expect("the page's feed never followed the intro");
        site.push(
            9,
            &json!({"version": 9, "changes": [{"region": "held", "data": {}}]}).to_string(),
        );
        assert_eq!(shown(&mut page, 2), both(false), "{width}");
        shot(&mut page, "document-held", width);
    }
}

#[test]
fn pass_on_posts_once_and_the_next_read_leaves_the_comment_out() {
    for (width, height, mobile) in SCREENS {
        let site = served(&[callout(), message()], true);
        let Some(mut page) = chrome() else { return };
        page.viewport(width, height, mobile);
        page.open(&site.url(PAGE));
        assert_eq!(shown(&mut page, 2), both(true), "{width}");
        shot(&mut page, "document-pass", width);
        // Passed, the server holds the message alone.
        holding(&site, &[message()], true);
        page.eval("document.querySelector('#held-4 button').click()");
        let left = shown(&mut page, 1);
        assert_eq!(left[0]["id"], "held-5", "{width}: {left}");
        assert_eq!(
            site.posts(),
            [(
                symphony_api::DOCUMENT_PASS_ROUTE.to_string(),
                json!({"id": 4})
            )]
        );
    }
}

#[test]
fn a_comment_whose_paragraph_changed_is_drawn_at_its_sections_end_and_passed_on_there() {
    for (width, height, mobile) in SCREENS {
        let site = served(&[callout()], true);
        let Some(mut page) = chrome() else { return };
        page.viewport(width, height, mobile);
        page.open(&site.url(PAGE));
        assert_eq!(shown(&mut page, 1)[0]["under"], "intro", "{width}");
        // Its paragraph's words change: the server says its pass writes it at its section's end,
        // and the page holding it reads that as its copy moves.
        holding(&site, &[at_its_sections_end()], true);
        test_time::until_true("the page's feed following the intro", super::STALL, || {
            site.following("intro")
        })
        .expect("the page's feed never followed the intro");
        let intro = json!({"start": 3, "end": 3, "key": "intro", "hash": "intro-changed",
            "part": 1, "blocks": [{"type": "paragraph",
                "spans": [{"type": "text", "text": "The catalogue holds 140 lamps."}]}]});
        let change = json!({"version": 8, "changes": [{"region": "document",
            "data": {"path": "docs/PROBE.md", "sections": [intro]}}]});
        site.push(8, &change.to_string());
        page.reveal("[data-doc-key=\"decision\"]");
        assert!(
            page.wait_for(
                "document.querySelector('[data-doc-key=\"decision\"] #held-4') !== null",
                SHOWN
            ),
            "{width}: never drawn at its section's end: {}",
            drawn(&mut page)
        );
        page.frames_with_fonts();
        assert_eq!(
            drawn(&mut page),
            json!([{"id": "held-4", "under": "decision", "author": "ana", "kind": "question",
                "mark": "held", "words": WORDS, "buttons": ["Pass on"], "within": true}]),
            "{width}"
        );
        shot(&mut page, "document-held-changed", width);
        // Passed on there, the server holds nothing more.
        holding(&site, &[], true);
        page.eval("document.querySelector('#held-4 button').click()");
        assert!(
            page.wait_for(
                "document.querySelectorAll('.held-comment').length === 0",
                SHOWN
            ),
            "{width}: still drawn once passed: {}",
            drawn(&mut page)
        );
        assert_eq!(
            site.posts(),
            [(
                symphony_api::DOCUMENT_PASS_ROUTE.to_string(),
                json!({"id": 4})
            )]
        );
    }
}
