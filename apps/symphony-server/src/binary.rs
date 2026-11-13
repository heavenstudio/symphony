//! What a `symphony-server` binary runs: the plugins it holds installed, then the command line
//! ([`crate::cli::main`]), with the server log running while it serves a checkout.

/// A `symphony-server` binary holding `plugins`: they are installed, the server log runs while it
/// serves a checkout, and [`crate::cli::main`] does the rest. The exit code.
pub fn run(plugins: &'static [&'static dyn symphony_plugin::Plugin], args: &[String]) -> i32 {
    if let Err(problem) = symphony::plugins::install(plugins) {
        eprintln!("{problem}");
        return 2;
    }
    run_holding_none(args)
}

/// A `symphony-server` binary holding no plugin, the Symphony's own: [`run`] with none to install.
/// It installs nothing, so it links none of what measures a call into a plugin
/// ([`symphony::plugins::install`]). The exit code.
pub fn run_holding_none(args: &[String]) -> i32 {
    let logging = if args.first().is_some_and(|arg| arg == "serve") {
        match args
            .windows(2)
            .find(|pair| pair[0] == "--home")
            .map(|_| crate::log::Log::start())
            .transpose()
        {
            Ok(logging) => logging,
            Err(e) => {
                eprintln!("cannot start server log: {e}");
                return 1;
            }
        }
    } else {
        None
    };
    let code = crate::cli::main(args);
    if let Some(log) = logging
        && let Err(e) = log.finish()
    {
        eprintln!("cannot finish server log: {e}");
        return 1;
    }
    code
}
