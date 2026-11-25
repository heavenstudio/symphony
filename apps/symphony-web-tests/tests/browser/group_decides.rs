//! What a page draws for a reader who decides a thing and for one who does not
//! (docs/workflow/TASKS.md, "People"), the release client against the stand-in server answering
//! with the owner the server says to each (`Owner.decides`): on a desk and a phone, a task's
//! priority, cancel and approve drawn for one who decides it, and for one who does not its
//! priority alone, as text; a card's verdicts, Dismiss and Later for one who decides it, and for
//! one who does not its message alone; the Settings page's controls for one of its
//! group, and its values alone for anyone else, whose read carries no preference to write.
//!
//! With `SYMPHONY_DECIDES_SHOTS` naming a folder, each page is saved there, whole, at each width
//! and in each scheme: a visitor who is not the person at the machine reaches a server only under
//! another Tailscale login, so their page is pictured from the stand-in server.
use super::{SHOWN, chrome, chrome::Chrome, common, document_cards, look_parity, site::Site};
use serde_json::{Value, json};

const TASK: &str = "/task/visual/in-progress";

/// The desk's and the phone's screens, by the names their pictures take.
const SCREENS: [(&str, u32, u32, bool); 2] =
    [("wide", 1440, 900, false), ("phone", 390, 844, true)];

/// Whose a thing is, as the server says it to a reader who `decides` it or not.
fn owner(decides: bool) -> Value {
    json!({"name":"design","kind":"group","by":"map","said":"from the area visual",
           "hand_to":[],"decides":decides})
}

fn screen(chrome: &mut Chrome, width: u32, height: u32, mobile: bool) {
    chrome.call(
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":if mobile { 3 } else { 1 },"mobile":mobile}),
    );
}

/// The whole page, its window made as tall as it is, saved as `NAME-WIDTH-light.png` and
/// `NAME-WIDTH-dark.png` in the folder `SYMPHONY_DECIDES_SHOTS` names; nothing without one. The
/// window and the scheme are put back as they were.
fn shot(
    chrome: &mut Chrome,
    name: &str,
    (screen_name, width, height, mobile): (&str, u32, u32, bool),
) {
    let Some(folder) = std::env::var_os("SYMPHONY_DECIDES_SHOTS") else {
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
        chrome.wait_for("document.getAnimations().length === 0", SHOWN);
        std::fs::write(
            folder.join(format!("{name}-{screen_name}-{scheme}.png")),
            chrome.screenshot(),
        )
        .unwrap();
    }
    chrome.call("Emulation.setEmulatedMedia", json!({"features": []}));
    screen(chrome, width, height, mobile);
}

/// The look-parity task, in progress, its owner said to a reader who `decides` it, with its
/// approve offered.
fn task_site(decides: bool) -> Site {
    let responses = look_parity::responses();
    let site = super::site();
    let mut page = responses["/api/task-page"].clone();
    page["owner"] = owner(decides);
    site.set_json("/api/task-page", &page);
    site.set_json("/api/task-history", &responses["/api/task-history"]);
    site.set_json(
        "/api/task-offer",
        &json!({"action":"owner-approve","label":"Approve 1a2b3c4","branch":"visual/in-progress","payload":"1a2b3c4"}),
    );
    site.set_file(
        symphony_api::TASK_TIMELINE_PATH,
        common::fixture("task-timeline.json").into_bytes(),
        "application/json",
    );
    site
}

fn open(chrome: &mut Chrome, site: &Site, path: &str, ready: &str) {
    chrome.open(&site.url(path));
    assert!(
        chrome.wait_for(
            &format!(
                "document.documentElement.dataset.navigation === 'ready' && document.querySelector({}) !== null && document.fonts.status === 'loaded'",
                json!(ready)
            ),
            SHOWN
        ),
        "{path}: {}",
        chrome.eval("document.body ? document.body.innerText.slice(0, 1000) : '(no body)'")
    );
}

/// What a task's head and composer draw: its owner line, a priority select, a cancel, the
/// approve and the priority as text, and whether the page stays within the screen.
fn task_drawn(chrome: &mut Chrome) -> Value {
    chrome.eval(
        r#"(() => ({
            owner: document.querySelector('.task-head .owner-name')?.textContent ?? null,
            select: document.querySelector('#task-priority') !== null,
            cancel: [...document.querySelectorAll('.task-head button')].some(b => b.textContent === 'Cancel task'),
            approve: [...document.querySelectorAll('button')].some(b => b.textContent.includes('Approve 1a2b3c4')),
            priority: document.querySelector('.task-head .priority')?.textContent ?? null,
            within: document.documentElement.scrollWidth <= innerWidth,
            fault: /could not be read|server answered/.test(document.body.innerText),
        }))()"#,
    )
}

#[test]
fn a_tasks_deciding_controls_are_drawn_for_one_who_decides_it_and_its_priority_alone_else() {
    let Some(mut chrome) = chrome() else { return };
    for decides in [false, true] {
        let site = task_site(decides);
        for at in SCREENS {
            let (_, width, height, mobile) = at;
            screen(&mut chrome, width, height, mobile);
            open(&mut chrome, &site, TASK, "[data-ready=task] .task-head");
            if decides {
                assert!(
                    chrome.wait_for(
                        "[...document.querySelectorAll('button')].some(b => b.textContent.includes('Approve 1a2b3c4'))",
                        SHOWN
                    ),
                    "the approve is offered to one who decides it"
                );
            } else {
                // The offer is read; drawn for no one who does not decide it.
                assert!(chrome.wait_for(
                    "performance.getEntriesByType('resource').some(e => e.name.includes('/api/task-offer'))",
                    SHOWN
                ));
                chrome.frames_with_fonts();
            }
            let drawn = task_drawn(&mut chrome);
            let expected = if decides {
                json!({"owner":"group: design","select":true,"cancel":true,"approve":true,"priority":null,"within":true,"fault":false})
            } else {
                json!({"owner":"group: design","select":false,"cancel":false,"approve":false,"priority":"p1","within":true,"fault":false})
            };
            assert_eq!(drawn, expected, "{width}, decides {decides}");
            let name = if decides {
                "task-decides"
            } else {
                "task-reads"
            };
            shot(&mut chrome, name, at);
        }
        assert_eq!(site.posts(), [], "nothing sent on opening");
    }
}

/// What the card `alpha`'s open editor draws: its verdicts, Dismiss, Later, its send
/// button's words and its box's label, and whether the page stays within the screen.
fn card_drawn(chrome: &mut Chrome) -> Value {
    chrome.eval(
        r#"(() => {
            const card = document.querySelector('[data-doc-key="card:alpha"]');
            const buttons = [...card.querySelectorAll('form.verdict button')].map(b => b.textContent);
            return {
                opener: card.querySelector('.doc-actions button').textContent,
                verdicts: card.querySelector('.verdict-choices') !== null,
                dismiss: buttons.some(b => b.includes('Dismiss')),
                later: [...card.querySelectorAll('button')].some(b => b.textContent === 'Later'),
                send: buttons.includes('Send message'),
                label: card.querySelector('form.verdict textarea').getAttribute('aria-label'),
                within: document.documentElement.scrollWidth <= innerWidth,
                fault: /could not be read|server answered/.test(document.body.innerText),
            };
        })()"#,
    )
}

#[test]
fn a_cards_verdicts_and_later_are_drawn_for_one_who_decides_it_and_its_message_alone_else() {
    let Some(mut chrome) = chrome() else { return };
    for decides in [false, true] {
        let site = document_cards::probe_site();
        site.set_json("/api/postponed", &json!([]));
        site.set_json(
            "/api/postpone",
            &json!({"ok":true,"changed":false,"next":null}),
        );
        site.set_json(
            "/api/card-owners",
            &json!({"cards":{"alpha":owner(decides),"beta":owner(decides)}}),
        );
        for at in SCREENS {
            let (_, width, height, mobile) = at;
            screen(&mut chrome, width, height, mobile);
            open(
                &mut chrome,
                &site,
                "/docs/PROBE.md",
                "[data-doc-key=\"card:alpha\"] .owner-name",
            );
            document_cards::open_editor(&mut chrome, "alpha");
            chrome.frames_with_fonts();
            let drawn = card_drawn(&mut chrome);
            let expected = if decides {
                json!({"opener":"Close","verdicts":true,"dismiss":true,"later":true,
                       "send":true,"label":"Message or verdict note","within":true,"fault":false})
            } else {
                json!({"opener":"Close","verdicts":false,"dismiss":false,"later":false,
                       "send":true,"label":"Message","within":true,"fault":false})
            };
            assert_eq!(drawn, expected, "{width}, decides {decides}");
            let name = if decides {
                "card-decides"
            } else {
                "card-reads"
            };
            shot(&mut chrome, name, at);
        }
        // Opening its editor marks the card seen in the queue of one who decides it; nothing else,
        // and nothing for anyone else, whose queue it is not in.
        let posts = site.posts();
        assert!(
            (posts.iter())
                .all(|(route, body)| route == "/api/postpone" && body["action"] == "seen"),
            "{posts:?}"
        );
        if !decides {
            assert_eq!(posts, [], "no visit of a card not theirs");
        }
    }
}

/// The wire's sample of the Settings page (`wire.cases.json`'s `samples.settings`), as one of its
/// group reads it, or `stored` taken off as anyone else's read leaves it.
fn settings(changes: bool) -> Value {
    let cases: Value =
        serde_json::from_str(include_str!("../../../symphony-web/src/wire.cases.json"))
            .expect("the wire's cases");
    let mut page = cases["samples"]["settings"].clone();
    if !changes {
        page.as_object_mut().unwrap().remove("stored");
    }
    page
}

#[test]
fn the_settings_pages_controls_are_drawn_for_its_group_and_its_values_alone_for_anyone_else() {
    let Some(mut chrome) = chrome() else { return };
    for changes in [false, true] {
        let site = super::site();
        site.set_json(symphony_api::SETTINGS_PATH, &settings(changes));
        for at in SCREENS {
            let (_, width, height, mobile) = at;
            screen(&mut chrome, width, height, mobile);
            open(&mut chrome, &site, "/settings", "[data-ready=settings]");
            chrome.frames_with_fonts();
            let drawn = chrome.eval(
                r#"(() => {
                    const page = document.querySelector('.settings');
                    return {
                        entries: page.querySelectorAll('.setting-entry').length,
                        controls: page.querySelectorAll('.setting-control').length,
                        within: document.documentElement.scrollWidth <= innerWidth,
                    };
                })()"#,
            );
            let controls = if changes { 3 } else { 0 };
            assert_eq!(
                drawn,
                json!({"entries":6,"controls":controls,"within":true}),
                "{width}, changes {changes}"
            );
            let name = if changes {
                "settings-changes"
            } else {
                "settings-reads"
            };
            shot(&mut chrome, name, at);
        }
        assert_eq!(site.posts(), [], "nothing sent on opening");
    }
}
