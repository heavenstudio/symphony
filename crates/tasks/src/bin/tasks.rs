//! `tasks`: the task board's command line (`just tasks`), over the board's SurrealDB server. Its
//! writes hand their callback dispatch to a detached `tasks dispatch` of this same executable.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env = symphony_board::cli::Env::of_process();
    let (mut input, mut out, mut err) = (std::io::stdin(), std::io::stdout(), std::io::stderr());
    let code = match std::env::current_exe() {
        Ok(program) => symphony_board::cli::main_detaching(
            &args, &env, &mut input, &mut out, &mut err, &program,
        ),
        Err(_) => symphony_board::cli::main(&args, &env, &mut input, &mut out, &mut err),
    };
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}
