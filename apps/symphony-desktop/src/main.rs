//! `symphony-desktop`: the app, drawing what its library decides (`symphony_desktop`). No test opens
//! its windows; `just lint-rs` builds it.

#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::process::ExitCode;

use symphony_desktop::cli::{self, Asked};
use symphony_desktop::servers::Places;

#[cfg(target_os = "macos")]
mod app;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut out, mut err) = (io::stdout(), io::stderr());
    match cli::main(&args, |name| std::env::var(name).ok(), &mut out, &mut err) {
        Asked::Exit(code) => ExitCode::from(u8::try_from(code).unwrap_or(2)),
        Asked::Open(places) => open(&places, &mut err),
    }
}

#[cfg(target_os = "macos")]
fn open(places: &Places, _: &mut dyn Write) -> ExitCode {
    app::run(places)
}

#[cfg(not(target_os = "macos"))]
fn open(_: &Places, err: &mut dyn Write) -> ExitCode {
    let _ = writeln!(
        err,
        "✖ symphony-desktop: the app is for macOS alone for now"
    );
    ExitCode::from(2)
}
