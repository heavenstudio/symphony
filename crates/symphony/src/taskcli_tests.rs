//! Which `tasks` the board clients run from a checkout, and what a run that cannot start says.

use super::*;
use crate::test_tree::Tree;
use std::os::unix::fs::PermissionsExt;

fn program(tree: &Tree, rel: &str) -> PathBuf {
    let path = tree.write(rel, "#!/bin/sh\n");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn the_checkouts_link_runs_and_never_a_build_in_the_common_dir_or_the_source_launcher() {
    let tree = Tree::new();
    std::fs::create_dir_all(tree.path(".git")).unwrap();
    let launcher = program(&tree, "tools/rs-bin.sh");
    assert_eq!(tasks_binary(&tree.root), None);
    // A build a project's own launcher keeps in the common dir is no link of the checkout's.
    let elsewhere = program(&tree, ".git/builds/tasks-0123456789abcdef");
    tree.write(".git/builds/current", &format!("{}\n", elsewhere.display()));
    assert_eq!(tasks_binary(&tree.root), None);
    program(&tree, ".target/bin/tasks");
    assert_eq!(
        tasks_binary(&tree.root),
        Some(tree.path(".target/bin/tasks"))
    );
    let board = CliBoard::named_else_found(Vec::new(), &tree.root);
    assert_eq!(
        board.cli,
        [tree.path(".target/bin/tasks").display().to_string()]
    );
    assert!(
        board
            .cli
            .iter()
            .all(|word| *word != launcher.display().to_string())
    );
    assert_eq!(board.missing, None);
}

#[test]
fn a_named_command_wins_and_needs_no_build() {
    let tree = Tree::new();
    let board = CliBoard::named_else_found(vec!["fake".into(), "tasks".into()], &tree.root);
    assert_eq!(board.cli, ["fake", "tasks"]);
    assert_eq!(board.missing, None);
}

#[test]
fn with_no_build_it_runs_tasks_from_the_search_path_and_a_failed_start_says_how_to_link_one() {
    let tree = Tree::new();
    let board = CliBoard::named_else_found(Vec::new(), &tree.root);
    assert_eq!(board.cli, ["tasks"]);
    let fix = format!(
        "link the project's build of tasks at {}, or put tasks on PATH",
        tree.path(".target/bin/tasks").display()
    );
    assert_eq!(board.missing.as_deref(), Some(fix.as_str()));
    // A start that finds no program adds the fix; any other failure is said as it is.
    let mut gone = board;
    gone.cli = vec![tree.path("no-such-tasks").display().to_string()];
    let why = gone.app_session().unwrap_err().0;
    assert!(
        why.starts_with(&format!("cannot run {}: ", gone.cli[0])) && why.ends_with(&fix),
        "{why}"
    );
    let unrunnable = tree.write("not-runnable", "x\n");
    gone.cli = vec![unrunnable.display().to_string()];
    let why = gone.app_session().unwrap_err().0;
    assert!(
        why.starts_with("cannot run ") && !why.contains("link the"),
        "{why}"
    );
}

#[test]
fn a_board_run_without_names_runs_with_none_of_them_inherited_or_given() {
    let tree = Tree::new();
    let path = tree.write(
        "bin/tasks",
        "#!/bin/sh\ncat > /dev/null\nprintf '{\"ok\": {\"given\": \"%s\", \"kept\": \"%s\", \"inherited\": \"%s\"}}' \
         \"$TASKS_SERVER\" \"$KEPT\" \"$CARGO_MANIFEST_DIR\"\n",
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let env = [
        ("TASKS_SERVER".to_string(), "127.0.0.1:1".to_string()),
        ("KEPT".to_string(), "kept".to_string()),
    ];
    let cli = vec![path.display().to_string()];
    let said = |board: CliBoard| board.call("version", &json!({})).unwrap();
    let whole = said(CliBoard::with_command(cli.clone(), &env, &tree.root));
    assert_eq!(whole["given"], "127.0.0.1:1");
    assert_eq!(whole["kept"], "kept");
    let cleared = said(
        CliBoard::with_command(cli, &env, &tree.root)
            .without(&["TASKS_SERVER", "CARGO_MANIFEST_DIR"]),
    );
    assert_eq!(cleared["given"], "", "a name given and cleared is cleared");
    assert_eq!(cleared["kept"], "kept", "the rest are kept");
    assert_eq!(
        cleared["inherited"], "",
        "and one this process has is not inherited"
    );
    // Cargo gives its tests their package's folder, so the clearing above is of a real variable.
    if let Ok(dir) = std::env::var("CARGO_MANIFEST_DIR") {
        assert_eq!(whole["inherited"], dir.as_str());
    }
}
