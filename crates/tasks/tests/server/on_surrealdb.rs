//! A board still on SurrealDB, as an earlier release kept it (its store in `data/` in the board's
//! folder): every command that would reach, start, hold, snapshot or reclaim it is refused,
//! naming how it moves ([`symphony_board::server::GONE`]), and nothing is started or made; a
//! release of its hold, which starts nothing, still runs.

use crate::own_board::Repository;
use std::fs;
use symphony_board::cli::{self, connect};
use symphony_board::server::{FOLDER, GONE};

const HANDLE: &str = "russet-lynx";

/// `tasks ARGS` run in `repo`: its exit code, and what it said on each stream.
fn tasks(repo: &Repository, args: &[&str]) -> (i32, String, String) {
    let args: Vec<String> = args.iter().map(|word| (*word).to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cli::main(
        &args,
        &repo.env(),
        &mut std::io::empty(),
        &mut out,
        &mut err,
    );
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn every_command_on_a_board_still_on_surrealdb_is_refused_naming_how_it_moves_and_starts_nothing() {
    let repo = Repository::new("on-surrealdb");
    let board = repo.common().join(FOLDER);
    fs::create_dir_all(board.join("data")).unwrap();
    for (args, nothing) in [
        (
            &["database", "--as", HANDLE][..],
            "Nothing was held, stopped or read",
        ),
        (
            &["database", "hold", "a swap", "--as", HANDLE],
            "Nothing was held, stopped or read",
        ),
        (&["snapshot", "--as", HANDLE], "No snapshot was taken"),
        (
            &["snapshot", "--list", "--as", HANDLE],
            "No snapshot was listed",
        ),
        (&["list", "--as", HANDLE], "Nothing was started"),
    ] {
        let (code, out, err) = tasks(&repo, args);
        assert_eq!((code, out.as_str()), (2, ""), "{args:?}: {err}");
        assert!(err.contains(GONE), "{args:?}: {err}");
        assert!(err.contains(nothing), "{args:?}: {err}");
    }
    let refused = connect(&repo.env()).err().expect("no board is reached");
    assert!(refused.contains(GONE), "{refused}");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        symphony_board::reclaim_main(&[], &repo.env(), &mut out, &mut err),
        2
    );
    let err = String::from_utf8(err).unwrap();
    assert!(
        out.is_empty() && err.contains(GONE) && err.contains("Nothing was reclaimed"),
        "{err}"
    );

    // A release starts nothing, on whatever engine the board is.
    let hold = serde_json::json!({"by": HANDLE, "why": "a swap", "at": 1_800_000_000, "pid": 1});
    fs::write(board.join("hold"), hold.to_string()).unwrap();
    let (code, out, err) = tasks(&repo, &["database", "release", "--as", HANDLE]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with("✔ released the hold russet-lynx set at"),
        "{out}"
    );
    assert!(!board.join("hold").exists());

    // Nothing was made: no PostgreSQL data, no identity naming an engine.
    assert!(!board.join("postgresql").exists() && !board.join("backend.json").exists());
    assert!(board.join("data").is_dir(), "its store is left as it was");
}
