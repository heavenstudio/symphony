//! The board's dead space given back to the filesystem by PostgreSQL's VACUUM FULL and ANALYZE:
//!
//!     tasks-reclaim
//!     tasks-reclaim --dir FOLDER
//!
//! The board in the git common dir of the working directory, its server running meanwhile
//! (`symphony_board::server::Postgres::reclaim`): `tasks-reclaim`, the owner's to run. `--dir`
//! names a board folder elsewhere, a copy of the board's to rehearse on. It says what it did, or
//! why not (exit 2), in one line; `--help` alone says how it is run, and `--version` its version.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = symphony_board::reclaim_main(
        &args,
        &symphony_board::cli::Env::of_process(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}
