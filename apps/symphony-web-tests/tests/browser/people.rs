//! The People page (apps/symphony-web/src/people.tsx, the web client's `people` kind), the release
//! client against the stand-in server answering its read (`/api/people`) with the wire's own
//! sample: on a desk and a phone the person at the machine's own section first, then every person,
//! group and change drawn within the screen's width, the page lit in the header after Settings, a
//! power the board gives no one yet offered by no control, each field labelled, and nothing sent
//! on opening; the person at the machine naming themselves from that section, asked once more
//! before it is sent (`/api/people-profile`), the header then naming them and linking to it; an
//! add of someone else from its form (its fields cleared once it is done) and a power's toggle
//! posted as one change each (`/api/people-change`), the page drawn from the write's answer and
//! what it did said on the part that sent it, a refusal in the server's words, and no form
//! sending a power given to no one; a person put in a group, a group made and a
//! removal (asked by a named confirmation, kept or confirmed) each posted as one change; a reader
//! without administer, who is not the person at the machine, shown no section of theirs to name
//! and no control but their own profile's and notices', which post to
//! `/api/people-profile`, and no one's addresses but their own; a person's notices set on their
//! card, what changed posted alone; and a read that fails said in the page's place.
//!
//! With `SYMPHONY_PEOPLE_SHOTS` naming a folder, the reader's page and a person's notices being
//! set are saved there, whole, at each width and in each scheme: a visitor who is not the person at the machine reaches a server only
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
            ["You", "People", "Groups", "Log"]
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
                        adds: document.querySelectorAll('form[aria-label="Add someone else"], form[aria-label="Make a group"]').length,
                        // Given to no one yet: no control offers it, and no word explains it.
                        withheld: [...page.querySelectorAll('button')].filter(b => /^direct agents/.test(b.getAttribute('aria-label') ?? '')).length,
                        hints: page.querySelectorAll('.people-hint').length,
                        // The person at the machine, named by no profile yet, is shown the form that names them.
                        you: [document.querySelector('#you .people-state')?.textContent, [...document.querySelectorAll('#you label')].map(l => l.textContent)],
                        labels: [...document.querySelectorAll('#people-add label')].map(l => l.textContent),
                        removals: [...document.querySelectorAll('.people-actions > .dx-button')].map(b => `${b.textContent}:${b.dataset.style}`),
                        withinTheScreen: document.documentElement.scrollWidth <= innerWidth,
                        withinThePage: [...page.querySelectorAll('*')].every(e => e.getBoundingClientRect().right <= innerWidth + 0.5),
                    };
                })()"#
            ),
            json!({
                "reach": "Signs in by tailscale, github · git ana-lima",
                "held": ["read:own", "speak:own", "decide:group", "administer:none"],
                "log": [5, 4, 3, 2, 1],
                "newest": true,
                "adds": 2,
                "withheld": 0,
                "hints": 0,
                "you": [
                    "You write as owner.",
                    ["Short name", "Display name (optional)", "Git handles (optional)"],
                ],
                "labels": [
                    "Short name", "Display name (optional)",
                    "Sign-in, to open boards from their own device (optional)",
                    "Git handles (optional)", "Group (optional)",
                ],
                "removals": [
                    "Take off the board:outline", "Take off the board:outline",
                    "Take off the board:outline", "Remove the group:outline",
                ],
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

/// The person at the machine names themselves from their own section: asked once more, as a short
/// name is never changed once given, then posted alone; the page and the header then name them by
/// how they are shown, the header linking to that section.
#[test]
fn the_person_at_the_machine_names_themselves_asked_once_more_and_the_header_names_them() {
    let site = served(&sample());
    let owner = symphony_api::PersonName {
        name: "owner".into(),
        shown: "owner".into(),
    };
    site.serve_to(owner, "/people#you");
    let mut after = sample();
    after["mine"] = json!({"name": "stefano", "display": "Stefano", "git": ["stefano-b"]});
    site.set_json(symphony_api::PROFILE_PATH, &after);
    let Some(mut chrome) = chrome() else { return };
    for (width, height, mobile) in [(1440, 900, false), (390, 844, true)] {
        screen(&mut chrome, width, height, mobile);
        open(&mut chrome, &site, "people");
        let view = if mobile { "phone" } else { "wide" };
        shot(
            &mut chrome,
            &format!("people-you-unnamed-{view}"),
            (width, height, mobile),
        );
    }
    screen(&mut chrome, 1440, 900, false);
    open(&mut chrome, &site, "people");
    let header = r#"(() => {
        const link = document.querySelector('.app-you');
        return [link.textContent, new URL(link.href).pathname + new URL(link.href).hash];
    })()"#;
    assert_eq!(
        chrome.eval(header),
        json!(["owner", "/people#you"]),
        "the header names the reader as the server named them"
    );
    type_into(&mut chrome, "Your short name", "stefano");
    type_into(&mut chrome, "Your display name", "Stefano");
    type_into(&mut chrome, "Your git handles", "stefano-b");
    let keep = "document.querySelector(\"form[aria-label='Your name']\").requestSubmit()";
    chrome.eval(keep);
    let asked =
        "document.querySelector(\"#you [role=group][aria-label='Your short name: confirmation']\")";
    assert!(chrome.wait_for(&format!("{asked} !== null"), SHOWN));
    assert_eq!(
        chrome.eval(&format!("{asked}.querySelector('span').textContent")),
        json!("Write as stefano on every board? A short name is never changed once given.")
    );
    press(&mut chrome, "you", "Change it");
    assert_eq!(chrome.eval(&format!("{asked} === null")), json!(true));
    assert_eq!(
        site.posts(),
        [],
        "neither the first press nor changing it sends anything"
    );
    chrome.eval(keep);
    assert!(chrome.wait_for(&format!("{asked} !== null"), SHOWN));
    press(&mut chrome, "you", "Confirm the name");
    assert_eq!(said(&mut chrome, "you", false), "The profile is written.");
    assert_eq!(
        site.posts(),
        [(
            symphony_api::PROFILE_PATH.to_string(),
            json!({"name": "stefano", "display": "Stefano", "git": ["stefano-b"], "machine": true})
        )]
    );
    assert!(chrome.wait_for(
        "document.querySelector('.app-you')?.textContent === 'Stefano'",
        SHOWN
    ));
    assert_eq!(
        chrome.eval(
            r#"(() => {
                const link = document.querySelector('.app-you');
                return {
                    you: [...document.querySelectorAll('#you h3, #you .people-name')].map(e => e.textContent),
                    form: document.querySelector("form[aria-label='Your name']") === null,
                    href: new URL(link.href).pathname + new URL(link.href).hash,
                    label: link.getAttribute('aria-label'),
                };
            })()"#
        ),
        json!({
            "you": ["Stefano", "stefano"], "form": true, "href": "/people#you",
            "label": "Stefano, your profile",
        })
    );
    for (width, height, mobile) in [(1440, 900, false), (390, 844, true)] {
        screen(&mut chrome, width, height, mobile);
        let view = if mobile { "phone" } else { "wide" };
        assert_eq!(
            chrome.eval("document.documentElement.scrollWidth <= innerWidth"),
            json!(true),
            "{view}"
        );
        // The name at the row's end beside the bell: whole on a desk, a round initial on a phone.
        assert_eq!(
            chrome.eval(
                r#"(() => {
                    const link = document.querySelector('.app-you');
                    const box = link.getBoundingClientRect();
                    const bell = document.querySelector('#bell').getBoundingClientRect();
                    return {
                        size: innerWidth < 761 ? [Math.round(box.width), Math.round(box.height)] : link.scrollWidth <= link.clientWidth && box.width > 0,
                        row: Math.abs(box.top + box.bottom - bell.top - bell.bottom) < 4,
                    };
                })()"#
            ),
            if mobile {
                json!({"size": [32, 32], "row": true})
            } else {
                json!({"size": true, "row": true})
            },
            "{view}"
        );
        shot(
            &mut chrome,
            &format!("people-you-named-{view}"),
            (width, height, mobile),
        );
    }
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
                "form[aria-label='Add someone else'] button[aria-label='{power} for the new person']"
            ),
        );
    }
    chrome.eval("document.querySelector(\"form[aria-label='Add someone else']\").requestSubmit()");
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
    // The page is the write's answer: no read follows it; the form is cleared for the next one.
    assert!(chrome.wait_for("document.querySelector('#person-dora') !== null", SHOWN));
    assert!(chrome.wait_for(
        "[...document.querySelectorAll(\"form[aria-label='Add someone else'] input\")].every(i => i.value === '')",
        SHOWN
    ));
    // Direct agents, given to no one yet, is offered by no control on anyone's card.
    assert_eq!(
        chrome.eval(
            "document.querySelector(\"#person-bo [aria-label='direct agents for bo']\") === null"
        ),
        json!(true)
    );
    // A power's toggle is one change of the person's own; a refusal is said on their card.
    let refusal = "Forbidden: ana may not administer on this board";
    site.refuse_route(symphony_api::PEOPLE_CHANGE_PATH, Some(refusal));
    click(&mut chrome, "#person-bo button[aria-label='decide for bo']");
    assert!(said(&mut chrome, "person-bo", true).contains(refusal));
    assert_eq!(
        site.posts()[1..],
        [(
            symphony_api::PEOPLE_CHANGE_PATH.to_string(),
            json!({"action": "grant", "name": "bo", "powers": ["decide"]})
        )]
    );
    assert_eq!(
        texts(&mut chrome, "#person-ana .people-said"),
        ["", ""],
        "said on the part that sent it alone: not on ana's card, nor her notices"
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
    // A removal asks by a named confirmation, which keeps or sends it.
    press(&mut chrome, "person-cy", "Take off the board");
    assert_eq!(site.posts().len(), 2, "the first press sends nothing");
    let asked = "document.querySelector(\"#person-cy [role=group][aria-label='Take off the board: confirmation']\")";
    assert_eq!(
        chrome.eval(&format!("{asked}?.querySelector('span').textContent")),
        json!("Take cy off the board?")
    );
    press(&mut chrome, "person-cy", "Keep cy");
    assert_eq!(chrome.eval(&format!("{asked} === null")), json!(true));
    press(&mut chrome, "person-cy", "Take off the board");
    assert_eq!(site.posts().len(), 2, "nor does keeping");
    press(&mut chrome, "person-cy", "Confirm removal");
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
    // The server sends the person at the machine's own profile to them alone.
    reader.as_object_mut().unwrap().remove("mine");
    // As the server answers cy: no one's addresses or ways but cy's own.
    for person in reader["people"].as_array_mut().unwrap() {
        if person["name"] != "cy" {
            let person = person.as_object_mut().unwrap();
            person.remove("addresses");
            person.remove("notices");
        }
    }
    let site = served(&reader);
    site.set_json(symphony_api::PROFILE_PATH, &reader);
    let cy = symphony_api::PersonName {
        name: "cy".into(),
        shown: "Cy Moreau".into(),
    };
    site.serve_to(cy, "/people#person-cy");
    let Some(mut chrome) = chrome() else { return };
    for (width, height, mobile) in [(1440, 900, false), (390, 844, true)] {
        screen(&mut chrome, width, height, mobile);
        open(&mut chrome, &site, "people");
        assert_eq!(
            chrome.eval(
                r#"(() => {
                    const link = document.querySelector('.app-you');
                    return [link.textContent, link.getAttribute('aria-label'), new URL(link.href).pathname + new URL(link.href).hash];
                })()"#
            ),
            json!(["Cy Moreau", "Cy Moreau, your profile", "/people#person-cy"]),
            "the header names cy and links to their own card"
        );
        assert_eq!(
            chrome.eval(
                r#"(() => {
                    const page = document.querySelector('.people');
                    const controls = [...page.querySelectorAll('button, input, select, form, details')];
                    return {
                        outside: controls.filter(e => !e.closest('#person-cy .people-profile')).length,
                        signin: document.querySelector('input[aria-label="Sign-in of cy"]') !== null,
                        you: document.querySelector('#person-cy .people-you')?.textContent,
                        // Not the person at the machine: no section of theirs to name, nor its form.
                        mine: [document.querySelector('#you'), document.querySelector("form[aria-label='Your name']")].every(e => e === null),
                        addresses: [...document.querySelectorAll('.people-addresses')].map(p => p.closest('.people-person').id),
                        notices: [...document.querySelectorAll('.people-notices')].map(d => d.closest('.people-person').id),
                        said: document.querySelector('.people-refused').textContent === 'Changing who is on the board needs administer.',
                        // What a group holds, read among it, as each of its powers includes read.
                        staff: [...document.querySelectorAll('#group-staff .people-power')].map(t => `${t.textContent}:${t.dataset.held}`),
                        bo: [...document.querySelectorAll('#person-bo .people-power')].map(t => `${t.textContent}:${t.dataset.held}`),
                    };
                })()"#
            ),
            json!({
                "outside": 0, "signin": false, "you": "you", "mine": true, "said": true,
                "addresses": ["person-cy"], "notices": ["person-cy"],
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
fn a_persons_notices_are_set_on_their_card_and_posted_as_what_changed() {
    let site = served(&sample());
    let mut after = sample();
    after["people"][0]["addresses"]["whatsapp"] = json!("+15551234567");
    after["people"][0]["notices"][1]["way"] = json!("whatsapp");
    site.set_json(symphony_api::PROFILE_PATH, &after);
    let Some(mut chrome) = chrome() else { return };
    for (width, height, mobile) in [(1440, 900, false), (390, 844, true)] {
        screen(&mut chrome, width, height, mobile);
        open(&mut chrome, &site, "people");
        click(&mut chrome, "#person-ana .people-notices summary");
        assert_eq!(
            chrome.eval(
                r#"(() => {
                    const page = document.querySelector('.people');
                    const ana = document.querySelector('#person-ana');
                    const form = ana.querySelector('form[aria-label="Notices of ana"]');
                    return {
                        addresses: ana.querySelector('.people-addresses').textContent,
                        held: [...form.querySelectorAll('input')].map(i => `${i.getAttribute('aria-label')}=${i.value}`),
                        ways: [...form.querySelectorAll('.people-way')].map(w => w.textContent),
                        keep: form.querySelector('button[type=submit]') !== null,
                        // cy has no address: said so, and their form is there to set one.
                        cy: document.querySelector('#person-cy .people-addresses').textContent,
                        forms: [...document.querySelectorAll('.people-notices')].map(d => d.closest('.people-person').id),
                        withinTheScreen: document.documentElement.scrollWidth <= innerWidth,
                        withinThePage: [...page.querySelectorAll('*')].every(e => e.getBoundingClientRect().right <= innerWidth + 0.5),
                    };
                })()"#
            ),
            json!({
                "addresses": "Notices reach them by email ana@example.com · Slack U024BE7LH",
                "held": [
                    "Email for notices of ana=ana@example.com",
                    "Slack member id of ana=U024BE7LH",
                    "WhatsApp number of ana=",
                ],
                "ways": ["CardsSlack", "Questionsthe page alone", "Verdictsemail"],
                "keep": false,
                "cy": "No address for notices",
                "forms": ["person-ana", "person-cy"],
                "withinTheScreen": true,
                "withinThePage": true,
            }),
            "{width}"
        );
        type_into(&mut chrome, "WhatsApp number of ana", "+1 555 123 4567");
        click(&mut chrome, "[aria-label='Questions for ana by']");
        pick(&mut chrome, "WhatsApp");
        assert!(
            chrome.wait_for(
                "document.querySelector('form[aria-label=\"Notices of ana\"] button[type=submit]') !== null",
                SHOWN
            ),
            "{width}: a change, and Keep is there to keep it"
        );
        let view = if mobile { "phone" } else { "wide" };
        shot(
            &mut chrome,
            &format!("people-notices-{view}"),
            (width, height, mobile),
        );
    }
    chrome.eval("document.querySelector('form[aria-label=\"Notices of ana\"]').requestSubmit()");
    assert_eq!(
        said(&mut chrome, "person-ana", false),
        "The profile is written."
    );
    assert_eq!(
        site.posts(),
        [(
            symphony_api::PROFILE_PATH.to_string(),
            json!({
                "name": "ana",
                "whatsapp": "+1 555 123 4567",
                "notices": [{"kind": "question", "way": "whatsapp"}],
            })
        )]
    );
    // Drawn again from what the write left held, the form has nothing left to keep.
    assert!(chrome.wait_for(
        "document.querySelector('form[aria-label=\"Notices of ana\"] button[type=submit]') === null",
        SHOWN
    ));
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
