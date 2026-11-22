//! What the machine's server lends a held update's retry: its run of a command, ended once the
//! server stops, and its word that it writes a board, never given before it loaded the project.
//! The worker that calls a round at each sweep is `front/sweep_tests.rs`'s.

use std::sync::Arc;

use super::*;
use crate::front::MachineClock;

fn shared(tree: &crate::testing::Tree) -> Arc<Shared> {
    let root = tree.path("alpha");
    std::fs::create_dir_all(&root).unwrap();
    let gate = crate::testing::open_gate();
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

#[test]
fn a_server_that_is_no_installed_release_tries_nothing_again() {
    let tree = crate::testing::Tree::new();
    assert!(Retrying::of_this_server(&tree.path("machine")).is_none());
}

#[test]
fn a_projects_board_is_asked_in_its_checkout_by_the_release_binary_without_the_symphonys_board() {
    use std::os::unix::fs::PermissionsExt;
    let tree = crate::testing::Tree::new();
    let shared = shared(&tree);
    let host = Server {
        shared: &shared,
        clock: &MachineClock,
    };
    let root = tree.path("alpha");
    let tasks = tree.path("releases/release-2026-11-12/bin/tasks");
    std::fs::create_dir_all(tasks.parent().unwrap()).unwrap();
    std::fs::write(
        &tasks,
        "#!/bin/sh\ncat > asked\npwd > where\nenv > env\n\
         echo '{\"ok\": {\"slug\": \"tests/x\", \"status\": \"ready\"}}'\n",
    )
    .unwrap();
    std::fs::set_permissions(&tasks, std::fs::Permissions::from_mode(0o755)).unwrap();
    let board = host.theirs(&root, &tasks);
    assert_eq!(board.status("tests/x"), Ok(Some("ready".to_string())));
    let read = |name: &str| std::fs::read_to_string(root.join(name)).unwrap();
    assert!(
        read("asked").contains("\"method\":\"get\""),
        "{}",
        read("asked")
    );
    assert_eq!(
        std::path::Path::new(read("where").trim())
            .canonicalize()
            .unwrap(),
        root.canonicalize().unwrap()
    );
    let env = read("env");
    let names: Vec<&str> = (symphony::machine::moving::UNSET.iter().copied())
        .chain(symphony_board::sys::GIT_VARS.iter().copied())
        .collect();
    for line in env.lines() {
        let name = line.split('=').next().unwrap_or_default();
        assert!(!names.contains(&name), "{name} reached the project's board");
    }
}
