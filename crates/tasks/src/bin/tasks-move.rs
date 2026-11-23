//! A PostgreSQL snapshot put back over the board, or a move an earlier release left standing
//! settled:
//!
//!     tasks-move --restore SNAPSHOT --as HANDLE
//!     tasks-move postgresql --as HANDLE
//!
//! The board in the git common dir of the working directory, held meanwhile
//! (`symphony_board::server::Postgres::restore_over`, `Home::move_to`): the owner's to run. It
//! says what it did, or why not (exit 2); `--help` alone says how it is run, and `--version` its
//! version.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = symphony_board::move_main(
        &args,
        &symphony_board::cli::Env::of_process(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}
