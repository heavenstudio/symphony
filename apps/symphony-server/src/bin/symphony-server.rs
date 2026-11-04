//! `just symphony`: the Symphony's one server (src/lib.rs), holding no plugin.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(symphony_server::cli::run(&[], &args));
}
