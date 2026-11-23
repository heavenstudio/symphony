//! The Settings page (apps/symphony-web/src/settings.tsx, the web client's `settings` kind), the
//! release client against the stand-in server answering its one read (`/api/settings`) with the
//! wire's own sample: on a desk and a phone every part and entry drawn in the server's order
//! within the screen's width, lit in the header, with no control and nothing sent but reads; and
//! a read that fails said in the page's place.
use super::{SHOWN, chrome, chrome::Chrome, site, site::Site, texts};
use serde_json::{Value, json};

/// The wire's sample of the page (`wire.cases.json`'s `samples.settings`).
fn sample() -> Value {
    let cases: Value =
        serde_json::from_str(include_str!("../../../symphony-web/src/wire.cases.json"))
            .expect("the wire's cases");
    cases["samples"]["settings"].clone()
}

fn served() -> Site {
    let site = site();
    site.set_json(symphony_api::SETTINGS_PATH, &sample());
    site
}

fn screen(chrome: &mut Chrome, width: u32, height: u32, mobile: bool) {
    chrome.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":if mobile { 3 } else { 1 },"mobile":mobile}),
    );
}

fn open(chrome: &mut Chrome, site: &Site, ready: &str) {
    chrome.open(&site.url("/settings"));
    assert!(
        chrome.wait_for(
            &format!(
                "document.documentElement.dataset.navigation === 'ready' && document.querySelector('[data-ready={ready}]') !== null"
            ),
            SHOWN
        ),
        "{ready}: {}",
        chrome.eval("document.body ? document.body.innerText.slice(0, 1000) : '(no body)'")
    );
}

#[test]
fn the_settings_page_draws_every_entry_within_a_desk_and_a_phone_and_offers_no_control() {
    let site = served();
    let Some(mut chrome) = chrome() else { return };
    for (width, height, mobile) in [(1440, 900, false), (390, 844, true)] {
        screen(&mut chrome, width, height, mobile);
        open(&mut chrome, &site, "settings");
        assert_eq!(
            texts(&mut chrome, ".app-pages a.active[aria-current=page]"),
            ["Settings"],
            "{width}"
        );
        assert_eq!(
            texts(&mut chrome, ".settings-section > h2"),
            [
                "Integrations and tools",
                "Models and limits",
                "Shown here, changed by a reviewed task",
                "This device"
            ],
            "{width}"
        );
        assert_eq!(
            texts(&mut chrome, ".setting-entry h3"),
            [
                "Herdr panes",
                "Catalogue",
                "The model each role runs on",
                "Landing",
                "Workflow",
                "Appearance"
            ],
            "{width}"
        );
        assert_eq!(
            texts(&mut chrome, ".settings-problems li"),
            ["symphony.local.toml: [land] check (line 5) is not read"],
            "{width}"
        );
        // A reviewed setting says its file and line; a local value the committed one it is over.
        assert_eq!(
            chrome.eval(
                r#"(() => {
                    const froms = [...document.querySelectorAll('.setting-from')].map(e => e.textContent);
                    const page = document.querySelector('.settings');
                    return {
                        reviewed: froms.includes('[land] · symphony.toml [land], line 36'),
                        over: froms.some(text => text.includes('symphony.local.toml [agents.models] executor, line 2 · over symphony.toml [agents.models] executor, line 16')),
                        controls: page.querySelectorAll('button, input, select, textarea, form, a, [role=switch], [contenteditable]').length,
                        withinTheScreen: document.documentElement.scrollWidth <= innerWidth,
                        withinThePage: [...page.querySelectorAll('*')].every(e => e.getBoundingClientRect().right <= innerWidth + 0.5),
                    };
                })()"#
            ),
            json!({"reviewed":true,"over":true,"controls":0,"withinTheScreen":true,"withinThePage":true}),
            "{width}"
        );
    }
    // The page asked for its one read and sent nothing.
    assert!(
        site.requests()
            .iter()
            .any(|asked| asked.contains(symphony_api::SETTINGS_PATH))
    );
    assert_eq!(site.posts(), []);
}

#[test]
fn a_settings_read_that_fails_is_said_in_the_pages_place_and_one_missing_as_missing() {
    let site = served();
    let Some(mut chrome) = chrome() else { return };
    screen(&mut chrome, 1440, 900, false);
    site.fail_route(symphony_api::SETTINGS_PATH, true);
    open(&mut chrome, &site, "failed");
    let said = texts(&mut chrome, ".settings-error");
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("The settings could not be read.")
            && said[0].ends_with("Trying again…"),
        "{said:?}"
    );
    assert_eq!(texts(&mut chrome, ".setting-entry h3"), [""; 0]);
    // The page asks again by itself, and draws what the server then answers.
    site.fail_route(symphony_api::SETTINGS_PATH, false);
    assert!(
        chrome.wait_for(
            "document.querySelector('[data-ready=settings]') !== null && document.querySelector('.settings-error') === null",
            SHOWN
        ),
        "{}",
        chrome.eval("document.body.innerText.slice(0, 1000)")
    );
    site.miss_route(
        symphony_api::SETTINGS_PATH,
        Some("this server has no settings page"),
    );
    open(&mut chrome, &site, "missing");
    let said = texts(&mut chrome, ".settings-error");
    assert!(
        said[0].contains("this server has no settings page") && !said[0].contains("Trying again"),
        "{said:?}"
    );
}
