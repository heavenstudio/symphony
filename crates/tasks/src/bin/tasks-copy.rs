//! A whole board copied into SurrealDB, replacing what is there, and back out:
//!
//!     tasks-copy 127.0.0.1:PORT [DATABASE] < dump.json
//!     tasks-copy --board < dump.json
//!     tasks-copy --board --out > dump.json
//!
//! Namespace `sf`, database `tasks` (or DATABASE) on the server at that address, or the board this run
//! reaches. It says what it copied, or why not (exit 2), in one line; `--out` writes the board's dump
//! instead, in the shape the copy reads.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = tasks::copy_main(
        &args,
        &tasks::cli::Env::of_process(),
        &mut std::io::stdin(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}
