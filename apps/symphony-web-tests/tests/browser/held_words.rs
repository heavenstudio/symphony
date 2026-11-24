//! A message held for people on a task's thread in the release client (apps/symphony-web/src/
//! task-page.tsx, `HeldMessage`): at the board's line for it, its author's name and to whom, its
//! words and the state `held`, with Pass on only for a reader the page says may pass it on; a
//! press posts the pass (`/api/task-pass`) once and stays pressed, and the line its words became,
//! brought by the feed, marks it passed on by whom, its words then that line's alone. On a desk and
//! a phone, the entry is drawn within the screen's width. The task is the look-parity fixture's.
use super::{SHOWN, chrome, chrome::Chrome, look_parity, site::Site};
use serde_json::{Value, json};

const PAGE: &str = "/task/visual/in-progress";
const SLUG: &str = "visual/in-progress";
const WORDS: &str = "The phone picture cuts the header off.";

/// The board's line `4` for ana's message held for people, as a person's read sends it.
fn marker() -> Value {
    json!({
        "message": {"id": 4, "slug": SLUG, "sender": "board", "recipient": "owner", "kind": "msg",
            "text": "a message is held for people: #4", "ts": 4.0, "person": false},
        "stamp": "2026-11-24 10:00",
        "blocks": [{"type": "paragraph", "spans": [{"type": "text", "text": "a message is held for people: #4"}]}],
        "held": {"id": 4, "author": "ana", "text": WORDS, "to": "@executor"},
    })
}

/// The look-parity task's page, the reader passing held messages on where `passes`, and its thread
/// holding the board's line for ana's message.
fn served(passes: bool) -> Site {
    let site = crate::site();
    let mut page = look_parity::responses()["/api/task-page"].clone();
    page["passes"] = json!(passes);
    site.set_json("/api/task-page", &page);
    site.set_json(
        "/api/task-history",
        &json!({"entries": [marker()], "before": null, "after": 4}),
    );
    site.set_json(symphony_api::TASK_PASS_ROUTE, &json!({"ok": true}));
    site
}

/// The held entry as drawn, once it is: its state, author, mark, words and controls, and whether
/// it and the page stay within the screen's width.
fn held(page: &mut Chrome) -> Value {
    page.reveal("[id=\"thread-region-visual/in-progress\"]");
    page.reveal("#message-4");
    assert!(
        page.wait_for(
            "document.querySelector('#message-4 .entry') !== null",
            SHOWN
        ),
        "the held message was never drawn: {}",
        page.eval("document.querySelector('#task-thread')?.textContent")
    );
    page.eval(
        r#"(() => {
            const entry = document.querySelector('#message-4 .entry');
            const box = entry.getBoundingClientRect();
            return {
                held: entry.dataset.held,
                from: entry.querySelector('.from').textContent,
                mark: entry.querySelector('.entry-held').textContent,
                words: entry.querySelector('.entry-text')?.textContent ?? null,
                buttons: [...entry.querySelectorAll('button')].map(b => b.textContent),
                within: box.right <= innerWidth + 0.5 && document.documentElement.scrollWidth <= innerWidth,
            };
        })()"#,
    )
}

#[test]
fn a_held_message_is_its_authors_words_marked_held_with_pass_on_only_for_one_who_may() {
    for (width, height, mobile) in [(1440, 900, false), (390, 844, true)] {
        let site = served(true);
        let Some(mut page) = chrome() else { return };
        page.viewport(width, height, mobile);
        page.open(&site.url(PAGE));
        assert_eq!(
            held(&mut page),
            json!({"held": "held", "from": "ana → @executor", "mark": "held", "words": WORDS,
                "buttons": ["Pass on"], "within": true}),
            "{width}"
        );
        assert_eq!(site.posts(), []);
    }
    let site = served(false);
    let Some(mut page) = chrome() else { return };
    page.open(&site.url(PAGE));
    assert_eq!(
        held(&mut page)["buttons"],
        json!([]),
        "no control for one who may not press it"
    );
}

#[test]
fn pass_on_posts_once_and_the_line_its_words_became_marks_it_passed() {
    let site = served(true);
    let Some(mut page) = chrome() else { return };
    page.open(&site.url(PAGE));
    held(&mut page);
    super::wait_for_feed(&site).expect("the page never followed the feed");
    page.eval("document.querySelector('#message-4 button').click()");
    assert!(
        page.wait_for(
            "document.querySelector('#message-4 button').disabled",
            SHOWN
        ),
        "Pass on stays pressed until the feed brings the pass"
    );
    assert_eq!(
        site.posts(),
        [(
            symphony_api::TASK_PASS_ROUTE.to_string(),
            json!({"slug": SLUG, "id": 4})
        )]
    );
    let line = json!({"id": 5, "slug": SLUG, "sender": "ana", "recipient": "@executor",
        "kind": "msg", "text": format!("[passed on by owner] {WORDS}"), "ts": 5.0, "person": true,
        "stamp": "2026-11-24 10:01", "blocks": [],
        "held": {"id": 4, "author": "ana", "text": WORDS, "to": "@executor", "passed_by": "owner"}});
    site.push(
        4183,
        &json!({"version": 4183, "changes": [{"region": "message", "data": line}]}).to_string(),
    );
    page.reveal("#message-5");
    assert!(
        page.wait_for(
            "document.querySelector('#message-4 .entry')?.dataset.held === 'passed' && document.querySelector('#message-5 .entry') !== null",
            SHOWN
        ),
        "the pass never came: {}",
        page.eval("document.querySelector('#task-thread')?.textContent")
    );
    assert_eq!(
        held(&mut page),
        json!({"held": "passed", "from": "ana → @executor", "mark": "passed on by owner",
            "words": null, "buttons": [], "within": true})
    );
    assert_eq!(
        page.eval(
            "(e => [e.className, e.querySelector('.from').textContent, e.querySelector('.entry-text').textContent])(document.querySelector('#message-5 .entry'))"
        ),
        json!([
            "entry owner",
            "ana → @executor",
            format!("[passed on by owner] {WORDS}")
        ])
    );
    assert_eq!(site.posts().len(), 1);
}
