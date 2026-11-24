//! The People page (apps/symphony-web/src/people.tsx, the web client's `people` kind), the release
//! client against the stand-in server answering its read (`/api/people`) with the wire's own
//! sample: on a desk and a phone every person, group and change drawn within the screen's width,
//! the page lit in the header after Settings, and nothing sent on opening; an add from its form
//! and a power's toggle posted as one change each (`/api/people-change`), the page drawn from the
//! write's answer and what it did said on the part that sent it, a refusal in the server's words;
//! a person put in a group, a group made and a removal (asked once, sent at its second press) each
//! posted as one change; a reader without administer shown no control but their own profile's, which posts to
//! `/api/people-profile`; and a read that fails said in the page's place.
//!
//! With `SYMPHONY_PEOPLE_SHOTS` naming a folder, the reader's page is saved there, whole, at each
//! width and in each scheme: a visitor who is not the person at the machine reaches a server only
//! under another Tailscale login, so their page is pictured from the stand-in server.
use super::{SHOWN, chrome, chrome::Chrome, site, site::Site, texts};
use serde_json::{Value, json};

/// The wire's sample of the page (`wire.cases.json`'s `samples.people`), as the person at the
/// machine reads it.
fn sample() -> Value {
    let cases: Value =
        serde_json::from_str(include_str!("../../../symphony-web/src/wire.cases.json"))
            .expect("the wire's cases");
    cases["samples"]["people"].clone()
}

fn served(page: &Value) -> Site {
    let site = site();
    site.set_json(symphony_api::PEOPLE_PATH, page);
    site
}

fn screen(chrome: &mut Chrome, width: u32, height: u32, mobile: bool) {
    chrome.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":if mobile { 3 } else { 1 },"mobile":mobile}),
    );
}

fn open(chrome: &mut Chrome, site: &Site, ready: &str) {
    chrome.open(&site.url("/people"));
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

/// What the part `part` (`person-ana`, `people-add`) last said of a write, once it says one: its text
/// and whether it was refused.
fn said(chrome: &mut Chrome, part: &str, refused: bool) -> String {
    let found = format!("document.querySelector('#{part} .people-said[data-refused={refused}]')");
    assert!(
        chrome.wait_for(&format!("{found} !== null"), SHOWN),
        "{part}: {}",
        chrome.eval("document.body.innerText.slice(0, 2000)")
    );
    chrome
        .eval(&format!("{found}.textContent"))
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// `text` typed into the field labelled `label`, as an input event gives it.
fn type_into(chrome: &mut Chrome, label: &str, text: &str) {
    chrome.eval(&format!(
        "(() => {{ const input = document.querySelector('input[aria-label={label:?}]'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, {text:?}); input.dispatchEvent(new Event('input', {{bubbles: true}})); }})()"
    ));
}

/// The whole page, its window made as tall as it is, saved as `NAME-light.png` and
/// `NAME-dark.png` in the folder `SYMPHONY_PEOPLE_SHOTS` names; nothing without one. The window and
/// the scheme are put back as they were.
fn shot(chrome: &mut Chrome, name: &str, (width, height, mobile): (u32, u32, bool)) {
    let Some(folder) = std::env::var_os("SYMPHONY_PEOPLE_SHOTS") else {
        return;
    };
    let folder = std::path::PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    let tall = chrome
        .eval("Math.ceil(document.documentElement.scrollHeight)")
        .as_u64()
        .unwrap() as u32;
    screen(chrome, width, tall.max(height), mobile);
    for scheme in ["light", "dark"] {
        chrome.scheme(scheme);
        chrome.frames_with_fonts();
        // The colours' transitions into the scheme, finished.
        chrome.wait_for("document.getAnimations().length === 0", SHOWN);
        std::fs::write(
            folder.join(format!("{name}-{scheme}.png")),
            chrome.screenshot(),
        )
        .unwrap();
    }
    chrome.call("Emulation.setEmulatedMedia", json!({"features": []}));
    screen(chrome, width, height, mobile);
}

fn click(chrome: &mut Chrome, selector: &str) {
    chrome.eval(&format!(
        "document.querySelector({}).click()",
        json!(selector)
    ));
}

#[test]
fn the_people_page_draws_every_person_group_and_change_within_a_desk_and_a_phone() {
    let site = served(&sample());
    let Some(mut chrome) = chrome() else { return };
    for (width, height, mobile) in [(1440, 900, false), (390, 844, true)] {
        screen(&mut chrome, width, height, mobile);
        open(&mut chrome, &site, "people");
        assert_eq!(
            texts(&mut chrome, ".app-pages a.active[aria-current=page]"),
            ["People"],
            "{width}"
        );
        let pages = texts(&mut chrome, ".app-pages a");
        let settings = pages.iter().position(|page| page == "Settings");
        assert_eq!(
            settings
                .and_then(|at| pages.get(at + 1))
                .map(String::as_str),
            Some("People"),
            "{pages:?}"
        );
        assert_eq!(
            texts(&mut chrome, ".people-section > h2"),
            ["People", "Groups", "Log"]
        );
        assert_eq!(
            texts(&mut chrome, ".people-person-head h3"),
            ["Ana Lima", "bo", "Cy Okafor", "staff"]
        );
        assert_eq!(
            chrome.eval(
                r#"(() => {
                    const page = document.querySelector('.people');
                    const ana = document.querySelector('#person-ana');
                    return {
                        reach: ana.querySelector('.people-reach').textContent,
                        held: [...ana.querySelectorAll('.people-power')].map(b => `${b.textContent}:${b.dataset.held}`),
                        log: [...document.querySelectorAll('.people-log li')].map(li => li.value),
                        newest: document.querySelector('.people-log li').textContent.endsWith('owner put cy on the board, holding read'),
                        adds: document.querySelectorAll('form[aria-label="Add a person"], form[aria-label="Make a group"]').length,
                        withinTheScreen: document.documentElement.scrollWidth <= innerWidth,
                        withinThePage: [...page.querySelectorAll('*')].every(e => e.getBoundingClientRect().right <= innerWidth + 0.5),
                    };
                })()"#
            ),
            json!({
                "reach": "Signs in by tailscale, github · git ana-lima",
                "held": ["read:own", "speak:own", "decide:group", "direct agents:none", "administer:none"],
                "log": [5, 4, 3, 2, 1],
                "newest": true,
                "adds": 2,
                "withinTheScreen": true,
                "withinThePage": true,
            }),
            "{width}"
        );
    }
    assert!(
        site.requests()
            .iter()
            .any(|asked| asked.contains(symphony_api::PEOPLE_PATH))
    );
    assert_eq!(site.posts(), []);
}

#[test]
fn an_add_and_a_power_are_each_one_change_the_page_draws_from_its_answer_a_refusal_said() {
    let site = served(&sample());
    // What the server answers the add: Dora on the board, and the log's sixth change.
    let mut after = sample();
    after["people"].as_array_mut().unwrap().push(json!({
        "name": "dora", "display": "Dora", "powers": ["read", "speak", "decide"],
        "own": ["speak", "decide"], "groups": [], "signins": ["tailscale"], "git": [],
        "profile": true,
    }));
    after["log"].as_array_mut().unwrap().push(json!({
        "n": 6, "at": 1_763_025_000.0, "by": "owner", "did": "add", "name": "dora",
        "powers": ["speak", "decide"],
    }));
    after["revision"] = json!(6);
    site.set_json(symphony_api::PEOPLE_CHANGE_PATH, &after);
    let Some(mut chrome) = chrome() else { return };
    screen(&mut chrome, 1440, 900, false);
    open(&mut chrome, &site, "people");
    type_into(&mut chrome, "Short name", "dora");
    type_into(&mut chrome, "Display name", "Dora");
    type_into(&mut chrome, "Sign-in", "tailscale:dora@example.com");
    for power in ["speak", "decide"] {
        click(
            &mut chrome,
            &format!(
                "form[aria-label='Add a person'] button[aria-label='{power} for the new person']"
            ),
        );
    }
    chrome.eval("document.querySelector(\"form[aria-label='Add a person']\").requestSubmit()");
    assert_eq!(
        said(&mut chrome, "people-add", false),
        "owner put dora on the board, holding speak, decide."
    );
    assert_eq!(
        site.posts(),
        [(
            symphony_api::PEOPLE_CHANGE_PATH.to_string(),
            json!({
                "action": "add", "name": "dora", "display": "Dora",
                "powers": ["read", "speak", "decide"], "groups": [],
                "signins": ["tailscale:dora@example.com"], "git": [],
            })
        )]
    );
    // The page is the write's answer: no read follows it.
    assert!(chrome.wait_for("document.querySelector('#person-dora') !== null", SHOWN));
    // A power's toggle is one change of the person's own; a refusal is said on their card.
    let refusal = "Forbidden: direct agents is given to no one yet";
    site.refuse_route(symphony_api::PEOPLE_CHANGE_PATH, Some(refusal));
    click(
        &mut chrome,
        "#person-bo button[aria-label='direct agents for bo']",
    );
    assert!(said(&mut chrome, "person-bo", true).contains(refusal));
    assert_eq!(
        site.posts()[1],
        (
            symphony_api::PEOPLE_CHANGE_PATH.to_string(),
            json!({"action": "grant", "name": "bo", "powers": ["direct_agents"]})
        )
    );
    assert_eq!(
        texts(&mut chrome, "#person-ana .people-said"),
        [""],
        "said on the part that sent it alone"
    );
}

/// A press of the mouse on the option `text` of the select open now, once its list is placed.
fn pick(chrome: &mut Chrome, text: &str) {
    super::components::popup_ready(chrome);
    let point = chrome.eval(&format!(
        "(() => {{ const option = [...document.querySelectorAll('[role=listbox] [role=option]')].find(o => o.textContent.trim() === {}); const box = option.getBoundingClientRect(); return {{x: box.x + box.width / 2, y: box.y + box.height / 2}}; }})()",
        json!(text)
    ));
    for kind in ["mousePressed", "mouseReleased"] {
        chrome.call(
            "Input.dispatchMouseEvent",
            json!({"type": kind, "x": point["x"], "y": point["y"], "button": "left", "clickCount": 1}),
        );
    }
}

/// The button of `part` reading `text`, pressed.
fn press(chrome: &mut Chrome, part: &str, text: &str) {
    let pressed = chrome.eval(&format!(
        "(() => {{ const button = [...document.querySelectorAll('#{part} button')].find(b => b.textContent === {} && !b.disabled); button?.click(); return !!button; }})()",
        json!(text)
    ));
    assert_eq!(pressed, json!(true), "no {text} to press on {part}");
}

#[test]
fn a_person_put_in_a_group_a_group_made_and_a_removal_are_each_one_change() {
    let site = served(&sample());
    // What the server answers: bo in staff, the log's sixth change.
    let mut after = sample();
    after["people"][1]["groups"] = json!(["staff"]);
    after["log"].as_array_mut().unwrap().push(json!({
        "n": 6, "at": 1_763_025_000.0, "by": "owner", "did": "join", "name": "bo", "group": "staff",
    }));
    after["revision"] = json!(6);
    site.set_json(symphony_api::PEOPLE_CHANGE_PATH, &after);
    let Some(mut chrome) = chrome() else { return };
    screen(&mut chrome, 1440, 900, false);
    open(&mut chrome, &site, "people");
    click(&mut chrome, "[aria-label='Put bo in a group']");
    pick(&mut chrome, "staff");
    assert_eq!(
        said(&mut chrome, "person-bo", false),
        "owner put bo in staff."
    );
    // In the group now, bo is drawn in it and offered no group to join (the select's list, in the
    // page's overlay, keeps its label: bo's row is asked).
    assert_eq!(
        chrome.eval(
            "[document.querySelector(\"#person-bo [aria-label='Put bo in a group']\") === null, document.querySelector(\"#person-bo [aria-label='Take bo out of staff']\") !== null]"
        ),
        json!([true, true])
    );
    // A group made, with the powers chosen for it.
    type_into(&mut chrome, "Group name", "crew");
    click(
        &mut chrome,
        "#people-add-group button[aria-label='speak for the new group']",
    );
    assert!(chrome.wait_for(
        "[...document.querySelectorAll('#people-add-group button')].some(b => b.textContent === 'Make the group' && !b.disabled)",
        SHOWN
    ));
    press(&mut chrome, "people-add-group", "Make the group");
    // The answer holds no new change, so the part says nothing was written.
    assert_eq!(
        said(&mut chrome, "people-add-group", false),
        "It holds that already: nothing was written."
    );
    // A removal asks once, and is sent at its second press.
    press(&mut chrome, "person-cy", "Take off the board");
    assert_eq!(site.posts().len(), 2, "the first press sends nothing");
    press(&mut chrome, "person-cy", "Take off the board: yes");
    assert!(chrome.wait_for(
        "document.querySelector('#person-cy .people-said[data-refused]') !== null",
        SHOWN
    ));
    let change = symphony_api::PEOPLE_CHANGE_PATH.to_string();
    assert_eq!(
        site.posts(),
        [
            (
                change.clone(),
                json!({"action": "join", "name": "bo", "group": "staff"})
            ),
            (
                change.clone(),
                json!({"action": "group-add", "name": "crew", "powers": ["speak"]})
            ),
            (change, json!({"action": "remove", "name": "cy"})),
        ]
    );
}

#[test]
fn a_reader_without_administer_is_shown_no_control_but_their_own_profile() {
    let mut reader = sample();
    reader["administers"] = json!(false);
    reader["machine"] = json!(false);
    reader["you"] = json!("cy");
    let site = served(&reader);
    site.set_json(symphony_api::PROFILE_PATH, &reader);
    let Some(mut chrome) = chrome() else { return };
    for (width, height, mobile) in [(1440, 900, false), (390, 844, true)] {
        screen(&mut chrome, width, height, mobile);
        open(&mut chrome, &site, "people");
        assert_eq!(
            chrome.eval(
                r#"(() => {
                    const page = document.querySelector('.people');
                    const controls = [...page.querySelectorAll('button, input, select, form, details')];
                    return {
                        outside: controls.filter(e => !e.closest('#person-cy .people-profile')).length,
                        signin: document.querySelector('input[aria-label="Sign-in of cy"]') !== null,
                        you: document.querySelector('#person-cy .people-you')?.textContent,
                        said: document.querySelector('.people-sub').textContent.includes('needs administer'),
                        // What a group holds, read among it, as each of its powers includes read.
                        staff: [...document.querySelectorAll('#group-staff .people-power')].map(t => `${t.textContent}:${t.dataset.held}`),
                        bo: [...document.querySelectorAll('#person-bo .people-power')].map(t => `${t.textContent}:${t.dataset.held}`),
                    };
                })()"#
            ),
            json!({
                "outside": 0, "signin": false, "you": "you", "said": true,
                "staff": ["read:own", "decide:own"],
                "bo": ["read:own", "administer:own"],
            }),
            "{width}"
        );
        let view = if mobile { "phone" } else { "wide" };
        shot(
            &mut chrome,
            &format!("people-reader-{view}"),
            (width, height, mobile),
        );
    }
    click(&mut chrome, "#person-cy .people-profile summary");
    type_into(&mut chrome, "Git handles of cy", "cy-o");
    chrome.eval("document.querySelector('#person-cy .people-profile form').requestSubmit()");
    assert_eq!(
        said(&mut chrome, "person-cy", false),
        "The profile is written."
    );
    assert_eq!(
        site.posts(),
        [(
            symphony_api::PROFILE_PATH.to_string(),
            json!({"name": "cy", "git": ["cy-o"], "signins": []})
        )]
    );
}

#[test]
fn a_people_read_that_fails_is_said_in_the_pages_place_and_read_again() {
    let site = served(&sample());
    let Some(mut chrome) = chrome() else { return };
    screen(&mut chrome, 1440, 900, false);
    site.fail_route(symphony_api::PEOPLE_PATH, true);
    open(&mut chrome, &site, "failed");
    let said = texts(&mut chrome, ".people-error");
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("The people could not be read.") && said[0].ends_with("Trying again…"),
        "{said:?}"
    );
    site.fail_route(symphony_api::PEOPLE_PATH, false);
    assert!(
        chrome.wait_for(
            "document.querySelector('[data-ready=people]') !== null",
            SHOWN
        ),
        "read again: {}",
        chrome.eval("document.body.innerText.slice(0, 1000)")
    );
    assert_eq!(texts(&mut chrome, ".people-error"), [""; 0]);
}
