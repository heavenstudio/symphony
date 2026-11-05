//! The Symphony's server holding the shelf (src/shelf.rs), a fixture project's plugin: the server
//! the client's browser cases start.

#![forbid(unsafe_code)]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(symphony_server::binary::run(
        symphony_server::shelf::PLUGINS,
        &args,
    ));
}
