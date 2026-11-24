//! The Settings page (apps/symphony-web/src/settings.tsx, the web client's `settings` kind), the
//! release client against the stand-in server answering its read (`/api/settings`) with the
//! wire's own sample: on a desk and a phone every part and entry drawn in the server's order
//! within the screen's width, lit in the header, a control for each preference the board's store
//! keeps and none elsewhere, and nothing sent on opening; a value kept posted to its write
//! (`/api/setting`), the page read again after it, a field's draft kept across a reload and a
//! refusal said on its row; and a read that fails said in the page's place.
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
fn the_settings_page_draws_every_entry_within_a_desk_and_a_phone_with_the_stores_controls() {
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
                "Reviewed settings",
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
        // A reviewed setting says its file and line; a local value the store's it is over, and
        // the store's the committed one. A control is drawn for each preference the store keeps,
        // in the models entry, and none anywhere else.
        assert_eq!(
            chrome.eval(
                r#"(() => {
                    const froms = [...document.querySelectorAll('.setting-from')].map(e => e.textContent);
                    const page = document.querySelector('.settings');
                    const controls = 'button, input, select, textarea, form, a, [role=switch], [contenteditable]';
                    return {
                        reviewed: froms.includes('[land] · symphony.toml [land], line 36'),
                        over: froms.includes('[agents.models] executor · symphony.local.toml [agents.models] executor, line 2 · over "opus" (the board\'s store [agents.models] executor)'),
                        stored: froms.includes("[limits] low_volume · the board's store [limits] low_volume · over 4 (symphony.toml [limits] low_volume, line 12)"),
                        rows: page.querySelectorAll('#setting-models .setting-control').length,
                        outside: [...page.querySelectorAll(controls)].filter(e => !e.closest('.setting-control')).length,
                        waiting: [...page.querySelectorAll('.setting-waiting')].map(e => e.textContent.startsWith('Waiting for its delivery by pull request (docs/owner-save-4')),
                        withinTheScreen: document.documentElement.scrollWidth <= innerWidth,
                        withinThePage: [...page.querySelectorAll('*')].every(e => e.getBoundingClientRect().right <= innerWidth + 0.5),
                    };
                })()"#
            ),
            json!({"reviewed":true,"over":true,"stored":true,"rows":3,"outside":0,"waiting":[true],"withinTheScreen":true,"withinThePage":true}),
            "{width}"
        );
        // The page explains nothing of itself: its head is its title, with no legend of the tags
        // and no line under a section's heading or an entry's name but its state.
        assert_eq!(
            chrome.eval(
                r#"(() => ({
                    head: document.querySelector('.settings-head').textContent,
                    legend: document.querySelector('.settings-legend') !== null,
                    leads: document.querySelectorAll('.settings-section > h2 + p').length,
                    abouts: document.querySelectorAll('.setting-about').length,
                }))()"#
            ),
            json!({"head":"Settings","legend":false,"leads":0,"abouts":0}),
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

/// The row of the setting the page names `key`, as a script finds it.
fn row(key: &str) -> String {
    format!(
        "[...document.querySelectorAll('.setting-row')].find(r => r.querySelector('.setting-from')?.textContent.startsWith({key:?} + ' · '))"
    )
}

/// `text` typed into the field of the setting the page names `key`, as an input event gives it.
fn type_into(chrome: &mut Chrome, key: &str, text: &str) {
    chrome.eval(&format!(
        "(() => {{ const input = {}.querySelector('input'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, {text:?}); input.dispatchEvent(new Event('input', {{bubbles: true}})); }})()",
        row(key)
    ));
}

/// A click of the button of the setting the page names `key` that reads `label`.
fn press(chrome: &mut Chrome, key: &str, label: &str) {
    chrome.eval(&format!(
        "[...{}.querySelectorAll('button')].find(b => b.textContent === {label:?}).click()",
        row(key)
    ));
}

/// What the row of the setting the page names `key` last said of a write: its text and whether it
/// was refused, once it says one.
fn said(chrome: &mut Chrome, key: &str) -> Value {
    let found = format!("{}?.querySelector('.setting-said')", row(key));
    assert!(
        chrome.wait_for(&format!("{found} != null"), SHOWN),
        "{key}: {}",
        chrome.eval("document.body.innerText.slice(0, 1000)")
    );
    chrome.eval(&format!(
        "(() => {{ const p = {found}; return [p.textContent, p.dataset.refused, p.getAttribute('role')]; }})()"
    ))
}

/// How many times the page read its settings.
fn reads(site: &Site) -> usize {
    (site.requests().iter())
        .filter(|asked| asked.contains(symphony_api::SETTINGS_PATH))
        .count()
}

#[test]
fn a_kept_value_is_posted_and_read_again_its_draft_kept_and_a_refusal_said_on_its_row() {
    let site = served();
    let kept = "[limits] low_volume is 7 in the board's store, in force at once unless \
                symphony.local.toml writes it";
    site.set_json(
        symphony_api::SETTING_WRITE_PATH,
        &json!({"said": kept, "waiting": false}),
    );
    let Some(mut chrome) = chrome() else { return };
    screen(&mut chrome, 1440, 900, false);
    open(&mut chrome, &site, "settings");
    // Its field opens on the value in force; Keep waits for a change, and a commit waits for the
    // one already waiting for its delivery.
    assert_eq!(
        chrome.eval(&format!(
            "(() => {{ const r = {}; return [r.querySelector('input').value, r.querySelector('button[type=submit]').disabled, [...r.querySelectorAll('button')].find(b => b.textContent === \"Make it the project's\").disabled]; }})()",
            row("[limits] low_volume")
        )),
        json!(["3", true, true])
    );
    // The store's record opens on a click, its newest change first.
    chrome.eval(&format!(
        "{}.querySelector('.setting-record summary').click()",
        row("[limits] low_volume")
    ));
    let changes = format!(
        "{}.querySelectorAll('.setting-record li')",
        row("[limits] low_volume")
    );
    assert!(
        chrome.wait_for(&format!("{changes}.length === 2"), SHOWN),
        "the record"
    );
    assert_eq!(
        chrome.eval(&format!("[...{changes}].map(li => li.textContent)")),
        json!([
            "2026-11-22 20:15 · owner: 2 → 3",
            "2026-11-22 20:10 · owner: none → 2"
        ])
    );
    // A field's text is a draft on this device, kept across a reload.
    type_into(&mut chrome, "[limits] low_volume", "7");
    open(&mut chrome, &site, "settings");
    assert_eq!(
        chrome.eval(&format!(
            "{}.querySelector('input').value",
            row("[limits] low_volume")
        )),
        json!("7")
    );
    assert_eq!(site.posts(), []);
    // What the server reads after the write: the store holds 7.
    let mut after = sample();
    after["sections"][1]["entries"][0]["settings"][2]["value"] = json!("7");
    assert_eq!(
        after["sections"][1]["entries"][0]["settings"][2]["key"],
        "[limits] low_volume"
    );
    site.set_json(symphony_api::SETTINGS_PATH, &after);
    let before = reads(&site);
    chrome.eval(&format!(
        "{}.querySelector('form').requestSubmit()",
        row("[limits] low_volume")
    ));
    assert_eq!(
        said(&mut chrome, "[limits] low_volume"),
        json!([kept, "false", "status"])
    );
    assert_eq!(
        site.posts(),
        [(
            symphony_api::SETTING_WRITE_PATH.to_string(),
            json!({"key": "limits.low_volume", "action": "store", "value": 7})
        )]
    );
    // The page reads its settings again after the write and draws what it read, the draft gone.
    assert!(
        chrome.wait_for(
            &format!(
                "{}.querySelector('.setting-value').textContent === '7'",
                row("[limits] low_volume")
            ),
            SHOWN
        ),
        "the read after the write: {}",
        chrome.eval("document.body.innerText.slice(0, 1000)")
    );
    assert!(reads(&site) > before);
    assert_eq!(
        chrome.eval(&format!(
            "(() => {{ const r = {}; return [r.querySelector('input').value, r.querySelector('button[type=submit]').disabled]; }})()",
            row("[limits] low_volume")
        )),
        json!(["7", true])
    );
    // Remove and commit name the setting alone; a refusal is said on the row in its words.
    press(&mut chrome, "[agents.models] executor", "Remove");
    assert!(
        chrome.wait_for(
            &format!(
                "{}?.querySelector('.setting-said') != null",
                row("[agents.models] executor")
            ),
            SHOWN
        ),
        "remove answered"
    );
    let refusal = "not committed: the settings check refuses symphony.toml as it would be";
    site.miss_route(symphony_api::SETTING_WRITE_PATH, Some(refusal));
    press(
        &mut chrome,
        "[agents.models] executor",
        "Make it the project's",
    );
    assert!(
        chrome.wait_for(
            &format!(
                "{}?.querySelector('.setting-said[data-refused=true]') != null",
                row("[agents.models] executor")
            ),
            SHOWN
        ),
        "the refusal"
    );
    let shown = said(&mut chrome, "[agents.models] executor");
    assert!(
        shown[0].as_str().unwrap().contains(refusal) && shown[1] == "true" && shown[2] == "alert",
        "{shown}"
    );
    let posted: Vec<Value> = site.posts().into_iter().map(|(_, body)| body).collect();
    assert_eq!(
        posted[1..],
        [
            json!({"key": "agents.models.executor", "action": "remove"}),
            json!({"key": "agents.models.executor", "action": "commit"}),
        ]
    );
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
