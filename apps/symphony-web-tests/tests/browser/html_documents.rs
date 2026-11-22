//! An HTML document opened at its route: the app's page around it, the file drawn in a frame whose
//! scripts run, and never with the app's origin. From inside the frame the app's storage, cookies,
//! page and API are out of reach, the app's page cannot be navigated away and no popup opens. The
//! stand-in sends the file no policy of its own (`Site::set_html_page`), so what holds there is the
//! frame's sandbox alone; the server's policy on the file is tested beside it
//! (apps/symphony-server/src/media_files.rs, projects_tests.rs), and the real server draws both an
//! HTML document and a Markdown document's mockup of one here too.
use super::{SHOWN, chrome, chrome::Chrome, restored::Native, site};
use serde_json::{Value, json};

/// The document's route, under the convention's `/docs/`.
const ROUTE: &str = "/docs/analysis/probe.html";

/// A page whose script tries each thing the app keeps to itself and tells the page around it what
/// it got, each try's error name or `allowed`. Its last step writes into its own body, so a frame
/// that drew the file and ran its script shows it.
const PROBE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Probe</title></head>
<body><p id="ran">not run</p>
<script>
const tried = (what) => { try { what(); return 'allowed'; } catch (error) { return error.name; } };
(async () => {
  const report = { probe: true, origin: self.origin };
  report.localStorage = tried(() => localStorage.setItem('probe', 'written by the frame'));
  report.indexedDB = tried(() => indexedDB.open('probe'));
  report.cookie = tried(() => document.cookie);
  report.parent = tried(() => parent.document.title);
  report.parentStorage = tried(() => parent.localStorage.getItem('probe'));
  try { report.popup = window.open('/elsewhere') === null ? 'refused' : 'opened'; }
  catch (error) { report.popup = error.name; }
  report.top = tried(() => { top.location.href = '/elsewhere'; });
  try {
    const answer = await fetch('/api/board');
    report.fetch = 'answered ' + answer.status;
  } catch (error) {
    report.fetch = error.name;
  }
  document.getElementById('ran').textContent = 'ran';
  parent.postMessage(report, '*');
})();
</script></body></html>"#;

/// Before any page of the tab loads: the app's page keeps the first report a frame posts it.
const LISTEN: &str = "addEventListener('message', (event) => { if (event.data && event.data.probe && !window.probeReport) window.probeReport = event.data; });";

/// The probe's report once the page drew its frame and the frame's script ran.
fn opened(page: &mut Chrome, url: &str) -> Value {
    page.open(url);
    let drawn = page.wait_for(
        "document.querySelector('[data-ready=html] iframe.html-frame') !== null && window.probeReport !== undefined",
        SHOWN,
    );
    assert!(
        drawn,
        "the probe's frame was not drawn, or its script never ran, within {SHOWN:?}; the page shows: {}",
        page.eval("document.body ? document.body.innerHTML.slice(0, 2000) : '(no body)'")
    );
    page.eval("window.probeReport")
}

#[test]
fn an_html_document_runs_its_scripts_in_a_frame_that_never_reaches_the_app() {
    let site = site();
    site.set_html_page(ROUTE, PROBE);
    let Some(mut page) = chrome() else { return };
    page.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": LISTEN }),
    );
    for (width, height, mobile) in [(1440, 900, false), (390, 844, true)] {
        page.viewport(width, height, mobile);
        let report = opened(&mut page, &site.url(ROUTE));
        // Its script ran, in an opaque origin: not the app's.
        assert_eq!(report["origin"], "null", "{width}: {report}");
        for (what, refused) in [
            ("localStorage", "SecurityError"),
            ("indexedDB", "SecurityError"),
            ("cookie", "SecurityError"),
            ("parent", "SecurityError"),
            ("parentStorage", "SecurityError"),
            ("top", "SecurityError"),
            ("popup", "refused"),
            // A read of the API from another origin, which the stand-in answers without saying
            // that origin may read it, as the server does: the browser refuses the frame the answer.
            ("fetch", "TypeError"),
        ] {
            assert_eq!(report[what], refused, "{width}: {what} in {report}");
        }
        let page_state = page.eval(
            "(() => { const frame = document.querySelector('iframe.html-frame'); const shown = frame.getBoundingClientRect(); return { \
             path: location.pathname, sandbox: frame.getAttribute('sandbox'), src: frame.getAttribute('src'), \
             eyebrow: document.querySelector('.html-document .eyebrow')?.textContent, \
             header: document.querySelector('.app-pages a.active')?.textContent.trim(), \
             kept: localStorage.getItem('probe'), \
             left: shown.left >= 0, right: shown.right <= innerWidth, tall: shown.height >= 320, \
             overflow: document.documentElement.scrollWidth > innerWidth, frames: window.frames.length }; })()",
        );
        assert_eq!(
            page_state,
            json!({
                "path": ROUTE, "sandbox": "allow-scripts", "src": format!("{ROUTE}?raw"),
                "eyebrow": "docs/analysis/probe.html", "header": "Docs", "kept": null,
                "left": true, "right": true, "tall": true, "overflow": false,
                "frames": 1,
            }),
            "{width}"
        );
        assert!(
            !site.requests().iter().any(|path| path == "/elsewhere"),
            "{width}: the frame navigated the app or opened a page: {:?}",
            site.requests()
        );
    }
}

/// The real server, its routing and its policy: the HTML document's page draws its frame and the
/// probe's script runs in it, in an opaque origin and refused all the same; and a Markdown document
/// of the same folder showing that HTML document as a mockup draws it too, in a frame of its own,
/// both frames at the file's address.
#[test]
fn the_server_draws_an_html_document_and_a_markdown_documents_mockup_of_it() {
    let Some(mut page) = chrome() else { return };
    let site = Native::start_committed(
        None,
        "main",
        &[
            ("docs/probe.html", PROBE),
            ("docs/MOCKUP.md", "# Mockup\n\n![The probe](probe.html)\n"),
        ],
        None,
    );
    page.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": LISTEN }),
    );
    page.viewport(1280, 900, false);
    let report = opened(&mut page, &site.url("/docs/probe.html"));
    for (what, refused) in [
        ("origin", "null"),
        ("localStorage", "SecurityError"),
        ("cookie", "SecurityError"),
        ("parent", "SecurityError"),
        ("top", "SecurityError"),
        ("popup", "refused"),
        ("fetch", "TypeError"),
    ] {
        assert_eq!(report[what], refused, "{what} in {report}");
    }
    assert_eq!(
        page.eval(
            "[location.pathname, document.querySelector('iframe.html-frame').getAttribute('src')]"
        ),
        json!(["/docs/probe.html", "/docs/probe.html?raw"])
    );
    page.open(&site.url("/docs/MOCKUP.md"));
    let drawn = page.wait_for(
        "document.querySelector('.mockup-frame iframe') !== null && window.probeReport !== undefined",
        SHOWN,
    );
    assert!(
        drawn,
        "the mockup was not drawn, or its script never ran, within {SHOWN:?}; the page shows: {}",
        page.eval("document.body ? document.body.innerHTML.slice(0, 2000) : '(no body)'")
    );
    assert_eq!(page.eval("window.probeReport.origin"), json!("null"));
    // The server names the mockup's version in its address (`?v=`); the frame adds the file's query.
    let src = page.eval("document.querySelector('.mockup-frame iframe').getAttribute('src')");
    let (path, query) = src.as_str().unwrap().split_once('?').unwrap();
    assert_eq!(path, "/docs/probe.html", "{src}");
    assert!(symphony_api::asks_for_file(query), "{src}");
}
