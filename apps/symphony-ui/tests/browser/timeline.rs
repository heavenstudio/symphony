//! A task's timeline on its page (src/task_timeline.rs), the release client against the stand-in
//! server answering `/api/task-timeline` with the approved pictures' task: a list that fits a
//! phone's width, and lanes over the same hours on a wide screen.
use super::{SHOWN, binaries, chrome, look_parity, site, site::Site};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;

const TASK: &str = "/task/visual/in-progress";

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../../crates/symphony-api/fixtures/task-timeline.json"
    ))
    .expect("the timeline fixture")
}

/// The stand-in server answering the look-parity task's page and thread, and `timeline` for its
/// timeline.
fn served(timeline: &Value) -> Site {
    let site = site();
    let content = look_parity::responses();
    for path in ["/api/task-page", "/api/task-history"] {
        site.set_json(path, &content[path]);
    }
    site.set_json(symphony_api::TASK_TIMELINE_PATH, timeline);
    site
}

fn view(chrome: &mut super::chrome::Chrome, width: u32, phone: bool) {
    chrome.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width": width, "height": 900, "deviceScaleFactor": 1, "mobile": phone}),
    );
    chrome.eval(
        "new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))",
    );
}

/// Whether the elements `selector` matches are drawn (laid out with a size).
fn drawn(chrome: &mut super::chrome::Chrome, selector: &str) -> bool {
    chrome.eval(&format!(
        "[...document.querySelectorAll({})].some(e => e.getClientRects().length > 0 && e.getBoundingClientRect().width > 0)",
        json!(selector)
    )) == json!(true)
}

#[test]
fn a_tasks_timeline_is_a_list_that_fits_a_phone_and_lanes_on_a_wide_screen() {
    let site = served(&fixture());
    let Some(mut chrome) = chrome() else { return };
    view(&mut chrome, 390, true);
    chrome.open(&site.url(TASK));
    assert!(
        chrome.wait_for(
            "document.querySelectorAll('.tl-list .tl-row').length === 14",
            SHOWN
        ),
        "the timeline's rows were not drawn: {:?}",
        chrome.eval("document.querySelector('article.task')?.innerText")
    );
    assert!(
        site.targets()
            .iter()
            .any(|target| target == "/api/task-timeline?slug=visual%2Fin-progress"),
        "the page asks for its own task's timeline: {:?}",
        site.targets()
    );
    view(&mut chrome, 390, true);
    assert_eq!(
        chrome.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true),
        "the page scrolls sideways at 390 px: {}",
        chrome.eval("document.documentElement.scrollWidth")
    );
    assert!(drawn(&mut chrome, ".tl-list") && !drawn(&mut chrome, ".tl-lanes"));
    assert_eq!(
        super::texts(&mut chrome, ".tl-totals span"),
        [
            "6 h 18 m so far · 2 review rounds · 1 bounce",
            "Machine 1 h 33 m, queued 26 m"
        ]
    );
    let labels = super::texts(&mut chrome, ".tl-row .tl-label");
    assert_eq!(labels[0], "Ready, unclaimed");
    assert_eq!(labels[13], "Test, certificate verify · queued 10 m");
    // Every row keeps inside the card, its bar and its word too.
    assert_eq!(
        chrome.eval("(() => { const card = document.querySelector('.tl-card').getBoundingClientRect(); return [...document.querySelectorAll('.tl-row, .tl-row *')].every(e => { const r = e.getBoundingClientRect(); return r.width === 0 || (r.left >= card.left - 0.5 && r.right <= card.right + 0.5); }); })()"),
        json!(true)
    );
    // The theme colours the run that goes on now: striped, between its queue and its word.
    assert_eq!(
        chrome.eval("getComputedStyle(document.querySelector('.tl-list .tl-running')).backgroundImage.startsWith('repeating-linear-gradient')"),
        json!(true)
    );

    view(&mut chrome, 1280, false);
    assert!(drawn(&mut chrome, ".tl-lanes") && !drawn(&mut chrome, ".tl-list"));
    assert_eq!(
        super::texts(&mut chrome, ".tl-lane[data-lane] .tl-name b"),
        ["Stage", "Executor", "Reviewer", "Builds & tests"]
    );
    assert_eq!(
        super::texts(&mut chrome, ".tl-gate .tl-name"),
        ["certificate", "dependency", "nightly", "review"]
    );
    assert_eq!(super::texts(&mut chrome, ".tl-nowtag"), ["now 15:20"]);
    // The lanes share the hours: the line at now meets the end of every lane's track.
    assert_eq!(
        chrome.eval("(() => { const now = document.querySelector('.tl-now').getBoundingClientRect().left; return [...document.querySelectorAll('.tl-lane:not(.tl-gates-head) .tl-track')].every(t => Math.abs(t.getBoundingClientRect().right - now) < 1.5); })()"),
        json!(true)
    );
    assert_eq!(
        chrome.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
}

#[test]
fn a_task_with_no_run_kept_draws_no_builds_or_tests() {
    let mut timeline = fixture();
    timeline["runs"] = json!([]);
    let site = served(&timeline);
    let Some(mut chrome) = chrome() else { return };
    view(&mut chrome, 1280, false);
    chrome.open(&site.url(TASK));
    assert!(chrome.wait_for(
        "document.querySelectorAll('.tl-lanes .tl-lane[data-lane]').length === 3",
        SHOWN
    ));
    assert_eq!(
        chrome.eval("document.querySelector('.tl-lane[data-lane=runs], .tl-running, .tl-machine')"),
        Value::Null
    );
    assert_eq!(
        super::texts(&mut chrome, ".tl-totals"),
        ["6 h 18 m so far · 2 review rounds · 1 bounce"]
    );
}

/// The pictures a review of the timeline reads beside the approved ones
/// (docs/analysis/reports-timeline-*.svg): `symphony shot`, the workspace's build run as an agent
/// runs it, of the pictures' task on its page from the stand-in server, at a phone's width and wide,
/// light and dark, into the folder `SF_TIMELINE_SHOTS` names. Without it the case takes none.
#[test]
fn pictures_of_the_timeline_when_a_folder_is_named() {
    let Some(out) = std::env::var_os("SF_TIMELINE_SHOTS").map(PathBuf::from) else {
        eprintln!("SF_TIMELINE_SHOTS names no folder: no pictures taken");
        return;
    };
    let Some(browser) = super::chrome::find() else {
        eprintln!("no Chrome on this machine (SF_CHROME names one): no pictures taken");
        return;
    };
    std::fs::create_dir_all(&out).unwrap();
    let site = served(&fixture());
    let symphony = binaries::built("symphony");
    for (name, view) in [
        ("phone-light", &["--phone"][..]),
        ("phone-dark", &["--phone", "--scheme", "dark"]),
        ("wide-light", &["--size", "1280x900"]),
        ("wide-dark", &["--size", "1280x900", "--scheme", "dark"]),
    ] {
        let picture = out.join(format!("timeline-{name}.png"));
        let done = Command::new(&symphony)
            .args(["shot", "--timeout", "120", &site.url(TASK)])
            .args(view)
            .args(["--full-page", "--wait-for", ".tl-row", "--report", "--out"])
            .arg(&picture)
            .env("SF_CHROME", &browser)
            .output()
            .expect("run symphony shot");
        assert!(
            done.status.success() && picture.is_file(),
            "{name}: {}\n{}",
            String::from_utf8_lossy(&done.stdout),
            String::from_utf8_lossy(&done.stderr)
        );
        eprintln!(
            "{}: {}",
            picture.display(),
            String::from_utf8_lossy(&done.stdout).trim()
        );
    }
}
