//! `just agent-send`: agent-to-agent messaging through the board's registered native channels.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Read only the board connection and sender settings. Messaging credentials are never needed.
    let env = symphony_board::cli::Env {
        vars: [
            "TASKS_SESSION",
            "TASKS_DATABASE",
            "TASKS_SERVER",
            "TASKS_NOW",
        ]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok().map(|value| (name.into(), value)))
        .collect(),
        cwd: std::env::current_dir().unwrap_or_default(),
    };
    let code = symphony_board::cli::agent_send::main(
        &args,
        &env,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}
