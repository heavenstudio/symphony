//! A whole board copied into its database, replacing what is there, and back out:
//!
//!     tasks-copy postgresql:/SOCKET [DATABASE] < dump.json
//!     tasks-copy --board < dump.json
//!     tasks-copy --board --out > dump.json
//!
//! Database `tasks` (or DATABASE) on the PostgreSQL server whose socket folder the address names,
//! or the board this run reaches, in the database it is in. It says what it copied, or why not
//! (exit 2), in one line; `--out` writes the board's dump instead, in the shape the copy reads.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = symphony_board::copy_main(
        &args,
        &symphony_board::cli::Env::of_process(),
        &mut std::io::stdin(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}
