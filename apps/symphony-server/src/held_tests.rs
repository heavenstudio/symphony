//! What the machine's server lends a held update's retry: its run of a command, ended once the
//! server stops, and its word that it writes a board, never given before it loaded the project.
//! The worker that calls a round at each sweep is `front/sweep_tests.rs`'s.

use std::sync::Arc;
use std::time::Instant;

use super::*;
use crate::front::MachineClock;
use crate::gate::Gate;

fn shared(tree: &crate::testing::Tree) -> Arc<Shared> {
    let root = tree.path("alpha");
    std::fs::create_dir_all(&root).unwrap();
    let gate = Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now));
    Shared::project(
        i64::from(std::process::id()),
        1,
        &root,
        None,
        gate,
        "/alpha",
    )
}

#[test]
fn a_command_runs_to_its_end_within_its_bound_and_none_runs_once_the_server_stops() {
    let tree = crate::testing::Tree::new();
    let shared = shared(&tree);
    let host = Server {
        shared: &shared,
        clock: &MachineClock,
    };
    let mut said = Command::new("sh");
    said.args(["-c", "echo moved"]);
    let done = host.run(said, Duration::from_secs(240)).unwrap();
    assert!(done.status.success());
    assert_eq!(String::from_utf8_lossy(&done.stdout), "moved\n");
    shared.stop();
    let mut never = Command::new("sh");
    never
        .args(["-c", "echo ran > ran"])
        .current_dir(tree.path("alpha"));
    let stopped = host.run(never, Duration::from_secs(240)).unwrap_err();
    assert!(stopped.contains("cancelled"), "{stopped}");
}

#[test]
fn a_project_not_loaded_here_is_never_said_to_be_written() {
    let tree = crate::testing::Tree::new();
    let shared = shared(&tree);
    let host = Server {
        shared: &shared,
        clock: &MachineClock,
    };
    assert_eq!(
        host.writes(symphony_board::SCHEMA),
        Err("this server has not loaded the project".to_string())
    );
    assert!(
        host.now() > 1_700_000_000.0,
        "the server's clock reads the wall's time"
    );
}
