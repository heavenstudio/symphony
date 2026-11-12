//! A program Cargo runs from the repository is refused its live board before the board's server is
//! started, whichever program reaches it: `tasks-copy --board`, copying into the board or dumping
//! it, and `tasks-cutover`, moving an SQLite board into it, as `tasks` is.

use super::{COPY, Held, Live};
use std::path::Path;

const CUTOVER: &str = env!("CARGO_BIN_EXE_tasks-cutover");

#[test]
fn a_copy_or_a_move_cargo_runs_is_refused_the_live_board_before_its_server_is_started() {
    let live = Live::new("main-only-first", Held::Own);
    let cargo = live.wt.join("crates/tasks");
    std::fs::create_dir_all(&cargo).unwrap();
    let under = [("CARGO_MANIFEST_DIR", cargo.to_str().unwrap())];
    let why = live.under_cargo(&cargo);
    live.home().stop().unwrap();
    // The SQLite board `tasks-cutover` would move.
    let board = live.main.join(".git/live-tasks.db");
    crate::sqlite::make(&board, &crate::sqlite::sample());

    for (program, args, said) in [
        (COPY, &["--board"][..], format!("❌ tasks-copy: {why}\n")),
        (
            COPY,
            &["--board", "--out"],
            format!("❌ tasks-copy: {why}\n"),
        ),
        (
            CUTOVER,
            &[],
            format!("❌ tasks-cutover: {why}; the board stays in SQLite, never frozen\n"),
        ),
    ] {
        let ran = live.run(Path::new(program), &live.wt, &under, args, "{}");
        assert_eq!(
            (ran.code, ran.err.as_str()),
            (2, said.as_str()),
            "{program} {args:?}: {}",
            ran.out
        );
        assert!(
            live.home().recorded().is_none(),
            "{program} {args:?} started the live board's server"
        );
    }
    assert!(!live.main.join(".git/live-tasks.db.moved").exists());
}
