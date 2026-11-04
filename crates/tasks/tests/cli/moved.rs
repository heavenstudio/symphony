//! A board still in SQLite (`<project id>-tasks.db` in the git common dir, `lamp-works-tasks.db` here)
//! is refused until `just tasks-cutover` has copied it to SurrealDB and said so beside it, in
//! `<file>.moved`: an agent that merged main before the move would write where the copy is about to
//! replace everything. The copy itself reaches the board either way.

use crate::run;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use symphony_board::cli::Env;
use symphony_board::sys::Command;

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
    let done =
        symphony_board::sys::output(Command::new("git").args(["init", "-q"]).arg(&dir)).unwrap();
    assert!(done.status.success());
    fs::write(
        dir.join("symphony.toml"),
        "[project]\nid = \"lamp-works\"\n",
    )
    .unwrap();
    fs::write(dir.join(".git/lamp-works-tasks.db"), b"").unwrap();
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
    fs::write(dir.join(".git/lamp-works-tasks.db.moved"), b"").unwrap();
    let ran = run(&env, &["list"], "");
    assert_eq!(ran.code, 2, "the server it needs cannot start");
    assert!(!ran.err.contains("has not moved"), "{}", ran.err);
}

/// A move cut short leaves its marker's own unfinished file, which is not the marker: the board is
/// refused until a move writes `.moved` itself.
#[test]
fn a_marker_a_move_never_finished_is_no_marker() {
    let (dir, env) = unmoved("unfinished");
    fs::write(dir.join(".git/lamp-works-tasks.db.moved.4242"), b"moved").unwrap();
    let ran = run(&env, &["list"], "");
    assert_eq!((ran.code, ran.err), (2, format!("✖ {REFUSED}\n")));
    assert!(!dir.join(".git/sf-tasks").exists());
}

/// Describe the marker's actual state so an interrupted move is distinguishable
/// from a completed move.
#[test]
fn a_board_a_move_froze_and_never_marked_says_to_run_the_move_again() {
    let (dir, env) = unmoved("frozen");
    let board = rusqlite::Connection::open(dir.join(".git/lamp-works-tasks.db")).unwrap();
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
    fs::remove_file(dir.join(".git/lamp-works-tasks.db")).unwrap();
    let ran = run(&env, &["list"], "");
    assert!(!ran.err.contains("has not moved"), "{}", ran.err);
}

/// The board is named by the project's id, so a project file that names none, or no file at all,
/// has no SQLite board, whatever the common dir holds.
#[test]
fn a_project_naming_no_id_has_no_sqlite_board() {
    for (name, project) in [
        ("unnamed", Some("[project]\nname = \"Lamp Works\"\n")),
        ("fileless", None),
    ] {
        let (dir, env) = unmoved(name);
        match project {
            Some(text) => fs::write(dir.join("symphony.toml"), text).unwrap(),
            None => fs::remove_file(dir.join("symphony.toml")).unwrap(),
        }
        let ran = run(&env, &["list"], "");
        assert!(!ran.err.contains("has not moved"), "{name}: {}", ran.err);
        assert_eq!(ran.code, 2, "{name}: the server it needs cannot start");
    }
}

#[test]
fn the_copy_reaches_the_board_before_it_has_moved() {
    let (_, env) = unmoved("copy");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = ["--board".to_string(), "--out".to_string()];
    let code = symphony_board::copy_main(&args, &env, &mut &b""[..], &mut out, &mut err);
    let said = String::from_utf8(err).unwrap();
    assert_eq!(code, 2, "the server it needs cannot start");
    assert!(!said.contains("has not moved"), "{said}");
}

/// The board a move reads is the project's own file in the common dir, else the one `TASKS_DB`
/// names; a project file naming an empty id is refused, naming that file.
#[test]
fn the_board_to_move_is_the_projects_own_or_the_one_tasks_db_names() {
    use symphony_board::cutover::board_path;
    let (dir, mut env) = unmoved("path");
    let board = board_path(&env).unwrap();
    assert_eq!(board.file_name().unwrap(), "lamp-works-tasks.db");
    let common = board.parent().unwrap();
    assert_eq!(
        common.canonicalize().unwrap(),
        dir.join(".git").canonicalize().unwrap()
    );
    fs::write(dir.join("symphony.toml"), "[project]\nid = \"\"\n").unwrap();
    assert_eq!(
        board_path(&env),
        Err(format!(
            "{} names no [project] id, so this repository has no SQLite board",
            common.parent().unwrap().join("symphony.toml").display()
        ))
    );
    env.vars
        .insert("TASKS_DB".into(), "/elsewhere/board.db".into());
    assert_eq!(board_path(&env), Ok(PathBuf::from("/elsewhere/board.db")));
}
