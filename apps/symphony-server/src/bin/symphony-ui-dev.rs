//! Explicit development launcher; symphony-server and the shared server never invoke it.
fn main() -> std::process::ExitCode {
    match symphony_server::development::main(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("symphony-ui-dev: {why}");
            std::process::ExitCode::FAILURE
        }
    }
}
