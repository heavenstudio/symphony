//! Every refusal of the live-board rule names only what its own program takes: each flag and
//! variable a refusal of `tasks`, `agent-send`, `tasks-copy` or `tasks-cutover` names in backticks
//! is in that program's `--help`.

use super::{COPY, Held, Live, SEND, TASKS};
use crate::Ran;
use std::path::Path;

const CUTOVER: &str = env!("CARGO_BIN_EXE_tasks-cutover");

/// The flags (`--database`) and variables (`TASKS_DATABASE`) `said` names in backticks.
fn named(said: &str) -> Vec<String> {
    said.split('`')
        .skip(1)
        .step_by(2)
        .flat_map(|span| span.split(|c: char| c.is_whitespace() || c == '=' || c == ','))
        .filter(|word| {
            word.starts_with("--")
                || (word.contains('_')
                    && word
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
        })
        .map(str::to_string)
        .collect()
}

/// Whether `help` names `word` whole: no letter, digit, `_` or `-` either side of it.
fn names(help: &str, word: &str) -> bool {
    let part =
        |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    help.match_indices(word).any(|(at, _)| {
        !part(help[..at].chars().next_back()) && !part(help[at + word.len()..].chars().next())
    })
}

#[test]
fn the_check_reads_flags_and_variables_out_of_backticks_alone() {
    assert_eq!(
        named(
            "a board (`TASKS_DATABASE=NAME` with `--board`, or `tasks-copy ADDR NAME`); \
             `just tasks migrate` in MAIN; give it (`TASKS_SERVER` and `TASKS_DATABASE`), --out"
        ),
        [
            "TASKS_DATABASE",
            "--board",
            "TASKS_SERVER",
            "TASKS_DATABASE"
        ]
    );
    assert!(names("  --database NAME  the board", "--database"));
    assert!(names("default: $TASKS_SESSION)", "TASKS_SESSION"));
    assert!(!names("  --databases", "--database"));
    assert!(!names("TASKS_DATABASE_X", "TASKS_DATABASE"));
}

/// What `program --help` prints, which must succeed.
fn help_of(live: &Live, program: &str) -> String {
    let ran = live.run(Path::new(program), &live.outside, &[], &["--help"], "");
    assert_eq!(ran.code, 0, "{program} --help: {}", ran.err);
    ran.out
}

/// A refusal, which `ran` must be: its standard error.
fn refusal(program: &str, ran: Ran) -> (String, String) {
    assert_ne!(ran.code, 0, "{program}: {}", ran.out);
    assert!(!ran.err.trim().is_empty(), "{program} said no refusal");
    (program.to_string(), ran.err)
}

/// The refusal of each program to the board `live`, reached from the worktree with `vars`: `tasks`,
/// `agent-send`, `tasks-copy` by its board and by its server's address from outside the
/// repository, and `tasks-cutover`, given an SQLite board to move for the run.
fn refusals(live: &Live, vars: &[(&str, &str)]) -> Vec<(String, String)> {
    let addr = live.addr();
    let mut said = vec![
        refusal(
            TASKS,
            live.run(Path::new(TASKS), &live.wt, vars, &["list"], ""),
        ),
        refusal(
            SEND,
            live.run(
                Path::new(SEND),
                &live.wt,
                vars,
                &["someone", "hello", "--as", "me"],
                "",
            ),
        ),
        refusal(
            COPY,
            live.run(Path::new(COPY), &live.wt, vars, &["--board"], "{}"),
        ),
        refusal(
            COPY,
            live.run(Path::new(COPY), &live.outside, vars, &[&addr], "{}"),
        ),
    ];
    let board = live.main.join(".git/live-tasks.db");
    crate::sqlite::make(&board, &crate::sqlite::sample());
    said.push(refusal(
        CUTOVER,
        live.run(Path::new(CUTOVER), &live.wt, vars, &[], ""),
    ));
    for end in ["", "-wal", "-shm", ".cutover"] {
        let _ = std::fs::remove_file(live.main.join(format!(".git/live-tasks.db{end}")));
    }
    said
}

#[test]
fn every_refusal_names_only_the_flags_and_variables_its_own_program_takes() {
    let live = Live::older("main-only-flags", Held::Other);
    // A branch's build, then a main checkout that holds no source, then a process Cargo runs.
    let mut said = refusals(&live, &[]);
    live.hold(Held::Nothing);
    said.extend(refusals(&live, &[]));
    let cargo = live.wt.join("crates/tasks");
    std::fs::create_dir_all(&cargo).unwrap();
    said.extend(refusals(
        &live,
        &[("CARGO_MANIFEST_DIR", cargo.to_str().unwrap())],
    ));
    assert_eq!(live.version("tasks"), Some(super::before()));

    for (program, refusal) in said {
        let words = named(&refusal);
        assert!(!words.is_empty(), "{program} names no way out: {refusal}");
        let help = help_of(&live, &program);
        for word in words {
            assert!(
                names(&help, &word),
                "{program}'s refusal names {word}, which its --help does not:\n{refusal}\n{help}"
            );
        }
    }
}
