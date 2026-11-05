//! New activity loads the persistent panel to toast it, whatever else the page reads meanwhile.
use super::super::{SHOWN, chrome, common, dist, site::Site};
use super::{document, paragraph};
use serde_json::json;

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
    let items: Vec<_> = (0..2)
        .map(|n| {
            json!({
                "id":format!("arrival-{n}"),"kind":"question","path":"docs/PROBE.md",
                "slug":format!("Arrived {n}"),"ts":"2026-11-02T06:00:00Z",
                "text":"A real activity notification","href":"/docs/PROBE.md","answered":false
            })
        })
        .collect();
    site.set_json("/api/activity", &json!({"items":items}));
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
        browser.eval("[...document.querySelectorAll('#toasts .pop a')].map(a=>a.textContent)"),
        json!(["question · Arrived 0", "question · Arrived 1"])
    );
}
