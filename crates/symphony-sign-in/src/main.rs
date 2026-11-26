//! `symphony-sign-in`: the Symphony's own sign-in service, in front of the server
//! (`docs/operations/SIGN_IN_SERVICE.md`).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let machine =
        symphony_notice::machine_folder(|name| std::env::var_os(name), cfg!(target_os = "macos"));
    symphony_sign_in::main(
        &args,
        machine,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}
