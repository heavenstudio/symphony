//! What a page's feed hears of a board that went: whether it is slow (it took the read and answered
//! late) or unreachable, said again only when it turns from one to the other.

use super::Hub;
use super::startup_tests::SLOW;
use crate::testing::{
    Tree,
    board::{Fixture, Seed},
    repository,
};
use serde_json::{Value, json};

/// The main page's `tasks-status` and `counts` events published after version `seen`, which moves
/// past them: each status's data, and for counts whether the task counts are gone.
fn board_said(hub: &Hub, seen: &mut u64) -> Vec<Value> {
    let pages = hub.pages.read().unwrap();
    let mut said = Vec::new();
    let after = *seen;
    for event in pages[""].history.iter().filter(|e| e.version > after) {
        *seen = event.version;
        let event: Value = serde_json::from_str(&event.json).unwrap();
        for change in event["changes"].as_array().unwrap() {
            match change["region"].as_str().unwrap() {
                "tasks-status" => said.push(change["data"].clone()),
                "counts" => said.push(json!({"counts gone": change["data"]["tasks"].is_null()})),
                _ => {}
            }
        }
    }
    said
}

/// A gone board is said slow or unreachable, said again only when it turns from one to the other,
/// and its counts go once, when it goes.
#[test]
fn a_gone_board_tells_pages_whether_it_is_slow_and_again_only_when_that_turns() {
    let tree = Tree::new();
    let root = repository(&tree);
    let f = Fixture::new(Seed::default().task("tools/clock", "ready"));
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();
    let refused = "SurrealDB: cannot connect to 127.0.0.1:1: Connection refused (os error 61)";
    let gone = |error: &str, slow: bool| json!({"available":false,"slow":slow,"error":error});
    let back = [json!({"available":true}), json!({"counts gone":false})];
    let nothing: [Value; 0] = [];
    let mut seen = 0;
    let mut heard = |step: &dyn Fn()| {
        step();
        board_said(&hub, &mut seen)
    };

    assert_eq!(heard(&|| hub.board_read(&f.store).unwrap()), back);
    assert_eq!(
        heard(&|| hub.board_error(refused.into())),
        [gone(refused, false), json!({"counts gone":true})]
    );
    assert_eq!(
        heard(&|| hub.board_error(format!("{refused}, again"))),
        nothing
    );
    assert_eq!(
        heard(&|| hub.board_error(SLOW.into())),
        [gone(SLOW, true)],
        "an unreachable board turned slow"
    );
    assert_eq!(heard(&|| hub.board_error(SLOW.into())), nothing);
    assert_eq!(
        heard(&|| hub.board_error(refused.into())),
        [gone(refused, false)],
        "a slow board turned unreachable"
    );
    assert_eq!(heard(&|| hub.board_read(&f.store).unwrap()), back);
    assert_eq!(
        heard(&|| hub.board_error(SLOW.into())),
        [gone(SLOW, true), json!({"counts gone":true})],
        "an answering board turned slow"
    );
}
