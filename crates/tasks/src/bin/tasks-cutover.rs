//! The task board's move from SQLite to SurrealDB, run once (`tasks-cutover`, [`symphony_board::cutover`]):
//!
//!     tasks-cutover
//!
//! It says what it moved and where the frozen SQLite board is kept, or why not (exit 2), in one line.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = symphony_board::cutover::main(
        &args,
        &symphony_board::cli::Env::of_process(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}
