//! A program Cargo runs from the repository is refused its live board before the board's server is
//! started, whichever program reaches it: `tasks-copy --board`, copying into the board or dumping
//! it, as `tasks` is.

use super::{COPY, Held, Live};
use std::path::Path;

#[test]
fn a_copy_cargo_runs_is_refused_the_live_board_before_its_server_is_started() {
    let live = Live::new("main-only-first", Held::Own);
    let cargo = live.wt.join("crates/tasks");
    std::fs::create_dir_all(&cargo).unwrap();
    let under = [("CARGO_MANIFEST_DIR", cargo.to_str().unwrap())];
    let why = live.under_cargo(&cargo);
    live.home().stop_board().unwrap();

    for args in [&["--board"][..], &["--board", "--out"]] {
        let said = format!("❌ tasks-copy: {why}\n");
        let ran = live.run(Path::new(COPY), &live.wt, &under, args, "{}");
        assert_eq!(
            (ran.code, ran.err.as_str()),
            (2, said.as_str()),
            "{COPY} {args:?}: {}",
            ran.out
        );
        assert!(
            !live.serving(),
            "{COPY} {args:?} started the live board's server"
        );
    }
}
