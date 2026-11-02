//! `WatchLineTest`: an ask reaches the listening agent as one `ASK` line.

use symphony::asks::{AskStore, Poller, watch_line};

use super::common::Tree;

#[test]
fn the_line_names_the_place_and_the_question() {
    assert_eq!(
        watch_line(7, "docs/A.md", "bands", "why\nfive?"),
        "ASK 7 docs/A.md#bands — why five?"
    );
    assert_eq!(watch_line(8, "docs/A.md", "", "hm"), "ASK 8 docs/A.md — hm");
}

#[test]
fn the_poller_reports_what_is_open_then_each_new_question_once() {
    let tree = Tree::new();
    let store = AskStore::open(&tree.path("asks.db")).unwrap();
    let waiting = store
        .ask("docs/A.md", "asked before anyone listened", "", "")
        .unwrap();
    let done = store.ask("docs/A.md", "already answered", "", "").unwrap();
    store.answer(done, "yes", "agent").unwrap();
    let mut lines: Vec<String> = Vec::new();
    let mut poller = Poller::new();
    poller
        .poll(&store, &mut |line: &str| lines.push(line.to_string()))
        .unwrap();
    assert_eq!(
        lines,
        [format!(
            "ASK {waiting} docs/A.md — asked before anyone listened"
        )]
    );
    poller
        .poll(&store, &mut |line: &str| lines.push(line.to_string()))
        .unwrap();
    assert_eq!(lines.len(), 1);
    store.reply(done, "one more").unwrap();
    let fresh = store.ask("docs/B.md", "new", "x", "").unwrap();
    store
        .answer(fresh, "an answer is not an event", "agent")
        .unwrap();
    poller
        .poll(&store, &mut |line: &str| lines.push(line.to_string()))
        .unwrap();
    assert_eq!(
        lines[1..],
        [
            format!("ASK {done} docs/A.md — one more"),
            format!("ASK {fresh} docs/B.md#x — new")
        ]
    );
}
