//! [`super::report`]: a verdict's trouble filed on the board of the project whose folder it was
//! saved in, through the one store this process reaches that board by.

use std::os::unix::fs::PermissionsExt;

use super::report;
use crate::test_tree::Tree;

/// Filing a verdict's trouble from a folder this process serves, once the folder's `.git` went,
/// runs no `tasks` in the folder to find the app's session: such a command looks the board up for
/// itself and, without the `.git`, finds the board of the checkout that holds the folder. The app's
/// session is looked for on the store the report files on, held to the project
/// ([`crate::taskview::serve`]), which is refused as a folder that is gone, so the outer checkout's
/// board is never looked for.
#[test]
fn a_report_from_a_served_folder_left_without_its_git_runs_no_board_command_there() {
    assert!(
        symphony_env::TASKS_CLI
            .text()
            .is_none_or(|named| named.trim().is_empty()),
        "SYMPHONY_TASKS_CLI names the board's command for every run of this process"
    );
    let tree = Tree::checkout();
    tree.write("artifacts/inner/.git/HEAD", "ref: refs/heads/main\n");
    tree.write(
        "artifacts/inner/symphony.toml",
        "[project]\nid = \"inner\"\n",
    );
    // The project's build of the board's command, as `tasks_binary` finds it: it says it ran.
    let ran = tree.path("ran");
    let build = tree.write(
        "artifacts/inner/.target/bin/tasks",
        &format!("#!/bin/sh\necho \"$@\" >> '{}'\n", ran.display()),
    );
    std::fs::set_permissions(&build, std::fs::Permissions::from_mode(0o755)).unwrap();
    let inner = tree.path("artifacts/inner");
    assert!(crate::taskview::serve(&inner).is_some());
    std::fs::remove_dir_all(inner.join(".git")).unwrap();

    report(&inner, "a verdict's trouble");
    assert!(
        !ran.exists(),
        "a board command ran in the folder: {}",
        std::fs::read_to_string(&ran).unwrap_or_default()
    );
    assert!(
        !tree.path(".git/symphony").exists(),
        "the outer checkout's board was looked for"
    );
}
