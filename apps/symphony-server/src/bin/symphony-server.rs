//! `just symphony`: the Symphony's one server (src/lib.rs).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let logging = if args.first().is_some_and(|arg| arg == "serve") {
        args.windows(2)
            .find(|pair| pair[0] == "--home")
            .map(|_| symphony_server::log::Log::start())
            .transpose()
            .unwrap_or_else(|e| {
                eprintln!("cannot start server log: {e}");
                std::process::exit(1)
            })
    } else {
        None
    };
    let code = symphony_server::cli::main(&args);
    if let Some(log) = logging
        && let Err(e) = log.finish()
    {
        eprintln!("cannot finish server log: {e}");
        std::process::exit(1);
    }
    std::process::exit(code);
}
