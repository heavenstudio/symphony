//! The task board's status off the feed on `/tasks`: slow when the server reached the board and its
//! answer came late, unavailable when the board refused the connection, nothing once it answers.
use super::{SHOWN, chrome, site, wait_for_feed};
use serde_json::{Value, json};

/// One `tasks-status` event the feed sends, and the alert the page shows after it.
struct Case {
    version: u64,
    status: Value,
    alert: &'static str,
}

/// The page's alert text, `''` when it shows none.
const ALERT: &str = "document.querySelector('main[data-ready=board] .page-notices [role=alert]')?.textContent ?? ''";

/// What the page shows, for a failure's message.
const PAGE: &str = "document.body ? document.body.innerText.slice(0, 2000) : '(no body)'";

#[test]
fn the_tasks_page_says_a_late_board_is_slow_and_a_refused_one_unavailable() {
    let site = site();
    let Some(mut chrome) = chrome() else { return };
    chrome.open(&site.url("/tasks"));
    assert!(
        chrome.wait_for("document.querySelector('.kanban') !== null", SHOWN),
        "the task board was not drawn; the page shows {}",
        chrome.eval(PAGE)
    );
    if let Err(stall) = wait_for_feed(&site) {
        panic!(
            "{stall} ({} feed requests); it shows {}",
            site.feed_requests(),
            chrome.eval(PAGE)
        );
    }
    let cases = [
        Case {
            version: 4183,
            status: json!({"available":false,"slow":true,"error":"the board's database: the server at 127.0.0.1:1 is slow: no answer within 3s"}),
            alert: "The task board is slow to answer. Retrying…",
        },
        Case {
            version: 4184,
            status: json!({"available":false,"slow":false,"error":"the board's database: cannot connect to 127.0.0.1:1: Connection refused (os error 61)"}),
            alert: "The task board is unavailable. Reconnecting…",
        },
        Case {
            version: 4185,
            status: json!({"available":true}),
            alert: "",
        },
    ];
    for case in cases {
        let event = json!({"version":case.version,"changes":[{"region":"tasks-status","data":case.status}]});
        site.push(case.version, &event.to_string());
        assert!(
            chrome.wait_for(&format!("({ALERT}) === {}", json!(case.alert)), SHOWN),
            "after {}, the page alerts {}",
            case.status,
            chrome.eval(ALERT)
        );
    }
}
