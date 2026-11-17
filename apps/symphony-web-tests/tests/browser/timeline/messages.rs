//! Message selection, its shared thread reads and the real scratch board's role history.
use super::*;
use crate::{common, owner_controls::ScratchBoard, restored::Native};

fn history_fixture() -> Value {
    let mut t = fixture();
    let start = t["created"].as_f64().unwrap();
    let endpoint = |role: &str, by: &str| json!({"state":"known","candidates":[{"role":role,"by":by,"source":SLUG}]});
    let mut roles = Vec::new();
    for (by, from, to, id) in [
        ("first-orchestrator", start, Some(start + 3600.0), 1),
        ("next-orchestrator", start + 3600.0, None, 3),
    ] {
        roles.push(json!({"role":"orchestrator","by":by,"source":SLUG,"from":from,"from_id":id,"to":to,"to_id":to.map(|_|3),"uncertain":false}));
    }
    let messages = vec![
        json!({"id":2,"at":start+1800.25,"sender":"first-orchestrator","recipient":"@executor","kind":"msg","from":endpoint("orchestrator","first-orchestrator"),"to":endpoint("executor","quiet-wren"),"after_close":false}),
        json!({"id":4,"at":start+4000.0,"sender":"quiet-wren","recipient":"@orchestrator","kind":"msg","from":endpoint("executor","quiet-wren"),"to":endpoint("orchestrator","next-orchestrator"),"after_close":false}),
        json!({"id":5,"at":start+4000.0,"sender":"owner","recipient":null,"kind":"msg","from":{"state":"outside","candidates":[]},"to":{"state":"everyone","candidates":[]},"after_close":null}),
    ];
    t["history"] = json!({"sources_complete":true,"sources":[{"slug":SLUG,"state":"complete","first_id":1,"last_id":5,"before":null}],"roles":roles,"messages":messages});
    t
}

fn thread(timeline: &Value, ids: &[i64], before: Option<i64>) -> Value {
    let entries: Vec<_> = timeline["history"]["messages"].as_array().unwrap().iter()
        .filter(|m| ids.contains(&m["id"].as_i64().unwrap()))
        .map(|m|json!({"message":{"id":m["id"],"slug":SLUG,"sender":m["sender"],"recipient":m["recipient"],"kind":"msg","ts":m["at"],"text":format!("Original body {}",m["id"])},"stamp":"11-11 10:00","blocks":[{"type":"paragraph","spans":[{"type":"text","text":format!("Original body {}",m["id"])}]}]}))
        .collect();
    json!({"entries":entries,"before":before})
}

fn message_site(t: &Value) -> Site {
    let site = served(t);
    site.set_json("/api/task-history", &thread(t, &[4, 5], Some(4)));
    site.set_json(
        "/api/task-history?slug=visual%2Fin-progress&before=4",
        &thread(t, &[2], None),
    );
    site
}

fn pick(chrome: &mut crate::chrome::Chrome, id: i64) {
    chrome.eval(&format!("(() => {{ const choice = document.getElementById('tl-message-choice'); choice.value='{id}'; choice.dispatchEvent(new Event('change',{{bubbles:true}})); }})()"));
    assert!(chrome.wait_for(&format!("document.querySelector('.tl-message-details h3')?.textContent.startsWith('Message #{id} ')"), SHOWN));
    frames(chrome);
}

fn picture(chrome: &mut crate::chrome::Chrome, name: &str) {
    chrome.eval("document.getElementById('timeline').scrollIntoView(); document.fonts.ready.then(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))))");
    let shot = chrome.call(
        "Page.captureScreenshot",
        json!({"format":"png","captureBeyondViewport":true}),
    );
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../artifacts/app/timeline-orchestrator-messages");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join(format!("{name}.base64")),
        shot["data"].as_str().unwrap(),
    )
    .unwrap();
}

#[test]
fn addressed_messages_share_the_clock_keep_collisions_and_select_both_directions() {
    let t = history_fixture();
    let site = message_site(&t);
    let Some(mut page) = chrome() else { return };
    view(&mut page, 1280, false);
    page.open(&site.url(TASK));
    assert!(page.wait_for(
        "document.querySelectorAll('#tl-message-choice option').length === 4",
        SHOWN
    ));
    assert_eq!(
        page.eval("[...document.querySelectorAll('[data-lane=orchestrator] .tl-seg')].length"),
        json!(2)
    );
    assert_eq!(page.eval("[...document.querySelectorAll('.tl-message-mark')].map(m=>[m.dataset.message,m.style.left])"), json!([["2","8.42%"],["4","18.07%"],["5","18.07%"]]));
    // A message before the newest thread page reuses its before cursor, once.
    let older = "/api/task-history?slug=visual%2Fin-progress&before=4";
    site.hold_route(older, true);
    pick(&mut page, 2);
    assert!(page.wait_for(
        "document.querySelector('.tl-message-details')?.textContent.includes('Loading message')",
        SHOWN
    ));
    site.hold_route(older, false);
    assert!(page.wait_for(
        "document.querySelector('.tl-message-details .entry') !== null",
        SHOWN
    ));
    assert_eq!(
        site.targets()
            .iter()
            .filter(|p| p.as_str() == older)
            .count(),
        1
    );
    assert_eq!(page.eval("[document.querySelector('.tl-address-arrow').dataset.from,document.querySelector('.tl-address-arrow').dataset.to]"), json!(["orchestrator","executor"]));
    assert!(
        page.eval("document.querySelector('.tl-message-details').textContent")
            .as_str()
            .unwrap()
            .contains(&t["history"]["messages"][0]["at"].to_string())
    );
    page.eval("document.getElementById('tl-message-choice').focus()");
    components::key(&mut page, "ArrowRight", "ArrowRight", 39);
    assert!(page.wait_for(
        "document.querySelector('.tl-message-details h3')?.textContent.startsWith('Message #4 ')",
        SHOWN
    ));
    assert_eq!(page.eval("[document.querySelector('.tl-address-arrow').dataset.from,document.querySelector('.tl-address-arrow').dataset.to]"), json!(["executor","orchestrator"]));
    assert!(
        page.eval("document.querySelector('.tl-address-arrow').classList.contains('tl-arrow-up')")
            .as_bool()
            .unwrap()
    );
    pick(&mut page, 5);
    assert!(
        page.eval(
            "document.querySelector('.tl-message-details').textContent.includes('Everyone on task')"
        )
        .as_bool()
        .unwrap()
    );
    assert_eq!(
        page.eval("document.querySelectorAll('#tl-message-choice option[value]').length"),
        json!(4)
    );
    assert_eq!(
        page.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
}

#[test]
fn phone_messages_keep_the_box_and_selected_row_through_show_all_and_collapse() {
    let site = message_site(&history_fixture());
    let Some(mut page) = chrome() else { return };
    view(&mut page, 390, true);
    page.open(&site.url(TASK));
    assert!(page.wait_for("document.querySelector('.tl-more') !== null && document.getElementById('tl-message-choice') !== null", SHOWN));
    pick(&mut page, 2);
    let in_view = "(() => {const b=document.querySelector('.tl-body').getBoundingClientRect(),r=document.querySelector('.tl-message-row[aria-pressed=true]').getBoundingClientRect();return r.top>=b.top && r.bottom<=b.bottom})()";
    assert_eq!(page.eval(in_view), json!(true));
    assert_eq!(
        page.eval(
            "Math.round(document.getElementById('timeline-slot').getBoundingClientRect().height)"
        ),
        json!(240)
    );
    tap(&mut page, ".tl-more");
    assert_eq!(
        page.eval("document.querySelector('.tl-more').getAttribute('aria-expanded')"),
        json!("true")
    );
    tap(&mut page, ".tl-more");
    assert_eq!(page.eval(in_view), json!(true));
    assert_eq!(
        page.eval(
            "Math.round(document.getElementById('timeline-slot').getBoundingClientRect().height)"
        ),
        json!(240)
    );
    assert_eq!(
        page.eval("document.documentElement.scrollWidth <= innerWidth"),
        json!(true)
    );
}

#[test]
fn partial_history_and_a_failed_message_read_stay_explicit_until_retry() {
    let mut t = history_fixture();
    t["history"]["sources"][0]["state"] = json!("partial");
    t["history"]["sources"][0]["before"] = json!(1);
    t["history"]["messages"][0]["to"] = json!({"state":"unknown","candidates":[]});
    let site = message_site(&t);
    let older = "/api/task-history?slug=visual%2Fin-progress&before=4";
    site.fail_route(older, true);
    let Some(mut page) = chrome() else { return };
    view(&mut page, 1280, false);
    page.open(&site.url(TASK));
    assert!(page.wait_for(
        "document.getElementById('tl-message-choice') !== null",
        SHOWN
    ));
    pick(&mut page, 2);
    assert!(page.wait_for("document.querySelector('.tl-message-details [role=status]')?.textContent.includes('Board snapshot unavailable')", SHOWN));
    assert!(page.eval("document.querySelector('.tl-message-panel').textContent.includes('Incomplete role history') && document.querySelector('.tl-message-details').textContent.includes('Individual endpoint unknown')").as_bool().unwrap());
    site.fail_route(older, false);
    tap(&mut page, ".tl-message-details [role=status] button");
    assert!(page.wait_for("document.querySelector('.tl-message-details .entry')?.textContent.includes('Original body 2')", SHOWN));
    assert_eq!(
        site.targets()
            .iter()
            .filter(|p| p.as_str() == older)
            .count(),
        2
    );
}

#[test]
fn a_newer_selected_message_joins_the_existing_newest_thread_read() {
    let t = history_fixture();
    let site = message_site(&t);
    site.answer_in_turn(
        "/api/task-history",
        &[thread(&t, &[2], None), thread(&t, &[2, 4, 5], None)],
    );
    let Some(mut page) = chrome() else { return };
    view(&mut page, 1280, false);
    page.open(&site.url(TASK));
    assert!(page.wait_for(
        "document.getElementById('tl-message-choice') !== null",
        SHOWN
    ));
    pick(&mut page, 4);
    assert!(page.wait_for("document.querySelector('.tl-message-details .entry')?.textContent.includes('Original body 4')", SHOWN));
    assert_eq!(
        site.targets()
            .iter()
            .filter(|p| p.starts_with("/api/task-history?"))
            .count(),
        2
    );
    assert!(!site.targets().iter().any(|p| p.contains("before=")));
}

#[test]
fn timeline_history_pictures_compare_the_committed_client_with_the_new_client() {
    let t = history_fixture();
    let old = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../symphony-server/client/public");
    let before = Site::start(&old, &common::fixture("board.json"));
    let content = look_parity::responses();
    before.set_json("/api/task-page", &content["/api/task-page"]);
    before.set_json("/api/task-history", &thread(&t, &[2, 4, 5], None));
    before.set_json(symphony_api::TASK_TIMELINE_PATH, &t);
    let after = message_site(&t);
    let Some(mut page) = chrome() else { return };
    for width in [1280, 390] {
        for scheme in ["light", "dark"] {
            view(&mut page, width, width < 900);
            page.call(
                "Emulation.setEmulatedMedia",
                json!({"features":[{"name":"prefers-color-scheme","value":scheme}]}),
            );
            for (version, site) in [("before", &before), ("after", &after)] {
                page.open(&site.url(TASK));
                assert!(page.wait_for("document.querySelector('.tl-drawn') !== null", SHOWN));
                if version == "after" {
                    pick(&mut page, 4);
                }
                picture(&mut page, &format!("{version}-{width}-{scheme}"));
            }
        }
    }
}

#[test]
fn a_scratch_board_handover_and_both_directions_reach_the_real_task_page() {
    let board = ScratchBoard::start();
    let call = |method, args| board.call_in_database("browser_owner_controls", method, args);
    let a = call("register", json!({"kind":"orchestrator"}))["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let b = call("register", json!({"kind":"orchestrator"}))["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let worker = call("register", json!({"kind":"worker"}))["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let slug = "fixture/timeline-messages";
    call(
        "create",
        json!({"slug":slug,"title":"Timeline handover","by":a}),
    );
    call("claim", json!({"slug":slug,"handle":worker}));
    let first = call(
        "say",
        json!({"slug":slug,"sender":a,"to":"@executor","text":"Before handover"}),
    );
    let native = Native::start_with_board(Some(board.address()));
    let handover = Command::new(binaries::tasks())
        .args(["handover", slug, &b, "--as", &a])
        .env("TASKS_SERVER", board.address())
        .env("TASKS_DATABASE", "browser_owner_controls")
        .env("TASKS_SWEEP", "off")
        .env_remove("TASKS_SESSION")
        .current_dir(native.root())
        .output()
        .unwrap();
    assert!(handover.status.success(), "handover: {handover:?}");
    let second = call(
        "say",
        json!({"slug":slug,"sender":worker,"to":"@orchestrator","text":"After handover"}),
    );
    let Some(mut page) = chrome() else { return };
    for width in [1280, 390] {
        view(&mut page, width, width < 900);
        page.open(&native.url(&format!("/task/{slug}")));
        assert!(
            page.wait_for(
                "document.querySelectorAll('[data-lane=orchestrator] .tl-seg').length === 2",
                SHOWN
            ),
            "{:?}",
            page.eval("document.body.innerText")
        );
        for id in [
            first["id"].as_i64().unwrap(),
            second["id"].as_i64().unwrap(),
        ] {
            pick(&mut page, id);
        }
        assert!(
            page.eval(&format!(
                "document.querySelector('.tl-message-details').textContent.includes({})",
                json!(b)
            ))
            .as_bool()
            .unwrap()
        );
        assert_eq!(
            page.eval("document.documentElement.scrollWidth <= innerWidth"),
            json!(true)
        );
    }
}
