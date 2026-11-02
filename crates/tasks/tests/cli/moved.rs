//! A board still in SQLite (`star-foundry-tasks.db` in the git common dir) is refused until
//! `just tasks-cutover` has copied it to SurrealDB and said so beside it, in `<file>.moved`: an agent
//! that merged main before the move would write where the copy is about to replace everything. The
//! copy itself reaches the board either way.

use crate::run;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tasks::cli::Env;

const REFUSED: &str = "the task board has not moved to SurrealDB yet: the lander runs `just tasks-cutover` once, right after landing (docs/workflow/TASKS.md#the-move-to-surrealdb)";
const UNFINISHED: &str = "the task board is frozen by a move to SurrealDB that has not finished: unless one is running, run `just tasks-cutover` again (docs/workflow/TASKS.md#the-move-to-surrealdb)";

/// A repository of its own holding a SQLite board in its git dir, and an environment that names no
/// server, so the board is the one in that common dir, whose server cannot start (`SF_SURREAL`
/// names no binary).
fn unmoved(name: &str) -> (PathBuf, Env) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("moved-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let done = Command::new("git")
        .args(["init", "-q"])
        .arg(&dir)
        .output()
        .unwrap();
    assert!(done.status.success());
    fs::write(dir.join(".git/star-foundry-tasks.db"), b"").unwrap();
    let surreal = dir.join("no-surreal").display().to_string();
    let vars = [("SF_SURREAL".to_string(), surreal)].into_iter().collect();
    (dir.clone(), Env { vars, cwd: dir })
}

#[test]
fn a_command_on_a_board_still_in_sqlite_is_refused_and_starts_no_server() {
    let (dir, env) = unmoved("command");
    let ran = run(&env, &["list"], "");
    assert_eq!((ran.code, ran.err), (2, format!("✖ {REFUSED}\n")));
    assert!(!dir.join(".git/sf-tasks").exists());
}

#[test]
fn the_api_answers_each_call_with_it() {
    let (_, env) = unmoved("api");
    let ran = run(&env, &["api"], r#"{"method": "sessions"}"#);
    let said: Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(said, json!({"error": REFUSED}));
}

#[test]
fn once_it_has_moved_the_board_is_reached() {
    let (dir, env) = unmoved("moved");
    fs::write(dir.join(".git/star-foundry-tasks.db.moved"), b"").unwrap();
    let ran = run(&env, &["list"], "");
    assert_eq!(ran.code, 2, "the server it needs cannot start");
    assert!(!ran.err.contains("has not moved"), "{}", ran.err);
}

/// A move cut short leaves its marker's own unfinished file, which is not the marker: the board is
/// refused until a move writes `.moved` itself.
#[test]
fn a_marker_a_move_never_finished_is_no_marker() {
    let (dir, env) = unmoved("unfinished");
    fs::write(dir.join(".git/star-foundry-tasks.db.moved.4242"), b"moved").unwrap();
    let ran = run(&env, &["list"], "");
    assert_eq!((ran.code, ran.err), (2, format!("✖ {REFUSED}\n")));
    assert!(!dir.join(".git/sf-tasks").exists());
}

/// Describe the marker's actual state so an interrupted move is distinguishable
/// from a completed move.
#[test]
fn a_board_a_move_froze_and_never_marked_says_to_run_the_move_again() {
    let (dir, env) = unmoved("frozen");
    let board = rusqlite::Connection::open(dir.join(".git/star-foundry-tasks.db")).unwrap();
    board
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE tasks (slug TEXT);
             CREATE TRIGGER sf_moved_tasks_insert BEFORE INSERT ON tasks BEGIN SELECT RAISE(ABORT, 'frozen'); END;",
        )
        .unwrap();
    drop(board);
    let ran = run(&env, &["list"], "");
    assert_eq!((ran.code, ran.err), (2, format!("✖ {UNFINISHED}\n")));
    assert!(!dir.join(".git/sf-tasks").exists());
}

#[test]
fn a_repository_with_no_sqlite_board_has_nothing_to_move() {
    let (dir, env) = unmoved("fresh");
    fs::remove_file(dir.join(".git/star-foundry-tasks.db")).unwrap();
    let ran = run(&env, &["list"], "");
    assert!(!ran.err.contains("has not moved"), "{}", ran.err);
}

#[test]
fn the_copy_reaches_the_board_before_it_has_moved() {
    let (_, env) = unmoved("copy");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = ["--board".to_string(), "--out".to_string()];
    let code = tasks::copy_main(&args, &env, &mut &b""[..], &mut out, &mut err);
    let said = String::from_utf8(err).unwrap();
    assert_eq!(code, 2, "the server it needs cannot start");
    assert!(!said.contains("has not moved"), "{said}");
}
