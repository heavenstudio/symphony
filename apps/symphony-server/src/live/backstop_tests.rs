//! Run the hub's worker with a watcher that delivers nothing, in a child with a private board.

use super::Hub;
use crate::testing::{Tree, board, git, repository};
use std::time::{Duration, Instant};
use symphony_board::sys::Command;

const CHILD: &str = "SF_HUB_BACKSTOP_CHILD";
const TEST: &str =
    "live::backstop_tests::a_hub_whose_watcher_delivers_nothing_still_refreshes_within_a_period";

#[test]
fn a_hub_whose_watcher_delivers_nothing_still_refreshes_within_a_period() {
    if std::env::var_os(CHILD).is_none() {
        let board = board::client();
        let done = symphony_board::sys::output(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(CHILD, "1")
                .env("TASKS_SERVER", board.addr())
                .env("TASKS_DATABASE", board.database())
                .env_remove("ASKS_DB")
                .env_remove("TASKS_SESSION")
                .env_remove("SYMPHONY_UI_DIST")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_COMMON_DIR")
                .env_remove("GIT_INDEX_FILE"),
        )
        .unwrap();
        let out = String::from_utf8_lossy(&done.stdout);
        let err = String::from_utf8_lossy(&done.stderr);
        assert!(done.status.success(), "{out}\n{err}");
        assert!(out.contains("1 passed"), "child ran no test: {out}");
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

    let edited = Instant::now();
    tree.write(
        "main/docs/TOPIC.md",
        "# Topic\n\n- [ ] **main-new** — Missed by the watcher.\n",
    );
    tree.write(
        "served/docs/TOPIC.md",
        "# Topic\n\n- [ ] **served-new** — Missed by the watcher.\n",
    );
    while cached("") != ["main-new"] || cached("served") != ["served-new"] {
        assert!(
            edited.elapsed() < period + Duration::from_secs(30),
            "no refresh after a period: {:?} {:?}",
            cached(""),
            cached("served")
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    hub.stop();
}
