//! New activity loads the persistent panel to toast it, whatever else the page reads meanwhile.
use super::super::{SHOWN, chrome, chrome::Chrome, common, dist, site::Site};
use super::{document, paragraph};
use serde_json::{Value, json};

/// Hold, from the page's first script on, every timer the page sets for a toast's lifetime
/// (`TOAST_MS`), so the case says when that lifetime runs out instead of racing it: on a loaded
/// machine the page's clock runs seconds between two of the case's polls, past a toast's whole
/// lifetime. The page's other timers run as they are. Call it before `open`.
pub(super) fn hold_toast_lifetimes(browser: &mut Chrome) {
    browser.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({"source": format!(
            r#"window.heldToastLifetimes = [];
            const pageTimer = window.setTimeout;
            window.setTimeout = function (callback, delay, ...rest) {{
                if (delay === {lifetime}) {{ heldToastLifetimes.push(callback); return 0; }}
                return pageTimer.call(this, callback, delay, ...rest);
            }};"#,
            lifetime = symphony_ui::state::TOAST_MS
        )}),
    );
}

/// `/api/activity` listing one unanswered question per slug, newest first.
fn activity(slugs: &[&str]) -> Value {
    let items: Vec<_> = slugs
        .iter()
        .map(|slug| {
            json!({
                "id":slug.to_lowercase().replace(' ', "-"),"kind":"question","path":"docs/PROBE.md",
                "slug":slug,"ts":"2026-11-02T06:00:00Z",
                "text":"A real activity notification","href":"/docs/PROBE.md","answered":false
            })
        })
        .collect();
    json!({ "items": items })
}

/// The toasts up, by their text.
const TOASTS_UP: &str = "[...document.querySelectorAll('#toasts .pop a')].map(a=>a.textContent)";

/// The persistent panel's script, as the shell's module manifest names it.
fn panel_script() -> String {
    let shell = std::fs::read_to_string(dist().join("index.html")).expect("the build's shell");
    let start = shell
        .find("/assets/symphony-shell-")
        .expect("the shell's script");
    let end = start + shell[start..].find(".js").expect("its name") + 3;
    let manifest = std::fs::read_to_string(dist().join(&shell[start + 1..end]))
        .expect("the shell's script file");
    let entry = r#""panel":{"script":""#;
    let start = manifest.find(entry).expect("the panel's module entry") + entry.len();
    manifest[start..start + manifest[start..].find('"').unwrap()].to_owned()
}

#[test]
fn activity_that_arrives_while_the_panel_loads_toasts_after_another_read() {
    // A later read of the activity (any region change's or a reconnect's) can land before the
    // panel's module does; it must not make the arrival look old to the panel, or no toast ever
    // shows. The panel's script is held until that read has landed, so the order is fixed, not
    // raced.
    let site = Site::start(dist(), &common::fixture("board.json"));
    document(&site, vec![paragraph("A short document.")]);
    let Some(mut browser) = chrome() else { return };
    hold_toast_lifetimes(&mut browser);
    browser.open(&site.url("/docs/PROBE.md"));
    assert!(browser.wait_for("document.querySelector('.document-page')?.dataset.feed==='live' && document.querySelector('[data-doc-key=intro] p')!==null && Array.isArray(window.reviewKnownIds)", SHOWN));
    assert_eq!(
        browser
            .eval("window.reviewPanelReady===true || document.getElementById('quick-root')!==null"),
        json!(false),
        "nothing has loaded the panel before the arrival"
    );
    let panel = panel_script();
    site.hold_route(&panel, true);
    browser.eval("window.frameReads=0;addEventListener('review:frame',()=>frameReads++)");
    site.set_json("/api/activity", &activity(&["Arrived 0", "Arrived 1"]));
    site.push(
        4183,
        r#"{"version":4183,"changes":[{"region":"activity","data":{}}]}"#,
    );
    assert!(
        browser.wait_for(
            "frameReads>0 && window.reviewKnownIds.length===2 && document.getElementById('quick-root')!==null",
            SHOWN
        ),
        "the arrival is read and starts loading the panel"
    );
    browser.eval("window.frameReads=0");
    site.push(
        4184,
        r#"{"version":4184,"changes":[{"region":"pending","data":{}}]}"#,
    );
    assert!(
        browser.wait_for("frameReads>0", SHOWN),
        "another read lands before the panel"
    );
    assert_eq!(
        browser.eval("window.reviewPanelReady===true"),
        json!(false),
        "the panel is still held"
    );
    site.hold_route(&panel, false);
    assert!(
        browser.wait_for(
            "document.querySelectorAll('#toasts .pop').length===2",
            SHOWN
        ),
        "both arrivals toast once the panel loads: {}",
        browser.eval("document.getElementById('toasts')?.innerText ?? null")
    );
    assert_eq!(
        browser.eval(TOASTS_UP),
        json!(["question · Arrived 0", "question · Arrived 1"])
    );
    assert_eq!(
        browser.eval("heldToastLifetimes.length"),
        json!(1),
        "the batch set its one lifetime, held here"
    );
    browser.eval("heldToastLifetimes.splice(0).forEach(lifetime=>lifetime())");
    assert!(
        browser.wait_for(
            "document.querySelectorAll('#toasts .pop').length===0",
            SHOWN
        ),
        "the batch leaves when its lifetime runs out"
    );
}

#[test]
fn a_batch_arriving_inside_an_earlier_one_s_lifetime_keeps_its_own() {
    // Each batch of toasts leaves `TOAST_MS` after its own arrival. The first batch's lifetime
    // running out while the second is up must take down the first alone: the second still has
    // its whole lifetime to go. The lifetimes are held, so the case ends each one in turn.
    let site = Site::start(dist(), &common::fixture("board.json"));
    document(&site, vec![paragraph("A short document.")]);
    let Some(mut browser) = chrome() else { return };
    hold_toast_lifetimes(&mut browser);
    browser.open(&site.url("/docs/PROBE.md"));
    assert!(browser.wait_for("document.querySelector('.document-page')?.dataset.feed==='live' && document.querySelector('[data-doc-key=intro] p')!==null && Array.isArray(window.reviewKnownIds)", SHOWN));
    site.set_json("/api/activity", &activity(&["First 0"]));
    site.push(
        4183,
        r#"{"version":4183,"changes":[{"region":"activity","data":{}}]}"#,
    );
    assert!(
        browser.wait_for(
            "document.querySelectorAll('#toasts .pop').length===1 && heldToastLifetimes.length===1",
            SHOWN
        ),
        "the first batch is up, its lifetime held"
    );
    site.set_json(
        "/api/activity",
        &activity(&["Second 0", "Second 1", "First 0"]),
    );
    site.push(
        4184,
        r#"{"version":4184,"changes":[{"region":"activity","data":{}}]}"#,
    );
    assert!(
        browser.wait_for(
            "document.querySelectorAll('#toasts .pop').length===3 && heldToastLifetimes.length===2",
            SHOWN
        ),
        "the second batch joins the first inside its lifetime: {}",
        browser.eval(TOASTS_UP)
    );
    browser.eval("heldToastLifetimes[0]()");
    assert!(
        browser.wait_for(
            "![...document.querySelectorAll('#toasts .pop a')].some(a=>a.textContent.includes('First'))",
            SHOWN
        ),
        "the first batch leaves when its lifetime runs out"
    );
    assert_eq!(
        browser.eval(TOASTS_UP),
        json!(["question · Second 0", "question · Second 1"]),
        "the second batch stays for its own lifetime"
    );
    browser.eval("heldToastLifetimes[1]()");
    assert!(
        browser.wait_for(
            "document.querySelectorAll('#toasts .pop').length===0",
            SHOWN
        ),
        "the second batch leaves when its own lifetime runs out"
    );
}
