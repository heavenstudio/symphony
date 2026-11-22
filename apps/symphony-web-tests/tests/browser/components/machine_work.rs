//! The release client shares two-machine resource phases across cards, agents and task detail.

use crate::{SHOWN, chrome, look_parity, site, texts, wait_for_feed};
use serde_json::{Value, json};

mod columns;

/// The work as each page reads it: the task board apart (`/api/machine-work`, its listings being
/// read with `work=0`), the task and agent pages in their own views.
fn install_work(site: &crate::Site, work: &Value) {
    site.set_json("/api/machine-work", work);
    for (endpoint, mut response) in look_parity::responses() {
        match endpoint.as_str() {
            "/api/task-board" => {
                response["machine_work"] = work.clone();
                let current_test = work["machines"][1]["freshness"] == "incomplete";
                response["counts"] = json!([
                    ["in_progress", usize::from(!current_test)],
                    ["test", usize::from(current_test)]
                ]);
            }
            "/api/task-page" => response["machine_work"] = work.clone(),
            "/api/agent-page" => response["view"]["machine_work"] = work.clone(),
            _ => continue,
        }
        site.set_json(&endpoint, &response);
    }
}

fn machine(id: &str, kind: &str, state: &str) -> Value {
    json!({"machine_id":id,"label":id,"producer_id":"fixture","sequence":1,"age_ms":0,
        "freshness":"incomplete","complete":false,"reason":"partial native inventory","counts":null,
        "operations":[{"machine_id":id,"operation_id":"same-text-distinct-machines","parent_id":null,
            "handle":"example","task":"visual/in-progress","checkout":"opaque","recipe":"fixture",
            "kind":kind,"state":state,"resource":"native resource","reason":"fixture","phase_since":1.0}],
    })
}

fn snapshot() -> Value {
    json!({"freshness":"unknown","unassigned":[],"machines":[machine("alpha","build","queued"),machine("beta","test","running")],
        "guidance":{"alpha":{"build":2,"test":1},"beta":{"build":1,"test":3}},
    })
}

fn labels(chrome: &mut crate::Chrome, selector: &str, expected: &[&str]) {
    let condition = format!(
        "JSON.stringify([...document.querySelectorAll({})].map(e=>e.textContent.trim())) === JSON.stringify({})",
        json!(selector),
        json!(expected)
    );
    assert!(
        chrome.wait_for(&condition, SHOWN),
        "expected {expected:?}; phases {:?}; page {}",
        texts(chrome, selector),
        chrome.eval("document.body.innerText")
    );
}

#[test]
fn machine_work_phases_and_advisory_targets_stay_consistent_through_live_completion_and_feed_loss()
{
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    let mut version = 4183;
    for (path, selector, reveal) in [
        (
            "/agents",
            ".agent .work-phase",
            "document.getElementById('agent-example')",
        ),
        (
            "/task/visual/in-progress",
            ".task-head .work-phase",
            "document.querySelector('.task-head')",
        ),
    ] {
        site.hold_route("/api/board", false);
        let mut work = snapshot();
        for (endpoint, mut response) in look_parity::responses() {
            match endpoint.as_str() {
                "/api/task-board" | "/api/task-page" => response["machine_work"] = work.clone(),
                "/api/agent-page" => response["view"]["machine_work"] = work.clone(),
                _ => {}
            }
            site.set_json(&endpoint, &response);
        }
        install_work(&site, &work);
        if path == "/agents" {
            chrome.viewport(1280, 900, false);
            chrome.scheme("light");
        }
        chrome.open(&site.url(path));
        assert!(
            chrome.wait_for(&format!("({reveal}) !== null"), SHOWN),
            "{path} did not mount its resource owner"
        );
        chrome.eval(&format!("({reveal}).scrollIntoView({{block:'center'}})"));
        labels(
            &mut chrome,
            selector,
            &[
                "Observed Build queued · alpha · partial inventory",
                "Observed Test running · beta · partial inventory",
            ],
        );
        if path == "/agents" {
            chrome.eval("scrollTo(0,0)");
            columns::screenshot(&mut chrome, "agents-desktop");
        }
        wait_for_feed(&site).expect("the page never followed the feed");
        if path == "/agents" {
            let targets = texts(&mut chrome, ".machine-guidance summary");
            assert_eq!(
                targets,
                [
                    "Advisory targets · builds 2 · tests 1",
                    "Advisory targets · builds 1 · tests 3"
                ]
            );
        }
        let beta = texts(&mut chrome, ".machine-work-row[data-machine-id=beta]");
        work["machines"][0]["operations"][0]["state"] = json!("running");
        install_work(&site, &work);
        site.push(
            version,
            &json!({"version":version,"changes":[{"region":"machine-work","data":work}]})
                .to_string(),
        );
        version += 1;
        labels(
            &mut chrome,
            selector,
            &[
                "Observed Build running · alpha · partial inventory",
                "Observed Test running · beta · partial inventory",
            ],
        );
        work["machines"][0]["operations"] = json!([]);
        install_work(&site, &work);
        site.push(
            version,
            &json!({"version":version,"changes":[{"region":"machine-work","data":work}]})
                .to_string(),
        );
        version += 1;
        labels(
            &mut chrome,
            selector,
            &["Observed Test running · beta · partial inventory"],
        );
        assert_eq!(
            texts(&mut chrome, ".machine-work-row[data-machine-id=beta]"),
            beta,
            "alpha consumed another machine's displayed work or guidance"
        );
        work["machines"][1]["freshness"] = json!("stale");
        install_work(&site, &work);
        site.push(
            version,
            &json!({"version":version,"changes":[{"region":"machine-work","data":work}]})
                .to_string(),
        );
        version += 1;
        labels(
            &mut chrome,
            selector,
            &["Observed Test running · beta · stale"],
        );
        site.hold_route("/api/board", true);
        site.drop_feeds();
        labels(
            &mut chrome,
            selector,
            &["Observed Test running · beta · disconnected"],
        );
        assert_eq!(
            chrome.eval("document.querySelector('main')?.dataset.feed"),
            json!("stale")
        );
        assert!(
            !chrome
                .eval("document.body.innerText")
                .as_str()
                .unwrap()
                .contains("Builds: 0 running")
        );
    }
}
