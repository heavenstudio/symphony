//! The Symphony's board reader follows the project's limits as its file stands: a store
//! `taskview::connect_in` opens and keeps, as the server keeps its reader and its data worker's
//! store, holds to an edit of the main checkout's `symphony.toml` at its next read, with no
//! restart. A scratch database of its own, never the live board.

use super::board::Fixture;
use crate::Tree;
use std::collections::BTreeMap;
use symphony_board::cli::Env;
use symphony_board::limits::Limits;

#[test]
fn a_store_the_symphony_keeps_holds_to_an_edit_of_the_limits_with_no_restart() {
    let fixture = Fixture::empty();
    let tree = Tree::new();
    // A main checkout, as the board finds one: its `.git` folder and the project file beside it.
    let root = tree.path("lamp");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    let file = root.join("symphony.toml");
    std::fs::write(&file, "[limits]\norchestrator_slots = 2\n").unwrap();
    let client = fixture.other_database();
    let env = Env {
        vars: BTreeMap::from([
            ("TASKS_SERVER".to_string(), client.addr()),
            ("TASKS_DATABASE".to_string(), client.database().to_string()),
        ]),
        cwd: root.clone(),
    };
    let store = symphony::taskview::connect_in(&root, env).unwrap();
    assert_eq!(store.slots(), Some(2));
    std::fs::write(
        &file,
        "[limits]\norchestrator_slots = 12\nlow_volume = 1\nmachine_agents = 5\n\n[board]\nreview_alarm_hours = 6\n",
    )
    .unwrap();
    let limits = store.limits();
    assert_eq!(
        (
            limits.orchestrator_slots,
            limits.low_volume,
            limits.machine_agents,
            limits.alarm.raise_hours
        ),
        (Some(12), 1, Some(5), 6)
    );
    std::fs::remove_file(&file).unwrap();
    assert_eq!(store.limits(), Limits::default());
}
