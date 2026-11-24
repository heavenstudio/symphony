//! `tasks`: the task board's command line (`just tasks`), over the board's PostgreSQL server. Its
//! writes hand their callback dispatch to a detached `tasks dispatch` of this same executable
//! ([`symphony_board::cli::process_main`], every `tasks` build's whole `main`).

use std::process::ExitCode;

fn main() -> ExitCode {
    symphony_board::cli::process_main()
}
