//! A task's execution attempts on its page, in the real client, wide and on a phone
//! (docs/engineering/EXECUTION_ATTEMPTS.md, "On a task's page"). A task with attempts draws its history, newest
//! first, from its own read; a control waits on its confirmation, then posts the attempt's revision
//! as read and draws the history it answers; a view's address goes to the window the click opened
//! alone, never into the page or the device; and a task with none loads none of the attempts'
//! chunk and reads nothing of them.
//!
//! The stand-in server answers with the wire types' own serializations (`wire.cases.json`, written
//! by `crates/symphony-api/examples/typescript.rs`); the routes themselves, their authority and
//! refusals, are proved on a private board (`apps/symphony-server/src/attempts_tests.rs`). With
//! `SYMPHONY_ATTEMPT_SHOTS` naming a folder, the page saves its pictures there.

use super::{SHOWN, chrome::Chrome, look_parity, open, samples, site::Site, texts};
use serde_json::{Value, json};
use symphony_api::{ATTEMPT_CONTROL_ROUTE, ATTEMPT_VIEW_ROUTE, TASK_ATTEMPTS_PATH};

/// The task whose attempts are drawn.
const SLUG: &str = "w/a";

/// The stand-in server: `w/a`'s page counting `attempts`, its thread, its attempts (the sample's
/// three), a control answered with them after a cancel was asked of the running one, and a view.
fn site(attempts: usize) -> (Site, Value) {
    let samples = samples();
    let site = crate::site();
    let task = super::site::example_listing("in_progress")["tasks"][0].clone();
    let mut page = json!({"task": task, "blocks": [{"type": "paragraph", "spans": [{"type": "text",
        "text": "Run it on the provider."}]}], "children": [], "children_count": 0});
    page["task"]["slug"] = json!(SLUG);
    if attempts > 0 {
        page["attempts"] = json!(attempts);
    }
    site.set_json("/api/task-page", &page);
    site.set_json(
        "/api/task-history",
        &look_parity::responses()["/api/task-history"],
    );
    site.set_json(TASK_ATTEMPTS_PATH, &samples["attempts"]);
    let mut asked = samples["attempts"].clone();
    asked["attempts"][1]["cancellation"] = json!("requested");
    asked["attempts"][1]["revision"] = json!(5);
    asked["attempts"][1]["controls"]["cancel"] = json!(false);
    site.set_json(ATTEMPT_CONTROL_ROUTE, &asked);
    site.set_json(ATTEMPT_VIEW_ROUTE, &samples["attemptLink"]);
    (site, samples)
}

/// The page as its fonts and two frames leave it, the attempts scrolled to, in each scheme, saved
/// as `NAME-light.png` and `NAME-dark.png` in the folder `SYMPHONY_ATTEMPT_SHOTS` names; nothing
/// without one. The page is left in the browser's own scheme.
fn shot(page: &mut Chrome, name: &str) {
    let Some(folder) = std::env::var_os("SYMPHONY_ATTEMPT_SHOTS") else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    for scheme in ["light", "dark"] {
        page.scheme(scheme);
        page.eval("document.querySelector('.attempts')?.scrollIntoView({block: 'start'})");
        page.frames_with_fonts();
        // The colours' transitions into the scheme, finished.
        page.wait_for("document.getAnimations().length === 0", SHOWN);
        std::fs::write(
            folder.join(format!("{name}-{scheme}.png")),
            page.screenshot(),
        )
        .unwrap();
    }
    page.call("Emulation.setEmulatedMedia", json!({"features": []}));
}

/// Clicks the button reading `label` in the attempt whose id starts with `id`.
fn click(page: &mut Chrome, id: &str, label: &str) {
    let clicked = page.eval(&format!(
        "(() => {{ const button = [...document.querySelectorAll('[data-attempt^={}] button')].find(one => one.textContent === {}); button?.click(); return !!button; }})()",
        json!(id),
        json!(label)
    ));
    assert_eq!(clicked, json!(true), "no {label} on {id}");
}

#[test]
fn a_tasks_attempts_are_drawn_newest_first_and_a_control_waits_on_its_confirmation() {
    let (site, _) = site(3);
    let Some(mut page) = super::chrome() else {
        panic!("Chrome is required for the attempts' acceptance")
    };
    open(&mut page, &site, &format!("/task/{SLUG}"), 1280);
    assert!(
        page.wait_for(
            "document.querySelectorAll('.attempts .attempt').length === 3",
            SHOWN
        ),
        "{:?}",
        page.eval("document.querySelector('main')?.innerText")
    );
    assert_eq!(
        page.eval(
            "[...document.querySelectorAll('.attempt')].map(one => one.dataset.attempt.slice(0, 1))"
        ),
        json!(["c", "b", "a"])
    );
    assert_eq!(
        json!(texts(&mut page, ".attempts h2")),
        json!(["Attempts 3"])
    );
    assert_eq!(
        json!(texts(&mut page, "[data-attempt^=b] button")),
        json!(["Cancel", "Stop: pause", "Stop: terminate", "View"])
    );
    assert_eq!(
        json!(texts(&mut page, "[data-attempt^=c] button")),
        json!(["Cancel"]),
        "a provider this server does not hold offers no resume or view"
    );
    let said = page.eval("document.querySelector('[data-attempt^=b]').innerText");
    assert!(
        said.as_str()
            .is_some_and(|said| said.contains("resume of aaaaaaaa · resource res-2 (g1)")),
        "{said}"
    );
    assert!(
        site.targets()
            .iter()
            .any(|target| target == &format!("{TASK_ATTEMPTS_PATH}?slug=w%2Fa")),
        "{:?}",
        site.targets()
    );
    shot(&mut page, "attempts-wide");

    click(&mut page, "b", "Stop: pause");
    assert_eq!(
        json!(texts(&mut page, "[data-attempt^=b] [role=group] span")),
        json!(["Stop: pause attempt bbbbbbbb?"])
    );
    assert!(
        site.posts().is_empty(),
        "nothing is asked before it is confirmed"
    );
    click(&mut page, "b", "Keep it");
    click(&mut page, "b", "Cancel");
    click(&mut page, "b", "Confirm");
    assert!(page.wait_for(
        "document.querySelector('.attempts [role=status]')?.textContent === 'Cancel of bbbbbbbb: asked'",
        SHOWN
    ));
    let posts = site.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    assert_eq!(posts[0].0, ATTEMPT_CONTROL_ROUTE);
    assert_eq!(
        posts[0].1,
        json!({"slug": SLUG, "attempt": "b".repeat(32), "revision": 4, "act": "cancel",
            "mode": null, "key": null}),
        "the revision as read, and nothing of who asks"
    );
    assert!(page.wait_for(
        "document.querySelector('[data-attempt^=b] .attempt-state').textContent === 'running, cancel requested'",
        SHOWN
    ));
    assert_eq!(
        json!(texts(&mut page, "[data-attempt^=b] button")),
        json!(["Stop: pause", "Stop: terminate", "View"]),
        "the answer's controls"
    );

    open(&mut page, &site, &format!("/task/{SLUG}"), 390);
    assert!(page.wait_for(
        "document.querySelectorAll('.attempts .attempt').length === 3",
        SHOWN
    ));
    assert_eq!(
        page.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true),
        "no sideways scroll on a phone"
    );
    shot(&mut page, "attempts-phone");
}

#[test]
fn a_view_goes_to_the_window_the_click_opened_and_is_kept_nowhere() {
    let (site, _) = site(3);
    let Some(mut page) = super::chrome() else {
        panic!("Chrome is required for the attempts' acceptance")
    };
    open(&mut page, &site, &format!("/task/{SLUG}"), 1280);
    assert!(page.wait_for(
        "document.querySelectorAll('.attempts .attempt').length === 3",
        SHOWN
    ));
    // The window the click opens, which the page sends the address to and tells nothing of itself.
    page.eval(
        "window.viewed = []; window.open = (url, target) => { const opened = { url, target, opener: window, \
         location: { replace: address => window.viewed.push([opened.opener, address]) }, close() {} }; \
         window.viewed.push(['opened', url, target]); return opened; }",
    );
    click(&mut page, "b", "View");
    assert!(page.wait_for("window.viewed.length === 2", SHOWN));
    assert_eq!(
        page.eval("window.viewed"),
        json!([
            ["opened", "", "_blank"],
            [null, "https://provider.invalid/res-2"]
        ]),
        "opened blank within the click, its opener cut, then sent the address"
    );
    assert_eq!(
        site.posts(),
        vec![(
            ATTEMPT_VIEW_ROUTE.to_owned(),
            json!({"slug": SLUG, "attempt": "b".repeat(32)})
        )]
    );
    assert_eq!(
        page.eval(
            "[document.documentElement.outerHTML, JSON.stringify({...localStorage}), JSON.stringify({...sessionStorage})].some(kept => kept.includes('provider.invalid'))"
        ),
        json!(false),
        "the address is in no page, storage or state the page keeps"
    );
}

#[test]
fn a_task_with_no_attempts_loads_and_reads_nothing_of_them() {
    let (site, _) = site(0);
    let Some(mut page) = super::chrome() else {
        panic!("Chrome is required for the attempts' acceptance")
    };
    open(&mut page, &site, &format!("/task/{SLUG}"), 1280);
    assert!(page.wait_for(
        "document.querySelector('.task-body')?.textContent.includes('Run it on the provider.')",
        SHOWN
    ));
    page.eval("window.scrollTo(0, document.body.scrollHeight)");
    assert!(page.wait_for("document.querySelector('#task-thread') !== null", SHOWN));
    assert_eq!(
        page.eval("document.querySelector('.attempts') === null"),
        json!(true)
    );
    let asked = site.requests();
    assert!(
        !asked
            .iter()
            .any(|path| path.contains("attempts") || path.contains(TASK_ATTEMPTS_PATH)),
        "{asked:?}"
    );
}
