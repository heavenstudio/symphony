//! Run the hub's worker with a watcher that delivers nothing, in a child with a private board.

use super::Hub;
use crate::testing::{Tree, board, git, repository};
use std::time::Duration;

const CHILD: &str = "SYMPHONY_HUB_BACKSTOP_CHILD";
const TEST: &str =
    "live::backstop_tests::a_hub_whose_watcher_delivers_nothing_still_refreshes_within_a_period";

#[test]
fn a_hub_whose_watcher_delivers_nothing_still_refreshes_within_a_period() {
    if std::env::var_os(CHILD).is_none() {
        let board = board::database();
        crate::testing::child_on(&board, CHILD, TEST);
        return;
    }

    let mut tree = Tree::new();
    tree.root = tree.root.canonicalize().unwrap();
    let root = repository(&tree);
    git(
        &root,
        &[
            "worktree",
            "add",
            "-b",
            "served",
            tree.path("served").to_str().unwrap(),
        ],
    );
    let period = Duration::from_millis(500);
    let hub = Hub::start_watching::<notify::NullWatcher>(
        &root,
        &root.join("build/client"),
        false,
        false,
        period,
        symphony::taskview::connect,
    )
    .unwrap();
    hub.start_initial_board();
    hub.ensure_page("served").unwrap();
    for scope in ["", "served"] {
        hub.document(scope, "docs/TOPIC.md").unwrap();
    }
    // Read the cached documents, as a GET would read the file itself.
    let cached = |scope: &str| {
        hub.pages.read().unwrap()[scope].documents["docs/TOPIC.md"]
            .waiting
            .clone()
    };
    assert_eq!(cached(""), ["initial-card"]);
    assert_eq!(cached("served"), ["initial-card"]);

    tree.write(
        "main/docs/TOPIC.md",
        "# Topic\n\n- [ ] **main-new** — Missed by the watcher.\n",
    );
    tree.write(
        "served/docs/TOPIC.md",
        "# Topic\n\n- [ ] **served-new** — Missed by the watcher.\n",
    );
    // The watcher sends nothing, so only the period's own look can refresh them; the period
    // itself is counted on an injected clock in `crate::backstop`'s tests.
    let refreshed = test_time::until_true(
        "a refresh after a period",
        test_time::backstop(period + Duration::from_secs(30)),
        || cached("") == ["main-new"] && cached("served") == ["served-new"],
    );
    if let Err(stall) = refreshed {
        panic!("{stall}; {:?} {:?}", cached(""), cached("served"));
    }
    hub.stop();
}
