//! `tasks`: the task board's command line (`just tasks`), over the board's SurrealDB server.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = tasks::cli::main(
        &args,
        &tasks::cli::Env::of_process(),
        &mut std::io::stdin(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}
