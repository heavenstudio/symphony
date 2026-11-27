//! The Symphony's own sign-in service (`docs/operations/SIGN_IN_SERVICE.md`): a program that stands
//! in front of the server, signs each visitor in with Google by OpenID Connect or with GitHub by
//! OAuth 2, keeps the client secrets and the session, and passes every request on signed with the
//! one header the server verifies (`docs/operations/SIGNED_IN.md`). Nothing links it and it links nothing of the core: the
//! server knows it only by the public key the machine folder's `sign-in.toml` names, and any other
//! service that signs the same header replaces it.
//!
//! - [`settings`]: its two files in the machine folder, the settings and the private key, each
//!   its account's alone, and the command that makes the key pair.
//! - `word`: the header it signs, afresh for each request it passes on.
//! - `google` and `github`: the providers: where a visitor is sent, the code exchanged (`curl`),
//!   and whom the provider's answer names, checked.
//! - `flow`: a sign-in under way, in its browser's flow cookie, signed; nothing is kept for it.
//! - `kept`: the sessions, in memory alone.
//! - `front`: the listener: its own paths under [`PREFIX`], which the server behind never sees,
//!   and every other request passed on signed, or not at all.
//!
//! Nothing it says names a secret, a code, a token, a cookie, a word or the key.

mod curl;
mod flow;
mod front;
mod github;
mod google;
mod kept;
mod random;
pub mod settings;
mod word;

#[cfg(test)]
mod stand_in;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

/// The paths the service answers itself and never passes on: each provider's way back, the
/// visitor's own page and sign-out.
pub const PREFIX: &str = "/.sign-in/";

/// What `--help` prints.
const USAGE: &str = "usage: symphony-sign-in key | serve

  key    make the service's key pair: write sign-in-service.key in the machine folder, mode 600,
         and print the key line sign-in.toml takes
  serve  sign visitors in and pass their requests on to the server, as
         sign-in-service.toml in the machine folder says
";

/// The program, run with `args` (its own name left out) in the machine folder `machine`
/// ([`symphony_notice::machine_folder`]), printing on `out` and `err`.
pub fn main(
    args: &[String],
    machine: Result<PathBuf, String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let done = match args {
        [help] if help == "--help" || help == "-h" => {
            return said(&out.write_all(USAGE.as_bytes()), ExitCode::SUCCESS);
        }
        [command] if command == "key" => machine
            .and_then(|machine| settings::make_key(&machine))
            .and_then(|line| {
                writeln!(out, "{line}").map_err(|error| format!("the key line: {error}"))
            }),
        [command] if command == "serve" => machine.and_then(|machine| front::serve(&machine)),
        _ => {
            return said(&err.write_all(USAGE.as_bytes()), ExitCode::from(2));
        }
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => said(&writeln!(err, "symphony-sign-in: {why}"), ExitCode::FAILURE),
    }
}

/// `code`, or a failure where what it prints could not be written.
fn said(written: &std::io::Result<()>, code: ExitCode) -> ExitCode {
    if written.is_ok() {
        code
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
