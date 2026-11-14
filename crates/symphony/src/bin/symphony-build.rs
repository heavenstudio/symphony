//! `symphony-build COMMAND …` (`tools/rs-bin.sh symphony-build`): the commands only this
//! repository's own build and release run use (`symphony::cli::build_main`): `publish-client`,
//! `release-inputs`, `size-budget`, `split-repair` and `release`. It is never shipped: no project is
//! given it (`init::BUILDS` does not name it), and the shipped `symphony` links none of them.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code =
        symphony::cli::build_main(&args, &mut std::io::stdout().lock(), &mut std::io::stderr());
    ExitCode::from(u8::try_from(code).unwrap_or(2))
}
